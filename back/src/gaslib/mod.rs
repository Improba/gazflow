//! Parseur pour les fichiers XML au format GasLib.
//!
//! Référence : <https://gaslib.zib.de/documentation.html>

mod compressor;
mod parser;
mod scenario;
mod solution;

pub use parser::load_network;
pub use scenario::{ScenarioDemands, load_scenario_demands};
pub use solution::{ReferenceSolution, load_reference_solution};
