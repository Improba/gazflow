//! Solveur d'écoulement en régime permanent (steady-state).
//!
//! Algorithme : Newton-Raphson nodal.
//! Équations : Darcy-Weisbach pour la perte de charge.

pub(crate) mod gas_properties;
pub(crate) mod iterative;
pub(crate) mod newton;
mod steady_state;

pub use steady_state::{
    SolverControl, SolverProgress, SolverResult, solve_steady_state, solve_steady_state_jacobi,
    solve_steady_state_with_initial_pressures, solve_steady_state_with_progress,
};
