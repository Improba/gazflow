//! Vérifications de connectivité pour le routage transport.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::graph::GasNetwork;

/// Vérifie que le sous-graphe hydrauliquement actif connecte tous les nœuds
/// impliqués dans le scénario (demandes + pressions fixées).
pub fn routing_supports_demands(network: &GasNetwork, demands: &HashMap<String, f64>) -> bool {
    let mut required: HashSet<String> = demands
        .iter()
        .filter(|(_, q)| q.is_finite() && q.abs() > 1e-12)
        .map(|(id, _)| id.clone())
        .collect();

    for node in network.nodes() {
        if node.pressure_fixed_bar.is_some() {
            required.insert(node.id.clone());
        }
    }

    if required.is_empty() {
        return true;
    }

    let adj = active_adjacency(network);
    if adj.is_empty() {
        return false;
    }

    let start = required.iter().next().expect("non-empty required");
    let reachable = bfs_reachable(&adj, start);

    required.iter().all(|id| reachable.contains(id))
}

fn active_adjacency(network: &GasNetwork) -> HashMap<String, Vec<String>> {
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for pipe in network.pipes().filter(|p| p.hydraulically_active()) {
        adj.entry(pipe.from.clone())
            .or_default()
            .push(pipe.to.clone());
        adj.entry(pipe.to.clone())
            .or_default()
            .push(pipe.from.clone());
    }
    adj
}

fn bfs_reachable(adj: &HashMap<String, Vec<String>>, start: &str) -> HashSet<String> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::from([start.to_string()]);
    while let Some(u) = queue.pop_front() {
        if !visited.insert(u.clone()) {
            continue;
        }
        if let Some(neighbors) = adj.get(&u) {
            for v in neighbors {
                if !visited.contains(v) {
                    queue.push_back(v.clone());
                }
            }
        }
    }
    visited
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{ConnectionKind, Node, Pipe};

    #[test]
    fn disconnected_active_subgraph_rejected() {
        let mut net = GasNetwork::new();
        for id in ["A", "B", "C"] {
            net.add_node(Node {
                id: id.into(),
                ..Default::default()
            });
        }
        net.add_pipe(Pipe {
            id: "P1".into(),
            from: "A".into(),
            to: "B".into(),
            kind: ConnectionKind::Pipe,
            ..Default::default()
        });
        net.add_pipe(Pipe {
            id: "P2".into(),
            from: "C".into(),
            to: "C".into(),
            kind: ConnectionKind::Valve,
            is_open: false,
            ..Default::default()
        });

        let mut demands = HashMap::new();
        demands.insert("A".into(), 1.0);
        demands.insert("C".into(), -1.0);
        assert!(!routing_supports_demands(&net, &demands));
    }

    #[test]
    fn connected_subgraph_accepted() {
        let mut net = GasNetwork::new();
        for id in ["A", "B", "C"] {
            net.add_node(Node {
                id: id.into(),
                ..Default::default()
            });
        }
        net.add_pipe(Pipe {
            id: "P1".into(),
            from: "A".into(),
            to: "B".into(),
            kind: ConnectionKind::Pipe,
            ..Default::default()
        });
        net.add_pipe(Pipe {
            id: "P2".into(),
            from: "B".into(),
            to: "C".into(),
            kind: ConnectionKind::Pipe,
            ..Default::default()
        });

        let mut demands = HashMap::new();
        demands.insert("A".into(), 1.0);
        demands.insert("C".into(), -1.0);
        assert!(routing_supports_demands(&net, &demands));
    }
}
