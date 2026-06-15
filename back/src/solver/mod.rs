//! Solveur d'écoulement en régime permanent (steady-state).
//!
//! Algorithme : Newton-Raphson nodal.
//! Équations : Darcy-Weisbach pour la perte de charge.

pub mod capacity;
pub mod config;
pub mod contingency;
pub mod demand;
pub mod eos;
pub mod gas_properties;
pub(crate) mod iterative;
pub(crate) mod newton;
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
pub use demand::{ClientCategory, DemandProfile, normalize_daily_weights, resolve_demands};
pub use gas_properties::GasComposition;
pub use regulator::{EquipmentState, RegulatorMode};
pub use steady_state::{
    SolverControl, SolverProgress, SolverResult, solve_steady_state, solve_steady_state_jacobi,
    solve_steady_state_with_composition, solve_steady_state_with_initial_pressures,
    solve_steady_state_with_progress,
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
