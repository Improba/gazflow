use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use quick_xml::de::from_str;
use serde::Deserialize;

use super::compressor::load_compressor_ratios;
use crate::graph::{ConnectionKind, GasNetwork};

// ---------------------------------------------------------------------------
// Structures XML miroir du schéma GasLib (.net)
//
// GasLib utilise des namespaces XML (ex: <framework:nodes>). quick-xml avec
// serde traite `prefix:local` comme un nom d'élément opaque. On définit
// donc deux variantes pour chaque conteneur (avec et sans préfixe) afin
// de supporter à la fois le format GasLib réel et les tests simplifiés.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename = "network")]
struct XmlNetwork {
    #[serde(alias = "framework:nodes", alias = "nodes")]
    nodes: XmlNodes,
    #[serde(alias = "framework:connections", alias = "connections")]
    connections: XmlConnections,
}

#[derive(Debug, Deserialize)]
struct XmlNodes {
    #[serde(rename = "$value", default)]
    entries: Vec<XmlNode>,
}

/// Un nœud du réseau.  Peut être <node>, <source>, <sink> ou <innode> dans GasLib.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct XmlNode {
    #[serde(rename = "@id")]
    id: String,
    #[serde(rename = "@x")]
    x: f64,
    #[serde(rename = "@y")]
    y: f64,
    #[serde(rename = "@geoWGS84Long", default)]
    lon: Option<f64>,
    #[serde(rename = "@geoWGS84Lat", default)]
    lat: Option<f64>,
    #[serde(rename = "@height", default)]
    height_attr: Option<f64>,
    #[serde(rename = "height", default)]
    height: Option<XmlValue>,
    #[serde(default)]
    pressure: Option<XmlBound>,
    #[serde(rename = "pressureMin", default)]
    pressure_min: Option<XmlValue>,
    #[serde(rename = "pressureMax", default)]
    pressure_max: Option<XmlValue>,
}

#[derive(Debug, Deserialize)]
struct XmlBound {
    #[serde(rename = "@lower", default)]
    lower: Option<f64>,
    #[serde(rename = "@upper", default)]
    upper: Option<f64>,
    #[serde(rename = "@value", default)]
    value: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct XmlValue {
    #[serde(rename = "@value")]
    value: f64,
    #[serde(rename = "@unit", default)]
    unit: Option<String>,
}

#[derive(Debug, Deserialize)]
struct XmlConnections {
    #[serde(rename = "$value", default)]
    entries: Vec<XmlConnection>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct XmlConnectionRaw {
    #[serde(rename = "@id")]
    id: String,
    #[serde(rename = "@from")]
    from: String,
    #[serde(rename = "@to")]
    to: String,
    #[serde(rename = "@length", default)]
    length_km_attr: Option<f64>,
    #[serde(rename = "@diameter", default)]
    diameter_mm_attr: Option<f64>,
    #[serde(rename = "@roughness", default)]
    roughness_mm_attr: Option<f64>,
    #[serde(rename = "length", default)]
    length: Option<XmlValue>,
    #[serde(rename = "diameter", default)]
    diameter: Option<XmlValue>,
    #[serde(rename = "roughness", default)]
    roughness: Option<XmlValue>,
    #[serde(rename = "flowMin", default)]
    flow_min: Option<XmlValue>,
    #[serde(rename = "flowMax", default)]
    flow_max: Option<XmlValue>,
    #[serde(rename = "@dragFactor", default)]
    drag_factor_attr: Option<f64>,
    #[serde(rename = "dragFactor", default)]
    drag_factor: Option<XmlValue>,
}

#[derive(Debug, Deserialize)]
enum XmlConnection {
    #[serde(rename = "pipe")]
    Pipe(XmlConnectionRaw),
    #[serde(rename = "valve")]
    Valve(XmlConnectionRaw),
    #[serde(rename = "shortPipe")]
    ShortPipe(XmlConnectionRaw),
    #[serde(rename = "resistor")]
    Resistor(XmlConnectionRaw),
    #[serde(rename = "controlValve")]
    ControlValve(XmlConnectionRaw),
    #[serde(rename = "compressorStation")]
    CompressorStation(XmlConnectionRaw),
}

impl XmlConnection {
    fn raw(&self) -> &XmlConnectionRaw {
        match self {
            Self::Pipe(raw)
            | Self::Valve(raw)
            | Self::ShortPipe(raw)
            | Self::Resistor(raw)
            | Self::ControlValve(raw)
            | Self::CompressorStation(raw) => raw,
        }
    }

    fn kind(&self) -> ConnectionKind {
        match self {
            Self::Pipe(_) => ConnectionKind::Pipe,
            Self::Valve(_) => ConnectionKind::Valve,
            Self::ShortPipe(_) => ConnectionKind::ShortPipe,
            Self::Resistor(_) => ConnectionKind::Resistor,
            // MVP: les controlValve GasLib sont traitées comme liaisons quasi-passantes
            // (évite de couper le réseau sur des états partiels de contrôle).
            Self::ControlValve(_) => ConnectionKind::ShortPipe,
            Self::CompressorStation(_) => ConnectionKind::CompressorStation,
        }
    }
}

fn connection_defaults(kind: ConnectionKind) -> (f64, f64, f64) {
    match kind {
        ConnectionKind::Pipe => (1.0, 500.0, 0.012),
        // MVP: valve/shortPipe/compressor sont traités comme des liaisons quasi-passantes.
        ConnectionKind::Valve
        | ConnectionKind::ShortPipe
        | ConnectionKind::CompressorStation
        | ConnectionKind::Resistor => (0.001, 1000.0, 0.012),
    }
}

fn parse_length_km(value: &XmlValue) -> f64 {
    match value.unit.as_deref() {
        Some("km") | None => value.value,
        Some("m") => value.value / 1000.0,
        _ => value.value,
    }
}

fn parse_diameter_mm(value: &XmlValue) -> f64 {
    match value.unit.as_deref() {
        Some("mm") | None => value.value,
        Some("m") => value.value * 1000.0,
        _ => value.value,
    }
}

fn parse_roughness_mm(value: &XmlValue) -> f64 {
    match value.unit.as_deref() {
        Some("mm") | None => value.value,
        Some("m") => value.value * 1000.0,
        _ => value.value,
    }
}

fn valve_is_open(kind: ConnectionKind, raw: &XmlConnectionRaw) -> bool {
    if kind != ConnectionKind::Valve {
        return true;
    }
    let Some(min) = raw.flow_min.as_ref().map(|v| v.value) else {
        return true;
    };
    let Some(max) = raw.flow_max.as_ref().map(|v| v.value) else {
        return true;
    };
    !(min.abs() < 1e-12 && max.abs() < 1e-12)
}

// ---------------------------------------------------------------------------
// Chargement
// ---------------------------------------------------------------------------

/// Charge un fichier réseau GasLib (.net) et construit le `GasNetwork`.
pub fn load_network<P: AsRef<Path>>(path: P) -> Result<GasNetwork> {
    let xml = std::fs::read_to_string(path.as_ref())
        .with_context(|| format!("lecture de {:?}", path.as_ref()))?;

    let raw: XmlNetwork = from_str(&xml).with_context(|| "parsing XML GasLib")?;
    let cs_path = path.as_ref().with_extension("cs");
    let compressor_ratios = if cs_path.exists() {
        match load_compressor_ratios(&cs_path) {
            Ok(map) => map,
            Err(err) => {
                tracing::warn!(
                    "unable to load compressor station file {:?}: {err:#}",
                    cs_path
                );
                HashMap::new()
            }
        }
    } else {
        HashMap::new()
    };

    let mut net = GasNetwork::new();

    for node in &raw.nodes.entries {
        let pressure_lower_bar = node
            .pressure
            .as_ref()
            .and_then(|p| p.lower)
            .or_else(|| node.pressure_min.as_ref().map(|v| v.value));
        let pressure_upper_bar = node
            .pressure
            .as_ref()
            .and_then(|p| p.upper)
            .or_else(|| node.pressure_max.as_ref().map(|v| v.value));
        net.add_node(crate::graph::Node {
            id: node.id.clone(),
            x: node.x,
            y: node.y,
            lon: node.lon,
            lat: node.lat,
            height_m: node
                .height
                .as_ref()
                .map(|h| h.value)
                .or(node.height_attr)
                .unwrap_or(0.0),
            pressure_lower_bar,
            pressure_upper_bar,
            pressure_fixed_bar: node.pressure.as_ref().and_then(|p| p.value),
        });
    }

    for conn in &raw.connections.entries {
        let src = conn.raw();
        let kind = conn.kind();
        let is_open = valve_is_open(kind, src);
        let (default_length_km, default_diameter_mm, default_roughness_mm) =
            connection_defaults(kind);
        let compressor_ratio_max = if kind == ConnectionKind::CompressorStation {
            Some(compressor_ratios.get(&src.id).copied().unwrap_or(1.08))
        } else {
            None
        };
        net.add_pipe(crate::graph::Pipe {
            id: src.id.clone(),
            from: src.from.clone(),
            to: src.to.clone(),
            kind,
            is_open,
            length_km: src
                .length
                .as_ref()
                .map(parse_length_km)
                .or(src.length_km_attr)
                .unwrap_or(default_length_km),
            diameter_mm: src
                .diameter
                .as_ref()
                .map(parse_diameter_mm)
                .or(src.diameter_mm_attr)
                .unwrap_or(default_diameter_mm),
            roughness_mm: src
                .roughness
                .as_ref()
                .map(parse_roughness_mm)
                .or(src.roughness_mm_attr)
                .or(src.drag_factor.as_ref().map(|v| v.value))
                .or(src.drag_factor_attr)
                .unwrap_or(default_roughness_mm),
            compressor_ratio_max,
        });
    }

    Ok(net)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Debug, Serialize)]
    struct NetworkSnapshot {
        node_count: usize,
        edge_count: usize,
        nodes: Vec<crate::graph::Node>,
        pipes: Vec<crate::graph::Pipe>,
    }

    #[test]
    fn test_parse_minimal_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<network xmlns:framework="http://gaslib.zib.de/Framework">
  <nodes>
    <node id="N1" x="10.0" y="50.0" geoWGS84Long="10.0" geoWGS84Lat="50.0"/>
    <node id="N2" x="11.0" y="51.0" geoWGS84Long="11.0" geoWGS84Lat="51.0"/>
  </nodes>
  <connections>
    <pipe id="P1" from="N1" to="N2" length="50.0" diameter="500.0" roughness="0.012"/>
  </connections>
</network>"#;

        let raw: XmlNetwork = from_str(xml).expect("parsing XML");
        assert_eq!(raw.nodes.entries.len(), 2);
        assert_eq!(raw.connections.entries.len(), 1);
    }

    #[test]
    fn test_parse_with_namespace_prefix() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<network xmlns:framework="http://gaslib.zib.de/Framework">
  <framework:nodes>
    <node id="A" x="0" y="0"/>
    <node id="B" x="1" y="1"/>
  </framework:nodes>
  <framework:connections>
    <pipe id="P" from="A" to="B"/>
  </framework:connections>
</network>"#;

        let raw: XmlNetwork = from_str(xml).expect("parsing XML with ns prefix");
        assert_eq!(raw.nodes.entries.len(), 2);
        assert_eq!(raw.connections.entries.len(), 1);
    }

    #[test]
    fn test_parse_connection_kinds() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<network>
  <nodes>
    <node id="A" x="0" y="0"/>
    <node id="B" x="1" y="0"/>
    <node id="C" x="2" y="0"/>
    <node id="D" x="3" y="0"/>
    <node id="E" x="4" y="0"/>
  </nodes>
  <connections>
    <pipe id="P1" from="A" to="B" length="10.0" diameter="500.0" roughness="0.02"/>
    <valve id="V1" from="B" to="C"/>
    <shortPipe id="SP1" from="C" to="D"/>
    <resistor id="R1" from="D" to="E"/>
    <compressorStation id="CS1" from="E" to="A"/>
  </connections>
</network>"#;

        let dir = std::env::temp_dir().join("gazflow_test_xml");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test_connections.net");
        std::fs::write(&path, xml).unwrap();

        let net = load_network(&path).expect("load_network");
        assert_eq!(net.edge_count(), 5);

        let mut kinds: Vec<_> = net.pipes().map(|p| p.kind).collect();
        kinds.sort_by_key(|k| match k {
            ConnectionKind::Pipe => 0,
            ConnectionKind::Valve => 1,
            ConnectionKind::ShortPipe => 2,
            ConnectionKind::Resistor => 3,
            ConnectionKind::CompressorStation => 4,
        });
        assert_eq!(
            kinds,
            vec![
                ConnectionKind::Pipe,
                ConnectionKind::Valve,
                ConnectionKind::ShortPipe,
                ConnectionKind::Resistor,
                ConnectionKind::CompressorStation,
            ]
        );

        let valve = net.pipes().find(|p| p.id == "V1").expect("valve exists");
        assert_eq!(valve.length_km, 0.001);
        assert_eq!(valve.diameter_mm, 1000.0);
        assert!(valve.is_open, "default valve should be treated as open");
    }

    #[test]
    fn test_parse_closed_valve_from_zero_flow_bounds() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<network>
  <nodes>
    <node id="A" x="0" y="0"/>
    <node id="B" x="1" y="0"/>
  </nodes>
  <connections>
    <valve id="V1" from="A" to="B">
      <flowMin value="0.0"/>
      <flowMax value="0.0"/>
    </valve>
  </connections>
</network>"#;

        let dir = std::env::temp_dir().join("gazsim_test_xml");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test_closed_valve.net");
        std::fs::write(&path, xml).unwrap();

        let net = load_network(&path).expect("load_network");
        let valve = net.pipes().find(|p| p.id == "V1").expect("valve exists");
        assert!(
            !valve.is_open,
            "zero flow bounds should mark valve as closed"
        );
    }

    #[test]
    fn test_parse_child_value_elements() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<network>
  <nodes>
    <source id="A" x="0" y="0">
      <height value="12.5"/>
      <pressureMin value="40.0"/>
      <pressureMax value="70.0"/>
    </source>
    <sink id="B" x="1" y="1">
      <height value="4.0"/>
      <pressureMin value="35.0"/>
      <pressureMax value="60.0"/>
    </sink>
  </nodes>
  <connections>
    <pipe id="P1" from="A" to="B">
      <length value="55.0"/>
      <diameter value="500.0"/>
      <roughness value="0.1"/>
    </pipe>
  </connections>
</network>"#;

        let dir = std::env::temp_dir().join("gazflow_test_xml");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test_child_values.net");
        std::fs::write(&path, xml).unwrap();

        let net = load_network(&path).expect("load_network");
        let node_a = net.nodes().find(|n| n.id == "A").expect("node A");
        let pipe = net.pipes().find(|p| p.id == "P1").expect("pipe P1");

        assert_eq!(node_a.height_m, 12.5);
        assert_eq!(node_a.pressure_lower_bar, Some(40.0));
        assert_eq!(node_a.pressure_upper_bar, Some(70.0));
        assert_eq!(pipe.length_km, 55.0);
        assert_eq!(pipe.diameter_mm, 500.0);
        assert_eq!(pipe.roughness_mm, 0.1);
    }

    #[test]
    fn test_parse_gaslib_11_topology() {
        let path = Path::new("dat/GasLib-11.net");
        if !path.exists() {
            eprintln!("skip: {:?} not found", path);
            return;
        }

        let net = load_network(path).expect("load GasLib-11");
        assert_eq!(net.node_count(), 11);
        assert_eq!(net.edge_count(), 11);

        let mut pipes = 0usize;
        let mut valves = 0usize;
        let mut compressors = 0usize;
        for edge in net.pipes() {
            match edge.kind {
                ConnectionKind::Pipe => pipes += 1,
                ConnectionKind::Valve => valves += 1,
                ConnectionKind::CompressorStation => compressors += 1,
                ConnectionKind::ShortPipe | ConnectionKind::Resistor => {}
            }
        }
        assert_eq!(pipes, 8, "GasLib-11 should contain 8 pipes");
        assert_eq!(valves, 1, "GasLib-11 should contain 1 valve");
        assert_eq!(compressors, 2, "GasLib-11 should contain 2 compressors");

        for pipe in net
            .pipes()
            .filter(|p| p.kind == ConnectionKind::CompressorStation)
        {
            let ratio = pipe.compressor_ratio_max.unwrap_or(1.0);
            assert!(
                ratio >= 1.0,
                "compressor ratio should be >= 1 for {}",
                pipe.id
            );
        }
    }

    #[test]
    fn test_all_nodes_have_gps() {
        let path = Path::new("dat/GasLib-11.net");
        if !path.exists() {
            eprintln!("skip: {:?} not found", path);
            return;
        }

        let net = load_network(path).expect("load GasLib-11");
        for node in net.nodes() {
            assert!(
                node.x.is_finite(),
                "invalid x for node {}: {}",
                node.id,
                node.x
            );
            assert!(
                node.y.is_finite(),
                "invalid y for node {}: {}",
                node.id,
                node.y
            );
            match (node.lon, node.lat) {
                (Some(lon), Some(lat)) => {
                    assert!(
                        (-180.0..=180.0).contains(&lon),
                        "invalid lon for node {}: {}",
                        node.id,
                        lon
                    );
                    assert!(
                        (-90.0..=90.0).contains(&lat),
                        "invalid lat for node {}: {}",
                        node.id,
                        lat
                    );
                }
                (None, None) => {}
                _ => panic!("partial GPS data for node {}", node.id),
            }
        }
    }

    #[test]
    fn test_parse_gaslib_24_extended_connection_kinds() {
        let path = Path::new("dat/GasLib-24.net");
        if !path.exists() {
            eprintln!("skip: {:?} not found", path);
            return;
        }

        let net = load_network(path).expect("load GasLib-24");
        let has_resistor = net.pipes().any(|p| p.kind == ConnectionKind::Resistor);
        assert!(
            has_resistor,
            "GasLib-24 should include at least one resistor connection"
        );
    }

    #[test]
    fn test_gaslib_11_snapshot() {
        let path = Path::new("dat/GasLib-11.net");
        if !path.exists() {
            eprintln!("skip: {:?} not found", path);
            return;
        }

        let net = load_network(path).expect("load GasLib-11");
        let mut nodes: Vec<_> = net.nodes().cloned().collect();
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        let mut pipes: Vec<_> = net.pipes().cloned().collect();
        pipes.sort_by(|a, b| a.id.cmp(&b.id));

        let snapshot = NetworkSnapshot {
            node_count: net.node_count(),
            edge_count: net.edge_count(),
            nodes,
            pipes,
        };

        insta::assert_yaml_snapshot!("gaslib_11_network", snapshot);
    }

    #[test]
    fn test_load_network_builds_graph() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<network>
  <nodes>
    <node id="S" x="10.0" y="50.0" geoWGS84Long="10.0" geoWGS84Lat="50.0"/>
    <node id="D" x="11.0" y="51.0" geoWGS84Long="11.0" geoWGS84Lat="51.0"/>
  </nodes>
  <connections>
    <pipe id="P1" from="S" to="D" length="100.0" diameter="500.0" roughness="0.012"/>
  </connections>
</network>"#;

        let dir = std::env::temp_dir().join("gazflow_test_xml");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.net");
        std::fs::write(&path, xml).unwrap();

        let net = load_network(&path).expect("load_network");
        assert_eq!(net.node_count(), 2);
        assert_eq!(net.edge_count(), 1);
    }
}
