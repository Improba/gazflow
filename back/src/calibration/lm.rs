use std::collections::HashMap;

use crate::graph::GasNetwork;

use super::{
    CalibrationParameter, MeasurementType, ScadaMeasurement,
    objective::{
        aligned_uncertainties_with_params, compute_residuals_with_params,
    },
};

const MIN_FACTOR: f64 = 1e-6;
const LM_MAX_ITERATIONS: usize = 20;
const INITIAL_LAMBDA: f64 = 1e-4;
const LAMBDA_INCREASE: f64 = 10.0;
const LAMBDA_DECREASE: f64 = 0.3;
const FD_RELATIVE_STEP: f64 = 2e-2;
const FD_MIN_STEP: f64 = 1e-3;
pub const MAX_LM_PARAMS: usize = 5;

/// Minimise $\Phi(\theta)=\frac12\sum_i \bigl(r_i(\theta)/\sigma_i\bigr)^2$ avec LM multi-paramètres
/// ($|\theta| \le$ [`MAX_LM_PARAMS`]) et Jacobien FD $m \times n$.
pub fn optimize_lm(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    mut params: Vec<CalibrationParameter>,
) -> Vec<CalibrationParameter> {
    if params.is_empty() || params.len() > MAX_LM_PARAMS {
        return params;
    }
    if !params.iter().all(is_scalar_param) {
        return params;
    }

    params = params
        .into_iter()
        .map(|p| param_with_value(&p, param_value(&p).unwrap_or(1.0)))
        .collect();

    let mut lambda = INITIAL_LAMBDA;
    let mut residuals = evaluate_residuals_with_params(network, demands, measurements, &params);
    let mut inv_sigmas = inverse_sigmas(network, demands, measurements, &params);
    if residuals.is_empty() || residuals.len() != inv_sigmas.len() {
        return params;
    }
    let mut weighted = apply_weights(&residuals, &inv_sigmas);
    let mut cost = squared_error_cost(&weighted);
    let n = params.len();

    for _ in 0..LM_MAX_ITERATIONS {
        let jacobian = finite_difference_jacobian_matrix(
            network,
            demands,
            measurements,
            &params,
            &residuals,
            &inv_sigmas,
        );
        if jacobian.len() != residuals.len() * n || jacobian.is_empty() {
            break;
        }

        let mut jt_j = vec![0.0; n * n];
        let mut jt_r = vec![0.0; n];
        for i in 0..residuals.len() {
            for col in 0..n {
                let j = jacobian[i * n + col];
                jt_r[col] += j * weighted[i];
                for row in 0..n {
                    jt_j[row * n + col] += j * jacobian[i * n + row];
                }
            }
        }

        let mut delta = vec![0.0; n];
        let mut solved = false;
        for _attempt in 0..8 {
            let mut system = jt_j.clone();
            for i in 0..n {
                let diag = system[i * n + i].max(1e-12);
                system[i * n + i] += lambda * diag;
            }
            if let Some(step) = solve_symmetric_positive(&system, &jt_r, n) {
                delta = step.iter().map(|v| -v).collect();
                if delta.iter().all(|d| d.is_finite()) {
                    solved = true;
                    break;
                }
            }
            lambda = (lambda * LAMBDA_INCREASE).min(1e9);
        }
        if !solved {
            break;
        }

        if delta.iter().all(|d| d.abs() <= 1e-12) {
            break;
        }

        let trial_params = apply_delta(&params, &delta);
        let trial_residuals =
            evaluate_residuals_with_params(network, demands, measurements, &trial_params);
        let trial_inv = inverse_sigmas(network, demands, measurements, &trial_params);
        if trial_residuals.len() != residuals.len() || trial_residuals.is_empty() {
            lambda = (lambda * LAMBDA_INCREASE).min(1e9);
            continue;
        }

        let trial_weighted = apply_weights(&trial_residuals, &trial_inv);
        let trial_cost = squared_error_cost(&trial_weighted);
        if trial_cost < cost {
            params = trial_params;
            residuals = trial_residuals;
            inv_sigmas = trial_inv;
            weighted = trial_weighted;
            cost = trial_cost;
            lambda = (lambda * LAMBDA_DECREASE).max(1e-9);
            if cost <= 1e-16 {
                break;
            }
        } else {
            lambda = (lambda * LAMBDA_INCREASE).min(1e9);
        }
    }

    params
}

/// Minimise $\Phi(\theta)=\frac12\sum_i \bigl(r_i(\theta)/\sigma_i\bigr)^2$ avec $r_i = y_i - \hat y_i(\theta)$.
/// Pas LM de Marquardt avec amortissement $\lambda$ sur $J^{\mathsf T}J$ (1 paramètre).
pub fn optimize_global_roughness_lm(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    initial_factor: f64,
) -> f64 {
    let mut factor = initial_factor.max(MIN_FACTOR);
    let mut lambda = INITIAL_LAMBDA;
    let params = CalibrationParameter::GlobalRoughnessFactor { factor };
    let mut residuals = evaluate_residuals(network, demands, measurements, factor);
    let mut inv_sigmas =
        inverse_sigmas(network, demands, measurements, std::slice::from_ref(&params));
    if residuals.is_empty() || residuals.len() != inv_sigmas.len() {
        return factor;
    }
    let mut weighted = apply_weights(&residuals, &inv_sigmas);
    let mut cost = squared_error_cost(&weighted);

    for _ in 0..LM_MAX_ITERATIONS {
        let jacobian = finite_difference_jacobian(
            network,
            demands,
            measurements,
            factor,
            &residuals,
            &inv_sigmas,
        );
        if jacobian.len() != residuals.len() || jacobian.is_empty() {
            break;
        }

        let mut jt_j = 0.0;
        let mut jt_r = 0.0;
        for (&j, &r) in jacobian.iter().zip(weighted.iter()) {
            jt_j += j * j;
            jt_r += j * r;
        }

        let diagonal = jt_j.max(1e-12);
        let denominator = jt_j + lambda * diagonal;
        if !denominator.is_finite() || denominator <= f64::EPSILON {
            lambda = (lambda * LAMBDA_INCREASE).min(1e9);
            continue;
        }

        let delta = -jt_r / denominator;
        if !delta.is_finite() {
            break;
        }
        if delta.abs() <= 1e-9 * factor.max(1.0) {
            break;
        }

        let trial_factor = (factor + delta).max(MIN_FACTOR);
        let trial_residuals = evaluate_residuals(network, demands, measurements, trial_factor);
        let trial_params = CalibrationParameter::GlobalRoughnessFactor {
            factor: trial_factor,
        };
        let trial_inv = inverse_sigmas(
            network,
            demands,
            measurements,
            std::slice::from_ref(&trial_params),
        );
        if trial_residuals.len() != residuals.len() || trial_residuals.is_empty() {
            lambda = (lambda * LAMBDA_INCREASE).min(1e9);
            continue;
        }

        let trial_weighted = apply_weights(&trial_residuals, &trial_inv);
        let trial_cost = squared_error_cost(&trial_weighted);
        if trial_cost < cost {
            factor = trial_factor;
            residuals = trial_residuals;
            inv_sigmas = trial_inv;
            weighted = trial_weighted;
            cost = trial_cost;
            lambda = (lambda * LAMBDA_DECREASE).max(1e-9);
            if cost <= 1e-16 {
                break;
            }
        } else {
            lambda = (lambda * LAMBDA_INCREASE).min(1e9);
        }
    }

    factor.max(MIN_FACTOR)
}

fn inverse_sigmas(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    params: &[CalibrationParameter],
) -> Vec<f64> {
    aligned_uncertainties_with_params(network, demands, measurements, params)
        .into_iter()
        .map(|sigma| {
            if sigma.is_finite() && sigma > 0.0 {
                1.0 / sigma
            } else {
                1.0
            }
        })
        .collect()
}

fn apply_weights(residuals: &[f64], inv_sigmas: &[f64]) -> Vec<f64> {
    residuals
        .iter()
        .zip(inv_sigmas.iter())
        .map(|(r, w)| r * w)
        .collect()
}

fn evaluate_residuals(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    factor: f64,
) -> Vec<f64> {
    evaluate_residuals_with_params(
        network,
        demands,
        measurements,
        &[CalibrationParameter::GlobalRoughnessFactor {
            factor: factor.max(MIN_FACTOR),
        }],
    )
}

fn evaluate_residuals_with_params(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    params: &[CalibrationParameter],
) -> Vec<f64> {
    compute_residuals_with_params(network, demands, measurements, params)
}

fn is_scalar_param(param: &CalibrationParameter) -> bool {
    matches!(
        param,
        CalibrationParameter::GlobalRoughnessFactor { .. }
            | CalibrationParameter::DemandScale { .. }
    )
}

fn param_value(param: &CalibrationParameter) -> Option<f64> {
    match param {
        CalibrationParameter::GlobalRoughnessFactor { factor } => Some(*factor),
        CalibrationParameter::DemandScale { factor, .. } => Some(*factor),
        CalibrationParameter::PerPipeRoughnessMultiplier { .. } => None,
    }
}

fn param_with_value(param: &CalibrationParameter, value: f64) -> CalibrationParameter {
    let value = value.max(MIN_FACTOR);
    match param {
        CalibrationParameter::GlobalRoughnessFactor { .. } => {
            CalibrationParameter::GlobalRoughnessFactor { factor: value }
        }
        CalibrationParameter::DemandScale { node_id, .. } => CalibrationParameter::DemandScale {
            node_id: node_id.clone(),
            factor: value,
        },
        CalibrationParameter::PerPipeRoughnessMultiplier { multipliers } => {
            CalibrationParameter::PerPipeRoughnessMultiplier {
                multipliers: multipliers.clone(),
            }
        }
    }
}

fn apply_delta(params: &[CalibrationParameter], delta: &[f64]) -> Vec<CalibrationParameter> {
    params
        .iter()
        .zip(delta.iter())
        .map(|(param, d)| {
            let current = param_value(param).unwrap_or(1.0);
            param_with_value(param, current + d)
        })
        .collect()
}

fn finite_difference_jacobian_matrix(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    params: &[CalibrationParameter],
    base_residuals: &[f64],
    inv_sigmas: &[f64],
) -> Vec<f64> {
    let m = base_residuals.len();
    let n = params.len();
    if m == 0 || n == 0 {
        return Vec::new();
    }

    let mut jacobian = vec![0.0; m * n];
    for (col, param) in params.iter().enumerate() {
        let value = param_value(param).unwrap_or(1.0).max(MIN_FACTOR);
        let step = (value.abs() * FD_RELATIVE_STEP).max(FD_MIN_STEP);
        let plus_params = apply_delta(params, &{
            let mut d = vec![0.0; n];
            d[col] = step;
            d
        });
        let plus_residuals =
            evaluate_residuals_with_params(network, demands, measurements, &plus_params);
        if plus_residuals.len() != m {
            return Vec::new();
        }

        let minus_value = (value - step).max(MIN_FACTOR);
        let can_use_central = minus_value < value - 0.5 * step;
        let column = if can_use_central {
            let minus_params = {
                let mut trial = params.to_vec();
                trial[col] = param_with_value(param, minus_value);
                trial
            };
            let minus_residuals =
                evaluate_residuals_with_params(network, demands, measurements, &minus_params);
            if minus_residuals.len() != m {
                return Vec::new();
            }
            let denom = value + step - minus_value;
            if denom.abs() <= f64::EPSILON {
                return Vec::new();
            }
            plus_residuals
                .iter()
                .zip(minus_residuals.iter())
                .map(|(&rp, &rm)| (rp - rm) / denom)
                .collect::<Vec<_>>()
        } else {
            let denom = step;
            plus_residuals
                .iter()
                .zip(base_residuals.iter())
                .map(|(&rp, &r)| (rp - r) / denom)
                .collect::<Vec<_>>()
        };

        for (row, (&j, &w)) in column.iter().zip(inv_sigmas.iter()).enumerate() {
            jacobian[row * n + col] = j * w;
        }
    }
    jacobian
}

fn solve_symmetric_positive(matrix: &[f64], rhs: &[f64], n: usize) -> Option<Vec<f64>> {
    let mut a = matrix.to_vec();
    let mut b = rhs.to_vec();
    for col in 0..n {
        let pivot = a[col * n + col];
        if !pivot.is_finite() || pivot.abs() <= f64::EPSILON {
            return None;
        }
        for row in (col + 1)..n {
            let factor = a[row * n + col] / pivot;
            if !factor.is_finite() {
                return None;
            }
            for k in col..n {
                a[row * n + k] -= factor * a[col * n + k];
            }
            b[row] -= factor * b[col];
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let mut sum = b[row];
        for col in (row + 1)..n {
            sum -= a[row * n + col] * x[col];
        }
        let diag = a[row * n + row];
        if !diag.is_finite() || diag.abs() <= f64::EPSILON {
            return None;
        }
        x[row] = sum / diag;
        if !x[row].is_finite() {
            return None;
        }
    }
    Some(x)
}

fn finite_difference_jacobian(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    factor: f64,
    base_residuals: &[f64],
    inv_sigmas: &[f64],
) -> Vec<f64> {
    let step = (factor.abs() * FD_RELATIVE_STEP).max(FD_MIN_STEP);
    let plus_factor = factor + step;
    let plus_residuals = evaluate_residuals(network, demands, measurements, plus_factor);
    if plus_residuals.len() != base_residuals.len() || plus_residuals.is_empty() {
        return Vec::new();
    }

    let minus_factor = (factor - step).max(MIN_FACTOR);
    let can_use_central = (factor - step) > MIN_FACTOR;
    let raw_jacobian = if can_use_central {
        let minus_residuals = evaluate_residuals(network, demands, measurements, minus_factor);
        if minus_residuals.len() == base_residuals.len() {
            let denom = plus_factor - minus_factor;
            if denom.abs() > f64::EPSILON {
                plus_residuals
                    .iter()
                    .zip(minus_residuals.iter())
                    .map(|(&rp, &rm)| (rp - rm) / denom)
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    } else {
        let denom = plus_factor - factor;
        if denom.abs() <= f64::EPSILON {
            return Vec::new();
        }
        plus_residuals
            .iter()
            .zip(base_residuals.iter())
            .map(|(&rp, &r)| (rp - r) / denom)
            .collect()
    };

    if raw_jacobian.is_empty() {
        return raw_jacobian;
    }
    raw_jacobian
        .iter()
        .zip(inv_sigmas.iter())
        .map(|(j, w)| j * w)
        .collect()
}

fn squared_error_cost(residuals: &[f64]) -> f64 {
    residuals.iter().map(|r| 0.5 * r * r).sum()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{
        graph::{ConnectionKind, EquipmentSpec, GasNetwork, Node, Pipe},
        solver,
    };

    use crate::calibration::objective::compute_residuals;
    use super::*;

    fn three_node_network(roughness_mm: f64) -> GasNetwork {
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
            id: "mid".into(),
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
        net.add_node(Node {
            id: "sink".into(),
            x: 2.0,
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
            to: "mid".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 180.0,
            diameter_mm: 500.0,
            roughness_mm,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        net.add_pipe(Pipe {
            id: "pipe_2".into(),
            from: "mid".into(),
            to: "sink".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 210.0,
            diameter_mm: 500.0,
            roughness_mm,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        net
    }

    fn rmse_for_factor(
        network: &GasNetwork,
        demands: &HashMap<String, f64>,
        measurements: &[ScadaMeasurement],
        factor: f64,
    ) -> f64 {
        let residuals = compute_residuals(
            network,
            demands,
            measurements,
            &CalibrationParameter::GlobalRoughnessFactor { factor },
        );
        if residuals.is_empty() {
            return f64::INFINITY;
        }
        (residuals.iter().map(|r| r * r).sum::<f64>() / residuals.len() as f64).sqrt()
    }

    #[test]
    fn lm_beats_coarse_grid_for_factor_1_2() {
        let network = three_node_network(0.012);
        let mut true_network = network.clone();
        for pipe in true_network.graph.edge_weights_mut() {
            pipe.roughness_mm *= 1.2;
        }

        let demands = HashMap::from([(String::from("mid"), -8.0), (String::from("sink"), -16.0)]);
        let synthetic = solver::solve_steady_state_with_composition(
            &true_network,
            &demands,
            solver::GasComposition::default(),
            1000,
            5e-4,
        )
        .expect("synthetic solve");
        let measurements = vec![
            ScadaMeasurement {
                id: "mid".to_string(),
                measurement_type: MeasurementType::Pressure,
                value: synthetic.pressures["mid"],
                timestamp: None,
                uncertainty: None,
            },
            ScadaMeasurement {
                id: "sink".to_string(),
                measurement_type: MeasurementType::Pressure,
                value: synthetic.pressures["sink"],
                timestamp: None,
                uncertainty: None,
            },
        ];

        let lm_factor = optimize_global_roughness_lm(&network, &demands, &measurements, 1.0);
        let lm_rmse = rmse_for_factor(&network, &demands, &measurements, lm_factor);

        let mut grid_factor = 0.5;
        let mut grid_best_rmse = f64::INFINITY;
        while grid_factor <= 2.0 + 1e-12 {
            grid_best_rmse = grid_best_rmse.min(rmse_for_factor(
                &network,
                &demands,
                &measurements,
                grid_factor,
            ));
            grid_factor += 0.25;
        }

        let true_rmse = rmse_for_factor(&network, &demands, &measurements, 1.2);
        assert!(
            lm_rmse + 1e-8 < grid_best_rmse,
            "LM should beat coarse grid: lm={lm_rmse}, grid={grid_best_rmse}"
        );
        assert!(
            lm_rmse <= true_rmse + 1e-7,
            "LM should recover the synthetic optimum around factor 1.2: lm={lm_rmse}, true={true_rmse}, factor={lm_factor}"
        );
    }

    #[test]
    fn lm_two_parameter_roughness_and_demand() {
        const TRUE_ROUGHNESS_FACTOR: f64 = 1.18;
        const TRUE_DEMAND_SCALE: f64 = 1.12;

        let network = three_node_network(0.012);
        let mut true_network = network.clone();
        for pipe in true_network.graph.edge_weights_mut() {
            pipe.roughness_mm *= TRUE_ROUGHNESS_FACTOR;
        }

        let base_demands = HashMap::from([
            (String::from("mid"), -7.0),
            (String::from("sink"), -14.0),
        ]);
        let mut true_demands = base_demands.clone();
        true_demands.insert(
            String::from("sink"),
            base_demands["sink"] * TRUE_DEMAND_SCALE,
        );

        let synthetic = solver::solve_steady_state_with_composition(
            &true_network,
            &true_demands,
            solver::GasComposition::default(),
            1000,
            5e-4,
        )
        .expect("synthetic solve");
        let measurements = vec![
            ScadaMeasurement {
                id: "mid".to_string(),
                measurement_type: MeasurementType::Pressure,
                value: synthetic.pressures["mid"],
                timestamp: None,
                uncertainty: None,
            },
            ScadaMeasurement {
                id: "sink".to_string(),
                measurement_type: MeasurementType::Pressure,
                value: synthetic.pressures["sink"],
                timestamp: None,
                uncertainty: None,
            },
        ];

        let initial = vec![
            CalibrationParameter::GlobalRoughnessFactor { factor: 1.0 },
            CalibrationParameter::DemandScale {
                node_id: "sink".to_string(),
                factor: 1.0,
            },
        ];
        let optimized = optimize_lm(&network, &base_demands, &measurements, initial);

        let roughness_factor = match &optimized[0] {
            CalibrationParameter::GlobalRoughnessFactor { factor } => *factor,
            other => panic!("expected roughness factor, got {other:?}"),
        };
        let demand_scale = match &optimized[1] {
            CalibrationParameter::DemandScale { factor, .. } => *factor,
            other => panic!("expected demand scale, got {other:?}"),
        };

        let final_residuals = compute_residuals_with_params(
            &network,
            &base_demands,
            &measurements,
            &optimized,
        );
        let initial_residuals = compute_residuals_with_params(
            &network,
            &base_demands,
            &measurements,
            &[
                CalibrationParameter::GlobalRoughnessFactor { factor: 1.0 },
                CalibrationParameter::DemandScale {
                    node_id: "sink".to_string(),
                    factor: 1.0,
                },
            ],
        );
        let final_rmse = (final_residuals.iter().map(|r| r * r).sum::<f64>()
            / final_residuals.len() as f64)
            .sqrt();
        let initial_rmse = (initial_residuals.iter().map(|r| r * r).sum::<f64>()
            / initial_residuals.len() as f64)
            .sqrt();

        assert!(
            final_rmse < initial_rmse,
            "two-parameter LM should improve fit: before={initial_rmse}, after={final_rmse}"
        );
        assert!(
            (roughness_factor - TRUE_ROUGHNESS_FACTOR).abs() < 0.08,
            "roughness factor should be near {TRUE_ROUGHNESS_FACTOR}, got {roughness_factor}"
        );
        assert!(
            (demand_scale - TRUE_DEMAND_SCALE).abs() < 0.08,
            "demand scale should be near {TRUE_DEMAND_SCALE}, got {demand_scale}"
        );
    }
}
