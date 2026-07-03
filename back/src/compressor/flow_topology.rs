//! Estimation topologique du débit compresseur pour la carte (.cs).

use std::collections::{HashMap, HashSet, VecDeque};

use petgraph::Direction;
use petgraph::visit::EdgeRef;
use petgraph::graph::NodeIndex;

use crate::graph::{ConnectionKind, GasNetwork, Pipe};

const TOPOLOGY_REACH_MAX_HOPS: usize = 12;
const DISTRIBUTION_LOCAL_HOPS: usize = 6;
const DISTRIBUTION_EXCLUSION_HOPS: usize = 4;
pub(crate) const HIGH_TRANSPORT_CAP_THRESHOLD: f64 = 3.0;
const TRANSPORT_CAP_THRESHOLD: f64 = 2.0;

#[derive(Debug, Clone, Default)]
struct CompressorFlowRoles {
    /// Compresseurs hub (ex. CS1 : aval de branches parallèles) → Q ≈ livraison totale.
    hub_mergers: HashSet<String>,
    /// Branche parallèle alimentant un hub → (merger_id, nb branches parallèles).
    branch_to_merger: HashMap<String, (String, usize)>,
}

fn is_transport_compressor(pipe: &Pipe) -> bool {
    pipe.equipment
        .compressor_pressure_cap_ratio
        .unwrap_or(1.0)
        >= TRANSPORT_CAP_THRESHOLD
}

fn is_high_transport_compressor(pipe: &Pipe) -> bool {
    pipe.equipment
        .compressor_pressure_cap_ratio
        .unwrap_or(1.0)
        >= HIGH_TRANSPORT_CAP_THRESHOLD
}

fn active_compressor_pipes(network: &GasNetwork) -> Vec<&Pipe> {
    network
        .pipes()
        .filter(|p| p.kind == ConnectionKind::CompressorStation && p.hydraulically_active())
        .collect()
}

/// Atteignabilité aval (pipes/vannes) sans traverser une autre station compresseur.
fn reaches_merge_inlet(network: &GasNetwork, start_node: &str, merge_inlet: &str) -> bool {
    let (Some(start), Some(goal)) = (
        network.node_index(start_node),
        network.node_index(merge_inlet),
    ) else {
        return false;
    };
    if start == goal {
        return true;
    }

    let mut visited = HashSet::new();
    let mut queue = VecDeque::from([(start, 0usize)]);
    while let Some((node, depth)) = queue.pop_front() {
        if depth >= TOPOLOGY_REACH_MAX_HOPS {
            continue;
        }
        if !visited.insert(node) {
            continue;
        }
        for edge in network.graph.edges(node) {
            if edge.weight().kind == ConnectionKind::CompressorStation {
                continue;
            }
            let target = edge.target();
            if target == goal {
                return true;
            }
            queue.push_back((target, depth + 1));
        }
    }
    false
}

fn traversable_without_compressor_crossing(pipe: &Pipe) -> bool {
    pipe.hydraulically_active()
        && matches!(
            pipe.kind,
            ConnectionKind::Pipe | ConnectionKind::ShortPipe | ConnectionKind::Valve
        )
}

/// Variante pour la recherche de sinks aval en mode variables de décision.
/// Inclut `Resistor` et `ControlValve` (quasi-transparent dans le MVP courant),
/// gated par `hydraulically_active()` (une CV fermée reste bloquante).
fn traversable_for_decision_reach(pipe: &Pipe) -> bool {
    pipe.hydraulically_active()
        && matches!(
            pipe.kind,
            ConnectionKind::Pipe
                | ConnectionKind::ShortPipe
                | ConnectionKind::Valve
                | ConnectionKind::Resistor
                | ConnectionKind::ControlValve
        )
}

/// Sinks bornés (enveloppes scénario) atteignables à l'aval d'une sortie compresseur.
///
/// Le parcours est non orienté sur `Pipe`/`ShortPipe`/`Valve` actifs et ne traverse
/// aucun arc compresseur actif.
pub(crate) fn downstream_bounded_sinks(
    network: &GasNetwork,
    cs_from_node: &str,
) -> Vec<(String, f64)> {
    let Some(start) = network.node_index(cs_from_node) else {
        return Vec::new();
    };

    let mut visited = HashSet::new();
    let mut queue = VecDeque::from([start]);
    let mut sink_seen = HashSet::new();
    let mut sinks = Vec::new();

    while let Some(node) = queue.pop_front() {
        if !visited.insert(node) {
            continue;
        }

        if let Some(node_data) = network.graph.node_weight(node)
            && node_data.id.starts_with("sink_")
            && network
                .scenario_pressure_envelope_nodes
                .contains(&node_data.id)
            && let Some(lower_bar) = node_data.pressure_lower_bar
            && sink_seen.insert(node_data.id.clone())
        {
            sinks.push((node_data.id.clone(), lower_bar));
        }

        for edge in network.graph.edges(node) {
            if !traversable_for_decision_reach(edge.weight()) {
                continue;
            }
            queue.push_back(edge.target());
        }
        for edge in network.graph.edges_directed(node, Direction::Incoming) {
            if !traversable_for_decision_reach(edge.weight()) {
                continue;
            }
            queue.push_back(edge.source());
        }
    }

    sinks.sort_by(|a, b| a.0.cmp(&b.0));
    sinks
}

fn classify_compressor_flow_roles(network: &GasNetwork) -> CompressorFlowRoles {
    let high_transport: Vec<&Pipe> = active_compressor_pipes(network)
        .into_iter()
        .filter(|p| is_high_transport_compressor(p))
        .collect();

    let mut roles = CompressorFlowRoles::default();

    for candidate in &high_transport {
        let feeder_count = high_transport
            .iter()
            .filter(|other| other.id != candidate.id)
            .filter(|other| reaches_merge_inlet(network, &other.to, &candidate.from))
            .count();
        if feeder_count >= 2 {
            roles.hub_mergers.insert(candidate.id.clone());
        }
    }

    for merger in &high_transport {
        if !roles.hub_mergers.contains(&merger.id) {
            continue;
        }
        let feeders: Vec<&Pipe> = high_transport
            .iter()
            .copied()
            .filter(|other| other.id != merger.id)
            .filter(|other| reaches_merge_inlet(network, &other.to, &merger.from))
            .collect();
        let n = feeders.len().max(1);
        for feeder in feeders {
            roles
                .branch_to_merger
                .insert(feeder.id.clone(), (merger.id.clone(), n));
        }
    }

    roles
}

fn count_distribution_compressors(network: &GasNetwork) -> usize {
    active_compressor_pipes(network)
        .into_iter()
        .filter(|p| is_transport_compressor(p) && !is_high_transport_compressor(p))
        .count()
}

fn undirected_neighbors_without_compressors(
    network: &GasNetwork,
    node: NodeIndex,
) -> impl Iterator<Item = NodeIndex> + use<'_> {
    let outgoing = network.graph.edges(node).filter_map(|edge| {
        if edge.weight().kind == ConnectionKind::CompressorStation {
            None
        } else {
            Some(edge.target())
        }
    });
    let incoming = network
        .graph
        .edges_directed(node, Direction::Incoming)
        .filter_map(|edge| {
            if edge.weight().kind == ConnectionKind::CompressorStation {
                None
            } else {
                Some(edge.source())
            }
        });
    outgoing.chain(incoming)
}

fn nodes_within_hops(
    network: &GasNetwork,
    seeds: &[NodeIndex],
    max_hops: usize,
) -> HashSet<NodeIndex> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    for &seed in seeds {
        queue.push_back((seed, 0usize));
    }
    while let Some((node, depth)) = queue.pop_front() {
        if depth >= max_hops {
            continue;
        }
        if !visited.insert(node) {
            continue;
        }
        for neighbor in undirected_neighbors_without_compressors(network, node) {
            queue.push_back((neighbor, depth + 1));
        }
    }
    visited
}

fn high_transport_endpoint_nodes(network: &GasNetwork) -> Vec<NodeIndex> {
    active_compressor_pipes(network)
        .into_iter()
        .filter(|p| is_high_transport_compressor(p))
        .filter_map(|p| network.node_index(&p.from))
        .collect()
}

fn distribution_peer_flow_m3s(
    network: &GasNetwork,
    pipe: &Pipe,
    demands: &HashMap<String, f64>,
    demand_scale: f64,
) -> f64 {
    let peers: Vec<f64> = active_compressor_pipes(network)
        .into_iter()
        .filter(|p| p.id != pipe.id)
        .filter(|p| is_transport_compressor(p) && !is_high_transport_compressor(p))
        .filter_map(|peer| {
            let subtree = compressor_subtree_delivery_m3s(network, peer, demands, demand_scale);
            if subtree > 1e-6 {
                return Some(subtree);
            }
            let adjacent = adjacent_sink_delivery_m3s(network, peer, demands, demand_scale);
            if adjacent > 1e-6 {
                Some(adjacent)
            } else {
                None
            }
        })
        .collect();
    if peers.is_empty() {
        0.0
    } else {
        peers.iter().sum::<f64>() / peers.len() as f64
    }
}

fn adjacent_sink_delivery_m3s(
    network: &GasNetwork,
    pipe: &Pipe,
    demands: &HashMap<String, f64>,
    demand_scale: f64,
) -> f64 {
    let scale = demand_scale.max(0.0);
    let mut delivery = 0.0_f64;
    for endpoint in [&pipe.from, &pipe.to] {
        let Some(idx) = network.node_index(endpoint) else {
            continue;
        };
        for neighbor in undirected_neighbors_without_compressors(network, idx) {
            if let Some(node_id) = network.graph.node_weight(neighbor).map(|n| n.id.as_str()) {
                if let Some(&q) = demands.get(node_id)
                    && q < 0.0
                {
                    delivery += q.abs();
                }
            }
        }
    }
    delivery * scale
}

/// Consommations locales au compresseur distribution, hors voisinage des CS transport.
fn compressor_subtree_delivery_m3s(
    network: &GasNetwork,
    pipe: &Pipe,
    demands: &HashMap<String, f64>,
    demand_scale: f64,
) -> f64 {
    let scale = demand_scale.max(0.0);
    if scale <= 0.0 {
        return 0.0;
    }
    let Some(from_idx) = network.node_index(&pipe.from) else {
        return 0.0;
    };
    let Some(to_idx) = network.node_index(&pipe.to) else {
        return 0.0;
    };

    let local_zone = nodes_within_hops(network, &[from_idx, to_idx], DISTRIBUTION_LOCAL_HOPS);
    let ht_exclusion = nodes_within_hops(
        network,
        &high_transport_endpoint_nodes(network),
        DISTRIBUTION_EXCLUSION_HOPS,
    );

    let mut delivery = 0.0_f64;
    for node in local_zone {
        if ht_exclusion.contains(&node) {
            continue;
        }
        if let Some(node_id) = network.graph.node_weight(node).map(|n| n.id.as_str()) {
            if let Some(&q) = demands.get(node_id)
                && q < 0.0
            {
                delivery += q.abs();
            }
        }
    }

    delivery * scale
}

/// Débit normal estimé pour l'évaluation carte, en tenant compte de la topologie hub/branche.
pub fn estimated_map_flow_m3s(
    network: &GasNetwork,
    pipe: &Pipe,
    total_delivery_m3s: f64,
    active_compressors: usize,
    demands: Option<&HashMap<String, f64>>,
    demand_scale: f64,
) -> f64 {
    if total_delivery_m3s <= 0.0 || !total_delivery_m3s.is_finite() {
        return 0.0;
    }

    let roles = classify_compressor_flow_roles(network);

    if roles.hub_mergers.contains(&pipe.id) {
        return total_delivery_m3s;
    }
    if let Some((_, n_parallel)) = roles.branch_to_merger.get(&pipe.id) {
        return total_delivery_m3s / (*n_parallel as f64);
    }
    if is_high_transport_compressor(pipe) {
        let n = active_compressor_pipes(network)
            .into_iter()
            .filter(|p| is_high_transport_compressor(p))
            .count()
            .max(1);
        return total_delivery_m3s / n as f64;
    }
    if is_transport_compressor(pipe) {
        if let Some(demands) = demands {
            let subtree = compressor_subtree_delivery_m3s(network, pipe, demands, demand_scale);
            if subtree > 1e-6 {
                return subtree;
            }
            let adjacent = adjacent_sink_delivery_m3s(network, pipe, demands, demand_scale);
            if adjacent > 1e-6 {
                return adjacent;
            }
            let peer = distribution_peer_flow_m3s(network, pipe, demands, demand_scale);
            if peer > 1e-6 {
                return peer;
            }
        }
        let n = count_distribution_compressors(network).max(1);
        return total_delivery_m3s / n as f64;
    }
    if active_compressors == 0 {
        return 0.0;
    }
    total_delivery_m3s / active_compressors as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EquipmentSpec, Node};

    fn add_cs(
        net: &mut GasNetwork,
        id: &str,
        from: &str,
        to: &str,
        cap: f64,
    ) {
        for node_id in [from, to] {
            if net.node_index(node_id).is_none() {
                net.add_node(Node {
                    id: node_id.into(),
                    ..Default::default()
                });
            }
        }
        net.add_pipe(Pipe {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            kind: ConnectionKind::CompressorStation,
            equipment: EquipmentSpec {
                compressor_pressure_cap_ratio: Some(cap),
                ..Default::default()
            },
            ..Default::default()
        });
    }

    fn add_link(net: &mut GasNetwork, from: &str, to: &str, id: &str) {
        for node_id in [from, to] {
            if net.node_index(node_id).is_none() {
                net.add_node(Node {
                    id: node_id.into(),
                    ..Default::default()
                });
            }
        }
        net.add_pipe(Pipe {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            kind: ConnectionKind::Valve,
            ..Default::default()
        });
    }

    #[test]
    fn downstream_bounded_sinks_do_not_cross_compressor() {
        let mut net = GasNetwork::new();
        add_cs(&mut net, "CS_A", "src", "mid", 4.0);
        add_link(&mut net, "mid", "sink_local", "v_local");
        add_cs(&mut net, "CS_B", "mid", "far", 4.0);
        add_link(&mut net, "far", "sink_far", "v_far");

        if let Some(node) = net.node_mut("sink_local") {
            node.pressure_lower_bar = Some(41.0);
        }
        if let Some(node) = net.node_mut("sink_far") {
            node.pressure_lower_bar = Some(55.0);
        }
        net.scenario_pressure_envelope_nodes
            .insert("sink_local".into());
        net.scenario_pressure_envelope_nodes.insert("sink_far".into());

        let sinks = downstream_bounded_sinks(&net, "mid");
        assert_eq!(sinks, vec![("sink_local".into(), 41.0)]);
    }

    #[test]
    fn hub_merger_gets_total_delivery_branch_gets_split() {
        let mut net = GasNetwork::new();
        // CS2/CS3 branches → hub innode_14 → CS1 (582-like)
        add_cs(&mut net, "CS2", "innode_12", "innode_13", 4.09);
        add_cs(&mut net, "CS3", "innode_11", "innode_10", 4.09);
        add_cs(&mut net, "CS1", "innode_14", "innode_389", 4.09);
        add_link(&mut net, "innode_13", "innode_14", "v25");
        add_link(&mut net, "innode_10", "innode_14", "v1");

        let total = 90.0;
        let cs1 = net.pipes().find(|p| p.id == "CS1").unwrap();
        let cs2 = net.pipes().find(|p| p.id == "CS2").unwrap();
        let cs3 = net.pipes().find(|p| p.id == "CS3").unwrap();

        assert!((estimated_map_flow_m3s(&net, cs1, total, 3, None, 1.0) - 90.0).abs() < 1e-9);
        assert!((estimated_map_flow_m3s(&net, cs2, total, 3, None, 1.0) - 45.0).abs() < 1e-9);
        assert!((estimated_map_flow_m3s(&net, cs3, total, 3, None, 1.0) - 45.0).abs() < 1e-9);
    }

    #[test]
    fn distribution_compressor_uses_subtree_delivery_not_total_split() {
        let mut net = GasNetwork::new();
        for node_id in ["n_in", "n_out", "sink_a"] {
            net.add_node(Node {
                id: node_id.into(),
                ..Default::default()
            });
        }
        add_cs(&mut net, "CS4", "n_in", "n_out", 2.1);
        add_link(&mut net, "n_out", "sink_a", "p1");
        let mut demands = HashMap::new();
        demands.insert("sink_a".into(), -12.0);

        let cs4 = net.pipes().find(|p| p.id == "CS4").unwrap();
        let q = estimated_map_flow_m3s(&net, cs4, 90.0, 1, Some(&demands), 1.0);
        assert!(
            (q - 12.0).abs() < 1e-9,
            "distribution CS should use local subtree demand, got {q}"
        );
    }

    #[test]
    fn load_gaslib_582_cs1_is_hub_merger_if_present() {
        use std::path::Path;

        use crate::gaslib::load_network;

        let path = Path::new("dat/GasLib-582.net");
        if !path.exists() {
            eprintln!("skip: GasLib-582.net not found");
            return;
        }
        let net = load_network(path).expect("582");
        let cs1 = net.pipes().find(|p| p.id == "compressorStation_1").unwrap();
        let total = 90.13;
        let q = estimated_map_flow_m3s(&net, cs1, total, 5, None, 1.0);
        assert!(
            q > 80.0,
            "CS1 hub should carry near-total delivery, got {q:.2}"
        );
        let cs2 = net.pipes().find(|p| p.id == "compressorStation_2").unwrap();
        let q2 = estimated_map_flow_m3s(&net, cs2, total, 5, None, 1.0);
        assert!(q2 < 60.0 && q2 > 30.0, "CS2 branch expected ~45, got {q2:.2}");

        let scenario_path = Path::new("dat/Nominations-582-v2-20211129/nomination_mild_618.scn");
        if !scenario_path.exists() {
            return;
        }
        let scenario = crate::gaslib::load_scenario_demands(scenario_path).expect("scenario");
        let cs4 = net.pipes().find(|p| p.id == "compressorStation_4").unwrap();
        let cs5 = net.pipes().find(|p| p.id == "compressorStation_5").unwrap();
        let q4 = estimated_map_flow_m3s(&net, cs4, total, 5, Some(&scenario.demands), 1.0);
        let q5 = estimated_map_flow_m3s(&net, cs5, total, 5, Some(&scenario.demands), 1.0);
        assert!(
            q4 < 30.0,
            "CS4 south branch should be well below transport split, got {q4:.2}"
        );
        assert!(
            q5 < 30.0,
            "CS5 south branch should be well below transport split, got {q5:.2}"
        );
    }
}
