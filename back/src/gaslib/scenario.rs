use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use quick_xml::de::from_str;
use serde::{Deserialize, Serialize};

use crate::graph::{ConnectionKind, GasNetwork};

#[derive(Debug, Deserialize)]
#[serde(rename = "boundaryValue")]
struct XmlBoundaryValue {
    #[serde(alias = "scenario")]
    scenario: XmlScenario,
}

#[derive(Debug, Deserialize)]
struct XmlScenario {
    #[serde(rename = "@id", default)]
    id: Option<String>,
    #[serde(rename = "node", default)]
    nodes: Vec<XmlScenarioNode>,
}

#[derive(Debug, Deserialize)]
struct XmlScenarioNode {
    #[serde(rename = "@id")]
    id: String,
    #[serde(rename = "@type", default)]
    node_type: Option<String>,
    #[serde(rename = "flow", default)]
    flows: Vec<XmlFlowBound>,
    #[serde(rename = "pressure", default)]
    pressures: Vec<XmlPressureBound>,
}

#[derive(Debug, Deserialize)]
struct XmlFlowBound {
    #[serde(rename = "@bound", default)]
    bound: Option<String>,
    #[serde(rename = "@value")]
    value: f64,
    #[serde(rename = "@unit", default)]
    unit: Option<String>,
}

#[derive(Debug, Deserialize)]
struct XmlPressureBound {
    #[serde(rename = "@bound", default)]
    bound: Option<String>,
    #[serde(rename = "@value")]
    value: f64,
    #[serde(rename = "@unit", default)]
    unit: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PressureSlackHint {
    pub node_id: String,
    pub pressure_bar: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ZeroFlowBoundaryAnchor {
    pub node_id: String,
    /// Borne basse scénario ; complétée depuis le `.net` à l'enrichissement si absente.
    pub scenario_pressure_bar: Option<f64>,
}

/// Enveloppe pression scénario (entry/exit à Q nominé) — inégalités GasLib, pas égalités Newton.
#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioPressureEnvelope {
    pub node_id: String,
    pub lower_bar: Option<f64>,
    pub upper_bar: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioDemands {
    pub scenario_id: Option<String>,
    pub demands: HashMap<String, f64>,
    /// Slack pression implicite (ex. sortie principale avec borne basse seule).
    pub pressure_slack: Option<PressureSlackHint>,
    /// Hubs de balance transport (boundaries Q≈0 les plus connectés).
    pub balance_hubs: Vec<PressureSlackHint>,
    /// Junctions internes fortement couplées à des boundaries Q≈0.
    pub junction_anchors: Vec<PressureSlackHint>,
    /// Boundaries Q≈0 très connectées (source/sink), hors hubs balance.
    pub boundary_spine_anchors: Vec<PressureSlackHint>,
    /// Ancrages ajoutés itérativement depuis le bilan massique post-solve.
    pub mass_balance_anchors: Vec<PressureSlackHint>,
    /// Boundaries nominalement à Q=0 (entries/exits).
    pub zero_flow_boundary_anchors: Vec<ZeroFlowBoundaryAnchor>,
    /// Entries/exits dont le débit nominatif est retiré avant solve (abandon partiel de nomination Q, v18).
    pub contract_flow_relaxed: Vec<String>,
    /// Pression fixée lors de l'abandon Q v18 (typ. pression résolue partielle).
    pub contract_pressure_anchors: Vec<PressureSlackHint>,
    /// Enveloppes pression `.scn` sur boundaries à Q nominé (hors slack).
    pub pressure_envelopes: Vec<ScenarioPressureEnvelope>,
}

/// Applique les enveloppes pression scénario au réseau (intersection avec bornes `.net`).
pub fn apply_scenario_pressure_envelopes(
    network: &mut GasNetwork,
    scenario: &ScenarioDemands,
) {
    let reserved = all_applied_anchor_ids(scenario);
    for envelope in &scenario.pressure_envelopes {
        if reserved.contains(&envelope.node_id) {
            continue;
        }
        let Some(node) = network.node_mut(&envelope.node_id) else {
            continue;
        };
        if node.pressure_fixed_bar.is_some() {
            continue;
        }
        node.pressure_lower_bar =
            merge_pressure_bound(node.pressure_lower_bar, envelope.lower_bar, true);
        node.pressure_upper_bar =
            merge_pressure_bound(node.pressure_upper_bar, envelope.upper_bar, false);
        network
            .scenario_pressure_envelope_nodes
            .insert(envelope.node_id.clone());
    }
    apply_shortpipe_coupled_envelopes(network, scenario);
    apply_scenario_pressure_floor_anchors(network, scenario);
}

/// Paire entry/exit GasLib au même point physique (liaison `shortPipe`).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ShortPipeBoundaryPair {
    pub shortpipe_id: String,
    pub sink_id: String,
    pub source_id: String,
}

/// Détecte les couples `sink_*` ↔ `source_*` reliés par un `shortPipe`.
pub fn detect_shortpipe_boundary_pairs(network: &GasNetwork) -> Vec<ShortPipeBoundaryPair> {
    let mut pairs = Vec::new();
    for pipe in network.pipes() {
        if pipe.kind != ConnectionKind::ShortPipe {
            continue;
        }
        let (sink_id, source_id) = match (pipe.from.as_str(), pipe.to.as_str()) {
            (from, to) if from.starts_with("sink_") && to.starts_with("source_") => {
                (from.to_string(), to.to_string())
            }
            (from, to) if from.starts_with("source_") && to.starts_with("sink_") => {
                (to.to_string(), from.to_string())
            }
            _ => continue,
        };
        pairs.push(ShortPipeBoundaryPair {
            shortpipe_id: pipe.id.clone(),
            sink_id,
            source_id,
        });
    }
    pairs.sort_by(|a, b| a.sink_id.cmp(&b.sink_id).then_with(|| a.source_id.cmp(&b.source_id)));
    pairs
}

pub fn shortpipe_coupled_envelopes_enabled() -> bool {
    std::env::var("GAZFLOW_SCENARIO_SHORTPIPE_COUPLED_ENVELOPES")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Propage l'enveloppe P du `sink_*` nominé vers le `source_*` couplé (même point physique).
fn apply_shortpipe_coupled_envelopes(network: &mut GasNetwork, scenario: &ScenarioDemands) {
    if !shortpipe_coupled_envelopes_enabled() || !scenario_pressure_envelopes_enabled() {
        return;
    }
    let pairs = detect_shortpipe_boundary_pairs(network);
    let env_map: HashMap<&str, &ScenarioPressureEnvelope> = scenario
        .pressure_envelopes
        .iter()
        .map(|e| (e.node_id.as_str(), e))
        .collect();
    let reserved = all_applied_anchor_ids(scenario);
    for pair in pairs {
        let Some(env) = env_map.get(pair.sink_id.as_str()) else {
            continue;
        };
        if reserved.contains(&pair.source_id) {
            continue;
        }
        let Some(node) = network.node_mut(&pair.source_id) else {
            continue;
        };
        if node.pressure_fixed_bar.is_some() {
            continue;
        }
        node.pressure_lower_bar =
            merge_pressure_bound(node.pressure_lower_bar, env.lower_bar, true);
        node.pressure_upper_bar =
            merge_pressure_bound(node.pressure_upper_bar, env.upper_bar, false);
        network
            .scenario_pressure_envelope_nodes
            .insert(pair.source_id.clone());
    }
}

pub fn scenario_pressure_floor_anchor_enabled() -> bool {
    std::env::var("GAZFLOW_SCENARIO_PRESSURE_FLOOR_ANCHOR")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Ancrage P à la borne basse scénario (égalité — change le problème, bench opt-in Phase I-c).
fn apply_scenario_pressure_floor_anchors(network: &mut GasNetwork, scenario: &ScenarioDemands) {
    if !scenario_pressure_floor_anchor_enabled() || !scenario_pressure_envelopes_enabled() {
        return;
    }
    let reserved = all_applied_anchor_ids(scenario);
    for envelope in &scenario.pressure_envelopes {
        if reserved.contains(&envelope.node_id) {
            continue;
        }
        let Some(lower) = envelope.lower_bar else {
            continue;
        };
        let Some(node) = network.node_mut(&envelope.node_id) else {
            continue;
        };
        if node.pressure_fixed_bar.is_none() {
            node.pressure_fixed_bar = Some(lower);
        }
    }
}

pub fn shortpipe_partner_for(network: &GasNetwork, node_id: &str) -> Option<String> {
    detect_shortpipe_boundary_pairs(network)
        .into_iter()
        .find_map(|p| {
            if p.sink_id == node_id {
                Some(p.source_id)
            } else if p.source_id == node_id {
                Some(p.sink_id)
            } else {
                None
            }
        })
}

pub fn scenario_pressure_envelopes_enabled() -> bool {
    std::env::var("GAZFLOW_SCENARIO_PRESSURE_ENVELOPES")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Clamp / pénalité pression dans le Newton (nécessite [`scenario_pressure_envelopes_enabled`]).
pub fn scenario_pressure_in_newton_enabled() -> bool {
    std::env::var("GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Clamp pression en line-search Newton (avec [`scenario_pressure_in_newton_enabled`]).
pub fn scenario_pressure_clamp_in_newton_enabled() -> bool {
    std::env::var("GAZFLOW_SCENARIO_PRESSURE_CLAMP")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub fn transport_minimal_anchors_enabled() -> bool {
    std::env::var("GAZFLOW_TRANSPORT_MINIMAL_ANCHORS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Poids pénalité enveloppe pression dans le résidu Newton (m³/s par bar).
pub fn scenario_pressure_penalty_weight() -> f64 {
    std::env::var("GAZFLOW_SCENARIO_PRESSURE_PENALTY_WEIGHT")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|w: &f64| w.is_finite() && *w > 0.0)
        .unwrap_or(0.01)
}

fn merge_pressure_bound(
    existing: Option<f64>,
    incoming: Option<f64>,
    take_max: bool,
) -> Option<f64> {
    match (existing, incoming) {
        (None, inc) => inc,
        (Some(ex), None) => Some(ex),
        (Some(ex), Some(inc)) => Some(if take_max { ex.max(inc) } else { ex.min(inc) }),
    }
}

/// Applique les conditions aux limites du scénario au réseau (slack pression + hub balance).
pub fn apply_scenario_boundaries(network: &mut GasNetwork, scenario: &ScenarioDemands) {
    if !network.nodes().any(|n| n.pressure_fixed_bar.is_some()) {
        if let Some(slack) = scenario.pressure_slack.as_ref() {
            if let Some(node) = network.node_mut(&slack.node_id) {
                node.pressure_fixed_bar = Some(slack.pressure_bar);
            }
        }
    }
    for hub in &scenario.balance_hubs {
        if let Some(node) = network.node_mut(&hub.node_id) {
            if node.pressure_fixed_bar.is_none() {
                node.pressure_fixed_bar = Some(hub.pressure_bar);
            }
        }
    }
    for anchor in &scenario.boundary_spine_anchors {
        if let Some(node) = network.node_mut(&anchor.node_id) {
            if node.pressure_fixed_bar.is_none() {
                node.pressure_fixed_bar = Some(anchor.pressure_bar);
            }
        }
    }
    for anchor in &scenario.junction_anchors {
        if let Some(node) = network.node_mut(&anchor.node_id) {
            if node.pressure_fixed_bar.is_none() {
                node.pressure_fixed_bar = Some(anchor.pressure_bar);
            }
        }
    }
    for anchor in &scenario.mass_balance_anchors {
        if let Some(node) = network.node_mut(&anchor.node_id) {
            if node.pressure_fixed_bar.is_none() {
                node.pressure_fixed_bar = Some(anchor.pressure_bar);
            }
        }
    }
    for anchor in &scenario.contract_pressure_anchors {
        if let Some(node) = network.node_mut(&anchor.node_id) {
            if node.pressure_fixed_bar.is_none() {
                node.pressure_fixed_bar = Some(anchor.pressure_bar);
            }
        }
    }
}

/// Active l'abandon itératif de Q nominatif sur boundaries (v18, **opt-in** bench).
pub fn contract_boundary_refinement_enabled() -> bool {
    std::env::var("GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Enrichit le scénario transport et applique les ancrages pression statiques au réseau.
///
/// N'inclut pas le refinement itératif (`solve_with_mass_balance_refinement`) : réservé à
/// `compressor_diag` et aux benches explicites.
pub fn prepare_transport_scenario(
    base: &GasNetwork,
    scenario: &mut ScenarioDemands,
) -> GasNetwork {
    enrich_scenario_with_balance_hub(base, scenario);
    network_with_scenario_boundaries(base, scenario)
}

/// Retire la demande imposée sur le nœud slack pression et les boundaries dont la nomination Q a été abandonnée (v18).
///
/// **Slack pression** (`sink_109` mild_618) : en transport GasLib, P est fixée (référence) et Q
/// est une inconnue du solveur — imposer les deux serait sur-contraint. Le Q nominal du `.scn`
/// est donc retiré.
///
/// **Entries/exits à Q nominé** : en métier, Q imposé + enveloppe pression (inégalité) est
/// standard ; ce n'est pas sur-contraint. Le MVP GazFlow impose Q en égalité et laisse P libre
/// (bornes `.net` vérifiées a posteriori). v18 retire optionnellement Q — cela **viole** la nomination.
pub fn effective_solver_demands(
    demands: &HashMap<String, f64>,
    scenario: &ScenarioDemands,
) -> HashMap<String, f64> {
    let mut adjusted = demands.clone();
    if let Some(slack) = scenario.pressure_slack.as_ref() {
        adjusted.remove(&slack.node_id);
    }
    for node_id in &scenario.contract_flow_relaxed {
        adjusted.remove(node_id);
    }
    adjusted
}

/// Alias historique — préférer [`effective_solver_demands`].
pub fn demands_without_pressure_slack(
    demands: &HashMap<String, f64>,
    scenario: &ScenarioDemands,
) -> HashMap<String, f64> {
    effective_solver_demands(demands, scenario)
}

/// Charge un fichier GasLib `.scn` et retourne les demandes nodales.
///
/// Convention de signe utilisée:
/// - `entry` -> demande positive (injection)
/// - `exit` -> demande négative (consommation)
pub fn load_scenario_demands<P: AsRef<Path>>(path: P) -> Result<ScenarioDemands> {
    let xml = std::fs::read_to_string(path.as_ref())
        .with_context(|| format!("lecture de {:?}", path.as_ref()))?;
    parse_scenario_demands_from_str(&xml)
}

fn parse_scenario_demands_from_str(xml: &str) -> Result<ScenarioDemands> {
    let raw: XmlBoundaryValue =
        from_str(xml).with_context(|| "parsing XML GasLib scenario (.scn)")?;

    let nodes = &raw.scenario.nodes;
    let demands = nodes
        .iter()
        .filter_map(|node| {
            let magnitude = extract_flow_value(&node.flows)?;
            let sign = match node.node_type.as_deref() {
                Some("entry") => 1.0,
                Some("exit") => -1.0,
                _ => 1.0,
            };
            Some((node.id.clone(), sign * magnitude))
        })
        .collect();

    let pressure_slack = detect_pressure_slack(nodes);
    let slack_id_owned = pressure_slack.as_ref().map(|s| s.node_id.clone());
    let pressure_envelopes =
        collect_scenario_pressure_envelopes(nodes, slack_id_owned.as_deref());

    Ok(ScenarioDemands {
        scenario_id: raw.scenario.id,
        demands,
        pressure_slack,
        balance_hubs: Vec::new(),
        junction_anchors: Vec::new(),
        boundary_spine_anchors: Vec::new(),
        mass_balance_anchors: Vec::new(),
        zero_flow_boundary_anchors: collect_zero_flow_boundary_anchors(nodes),
        contract_flow_relaxed: initial_contract_flow_relaxed(nodes),
        contract_pressure_anchors: Vec::new(),
        pressure_envelopes,
    })
}

fn extract_scenario_pressure_bounds(node: &XmlScenarioNode) -> (Option<f64>, Option<f64>) {
    let mut lower = None;
    let mut upper = None;
    for p in &node.pressures {
        let abs = pressure_to_bar_absolute(p.value, p.unit.as_deref());
        match p.bound.as_deref() {
            Some("lower") => lower = Some(abs),
            Some("upper") => upper = Some(abs),
            Some("both") => {
                lower = Some(abs);
                upper = Some(abs);
            }
            _ => {}
        }
    }
    (lower, upper)
}

fn collect_scenario_pressure_envelopes(
    nodes: &[XmlScenarioNode],
    slack_id: Option<&str>,
) -> Vec<ScenarioPressureEnvelope> {
    nodes
        .iter()
        .filter_map(|node| {
            let node_type = node.node_type.as_deref();
            if node_type != Some("entry") && node_type != Some("exit") {
                return None;
            }
            if slack_id == Some(node.id.as_str()) {
                return None;
            }
            let flow_mag = extract_flow_value(&node.flows).unwrap_or(0.0).abs();
            if flow_mag < 1e-6 {
                return None;
            }
            let (lower_bar, upper_bar) = extract_scenario_pressure_bounds(node);
            if lower_bar.is_none() && upper_bar.is_none() {
                return None;
            }
            Some(ScenarioPressureEnvelope {
                node_id: node.id.clone(),
                lower_bar,
                upper_bar,
            })
        })
        .collect()
}

fn dual_pressure_contract_relaxation_enabled() -> bool {
    std::env::var("GAZFLOW_RELAX_DUAL_PRESSURE_CONTRACTS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Entries/exits avec Q nominé et enveloppe pression scénario (lower+upper).
/// Nom historique « dual pressure » : ce n'est pas une sur-contrainte P+Q au solveur
/// (enveloppe P = inégalité GasLib, non imposée au Newton dans le MVP).
fn detect_dual_pressure_contract_flow_nodes(
    nodes: &[XmlScenarioNode],
    slack_id: Option<&str>,
) -> Vec<String> {
    nodes
        .iter()
        .filter_map(|node| {
            let node_type = node.node_type.as_deref();
            if node_type != Some("entry") && node_type != Some("exit") {
                return None;
            }
            let flow_mag = extract_flow_value(&node.flows).unwrap_or(0.0).abs();
            if flow_mag < 1e-6 {
                return None;
            }
            if slack_id == Some(node.id.as_str()) {
                return None;
            }
            let mut has_lower = false;
            let mut has_upper = false;
            for p in &node.pressures {
                match p.bound.as_deref() {
                    Some("lower") => has_lower = true,
                    Some("upper") => has_upper = true,
                    Some("both") => {
                        has_lower = true;
                        has_upper = true;
                    }
                    _ => {}
                }
            }
            if has_lower && has_upper {
                Some(node.id.clone())
            } else {
                None
            }
        })
        .collect()
}

fn initial_contract_flow_relaxed(nodes: &[XmlScenarioNode]) -> Vec<String> {
    if !dual_pressure_contract_relaxation_enabled() {
        return Vec::new();
    }
    let slack_id = detect_pressure_slack(nodes).map(|s| s.node_id);
    detect_dual_pressure_contract_flow_nodes(nodes, slack_id.as_deref())
}

fn collect_zero_flow_boundary_anchors(nodes: &[XmlScenarioNode]) -> Vec<ZeroFlowBoundaryAnchor> {
    let mut anchors = Vec::new();
    for node in nodes {
        let node_type = node.node_type.as_deref();
        if node_type != Some("exit") && node_type != Some("entry") {
            continue;
        }
        let flow_mag = extract_flow_value(&node.flows).unwrap_or(0.0).abs();
        if flow_mag > 1e-6 {
            continue;
        }
        let mut lower: Option<f64> = None;
        for p in &node.pressures {
            if matches!(p.bound.as_deref(), Some("lower") | Some("both")) {
                lower = Some(pressure_to_bar_absolute(p.value, p.unit.as_deref()));
            }
        }
        anchors.push(ZeroFlowBoundaryAnchor {
            node_id: node.id.clone(),
            scenario_pressure_bar: lower,
        });
    }
    anchors
}

fn resolve_anchor_pressure_bar(
    network: &GasNetwork,
    node_id: &str,
    scenario_lower: Option<f64>,
) -> Option<f64> {
    if let Some(p) = scenario_lower {
        return Some(p);
    }
    network
        .nodes()
        .find(|n| n.id == node_id)
        .and_then(|n| n.pressure_lower_bar)
}

fn zero_flow_boundary_kind_sets(
    scenario: &ScenarioDemands,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
) {
    let mut entries = std::collections::HashSet::new();
    let mut exits = std::collections::HashSet::new();
    for anchor in &scenario.zero_flow_boundary_anchors {
        if anchor.node_id.starts_with("source_") {
            entries.insert(anchor.node_id.clone());
        } else if anchor.node_id.starts_with("sink_") {
            exits.insert(anchor.node_id.clone());
        }
    }
    (entries, exits)
}

fn count_mixed_zero_flow_neighbors(
    network: &GasNetwork,
    node_id: &str,
    zero_flow_entries: &std::collections::HashSet<String>,
    zero_flow_exits: &std::collections::HashSet<String>,
) -> (usize, usize, bool) {
    let mut neighbors = std::collections::HashSet::new();
    for pipe in network.pipes().filter(|p| p.hydraulically_active()) {
        if pipe.from == node_id {
            neighbors.insert(pipe.to.as_str());
        } else if pipe.to == node_id {
            neighbors.insert(pipe.from.as_str());
        }
    }
    let mut entry_neighbors = 0usize;
    let mut exit_neighbors = 0usize;
    for id in neighbors {
        if zero_flow_entries.contains(id) {
            entry_neighbors += 1;
        }
        if zero_flow_exits.contains(id) {
            exit_neighbors += 1;
        }
    }
    (
        entry_neighbors,
        exit_neighbors,
        entry_neighbors > 0 && exit_neighbors > 0,
    )
}

fn node_hydraulic_degree(network: &GasNetwork, node_id: &str) -> usize {
    network
        .pipes()
        .filter(|p| p.hydraulically_active())
        .filter(|p| p.from == node_id || p.to == node_id)
        .count()
}

fn is_compressor_endpoint(network: &GasNetwork, node_id: &str) -> bool {
    network.pipes().any(|pipe| {
        pipe.kind == ConnectionKind::CompressorStation
            && pipe.hydraulically_active()
            && (pipe.from == node_id || pipe.to == node_id)
    })
}

fn all_applied_anchor_ids(scenario: &ScenarioDemands) -> std::collections::HashSet<String> {
    scenario
        .pressure_slack
        .iter()
        .map(|s| s.node_id.clone())
        .chain(scenario.balance_hubs.iter().map(|h| h.node_id.clone()))
        .chain(scenario.boundary_spine_anchors.iter().map(|h| h.node_id.clone()))
        .chain(scenario.junction_anchors.iter().map(|h| h.node_id.clone()))
        .chain(scenario.mass_balance_anchors.iter().map(|h| h.node_id.clone()))
        .chain(scenario.contract_pressure_anchors.iter().map(|h| h.node_id.clone()))
        .collect()
}

const MIN_CONTRACT_RELAX_IMBALANCE_M3S: f64 = 1.5;
const MAX_CONTRACT_RELAX_PER_PASS: usize = 3;

fn contract_fix_pressure_on_relax() -> bool {
    std::env::var("GAZFLOW_CONTRACT_FIX_PRESSURE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Abandonne la contrainte de débit nominatif sur des boundaries (v18, opt-in bench).
pub fn try_relax_contract_boundary(
    scenario: &mut ScenarioDemands,
    top_imbalances: &[(String, f64)],
    solved_pressures: &HashMap<String, f64>,
) -> bool {
    if !contract_boundary_refinement_enabled() {
        return false;
    }
    let slack_id = scenario
        .pressure_slack
        .as_ref()
        .map(|s| s.node_id.as_str());
    let relaxed_at_start = scenario.contract_flow_relaxed.len();
    let mut relaxed = false;
    for (node_id, imbalance) in top_imbalances {
        if scenario.contract_flow_relaxed.len() >= relaxed_at_start + MAX_CONTRACT_RELAX_PER_PASS {
            break;
        }
        if imbalance.abs() < MIN_CONTRACT_RELAX_IMBALANCE_M3S {
            continue;
        }
        if !node_id.starts_with("source_") && !node_id.starts_with("sink_") {
            continue;
        }
        if Some(node_id.as_str()) == slack_id {
            continue;
        }
        if scenario.contract_flow_relaxed.iter().any(|id| id == node_id) {
            continue;
        }
        scenario.contract_flow_relaxed.push(node_id.clone());
        if contract_fix_pressure_on_relax() {
            if let Some(pressure_bar) = solved_pressures
                .get(node_id)
                .copied()
                .filter(|p| p.is_finite() && *p > 0.0)
            {
                scenario.contract_pressure_anchors.push(PressureSlackHint {
                    node_id: node_id.clone(),
                    pressure_bar,
                });
            }
        }
        relaxed = true;
        if scenario.contract_flow_relaxed.len() >= relaxed_at_start + MAX_CONTRACT_RELAX_PER_PASS {
            break;
        }
    }
    relaxed
}

/// Clone réseau baseline avec toutes les conditions aux limites scénario appliquées.
pub fn network_with_scenario_boundaries(
    base: &GasNetwork,
    scenario: &ScenarioDemands,
) -> GasNetwork {
    let mut network = base.clone();
    apply_scenario_boundaries(&mut network, scenario);
    if scenario_pressure_envelopes_enabled() {
        apply_scenario_pressure_envelopes(&mut network, scenario);
    }
    network
}

const MIN_MASS_BALANCE_ANCHOR_IMBALANCE_M3S: f64 = 1.0;

/// Ajoute un ancrage pression sur le pire `innode_*` libre du bilan massique.
pub fn try_add_mass_balance_anchor(
    network: &GasNetwork,
    scenario: &mut ScenarioDemands,
    top_imbalances: &[(String, f64)],
    solved_pressures: Option<&HashMap<String, f64>>,
) -> bool {
    let reserved = all_applied_anchor_ids(scenario);
    for (node_id, imbalance) in top_imbalances {
        if imbalance.abs() < MIN_MASS_BALANCE_ANCHOR_IMBALANCE_M3S {
            continue;
        }
        if !node_id.starts_with("innode_") {
            continue;
        }
        if reserved.contains(node_id) {
            continue;
        }
        if is_compressor_endpoint(network, node_id) {
            continue;
        }
        let net_node = network.nodes().find(|n| n.id == *node_id);
        let Some(pressure_bar) = solved_pressures
            .and_then(|p| p.get(node_id).copied())
            .filter(|p| p.is_finite() && *p > 0.0)
            .or_else(|| net_node.and_then(|n| n.pressure_lower_bar))
        else {
            continue;
        };
        scenario.mass_balance_anchors.push(PressureSlackHint {
            node_id: node_id.clone(),
            pressure_bar,
        });
        return true;
    }
    false
}

const MIN_BOUNDARY_SPINE_DEGREE: usize = 4;

/// Boundaries Q≈0 très connectées (ex. `source_17`) pour fermer les boucles locales.
pub fn detect_boundary_spine_anchors(
    network: &GasNetwork,
    scenario: &ScenarioDemands,
    max_anchors: usize,
) -> Vec<PressureSlackHint> {
    let (zero_flow_entries, zero_flow_exits) = zero_flow_boundary_kind_sets(scenario);
    let reserved = all_applied_anchor_ids(scenario);

    let mut ranked: Vec<(bool, usize, usize, PressureSlackHint)> = scenario
        .zero_flow_boundary_anchors
        .iter()
        .filter(|a| !reserved.contains(&a.node_id))
        .filter(|a| a.node_id.starts_with("source_") || a.node_id.starts_with("sink_"))
        .filter_map(|anchor| {
            let degree = node_hydraulic_degree(network, &anchor.node_id);
            if degree < MIN_BOUNDARY_SPINE_DEGREE {
                return None;
            }
            let (entry_neighbors, exit_neighbors, mixed) = count_mixed_zero_flow_neighbors(
                network,
                &anchor.node_id,
                &zero_flow_entries,
                &zero_flow_exits,
            );
            let zf_neighbors = entry_neighbors + exit_neighbors;
            if zf_neighbors < MIN_ZERO_FLOW_BOUNDARY_NEIGHBORS {
                return None;
            }
            let pressure_bar =
                resolve_anchor_pressure_bar(network, &anchor.node_id, anchor.scenario_pressure_bar)?;
            Some((
                mixed,
                zf_neighbors,
                degree,
                PressureSlackHint {
                    node_id: anchor.node_id.clone(),
                    pressure_bar,
                },
            ))
        })
        .collect();

    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.3.node_id.cmp(&b.3.node_id))
    });
    ranked
        .into_iter()
        .take(max_anchors)
        .map(|(_, _, _, anchor)| anchor)
        .collect()
}

/// Choisit les hubs de balance (boundaries Q≈0 les plus connectés) pour ancrer la pression locale.
pub fn detect_balance_hubs_for_network(
    network: &GasNetwork,
    scenario: &ScenarioDemands,
    max_hubs: usize,
) -> Vec<PressureSlackHint> {
    let slack_id = scenario
        .pressure_slack
        .as_ref()
        .map(|s| s.node_id.as_str());
    let mut ranked: Vec<(usize, PressureSlackHint)> = scenario
        .zero_flow_boundary_anchors
        .iter()
        .filter(|a| Some(a.node_id.as_str()) != slack_id)
        .filter_map(|node| {
            let degree = node_hydraulic_degree(network, &node.node_id);
            if degree == 0 {
                return None;
            }
            let pressure_bar =
                resolve_anchor_pressure_bar(network, &node.node_id, node.scenario_pressure_bar)?;
            Some((
                degree,
                PressureSlackHint {
                    node_id: node.node_id.clone(),
                    pressure_bar,
                },
            ))
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.node_id.cmp(&b.1.node_id)));
    ranked
        .into_iter()
        .take(max_hubs.max(1))
        .map(|(_, hub)| hub)
        .collect()
}

const MIN_JUNCTION_DEGREE: usize = 4;
const MIN_JUNCTION_DEGREE_MIXED: usize = 3;
const MIN_JUNCTION_DEGREE_EXIT_HUB: usize = 3;
const MIN_ZERO_FLOW_BOUNDARY_NEIGHBORS: usize = 2;

fn min_junction_degree(entry_neighbors: usize, exit_neighbors: usize, mixed: bool) -> usize {
    if mixed {
        return MIN_JUNCTION_DEGREE_MIXED;
    }
    if entry_neighbors == 0 && exit_neighbors >= MIN_ZERO_FLOW_BOUNDARY_NEIGHBORS {
        return MIN_JUNCTION_DEGREE_EXIT_HUB;
    }
    MIN_JUNCTION_DEGREE
}

/// Junctions internes (ex. `innode_381`) adjacentes à entry+exit Q≈0.
pub fn detect_junction_balance_anchors(
    network: &GasNetwork,
    scenario: &ScenarioDemands,
    max_anchors: usize,
) -> Vec<PressureSlackHint> {
    let (zero_flow_entries, zero_flow_exits) = zero_flow_boundary_kind_sets(scenario);
    let reserved = all_applied_anchor_ids(scenario);

    let mut ranked: Vec<(bool, usize, usize, PressureSlackHint)> = network
        .nodes()
        .filter(|node| node.id.starts_with("innode_"))
        .filter(|node| !is_compressor_endpoint(network, &node.id))
        .filter(|node| node.pressure_fixed_bar.is_none())
        .filter(|node| !reserved.contains(&node.id))
        .filter(|node| {
            scenario
                .demands
                .get(&node.id)
                .map(|q| q.abs() <= 1e-6)
                .unwrap_or(true)
        })
        .filter_map(|node| {
            let degree = node_hydraulic_degree(network, &node.id);
            let (entry_neighbors, exit_neighbors, mixed) = count_mixed_zero_flow_neighbors(
                network,
                &node.id,
                &zero_flow_entries,
                &zero_flow_exits,
            );
            let zf_neighbors = entry_neighbors + exit_neighbors;
            if zf_neighbors < MIN_ZERO_FLOW_BOUNDARY_NEIGHBORS {
                return None;
            }
            let min_degree = min_junction_degree(entry_neighbors, exit_neighbors, mixed);
            if degree < min_degree {
                return None;
            }
            let pressure_bar = node.pressure_lower_bar?;
            Some((
                mixed,
                zf_neighbors,
                degree,
                PressureSlackHint {
                    node_id: node.id.clone(),
                    pressure_bar,
                },
            ))
        })
        .collect();

    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.3.node_id.cmp(&b.3.node_id))
    });
    ranked
        .into_iter()
        .take(max_anchors)
        .map(|(_, _, _, anchor)| anchor)
        .collect()
}

/// Meilleure junction exit-only (ex. `innode_314` → `sink_94`/`sink_95`).
pub fn detect_exit_hub_junction_anchor(
    network: &GasNetwork,
    scenario: &ScenarioDemands,
) -> Option<PressureSlackHint> {
    let (zero_flow_entries, zero_flow_exits) = zero_flow_boundary_kind_sets(scenario);
    let reserved = all_applied_anchor_ids(scenario);

    let mut ranked: Vec<(usize, usize, PressureSlackHint)> = network
        .nodes()
        .filter(|node| node.id.starts_with("innode_"))
        .filter(|node| !is_compressor_endpoint(network, &node.id))
        .filter(|node| node.pressure_fixed_bar.is_none())
        .filter(|node| !reserved.contains(&node.id))
        .filter(|node| {
            scenario
                .demands
                .get(&node.id)
                .map(|q| q.abs() <= 1e-6)
                .unwrap_or(true)
        })
        .filter_map(|node| {
            let degree = node_hydraulic_degree(network, &node.id);
            let (entry_neighbors, exit_neighbors, _) = count_mixed_zero_flow_neighbors(
                network,
                &node.id,
                &zero_flow_entries,
                &zero_flow_exits,
            );
            if entry_neighbors != 0 || exit_neighbors < MIN_ZERO_FLOW_BOUNDARY_NEIGHBORS {
                return None;
            }
            let min_degree = min_junction_degree(0, exit_neighbors, false);
            if degree < min_degree {
                return None;
            }
            let pressure_bar = node.pressure_lower_bar?;
            Some((
                exit_neighbors,
                degree,
                PressureSlackHint {
                    node_id: node.id.clone(),
                    pressure_bar,
                },
            ))
        })
        .collect();

    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.node_id.cmp(&b.2.node_id))
    });
    ranked.into_iter().next().map(|(_, _, anchor)| anchor)
}

/// Enrichit le scénario avec les ancrages pression détectés sur le réseau chargé.
pub fn enrich_scenario_with_balance_hub(
    network: &GasNetwork,
    scenario: &mut ScenarioDemands,
) {
    if transport_minimal_anchors_enabled() {
        return;
    }
    scenario.balance_hubs = detect_balance_hubs_for_network(network, scenario, 2);
    scenario.boundary_spine_anchors = detect_boundary_spine_anchors(network, scenario, 1);
    scenario.junction_anchors = detect_junction_balance_anchors(network, scenario, 2);
    if let Some(exit_hub) = detect_exit_hub_junction_anchor(network, scenario) {
        if !scenario
            .junction_anchors
            .iter()
            .any(|a| a.node_id == exit_hub.node_id)
        {
            scenario.junction_anchors.push(exit_hub);
        }
    }
}

/// Détecte le nœud slack pression pour les scénarios transport GasLib.
///
/// Heuristique : sortie avec débit significatif et borne pression basse seule
/// (typique des nœuds de balancement type sink_109 sur GasLib-582).
fn detect_pressure_slack(nodes: &[XmlScenarioNode]) -> Option<PressureSlackHint> {
    let mut best: Option<(String, f64, f64)> = None;

    for node in nodes {
        let flow_mag = extract_flow_value(&node.flows).unwrap_or(0.0).abs();
        if flow_mag < 5.0 {
            continue;
        }

        let mut lower: Option<f64> = None;
        let mut upper: Option<f64> = None;
        for p in &node.pressures {
            let abs = pressure_to_bar_absolute(p.value, p.unit.as_deref());
            match p.bound.as_deref() {
                Some("lower") => lower = Some(abs),
                Some("upper") => upper = Some(abs),
                Some("both") => {
                    lower = Some(abs);
                    upper = Some(abs);
                }
                _ => {}
            }
        }

        if lower.is_some() && upper.is_none() {
            let pressure = lower?;
            let replace = best
                .as_ref()
                .map(|(_, _, prev_flow)| flow_mag > *prev_flow)
                .unwrap_or(true);
            if replace {
                best = Some((node.id.clone(), pressure, flow_mag));
            }
        }
    }

    best.map(|(node_id, pressure_bar, _)| PressureSlackHint {
        node_id,
        pressure_bar,
    })
}

fn pressure_to_bar_absolute(value: f64, unit: Option<&str>) -> f64 {
    match unit {
        Some("barg") => value + 1.01325,
        _ => value,
    }
}

fn extract_flow_value(flows: &[XmlFlowBound]) -> Option<f64> {
    if flows.is_empty() {
        return None;
    }

    let mut lower: Option<f64> = None;
    let mut upper: Option<f64> = None;
    let mut first: Option<f64> = None;

    for flow in flows {
        let value = convert_flow_to_m3_per_s(flow.value, flow.unit.as_deref());
        if first.is_none() {
            first = Some(value);
        }
        match flow.bound.as_deref() {
            Some("lower") => lower = Some(value),
            Some("upper") => upper = Some(value),
            Some("both") => {
                lower = Some(value);
                upper = Some(value);
            }
            _ => {}
        }
    }

    match (lower, upper, first) {
        (Some(l), Some(u), _) => Some((l + u) / 2.0),
        (Some(l), None, _) => Some(l),
        (None, Some(u), _) => Some(u),
        (None, None, Some(v)) => Some(v),
        (None, None, None) => None,
    }
}

fn convert_flow_to_m3_per_s(value: f64, unit: Option<&str>) -> f64 {
    match unit {
        Some("1000m_cube_per_hour") => value * 1000.0 / 3600.0,
        Some("m_cube_per_hour") => value / 3600.0,
        Some("m_cube_per_second") => value,
        _ => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::path::Path;

    #[test]
    fn test_parse_scenario_scn() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<boundaryValue xmlns="http://gaslib.zib.de/Gas" xmlns:framework="http://gaslib.zib.de/Framework">
  <scenario id="GasLib_11_scenario">
    <node type="entry" id="entry01">
      <flow bound="lower" value="160.00" unit="1000m_cube_per_hour"/>
      <flow bound="upper" value="160.00" unit="1000m_cube_per_hour"/>
    </node>
    <node type="exit" id="exit01">
      <flow bound="lower" value="100.00" unit="1000m_cube_per_hour"/>
      <flow bound="upper" value="100.00" unit="1000m_cube_per_hour"/>
    </node>
    <node type="exit" id="exit02">
      <flow bound="both" value="120.00" unit="1000m_cube_per_hour"/>
    </node>
  </scenario>
</boundaryValue>"#;

        let parsed = parse_scenario_demands_from_str(xml).expect("scenario parsing");
        assert_eq!(parsed.scenario_id.as_deref(), Some("GasLib_11_scenario"));
        assert!((parsed.demands["entry01"] - 44.444_444_444).abs() < 1e-9);
        assert!((parsed.demands["exit01"] + 27.777_777_777).abs() < 1e-9);
        assert!((parsed.demands["exit02"] + 33.333_333_333).abs() < 1e-9);
        assert!(parsed.pressure_slack.is_none());
    }

    #[test]
    fn test_detect_transport_pressure_slack() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<boundaryValue>
  <scenario id="transport">
    <node type="exit" id="sink_109">
      <pressure unit="barg" bound="lower" value="50.0"/>
      <flow unit="1000m_cube_per_hour" bound="lower" value="920.1659"/>
      <flow unit="1000m_cube_per_hour" bound="upper" value="920.1659"/>
    </node>
  </scenario>
</boundaryValue>"#;

        let parsed = parse_scenario_demands_from_str(xml).expect("parse");
        let slack = parsed.pressure_slack.expect("slack");
        assert_eq!(slack.node_id, "sink_109");
        assert!((slack.pressure_bar - 51.01325).abs() < 1e-4);
    }

    #[test]
    fn test_apply_scenario_boundaries_sets_slack() {
        use crate::graph::Node;

        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "sink_109".into(),
            x: 0.0,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
        });

        let scenario = ScenarioDemands {
            scenario_id: None,
            demands: HashMap::new(),
            pressure_slack: Some(PressureSlackHint {
                node_id: "sink_109".into(),
                pressure_bar: 51.01325,
            }),
            balance_hubs: Vec::new(),
            junction_anchors: Vec::new(),
            boundary_spine_anchors: Vec::new(),
            mass_balance_anchors: Vec::new(),
            zero_flow_boundary_anchors: Vec::new(),
            contract_flow_relaxed: Vec::new(),
            contract_pressure_anchors: Vec::new(),
            pressure_envelopes: Vec::new(),
        };

        apply_scenario_boundaries(&mut net, &scenario);
        let fixed = net
            .nodes()
            .find(|n| n.id == "sink_109")
            .and_then(|n| n.pressure_fixed_bar);
        assert_eq!(fixed, Some(51.01325));
    }

    #[test]
    fn test_scenario_keeps_unknown_node_type_positive() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<boundaryValue>
  <scenario id="x">
    <node type="sink" id="n1">
      <flow value="42.0"/>
    </node>
  </scenario>
</boundaryValue>"#;

        let parsed = parse_scenario_demands_from_str(xml).expect("scenario parsing");
        assert_eq!(parsed.demands.get("n1"), Some(&42.0));
    }

    #[test]
    fn test_parse_gaslib_11_scenario_file() {
        let path = Path::new("dat/GasLib-11.scn");
        if !path.exists() {
            eprintln!("skip: {:?} not found", path);
            return;
        }

        let parsed = load_scenario_demands(path).expect("load scenario file");
        assert_eq!(parsed.scenario_id.as_deref(), Some("GasLib_11_scenario"));
        assert_eq!(parsed.demands.len(), 6);
        assert!((parsed.demands["entry01"] - 44.444_444_444).abs() < 1e-9);
        assert!((parsed.demands["entry02"] - 38.888_888_888).abs() < 1e-9);
        assert!((parsed.demands["entry03"] - 0.0).abs() < 1e-9);
        assert!((parsed.demands["exit01"] + 27.777_777_777).abs() < 1e-9);
        assert!((parsed.demands["exit02"] + 33.333_333_333).abs() < 1e-9);
        assert!((parsed.demands["exit03"] + 22.222_222_222).abs() < 1e-9);
        assert!(parsed.pressure_slack.is_none());

        let sum: f64 = parsed.demands.values().sum();
        assert!(
            sum.abs() < 1e-9,
            "scenario should be globally balanced, got sum={sum}"
        );
    }

    #[test]
    fn test_parse_gaslib_582_scenario_slack() {
        let path = Path::new("dat/GasLib-582.scn");
        if !path.exists() {
            eprintln!("skip: {:?} not found", path);
            return;
        }

        let parsed = load_scenario_demands(path).expect("load 582 scenario");
        let slack = parsed
            .pressure_slack
            .as_ref()
            .expect("582 scenario should expose pressure slack");
        assert_eq!(slack.node_id, "sink_109");
    }

    #[test]
    fn test_demands_without_pressure_slack() {
        let mut demands = HashMap::new();
        demands.insert("sink_109".into(), -255.0);
        let scenario = ScenarioDemands {
            scenario_id: None,
            demands: demands.clone(),
            pressure_slack: Some(PressureSlackHint {
                node_id: "sink_109".into(),
                pressure_bar: 51.01325,
            }),
            balance_hubs: Vec::new(),
            junction_anchors: Vec::new(),
            boundary_spine_anchors: Vec::new(),
            mass_balance_anchors: Vec::new(),
            zero_flow_boundary_anchors: Vec::new(),
            contract_flow_relaxed: Vec::new(),
            contract_pressure_anchors: Vec::new(),
            pressure_envelopes: Vec::new(),
        };
        let adjusted = effective_solver_demands(&demands, &scenario);
        assert!(!adjusted.contains_key("sink_109"));
    }

    #[test]
    fn test_effective_solver_demands_contract_relaxed() {
        let mut demands = HashMap::new();
        demands.insert("sink_24".into(), -5.0);
        let scenario = ScenarioDemands {
            scenario_id: None,
            demands: demands.clone(),
            pressure_slack: None,
            balance_hubs: Vec::new(),
            junction_anchors: Vec::new(),
            boundary_spine_anchors: Vec::new(),
            mass_balance_anchors: Vec::new(),
            zero_flow_boundary_anchors: Vec::new(),
            contract_flow_relaxed: vec!["sink_24".into()],
            contract_pressure_anchors: Vec::new(),
            pressure_envelopes: Vec::new(),
        };
        let adjusted = effective_solver_demands(&demands, &scenario);
        assert!(!adjusted.contains_key("sink_24"));
    }

    #[test]
    #[serial]
    fn test_try_relax_contract_boundary() {
        unsafe { std::env::set_var("GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT", "1") };
        let mut scenario = ScenarioDemands {
            scenario_id: None,
            demands: HashMap::new(),
            pressure_slack: Some(PressureSlackHint {
                node_id: "sink_109".into(),
                pressure_bar: 51.0,
            }),
            balance_hubs: Vec::new(),
            junction_anchors: Vec::new(),
            boundary_spine_anchors: Vec::new(),
            mass_balance_anchors: Vec::new(),
            zero_flow_boundary_anchors: Vec::new(),
            contract_flow_relaxed: Vec::new(),
            contract_pressure_anchors: Vec::new(),
            pressure_envelopes: Vec::new(),
        };
        let mut pressures = HashMap::new();
        pressures.insert("sink_24".into(), 45.0);
        let imbalances = vec![("sink_24".into(), -2.0)];
        assert!(try_relax_contract_boundary(
            &mut scenario,
            &imbalances,
            &pressures
        ));
        assert_eq!(scenario.contract_flow_relaxed, vec!["sink_24"]);
        assert!(scenario.contract_pressure_anchors.is_empty());
        unsafe { std::env::remove_var("GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT") };
    }

    #[test]
    #[serial]
    fn test_contract_boundary_refinement_defaults_off() {
        unsafe { std::env::remove_var("GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT") };
        assert!(!contract_boundary_refinement_enabled());
        unsafe { std::env::set_var("GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT", "1") };
        assert!(contract_boundary_refinement_enabled());
        unsafe { std::env::remove_var("GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT") };
    }

    #[test]
    #[serial]
    fn test_try_relax_contract_boundary_respects_enable_flag() {
        unsafe { std::env::set_var("GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT", "0") };
        let mut scenario = ScenarioDemands {
            scenario_id: None,
            demands: HashMap::new(),
            pressure_slack: None,
            balance_hubs: Vec::new(),
            junction_anchors: Vec::new(),
            boundary_spine_anchors: Vec::new(),
            mass_balance_anchors: Vec::new(),
            zero_flow_boundary_anchors: Vec::new(),
            contract_flow_relaxed: Vec::new(),
            contract_pressure_anchors: Vec::new(),
            pressure_envelopes: Vec::new(),
        };
        let mut pressures = HashMap::new();
        pressures.insert("sink_24".into(), 45.0);
        let imbalances = vec![("sink_24".into(), -2.0)];
        assert!(!try_relax_contract_boundary(
            &mut scenario,
            &imbalances,
            &pressures
        ));
        unsafe { std::env::remove_var("GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT") };
    }

    #[test]
    fn test_collect_scenario_pressure_envelopes() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<boundaryValue>
  <scenario id="transport">
    <node type="exit" id="sink_109">
      <pressure unit="barg" bound="lower" value="50.0"/>
      <flow unit="1000m_cube_per_hour" bound="both" value="920.1659"/>
    </node>
    <node type="exit" id="sink_24">
      <pressure unit="barg" bound="lower" value="40.0"/>
      <pressure unit="barg" bound="upper" value="55.0"/>
      <flow unit="1000m_cube_per_hour" bound="both" value="100.0"/>
    </node>
    <node type="exit" id="sink_0">
      <flow unit="1000m_cube_per_hour" bound="both" value="0.0"/>
      <pressure unit="barg" bound="lower" value="30.0"/>
    </node>
  </scenario>
</boundaryValue>"#;

        let parsed = parse_scenario_demands_from_str(xml).expect("parse");
        assert_eq!(parsed.pressure_envelopes.len(), 1);
        let env = &parsed.pressure_envelopes[0];
        assert_eq!(env.node_id, "sink_24");
        assert!(env.lower_bar.is_some());
        assert!(env.upper_bar.is_some());
        assert!(
            !parsed
                .pressure_envelopes
                .iter()
                .any(|e| e.node_id == "sink_109"),
            "slack excluded from envelopes"
        );
    }

    #[test]
    fn test_apply_scenario_pressure_envelopes_merges_net_bounds() {
        use crate::graph::Node;

        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "sink_24".into(),
            x: 0.0,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: Some(35.0),
            pressure_upper_bar: Some(60.0),
            pressure_fixed_bar: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
        });

        let scenario = ScenarioDemands {
            scenario_id: None,
            demands: HashMap::new(),
            pressure_slack: None,
            balance_hubs: Vec::new(),
            junction_anchors: Vec::new(),
            boundary_spine_anchors: Vec::new(),
            mass_balance_anchors: Vec::new(),
            zero_flow_boundary_anchors: Vec::new(),
            contract_flow_relaxed: Vec::new(),
            contract_pressure_anchors: Vec::new(),
            pressure_envelopes: vec![ScenarioPressureEnvelope {
                node_id: "sink_24".into(),
                lower_bar: Some(40.0),
                upper_bar: Some(55.0),
            }],
        };

        apply_scenario_pressure_envelopes(&mut net, &scenario);
        let node = net.nodes().find(|n| n.id == "sink_24").expect("node");
        assert_eq!(node.pressure_lower_bar, Some(40.0));
        assert_eq!(node.pressure_upper_bar, Some(55.0));
        assert!(net.scenario_pressure_envelope_nodes.contains("sink_24"));
    }

    #[test]
    #[serial]
    fn test_transport_minimal_anchors_skips_enrich() {
        let net_path = Path::new("dat/GasLib-582.net");
        let scn_path = Path::new("dat/Nominations-582-v2-20211129/nomination_mild_618.scn");
        if !net_path.exists() || !scn_path.exists() {
            eprintln!("skip: 582 mild_618 data missing");
            return;
        }
        unsafe { std::env::set_var("GAZFLOW_TRANSPORT_MINIMAL_ANCHORS", "1") };
        let network = crate::gaslib::load_network(net_path).expect("net");
        let mut scenario = load_scenario_demands(scn_path).expect("scn");
        enrich_scenario_with_balance_hub(&network, &mut scenario);
        assert!(scenario.balance_hubs.is_empty());
        assert!(scenario.junction_anchors.is_empty());
        assert!(scenario.boundary_spine_anchors.is_empty());
        unsafe { std::env::remove_var("GAZFLOW_TRANSPORT_MINIMAL_ANCHORS") };
    }

    #[test]
    fn test_detect_shortpipe_boundary_pairs() {
        use crate::graph::{ConnectionKind, GasNetwork, Node, Pipe};

        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "sink_122".into(),
            ..Default::default()
        });
        net.add_node(Node {
            id: "source_10".into(),
            ..Default::default()
        });
        net.add_pipe(Pipe {
            id: "shortPipe_55".into(),
            from: "sink_122".into(),
            to: "source_10".into(),
            kind: ConnectionKind::ShortPipe,
            ..Default::default()
        });
        let pairs = detect_shortpipe_boundary_pairs(&net);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].sink_id, "sink_122");
        assert_eq!(pairs[0].source_id, "source_10");
    }

    #[test]
    fn test_mild_618_balance_hub_is_sink_2() {
        unsafe { std::env::remove_var("GAZFLOW_TRANSPORT_MINIMAL_ANCHORS") };
        let net_path = Path::new("dat/GasLib-582.net");
        let scn_path = Path::new("dat/Nominations-582-v2-20211129/nomination_mild_618.scn");
        if !net_path.exists() || !scn_path.exists() {
            eprintln!("skip: 582 mild_618 data missing");
            return;
        }
        let network = crate::gaslib::load_network(net_path).expect("net");
        let mut scenario = load_scenario_demands(scn_path).expect("scn");
        enrich_scenario_with_balance_hub(&network, &mut scenario);
        let hub = scenario
            .balance_hubs
            .first()
            .expect("balance hub");
        assert_eq!(hub.node_id, "sink_2");
        assert!(
            scenario.balance_hubs.iter().any(|h| h.node_id == "sink_96"),
            "second hub should include sink_96, got {:?}",
            scenario
                .balance_hubs
                .iter()
                .map(|h| &h.node_id)
                .collect::<Vec<_>>()
        );
        assert!(
            scenario
                .boundary_spine_anchors
                .iter()
                .any(|a| a.node_id == "source_17"),
            "spine anchor should include source_17, got {:?}",
            scenario
                .boundary_spine_anchors
                .iter()
                .map(|a| &a.node_id)
                .collect::<Vec<_>>()
        );
        assert!(
            scenario
                .junction_anchors
                .iter()
                .any(|a| a.node_id == "innode_381"),
            "junction anchor should include innode_381, got {:?}",
            scenario
                .junction_anchors
                .iter()
                .map(|a| &a.node_id)
                .collect::<Vec<_>>()
        );
        assert!(
            scenario.junction_anchors.len() >= 3,
            "expected mixed + exit-hub junctions, got {:?}",
            scenario
                .junction_anchors
                .iter()
                .map(|a| &a.node_id)
                .collect::<Vec<_>>()
        );
        assert!(
            scenario.junction_anchors.iter().any(|a| {
                matches!(
                    a.node_id.as_str(),
                    "innode_314" | "innode_315" | "innode_331"
                )
            }),
            "exit-hub junction expected, got {:?}",
            scenario
                .junction_anchors
                .iter()
                .map(|a| &a.node_id)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_units_scn_to_si() {
        let v1 = convert_flow_to_m3_per_s(1.0, Some("1000m_cube_per_hour"));
        let v2 = convert_flow_to_m3_per_s(3600.0, Some("m_cube_per_hour"));
        let v3 = convert_flow_to_m3_per_s(1.0, Some("m_cube_per_second"));

        assert!((v1 - (1000.0 / 3600.0)).abs() < 1e-12);
        assert!((v2 - 1.0).abs() < 1e-12);
        assert!((v3 - 1.0).abs() < 1e-12);
    }
}
