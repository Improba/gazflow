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
    PressureSlackHint, ScenarioDemands, ScenarioPressureEnvelope, ShortPipeBoundaryPair,
    ZeroFlowBoundaryAnchor, apply_scenario_boundaries, apply_scenario_pressure_envelopes,
    contract_boundary_refinement_enabled, demands_without_pressure_slack,
    detect_shortpipe_boundary_pairs, effective_solver_demands,
    effective_solver_demands_for_network, enrich_scenario_with_balance_hub,
    load_scenario_demands, network_with_scenario_boundaries, prepare_transport_scenario,
    scenario_pressure_envelopes_enabled, scenario_pressure_floor_anchor_enabled,
    scenario_boundary_active_envelopes_enabled, scenario_boundary_partial_accept_enabled,
    scenario_pressure_clamp_in_newton_enabled, scenario_pressure_in_newton_enabled,
    scenario_pressure_penalty_weight, shortpipe_coupled_envelopes_enabled,
    shortpipe_merge_boundaries_enabled, shortpipe_partner_for,
    transport_minimal_anchors_enabled, try_add_mass_balance_anchor, try_relax_contract_boundary,
};
pub use solution::{ReferenceSolution, load_reference_solution};
