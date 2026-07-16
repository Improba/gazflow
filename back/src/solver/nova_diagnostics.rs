//! Diagnostics NoVa : slips pression, alimentation amont, trace par sink.
//!
//! Évalue un résultat de solve contre les **bornes contractuelles scénario** (enveloppes
//! pression `.scn`), pas contre le plancher générique `.net`. Utilisé par l'API simulation
//! pour produire le verdict NoVa (Phase VII-bis → interface Natran).

use serde::Serialize;

use crate::gaslib::ScenarioDemands;
use crate::graph::GasNetwork;
use crate::solver::steady_state::{
    BoundaryPressureSupplyReport, ScenarioPressureMargin, ScenarioPressureSlip,
    boundary_pressure_supply_reports, scenario_pressure_margins, scenario_pressure_slips,
    upstream_pressure_trace,
};
use crate::solver::SolverResult;

/// Un saut de la trace amont (nœud → pression résolue).
#[derive(Debug, Clone, Serialize)]
pub struct UpstreamHop {
    pub node_id: String,
    pub pressure_bar: f64,
}

/// Diagnostic NoVa par sink déficitaire : trace amont + pression max reachable vs besoin.
#[derive(Debug, Clone, Serialize)]
pub struct SinkDiagnostic {
    pub node_id: String,
    pub trace: Vec<UpstreamHop>,
    pub max_upstream_pressure_bar: f64,
    pub required_lower_bar: Option<f64>,
    pub supply_gap_bar: f64,
}

/// Diagnostic NoVa agrégé (renvoyé par l'API simulation quand un scénario est fourni).
#[derive(Debug, Clone, Serialize, Default)]
pub struct NovaDiagnostics {
    pub pressure_slips: Vec<ScenarioPressureSlip>,
    #[serde(default)]
    pub pressure_margins: Vec<ScenarioPressureMargin>,
    pub boundary_supply: Vec<BoundaryPressureSupplyReport>,
    pub sink_diagnostics: Vec<SinkDiagnostic>,
}

const SINK_DIAGNOSTIC_MAX_HOPS: usize = 6;
const SINK_DIAGNOSTIC_MAX_SINKS: usize = 25;

/// Calcule les diagnostics NoVa : clone le réseau, applique les enveloppes pression scénario
/// (bornes contractuelles), puis évalue `result` contre ces bornes.
///
/// Ne modifie ni le réseau ni le résultat ; n'affecte pas le solve. Coût O(slips × hops).
pub fn compute_nova_diagnostics(
    base_network: &GasNetwork,
    scenario: &ScenarioDemands,
    result: &SolverResult,
) -> NovaDiagnostics {
    let mut diag_net = base_network.clone();
    crate::gaslib::apply_scenario_pressure_envelopes(&mut diag_net, scenario);

    let pressure_slips = scenario_pressure_slips(&diag_net, result);
    let pressure_margins = scenario_pressure_margins(&diag_net, result);
    let boundary_supply =
        boundary_pressure_supply_reports(&diag_net, result, &pressure_slips, SINK_DIAGNOSTIC_MAX_HOPS);

    let sink_diagnostics = pressure_slips
        .iter()
        .filter(|s| s.shortfall_bar > 0.0)
        .take(SINK_DIAGNOSTIC_MAX_SINKS)
        .map(|s| {
            let trace = upstream_pressure_trace(&diag_net, result, &s.node_id, SINK_DIAGNOSTIC_MAX_HOPS);
            let max_up = trace
                .iter()
                .map(|(_, p)| *p)
                .fold(s.solved_pressure_bar, f64::max);
            let supply_gap_bar = s
                .lower_bar
                .map(|lo| (lo - max_up).max(0.0))
                .unwrap_or(0.0);
            SinkDiagnostic {
                node_id: s.node_id.clone(),
                trace: trace
                    .into_iter()
                    .map(|(node_id, pressure_bar)| UpstreamHop {
                        node_id,
                        pressure_bar,
                    })
                    .collect(),
                max_upstream_pressure_bar: max_up,
                required_lower_bar: s.lower_bar,
                supply_gap_bar,
            }
        })
        .collect();

    NovaDiagnostics {
        pressure_slips,
        pressure_margins,
        boundary_supply,
        sink_diagnostics,
    }
}

/// Verdict NoVa dérivé : faisable seulement si convergé, scale nominal atteint, aucun slip.
pub fn nova_verdict(
    diagnostics: &NovaDiagnostics,
    converged: bool,
    tol_m3s: f64,
    result: &SolverResult,
) -> NovaVerdict {
    let deficit_sinks: Vec<String> = diagnostics
        .pressure_slips
        .iter()
        .filter(|s| s.shortfall_bar > 0.0)
        .map(|s| s.node_id.clone())
        .collect();
    let excess_nodes: Vec<String> = diagnostics
        .pressure_slips
        .iter()
        .filter(|s| s.excess_bar > 0.0)
        .map(|s| s.node_id.clone())
        .collect();
    let effectively_converged = converged && result.residual <= tol_m3s;
    let scale_ok = result
        .demand_scale_achieved
        .map(|s| s >= 1.0)
        .unwrap_or(true);
    let feasible = effectively_converged
        && scale_ok
        && deficit_sinks.is_empty()
        && excess_nodes.is_empty();
    let cause = if !effectively_converged {
        NovaCause::NotSolvedLocal
    } else if converged && !scale_ok {
        NovaCause::ScaleNotAchieved
    } else if !feasible
        && diagnostics
            .sink_diagnostics
            .iter()
            .all(|d| d.supply_gap_bar > 0.0)
        && !diagnostics.sink_diagnostics.is_empty()
    {
        NovaCause::PressureReachability
    } else if !feasible && !deficit_sinks.is_empty() {
        NovaCause::PressureDeficit
    } else if !feasible && !excess_nodes.is_empty() {
        NovaCause::PressureExcess
    } else if !feasible {
        NovaCause::PressureDeficit
    } else {
        NovaCause::Feasible
    };
    let solver_signature = if !effectively_converged {
        NovaSolverSignature::Unresolved
    } else {
        NovaSolverSignature::NewtonPosthoc
    };
    NovaVerdict {
        feasible,
        deficit_sinks,
        cause,
        converged: effectively_converged,
        demand_scale_achieved: result.demand_scale_achieved,
        residual_m3s: result.residual,
        iterations: result.iterations,
        solver_signature,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum NovaCause {
    Feasible,
    PressureDeficit,
    PressureExcess,
    PressureReachability,
    NotSolvedLocal,
    ScaleNotAchieved,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum NovaSolverSignature {
    NewtonPosthoc,
    IpoptEscalation,
    Unresolved,
}

#[derive(Debug, Clone, Serialize)]
pub struct NovaVerdict {
    pub feasible: bool,
    pub deficit_sinks: Vec<String>,
    pub cause: NovaCause,
    pub converged: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demand_scale_achieved: Option<f64>,
    pub residual_m3s: f64,
    pub iterations: usize,
    pub solver_signature: NovaSolverSignature,
}

/// Phase VIII — rapport de faisabilité NoVa borné (solveur local).
///
/// Vérifie les bornes pression `[pressure_lower_bar, pressure_upper_bar]` de **tous** les
/// nœuds (bornes natives `.net` + enveloppes scénario déjà posées sur le réseau passé en
/// argument) contre la solution `result`. Un nœud fixé (slack) est ignoré (sa pression est
/// imposée, supposée dans ses bornes par construction).
///
/// Critère : faisable si (a) le solveur a convergé (`result.residual < tol`) ET (b) aucun nœud
/// ne viole ses bornes au-delà de `pressure_tol_bar`. Comme un solveur local ne peut pas prouver
/// l'infeasibilité (Pfetsch et al., ZIB-12-41), un échec est renvoyé comme `NotSolved`, jamais
/// comme « infeasible ».
#[derive(Debug, Clone, Serialize)]
pub struct NovaFeasibilityReport {
    pub converged: bool,
    pub residual_m3s: f64,
    pub feasible: bool,
    pub cause: NovaFeasibilityCause,
    pub worst_lower_shortfall_bar: f64,
    pub worst_upper_excess_bar: f64,
    pub violations: Vec<NovaBoundViolation>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum NovaFeasibilityCause {
    Feasible,
    NotSolvedLocal,
    BoundViolation,
}

#[derive(Debug, Clone, Serialize)]
pub struct NovaBoundViolation {
    pub node_id: String,
    pub pressure_bar: f64,
    pub lower_bar: Option<f64>,
    pub upper_bar: Option<f64>,
    pub shortfall_bar: f64,
    pub excess_bar: f64,
}

/// Construit le rapport de faisabilité NoVa. `network` doit déjà porter les bornes (natives
/// `.net` + enveloppes scénario appliquées). `tol_m3s` = tolérance de convergence du solveur ;
/// `pressure_tol_bar` = marge sur les bornes pression (défaut 0,05 bar).
pub fn nova_feasibility_report(
    network: &GasNetwork,
    result: &SolverResult,
    converged: bool,
    tol_m3s: f64,
    pressure_tol_bar: f64,
) -> NovaFeasibilityReport {
    let mut violations = Vec::new();
    let mut worst_lo = 0.0_f64;
    let mut worst_hi = 0.0_f64;
    for n in network.nodes() {
        if n.pressure_fixed_bar.is_some() {
            continue;
        }
        let Some(&p) = result.pressures.get(&n.id) else { continue };
        let lo = n.pressure_lower_bar;
        let hi = n.pressure_upper_bar;
        let shortfall = lo.map(|l| (l - p).max(0.0)).unwrap_or(0.0);
        let excess = hi.map(|h| (p - h).max(0.0)).unwrap_or(0.0);
        if shortfall > pressure_tol_bar || excess > pressure_tol_bar {
            violations.push(NovaBoundViolation {
                node_id: n.id.clone(),
                pressure_bar: p,
                lower_bar: lo,
                upper_bar: hi,
                shortfall_bar: shortfall,
                excess_bar: excess,
            });
            worst_lo = worst_lo.max(shortfall);
            worst_hi = worst_hi.max(excess);
        }
    }
    violations.sort_by(|a, b| {
        (b.shortfall_bar + b.excess_bar)
            .partial_cmp(&(a.shortfall_bar + a.excess_bar))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    violations.truncate(50);
    let bound_ok = violations.is_empty();
    let cause = if !converged || result.residual > tol_m3s {
        NovaFeasibilityCause::NotSolvedLocal
    } else if !bound_ok {
        NovaFeasibilityCause::BoundViolation
    } else {
        NovaFeasibilityCause::Feasible
    };
    let feasible = matches!(cause, NovaFeasibilityCause::Feasible);
    NovaFeasibilityReport {
        converged,
        residual_m3s: result.residual,
        feasible,
        cause,
        worst_lower_shortfall_bar: worst_lo,
        worst_upper_excess_bar: worst_hi,
        violations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::graph::{ConnectionKind, EquipmentSpec, GasNetwork, Node, Pipe};
    use crate::solver::steady_state::ScenarioPressureSlip;

    fn tiny_network() -> GasNetwork {
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
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net.add_node(Node {
            id: "T".into(),
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
            id: "P".into(),
            from: "S".into(),
            to: "T".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 5.0,
            diameter_mm: 500.0,
            roughness_mm: 0.05,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        net
    }

    fn scenario_with_lower_bound(sink_id: &str, lower: f64, upper: f64) -> ScenarioDemands {
        ScenarioDemands {
            scenario_id: None,
            demands: HashMap::from([(sink_id.to_string(), -3.0)]),
            pressure_slack: None,
            balance_hubs: Vec::new(),
            junction_anchors: Vec::new(),
            boundary_spine_anchors: Vec::new(),
            mass_balance_anchors: Vec::new(),
            zero_flow_boundary_anchors: Vec::new(),
            contract_flow_relaxed: Vec::new(),
            contract_pressure_anchors: Vec::new(),
            pressure_envelopes: vec![crate::gaslib::ScenarioPressureEnvelope {
                node_id: sink_id.to_string(),
                lower_bar: Some(lower),
                upper_bar: Some(upper),
            }],
        }
    }

    fn synthetic_result(p_t: f64) -> SolverResult {
        let mut r = SolverResult::from_core(
            HashMap::from([("S".to_string(), 70.0), ("T".to_string(), p_t)]),
            HashMap::new(),
            1,
            0.0,
        );
        r.demand_scale_achieved = Some(1.0);
        r
    }

    #[test]
    fn detects_pressure_deficit_and_supply_gap() {
        let net = tiny_network();
        let scenario = scenario_with_lower_bound("T", 80.0, 120.0);
        let result = synthetic_result(50.0);

        let diag = compute_nova_diagnostics(&net, &scenario, &result);

        // Le sink T est en déficit : 80 - 50 = 30 bar.
        let slip = diag
            .pressure_slips
            .iter()
            .find(|s: &&ScenarioPressureSlip| s.node_id == "T")
            .expect("T should be in pressure_slips");
        assert!((slip.shortfall_bar - 30.0).abs() < 1e-6, "shortfall={}", slip.shortfall_bar);

        // Trace amont : T (50) → S (70). max_up = 70, besoin 80 → gap 10.
        let sink_diag = diag
            .sink_diagnostics
            .iter()
            .find(|d| d.node_id == "T")
            .expect("T should be in sink_diagnostics");
        assert!((sink_diag.max_upstream_pressure_bar - 70.0).abs() < 1e-6);
        assert!((sink_diag.supply_gap_bar - 10.0).abs() < 1e-6, "gap={}", sink_diag.supply_gap_bar);
        assert_eq!(sink_diag.required_lower_bar, Some(80.0));
    }

    #[test]
    fn verdict_infeasible_when_excess_only() {
        let net = tiny_network();
        let scenario = scenario_with_lower_bound("T", 40.0, 45.0);
        let result = synthetic_result(50.0); // 50 > 45 → excess 5 bar, no shortfall
        let diag = compute_nova_diagnostics(&net, &scenario, &result);

        let verdict = nova_verdict(&diag, true, 1e-3, &result);
        assert!(!verdict.feasible);
        assert_eq!(verdict.cause, NovaCause::PressureExcess);
        assert!(verdict.deficit_sinks.is_empty());
        let slip = diag
            .pressure_slips
            .iter()
            .find(|s| s.node_id == "T")
            .expect("T should be in pressure_slips");
        assert!((slip.excess_bar - 5.0).abs() < 1e-6, "excess={}", slip.excess_bar);
    }

    #[test]
    fn verdict_infeasible_when_deficit_present() {
        let net = tiny_network();
        let scenario = scenario_with_lower_bound("T", 80.0, 120.0);
        let result = synthetic_result(50.0);
        let diag = compute_nova_diagnostics(&net, &scenario, &result);

        let verdict = nova_verdict(&diag, true, 1e-3, &result);
        assert!(!verdict.feasible);
        assert_eq!(verdict.cause, NovaCause::PressureReachability);
        assert_eq!(verdict.deficit_sinks, vec!["T".to_string()]);
        assert_eq!(verdict.solver_signature, NovaSolverSignature::NewtonPosthoc);
    }

    #[test]
    fn verdict_not_solved_when_non_converged() {
        let net = tiny_network();
        let scenario = scenario_with_lower_bound("T", 40.0, 120.0);
        let mut result = synthetic_result(50.0);
        result.residual = 5.0;
        let diag = compute_nova_diagnostics(&net, &scenario, &result);

        let verdict = nova_verdict(&diag, false, 1e-3, &result);
        assert!(!verdict.feasible);
        assert!(!verdict.converged);
        assert_eq!(verdict.cause, NovaCause::NotSolvedLocal);
        assert_eq!(verdict.solver_signature, NovaSolverSignature::Unresolved);
    }

    #[test]
    fn verdict_scale_not_achieved_when_below_one() {
        let net = tiny_network();
        let scenario = scenario_with_lower_bound("T", 40.0, 120.0);
        let mut result = synthetic_result(50.0);
        result.demand_scale_achieved = Some(0.8);
        let diag = compute_nova_diagnostics(&net, &scenario, &result);

        let verdict = nova_verdict(&diag, true, 1e-3, &result);
        assert!(!verdict.feasible);
        assert_eq!(verdict.cause, NovaCause::ScaleNotAchieved);
        assert_eq!(verdict.solver_signature, NovaSolverSignature::NewtonPosthoc);
    }

    #[test]
    fn verdict_feasible_when_scale_none_and_converged() {
        let net = tiny_network();
        let scenario = scenario_with_lower_bound("T", 40.0, 120.0);
        let mut result = synthetic_result(50.0);
        result.demand_scale_achieved = None;
        let diag = compute_nova_diagnostics(&net, &scenario, &result);

        let verdict = nova_verdict(&diag, true, 1e-3, &result);
        assert!(verdict.feasible);
        assert_eq!(verdict.cause, NovaCause::Feasible);
        assert_eq!(verdict.solver_signature, NovaSolverSignature::NewtonPosthoc);
    }

    #[test]
    fn verdict_feasible_when_no_deficit() {
        let net = tiny_network();
        let scenario = scenario_with_lower_bound("T", 40.0, 120.0);
        let result = synthetic_result(50.0); // 50 >= 40 → OK
        let diag = compute_nova_diagnostics(&net, &scenario, &result);

        let verdict = nova_verdict(&diag, true, 1e-3, &result);
        assert!(verdict.feasible);
        assert_eq!(verdict.cause, NovaCause::Feasible);
        assert_eq!(verdict.solver_signature, NovaSolverSignature::NewtonPosthoc);
    }

    #[test]
    fn pressure_margins_positive_when_feasible() {
        let net = tiny_network();
        let scenario = scenario_with_lower_bound("T", 40.0, 120.0);
        let result = synthetic_result(50.0);
        let diag = compute_nova_diagnostics(&net, &scenario, &result);

        let margin = diag
            .pressure_margins
            .iter()
            .find(|m| m.node_id == "T")
            .expect("T should be in pressure_margins");
        assert!(margin.margin_lower_bar.unwrap() > 0.0);
        assert!(margin.margin_upper_bar.unwrap() > 0.0);
    }

    #[test]
    fn pressure_margins_negative_when_deficit() {
        let net = tiny_network();
        let scenario = scenario_with_lower_bound("T", 80.0, 120.0);
        let result = synthetic_result(50.0);
        let diag = compute_nova_diagnostics(&net, &scenario, &result);

        let margin = diag
            .pressure_margins
            .iter()
            .find(|m| m.node_id == "T")
            .expect("T should be in pressure_margins");
        assert!(margin.margin_lower_bar.unwrap() < 0.0);
        assert!(margin.from_scenario_envelope);
    }

    #[test]
    fn nova_feasibility_report_feasible_when_in_bounds() {
        // T a des bornes natives [40, 120] ; P_T = 50 → dans les bornes → faisable.
        let mut net = tiny_network();
        if let Some(t) = net.node_mut("T") {
            t.pressure_lower_bar = Some(40.0);
            t.pressure_upper_bar = Some(120.0);
        }
        let result = synthetic_result(50.0);
        let report = nova_feasibility_report(&net, &result, true, 1e-3, 0.05);
        assert!(report.feasible);
        assert_eq!(report.cause, NovaFeasibilityCause::Feasible);
        assert!(report.violations.is_empty());
    }

    #[test]
    fn nova_feasibility_report_violation_when_below_lower() {
        let mut net = tiny_network();
        if let Some(t) = net.node_mut("T") {
            t.pressure_lower_bar = Some(80.0);
            t.pressure_upper_bar = Some(120.0);
        }
        let result = synthetic_result(50.0); // 50 < 80 → shortfall 30
        let report = nova_feasibility_report(&net, &result, true, 1e-3, 0.05);
        assert!(!report.feasible);
        assert_eq!(report.cause, NovaFeasibilityCause::BoundViolation);
        assert!((report.worst_lower_shortfall_bar - 30.0).abs() < 1e-6);
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].node_id, "T");
    }

    #[test]
    fn nova_feasibility_report_not_solved_when_non_converged() {
        let mut net = tiny_network();
        if let Some(t) = net.node_mut("T") {
            t.pressure_lower_bar = Some(40.0);
            t.pressure_upper_bar = Some(120.0);
        }
        let mut result = synthetic_result(50.0);
        result.residual = 5.0; // non convergé
        let report = nova_feasibility_report(&net, &result, false, 1e-3, 0.05);
        assert!(!report.feasible);
        // Non convergé prévaut sur la vérification des bornes : un solveur local ne prouve
        // jamais l'infeasibilité.
        assert_eq!(report.cause, NovaFeasibilityCause::NotSolvedLocal);
    }

    #[test]
    fn nova_feasibility_report_ignores_fixed_nodes() {
        // S est fixé (slack) à 70 bar avec borne [80, 120] ; il doit être ignoré (pas de
        // violation signalée sur un nœud fixé).
        let mut net = tiny_network();
        if let Some(s) = net.node_mut("S") {
            s.pressure_lower_bar = Some(80.0);
            s.pressure_upper_bar = Some(120.0);
        }
        if let Some(t) = net.node_mut("T") {
            t.pressure_lower_bar = Some(40.0);
            t.pressure_upper_bar = Some(120.0);
        }
        let result = synthetic_result(50.0);
        let report = nova_feasibility_report(&net, &result, true, 1e-3, 0.05);
        assert!(report.feasible);
        assert!(report.violations.is_empty(), "fixed node S should be ignored");
    }

    /// Intégration GasLib-582 + mild_618 : valide le résolveur scénario et la cohérence
    /// des diagnostics (bornes contractuelles ~26 bar, pas le plancher .net ~2 bar).
    /// Pas de solve complet (lent sur 582) : on construit un résultat synthétique où
    /// tous les sinks sont à 5 bar, sous leurs bornes contractuelles.
    #[test]
    #[serial_test::serial]
    fn gaslib_582_mild_618_diagnostics_use_contractual_bounds() {
        use crate::gaslib::{
            enrich_scenario_with_balance_hub, load_network, load_scenario_demands, resolve_scenario_path,
        };
        use std::path::Path;

        // `compute_nova_diagnostics` applique les enveloppes lui-même, indépendamment
        // des flags globaux (GAZFLOW_SCENARIO_PRESSURE_ENVELOPES etc.) — on ne touche
        // donc pas aux variables d'environnement pour éviter toute interférence avec
        // les autres tests (non-série) qui y sont sensibles.

        let net_path = Path::new("dat/GasLib-582.net");
        if !net_path.exists() {
            eprintln!("skip: GasLib-582 data missing");
            return;
        }
        let scn_path = resolve_scenario_path(Path::new("dat"), "GasLib-582", "nomination_mild_618");
        let scn_path = match scn_path {
            Some(p) => p,
            None => {
                eprintln!("skip: nomination_mild_618.scn not resolved");
                return;
            }
        };

        let network = load_network(net_path).expect("load network");
        let mut scenario = load_scenario_demands(&scn_path).expect("load scenario");
        enrich_scenario_with_balance_hub(&network, &mut scenario);

        // Résultat synthétique : 5 bar partout (sous toute borne contractuelle ~26 bar).
        let pressures: HashMap<String, f64> = network
            .nodes()
            .map(|n| (n.id.clone(), 5.0))
            .collect();
        let result = SolverResult {
            pressures,
            flows: HashMap::new(),
            iterations: 1,
            residual: 0.0,
            equipment_states: Vec::new(),
            warnings: Vec::new(),
            demand_scale_achieved: Some(1.0),
        };

        let diag = compute_nova_diagnostics(&network, &scenario, &result);

        // Au moins une enveloppe contractuelle >= 20 bar doit apparaître (mild_618 a des
        // bornes ~26 bar sur les sinks). Cela distingue les bornes contractuelles du
        // plancher .net (~2 bar) — régression du bug Phase VII-bis.
        let high_contractual = diag
            .pressure_slips
            .iter()
            .any(|s| s.lower_bar.map(|lo| lo >= 20.0).unwrap_or(false));
        assert!(
            high_contractual,
            "aucun slip avec borne contractuelle >= 20 bar (slips={})",
            diag.pressure_slips.len()
        );

        // sink_diagnostics expose le gap amont (max_up = 5 bar partout → gap = borne - 5).
        for d in &diag.sink_diagnostics {
            assert!(d.supply_gap_bar >= 0.0, "gap négatif pour {}", d.node_id);
        }
        assert!(
            diag.sink_diagnostics
                .iter()
                .any(|d| d.required_lower_bar.map(|lo| lo >= 20.0).unwrap_or(false)),
            "aucun sink_diagnostic avec borne contractuelle >= 20 bar"
        );

        // Verdict infeasible (tous les sinks sous leur borne).
        let verdict = nova_verdict(&diag, true, 1e-3, &result);
        assert!(!verdict.feasible);
    }
}
