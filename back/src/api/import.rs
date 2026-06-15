//! Endpoint REST d'import réseau (P6.8).

use std::collections::HashMap;

use anyhow::Context;
use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};

use crate::graph::{EquipmentSpec, GasNetwork, RawNetwork, RawNodeRole};
use crate::import::{
    self, ValidationError, import_csv_str, import_geojson_str, import_shapefile_pair_bytes,
    load_mapping_from_str, validate_topology,
};
use crate::solver::GasComposition;

use super::SharedState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ImportNetworkRequest {
    pub format: String,
    pub name: Option<String>,
    pub mapping_yaml: String,
    #[serde(default)]
    pub nodes_geojson: Option<String>,
    #[serde(default)]
    pub pipes_geojson: Option<String>,
    #[serde(default)]
    pub network_geojson: Option<String>,
    #[serde(default)]
    pub nodes_csv: Option<String>,
    #[serde(default)]
    pub pipes_csv: Option<String>,
    #[serde(default)]
    pub nodes_shp_b64: Option<String>,
    #[serde(default)]
    pub nodes_dbf_b64: Option<String>,
    #[serde(default)]
    pub pipes_shp_b64: Option<String>,
    #[serde(default)]
    pub pipes_dbf_b64: Option<String>,
    #[serde(default)]
    pub validate_only: bool,
    #[serde(default)]
    pub activate: bool,
    #[serde(default)]
    pub default_demands: HashMap<String, f64>,
    #[serde(default)]
    pub gas_composition: Option<GasComposition>,
}

#[derive(Debug, Serialize)]
pub struct ImportPreviewNodeDto {
    pub id: String,
    pub lon: f64,
    pub lat: f64,
    #[serde(rename = "role")]
    pub role: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ImportPreviewPipeDto {
    pub id: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Serialize)]
pub struct ImportPreviewGeometry {
    pub nodes: Vec<ImportPreviewNodeDto>,
    pub pipes: Vec<ImportPreviewPipeDto>,
}

#[derive(Debug, Serialize)]
pub struct ImportNetworkResponse {
    pub network_id: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub active: bool,
    pub validate_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<ImportPreviewGeometry>,
}

fn preview_geometry_from_raw(raw: &RawNetwork) -> Option<ImportPreviewGeometry> {
    let mut nodes = Vec::new();
    for node in &raw.nodes {
        let (lon, lat) = preview_lon_lat(node)?;
        nodes.push(ImportPreviewNodeDto {
            id: node.id.clone(),
            lon,
            lat,
            role: raw_node_role_label(node.role),
        });
    }
    if nodes.len() < 2 {
        return None;
    }
    Some(ImportPreviewGeometry {
        nodes,
        pipes: raw
            .pipes
            .iter()
            .map(|p| ImportPreviewPipeDto {
                id: p.id.clone(),
                from: p.from.clone(),
                to: p.to.clone(),
            })
            .collect(),
    })
}

fn preview_lon_lat(node: &crate::graph::RawNode) -> Option<(f64, f64)> {
    if let (Some(lon), Some(lat)) = (node.lon, node.lat) {
        return Some((lon, lat));
    }
    if node.x.is_finite() && node.y.is_finite() {
        return Some((node.x, node.y));
    }
    None
}

fn raw_node_role_label(role: RawNodeRole) -> &'static str {
    match role {
        RawNodeRole::Source => "source",
        RawNodeRole::Sink => "sink",
        RawNodeRole::Innode => "innode",
    }
}

fn import_response(
    network_id: String,
    preview: Option<ImportPreviewGeometry>,
    node_count: usize,
    edge_count: usize,
    active: bool,
    validate_only: bool,
) -> ImportNetworkResponse {
    ImportNetworkResponse {
        network_id,
        node_count,
        edge_count,
        active,
        validate_only,
        preview,
    }
}

pub async fn post_import_network(
    State(state): State<SharedState>,
    Json(payload): Json<ImportNetworkRequest>,
) -> Result<Json<ImportNetworkResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !payload.validate_only
        && state.simulation_slots.available_permits() != state.simulation_capacity
    {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "cannot import while simulations are running"
            })),
        ));
    }

    let mapping = load_mapping_from_str(&payload.mapping_yaml).map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
    })?;

    let raw = parse_import_payload(&payload, &mapping).map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
    })?;

    validate_topology(&raw).map_err(validation_http_error)?;

    let preview = preview_geometry_from_raw(&raw);

    if payload.validate_only {
        return Ok(Json(import_response(
            payload.name.unwrap_or_else(|| "preview".to_string()),
            preview,
            raw.nodes.len(),
            raw.pipes.len(),
            false,
            true,
        )));
    }

    let default_demands = if payload.default_demands.is_empty() {
        default_demands_from_mapping(&raw, &mapping)
    } else {
        payload.default_demands
    };
    let gas_composition = payload
        .gas_composition
        .unwrap_or_else(|| mapping.defaults.resolved_gas_composition());

    let network = GasNetwork::from_raw(raw).map_err(|err| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
    })?;

    let network_id = payload
        .name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .map(|s| format!("import-{s}"))
        .unwrap_or_else(|| format!("import-{}", chrono_like_id()));

    {
        let mut imported = state
            .imported
            .write()
            .expect("imported lock should not be poisoned");
        imported.insert(
            network_id.clone(),
            super::ImportedDataset {
                network,
                default_demands,
                gas_composition,
            },
        );
    }

    {
        let mut available = state
            .available_datasets
            .write()
            .expect("available datasets lock should not be poisoned");
        if !available.iter().any(|id| id == &network_id) {
            available.push(network_id.clone());
        }
    }

    let activate = payload.activate || payload.name.is_some();

    if activate {
        let (node_count, edge_count) = {
            let imported = state
                .imported
                .read()
                .expect("imported lock should not be poisoned");
            let dataset = imported.get(&network_id).ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "imported dataset missing after insert" })),
                )
            })?;
            let nc = dataset.network.node_count();
            let ec = dataset.network.edge_count();
            super::activate_imported_dataset(&state, &network_id, dataset);
            (nc, ec)
        };
        Ok(Json(import_response(
            network_id, preview, node_count, edge_count, true, false,
        )))
    } else {
        let imported = state
            .imported
            .read()
            .expect("imported lock should not be poisoned");
        let dataset = imported.get(&network_id).expect("dataset");
        Ok(Json(import_response(
            network_id,
            preview,
            dataset.network.node_count(),
            dataset.network.edge_count(),
            false,
            false,
        )))
    }
}

fn parse_import_payload(
    payload: &ImportNetworkRequest,
    mapping: &import::MappingConfig,
) -> anyhow::Result<crate::graph::RawNetwork> {
    match payload.format.to_ascii_lowercase().as_str() {
        "geojson" => {
            let mut chunks = Vec::new();
            if let Some(ref network) = payload.network_geojson {
                chunks.push(network.as_str());
            } else {
                if let Some(ref nodes) = payload.nodes_geojson {
                    chunks.push(nodes.as_str());
                }
                if let Some(ref pipes) = payload.pipes_geojson {
                    chunks.push(pipes.as_str());
                }
            }
            if chunks.is_empty() {
                anyhow::bail!("geojson: fournir network_geojson ou nodes_geojson/pipes_geojson");
            }
            import_geojson_str(&chunks, mapping)
        }
        "csv" => {
            let nodes = payload
                .nodes_csv
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("csv: nodes_csv requis"))?;
            let pipes = payload
                .pipes_csv
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("csv: pipes_csv requis"))?;
            import_csv_str(nodes, pipes, mapping)
        }
        "shapefile" => {
            let nodes_shp = decode_b64(payload.nodes_shp_b64.as_deref(), "nodes_shp_b64")?;
            let nodes_dbf = decode_b64(payload.nodes_dbf_b64.as_deref(), "nodes_dbf_b64")?;
            let pipes_shp = decode_b64(payload.pipes_shp_b64.as_deref(), "pipes_shp_b64")?;
            let pipes_dbf = decode_b64(payload.pipes_dbf_b64.as_deref(), "pipes_dbf_b64")?;
            import_shapefile_pair_bytes(&nodes_shp, &nodes_dbf, &pipes_shp, &pipes_dbf, mapping)
        }
        other => anyhow::bail!("format d'import inconnu: {other}"),
    }
}

fn decode_b64(value: Option<&str>, field: &str) -> anyhow::Result<Vec<u8>> {
    let Some(raw) = value else {
        anyhow::bail!("shapefile: {field} requis (base64)");
    };
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, raw.trim())
        .with_context(|| format!("décodage base64 {field}"))
}

fn default_demands_from_mapping(
    raw: &RawNetwork,
    mapping: &import::MappingConfig,
) -> HashMap<String, f64> {
    let Some(q) = mapping.defaults.sink_demand_m3s else {
        return HashMap::new();
    };
    raw.nodes
        .iter()
        .filter(|n| n.role == RawNodeRole::Sink)
        .map(|n| (n.id.clone(), q))
        .collect()
}

fn validation_http_error(err: ValidationError) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(serde_json::json!({
            "error": err.to_string(),
            "validation": format!("{err:?}"),
        })),
    )
}

fn chrono_like_id() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

#[cfg(test)]
mod tests {
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use serde_json::Value;
    use tower::ServiceExt;

    use super::*;
    use crate::api::create_router_with_runtime_limits_and_datasets;
    use crate::graph::{ConnectionKind, Node, Pipe};
    use crate::import::test_corpus_root;

    fn test_router() -> axum::Router {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "source".into(),
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
            id: "sink".into(),
            x: 1.0,
            y: 0.0,
            lon: Some(11.0),
            lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net.add_pipe(Pipe {
            id: "p1".into(),
            from: "source".into(),
            to: "sink".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 10.0,
            diameter_mm: 500.0,
            roughness_mm: 0.05,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        create_router_with_runtime_limits_and_datasets(
            net,
            HashMap::new(),
            "GasLib-11".to_string(),
            vec!["GasLib-11".to_string()],
            std::path::PathBuf::from("dat"),
            2,
            1,
        )
    }

    fn load_minimal_line_corpus() -> (String, String, String) {
        let root = test_corpus_root();
        let mapping = std::fs::read_to_string(root.join("synthetic/minimal-line/mapping.yaml"))
            .expect("mapping");
        let nodes = std::fs::read_to_string(root.join("synthetic/minimal-line/nodes.geojson"))
            .expect("nodes");
        let pipes = std::fs::read_to_string(root.join("synthetic/minimal-line/pipes.geojson"))
            .expect("pipes");
        (mapping, nodes, pipes)
    }

    async fn post_json(app: &axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .expect("response");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let parsed: Value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).to_string()))
        };
        (status, parsed)
    }

    async fn get_json(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .expect("response");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let parsed: Value = serde_json::from_slice(&bytes).expect("json");
        (status, parsed)
    }

    #[test]
    fn preview_geometry_from_minimal_line_corpus() {
        let root = test_corpus_root();
        let mapping = std::fs::read_to_string(root.join("synthetic/minimal-line/mapping.yaml"))
            .expect("mapping");
        let nodes = std::fs::read_to_string(root.join("synthetic/minimal-line/nodes.geojson"))
            .expect("nodes");
        let pipes = std::fs::read_to_string(root.join("synthetic/minimal-line/pipes.geojson"))
            .expect("pipes");
        let mapping_cfg = load_mapping_from_str(&mapping).expect("mapping yaml");
        let raw =
            import_geojson_str(&[nodes.as_str(), pipes.as_str()], &mapping_cfg).expect("geojson");
        let preview = preview_geometry_from_raw(&raw).expect("preview");
        assert_eq!(preview.nodes.len(), 3);
        assert_eq!(preview.pipes.len(), 2);
        assert!(preview.nodes.iter().any(|n| n.role == "source"));
    }

    #[tokio::test]
    async fn test_api_import_upload() {
        let (mapping, nodes, pipes) = load_minimal_line_corpus();

        let body = serde_json::json!({
            "format": "geojson",
            "name": "minimal-line",
            "mapping_yaml": mapping,
            "nodes_geojson": nodes,
            "pipes_geojson": pipes,
            "activate": true,
            "default_demands": { "LVR01": -50.0 }
        });

        let app = test_router();
        let (status, parsed) = post_json(&app, "/api/import", body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["network_id"], "import-minimal-line");
        assert_eq!(parsed["node_count"], 3);
        assert_eq!(parsed["edge_count"], 2);
        assert_eq!(parsed["active"], true);
        assert!(parsed.get("preview").and_then(|v| v.as_object()).is_some());

        let (sim_status, _) = get_json(&app, "/api/simulate").await;
        assert_eq!(sim_status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_import_validate_only() {
        let (mapping, nodes, pipes) = load_minimal_line_corpus();
        let body = serde_json::json!({
            "format": "geojson",
            "name": "preview-line",
            "mapping_yaml": mapping,
            "nodes_geojson": nodes,
            "pipes_geojson": pipes,
            "validate_only": true
        });

        let app = test_router();
        let (status, parsed) = post_json(&app, "/api/import", body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["validate_only"], true);
        assert_eq!(parsed["active"], false);
        assert_eq!(parsed["node_count"], 3);
        let preview = parsed["preview"].as_object().expect("preview geometry");
        assert_eq!(preview["nodes"].as_array().expect("nodes").len(), 3);
        assert_eq!(preview["pipes"].as_array().expect("pipes").len(), 2);

        let (networks_status, networks) = get_json(&app, "/api/networks").await;
        assert_eq!(networks_status, StatusCode::OK);
        assert!(
            !networks["available"]
                .as_array()
                .expect("available")
                .iter()
                .any(|id| id == "import-preview-line"),
            "validate_only ne doit pas enregistrer le réseau"
        );
    }

    #[tokio::test]
    async fn test_api_import_topo_error_returns_422() {
        let root = test_corpus_root();
        let mapping = std::fs::read_to_string(root.join("synthetic/minimal-line/mapping.yaml"))
            .expect("mapping");
        let orphan =
            std::fs::read_to_string(root.join("synthetic/topo-errors/orphan-node.geojson"))
                .expect("orphan");

        let body = serde_json::json!({
            "format": "geojson",
            "mapping_yaml": mapping,
            "nodes_geojson": orphan,
            "validate_only": true
        });

        let app = test_router();
        let (status, parsed) = post_json(&app, "/api/import", body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(parsed["error"].as_str().unwrap().contains("orphelin"));
        assert!(parsed.get("validation").is_some());
    }

    #[tokio::test]
    async fn test_api_import_invalid_mapping_returns_400() {
        let body = serde_json::json!({
            "format": "geojson",
            "mapping_yaml": "format: geojson\nnodes: [broken",
            "nodes_geojson": "{}"
        });

        let app = test_router();
        let (status, parsed) = post_json(&app, "/api/import", body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(parsed["error"].as_str().unwrap().contains("mapping"));
    }

    #[tokio::test]
    async fn test_api_import_csv_then_simulate() {
        let root = test_corpus_root();
        let mapping = std::fs::read_to_string(root.join("synthetic/gravity-pipe/mapping.yaml"))
            .expect("mapping");
        let nodes_csv =
            std::fs::read_to_string(root.join("synthetic/gravity-pipe/nodes.csv")).expect("nodes");
        let pipes_csv =
            std::fs::read_to_string(root.join("synthetic/gravity-pipe/pipes.csv")).expect("pipes");

        let body = serde_json::json!({
            "format": "csv",
            "name": "gravity-uphill",
            "mapping_yaml": mapping,
            "nodes_csv": nodes_csv,
            "pipes_csv": pipes_csv,
            "activate": true
        });

        let app = test_router();
        let (status, parsed) = post_json(&app, "/api/import", body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["network_id"], "import-gravity-uphill");
        assert_eq!(parsed["node_count"], 2);

        let (sim_status, _) = get_json(&app, "/api/simulate").await;
        assert_eq!(sim_status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_import_inactive_then_select() {
        let (mapping, nodes, pipes) = load_minimal_line_corpus();
        let body = serde_json::json!({
            "format": "geojson",
            "mapping_yaml": mapping,
            "nodes_geojson": nodes,
            "pipes_geojson": pipes,
            "activate": false,
            "default_demands": { "LVR01": -50.0 }
        });

        let app = test_router();
        let (status, parsed) = post_json(&app, "/api/import", body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["active"], false);
        let network_id = parsed["network_id"].as_str().expect("id").to_string();
        assert!(network_id.starts_with("import-"));

        let (networks_status, networks) = get_json(&app, "/api/networks").await;
        assert_eq!(networks_status, StatusCode::OK);
        assert!(
            networks["available"]
                .as_array()
                .expect("available")
                .iter()
                .any(|id| id.as_str() == Some(network_id.as_str()))
        );

        let (select_status, selected) = post_json(
            &app,
            "/api/network",
            serde_json::json!({ "dataset_id": network_id }),
        )
        .await;
        assert_eq!(select_status, StatusCode::OK);
        assert_eq!(selected["active"], network_id);
        assert_eq!(selected["node_count"], 3);

        let (sim_status, _) = get_json(&app, "/api/simulate").await;
        assert_eq!(sim_status, StatusCode::OK);
    }

    #[test]
    fn default_demands_apply_only_to_sink_nodes() {
        use crate::graph::{RawNetwork, RawNode, RawNodeRole, RawPipe};
        use crate::import::mapping::{MappingConfig, MappingDefaults};

        let raw = RawNetwork {
            nodes: vec![
                RawNode {
                    id: "SRC".into(),
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
                    id: "JNC".into(),
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
                },
                RawNode {
                    id: "LVR".into(),
                    role: RawNodeRole::Sink,
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
                },
            ],
            pipes: vec![RawPipe {
                id: "P".into(),
                from: "SRC".into(),
                to: "LVR".into(),
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
        };
        let mapping = MappingConfig {
            format: "geojson".into(),
            nodes: Default::default(),
            pipes: Default::default(),
            defaults: MappingDefaults {
                sink_demand_m3s: Some(-12.5),
                gas_composition: None,
            },
        };
        let demands = default_demands_from_mapping(&raw, &mapping);
        assert_eq!(demands.len(), 1);
        assert_eq!(demands.get("LVR"), Some(&-12.5));
        assert!(!demands.contains_key("JNC"));
    }
}
