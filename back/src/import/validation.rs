use std::collections::{HashMap, HashSet, VecDeque};

use thiserror::Error;

use crate::graph::RawNetwork;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("nœud orphelin: {node_id}")]
    OrphanNode { node_id: String },
    #[error("aucune source à pression imposée (ex. ALIM avec P_CONSIGNE_BAR)")]
    NoSlack,
    #[error("graphe non connexe ({components} composantes)")]
    DisconnectedGraph { components: usize },
    #[error("arc {pipe_id}: nœud amont {node_id} inconnu")]
    UnknownFromNode { pipe_id: String, node_id: String },
    #[error("arc {pipe_id}: nœud aval {node_id} inconnu")]
    UnknownToNode { pipe_id: String, node_id: String },
    #[error("réseau sans nœud")]
    EmptyNetwork,
}

pub fn validate_topology(raw: &RawNetwork) -> Result<(), ValidationError> {
    if raw.nodes.is_empty() {
        return Err(ValidationError::EmptyNetwork);
    }

    let node_ids: HashSet<&str> = raw.nodes.iter().map(|n| n.id.as_str()).collect();

    for pipe in &raw.pipes {
        if !node_ids.contains(pipe.from.as_str()) {
            return Err(ValidationError::UnknownFromNode {
                pipe_id: pipe.id.clone(),
                node_id: pipe.from.clone(),
            });
        }
        if !node_ids.contains(pipe.to.as_str()) {
            return Err(ValidationError::UnknownToNode {
                pipe_id: pipe.id.clone(),
                node_id: pipe.to.clone(),
            });
        }
    }

    let mut incident: HashMap<&str, usize> = HashMap::new();
    for pipe in &raw.pipes {
        *incident.entry(pipe.from.as_str()).or_default() += 1;
        *incident.entry(pipe.to.as_str()).or_default() += 1;
    }
    for node in &raw.nodes {
        if incident.get(node.id.as_str()).copied().unwrap_or(0) == 0 {
            return Err(ValidationError::OrphanNode {
                node_id: node.id.clone(),
            });
        }
    }

    let has_slack = raw.nodes.iter().any(|n| n.pressure_fixed_bar.is_some());
    if !has_slack {
        return Err(ValidationError::NoSlack);
    }

    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for id in &node_ids {
        adj.entry(id).or_default();
    }
    for pipe in &raw.pipes {
        adj.entry(pipe.from.as_str())
            .or_default()
            .push(pipe.to.as_str());
        adj.entry(pipe.to.as_str())
            .or_default()
            .push(pipe.from.as_str());
    }

    let start = raw.nodes[0].id.as_str();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::from([start]);
    while let Some(u) = queue.pop_front() {
        if !visited.insert(u) {
            continue;
        }
        if let Some(neighbors) = adj.get(u) {
            for v in neighbors {
                if !visited.contains(v) {
                    queue.push_back(v);
                }
            }
        }
    }

    if visited.len() != node_ids.len() {
        let components = count_components(&adj, &node_ids);
        return Err(ValidationError::DisconnectedGraph { components });
    }

    Ok(())
}

/// Validation allégée pour l'édition incrémentale du réseau.
///
/// Contrairement à `validate_topology`, cette vérification n'impose pas
/// connectivité globale ni absence de nœuds orphelins afin de permettre les
/// états intermédiaires pendant des opérations CRUD.
pub fn validate_network_incremental(raw: &RawNetwork) -> Result<(), ValidationError> {
    if raw.nodes.is_empty() {
        return Err(ValidationError::EmptyNetwork);
    }

    let node_ids: HashSet<&str> = raw.nodes.iter().map(|n| n.id.as_str()).collect();
    for pipe in &raw.pipes {
        if !node_ids.contains(pipe.from.as_str()) {
            return Err(ValidationError::UnknownFromNode {
                pipe_id: pipe.id.clone(),
                node_id: pipe.from.clone(),
            });
        }
        if !node_ids.contains(pipe.to.as_str()) {
            return Err(ValidationError::UnknownToNode {
                pipe_id: pipe.id.clone(),
                node_id: pipe.to.clone(),
            });
        }
    }

    let has_slack = raw.nodes.iter().any(|n| n.pressure_fixed_bar.is_some());
    if !has_slack {
        return Err(ValidationError::NoSlack);
    }

    Ok(())
}

fn count_components(adj: &HashMap<&str, Vec<&str>>, nodes: &HashSet<&str>) -> usize {
    let mut visited = HashSet::new();
    let mut components = 0;
    for &n in nodes {
        if visited.contains(n) {
            continue;
        }
        components += 1;
        let mut queue = VecDeque::from([n]);
        while let Some(u) = queue.pop_front() {
            if !visited.insert(u) {
                continue;
            }
            if let Some(neighbors) = adj.get(u) {
                for v in neighbors {
                    if !visited.contains(v) {
                        queue.push_back(v);
                    }
                }
            }
        }
    }
    components
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{ConnectionKind, EquipmentSpec, RawNetwork, RawNode, RawNodeRole, RawPipe};

    fn sample_valid() -> RawNetwork {
        RawNetwork {
            nodes: vec![
                RawNode {
                    id: "S".into(),
                    role: RawNodeRole::Source,
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
                },
                RawNode {
                    id: "D".into(),
                    role: RawNodeRole::Sink,
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
                },
            ],
            pipes: vec![RawPipe {
                id: "P".into(),
                from: "S".into(),
                to: "D".into(),
                kind: ConnectionKind::Pipe,
                is_open: true,
                length_km: 1.0,
                diameter_mm: 500.0,
                roughness_mm: 0.05,
                compressor_ratio_max: None,
                flow_min_m3s: None,
                flow_max_m3s: None,
                equipment: EquipmentSpec::default(),
            }],
            source: None,
        }
    }

    #[test]
    fn valid_network_passes() {
        validate_topology(&sample_valid()).expect("valid");
    }

    #[test]
    fn orphan_node_detected() {
        let mut raw = sample_valid();
        raw.nodes.push(RawNode {
            id: "ORPHAN".into(),
            role: RawNodeRole::Innode,
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
        assert!(matches!(
            validate_topology(&raw),
            Err(ValidationError::OrphanNode { .. })
        ));
    }

    #[test]
    fn no_slack_detected() {
        let mut raw = sample_valid();
        raw.nodes[0].pressure_fixed_bar = None;
        assert_eq!(validate_topology(&raw), Err(ValidationError::NoSlack));
    }

    #[test]
    fn empty_network_rejected() {
        let raw = RawNetwork {
            nodes: vec![],
            pipes: vec![],
            source: None,
        };
        assert_eq!(validate_topology(&raw), Err(ValidationError::EmptyNetwork));
    }

    #[test]
    fn unknown_pipe_endpoint_rejected() {
        let mut raw = sample_valid();
        raw.pipes.push(RawPipe {
            id: "P2".into(),
            from: "S".into(),
            to: "MISSING".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 1.0,
            diameter_mm: 500.0,
            roughness_mm: 0.05,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        assert!(matches!(
            validate_topology(&raw),
            Err(ValidationError::UnknownToNode { pipe_id, node_id })
            if pipe_id == "P2" && node_id == "MISSING"
        ));
    }

    #[test]
    fn disconnected_graph_rejected() {
        let raw = RawNetwork {
            nodes: vec![
                RawNode {
                    id: "S1".into(),
                    role: RawNodeRole::Source,
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
                },
                RawNode {
                    id: "D1".into(),
                    role: RawNodeRole::Sink,
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
                },
                RawNode {
                    id: "S2".into(),
                    role: RawNodeRole::Source,
                    x: 2.0,
                    y: 0.0,
                    lon: None,
                    lat: None,
                    height_m: 0.0,
                    pressure_lower_bar: None,
                    pressure_upper_bar: None,
                    pressure_fixed_bar: Some(60.0),
                    flow_min_m3s: None,
                    flow_max_m3s: None,
                },
                RawNode {
                    id: "D2".into(),
                    role: RawNodeRole::Sink,
                    x: 3.0,
                    y: 0.0,
                    lon: None,
                    lat: None,
                    height_m: 0.0,
                    pressure_lower_bar: None,
                    pressure_upper_bar: None,
                    pressure_fixed_bar: None,
                    flow_min_m3s: None,
                    flow_max_m3s: None,
                },
            ],
            pipes: vec![
                RawPipe {
                    id: "P1".into(),
                    from: "S1".into(),
                    to: "D1".into(),
                    kind: ConnectionKind::Pipe,
                    is_open: true,
                    length_km: 1.0,
                    diameter_mm: 500.0,
                    roughness_mm: 0.05,
                    compressor_ratio_max: None,
                    flow_min_m3s: None,
                    flow_max_m3s: None,
                    equipment: EquipmentSpec::default(),
                },
                RawPipe {
                    id: "P2".into(),
                    from: "S2".into(),
                    to: "D2".into(),
                    kind: ConnectionKind::Pipe,
                    is_open: true,
                    length_km: 1.0,
                    diameter_mm: 500.0,
                    roughness_mm: 0.05,
                    compressor_ratio_max: None,
                    flow_min_m3s: None,
                    flow_max_m3s: None,
                    equipment: EquipmentSpec::default(),
                },
            ],
            source: None,
        };
        assert!(matches!(
            validate_topology(&raw),
            Err(ValidationError::DisconnectedGraph { components: 2 })
        ));
    }

    #[test]
    fn incremental_allows_orphan_nodes() {
        let mut raw = sample_valid();
        raw.nodes.push(RawNode {
            id: "ORPHAN".into(),
            role: RawNodeRole::Innode,
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
        validate_network_incremental(&raw).expect("incremental valid");
    }

    #[test]
    fn incremental_rejects_unknown_pipe_endpoint() {
        let mut raw = sample_valid();
        raw.pipes.push(RawPipe {
            id: "P2".into(),
            from: "S".into(),
            to: "MISSING".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 1.0,
            diameter_mm: 500.0,
            roughness_mm: 0.05,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        assert!(matches!(
            validate_network_incremental(&raw),
            Err(ValidationError::UnknownToNode { pipe_id, node_id })
            if pipe_id == "P2" && node_id == "MISSING"
        ));
    }
}
