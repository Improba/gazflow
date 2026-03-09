//! Parseur pour les fichiers XML au format GasLib.
//!
//! Référence : <https://gaslib.zib.de/documentation.html>

mod parser;
mod scenario;

pub use parser::load_network;
pub use scenario::{ScenarioDemands, load_scenario_demands};
