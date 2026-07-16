//! Point d'entrée API pour la finalisation du verdict NoVa (Newton post-hoc, escalade IPOPT).

use std::collections::HashMap;

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
        Some(v) if v.eq_ignore_ascii_case("on-notsolved") => {
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

fn ipopt_bound_violation_cause(diagnostics: &NovaDiagnostics) -> NovaCause {
    let has_deficit = diagnostics
        .pressure_slips
        .iter()
        .any(|s| s.shortfall_bar > 0.0);
    let has_excess = diagnostics
        .pressure_slips
        .iter()
        .any(|s| s.excess_bar > 0.0);
    if has_deficit {
        NovaCause::PressureDeficit
    } else if has_excess {
        NovaCause::PressureExcess
    } else {
        NovaCause::PressureDeficit
    }
}

/// Dérive le verdict NoVa à partir des diagnostics et du résultat solveur local.
/// Peut escalader vers IPOPT si activé déclarativement et signature `Unresolved`.
pub(crate) fn finalize_nova_verdict(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    gas: GasComposition,
    diagnostics: &NovaDiagnostics,
    converged: bool,
    tol_m3s: f64,
    result: &SolverResult,
) -> NovaVerdict {
    let verdict = solver::nova_verdict(diagnostics, converged, tol_m3s, result);

    let mode = ipopt_escalation_mode();
    if !should_attempt_ipopt_escalation(&verdict, mode) {
        return verdict;
    }

    #[cfg(feature = "nlp-ipopt")]
    {
        if let Some(escalated) =
            try_ipopt_escalation(network, demands, gas, diagnostics, &verdict, result)
        {
            return escalated;
        }
    }

    verdict
}

#[cfg(feature = "nlp-ipopt")]
fn try_ipopt_escalation(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    gas: GasComposition,
    diagnostics: &NovaDiagnostics,
    base: &NovaVerdict,
    result: &SolverResult,
) -> Option<NovaVerdict> {
    use solver::{NovaIpoptOptions, NovaIpoptVerdict, solve_nova_with_ipopt};

    let opts = NovaIpoptOptions {
        initial_pressures_bar: Some(result.pressures.clone()),
        ..NovaIpoptOptions::default()
    };

    let ipopt = solve_nova_with_ipopt(network, demands, gas, &opts).ok()?;

    match ipopt {
        NovaIpoptVerdict::Feasible {
            residual_inf,
            iterations,
            ..
        } => Some(NovaVerdict {
            feasible: true,
            deficit_sinks: Vec::new(),
            cause: NovaCause::Feasible,
            converged: true,
            demand_scale_achieved: result.demand_scale_achieved,
            residual_m3s: residual_inf,
            iterations: iterations.max(0) as usize,
            solver_signature: NovaSolverSignature::IpoptEscalation,
        }),
        NovaIpoptVerdict::BoundViolation {
            residual_inf,
            iterations,
            ..
        } => {
            let deficit_sinks: Vec<String> = diagnostics
                .pressure_slips
                .iter()
                .filter(|s| s.shortfall_bar > 0.0)
                .map(|s| s.node_id.clone())
                .collect();
            Some(NovaVerdict {
                feasible: false,
                deficit_sinks,
                cause: ipopt_bound_violation_cause(diagnostics),
                converged: base.converged,
                demand_scale_achieved: result.demand_scale_achieved,
                residual_m3s: residual_inf,
                iterations: iterations.max(0) as usize,
                solver_signature: NovaSolverSignature::IpoptEscalation,
            })
        }
        NovaIpoptVerdict::NotSolvedLocal { .. } | NovaIpoptVerdict::Error { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::ScenarioPressureSlip;
    use serial_test::serial;

    fn slip(
        node_id: &str,
        shortfall_bar: f64,
        excess_bar: f64,
    ) -> ScenarioPressureSlip {
        ScenarioPressureSlip {
            node_id: node_id.into(),
            solved_pressure_bar: 40.0,
            lower_bar: None,
            upper_bar: None,
            shortfall_bar,
            excess_bar,
            from_scenario_envelope: false,
            shortpipe_partner_id: None,
        }
    }

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
    fn ipopt_escalation_mode_unknown_value_is_off() {
        unsafe { std::env::set_var("GAZFLOW_NOVA_IPOPT_ESCALATION", "maybe") };
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
    fn bound_violation_cause_prefers_deficit_then_excess() {
        let mixed = NovaDiagnostics {
            pressure_slips: vec![slip("sink_a", 10.0, 0.0), slip("node_b", 0.0, 10.0)],
            ..Default::default()
        };
        assert_eq!(
            ipopt_bound_violation_cause(&mixed),
            NovaCause::PressureDeficit
        );

        let excess_only = NovaDiagnostics {
            pressure_slips: vec![slip("node_b", 0.0, 10.0)],
            ..Default::default()
        };
        assert_eq!(
            ipopt_bound_violation_cause(&excess_only),
            NovaCause::PressureExcess
        );
    }
}
