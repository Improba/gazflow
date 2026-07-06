//! Outer-loop NoVa : consignes aval des control valves (Phase VII).

use std::collections::HashMap;

use crate::compressor::downstream_bounded_sinks_from;
use crate::gaslib::control_valve_decision_variables_enabled;
use crate::graph::{ConnectionKind, GasNetwork};
use crate::solver::steady_state::{SolverControl, SolverProgress, SolverResult, solve_steady_state_with_progress};
use crate::solver::config::SteadyStateConfig;

const DEFAULT_OUTER_MAX_ITERS: usize = 8;
const DEFAULT_RELAX: f64 = 0.5;
const SETPOINT_SETTLE_EPS: f64 = 1e-3;

#[derive(Debug, Clone, Default)]
pub struct ControlValveDecisionUpdate {
    pub cv_id: String,
    pub from_node: String,
    pub to_node: String,
    pub setpoint_before_bar: f64,
    pub setpoint_after_bar: f64,
    pub pressure_out_max_bar: f64,
    pub downstream_deficits: Vec<ControlValveSinkDeficit>,
}

#[derive(Debug, Clone, Default)]
pub struct ControlValveSinkDeficit {
    pub sink_id: String,
    pub lower_bar: f64,
    pub p_resolved_bar: f64,
    pub deficit_bar: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ControlValveDecisionUpdateStats {
    pub updated: usize,
    pub max_delta_bar: f64,
    pub total_slack_before: f64,
    pub total_slack_after: f64,
    pub updates: Vec<ControlValveDecisionUpdate>,
}

fn decision_setpoint_target(
    current_bar: f64,
    max_deficit_bar: f64,
    out_max_bar: f64,
) -> Option<f64> {
    if !current_bar.is_finite() || !max_deficit_bar.is_finite() || !out_max_bar.is_finite() {
        return None;
    }
    if max_deficit_bar <= 0.0 {
        return None;
    }
    let target = (current_bar + max_deficit_bar).min(out_max_bar).max(current_bar);
    if target <= current_bar + SETPOINT_SETTLE_EPS {
        None
    } else {
        Some(target)
    }
}

/// Ajuste les consignes aval des control valves pour réduire les déficits P des sinks aval.
pub fn apply_control_valve_decision_updates(
    network: &mut GasNetwork,
    result: &SolverResult,
    relax: f64,
) -> ControlValveDecisionUpdateStats {
    let downstream_by_cv: HashMap<String, Vec<(String, f64)>> = network
        .pipes()
        .filter(|p| p.kind == ConnectionKind::ControlValve && p.hydraulically_active())
        .map(|pipe| {
            (
                pipe.id.clone(),
                downstream_bounded_sinks_from(network, &pipe.to),
            )
        })
        .collect();

    let mut stats = ControlValveDecisionUpdateStats::default();
    let mut sink_slack_before: HashMap<String, f64> = HashMap::new();
    let mut sink_slack_after: HashMap<String, f64> = HashMap::new();

    for pipe in network.pipes_mut() {
        if pipe.kind != ConnectionKind::ControlValve || !pipe.hydraulically_active() {
            continue;
        }
        let Some(out_max) = pipe.equipment.control_valve_pressure_out_max_bar else {
            continue;
        };
        let p_out_resolved = result.pressures.get(&pipe.to).copied().unwrap_or(0.0);
        let current = pipe
            .equipment
            .regulator_setpoint_bar
            .unwrap_or(p_out_resolved.max(1.0));

        let mut deficits = Vec::new();
        let mut max_deficit = 0.0_f64;
        for (sink_id, lower_bar) in downstream_by_cv.get(&pipe.id).cloned().unwrap_or_default() {
            let p_resolved_bar = result.pressures.get(&sink_id).copied().unwrap_or(0.0);
            let deficit_bar = (lower_bar - p_resolved_bar).max(0.0);
            max_deficit = max_deficit.max(deficit_bar);
            deficits.push(ControlValveSinkDeficit {
                sink_id: sink_id.clone(),
                lower_bar,
                p_resolved_bar,
                deficit_bar,
            });
            sink_slack_before
                .entry(sink_id.clone())
                .or_insert(deficit_bar);
            sink_slack_after
                .entry(sink_id)
                .and_modify(|v| *v = v.min(deficit_bar))
                .or_insert(deficit_bar);
        }

        let setpoint_after = if max_deficit > 0.0 {
            if let Some(target) = decision_setpoint_target(current, max_deficit, out_max) {
                (current + relax * (target - current)).clamp(current, out_max)
            } else {
                current
            }
        } else {
            pipe.equipment.regulator_setpoint_bar = None;
            current
        };
        if max_deficit > 0.0 {
            let delta = (setpoint_after - current).abs();
            if delta > SETPOINT_SETTLE_EPS {
                pipe.equipment.regulator_setpoint_bar = Some(setpoint_after);
                stats.updated += 1;
                stats.max_delta_bar = stats.max_delta_bar.max(delta);
            }
        }

        let projected_lift = (setpoint_after - current).max(0.0);
        for d in &deficits {
            let projected = (d.deficit_bar - projected_lift).max(0.0);
            sink_slack_after
                .entry(d.sink_id.clone())
                .and_modify(|v| *v = v.min(projected))
                .or_insert(projected);
        }

        stats.updates.push(ControlValveDecisionUpdate {
            cv_id: pipe.id.clone(),
            from_node: pipe.from.clone(),
            to_node: pipe.to.clone(),
            setpoint_before_bar: current,
            setpoint_after_bar: setpoint_after,
            pressure_out_max_bar: out_max,
            downstream_deficits: deficits,
        });
    }

    stats.total_slack_before = sink_slack_before.values().sum::<f64>();
    stats.total_slack_after = sink_slack_after.values().sum::<f64>();
    stats
}

fn control_valve_relax() -> f64 {
    std::env::var("GAZFLOW_CONTROL_VALVE_DECISION_RELAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|r: &f64| r.is_finite() && *r > 0.0 && *r <= 1.0)
        .unwrap_or(DEFAULT_RELAX)
}

fn control_valve_outer_max_iters() -> usize {
    std::env::var("GAZFLOW_CONTROL_VALVE_OUTER_MAX_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_OUTER_MAX_ITERS)
}

/// Résout avec outer-loop de décision sur les consignes control valve (Phase VII).
pub fn solve_with_control_valve_decision_loop<F>(
    network: &mut GasNetwork,
    demands: &std::collections::HashMap<String, f64>,
    initial_pressures: Option<&std::collections::HashMap<String, f64>>,
    config: SteadyStateConfig,
    on_progress: &mut F,
) -> anyhow::Result<SolverResult>
where
    F: FnMut(SolverProgress) -> SolverControl,
{
    if !control_valve_decision_variables_enabled() {
        return solve_steady_state_with_progress(
            network,
            demands,
            initial_pressures,
            config,
            &mut *on_progress,
        );
    }

    let relax = control_valve_relax();
    let max_iters = control_valve_outer_max_iters();
    let mut warm_start = initial_pressures.cloned();
    let mut total_iterations = 0usize;
    let mut last_result: Option<SolverResult> = None;
    let mut best_result: Option<SolverResult> = None;
    let mut previous_slack: Option<f64> = None;

    for outer in 0..max_iters {
        let mut result = solve_steady_state_with_progress(
            network,
            demands,
            warm_start.as_ref(),
            config,
            &mut *on_progress,
        )?;
        total_iterations += result.iterations;
        result.iterations = total_iterations;
        warm_start = Some(result.pressures.clone());
        last_result = Some(result.clone());
        if best_result
            .as_ref()
            .is_none_or(|best| result.residual < best.residual)
        {
            best_result = Some(result.clone());
        }

        let updates = apply_control_valve_decision_updates(network, &result, relax);
        let slack = updates.total_slack_before;
        if let Some(prev) = previous_slack
            && (updates.updated == 0 || slack >= prev - SETPOINT_SETTLE_EPS)
        {
            tracing::debug!(
                outer = outer + 1,
                slack,
                updated = updates.updated,
                "control valve decision outer loop: converged or stalled"
            );
            return Ok(result);
        }
        previous_slack = Some(slack);
        if updates.updated == 0 {
            return Ok(result);
        }
    }

    Ok(last_result.or(best_result).expect("control valve outer loop produced no result"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_setpoint_target_bumps_toward_deficit() {
        let target = decision_setpoint_target(40.0, 5.0, 50.0).expect("bump");
        assert!((target - 45.0).abs() < 1e-9);
    }

    #[test]
    fn decision_setpoint_target_clamps_to_out_max() {
        let target = decision_setpoint_target(40.0, 20.0, 50.0).expect("cap");
        assert!((target - 50.0).abs() < 1e-9);
    }

    #[test]
    fn decision_setpoint_target_skips_zero_deficit() {
        assert!(decision_setpoint_target(40.0, 0.0, 50.0).is_none());
    }
}
