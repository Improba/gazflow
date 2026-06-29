//! Parseur pour les fichiers XML au format GasLib.
//!
//! Référence : <https://gaslib.zib.de/documentation.html>

mod compressor;
mod parser;
mod scenario;
mod solution;

pub use parser::{load_network, load_network_raw};
pub use scenario::{
    PressureSlackHint, ScenarioDemands, apply_scenario_boundaries, demands_without_pressure_slack,
    load_scenario_demands,
};
pub use solution::{ReferenceSolution, load_reference_solution};
