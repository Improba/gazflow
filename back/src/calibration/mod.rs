use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::graph::GasNetwork;

pub mod import;
pub mod lm;
pub mod objective;
pub mod optimizer;
pub mod report;

pub use self::import::parse_scada_csv;
pub use self::objective::compute_residuals;
pub use self::report::CalibrationReport;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementType {
    Pressure,
    Flow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScadaMeasurement {
    pub id: String,
    pub measurement_type: MeasurementType,
    pub value: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uncertainty: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CalibrationParameter {
    GlobalRoughnessFactor { factor: f64 },
    PerPipeRoughnessMultiplier { multipliers: HashMap<String, f64> },
    DemandScale { node_id: String, factor: f64 },
}

impl CalibrationParameter {
    fn identity_for_network(strategy: CalibrationStrategy, network: &GasNetwork) -> Self {
        match strategy {
            CalibrationStrategy::Global => Self::GlobalRoughnessFactor { factor: 1.0 },
            CalibrationStrategy::PerPipe => Self::PerPipeRoughnessMultiplier {
                multipliers: network.pipes().map(|p| (p.id.clone(), 1.0)).collect(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationStrategy {
    #[default]
    Global,
    PerPipe,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CalibrationRequest {
    pub measurements_csv: String,
    #[serde(default)]
    pub strategy: CalibrationStrategy,
    #[serde(default)]
    pub demands: Option<HashMap<String, f64>>,
}

pub fn calibrate_from_csv(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements_csv: &str,
    strategy: CalibrationStrategy,
) -> Result<CalibrationReport, String> {
    let measurements = parse_scada_csv(measurements_csv);
    calibrate(network, demands, &measurements, strategy)
}

pub fn calibrate(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    strategy: CalibrationStrategy,
) -> Result<CalibrationReport, String> {
    if measurements.is_empty() {
        return Err("no valid SCADA measurements provided".to_string());
    }

    let params_before = CalibrationParameter::identity_for_network(strategy, network);
    let residuals_before = compute_residuals(network, demands, measurements, &params_before);
    if residuals_before.is_empty() {
        return Err("unable to evaluate baseline residuals".to_string());
    }

    let params_after = match strategy {
        CalibrationStrategy::Global => {
            let initial_factor = match &params_before {
                CalibrationParameter::GlobalRoughnessFactor { factor } => *factor,
                CalibrationParameter::PerPipeRoughnessMultiplier { .. } => 1.0,
                CalibrationParameter::DemandScale { factor, .. } => *factor,
            };
            let factor =
                lm::optimize_global_roughness_lm(network, demands, measurements, initial_factor);
            CalibrationParameter::GlobalRoughnessFactor { factor }
        }
        CalibrationStrategy::PerPipe => optimizer::optimize_parameters(
            network,
            demands,
            measurements,
            &params_before,
            optimizer::CalibrationOptimizationConfig::for_strategy(strategy),
        ),
    };
    let residuals = compute_residuals(network, demands, measurements, &params_after);
    if residuals.is_empty() {
        return Err("unable to evaluate calibrated residuals".to_string());
    }

    let predictions =
        objective::compute_measurement_predictions(network, demands, measurements, &params_after);
    let observed: Vec<f64> = predictions.iter().map(|(obs, _)| *obs).collect();
    let predicted: Vec<f64> = predictions.iter().map(|(_, pred)| *pred).collect();
    let uncertainties =
        objective::aligned_uncertainties(network, demands, measurements, &params_after);

    Ok(CalibrationReport {
        params_before,
        params_after,
        rmse: report::rmse(&residuals, Some(&uncertainties)),
        r_squared: report::r_squared(&observed, &predicted),
        residuals,
    })
}

pub(crate) fn apply_calibration_parameter_set(
    network: &GasNetwork,
    base_demands: &HashMap<String, f64>,
    params: &[CalibrationParameter],
) -> (GasNetwork, HashMap<String, f64>) {
    let mut calibrated = network.clone();
    let mut scaled_demands = base_demands.clone();
    for param in params {
        match param {
            CalibrationParameter::GlobalRoughnessFactor { factor } => {
                let factor = factor.max(1e-6);
                for pipe in calibrated.graph.edge_weights_mut() {
                    pipe.roughness_mm = (pipe.roughness_mm * factor).max(1e-6);
                }
            }
            CalibrationParameter::PerPipeRoughnessMultiplier { multipliers } => {
                for pipe in calibrated.graph.edge_weights_mut() {
                    let factor = multipliers.get(&pipe.id).copied().unwrap_or(1.0).max(1e-6);
                    pipe.roughness_mm = (pipe.roughness_mm * factor).max(1e-6);
                }
            }
            CalibrationParameter::DemandScale { node_id, factor } => {
                let factor = factor.max(1e-6);
                if let Some(demand) = scaled_demands.get_mut(node_id) {
                    *demand *= factor;
                }
            }
        }
    }
    (calibrated, scaled_demands)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{
        graph::{ConnectionKind, EquipmentSpec, GasNetwork, Node, Pipe},
        solver,
    };

    use super::{CalibrationParameter, CalibrationStrategy, MeasurementType, ScadaMeasurement};

    fn two_node_network(roughness_mm: f64) -> GasNetwork {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "source".into(),
            x: 0.0,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net.add_node(Node {
            id: "sink".into(),
            x: 1.0,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net.add_pipe(Pipe {
            id: "pipe_1".into(),
            from: "source".into(),
            to: "sink".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 100.0,
            diameter_mm: 500.0,
            roughness_mm,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        net
    }

    #[test]
    fn calibration_improves_fit_on_synthetic_case() {
        let network = two_node_network(0.012);
        let mut true_network = network.clone();
        for pipe in true_network.graph.edge_weights_mut() {
            pipe.roughness_mm *= 1.8;
        }

        let demands = HashMap::from([(String::from("sink"), -12.0)]);
        let synthetic = solver::solve_steady_state_with_composition(
            &true_network,
            &demands,
            solver::GasComposition::default(),
            1000,
            5e-4,
        )
        .expect("synthetic solve");
        let target_sink_pressure = synthetic.pressures["sink"];

        let measurements = vec![ScadaMeasurement {
            id: "sink".to_string(),
            measurement_type: MeasurementType::Pressure,
            value: target_sink_pressure,
            timestamp: None,
            uncertainty: None,
        }];

        let baseline = super::compute_residuals(
            &network,
            &demands,
            &measurements,
            &CalibrationParameter::GlobalRoughnessFactor { factor: 1.0 },
        );
        let baseline_rmse = super::report::rmse(&baseline, None);

        let report = super::calibrate(
            &network,
            &demands,
            &measurements,
            CalibrationStrategy::Global,
        )
        .expect("calibration should succeed");

        assert!(
            report.rmse < baseline_rmse,
            "calibration should improve fit: before={baseline_rmse}, after={}",
            report.rmse
        );
        assert!(
            report.residuals[0].abs() < baseline[0].abs(),
            "final residual should be lower"
        );
        let preds = super::objective::compute_measurement_predictions(
            &network,
            &demands,
            &measurements,
            &report.params_after,
        );
        assert!(
            (report.residuals[0] - (preds[0].0 - preds[0].1)).abs() < 1e-12,
            "residual convention must be observed - predicted"
        );
    }
}
