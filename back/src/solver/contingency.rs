use std::collections::{HashMap, HashSet};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::graph::{ConnectionKind, GasNetwork};

use super::{SolverControl, SolverResult, SteadyStateConfig, solve_steady_state_with_progress};

const DEFAULT_MIN_PRESSURE_BAR: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContingencyAction {
    RemovePipe,
    CloseValve,
    ClosePipe,
    DisableSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContingencyElementType {
    Compressor,
    Pipe,
    Source,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContingencyCase {
    pub element_id: String,
    pub element_type: ContingencyElementType,
    pub action: ContingencyAction,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PressureViolation {
    pub node_id: String,
    pub pressure_bar: f64,
    pub threshold_bar: f64,
    pub deficit_bar: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContingencyResult {
    pub case: ContingencyCase,
    pub converged: bool,
    pub min_pressure_bar: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub violations: Vec<PressureViolation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solver_result: Option<SolverResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContingencyReport {
    pub results: Vec<ContingencyResult>,
    pub red_cases: Vec<ContingencyCase>,
    pub green_cases: Vec<ContingencyCase>,
}

pub fn generate_n_minus_1_cases(network: &GasNetwork) -> Vec<ContingencyCase> {
    let mut cases = Vec::new();

    cases.extend(
        network
            .pipes()
            .filter(|pipe| pipe.kind == ConnectionKind::CompressorStation)
            .map(|pipe| ContingencyCase {
                element_id: pipe.id.clone(),
                element_type: ContingencyElementType::Compressor,
                action: ContingencyAction::RemovePipe,
            }),
    );

    cases.extend(
        network
            .pipes()
            .filter(|pipe| pipe.kind == ConnectionKind::Valve)
            .map(|pipe| ContingencyCase {
                element_id: pipe.id.clone(),
                element_type: ContingencyElementType::Pipe,
                action: ContingencyAction::CloseValve,
            }),
    );

    cases.extend(
        network
            .nodes()
            .filter(|node| node.pressure_fixed_bar.is_some())
            .map(|node| ContingencyCase {
                element_id: node.id.clone(),
                element_type: ContingencyElementType::Source,
                action: ContingencyAction::DisableSource,
            }),
    );

    cases
}

pub fn apply_contingency(network: &GasNetwork, case: &ContingencyCase) -> GasNetwork {
    let mut modified = network.clone();
    match case.action {
        ContingencyAction::RemovePipe => {
            let edge_idx = modified.graph.edge_indices().find(|idx| {
                modified
                    .graph
                    .edge_weight(*idx)
                    .is_some_and(|p| p.id == case.element_id)
            });
            if let Some(idx) = edge_idx {
                modified.graph.remove_edge(idx);
            }
        }
        ContingencyAction::CloseValve | ContingencyAction::ClosePipe => {
            for pipe in modified.graph.edge_weights_mut() {
                if pipe.id == case.element_id {
                    pipe.is_open = false;
                }
            }
        }
        ContingencyAction::DisableSource => {
            if let Some(node) = modified.node_mut(&case.element_id) {
                node.pressure_fixed_bar = None;
            }
        }
    }
    modified
}

pub fn run_contingency_analysis(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    cases: &[ContingencyCase],
    config: SteadyStateConfig,
) -> ContingencyReport {
    let results: Vec<ContingencyResult> = cases
        .par_iter()
        .map(|case| evaluate_contingency_case(network, demands, case, config))
        .collect();

    finalize_contingency_report(results)
}

pub fn evaluate_contingency_case(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    case: &ContingencyCase,
    config: SteadyStateConfig,
) -> ContingencyResult {
    let scenario = apply_contingency(network, case);
    match solve_steady_state_with_progress(&scenario, demands, None, config, |_| {
        SolverControl::Continue
    }) {
        Ok(solver_result) => {
            let min_pressure_bar = solver_result
                .pressures
                .values()
                .copied()
                .reduce(f64::min)
                .unwrap_or(0.0);
            let violations = collect_pressure_violations(&scenario, demands, &solver_result);
            ContingencyResult {
                case: case.clone(),
                converged: true,
                min_pressure_bar,
                violations,
                solver_result: Some(solver_result),
            }
        }
        Err(_) => ContingencyResult {
            case: case.clone(),
            converged: false,
            min_pressure_bar: 0.0,
            violations: Vec::new(),
            solver_result: None,
        },
    }
}

pub fn finalize_contingency_report(results: Vec<ContingencyResult>) -> ContingencyReport {
    let mut red_cases = Vec::new();
    let mut green_cases = Vec::new();
    for result in &results {
        if !result.converged || !result.violations.is_empty() {
            red_cases.push(result.case.clone());
        } else {
            green_cases.push(result.case.clone());
        }
    }

    ContingencyReport {
        results,
        red_cases,
        green_cases,
    }
}

fn collect_pressure_violations(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    solver_result: &SolverResult,
) -> Vec<PressureViolation> {
    let default_threshold = network
        .nodes()
        .filter_map(|node| node.pressure_lower_bar)
        .reduce(f64::min)
        .unwrap_or(DEFAULT_MIN_PRESSURE_BAR);

    let node_by_id: HashMap<&str, _> = network.nodes().map(|n| (n.id.as_str(), n)).collect();
    let delivery_thresholds = delivery_thresholds_by_node(network);
    let monitored_nodes = monitored_node_ids(network, demands);

    let mut violations = Vec::new();
    for node_id in monitored_nodes {
        let pressure = solver_result
            .pressures
            .get(node_id.as_str())
            .copied()
            .unwrap_or(0.0);
        let threshold = delivery_thresholds
            .get(node_id.as_str())
            .copied()
            .or_else(|| {
                node_by_id
                    .get(node_id.as_str())
                    .and_then(|n| n.pressure_lower_bar)
            })
            .unwrap_or(default_threshold);
        if pressure + 1e-9 < threshold {
            violations.push(PressureViolation {
                node_id,
                pressure_bar: pressure,
                threshold_bar: threshold,
                deficit_bar: (threshold - pressure).max(0.0),
            });
        }
    }
    violations
}

fn monitored_node_ids(network: &GasNetwork, demands: &HashMap<String, f64>) -> Vec<String> {
    let mut ids = HashSet::new();
    for (node_id, demand) in demands {
        if *demand < 0.0 {
            ids.insert(node_id.clone());
        }
    }
    for pipe in network.pipes() {
        if pipe.kind == ConnectionKind::DeliveryStation {
            ids.insert(pipe.to.clone());
        }
    }
    ids.into_iter().collect()
}

fn delivery_thresholds_by_node(network: &GasNetwork) -> HashMap<String, f64> {
    let mut thresholds = HashMap::new();
    for pipe in network.pipes() {
        if pipe.kind != ConnectionKind::DeliveryStation {
            continue;
        }
        let Some(min_pressure) = pipe.equipment.delivery_min_pressure_bar else {
            continue;
        };
        let current = thresholds.entry(pipe.to.clone()).or_insert(min_pressure);
        *current = current.max(min_pressure);
    }
    thresholds
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EquipmentSpec, Node, Pipe};

    fn contingency_test_network() -> GasNetwork {
        let mut network = GasNetwork::new();
        network.add_node(Node {
            id: "S".into(),
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
        network.add_node(Node {
            id: "D".into(),
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
        network.add_pipe(Pipe {
            id: "C1".into(),
            from: "S".into(),
            to: "D".into(),
            kind: ConnectionKind::CompressorStation,
            is_open: true,
            length_km: 10.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: Some(1.08),
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        network.add_pipe(Pipe {
            id: "V1".into(),
            from: "S".into(),
            to: "D".into(),
            kind: ConnectionKind::Valve,
            is_open: true,
            length_km: 5.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        network
    }

    #[test]
    fn contingency_generate_cases_covers_expected_assets() {
        let network = contingency_test_network();
        let cases = generate_n_minus_1_cases(&network);

        assert!(cases.iter().any(|c| {
            c.element_id == "C1"
                && c.element_type == ContingencyElementType::Compressor
                && c.action == ContingencyAction::RemovePipe
        }));
        assert!(cases.iter().any(|c| {
            c.element_id == "V1"
                && c.element_type == ContingencyElementType::Pipe
                && c.action == ContingencyAction::CloseValve
        }));
        assert!(cases.iter().any(|c| {
            c.element_id == "S"
                && c.element_type == ContingencyElementType::Source
                && c.action == ContingencyAction::DisableSource
        }));
    }

    #[test]
    fn contingency_apply_case_closes_valve_and_disables_source() {
        let network = contingency_test_network();

        let closed = apply_contingency(
            &network,
            &ContingencyCase {
                element_id: "V1".into(),
                element_type: ContingencyElementType::Pipe,
                action: ContingencyAction::CloseValve,
            },
        );
        assert!(
            closed
                .pipes()
                .find(|pipe| pipe.id == "V1")
                .is_some_and(|pipe| !pipe.is_open)
        );

        let source_off = apply_contingency(
            &network,
            &ContingencyCase {
                element_id: "S".into(),
                element_type: ContingencyElementType::Source,
                action: ContingencyAction::DisableSource,
            },
        );
        assert_eq!(
            source_off
                .nodes()
                .find(|node| node.id == "S")
                .and_then(|node| node.pressure_fixed_bar),
            None
        );
    }

    #[test]
    fn contingency_run_analysis_flags_delivery_pressure_violation() {
        let mut network = GasNetwork::new();
        network.add_node(Node {
            id: "SRC".into(),
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
        network.add_node(Node {
            id: "DEL".into(),
            x: 1.0,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: Some(75.0),
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        network.add_pipe(Pipe {
            id: "PDL".into(),
            from: "SRC".into(),
            to: "DEL".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 5.0,
            diameter_mm: 600.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });

        let report = run_contingency_analysis(
            &network,
            &HashMap::from([("DEL".to_string(), -5.0)]),
            &[ContingencyCase {
                element_id: "unknown".into(),
                element_type: ContingencyElementType::Pipe,
                action: ContingencyAction::ClosePipe,
            }],
            SteadyStateConfig {
                max_iter: 400,
                tolerance: 1e-4,
                ..SteadyStateConfig::default()
            },
        );

        assert_eq!(report.results.len(), 1);
        let result = &report.results[0];
        assert!(result.converged);
        assert!(!result.violations.is_empty());
        assert_eq!(report.red_cases.len(), 1);
        assert_eq!(report.green_cases.len(), 0);
    }
}
