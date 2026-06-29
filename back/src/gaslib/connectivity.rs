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

/// Statistiques des composantes connexes du sous-graphe hydrauliquement actif.
///
/// Retourne `(total_components, components_without_fixed_pressure)`.
/// Une composante compte comme « avec pression fixée » si au moins un nœud a
/// `pressure_fixed_bar.is_some()`.
pub fn active_component_stats(network: &GasNetwork) -> (usize, usize) {
    let adj = active_adjacency(network);
    let fixed_pressure: HashSet<String> = network
        .nodes()
        .filter(|n| n.pressure_fixed_bar.is_some())
        .map(|n| n.id.clone())
        .collect();

    let mut all_nodes: HashSet<String> = network.nodes().map(|n| n.id.clone()).collect();
    for id in adj.keys() {
        all_nodes.insert(id.clone());
    }

    let mut visited = HashSet::new();
    let mut total_components = 0usize;
    let mut components_without_fixed = 0usize;

    for start in all_nodes {
        if visited.contains(&start) {
            continue;
        }
        total_components += 1;
        let component = if adj.contains_key(&start) {
            bfs_reachable(&adj, &start)
        } else {
            HashSet::from([start.clone()])
        };
        let has_fixed = component.iter().any(|id| fixed_pressure.contains(id));
        if !has_fixed {
            components_without_fixed += 1;
        }
        visited.extend(component);
    }

    (total_components, components_without_fixed)
}

/// Vrai si des nœuds à demande non nulle appartiennent à plusieurs composantes actives.
pub fn demands_span_multiple_active_components(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
) -> bool {
    let demand_nodes: Vec<&str> = demands
        .iter()
        .filter(|(_, q)| q.is_finite() && q.abs() > 1e-12)
        .map(|(id, _)| id.as_str())
        .collect();
    if demand_nodes.len() <= 1 {
        return false;
    }

    let adj = active_adjacency(network);
    let mut component_of: HashMap<String, usize> = HashMap::new();
    let mut comp_id = 0usize;
    let mut visited = HashSet::new();

    let mut all_nodes: HashSet<String> = network.nodes().map(|n| n.id.clone()).collect();
    for id in adj.keys() {
        all_nodes.insert(id.clone());
    }

    for start in all_nodes {
        if visited.contains(&start) {
            continue;
        }
        let component = if adj.contains_key(&start) {
            bfs_reachable(&adj, &start)
        } else {
            HashSet::from([start.clone()])
        };
        for id in &component {
            component_of.insert(id.clone(), comp_id);
        }
        visited.extend(component);
        comp_id += 1;
    }

    let mut seen_components = HashSet::new();
    for id in demand_nodes {
        if let Some(&c) = component_of.get(id) {
            seen_components.insert(c);
            if seen_components.len() > 1 {
                return true;
            }
        }
    }
    false
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

    #[test]
    fn active_component_stats_single_component() {
        let mut net = GasNetwork::new();
        for id in ["A", "B", "C"] {
            net.add_node(Node {
                id: id.into(),
                pressure_fixed_bar: if id == "A" { Some(70.0) } else { None },
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

        let (total, without_fixed) = active_component_stats(&net);
        assert_eq!(total, 1);
        assert_eq!(without_fixed, 0);
    }

    #[test]
    fn active_component_stats_fragmented_without_pressure() {
        let mut net = GasNetwork::new();
        for id in ["A", "B", "C", "D"] {
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
            to: "D".into(),
            kind: ConnectionKind::Pipe,
            ..Default::default()
        });

        let (total, without_fixed) = active_component_stats(&net);
        assert_eq!(total, 2);
        assert_eq!(without_fixed, 2);
    }

    #[test]
    fn demands_span_multiple_active_components_detected() {
        let mut net = GasNetwork::new();
        for id in ["A", "B", "C", "D"] {
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
            to: "D".into(),
            kind: ConnectionKind::Pipe,
            ..Default::default()
        });

        let mut demands = HashMap::new();
        demands.insert("A".into(), 1.0);
        demands.insert("C".into(), -1.0);
        assert!(demands_span_multiple_active_components(&net, &demands));
    }
}
