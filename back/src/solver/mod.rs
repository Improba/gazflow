//! Solveur d'écoulement en régime permanent (steady-state).
//!
//! Algorithme : Newton-Raphson nodal.
//! Équations : Darcy-Weisbach pour la perte de charge.

pub mod capacity;
pub(crate) mod compressor_loop;
pub mod config;
pub mod contingency;
pub mod continuation;
pub mod demand;
pub mod eos;
pub mod gas_properties;
pub(crate) mod iterative;
pub(crate) mod newton;
pub mod presets;
pub(crate) mod regulator;
mod steady_state;
pub mod timeseries;
pub mod transient;

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
pub use compressor_loop::{
    apply_map_ratios_after_continuation_step, compressor_accept_partial_enabled,
    estimate_station_norm_flow, estimated_compressor_map_flow_m3s, CompressorMapMode,
    RatioUpdateStats, compressor_map_mode,
};
pub use demand::{ClientCategory, DemandProfile, normalize_daily_weights, resolve_demands};
pub use gas_properties::GasComposition;
pub use presets::{
    NetworkTier, SolverPreset, preset_for_node_count, preset_from_request, preset_robust,
    recommended_demo_for_dataset, tier_for_dataset, tier_for_node_count,
};
pub use regulator::{EquipmentState, RegulatorMode};
pub use steady_state::{
    BoundaryNominationSlip, MassBalanceRefinementOutcome, MassBalanceReport, NodeMassImbalance,
    ScenarioPressureSlip, SolverControl, SolverProgress, SolverResult, boundary_nomination_slips,
    compressor_pressure_from_coeff, mass_balance_report, scenario_pressure_slips, solve_steady_state,
    solve_steady_state_jacobi, solve_steady_state_with_composition,
    solve_steady_state_with_initial_pressures, solve_steady_state_with_progress,
    solve_with_mass_balance_refinement, upstream_pressure_trace,
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
