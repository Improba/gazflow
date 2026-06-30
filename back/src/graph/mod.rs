//! Structure de données du réseau gazier basée sur petgraph.

pub mod equipment;
mod raw;
pub mod scenarios;

use std::collections::HashMap;

use anyhow::{Result, bail};
use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};

use crate::compressor::CompressorCatalog;

pub use equipment::EquipmentSpec;
pub use raw::{RawNetwork, RawNode, RawNodeRole, RawPipe};
pub use scenarios::{NetworkDiff, NetworkSnapshot, apply_diff, compute_diff};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    pub flow_min_m3s: Option<f64>,
    pub flow_max_m3s: Option<f64>,
}

impl Default for Node {
    fn default() -> Self {
        Self {
            id: String::new(),
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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionKind {
    Pipe,
    Valve,
    ShortPipe,
    Resistor,
    CompressorStation,
    /// Détendeur / régulateur à consigne aval (P8).
    PressureRegulator,
    /// Vanne de régulation à Cv variable (P8).
    ControlValve,
    /// Poste de livraison (détendeur + contrainte P_min) (P8).
    DeliveryStation,
}

impl ConnectionKind {
    pub fn from_label(label: &str) -> Self {
        match label.to_ascii_lowercase().as_str() {
            "regulator" | "detendeur" | "détendeur" | "pressure_regulator"
            | "pressureregulator" | "reg" | "prv" => Self::PressureRegulator,
            "control_valve" | "controlvalve" | "vanne_regulation" | "vanne_cv" | "cv" => {
                Self::ControlValve
            }
            "delivery_station" | "deliverystation" | "poste_livraison" | "poste" | "pdl_poste" => {
                Self::DeliveryStation
            }
            "valve" | "vanne" => Self::Valve,
            "shortpipe" | "short_pipe" | "liaison" => Self::ShortPipe,
            "resistor" => Self::Resistor,
            "compressor" | "compressor_station" | "compressorstation" | "compresseur" => {
                Self::CompressorStation
            }
            "pipe" | "tuyau" | "canalisation" => Self::Pipe,
            _ => Self::Pipe,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Pipe {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: ConnectionKind,
    pub is_open: bool,
    pub length_km: f64,
    pub diameter_mm: f64,
    pub roughness_mm: f64,
    #[serde(skip_serializing)]
    pub compressor_ratio_max: Option<f64>,
    #[serde(skip_serializing)]
    pub flow_min_m3s: Option<f64>,
    #[serde(skip_serializing)]
    pub flow_max_m3s: Option<f64>,
    #[serde(default, skip_serializing_if = "EquipmentSpec::is_empty")]
    pub equipment: EquipmentSpec,
}

impl Default for Pipe {
    fn default() -> Self {
        Self {
            id: String::new(),
            from: String::new(),
            to: String::new(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 0.0,
            diameter_mm: 0.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        }
    }
}

impl Pipe {
    /// Arc participant au bilan nodal (vanne fermée ou ouverture Cv nulle exclues).
    pub fn hydraulically_active(&self) -> bool {
        if !self.is_open {
            return false;
        }
        if self.kind == ConnectionKind::ControlValve {
            let opening = self.equipment.control_valve_opening_pct.unwrap_or(100.0);
            return opening > 0.0;
        }
        true
    }
}

/// Réseau gazier : graphe orienté (nœuds + tuyaux).
#[derive(Debug, Clone)]
pub struct GasNetwork {
    pub graph: DiGraph<Node, Pipe>,
    id_to_index: HashMap<String, NodeIndex>,
    pub compressor_catalog: Option<CompressorCatalog>,
}

impl Default for GasNetwork {
    fn default() -> Self {
        Self::new()
    }
}

impl GasNetwork {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            id_to_index: HashMap::new(),
            compressor_catalog: None,
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

    pub fn node_mut(&mut self, id: &str) -> Option<&mut Node> {
        self.id_to_index
            .get(id)
            .copied()
            .and_then(|idx| self.graph.node_weight_mut(idx))
    }

    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.graph.node_weights()
    }

    pub fn pipes(&self) -> impl Iterator<Item = &Pipe> {
        self.graph.edge_weights()
    }

    pub fn pipes_mut(&mut self) -> impl Iterator<Item = &mut Pipe> {
        self.graph.edge_weights_mut()
    }

    pub fn pipe_mut(&mut self, id: &str) -> Option<&mut Pipe> {
        self.graph.edge_weights_mut().find(|pipe| pipe.id == id)
    }

    /// Applique des surcharges d'équipement par identifiant de conduite (simulation).
    pub fn apply_equipment_overrides(
        &mut self,
        overrides: &std::collections::HashMap<String, EquipmentSpec>,
    ) {
        for idx in self.graph.edge_indices() {
            if let Some(pipe) = self.graph.edge_weight_mut(idx)
                && let Some(patch) = overrides.get(&pipe.id)
            {
                pipe.equipment.merge_from(patch);
            }
        }
    }

    /// Construit un `GasNetwork` depuis le modèle intermédiaire d'import.
    pub fn from_raw(raw: RawNetwork) -> Result<Self> {
        let mut net = Self::new();
        for node in raw.nodes {
            net.add_node(Node {
                id: node.id,
                x: node.x,
                y: node.y,
                lon: node.lon,
                lat: node.lat,
                height_m: node.height_m,
                pressure_lower_bar: node.pressure_lower_bar,
                pressure_upper_bar: node.pressure_upper_bar,
                pressure_fixed_bar: node.pressure_fixed_bar,
                flow_min_m3s: node.flow_min_m3s,
                flow_max_m3s: node.flow_max_m3s,
            });
        }
        for pipe in raw.pipes {
            if net.node_index(&pipe.from).is_none() {
                bail!("pipe {}: nœud amont {} inconnu", pipe.id, pipe.from);
            }
            if net.node_index(&pipe.to).is_none() {
                bail!("pipe {}: nœud aval {} inconnu", pipe.id, pipe.to);
            }
            net.add_pipe(Pipe {
                id: pipe.id,
                from: pipe.from,
                to: pipe.to,
                kind: pipe.kind,
                is_open: pipe.is_open,
                length_km: pipe.length_km,
                diameter_mm: pipe.diameter_mm,
                roughness_mm: pipe.roughness_mm,
                compressor_ratio_max: pipe.compressor_ratio_max,
                flow_min_m3s: pipe.flow_min_m3s,
                flow_max_m3s: pipe.flow_max_m3s,
                equipment: pipe.equipment,
            });
        }
        net.compressor_catalog = raw.compressor_catalog;
        Ok(net)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::EquipmentSpec;

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
            flow_min_m3s: None,
            flow_max_m3s: None,
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
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net.add_pipe(Pipe {
            id: "P1".into(),
            from: "A".into(),
            to: "B".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 50.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });

        assert_eq!(net.node_count(), 2);
        assert_eq!(net.edge_count(), 1);
    }

    #[test]
    fn from_raw_preserves_node_heights_and_slack() {
        use crate::graph::raw::{RawNetwork, RawNode, RawNodeRole, RawPipe};

        let raw = RawNetwork {
            nodes: vec![
                RawNode {
                    id: "UP".into(),
                    role: RawNodeRole::Source,
                    x: 1.0,
                    y: 45.0,
                    lon: Some(1.0),
                    lat: Some(45.0),
                    height_m: 200.0,
                    pressure_lower_bar: None,
                    pressure_upper_bar: None,
                    pressure_fixed_bar: Some(65.0),
                    flow_min_m3s: None,
                    flow_max_m3s: None,
                },
                RawNode {
                    id: "DOWN".into(),
                    role: RawNodeRole::Sink,
                    x: 1.01,
                    y: 45.0,
                    lon: Some(1.01),
                    lat: Some(45.0),
                    height_m: 50.0,
                    pressure_lower_bar: None,
                    pressure_upper_bar: None,
                    pressure_fixed_bar: None,
                    flow_min_m3s: None,
                    flow_max_m3s: None,
                },
            ],
            pipes: vec![RawPipe {
                id: "P1".into(),
                from: "UP".into(),
                to: "DOWN".into(),
                kind: ConnectionKind::Pipe,
                is_open: true,
                length_km: 5.0,
                diameter_mm: 600.0,
                roughness_mm: 0.05,
                compressor_ratio_max: None,
                flow_min_m3s: None,
                flow_max_m3s: None,
                equipment: EquipmentSpec::default(),
            }],
            source: Some("test".into()),
            compressor_catalog: None,
        };

        let net = GasNetwork::from_raw(raw).expect("from_raw");
        let up = net.nodes().find(|n| n.id == "UP").expect("UP");
        let down = net.nodes().find(|n| n.id == "DOWN").expect("DOWN");
        assert!((up.height_m - 200.0).abs() < 1e-9);
        assert!((down.height_m - 50.0).abs() < 1e-9);
        assert_eq!(up.pressure_fixed_bar, Some(65.0));
    }
}
