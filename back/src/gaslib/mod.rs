//! Parseur pour les fichiers XML au format GasLib.
//!
//! Référence : <https://gaslib.zib.de/documentation.html>

mod cdf;
mod connectivity;
mod parser;
mod routing;
mod scenario;
mod solution;

pub use cdf::{
    CombinedDecisions, apply_cdf_decision_ids, apply_cdf_decisions, cdf_path_for_network,
    load_combined_decisions,
};
pub use crate::compressor::{CompressorCatalog, load_compressor_catalog, load_compressor_ratios};
pub use parser::{load_network, load_network_raw};
pub use routing::{
    CdfRoutingConfig, CdfRoutingOutcome, ResolvedCdfRouting, apply_cdf_routing_by_id,
    resolve_and_apply_cdf_routing,
};
pub use scenario::{
    PressureSlackHint, ScenarioDemands, ZeroFlowBoundaryAnchor, apply_scenario_boundaries,
    demands_without_pressure_slack, enrich_scenario_with_balance_hub, load_scenario_demands,
};
pub use solution::{ReferenceSolution, load_reference_solution};
