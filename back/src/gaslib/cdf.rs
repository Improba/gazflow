//! Parseur des décisions combinées GasLib (`.cdf`) et application au réseau.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::graph::{ConnectionKind, GasNetwork, Pipe};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CdfElementKind {
    Valve,
    ControlValve,
    CompressorStation,
}

#[derive(Debug, Clone)]
pub struct CdfElementState {
    pub id: String,
    pub kind: CdfElementKind,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct CdfDecision {
    pub id: String,
    pub elements: Vec<CdfElementState>,
}

#[derive(Debug, Clone)]
pub struct CdfDecisionGroup {
    pub id: String,
    pub decisions: Vec<CdfDecision>,
}

#[derive(Debug, Clone)]
pub struct CombinedDecisions {
    pub groups: Vec<CdfDecisionGroup>,
}

pub fn load_combined_decisions<P: AsRef<Path>>(path: P) -> Result<CombinedDecisions> {
    let raw = std::fs::read_to_string(path.as_ref())
        .with_context(|| format!("lecture de {:?}", path.as_ref()))?;
    parse_combined_decisions_from_str(&raw)
}

pub fn cdf_path_for_network(net_path: &Path) -> Option<std::path::PathBuf> {
    let direct = net_path.with_extension("cdf");
    if direct.exists() {
        return Some(direct);
    }
    // Suivre le lien symbolique `.net` → fichier versionné (ex. GasLib-582-v2-….net).
    if let Some(parent) = net_path.parent() {
        if let Ok(link) = std::fs::read_link(net_path) {
            let resolved_net = parent.join(link);
            let sibling_cdf = resolved_net.with_extension("cdf");
            if sibling_cdf.exists() {
                return Some(sibling_cdf);
            }
        }
    }
    None
}

/// Applique une décision par groupe (une entrée par `CdfDecisionGroup`).
pub fn apply_cdf_decisions(network: &mut GasNetwork, decisions: &[&CdfDecision]) {
    for decision in decisions {
        for element in &decision.elements {
            apply_element_state(network, element);
        }
    }
}

pub fn apply_cdf_decision_ids(cdf: &CombinedDecisions, network: &mut GasNetwork, ids: &[&str]) {
    let selected = select_decisions_by_id(cdf, ids);
    apply_cdf_decisions(network, &selected);
}

fn select_decisions_by_id<'a>(
    cdf: &'a CombinedDecisions,
    ids: &[&str],
) -> Vec<&'a CdfDecision> {
    let id_to_decision: HashMap<&str, &CdfDecision> = cdf
        .groups
        .iter()
        .flat_map(|group| group.decisions.iter())
        .map(|d| (d.id.as_str(), d))
        .collect();

    ids.iter()
        .filter_map(|id| id_to_decision.get(id).copied())
        .collect()
}

fn apply_element_state(network: &mut GasNetwork, element: &CdfElementState) {
    let Some(pipe) = network.pipe_mut(&element.id) else {
        return;
    };

    match element.kind {
        CdfElementKind::Valve => {
            pipe.is_open = element.active;
        }
        CdfElementKind::ControlValve => {
            pipe.is_open = element.active;
            pipe.equipment.control_valve_opening_pct = Some(if element.active { 100.0 } else { 0.0 });
        }
        CdfElementKind::CompressorStation => {
            if element.active {
                pipe.is_open = true;
                if let Some(ratio) = pipe.equipment.compressor_nominal_ratio {
                    pipe.compressor_ratio_max = Some(ratio);
                }
            } else if pipe
                .equipment
                .internal_bypass_required
                .unwrap_or(false)
            {
                pipe.is_open = true;
                pipe.compressor_ratio_max = Some(1.0);
            } else {
                pipe.is_open = false;
            }
        }
    }
}

fn parse_combined_decisions_from_str(raw: &str) -> Result<CombinedDecisions> {
    let mut reader = Reader::from_str(raw);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut groups = Vec::new();
    let mut current_group: Option<CdfDecisionGroup> = None;
    let mut current_decision: Option<CdfDecision> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = local_name(e.name().as_ref());
                match name.as_str() {
                    "decisiongroup" => {
                        current_group = Some(CdfDecisionGroup {
                            id: read_attr_string(&reader, &e, "id").unwrap_or_default(),
                            decisions: Vec::new(),
                        });
                    }
                    "decision" => {
                        current_decision = Some(CdfDecision {
                            id: read_attr_string(&reader, &e, "id").unwrap_or_default(),
                            elements: Vec::new(),
                        });
                    }
                    "valve" => {
                        push_element(&mut current_decision, &reader, &e, CdfElementKind::Valve);
                    }
                    "controlvalve" => {
                        push_element(
                            &mut current_decision,
                            &reader,
                            &e,
                            CdfElementKind::ControlValve,
                        );
                    }
                    "compressorstation" => {
                        push_element(
                            &mut current_decision,
                            &reader,
                            &e,
                            CdfElementKind::CompressorStation,
                        );
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = local_name(e.name().as_ref());
                match name.as_str() {
                    "decision" => {
                        if let (Some(group), Some(decision)) =
                            (current_group.as_mut(), current_decision.take())
                        {
                            group.decisions.push(decision);
                        }
                    }
                    "decisiongroup" => {
                        if let Some(group) = current_group.take() {
                            groups.push(group);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(err) => return Err(err).with_context(|| "parsing XML combined decisions (.cdf)"),
        }
        buf.clear();
    }

    Ok(CombinedDecisions { groups })
}

fn push_element(
    current_decision: &mut Option<CdfDecision>,
    reader: &Reader<&[u8]>,
    e: &quick_xml::events::BytesStart<'_>,
    kind: CdfElementKind,
) {
    let Some(decision) = current_decision.as_mut() else {
        return;
    };
    let id = read_attr_string(reader, e, "id").unwrap_or_default();
    let active = read_attr_string(reader, e, "value")
        .and_then(|v| v.parse::<i32>().ok())
        .is_some_and(|v| v != 0);
    decision.elements.push(CdfElementState { id, kind, active });
}

fn local_name(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw).to_ascii_lowercase();
    s.rsplit(':').next().unwrap_or(&s).to_string()
}

fn read_attr_string(
    reader: &Reader<&[u8]>,
    e: &quick_xml::events::BytesStart<'_>,
    key: &str,
) -> Option<String> {
    e.attributes().flatten().find_map(|attr| {
        let attr_key = local_name(attr.key.as_ref());
        if attr_key != key.to_ascii_lowercase() {
            return None;
        }
        attr.decode_and_unescape_value(reader.decoder())
            .ok()
            .map(|v| v.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_parse_gaslib_582_cdf() {
        let path = Path::new("dat/GasLib-582.cdf");
        if !path.exists() {
            eprintln!("skip: {:?} not found", path);
            return;
        }
        let cdf = load_combined_decisions(path).expect("load cdf");
        assert_eq!(cdf.groups.len(), 2);
        assert!(cdf.groups[0].decisions.iter().any(|d| d.id == "d1"));
        assert!(cdf.groups[1].decisions.iter().any(|d| d.id == "d1_1"));
    }

    #[test]
    fn test_apply_cdf_decision_closes_unused_control_valves() {
        let xml = r#"<?xml version="1.0"?>
<combinedDecisions>
  <decisionGroup id="g1">
    <decision id="d1">
      <controlValve id="CV1" value="0"/>
      <valve id="V1" value="1"/>
    </decision>
  </decisionGroup>
</combinedDecisions>"#;
        let cdf = parse_combined_decisions_from_str(xml).expect("parse");
        let mut net = GasNetwork::new();
        net.add_node(crate::graph::Node {
            id: "A".into(),
            ..Default::default()
        });
        net.add_node(crate::graph::Node {
            id: "B".into(),
            ..Default::default()
        });
        net.add_pipe(Pipe {
            id: "CV1".into(),
            from: "A".into(),
            to: "B".into(),
            kind: ConnectionKind::ControlValve,
            is_open: true,
            equipment: crate::graph::EquipmentSpec::control_valve(100.0, 100.0),
            ..Default::default()
        });
        net.add_pipe(Pipe {
            id: "V1".into(),
            from: "A".into(),
            to: "B".into(),
            kind: ConnectionKind::Valve,
            is_open: true,
            ..Default::default()
        });

        apply_cdf_decisions(&mut net, &[&cdf.groups[0].decisions[0]]);
        let cv = net.pipes().find(|p| p.id == "CV1").expect("cv");
        assert!(!cv.hydraulically_active());
        let valve = net.pipes().find(|p| p.id == "V1").expect("valve");
        assert!(valve.is_open);
    }
}
