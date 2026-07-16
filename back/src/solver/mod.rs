//! Solveur d'écoulement en régime permanent (steady-state).
//!
//! Algorithme : Newton-Raphson nodal.
//! Équations : Darcy-Weisbach pour la perte de charge.

pub mod capacity;
pub(crate) mod compressor_loop;
pub(crate) mod control_valve_loop;
pub mod nova_capacity;
pub mod nova_diagnostics;
pub mod config;
pub mod contingency;
pub mod continuation;
pub mod demand;
pub mod eos;
pub mod gas_properties;
pub(crate) mod iterative;
pub(crate) mod newton;
#[cfg(feature = "nlp-ipopt")]
pub mod nlp_ipopt;
pub mod presets;
pub(crate) mod regulator;
mod steady_state;
pub mod timeseries;
pub mod transient;

pub use nova_capacity::{SinkCapacityReport, study_default_marginal_sinks, study_sinks_capacity};
pub use nova_diagnostics::{
    NovaCause, NovaDiagnostics, NovaSolverSignature, NovaVerdict, SinkDiagnostic, UpstreamHop,
    compute_nova_diagnostics, nova_verdict, NovaFeasibilityCause, NovaFeasibilityReport,
    NovaBoundViolation, nova_feasibility_report,
};
pub use capacity::{CapacityBounds, CapacityViolation, ConstrainedSolverResult};
pub use config::SteadyStateConfig;
pub use contingency::{
    ContingencyAction, ContingencyCase, ContingencyElementType, ContingencyReport,
    ContingencyResult, PressureViolation, apply_contingency, evaluate_contingency_case,
    finalize_contingency_report, generate_n_minus_1_cases, run_contingency_analysis,
};
pub use continuation::{
    ContinuationConfig, ContinuationStepEvent, solve_steady_state_with_continuation,
    solve_steady_state_with_preset,
};
pub use control_valve_loop::{
    ControlValveDecisionUpdate, ControlValveDecisionUpdateStats, ControlValveSinkDeficit,
    apply_control_valve_decision_updates, solve_with_control_valve_decision_loop,
};
pub use compressor_loop::{
    CompressorDecisionUpdate, CompressorDecisionUpdateStats, CompressorMapMode,
    DecisionSinkDeficit, RatioUpdateStats, apply_compressor_decision_updates,
    apply_map_ratios_after_continuation_step, compressor_accept_partial_enabled,
    compressor_map_mode, estimate_station_norm_flow, estimated_compressor_map_flow_m3s,
};
pub use demand::{ClientCategory, DemandProfile, normalize_daily_weights, resolve_demands};
pub use gas_properties::GasComposition;
pub use presets::{
    NetworkTier, SolverPreset, preset_for_node_count, preset_from_request, preset_robust,
    recommended_demo_for_dataset, tier_for_dataset, tier_for_node_count,
};
#[cfg(feature = "nlp-ipopt")]
pub use nlp_ipopt::{NovaIpoptOptions, NovaIpoptVerdict, solve_nova_with_ipopt};
pub use regulator::{EquipmentState, RegulatorMode};
pub use steady_state::{
    BoundaryNominationSlip, MassBalanceRefinementOutcome, MassBalanceReport, NodeMassImbalance,
    ScenarioPressureMargin, ScenarioPressureSlip, SolverControl, SolverProgress, SolverResult,
    boundary_nomination_slips, compressor_pressure_from_coeff, boundary_pressure_supply_reports,
    mass_balance_report, scenario_pressure_margins, scenario_pressure_slips, solve_steady_state,
    solve_steady_state_jacobi, solve_steady_state_with_composition,
    solve_steady_state_with_initial_pressures, solve_steady_state_with_progress,
    solve_with_mass_balance_refinement, upstream_pressure_trace, BoundaryPressureSupplyReport,
};
pub use timeseries::{
    TimeseriesConfig, TimeseriesControl, TimeseriesResult, TimeseriesStepResult, WeatherStep,
    simulate_timeseries, simulate_timeseries_with_progress,
};
pub use transient::{
    TransientConfig, TransientEvent, TransientMode, TransientResult, TransientStepResult,
    compute_linepack, simulate_transient, simulate_transient_pde, simulate_transient_quasi_steady,
    simulate_transient_with_mode,
};
