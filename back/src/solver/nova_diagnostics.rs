//! Diagnostics NoVa : slips pression, alimentation amont, trace par sink.
//!
//! Évalue un résultat de solve contre les **bornes contractuelles scénario** (enveloppes
//! pression `.scn`), pas contre le plancher générique `.net`. Utilisé par l'API simulation
//! pour produire le verdict NoVa (Phase VII-bis → interface Natran).

use std::collections::HashMap;

use serde::Serialize;

use crate::gaslib::ScenarioDemands;
use crate::graph::GasNetwork;
use crate::solver::steady_state::{
    BoundaryPressureSupplyReport, ScenarioPressureSlip, boundary_pressure_supply_reports,
    scenario_pressure_slips, upstream_pressure_trace,
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
        boundary_supply,
        sink_diagnostics,
    }
}

/// Verdict NoVa dérivé : faisable si aucun slip pression.
pub fn nova_verdict(diagnostics: &NovaDiagnostics, demand_scale_achieved: Option<f64>) -> NovaVerdict {
    let deficit_sinks: Vec<String> = diagnostics
        .pressure_slips
        .iter()
        .filter(|s| s.shortfall_bar > 0.0)
        .map(|s| s.node_id.clone())
        .collect();
    let feasible = deficit_sinks.is_empty()
        && demand_scale_achieved.map(|s| s >= 1.0).unwrap_or(true);
    let cause = if !feasible
        && diagnostics
            .sink_diagnostics
            .iter()
            .all(|d| d.supply_gap_bar > 0.0)
        && !diagnostics.sink_diagnostics.is_empty()
    {
        NovaCause::PressureReachability
    } else if !feasible {
        NovaCause::PressureDeficit
    } else {
        NovaCause::Feasible
    };
    NovaVerdict {
        feasible,
        deficit_sinks,
        cause,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum NovaCause {
    Feasible,
    PressureDeficit,
    PressureReachability,
}

#[derive(Debug, Clone, Serialize)]
pub struct NovaVerdict {
    pub feasible: bool,
    pub deficit_sinks: Vec<String>,
    pub cause: NovaCause,
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn verdict_infeasible_when_deficit_present() {
        let net = tiny_network();
        let scenario = scenario_with_lower_bound("T", 80.0, 120.0);
        let result = synthetic_result(50.0);
        let diag = compute_nova_diagnostics(&net, &scenario, &result);

        let verdict = nova_verdict(&diag, Some(1.0));
        assert!(!verdict.feasible);
        assert_eq!(verdict.cause, NovaCause::PressureReachability);
        assert_eq!(verdict.deficit_sinks, vec!["T".to_string()]);
    }

    #[test]
    fn verdict_feasible_when_no_deficit() {
        let net = tiny_network();
        let scenario = scenario_with_lower_bound("T", 40.0, 120.0);
        let result = synthetic_result(50.0); // 50 >= 40 → OK
        let diag = compute_nova_diagnostics(&net, &scenario, &result);

        let verdict = nova_verdict(&diag, Some(1.0));
        assert!(verdict.feasible);
        assert_eq!(verdict.cause, NovaCause::Feasible);
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
        let verdict = nova_verdict(&diag, Some(1.0));
        assert!(!verdict.feasible);
    }
}
