use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use quick_xml::de::from_str;
use serde::Deserialize;

use crate::graph::GasNetwork;

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
pub struct ZeroFlowExitAnchor {
    pub node_id: String,
    pub pressure_bar: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioDemands {
    pub scenario_id: Option<String>,
    pub demands: HashMap<String, f64>,
    /// Slack pression implicite (ex. sortie principale avec borne basse seule).
    pub pressure_slack: Option<PressureSlackHint>,
    /// Hubs de balance transport (exits Q≈0 les plus connectés).
    pub balance_hubs: Vec<PressureSlackHint>,
    /// Exits nominalement à Q=0 avec borne pression basse (candidats hub).
    pub zero_flow_exit_anchors: Vec<ZeroFlowExitAnchor>,
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
}

/// Retire la demande imposée sur le nœud slack pression.
///
/// Sur un réseau transport GasLib, le débit au point de référence pression est
/// une inconnue du solveur : imposer P et Q simultanément sur-contrainte le système.
pub fn demands_without_pressure_slack(
    demands: &HashMap<String, f64>,
    scenario: &ScenarioDemands,
) -> HashMap<String, f64> {
    let Some(slack) = scenario.pressure_slack.as_ref() else {
        return demands.clone();
    };
    let mut adjusted = demands.clone();
    adjusted.remove(&slack.node_id);
    adjusted
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

    Ok(ScenarioDemands {
        scenario_id: raw.scenario.id,
        demands,
        pressure_slack: detect_pressure_slack(nodes),
        balance_hubs: Vec::new(),
        zero_flow_exit_anchors: collect_zero_flow_exit_anchors(nodes),
    })
}

fn collect_zero_flow_exit_anchors(nodes: &[XmlScenarioNode]) -> Vec<ZeroFlowExitAnchor> {
    let mut anchors = Vec::new();
    for node in nodes {
        if node.node_type.as_deref() != Some("exit") {
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
        let Some(pressure_bar) = lower else {
            continue;
        };
        anchors.push(ZeroFlowExitAnchor {
            node_id: node.id.clone(),
            pressure_bar,
        });
    }
    anchors
}

/// Choisit les hubs de balance (exits Q≈0 les plus connectés) pour ancrer la pression locale.
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
        .zero_flow_exit_anchors
        .iter()
        .filter(|a| Some(a.node_id.as_str()) != slack_id)
        .filter_map(|node| {
            let degree = network
                .pipes()
                .filter(|p| p.hydraulically_active())
                .filter(|p| p.from == node.node_id || p.to == node.node_id)
                .count();
            if degree == 0 {
                return None;
            }
            Some((
                degree,
                PressureSlackHint {
                    node_id: node.node_id.clone(),
                    pressure_bar: node.pressure_bar,
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

/// Enrichit le scénario avec les hubs de balance détectés sur le réseau chargé.
pub fn enrich_scenario_with_balance_hub(
    network: &GasNetwork,
    scenario: &mut ScenarioDemands,
) {
    scenario.balance_hubs = detect_balance_hubs_for_network(network, scenario, 2);
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
            zero_flow_exit_anchors: Vec::new(),
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
            zero_flow_exit_anchors: Vec::new(),
        };
        let adjusted = demands_without_pressure_slack(&demands, &scenario);
        assert!(!adjusted.contains_key("sink_109"));
    }

    #[test]
    fn test_mild_618_balance_hub_is_sink_2() {
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
