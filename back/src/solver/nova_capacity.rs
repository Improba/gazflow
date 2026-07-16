//! Étude NoVa : débit maximal faisable par sink sous bornes pression (Phase VII-bis).

use anyhow::Result;
use serde::Serialize;

use crate::gaslib::{
    ScenarioDemands, effective_solver_demands_for_network,
    network_with_scenario_boundaries_for_nova, nova_sink_capacity_study_enabled,
};
use crate::graph::GasNetwork;

use super::continuation::solve_steady_state_with_preset;
use super::gas_properties::GasComposition;
use super::presets::SolverPreset;
use super::steady_state::{SolverControl, SolverResult};

const DEFAULT_BISECTION_STEPS: usize = 8;
const DEFAULT_PRESSURE_TOL_BAR: f64 = 0.05;

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

/// Borne contractuelle d'un sink = enveloppe pression scénario appliquée (ex. 26,013 bar
/// pour sink_88), PAS le plancher générique du `.net` (~2 bar). On lit donc sur le réseau
/// après `network_with_scenario_boundaries`.
fn sink_contractual_lower_bar(
    base_network: &GasNetwork,
    scenario: &ScenarioDemands,
    sink_id: &str,
) -> Option<f64> {
    let boundary_net = network_with_scenario_boundaries_for_nova(base_network, scenario);
    sink_pressure_lower_bar(&boundary_net, sink_id)
}

/// Garde-fou divergence : résidu explosé (solve non convergé / divergé). Au-dessus, on
/// considère le solve inexploitable. La pénalité soft-setpoint peut gonfler le résidu
/// (~60 m³/s) sans divergence : on reste large.
fn divergence_guard_m3s(preset: &SolverPreset) -> f64 {
    std::env::var("GAZFLOW_NOVA_CAPACITY_DIVERGENCE_GUARD")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|g: &f64| g.is_finite() && *g > 0.0)
        .unwrap_or(1.0e6)
}

fn sink_delivery_feasible(
    network: &GasNetwork,
    result: &SolverResult,
    sink_id: &str,
    divergence_guard: f64,
    pressure_tol_bar: f64,
) -> (bool, f64, Option<f64>) {
    let p = result.pressures.get(sink_id).copied().unwrap_or(0.0);
    let lower = sink_pressure_lower_bar(network, sink_id);
    let shortfall = lower.map(|lo| (lo - p).max(0.0)).unwrap_or(0.0);
    let pressure_ok = shortfall <= pressure_tol_bar;
    // Critère NoVa capacité = pression du sink sous sa borne contractuelle. Le résidu
    // global n'est qu'un garde-fou de divergence (la nomination mild_618 reste globalement
    // infeasible à cause des autres sinks, c'est attendu).
    let not_diverged = result.residual.is_finite() && result.residual < divergence_guard;
    (pressure_ok && not_diverged, shortfall, Some(p))
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
    let network = network_with_scenario_boundaries_for_nova(base_network, &scenario_scaled);
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
/// `bisection_steps` est explicite (l'API le passe ; le bench lit l'env via `bisection_steps()`).
pub fn study_sink_max_feasible_delivery(
    base_network: &GasNetwork,
    scenario: &ScenarioDemands,
    sink_id: &str,
    preset: &SolverPreset,
    gas: GasComposition,
    bisection_steps: usize,
) -> Result<SinkCapacityReport> {
    let nominal_q = scenario
        .demands
        .get(sink_id)
        .copied()
        .unwrap_or(0.0)
        .abs();
    let pressure_lower = sink_contractual_lower_bar(base_network, scenario, sink_id);
    let divergence_guard = divergence_guard_m3s(preset);
    let pressure_tol = pressure_tolerance_bar();
    let steps = bisection_steps;

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

    let (boundary_nominal, result_nominal) =
        solve_at_sink_scale(base_network, scenario, sink_id, 1.0, preset, gas)?;
    let (feasible_nominal, shortfall_nominal, p_nominal) = sink_delivery_feasible(
        &boundary_nominal,
        &result_nominal,
        sink_id,
        divergence_guard,
        pressure_tol,
    );

    let mut lo = 0.0_f64;
    let mut hi = 1.0_f64;
    let mut best_scale = if feasible_nominal { 1.0 } else { 0.0 };
    let mut best_result = result_nominal.clone();
    let mut best_shortfall = shortfall_nominal;
    let mut best_p = p_nominal;

    if !feasible_nominal {
        for _ in 0..steps {
            let mid = (lo + hi) * 0.5;
            let (boundary_mid, result) =
                solve_at_sink_scale(base_network, scenario, sink_id, mid, preset, gas)?;
            let (ok, shortfall, p) = sink_delivery_feasible(
                &boundary_mid,
                &result,
                sink_id,
                divergence_guard,
                pressure_tol,
            );
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

/// Étude capacité pour des sinks explicites (pas de gate env — pour l'API `/api/nova/capacity`).
pub fn study_sinks_capacity(
    base_network: &GasNetwork,
    scenario: &ScenarioDemands,
    sink_ids: &[String],
    preset: &SolverPreset,
    gas: GasComposition,
    bisection_steps: usize,
) -> Result<Vec<SinkCapacityReport>> {
    // Séquentiel : le solveur Newton utilise le pool rayon global en interne.
    // Lancer plusieurs solves en parallèle (par_iter ou thread::scope) sature le pool
    // et deadlock. On garde donc un solve à la fois.
    let mut reports = Vec::with_capacity(sink_ids.len());
    for sink_id in sink_ids {
        if !scenario.demands.contains_key(sink_id) {
            continue;
        }
        reports.push(study_sink_max_feasible_delivery(
            base_network,
            scenario,
            sink_id,
            preset,
            gas,
            bisection_steps,
        )?);
    }
    Ok(reports)
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
    let sink_ids: Vec<String> = default_marginal_sink_ids()
        .into_iter()
        .filter(|s| scenario.demands.contains_key(*s))
        .map(|s| s.to_string())
        .collect();
    study_sinks_capacity(
        base_network,
        scenario,
        &sink_ids,
        preset,
        gas,
        bisection_steps(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gaslib::ScenarioPressureEnvelope;
    use crate::graph::{ConnectionKind, EquipmentSpec, GasNetwork, Node, Pipe};
    use crate::solver::{GasComposition, preset_for_node_count};
    use std::collections::HashMap;

    #[test]
    fn default_marginal_sink_ids_includes_five_probes() {
        assert_eq!(default_marginal_sink_ids().len(), 5);
    }

    fn tiny_network() -> GasNetwork {
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
            id: "p".into(),
            from: "source".into(),
            to: "sink".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 10.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        net
    }

    fn scenario_with_high_bound() -> ScenarioDemands {
        ScenarioDemands {
            scenario_id: None,
            demands: HashMap::from([("sink".to_string(), -5.0)]),
            pressure_slack: None,
            balance_hubs: Vec::new(),
            junction_anchors: Vec::new(),
            boundary_spine_anchors: Vec::new(),
            mass_balance_anchors: Vec::new(),
            zero_flow_boundary_anchors: Vec::new(),
            contract_flow_relaxed: Vec::new(),
            contract_pressure_anchors: Vec::new(),
            pressure_envelopes: vec![ScenarioPressureEnvelope {
                node_id: "sink".to_string(),
                lower_bar: Some(80.0),
                upper_bar: Some(120.0),
            }],
        }
    }

    #[test]
    fn study_sinks_capacity_reports_zero_when_bound_unreachable() {
        // Source fixé à 70 bar, borne contractuelle sink = 80 bar → irréalisable même à
        // débit nul (P_sink = 70 < 80). La dichotomie doit converger vers fraction = 0.
        let net = tiny_network();
        let scenario = scenario_with_high_bound();
        let preset = preset_for_node_count(net.node_count());
        let sink_ids = vec!["sink".to_string()];

        let reports = study_sinks_capacity(
            &net,
            &scenario,
            &sink_ids,
            &preset,
            GasComposition::pure_ch4(),
            6,
        )
        .expect("capacity study should succeed");

        assert_eq!(reports.len(), 1);
        let r = &reports[0];
        assert_eq!(r.sink_id, "sink");
        assert_eq!(r.pressure_lower_bar, Some(80.0));
        assert!(!r.feasible_at_nominal, "nominal should be infeasible");
        assert!(
            r.max_feasible_q_m3s <= r.nominal_q_m3s,
            "Q max faisable doit être borné par l'enveloppe scénario"
        );
        assert!(
            r.feasible_fraction <= 1e-6,
            "fraction should be ~0 (unreachable bound), got {}",
            r.feasible_fraction
        );
    }
}
