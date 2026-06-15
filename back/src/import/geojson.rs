use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use super::mapping::{MappingConfig, raw_node_from_properties, raw_pipe_from_properties};
use crate::graph::{RawNetwork, RawNode};

pub fn import_geojson(paths: &[&Path], mapping: &MappingConfig) -> Result<RawNetwork> {
    let chunks: Vec<String> = paths
        .iter()
        .map(|p| std::fs::read_to_string(p).with_context(|| format!("lecture GeoJSON {:?}", p)))
        .collect::<Result<_>>()?;
    let refs: Vec<&str> = chunks.iter().map(|s| s.as_str()).collect();
    import_geojson_str(&refs, mapping)
}

pub fn import_geojson_str(contents: &[&str], mapping: &MappingConfig) -> Result<RawNetwork> {
    let mut nodes = Vec::new();
    let mut pipes = Vec::new();

    for (idx, text) in contents.iter().enumerate() {
        let fc: Value = serde_json::from_str(text)
            .with_context(|| format!("JSON GeoJSON invalide (chunk {idx})"))?;
        ingest_feature_collection(&fc, mapping, &mut nodes, &mut pipes)?;
    }

    dedupe_nodes(&mut nodes);
    Ok(RawNetwork {
        nodes,
        pipes,
        source: Some(format!("geojson:{}_chunks", contents.len())),
    })
}

fn ingest_feature_collection(
    fc: &Value,
    mapping: &MappingConfig,
    nodes: &mut Vec<RawNode>,
    pipes: &mut Vec<crate::graph::RawPipe>,
) -> Result<()> {
    let features = fc
        .get("features")
        .and_then(|v| v.as_array())
        .context("FeatureCollection attendue")?;

    for feature in features {
        let props = feature
            .get("properties")
            .cloned()
            .unwrap_or(Value::Object(serde_json::Map::new()));
        let geom = feature.get("geometry");
        let geom_type = geom.and_then(|g| g.get("type")).and_then(|t| t.as_str());

        match geom_type {
            Some("Point") => {
                let mut merged = props;
                if let Some(g) = geom
                    && let Some(obj) = merged.as_object_mut()
                {
                    obj.insert("geometry".to_string(), g.clone());
                }
                nodes.push(raw_node_from_properties(&merged, mapping)?);
            }
            Some("LineString") | Some("MultiLineString") => {
                pipes.push(raw_pipe_from_properties(&props, mapping)?);
            }
            None if props.get("ID_CANA").is_some() || props.get("NOEUD_AMONT").is_some() => {
                pipes.push(raw_pipe_from_properties(&props, mapping)?);
            }
            None if props.get("ID_NOEUD").is_some() || props.get("id").is_some() => {
                nodes.push(raw_node_from_properties(&props, mapping)?);
            }
            Some(other) => bail!("géométrie GeoJSON non supportée: {other}"),
            None => bail!("feature sans géométrie ni propriétés reconnues"),
        }
    }
    Ok(())
}

fn dedupe_nodes(nodes: &mut Vec<RawNode>) {
    let mut seen = std::collections::HashSet::new();
    nodes.retain(|n| seen.insert(n.id.clone()));
}
