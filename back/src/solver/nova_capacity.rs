//! Étude NoVa : débit maximal faisable par sink sous bornes pression (Phase VII-bis).

use anyhow::Result;
use serde::Serialize;

use crate::gaslib::{
    ScenarioDemands, effective_solver_demands_for_network, network_with_scenario_boundaries,
    nova_sink_capacity_study_enabled,
};
use crate::graph::GasNetwork;

use super::continuation::solve_steady_state_with_preset;
use super::gas_properties::GasComposition;
use super::presets::SolverPreset;
use super::steady_state::{SolverControl, SolverResult};

const DEFAULT_BISECTION_STEPS: usize = 8;
const DEFAULT_PRESSURE_TOL_BAR: f64 = 0.05;
const DEFAULT_RESIDUAL_FACTOR: f64 = 10.0;

/// Sinks marginaux GasLib-582 mild_618 (sondes Phase II–VII).
pub fn default_marginal_sink_ids() -> Vec<&'static str> {
    vec!["sink_88", "sink_83", "sink_108", "sink_125", "sink_122"]
}

#[derive(Debug, Clone, Serialize)]
pub struct SinkCapacityReport {
    pub sink_id: String,
    pub nominal_q_m3s: f64,
    pub max_feasible_q_m3s: f64,
    pub feasible_fraction: f64,
    pub pressure_lower_bar: Option<f64>,
    pub pressure_at_max_bar: Option<f64>,
    pub pressure_shortfall_bar: f64,
    pub residual_at_max_m3s: f64,
    pub bisection_steps: usize,
    pub feasible_at_nominal: bool,
}

fn bisection_steps() -> usize {
    std::env::var("GAZFLOW_NOVA_CAPACITY_BISECTION_STEPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_BISECTION_STEPS)
}

fn residual_tolerance_factor() -> f64 {
    std::env::var("GAZFLOW_NOVA_CAPACITY_RESIDUAL_FACTOR")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|f: &f64| f.is_finite() && *f > 0.0)
        .unwrap_or(DEFAULT_RESIDUAL_FACTOR)
}

fn pressure_tolerance_bar() -> f64 {
    std::env::var("GAZFLOW_NOVA_CAPACITY_PRESSURE_TOL_BAR")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|t: &f64| t.is_finite() && *t >= 0.0)
        .unwrap_or(DEFAULT_PRESSURE_TOL_BAR)
}

fn sink_pressure_lower_bar(network: &GasNetwork, sink_id: &str) -> Option<f64> {
    network
        .nodes()
        .find(|n| n.id == sink_id)
        .and_then(|n| n.pressure_lower_bar)
}

fn sink_delivery_feasible(
    network: &GasNetwork,
    result: &SolverResult,
    sink_id: &str,
    residual_tol: f64,
    pressure_tol_bar: f64,
) -> (bool, f64, Option<f64>) {
    let p = result.pressures.get(sink_id).copied().unwrap_or(0.0);
    let lower = sink_pressure_lower_bar(network, sink_id);
    let shortfall = lower.map(|lo| (lo - p).max(0.0)).unwrap_or(0.0);
    let pressure_ok = shortfall <= pressure_tol_bar;
    let residual_ok = result.residual <= residual_tol;
    (residual_ok && pressure_ok, shortfall, Some(p))
}

fn solve_at_sink_scale(
    base_network: &GasNetwork,
    scenario: &ScenarioDemands,
    sink_id: &str,
    scale: f64,
    preset: &SolverPreset,
    gas: GasComposition,
) -> Result<(GasNetwork, SolverResult)> {
    let mut scenario_scaled = scenario.clone();
    if let Some(q) = scenario_scaled.demands.get_mut(sink_id) {
        let nominal = scenario.demands.get(sink_id).copied().unwrap_or(*q);
        *q = nominal * scale.clamp(0.0, 1.0);
    }
    let network = network_with_scenario_boundaries(base_network, &scenario_scaled);
    let demands = effective_solver_demands_for_network(base_network, &scenario_scaled.demands, &scenario_scaled);
    let result = solve_steady_state_with_preset(
        &network,
        &demands,
        None,
        preset,
        gas,
        |_| SolverControl::Continue,
        None::<fn(crate::solver::continuation::ContinuationStepEvent)>,
    )?;
    Ok((network, result))
}

/// Recherche dichotomique du débit max faisable pour un sink (fraction de la nomination).
pub fn study_sink_max_feasible_delivery(
    base_network: &GasNetwork,
    scenario: &ScenarioDemands,
    sink_id: &str,
    preset: &SolverPreset,
    gas: GasComposition,
) -> Result<SinkCapacityReport> {
    let nominal_q = scenario
        .demands
        .get(sink_id)
        .copied()
        .unwrap_or(0.0)
        .abs();
    let pressure_lower = sink_pressure_lower_bar(base_network, sink_id);
    let residual_tol = preset.tolerance * residual_tolerance_factor();
    let pressure_tol = pressure_tolerance_bar();
    let steps = bisection_steps();

    if nominal_q <= 1e-12 {
        return Ok(SinkCapacityReport {
            sink_id: sink_id.to_string(),
            nominal_q_m3s: nominal_q,
            max_feasible_q_m3s: 0.0,
            feasible_fraction: 0.0,
            pressure_lower_bar: pressure_lower,
            pressure_at_max_bar: None,
            pressure_shortfall_bar: 0.0,
            residual_at_max_m3s: 0.0,
            bisection_steps: 0,
            feasible_at_nominal: true,
        });
    }

    let (_, result_nominal) =
        solve_at_sink_scale(base_network, scenario, sink_id, 1.0, preset, gas)?;
    let (feasible_nominal, shortfall_nominal, p_nominal) =
        sink_delivery_feasible(base_network, &result_nominal, sink_id, residual_tol, pressure_tol);

    let mut lo = 0.0_f64;
    let mut hi = if feasible_nominal { 1.0 } else { 1.0_f64 };
    let mut best_scale = if feasible_nominal { 1.0 } else { 0.0 };
    let mut best_result = result_nominal.clone();
    let mut best_shortfall = shortfall_nominal;
    let mut best_p = p_nominal;

    if !feasible_nominal {
        for _ in 0..steps {
            let mid = (lo + hi) * 0.5;
            let (_, result) = solve_at_sink_scale(base_network, scenario, sink_id, mid, preset, gas)?;
            let (ok, shortfall, p) =
                sink_delivery_feasible(base_network, &result, sink_id, residual_tol, pressure_tol);
            if ok {
                best_scale = mid;
                best_result = result;
                best_shortfall = shortfall;
                best_p = p;
                lo = mid;
            } else {
                hi = mid;
            }
        }
    }

    Ok(SinkCapacityReport {
        sink_id: sink_id.to_string(),
        nominal_q_m3s: nominal_q,
        max_feasible_q_m3s: nominal_q * best_scale,
        feasible_fraction: best_scale,
        pressure_lower_bar: pressure_lower,
        pressure_at_max_bar: best_p,
        pressure_shortfall_bar: best_shortfall,
        residual_at_max_m3s: best_result.residual,
        bisection_steps: steps,
        feasible_at_nominal: feasible_nominal,
    })
}

/// Étude capacité pour les sinks marginaux par défaut (opt-in via flag env).
pub fn study_default_marginal_sinks(
    base_network: &GasNetwork,
    scenario: &ScenarioDemands,
    preset: &SolverPreset,
    gas: GasComposition,
) -> Result<Vec<SinkCapacityReport>> {
    if !nova_sink_capacity_study_enabled() {
        return Ok(Vec::new());
    }
    let sink_ids: Vec<&'static str> = default_marginal_sink_ids()
        .into_iter()
        .filter(|s| scenario.demands.contains_key(*s))
        .collect();
    // Séquentiel : le solveur Newton utilise le pool rayon global en interne.
    // Lancer plusieurs solves en parallèle (par_iter ou thread::scope) sature le pool
    // et deadlock. On garde donc un solve à la fois ; l'étude reste opt-in (bench dédié).
    let mut reports = Vec::new();
    for sink_id in sink_ids {
        reports.push(study_sink_max_feasible_delivery(
            base_network,
            scenario,
            sink_id,
            preset,
            gas,
        )?);
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_marginal_sink_ids_includes_five_probes() {
        assert_eq!(default_marginal_sink_ids().len(), 5);
    }
}
