//! Structure de données du réseau gazier basée sur petgraph.

use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub lon: Option<f64>,
    pub lat: Option<f64>,
    pub height_m: f64,
    pub pressure_lower_bar: Option<f64>,
    pub pressure_upper_bar: Option<f64>,
    pub pressure_fixed_bar: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionKind {
    Pipe,
    Valve,
    ShortPipe,
    CompressorStation,
}

#[derive(Debug, Clone, Serialize)]
pub struct Pipe {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: ConnectionKind,
    pub length_km: f64,
    pub diameter_mm: f64,
    pub roughness_mm: f64,
}

/// Réseau gazier : graphe orienté (nœuds + tuyaux).
#[derive(Debug)]
pub struct GasNetwork {
    pub graph: DiGraph<Node, Pipe>,
    id_to_index: HashMap<String, NodeIndex>,
}

impl GasNetwork {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            id_to_index: HashMap::new(),
        }
    }

    pub fn add_node(&mut self, node: Node) {
        let id = node.id.clone();
        let idx = self.graph.add_node(node);
        self.id_to_index.insert(id, idx);
    }

    pub fn add_pipe(&mut self, pipe: Pipe) {
        if let (Some(&from), Some(&to)) = (
            self.id_to_index.get(&pipe.from),
            self.id_to_index.get(&pipe.to),
        ) {
            self.graph.add_edge(from, to, pipe);
        }
    }

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    pub fn node_index(&self, id: &str) -> Option<NodeIndex> {
        self.id_to_index.get(id).copied()
    }

    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.graph.node_weights()
    }

    pub fn pipes(&self) -> impl Iterator<Item = &Pipe> {
        self.graph.edge_weights()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_simple_network() {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "A".into(),
            x: 0.0,
            y: 0.0,
            lon: Some(10.0),
            lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
        });
        net.add_node(Node {
            id: "B".into(),
            x: 1.0,
            y: 1.0,
            lon: Some(11.0),
            lat: Some(51.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
        });
        net.add_pipe(Pipe {
            id: "P1".into(),
            from: "A".into(),
            to: "B".into(),
            kind: ConnectionKind::Pipe,
            length_km: 50.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
        });

        assert_eq!(net.node_count(), 2);
        assert_eq!(net.edge_count(), 1);
    }
}
