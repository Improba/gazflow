use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use quick_xml::de::from_str;
use serde::Deserialize;

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

#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioDemands {
    pub scenario_id: Option<String>,
    pub demands: HashMap<String, f64>,
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

    let demands = raw
        .scenario
        .nodes
        .iter()
        .filter_map(|node| {
            let magnitude = extract_flow_value(&node.flows)?;
            let sign = match node.node_type.as_deref() {
                Some("entry") => 1.0,
                Some("exit") => -1.0,
                // Cas fallback: on garde la valeur positive si type absent/inconnu.
                _ => 1.0,
            };
            Some((node.id.clone(), sign * magnitude))
        })
        .collect();

    Ok(ScenarioDemands {
        scenario_id: raw.scenario.id,
        demands,
    })
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

        let sum: f64 = parsed.demands.values().sum();
        assert!(
            sum.abs() < 1e-9,
            "scenario should be globally balanced, got sum={sum}"
        );
    }
}
