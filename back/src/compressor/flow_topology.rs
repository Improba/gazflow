//! Estimation topologique du débit compresseur pour la carte (.cs).

use std::collections::{HashMap, HashSet, VecDeque};

use petgraph::visit::EdgeRef;

use crate::graph::{ConnectionKind, GasNetwork, Pipe};

const TOPOLOGY_REACH_MAX_HOPS: usize = 12;
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

/// Débit normal estimé pour l'évaluation carte, en tenant compte de la topologie hub/branche.
pub fn estimated_map_flow_m3s(
    network: &GasNetwork,
    pipe: &Pipe,
    total_delivery_m3s: f64,
    active_compressors: usize,
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

        assert!((estimated_map_flow_m3s(&net, cs1, total, 3) - 90.0).abs() < 1e-9);
        assert!((estimated_map_flow_m3s(&net, cs2, total, 3) - 45.0).abs() < 1e-9);
        assert!((estimated_map_flow_m3s(&net, cs3, total, 3) - 45.0).abs() < 1e-9);
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
        let q = estimated_map_flow_m3s(&net, cs1, total, 5);
        assert!(
            q > 80.0,
            "CS1 hub should carry near-total delivery, got {q:.2}"
        );
        let cs2 = net.pipes().find(|p| p.id == "compressorStation_2").unwrap();
        let q2 = estimated_map_flow_m3s(&net, cs2, total, 5);
        assert!(q2 < 60.0 && q2 > 30.0, "CS2 branch expected ~45, got {q2:.2}");
    }
}
