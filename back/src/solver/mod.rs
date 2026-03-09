//! Solveur d'écoulement en régime permanent (steady-state).
//!
//! Algorithme : Newton-Raphson nodal.
//! Équations : Darcy-Weisbach pour la perte de charge.

mod steady_state;

pub use steady_state::{SolverResult, solve_steady_state};
