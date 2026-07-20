//! Point d'entrée API pour la finalisation du verdict NoVa (Newton post-hoc, escalade IPOPT).

use std::collections::HashMap;

use crate::gaslib::ScenarioDemands;
use crate::graph::GasNetwork;
use crate::solver::{
    self, GasComposition, NovaCause, NovaDiagnostics, NovaSolverSignature, NovaVerdict,
    SolverResult,
};

/// Mode d'escalade IPOPT piloté par `GAZFLOW_NOVA_IPOPT_ESCALATION` (désactivé par défaut).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IpoptEscalationMode {
    Off,
    /// `1`, `true`, `on` — escalade si signature Unresolved.
    On,
    /// `on-notsolved` — escalade si signature Unresolved et cause `NotSolvedLocal`.
    OnNotSolved,
}

/// Lit le mode d'escalade depuis l'environnement. Off par défaut (IPOPT jamais par défaut).
pub(crate) fn ipopt_escalation_mode() -> IpoptEscalationMode {
    static IPOPT_ENV_WITHOUT_FEATURE: std::sync::Once = std::sync::Once::new();
    match std::env::var("GAZFLOW_NOVA_IPOPT_ESCALATION")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        None | Some("") => IpoptEscalationMode::Off,
        Some(v) if matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "on") => {
            #[cfg(not(feature = "nlp-ipopt"))]
            IPOPT_ENV_WITHOUT_FEATURE.call_once(|| {
                eprintln!(
                    "warning: GAZFLOW_NOVA_IPOPT_ESCALATION is set but binary was built without nlp-ipopt feature"
                );
            });
            IpoptEscalationMode::On
        }
        Some(v) if v.eq_ignore_ascii_case("on-notsolved") || v.eq_ignore_ascii_case("maybe") => {
            #[cfg(not(feature = "nlp-ipopt"))]
            IPOPT_ENV_WITHOUT_FEATURE.call_once(|| {
                eprintln!(
                    "warning: GAZFLOW_NOVA_IPOPT_ESCALATION is set but binary was built without nlp-ipopt feature"
                );
            });
            IpoptEscalationMode::OnNotSolved
        }
        Some(_) => IpoptEscalationMode::Off,
    }
}

fn should_attempt_ipopt_escalation(verdict: &NovaVerdict, mode: IpoptEscalationMode) -> bool {
    if verdict.solver_signature != NovaSolverSignature::Unresolved {
        return false;
    }
    match mode {
        IpoptEscalationMode::Off => false,
        IpoptEscalationMode::On => true,
        IpoptEscalationMode::OnNotSolved => verdict.cause == NovaCause::NotSolvedLocal,
    }
}

fn solver_result_from_ipopt_pressures(
    pressures_bar: HashMap<String, f64>,
    base: &SolverResult,
    residual_inf: f64,
    iterations: i32,
) -> SolverResult {
    SolverResult {
        pressures: pressures_bar,
        flows: base.flows.clone(),
        iterations: iterations.max(0) as usize,
        residual: residual_inf,
        equipment_states: base.equipment_states.clone(),
        warnings: base.warnings.clone(),
        demand_scale_achieved: base.demand_scale_achieved,
    }
}

/// Verdict NoVa pour une escalade IPOPT en `BoundViolation` (testable sans IPOPT réel).
fn ipopt_bound_violation_verdict(
    network: &GasNetwork,
    scenario: Option<&ScenarioDemands>,
    pressures_bar: HashMap<String, f64>,
    base: &NovaVerdict,
    base_result: &SolverResult,
    tol_m3s: f64,
    residual_inf: f64,
    iterations: i32,
) -> NovaVerdict {
    let ipopt_result = solver_result_from_ipopt_pressures(
        pressures_bar,
        base_result,
        residual_inf,
        iterations,
    );

    match scenario {
        Some(sc) => {
            let diagnostics = solver::compute_nova_diagnostics(network, sc, &ipopt_result);
            let converged = residual_inf <= tol_m3s;
            let mut verdict =
                solver::nova_verdict(&diagnostics, converged, tol_m3s, &ipopt_result);
            verdict.solver_signature = NovaSolverSignature::IpoptEscalation;
            verdict.iterations = iterations.max(0) as usize;
            verdict
        }
        None => NovaVerdict {
            feasible: false,
            deficit_sinks: Vec::new(),
            cause: NovaCause::PressureDeficit,
            converged: base.converged,
            demand_scale_achieved: base_result.demand_scale_achieved,
            residual_m3s: residual_inf,
            iterations: iterations.max(0) as usize,
            solver_signature: NovaSolverSignature::IpoptEscalation,
        },
    }
}

/// Dérive le verdict NoVa à partir des diagnostics et du résultat solveur local.
/// Peut tenter des redémarrages locaux (pressions scalées) puis escalader vers IPOPT.
pub(crate) fn finalize_nova_verdict(
    network: &GasNetwork,
    scenario: Option<&ScenarioDemands>,
    demands: &HashMap<String, f64>,
    gas: GasComposition,
    diagnostics: &NovaDiagnostics,
    converged: bool,
    tol_m3s: f64,
    result: &SolverResult,
) -> NovaVerdict {
    let mut verdict = solver::nova_verdict(diagnostics, converged, tol_m3s, result);
    let mut working = result.clone();

    if verdict.cause == NovaCause::NotSolvedLocal
        && let Some((improved_verdict, improved_result)) =
            try_local_pressure_restarts(network, scenario, demands, gas, tol_m3s, &working)
    {
        verdict = improved_verdict;
        working = improved_result;
    }

    let mode = ipopt_escalation_mode();
    if !should_attempt_ipopt_escalation(&verdict, mode) {
        return verdict;
    }

    #[cfg(feature = "nlp-ipopt")]
    {
        if let Some(escalated) =
            try_ipopt_escalation(network, scenario, demands, gas, &verdict, &working, tol_m3s)
        {
            return escalated;
        }
    }

    #[cfg(not(feature = "nlp-ipopt"))]
    {
        let _ = (network, scenario, demands, gas, &working);
        if mode != IpoptEscalationMode::Off {
            // Documente honnêtement que l'escalade a été demandée mais absente du binaire.
            static WARNED: std::sync::Once = std::sync::Once::new();
            WARNED.call_once(|| {
                eprintln!(
                    "warning: NoVa IPOPT escalation requested but nlp-ipopt feature is not enabled"
                );
            });
        }
    }

    verdict
}

/// Nombre de redémarrages locaux (pressions scalées) si `NotSolvedLocal`.
/// `GAZFLOW_NOVA_LOCAL_RESTARTS` : entier ≥ 0 (défaut **2**).
fn local_restart_count() -> usize {
    std::env::var("GAZFLOW_NOVA_LOCAL_RESTARTS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(2)
}

fn try_local_pressure_restarts(
    network: &GasNetwork,
    scenario: Option<&ScenarioDemands>,
    demands: &HashMap<String, f64>,
    gas: GasComposition,
    tol_m3s: f64,
    base: &SolverResult,
) -> Option<(NovaVerdict, SolverResult)> {
    let n_restarts = local_restart_count();
    if n_restarts == 0 || base.pressures.is_empty() {
        return None;
    }

    let scales: Vec<f64> = match n_restarts {
        1 => vec![0.92],
        2 => vec![0.90, 1.10],
        _ => {
            let mut s = vec![0.85, 0.92, 1.08, 1.15];
            s.truncate(n_restarts);
            s
        }
    };

    let cfg = solver::SteadyStateConfig {
        gas_composition: gas,
        max_iter: 800,
        tolerance: tol_m3s,
        ..solver::SteadyStateConfig::default()
    };

    for scale in scales {
        let mut init = HashMap::new();
        for (id, &p) in &base.pressures {
            init.insert(id.clone(), (p * scale).clamp(5.0, 150.0));
        }
        let Ok(candidate) = solver::solve_steady_state_with_progress(
            network,
            demands,
            Some(&init),
            cfg,
            |_| solver::SolverControl::Continue,
        ) else {
            continue;
        };
        if candidate.residual > tol_m3s {
            continue;
        }
        let Some(sc) = scenario else {
            continue;
        };
        let diagnostics = solver::compute_nova_diagnostics(network, sc, &candidate);
        let mut v = solver::nova_verdict(&diagnostics, true, tol_m3s, &candidate);
        if v.feasible || v.cause != NovaCause::NotSolvedLocal {
            v.solver_signature = NovaSolverSignature::NewtonPosthoc;
            return Some((v, candidate));
        }
    }
    None
}

#[cfg(feature = "nlp-ipopt")]
fn try_ipopt_escalation(
    network: &GasNetwork,
    scenario: Option<&ScenarioDemands>,
    demands: &HashMap<String, f64>,
    gas: GasComposition,
    base: &NovaVerdict,
    result: &SolverResult,
    tol_m3s: f64,
) -> Option<NovaVerdict> {
    use solver::{NovaIpoptOptions, NovaIpoptVerdict, solve_nova_with_ipopt};

    let starts: Vec<Option<HashMap<String, f64>>> = vec![
        Some(result.pressures.clone()),
        None, // cold uniform start inside IPOPT defaults
        {
            let mut uniform = HashMap::new();
            for id in result.pressures.keys() {
                uniform.insert(id.clone(), 70.0);
            }
            Some(uniform)
        },
    ];

    for initial in starts {
        let opts = NovaIpoptOptions {
            initial_pressures_bar: initial,
            ..NovaIpoptOptions::default()
        };
        let Ok(ipopt) = solve_nova_with_ipopt(network, demands, gas, &opts) else {
            continue;
        };
        match ipopt {
            NovaIpoptVerdict::Feasible {
                residual_inf,
                iterations,
                ..
            } => {
                return Some(NovaVerdict {
                    feasible: true,
                    deficit_sinks: Vec::new(),
                    cause: NovaCause::Feasible,
                    converged: true,
                    demand_scale_achieved: result.demand_scale_achieved,
                    residual_m3s: residual_inf,
                    iterations: iterations.max(0) as usize,
                    solver_signature: NovaSolverSignature::IpoptEscalation,
                });
            }
            NovaIpoptVerdict::BoundViolation {
                pressures_bar,
                residual_inf,
                iterations,
                ..
            } => {
                return Some(ipopt_bound_violation_verdict(
                    network,
                    scenario,
                    pressures_bar,
                    base,
                    result,
                    tol_m3s,
                    residual_inf,
                    iterations,
                ));
            }
            NovaIpoptVerdict::NotSolvedLocal { .. } | NovaIpoptVerdict::Error { .. } => continue,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn unresolved_verdict(cause: NovaCause) -> NovaVerdict {
        NovaVerdict {
            feasible: false,
            deficit_sinks: vec![],
            cause,
            converged: false,
            demand_scale_achieved: None,
            residual_m3s: 1.0,
            iterations: 10,
            solver_signature: NovaSolverSignature::Unresolved,
        }
    }

    #[test]
    #[serial]
    fn ipopt_escalation_mode_off_by_default() {
        unsafe { std::env::remove_var("GAZFLOW_NOVA_IPOPT_ESCALATION") };
        assert_eq!(ipopt_escalation_mode(), IpoptEscalationMode::Off);
    }

    #[test]
    #[serial]
    fn ipopt_escalation_mode_parses_enabled_values() {
        for value in ["1", "true", "on", "ON", "True"] {
            unsafe { std::env::set_var("GAZFLOW_NOVA_IPOPT_ESCALATION", value) };
            assert_eq!(
                ipopt_escalation_mode(),
                IpoptEscalationMode::On,
                "expected On for {value}"
            );
        }
        unsafe { std::env::set_var("GAZFLOW_NOVA_IPOPT_ESCALATION", "on-notsolved") };
        assert_eq!(ipopt_escalation_mode(), IpoptEscalationMode::OnNotSolved);
        unsafe { std::env::remove_var("GAZFLOW_NOVA_IPOPT_ESCALATION") };
    }

    #[test]
    #[serial]
    fn ipopt_escalation_mode_maybe_is_on_notsolved() {
        unsafe { std::env::set_var("GAZFLOW_NOVA_IPOPT_ESCALATION", "maybe") };
        assert_eq!(ipopt_escalation_mode(), IpoptEscalationMode::OnNotSolved);
        unsafe { std::env::remove_var("GAZFLOW_NOVA_IPOPT_ESCALATION") };
    }

    #[test]
    #[serial]
    fn ipopt_escalation_mode_unknown_value_is_off() {
        unsafe { std::env::set_var("GAZFLOW_NOVA_IPOPT_ESCALATION", "whatever") };
        assert_eq!(ipopt_escalation_mode(), IpoptEscalationMode::Off);
        unsafe { std::env::remove_var("GAZFLOW_NOVA_IPOPT_ESCALATION") };
    }

    #[test]
    fn should_not_escalate_when_mode_off() {
        let verdict = unresolved_verdict(NovaCause::NotSolvedLocal);
        assert!(!should_attempt_ipopt_escalation(
            &verdict,
            IpoptEscalationMode::Off
        ));
    }

    #[test]
    fn on_notsolved_only_escalates_not_solved_local() {
        let not_solved = unresolved_verdict(NovaCause::NotSolvedLocal);
        let deficit = unresolved_verdict(NovaCause::PressureDeficit);
        assert!(should_attempt_ipopt_escalation(
            &not_solved,
            IpoptEscalationMode::OnNotSolved
        ));
        assert!(!should_attempt_ipopt_escalation(
            &deficit,
            IpoptEscalationMode::OnNotSolved
        ));
    }

    #[test]
    fn on_escalates_any_unresolved() {
        let verdict = unresolved_verdict(NovaCause::PressureDeficit);
        assert!(should_attempt_ipopt_escalation(&verdict, IpoptEscalationMode::On));
    }

    #[test]
    fn never_escalate_when_signature_not_unresolved() {
        let mut verdict = unresolved_verdict(NovaCause::NotSolvedLocal);
        verdict.solver_signature = NovaSolverSignature::NewtonPosthoc;
        assert!(!should_attempt_ipopt_escalation(&verdict, IpoptEscalationMode::On));
    }

    #[test]
    #[serial]
    fn finalize_skips_escalation_when_mode_off() {
        unsafe { std::env::remove_var("GAZFLOW_NOVA_IPOPT_ESCALATION") };
        let network = GasNetwork::new();
        let demands = HashMap::new();
        let gas = GasComposition::default();
        let diagnostics = NovaDiagnostics::default();
        let result = SolverResult::from_core(HashMap::new(), HashMap::new(), 5, 1.0);
        let verdict = finalize_nova_verdict(
            &network,
            None,
            &demands,
            gas,
            &diagnostics,
            false,
            1e-3,
            &result,
        );
        assert_eq!(verdict.solver_signature, NovaSolverSignature::Unresolved);
        assert_eq!(verdict.cause, NovaCause::NotSolvedLocal);
    }

    #[test]
    fn ipopt_bound_violation_without_scenario_is_pressure_deficit_no_deficits() {
        let network = GasNetwork::new();
        let base = unresolved_verdict(NovaCause::NotSolvedLocal);
        let result = SolverResult::from_core(HashMap::new(), HashMap::new(), 5, 1.0);
        let pressures = HashMap::from([("n1".to_string(), 10.0)]);
        let verdict = ipopt_bound_violation_verdict(
            &network,
            None,
            pressures,
            &base,
            &result,
            1e-3,
            1e-6,
            42,
        );
        assert_eq!(verdict.solver_signature, NovaSolverSignature::IpoptEscalation);
        assert_eq!(verdict.cause, NovaCause::PressureDeficit);
        assert!(verdict.deficit_sinks.is_empty());
        assert_eq!(verdict.iterations, 42);
    }

    #[test]
    fn ipopt_bound_violation_with_scenario_uses_ipopt_pressures() {
        use crate::gaslib::ScenarioDemands;
        use crate::graph::{ConnectionKind, EquipmentSpec, GasNetwork, Node, Pipe};

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
        let scenario = ScenarioDemands {
            scenario_id: None,
            demands: HashMap::from([("T".to_string(), -3.0)]),
            pressure_slack: None,
            balance_hubs: Vec::new(),
            junction_anchors: Vec::new(),
            boundary_spine_anchors: Vec::new(),
            mass_balance_anchors: Vec::new(),
            zero_flow_boundary_anchors: Vec::new(),
            contract_flow_relaxed: Vec::new(),
            contract_pressure_anchors: Vec::new(),
            pressure_envelopes: vec![crate::gaslib::ScenarioPressureEnvelope {
                node_id: "T".to_string(),
                lower_bar: Some(80.0),
                upper_bar: Some(120.0),
            }],
        };
        let newton_result = SolverResult::from_core(
            HashMap::from([("S".to_string(), 70.0), ("T".to_string(), 75.0)]),
            HashMap::new(),
            5,
            1.0,
        );
        let base = unresolved_verdict(NovaCause::NotSolvedLocal);
        let ipopt_pressures = HashMap::from([
            ("S".to_string(), 70.0),
            ("T".to_string(), 50.0),
        ]);
        let verdict = ipopt_bound_violation_verdict(
            &net,
            Some(&scenario),
            ipopt_pressures,
            &base,
            &newton_result,
            1e-3,
            1e-6,
            17,
        );
        assert_eq!(verdict.solver_signature, NovaSolverSignature::IpoptEscalation);
        assert!(!verdict.feasible);
        assert!(verdict.deficit_sinks.contains(&"T".to_string()));
        assert_eq!(verdict.iterations, 17);
    }
}
