//! Snapshots et diffs topologiques pour scénarios réseau (P12).

use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use super::{GasNetwork, Node, Pipe};

/// État sérialisable complet d'un réseau (nœuds + conduites).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkSnapshot {
    pub nodes: Vec<Node>,
    pub pipes: Vec<Pipe>,
}

/// Modifications par entité (ajouts, mises à jour, suppressions par id).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EntityDiff<T> {
    #[serde(default)]
    pub added: Vec<T>,
    #[serde(default)]
    pub updated: Vec<T>,
    #[serde(default)]
    pub removed: Vec<String>,
}

impl<T> Default for EntityDiff<T> {
    fn default() -> Self {
        Self {
            added: Vec::new(),
            updated: Vec::new(),
            removed: Vec::new(),
        }
    }
}

/// Diff topologique minimal entre un réseau de référence et une variante.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NetworkDiff {
    #[serde(default)]
    pub nodes: EntityDiff<Node>,
    #[serde(default)]
    pub pipes: EntityDiff<Pipe>,
}

impl Default for NetworkDiff {
    fn default() -> Self {
        Self {
            nodes: EntityDiff::default(),
            pipes: EntityDiff::default(),
        }
    }
}

impl NetworkSnapshot {
    pub fn from_network(network: &GasNetwork) -> Self {
        let mut nodes: Vec<_> = network.nodes().cloned().collect();
        let mut pipes: Vec<_> = network.pipes().cloned().collect();
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        pipes.sort_by(|a, b| a.id.cmp(&b.id));
        Self { nodes, pipes }
    }

    pub fn to_network(self) -> Result<GasNetwork> {
        let mut net = GasNetwork::new();
        for node in self.nodes {
            net.add_node(node);
        }
        for pipe in self.pipes {
            if net.node_index(&pipe.from).is_none() {
                bail!("pipe {}: unknown from node {}", pipe.id, pipe.from);
            }
            if net.node_index(&pipe.to).is_none() {
                bail!("pipe {}: unknown to node {}", pipe.id, pipe.to);
            }
            net.add_pipe(pipe);
        }
        Ok(net)
    }

    pub fn apply_diff(&mut self, diff: &NetworkDiff) {
        let mut nodes: HashMap<String, Node> = self
            .nodes
            .drain(..)
            .map(|n| (n.id.clone(), n))
            .collect();
        let mut pipes: HashMap<String, Pipe> = self
            .pipes
            .drain(..)
            .map(|p| (p.id.clone(), p))
            .collect();

        for id in &diff.nodes.removed {
            nodes.remove(id);
        }
        for node in &diff.nodes.updated {
            nodes.insert(node.id.clone(), node.clone());
        }
        for node in &diff.nodes.added {
            nodes.insert(node.id.clone(), node.clone());
        }

        for id in &diff.pipes.removed {
            pipes.remove(id);
        }
        for pipe in &diff.pipes.updated {
            pipes.insert(pipe.id.clone(), pipe.clone());
        }
        for pipe in &diff.pipes.added {
            pipes.insert(pipe.id.clone(), pipe.clone());
        }

        self.nodes = nodes.into_values().collect();
        self.pipes = pipes.into_values().collect();
        self.nodes.sort_by(|a, b| a.id.cmp(&b.id));
        self.pipes.sort_by(|a, b| a.id.cmp(&b.id));
    }
}

pub fn compute_diff(base: &GasNetwork, variant: &GasNetwork) -> NetworkDiff {
    compute_snapshot_diff(&NetworkSnapshot::from_network(base), &NetworkSnapshot::from_network(variant))
}

pub fn compute_snapshot_diff(base: &NetworkSnapshot, variant: &NetworkSnapshot) -> NetworkDiff {
    let base_nodes: HashMap<_, _> = base.nodes.iter().map(|n| (n.id.clone(), n)).collect();
    let variant_nodes: HashMap<_, _> = variant.nodes.iter().map(|n| (n.id.clone(), n)).collect();
    let base_pipes: HashMap<_, _> = base.pipes.iter().map(|p| (p.id.clone(), p)).collect();
    let variant_pipes: HashMap<_, _> = variant.pipes.iter().map(|p| (p.id.clone(), p)).collect();

    let mut node_diff = EntityDiff::default();
    for (id, node) in &variant_nodes {
        match base_nodes.get(id) {
            None => node_diff.added.push((*node).clone()),
            Some(base_node) if base_node != node => node_diff.updated.push((*node).clone()),
            _ => {}
        }
    }
    for id in base_nodes.keys() {
        if !variant_nodes.contains_key(id) {
            node_diff.removed.push(id.clone());
        }
    }

    let mut pipe_diff = EntityDiff::default();
    for (id, pipe) in &variant_pipes {
        match base_pipes.get(id) {
            None => pipe_diff.added.push((*pipe).clone()),
            Some(base_pipe) if base_pipe != pipe => pipe_diff.updated.push((*pipe).clone()),
            _ => {}
        }
    }
    for id in base_pipes.keys() {
        if !variant_pipes.contains_key(id) {
            pipe_diff.removed.push(id.clone());
        }
    }

    node_diff.removed.sort();
    pipe_diff.removed.sort();

    NetworkDiff {
        nodes: node_diff,
        pipes: pipe_diff,
    }
}

/// Applique un diff sur une copie du réseau de base.
pub fn apply_diff(base: &GasNetwork, diff: &NetworkDiff) -> Result<GasNetwork> {
    let mut snapshot = NetworkSnapshot::from_network(base);
    snapshot.apply_diff(diff);
    snapshot.to_network()
}

/// Vérifie l'absence de collisions id dans un diff.
pub fn validate_diff(diff: &NetworkDiff) -> Result<()> {
    let mut seen = HashSet::new();
    for id in diff
        .nodes
        .added
        .iter()
        .chain(diff.nodes.updated.iter())
        .map(|n| &n.id)
        .chain(diff.nodes.removed.iter())
    {
        if !seen.insert(format!("node:{id}")) {
            bail!("duplicate node id in diff: {id}");
        }
    }
    seen.clear();
    for id in diff
        .pipes
        .added
        .iter()
        .chain(diff.pipes.updated.iter())
        .map(|p| &p.id)
        .chain(diff.pipes.removed.iter())
    {
        if !seen.insert(format!("pipe:{id}")) {
            bail!("duplicate pipe id in diff: {id}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{ConnectionKind, EquipmentSpec};

    fn sample_base_network() -> GasNetwork {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "A".into(),
            x: 0.0,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net.add_node(Node {
            id: "B".into(),
            x: 1.0,
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
        net.add_pipe(Pipe {
            id: "P1".into(),
            from: "A".into(),
            to: "B".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 10.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        net
    }

    fn sample_variant_network() -> GasNetwork {
        let mut net = sample_base_network();
        net.add_node(Node {
            id: "C".into(),
            x: 2.0,
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
        if let Some(node) = net.node_mut("B") {
            node.x = 1.5;
        }
        net.add_pipe(Pipe {
            id: "P2".into(),
            from: "B".into(),
            to: "C".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 5.0,
            diameter_mm: 400.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        net
    }

    #[test]
    fn test_scenario_diff_roundtrip() {
        let base = sample_base_network();
        let variant = sample_variant_network();

        let diff = compute_diff(&base, &variant);
        validate_diff(&diff).expect("valid diff");

        let restored = apply_diff(&base, &diff).expect("apply diff");
        let roundtrip_diff = compute_diff(&variant, &restored);
        assert!(
            roundtrip_diff.nodes.added.is_empty()
                && roundtrip_diff.nodes.updated.is_empty()
                && roundtrip_diff.nodes.removed.is_empty()
                && roundtrip_diff.pipes.added.is_empty()
                && roundtrip_diff.pipes.updated.is_empty()
                && roundtrip_diff.pipes.removed.is_empty(),
            "roundtrip should match variant: {roundtrip_diff:?}"
        );

        let restored_snap = NetworkSnapshot::from_network(&restored);
        let variant_snap = NetworkSnapshot::from_network(&variant);
        assert_eq!(restored_snap, variant_snap);
    }

    #[test]
    fn snapshot_roundtrip_via_serde() {
        let base = sample_base_network();
        let snapshot = NetworkSnapshot::from_network(&base);
        let json = serde_json::to_string(&snapshot).expect("serialize");
        let parsed: NetworkSnapshot = serde_json::from_str(&json).expect("deserialize");
        let rebuilt = parsed.to_network().expect("to_network");
        assert_eq!(NetworkSnapshot::from_network(&base), NetworkSnapshot::from_network(&rebuilt));
    }
}
