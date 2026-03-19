use std::collections::HashMap;

use anyhow::Result;
use serde::Serialize;

use crate::graph::GasNetwork;

use super::{SolverControl, SolverProgress, SolverResult, solve_steady_state_with_progress};

/// Bounds on node flow capacity: (min_m3s, max_m3s).
#[derive(Debug, Clone)]
pub struct CapacityBounds {
    pub node_bounds: HashMap<String, (f64, f64)>,
    pub pipe_bounds: HashMap<String, (f64, f64)>,
}

impl CapacityBounds {
    /// Build capacity bounds from a GasNetwork by reading flow_min/flow_max from nodes and pipes.
    pub fn from_network(network: &GasNetwork) -> Self {
        let mut node_bounds = HashMap::new();
        for node in network.nodes() {
            if let (Some(min), Some(max)) = (node.flow_min_m3s, node.flow_max_m3s) {
                if (max - min).abs() > 1e-12 || min.abs() > 1e-12 {
                    node_bounds.insert(node.id.clone(), (min, max));
                }
            }
        }
        let mut pipe_bounds = HashMap::new();
        for pipe in network.pipes() {
            if let (Some(min), Some(max)) = (pipe.flow_min_m3s, pipe.flow_max_m3s) {
                pipe_bounds.insert(pipe.id.clone(), (min, max));
            }
        }
        CapacityBounds {
            node_bounds,
            pipe_bounds,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.node_bounds.is_empty() && self.pipe_bounds.is_empty()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CapacityViolation {
    pub element_id: String,
    pub element_type: ViolationElementType,
    pub bound_type: BoundType,
    pub limit: f64,
    pub actual: f64,
    pub margin: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViolationElementType {
    Node,
    Pipe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundType {
    Min,
    Max,
}

/// Compute the effective flow at each node from the solver result.
/// For free nodes: effective flow = demand (input).
/// For slack nodes: effective flow = net pipe balance (sum Q_out - sum Q_in at that node).
pub fn compute_node_effective_flows(
    network: &GasNetwork,
    result: &SolverResult,
    demands: &HashMap<String, f64>,
) -> HashMap<String, f64> {
    let mut flows: HashMap<String, f64> = network
        .nodes()
        .map(|n| (n.id.clone(), demands.get(&n.id).copied().unwrap_or(0.0)))
        .collect();

    let mut pipe_balance: HashMap<String, f64> = HashMap::new();
    for pipe in network.pipes() {
        let q = result.flows.get(&pipe.id).copied().unwrap_or(0.0);
        *pipe_balance.entry(pipe.from.clone()).or_default() -= q; // outgoing
        *pipe_balance.entry(pipe.to.clone()).or_default() += q; // incoming
    }
    for node in network.nodes() {
        if node.pressure_fixed_bar.is_some() {
            let pb = pipe_balance.get(&node.id).copied().unwrap_or(0.0);
            flows.insert(node.id.clone(), -pb);
        }
    }
    flows
}

/// Check all capacity bounds and return violations.
pub fn check_capacity_violations(
    network: &GasNetwork,
    result: &SolverResult,
    demands: &HashMap<String, f64>,
    bounds: &CapacityBounds,
) -> Vec<CapacityViolation> {
    let effective_flows = compute_node_effective_flows(network, result, demands);
    let mut violations = Vec::new();

    for (node_id, &(min, max)) in &bounds.node_bounds {
        if let Some(&actual) = effective_flows.get(node_id) {
            if actual < min - 1e-6 {
                violations.push(CapacityViolation {
                    element_id: node_id.clone(),
                    element_type: ViolationElementType::Node,
                    bound_type: BoundType::Min,
                    limit: min,
                    actual,
                    margin: actual - min,
                });
            }
            if actual > max + 1e-6 {
                violations.push(CapacityViolation {
                    element_id: node_id.clone(),
                    element_type: ViolationElementType::Node,
                    bound_type: BoundType::Max,
                    limit: max,
                    actual,
                    margin: actual - max,
                });
            }
        }
    }

    for (pipe_id, &(min, max)) in &bounds.pipe_bounds {
        if let Some(&actual) = result.flows.get(pipe_id) {
            if actual < min - 1e-6 {
                violations.push(CapacityViolation {
                    element_id: pipe_id.clone(),
                    element_type: ViolationElementType::Pipe,
                    bound_type: BoundType::Min,
                    limit: min,
                    actual,
                    margin: actual - min,
                });
            }
            if actual > max + 1e-6 {
                violations.push(CapacityViolation {
                    element_id: pipe_id.clone(),
                    element_type: ViolationElementType::Pipe,
                    bound_type: BoundType::Max,
                    limit: max,
                    actual,
                    margin: actual - max,
                });
            }
        }
    }

    violations
}

#[derive(Debug, Clone)]
pub struct ConstrainedSolverConfig {
    pub max_outer_iter: usize,
    pub demand_tolerance: f64,
    pub inner_max_iter: usize,
    pub inner_tolerance: f64,
    pub inner_snapshot_every: usize,
    pub relaxation_factor: f64,
}

impl Default for ConstrainedSolverConfig {
    fn default() -> Self {
        Self {
            max_outer_iter: 15,
            demand_tolerance: 5e-3,
            inner_max_iter: 1000,
            inner_tolerance: 5e-4,
            inner_snapshot_every: 5,
            relaxation_factor: 0.9,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ConstrainedSolverResult {
    pub pressures: HashMap<String, f64>,
    pub flows: HashMap<String, f64>,
    pub iterations: usize,
    pub residual: f64,
    pub outer_iterations: usize,
    pub adjusted_demands: HashMap<String, f64>,
    pub capacity_violations: Vec<CapacityViolation>,
    pub active_bounds: Vec<String>,
    pub objective_value: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub infeasibility_diagnostic: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConstrainedProgress {
    pub outer_iter: usize,
    pub inner_progress: SolverProgress,
}

/// Clamp demands to capacity bounds (interior).
fn clamp_demands(demands: &HashMap<String, f64>, bounds: &CapacityBounds) -> HashMap<String, f64> {
    let eps = 1e-6;
    let mut clamped = demands.clone();
    for (node_id, &(min, max)) in &bounds.node_bounds {
        if let Some(d) = clamped.get_mut(node_id) {
            *d = d.clamp(min + eps, max - eps);
        }
    }
    clamped
}

/// Identify which bounded nodes are "free" (non-slack) - these are the adjustable demands.
fn free_bounded_node_ids(network: &GasNetwork, bounds: &CapacityBounds) -> Vec<String> {
    network
        .nodes()
        .filter(|n| n.pressure_fixed_bar.is_none() && bounds.node_bounds.contains_key(&n.id))
        .map(|n| n.id.clone())
        .collect()
}

/// Identify which bounded nodes are slack - these can only be checked.
fn slack_bounded_node_ids(network: &GasNetwork, bounds: &CapacityBounds) -> Vec<String> {
    network
        .nodes()
        .filter(|n| n.pressure_fixed_bar.is_some() && bounds.node_bounds.contains_key(&n.id))
        .map(|n| n.id.clone())
        .collect()
}

/// Compute slack violations (how much each slack node exceeds its bounds).
fn compute_slack_excess(
    slack_ids: &[String],
    effective_flows: &HashMap<String, f64>,
    bounds: &CapacityBounds,
) -> f64 {
    let mut total_excess = 0.0;
    for sid in slack_ids {
        if let (Some(&actual), Some(&(min, max))) =
            (effective_flows.get(sid), bounds.node_bounds.get(sid))
        {
            if actual > max {
                total_excess += actual - max;
            } else if actual < min {
                total_excess += min - actual;
            }
        }
    }
    total_excess
}

/// Proportional demand reduction to satisfy slack bounds.
/// When total demand exceeds slack capacity, reduce free demands proportionally.
fn proportional_demand_reduction(
    demands: &mut HashMap<String, f64>,
    free_ids: &[String],
    slack_ids: &[String],
    effective_flows: &HashMap<String, f64>,
    bounds: &CapacityBounds,
    relaxation: f64,
) -> f64 {
    let mut slack_over = 0.0_f64;
    let mut slack_under = 0.0_f64;
    for sid in slack_ids {
        if let (Some(&actual), Some(&(min, max))) =
            (effective_flows.get(sid), bounds.node_bounds.get(sid))
        {
            if actual > max + 1e-6 {
                slack_over += actual - max;
            } else if actual < min - 1e-6 {
                slack_under += min - actual;
            }
        }
    }

    if slack_over < 1e-6 && slack_under < 1e-6 {
        return 0.0;
    }

    let excess = slack_over - slack_under;

    let total_adjustable: f64 = free_ids
        .iter()
        .filter_map(|id| demands.get(id))
        .map(|d| d.abs())
        .sum();

    if total_adjustable < 1e-12 {
        return excess.abs();
    }

    let mut max_delta = 0.0_f64;
    for fid in free_ids {
        if let (Some(d), Some(&(min, max))) = (demands.get_mut(fid), bounds.node_bounds.get(fid)) {
            let weight = d.abs() / total_adjustable;
            let adjustment = relaxation * excess * weight;
            let old = *d;
            *d = (*d + adjustment).clamp(min + 1e-6, max - 1e-6);
            max_delta = max_delta.max((*d - old).abs());
        }
    }

    max_delta
}

fn find_active_bounds(demands: &HashMap<String, f64>, bounds: &CapacityBounds) -> Vec<String> {
    let eps = 1e-4;
    let mut active = Vec::new();
    for (node_id, &(min, max)) in &bounds.node_bounds {
        if let Some(&d) = demands.get(node_id) {
            if (d - min).abs() < eps || (d - max).abs() < eps {
                active.push(node_id.clone());
            }
        }
    }
    active
}

fn compute_objective(
    target: &HashMap<String, f64>,
    actual: &HashMap<String, f64>,
    free_ids: &[String],
) -> f64 {
    free_ids
        .iter()
        .map(|id| {
            let t = target.get(id).copied().unwrap_or(0.0);
            let a = actual.get(id).copied().unwrap_or(0.0);
            (a - t).powi(2)
        })
        .sum()
}

/// Solve the gas network with capacity constraints.
/// Mode: clamp free demands, solve, check slack, reduce proportionally if needed.
pub fn solve_steady_state_constrained<F>(
    network: &GasNetwork,
    target_demands: &HashMap<String, f64>,
    bounds: &CapacityBounds,
    initial_pressures_bar: Option<&HashMap<String, f64>>,
    config: ConstrainedSolverConfig,
    mut on_progress: F,
) -> Result<ConstrainedSolverResult>
where
    F: FnMut(ConstrainedProgress) -> SolverControl,
{
    let free_ids = free_bounded_node_ids(network, bounds);
    let slack_ids = slack_bounded_node_ids(network, bounds);

    let mut demands = clamp_demands(target_demands, bounds);
    let mut warm_pressures: Option<HashMap<String, f64>> = initial_pressures_bar.cloned();
    let mut best_result: Option<SolverResult> = None;
    let mut prev_slack_excess = f64::MAX;
    let mut stagnation_count = 0usize;

    for outer_iter in 0..config.max_outer_iter {
        let result = solve_steady_state_with_progress(
            network,
            &demands,
            warm_pressures.as_ref(),
            config.inner_max_iter,
            config.inner_tolerance,
            config.inner_snapshot_every,
            |progress| {
                let cp = ConstrainedProgress {
                    outer_iter: outer_iter + 1,
                    inner_progress: progress,
                };
                on_progress(cp)
            },
        )?;

        let effective_flows = compute_node_effective_flows(network, &result, &demands);
        let slack_excess = compute_slack_excess(&slack_ids, &effective_flows, bounds);

        warm_pressures = Some(result.pressures.clone());
        best_result = Some(result);

        if slack_excess < config.demand_tolerance {
            let result = best_result.unwrap();
            let violations = check_capacity_violations(network, &result, &demands, bounds);
            let active = find_active_bounds(&demands, bounds);
            let objective = compute_objective(target_demands, &demands, &free_ids);
            return Ok(ConstrainedSolverResult {
                pressures: result.pressures,
                flows: result.flows,
                iterations: result.iterations,
                residual: result.residual,
                outer_iterations: outer_iter + 1,
                adjusted_demands: demands,
                capacity_violations: violations,
                active_bounds: active,
                objective_value: objective,
                infeasibility_diagnostic: None,
            });
        }

        if (prev_slack_excess - slack_excess).abs() < config.demand_tolerance * 0.1 {
            stagnation_count += 1;
        } else {
            stagnation_count = 0;
        }
        if stagnation_count >= 3 {
            let result = best_result.unwrap();
            let violations = check_capacity_violations(network, &result, &demands, bounds);
            let active = find_active_bounds(&demands, bounds);
            let objective = compute_objective(target_demands, &demands, &free_ids);
            return Ok(ConstrainedSolverResult {
                pressures: result.pressures,
                flows: result.flows,
                iterations: result.iterations,
                residual: result.residual,
                outer_iterations: outer_iter + 1,
                adjusted_demands: demands,
                capacity_violations: violations,
                active_bounds: active,
                objective_value: objective,
                infeasibility_diagnostic: Some(format!(
                    "stagnation after {} outer iterations, slack excess={:.3e}",
                    outer_iter + 1,
                    slack_excess
                )),
            });
        }
        prev_slack_excess = slack_excess;

        proportional_demand_reduction(
            &mut demands,
            &free_ids,
            &slack_ids,
            &effective_flows,
            bounds,
            config.relaxation_factor,
        );
    }

    let result = best_result.unwrap();
    let violations = check_capacity_violations(network, &result, &demands, bounds);
    let active = find_active_bounds(&demands, bounds);
    let objective = compute_objective(target_demands, &demands, &free_ids);
    Ok(ConstrainedSolverResult {
        pressures: result.pressures,
        flows: result.flows,
        iterations: result.iterations,
        residual: result.residual,
        outer_iterations: config.max_outer_iter,
        adjusted_demands: demands,
        capacity_violations: violations,
        active_bounds: active,
        objective_value: objective,
        infeasibility_diagnostic: Some(format!(
            "max outer iterations ({}) reached",
            config.max_outer_iter
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{ConnectionKind, GasNetwork, Node, Pipe};
    use crate::solver::solve_steady_state;

    fn make_two_node_network() -> GasNetwork {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "S".into(),
            x: 0.0,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
            flow_min_m3s: Some(0.0),
            flow_max_m3s: Some(100.0),
        });
        net.add_node(Node {
            id: "D".into(),
            x: 1.0,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
            flow_min_m3s: Some(-50.0),
            flow_max_m3s: Some(0.0),
        });
        net.add_pipe(Pipe {
            id: "P1".into(),
            from: "S".into(),
            to: "D".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 50.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net
    }

    fn make_y_network() -> GasNetwork {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "S".into(),
            x: 0.0,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
            flow_min_m3s: Some(0.0),
            flow_max_m3s: Some(30.0),
        });
        net.add_node(Node {
            id: "J".into(),
            x: 0.5,
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
            id: "A".into(),
            x: 1.0,
            y: 0.5,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
            flow_min_m3s: Some(-50.0),
            flow_max_m3s: Some(0.0),
        });
        net.add_node(Node {
            id: "B".into(),
            x: 1.0,
            y: -0.5,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
            flow_min_m3s: Some(-50.0),
            flow_max_m3s: Some(0.0),
        });
        net.add_pipe(Pipe {
            id: "P_SJ".into(),
            from: "S".into(),
            to: "J".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 50.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net.add_pipe(Pipe {
            id: "P_JA".into(),
            from: "J".into(),
            to: "A".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 30.0,
            diameter_mm: 400.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net.add_pipe(Pipe {
            id: "P_JB".into(),
            from: "J".into(),
            to: "B".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 30.0,
            diameter_mm: 400.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net
    }

    #[test]
    fn test_capacity_bounds_from_network() {
        let net = make_two_node_network();
        let bounds = CapacityBounds::from_network(&net);
        assert_eq!(bounds.node_bounds.len(), 2);
        assert!(bounds.node_bounds.contains_key("S"));
        assert!(bounds.node_bounds.contains_key("D"));
        let (min, max) = bounds.node_bounds["D"];
        assert!((min - (-50.0)).abs() < 1e-9);
        assert!((max - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_no_violation_when_within_bounds() {
        let net = make_two_node_network();
        let bounds = CapacityBounds::from_network(&net);
        let demands: HashMap<String, f64> = [("D".into(), -10.0)].into_iter().collect();
        let result = solve_steady_state(&net, &demands, 1000, 5e-4).unwrap();
        let violations = check_capacity_violations(&net, &result, &demands, &bounds);
        assert!(
            violations.is_empty(),
            "expected no violations, got: {:?}",
            violations
        );
    }

    #[test]
    fn test_detects_overflow_violation() {
        let net = make_two_node_network();
        let bounds = CapacityBounds {
            node_bounds: [("S".into(), (0.0, 5.0)), ("D".into(), (-50.0, 0.0))]
                .into_iter()
                .collect(),
            pipe_bounds: HashMap::new(),
        };
        let demands: HashMap<String, f64> = [("D".into(), -20.0)].into_iter().collect();
        let result = solve_steady_state(&net, &demands, 1000, 5e-4).unwrap();
        let violations = check_capacity_violations(&net, &result, &demands, &bounds);
        assert!(!violations.is_empty(), "expected a violation for source S");
        let v = violations.iter().find(|v| v.element_id == "S").unwrap();
        assert_eq!(v.bound_type, BoundType::Max);
        assert!(v.actual > 5.0);
    }

    #[test]
    fn test_detects_underflow_violation() {
        let net = make_two_node_network();
        let bounds = CapacityBounds {
            node_bounds: [
                ("S".into(), (0.0, 100.0)),
                ("D".into(), (-15.0, -5.0)), // sink must withdraw between 5 and 15
            ]
            .into_iter()
            .collect(),
            pipe_bounds: HashMap::new(),
        };
        let demands: HashMap<String, f64> = [("D".into(), -2.0)].into_iter().collect();
        let result = solve_steady_state(&net, &demands, 1000, 5e-4).unwrap();
        let violations = check_capacity_violations(&net, &result, &demands, &bounds);
        let v = violations.iter().find(|v| v.element_id == "D").unwrap();
        assert_eq!(v.bound_type, BoundType::Max);
    }

    #[test]
    fn test_effective_flows_match_demands_for_free_nodes() {
        let net = make_two_node_network();
        let demands: HashMap<String, f64> = [("D".into(), -10.0)].into_iter().collect();
        let result = solve_steady_state(&net, &demands, 1000, 5e-4).unwrap();
        let effective = compute_node_effective_flows(&net, &result, &demands);
        assert!(
            (effective["D"] - (-10.0)).abs() < 1e-3,
            "D effective: {}",
            effective["D"]
        );
        assert!(effective["S"] > 0.0, "S should supply: {}", effective["S"]);
    }

    #[test]
    fn test_constrained_no_iteration_when_bounds_ok() {
        let net = make_two_node_network();
        let bounds = CapacityBounds::from_network(&net);
        let demands: HashMap<String, f64> = [("D".into(), -10.0)].into_iter().collect();
        let result = solve_steady_state_constrained(
            &net,
            &demands,
            &bounds,
            None,
            ConstrainedSolverConfig::default(),
            |_| SolverControl::Continue,
        )
        .unwrap();
        assert_eq!(
            result.outer_iterations, 1,
            "should converge in 1 outer iteration"
        );
        assert!(result.capacity_violations.is_empty());
        assert!(result.infeasibility_diagnostic.is_none());
    }

    #[test]
    fn test_constrained_reduces_demand_on_slack_violation() {
        let net = make_y_network();
        let bounds = CapacityBounds::from_network(&net);
        let demands: HashMap<String, f64> =
            [("A".into(), -20.0), ("B".into(), -20.0)].into_iter().collect();
        let result = solve_steady_state_constrained(
            &net,
            &demands,
            &bounds,
            None,
            ConstrainedSolverConfig::default(),
            |_| SolverControl::Continue,
        )
        .unwrap();
        let adj_a = result.adjusted_demands["A"];
        let adj_b = result.adjusted_demands["B"];
        let total = adj_a.abs() + adj_b.abs();
        assert!(
            total < 35.0,
            "total demand should be reduced to ~30, got {total}"
        );
        assert!(
            result.outer_iterations > 1,
            "should need multiple iterations"
        );
    }

    #[test]
    fn test_constrained_vs_unconstrained_wide_bounds() {
        let net = make_two_node_network();
        let bounds = CapacityBounds {
            node_bounds: [
                ("S".into(), (0.0, 1000.0)),
                ("D".into(), (-1000.0, 0.0)),
            ]
            .into_iter()
            .collect(),
            pipe_bounds: HashMap::new(),
        };
        let demands: HashMap<String, f64> = [("D".into(), -10.0)].into_iter().collect();
        let unconstrained = solve_steady_state(&net, &demands, 1000, 5e-4).unwrap();
        let constrained = solve_steady_state_constrained(
            &net,
            &demands,
            &bounds,
            None,
            ConstrainedSolverConfig::default(),
            |_| SolverControl::Continue,
        )
        .unwrap();
        for (id, &p_unc) in &unconstrained.pressures {
            let p_con = constrained.pressures[id];
            assert!(
                (p_unc - p_con).abs() < 0.1,
                "pressure mismatch at {id}: unconstrained={p_unc}, constrained={p_con}"
            );
        }
    }

    #[test]
    fn test_constrained_infeasible_returns_diagnostic() {
        let net = make_y_network();
        let bounds = CapacityBounds {
            node_bounds: [
                ("S".into(), (0.0, 5.0)),
                ("A".into(), (-50.0, -10.0)),
                ("B".into(), (-50.0, -10.0)),
            ]
            .into_iter()
            .collect(),
            pipe_bounds: HashMap::new(),
        };
        let demands: HashMap<String, f64> =
            [("A".into(), -20.0), ("B".into(), -20.0)].into_iter().collect();
        let result = solve_steady_state_constrained(
            &net,
            &demands,
            &bounds,
            None,
            ConstrainedSolverConfig {
                max_outer_iter: 10,
                ..Default::default()
            },
            |_| SolverControl::Continue,
        )
        .unwrap();
        assert!(
            result.infeasibility_diagnostic.is_some(),
            "should report infeasibility"
        );
    }

    #[test]
    fn test_constrained_gaslib11_wide_bounds() {
        let path = std::path::Path::new("dat/GasLib-11.net");
        if !path.exists() {
            eprintln!("skip: GasLib-11 data not found");
            return;
        }
        let net = crate::gaslib::load_network(path).unwrap();
        let scn = crate::gaslib::load_scenario_demands(std::path::Path::new(
            "dat/GasLib-11.scn",
        ))
        .unwrap();
        let bounds = CapacityBounds::from_network(&net);

        let unconstrained = solve_steady_state(&net, &scn.demands, 1000, 5e-4).unwrap();
        let constrained = solve_steady_state_constrained(
            &net,
            &scn.demands,
            &bounds,
            None,
            ConstrainedSolverConfig::default(),
            |_| SolverControl::Continue,
        )
        .unwrap();

        assert_eq!(constrained.outer_iterations, 1);
        assert!(constrained.infeasibility_diagnostic.is_none());

        for (id, &p_unc) in &unconstrained.pressures {
            let p_con = constrained.pressures[id];
            assert!(
                (p_unc - p_con).abs() < 0.5,
                "pressure mismatch at {id}: {p_unc} vs {p_con}"
            );
        }
    }
}
