use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};
use shapefile::dbase::{FieldValue, Reader as DbfReader};
use shapefile::{Reader, Shape, ShapeReader, ShapeType};

use super::mapping::{MappingConfig, raw_node_from_properties, raw_pipe_from_properties};
use crate::graph::{RawNetwork, RawNode};

type MemoryShapefileReader = Reader<Cursor<Vec<u8>>, Cursor<Vec<u8>>>;

pub fn import_shapefile_pair(
    nodes_shp: &Path,
    nodes_dbf: &Path,
    pipes_shp: &Path,
    pipes_dbf: &Path,
    mapping: &MappingConfig,
) -> Result<RawNetwork> {
    let nodes_shp_bytes =
        std::fs::read(nodes_shp).with_context(|| format!("lecture SHP nœuds {:?}", nodes_shp))?;
    let nodes_dbf_bytes =
        std::fs::read(nodes_dbf).with_context(|| format!("lecture DBF nœuds {:?}", nodes_dbf))?;
    let pipes_shp_bytes = std::fs::read(pipes_shp)
        .with_context(|| format!("lecture SHP conduites {:?}", pipes_shp))?;
    let pipes_dbf_bytes = std::fs::read(pipes_dbf)
        .with_context(|| format!("lecture DBF conduites {:?}", pipes_dbf))?;
    import_shapefile_pair_bytes(
        &nodes_shp_bytes,
        &nodes_dbf_bytes,
        &pipes_shp_bytes,
        &pipes_dbf_bytes,
        mapping,
    )
}

pub fn import_shapefile_pair_bytes(
    nodes_shp: &[u8],
    nodes_dbf: &[u8],
    pipes_shp: &[u8],
    pipes_dbf: &[u8],
    mapping: &MappingConfig,
) -> Result<RawNetwork> {
    let mut nodes = read_point_layer(nodes_shp, nodes_dbf, mapping)?;
    let pipes = read_polyline_layer(pipes_shp, pipes_dbf, mapping)?;
    dedupe_nodes(&mut nodes);
    Ok(RawNetwork {
        nodes,
        pipes,
        source: Some("shapefile:pair".to_string()),
        compressor_catalog: None,
    })
}

fn read_point_layer(
    shp_bytes: &[u8],
    dbf_bytes: &[u8],
    mapping: &MappingConfig,
) -> Result<Vec<RawNode>> {
    let mut reader = open_reader(shp_bytes, dbf_bytes)?;
    let shape_type = reader.header().shape_type;
    if !matches!(
        shape_type,
        ShapeType::Point | ShapeType::PointZ | ShapeType::PointM
    ) {
        bail!("shapefile nœuds: type {shape_type:?} attendu Point");
    }

    let mut nodes = Vec::new();
    for (i, result) in reader.iter_shapes_and_records().enumerate() {
        let (shape, record) = result.with_context(|| format!("shapefile nœud #{i}"))?;
        let Shape::Point(point) = shape else {
            bail!("shapefile nœuds: entrée #{i} n'est pas un Point");
        };
        let mut props = record_to_json(&record);
        inject_point_geometry(&mut props, point.x, point.y);
        nodes.push(raw_node_from_properties(&props, mapping)?);
    }
    Ok(nodes)
}

fn read_polyline_layer(
    shp_bytes: &[u8],
    dbf_bytes: &[u8],
    mapping: &MappingConfig,
) -> Result<Vec<crate::graph::RawPipe>> {
    let mut reader = open_reader(shp_bytes, dbf_bytes)?;
    match reader.header().shape_type {
        ShapeType::Polyline | ShapeType::PolylineZ | ShapeType::PolylineM => {}
        other => bail!("shapefile conduites: type {other:?} attendu Polyline"),
    }

    let mut pipes = Vec::new();
    for (i, result) in reader.iter_shapes_and_records().enumerate() {
        let (_shape, record) = result.with_context(|| format!("shapefile conduite #{i}"))?;
        let props = record_to_json(&record);
        pipes.push(raw_pipe_from_properties(&props, mapping)?);
    }
    Ok(pipes)
}

fn open_reader(shp_bytes: &[u8], dbf_bytes: &[u8]) -> Result<MemoryShapefileReader> {
    let shp = Cursor::new(shp_bytes.to_vec());
    let dbf = Cursor::new(dbf_bytes.to_vec());
    let shape_reader = ShapeReader::new(shp).context("ouverture SHP shapefile")?;
    let dbf_reader = DbfReader::new(dbf).context("ouverture DBF shapefile")?;
    Ok(Reader::new(shape_reader, dbf_reader))
}

fn record_to_json(record: &shapefile::dbase::Record) -> Value {
    let mut map = Map::new();
    for (name, value) in record.as_ref() {
        map.insert(name.clone(), field_to_json(value));
    }
    Value::Object(map)
}

fn field_to_json(value: &FieldValue) -> Value {
    match value {
        FieldValue::Character(Some(s)) => Value::String(s.trim().to_string()),
        FieldValue::Character(None) => Value::Null,
        FieldValue::Numeric(Some(v)) => Value::from(*v),
        FieldValue::Numeric(None) => Value::Null,
        FieldValue::Logical(Some(v)) => Value::Bool(*v),
        FieldValue::Logical(None) => Value::Null,
        FieldValue::Date(Some(d)) => Value::String(format!("{d:?}")),
        FieldValue::Date(None) => Value::Null,
        FieldValue::Float(Some(v)) => Value::from(*v),
        FieldValue::Float(None) => Value::Null,
        FieldValue::Integer(v) => Value::from(*v),
        FieldValue::Currency(v) => Value::from(*v),
        FieldValue::DateTime(v) => Value::String(format!("{v:?}")),
        FieldValue::Double(v) => Value::from(*v),
        FieldValue::Memo(s) => Value::String(s.clone()),
    }
}

fn inject_point_geometry(props: &mut Value, lon: f64, lat: f64) {
    let Some(obj) = props.as_object_mut() else {
        return;
    };
    obj.insert(
        "geometry".to_string(),
        serde_json::json!({
            "type": "Point",
            "coordinates": [lon, lat]
        }),
    );
}

fn dedupe_nodes(nodes: &mut Vec<RawNode>) {
    let mut seen = std::collections::HashSet::new();
    nodes.retain(|n| seen.insert(n.id.clone()));
}
