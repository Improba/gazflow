use std::path::Path;

use anyhow::{Context, Result};
use quick_xml::de::from_str;
use serde::Deserialize;

use crate::graph::GasNetwork;

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
    height: Option<f64>,
    #[serde(default)]
    pressure: Option<XmlBound>,
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
struct XmlConnections {
    #[serde(rename = "$value", default)]
    entries: Vec<XmlConnection>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct XmlConnection {
    #[serde(rename = "@id")]
    id: String,
    #[serde(rename = "@from")]
    from: String,
    #[serde(rename = "@to")]
    to: String,
    #[serde(rename = "@length", default)]
    length_km: Option<f64>,
    #[serde(rename = "@diameter", default)]
    diameter_mm: Option<f64>,
    #[serde(rename = "@roughness", default)]
    roughness_mm: Option<f64>,
}

// ---------------------------------------------------------------------------
// Chargement
// ---------------------------------------------------------------------------

/// Charge un fichier réseau GasLib (.net) et construit le `GasNetwork`.
pub fn load_network<P: AsRef<Path>>(path: P) -> Result<GasNetwork> {
    let xml = std::fs::read_to_string(path.as_ref())
        .with_context(|| format!("lecture de {:?}", path.as_ref()))?;

    let raw: XmlNetwork =
        from_str(&xml).with_context(|| "parsing XML GasLib")?;

    let mut net = GasNetwork::new();

    for node in &raw.nodes.entries {
        net.add_node(crate::graph::Node {
            id: node.id.clone(),
            x: node.x,
            y: node.y,
            lon: node.lon,
            lat: node.lat,
            height_m: node.height.unwrap_or(0.0),
            pressure_lower_bar: node.pressure.as_ref().and_then(|p| p.lower),
            pressure_upper_bar: node.pressure.as_ref().and_then(|p| p.upper),
            pressure_fixed_bar: node.pressure.as_ref().and_then(|p| p.value),
        });
    }

    for conn in &raw.connections.entries {
        net.add_pipe(crate::graph::Pipe {
            id: conn.id.clone(),
            from: conn.from.clone(),
            to: conn.to.clone(),
            length_km: conn.length_km.unwrap_or(1.0),
            diameter_mm: conn.diameter_mm.unwrap_or(500.0),
            roughness_mm: conn.roughness_mm.unwrap_or(0.012),
        });
    }

    Ok(net)
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let dir = std::env::temp_dir().join("gazsim_test_xml");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.net");
        std::fs::write(&path, xml).unwrap();

        let net = load_network(&path).expect("load_network");
        assert_eq!(net.node_count(), 2);
        assert_eq!(net.edge_count(), 1);
    }
}
