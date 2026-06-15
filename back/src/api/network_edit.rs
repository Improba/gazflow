//! Endpoints REST d'édition du réseau (P12 MVP).

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::graph::{
    ConnectionKind, EquipmentSpec, GasNetwork, Node, Pipe, RawNetwork, RawNode, RawNodeRole,
    RawPipe,
};
use crate::import::validation::{ValidationError, validate_network_incremental};

use super::{ApiError, SharedState, active_dataset_id, active_network, clone_network};

#[derive(Debug, Serialize)]
pub(super) struct NetworkMutationResponse {
    node_count: usize,
    edge_count: usize,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateNodeRequest {
    id: String,
    x: f64,
    y: f64,
    #[serde(default)]
    lon: Option<f64>,
    #[serde(default)]
    lat: Option<f64>,
    #[serde(default)]
    height_m: Option<f64>,
    #[serde(default)]
    pressure_fixed_bar: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct UpdateNodeRequest {
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
    #[serde(default)]
    lon: Option<Option<f64>>,
    #[serde(default)]
    lat: Option<Option<f64>>,
    #[serde(default)]
    height_m: Option<f64>,
    #[serde(default)]
    pressure_fixed_bar: Option<Option<f64>>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum ConnectionKindInput {
    Pipe,
    Valve,
    ShortPipe,
    Resistor,
    CompressorStation,
    PressureRegulator,
    ControlValve,
    DeliveryStation,
}

impl From<ConnectionKindInput> for ConnectionKind {
    fn from(value: ConnectionKindInput) -> Self {
        match value {
            ConnectionKindInput::Pipe => ConnectionKind::Pipe,
            ConnectionKindInput::Valve => ConnectionKind::Valve,
            ConnectionKindInput::ShortPipe => ConnectionKind::ShortPipe,
            ConnectionKindInput::Resistor => ConnectionKind::Resistor,
            ConnectionKindInput::CompressorStation => ConnectionKind::CompressorStation,
            ConnectionKindInput::PressureRegulator => ConnectionKind::PressureRegulator,
            ConnectionKindInput::ControlValve => ConnectionKind::ControlValve,
            ConnectionKindInput::DeliveryStation => ConnectionKind::DeliveryStation,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct CreatePipeRequest {
    id: String,
    from: String,
    to: String,
    kind: ConnectionKindInput,
    length_km: f64,
    diameter_mm: f64,
    #[serde(default)]
    equipment: Option<EquipmentSpec>,
}

#[derive(Debug, Deserialize)]
pub(super) struct UpdatePipeRequest {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    kind: Option<ConnectionKindInput>,
    #[serde(default)]
    length_km: Option<f64>,
    #[serde(default)]
    diameter_mm: Option<f64>,
    #[serde(default)]
    equipment: Option<EquipmentSpec>,
}

pub(super) async fn post_node(
    State(state): State<SharedState>,
    Json(payload): Json<CreateNodeRequest>,
) -> Result<Json<NetworkMutationResponse>, (StatusCode, Json<ApiError>)> {
    apply_network_mutation(&state, move |raw| {
        if payload.id.trim().is_empty() {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "node id must not be empty",
            ));
        }
        if raw.nodes.iter().any(|n| n.id == payload.id) {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                format!("node id already exists: {}", payload.id),
            ));
        }
        raw.nodes.push(RawNode {
            id: payload.id,
            role: if payload.pressure_fixed_bar.is_some() {
                RawNodeRole::Source
            } else {
                RawNodeRole::Innode
            },
            x: payload.x,
            y: payload.y,
            lon: payload.lon,
            lat: payload.lat,
            height_m: payload.height_m.unwrap_or(0.0),
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: payload.pressure_fixed_bar,
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        Ok(None)
    })
    .map(Json)
}

pub(super) async fn put_node(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateNodeRequest>,
) -> Result<Json<NetworkMutationResponse>, (StatusCode, Json<ApiError>)> {
    apply_network_mutation(&state, move |raw| {
        let node = raw
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("node not found: {id}")))?;

        if let Some(x) = payload.x {
            node.x = x;
        }
        if let Some(y) = payload.y {
            node.y = y;
        }
        if let Some(lon) = payload.lon {
            node.lon = lon;
        }
        if let Some(lat) = payload.lat {
            node.lat = lat;
        }
        if let Some(height_m) = payload.height_m {
            node.height_m = height_m;
        }
        if let Some(pressure_fixed_bar) = payload.pressure_fixed_bar {
            node.pressure_fixed_bar = pressure_fixed_bar;
            node.role = if pressure_fixed_bar.is_some() {
                RawNodeRole::Source
            } else {
                RawNodeRole::Innode
            };
        }

        Ok(None)
    })
    .map(Json)
}

pub(super) async fn delete_node(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<Json<NetworkMutationResponse>, (StatusCode, Json<ApiError>)> {
    apply_network_mutation(&state, move |raw| {
        let before = raw.nodes.len();
        raw.nodes.retain(|n| n.id != id);
        if raw.nodes.len() == before {
            return Err(api_error(
                StatusCode::NOT_FOUND,
                format!("node not found: {id}"),
            ));
        }
        raw.pipes.retain(|p| p.from != id && p.to != id);
        Ok(Some(id))
    })
    .map(Json)
}

pub(super) async fn post_pipe(
    State(state): State<SharedState>,
    Json(payload): Json<CreatePipeRequest>,
) -> Result<Json<NetworkMutationResponse>, (StatusCode, Json<ApiError>)> {
    apply_network_mutation(&state, move |raw| {
        if payload.id.trim().is_empty() {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "pipe id must not be empty",
            ));
        }
        if raw.pipes.iter().any(|p| p.id == payload.id) {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                format!("pipe id already exists: {}", payload.id),
            ));
        }
        raw.pipes.push(RawPipe {
            id: payload.id,
            from: payload.from,
            to: payload.to,
            kind: payload.kind.into(),
            is_open: true,
            length_km: payload.length_km,
            diameter_mm: payload.diameter_mm,
            roughness_mm: 0.05,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: payload.equipment.unwrap_or_default(),
        });
        Ok(None)
    })
    .map(Json)
}

pub(super) async fn put_pipe(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdatePipeRequest>,
) -> Result<Json<NetworkMutationResponse>, (StatusCode, Json<ApiError>)> {
    apply_network_mutation(&state, move |raw| {
        let pipe = raw
            .pipes
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("pipe not found: {id}")))?;

        if let Some(from) = payload.from {
            pipe.from = from;
        }
        if let Some(to) = payload.to {
            pipe.to = to;
        }
        if let Some(kind) = payload.kind {
            pipe.kind = kind.into();
        }
        if let Some(length_km) = payload.length_km {
            pipe.length_km = length_km;
        }
        if let Some(diameter_mm) = payload.diameter_mm {
            pipe.diameter_mm = diameter_mm;
        }
        if let Some(equipment) = payload.equipment {
            pipe.equipment = equipment;
        }

        Ok(None)
    })
    .map(Json)
}

pub(super) async fn delete_pipe(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<Json<NetworkMutationResponse>, (StatusCode, Json<ApiError>)> {
    apply_network_mutation(&state, move |raw| {
        let before = raw.pipes.len();
        raw.pipes.retain(|p| p.id != id);
        if raw.pipes.len() == before {
            return Err(api_error(
                StatusCode::NOT_FOUND,
                format!("pipe not found: {id}"),
            ));
        }
        Ok(None)
    })
    .map(Json)
}

fn apply_network_mutation<F>(
    state: &SharedState,
    mutate: F,
) -> Result<NetworkMutationResponse, (StatusCode, Json<ApiError>)>
where
    F: FnOnce(&mut RawNetwork) -> Result<Option<String>, (StatusCode, Json<ApiError>)>,
{
    ensure_network_editable(state)?;

    let current = active_network(state);
    let mut raw = network_to_raw(current.as_ref());
    let removed_node = mutate(&mut raw)?;

    validate_network_incremental(&raw).map_err(validation_bad_request)?;

    let network = GasNetwork::from_raw(raw).map_err(|err| {
        api_error(
            StatusCode::BAD_REQUEST,
            format!("invalid network after mutation: {err}"),
        )
    })?;
    Ok(persist_network(state, network, removed_node.as_deref()))
}

fn persist_network(
    state: &SharedState,
    network: GasNetwork,
    removed_node: Option<&str>,
) -> NetworkMutationResponse {
    let node_count = network.node_count();
    let edge_count = network.edge_count();

    {
        let mut guard = state
            .network
            .write()
            .expect("network lock should not be poisoned");
        *guard = Arc::new(network.clone());
    }

    if let Some(node_id) = removed_node {
        let mut guard = state
            .default_demands
            .write()
            .expect("default demands lock should not be poisoned");
        Arc::make_mut(&mut *guard).remove(node_id);
    }

    let active_id = active_dataset_id(state);
    if active_id.starts_with("import-") {
        let mut imported = state
            .imported
            .write()
            .expect("imported lock should not be poisoned");
        if let Some(dataset) = imported.get_mut(&active_id) {
            dataset.network = clone_network(&network);
            if let Some(node_id) = removed_node {
                dataset.default_demands.remove(node_id);
            }
        }
    }

    NetworkMutationResponse {
        node_count,
        edge_count,
    }
}

fn ensure_network_editable(state: &SharedState) -> Result<(), (StatusCode, Json<ApiError>)> {
    if state.simulation_slots.available_permits() != state.simulation_capacity {
        return Err(api_error(
            StatusCode::CONFLICT,
            "cannot edit network while simulations are running",
        ));
    }
    Ok(())
}

fn validation_bad_request(err: ValidationError) -> (StatusCode, Json<ApiError>) {
    api_error(StatusCode::BAD_REQUEST, err.to_string())
}

fn api_error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: message.into(),
        }),
    )
}

fn network_to_raw(network: &GasNetwork) -> RawNetwork {
    let nodes: Vec<RawNode> = network
        .nodes()
        .map(|node| RawNode {
            id: node.id.clone(),
            role: if node.pressure_fixed_bar.is_some() {
                RawNodeRole::Source
            } else {
                RawNodeRole::Innode
            },
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
        })
        .collect();

    let pipes: Vec<RawPipe> = network
        .pipes()
        .map(|pipe| RawPipe {
            id: pipe.id.clone(),
            from: pipe.from.clone(),
            to: pipe.to.clone(),
            kind: pipe.kind,
            is_open: pipe.is_open,
            length_km: pipe.length_km,
            diameter_mm: pipe.diameter_mm,
            roughness_mm: pipe.roughness_mm,
            compressor_ratio_max: pipe.compressor_ratio_max,
            flow_min_m3s: pipe.flow_min_m3s,
            flow_max_m3s: pipe.flow_max_m3s,
            equipment: pipe.equipment.clone(),
        })
        .collect();

    let source = nodes
        .iter()
        .find(|node| node.pressure_fixed_bar.is_some())
        .map(|node| node.id.clone());

    RawNetwork {
        nodes,
        pipes,
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use serde_json::Value;
    use tower::ServiceExt;

    use super::super::create_router_with_runtime_limits;
    use super::*;

    fn test_router() -> Router {
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
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        create_router_with_runtime_limits(net, HashMap::new(), 2, 1)
    }

    async fn send_json(
        app: &Router,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        let request = match body {
            Some(body) => {
                builder = builder.header("content-type", "application/json");
                builder
                    .body(Body::from(body.to_string()))
                    .expect("request body")
            }
            None => builder.body(Body::empty()).expect("empty request"),
        };

        let response = app.clone().oneshot(request).await.expect("response");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let json = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).expect("json body")
        };
        (status, json)
    }

    #[tokio::test]
    async fn add_node_returns_updated_counts() {
        let app = test_router();
        let (status, body) = send_json(
            &app,
            "POST",
            "/api/network/nodes",
            Some(serde_json::json!({
                "id": "branch",
                "x": 2.0,
                "y": 0.5
            })),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["node_count"], 3);
        assert_eq!(body["edge_count"], 1);
    }

    #[tokio::test]
    async fn add_pipe_returns_updated_counts() {
        let app = test_router();

        let (node_status, _) = send_json(
            &app,
            "POST",
            "/api/network/nodes",
            Some(serde_json::json!({
                "id": "branch",
                "x": 2.0,
                "y": 0.0
            })),
        )
        .await;
        assert_eq!(node_status, StatusCode::OK);

        let (status, body) = send_json(
            &app,
            "POST",
            "/api/network/pipes",
            Some(serde_json::json!({
                "id": "p2",
                "from": "source",
                "to": "branch",
                "kind": "pipe",
                "length_km": 1.2,
                "diameter_mm": 300.0
            })),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["node_count"], 3);
        assert_eq!(body["edge_count"], 2);
    }

    #[tokio::test]
    async fn delete_node_cascades_connected_pipes() {
        let app = test_router();
        let (status, body) = send_json(&app, "DELETE", "/api/network/nodes/sink", None).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["node_count"], 1);
        assert_eq!(body["edge_count"], 0);
    }

    #[tokio::test]
    async fn invalid_pipe_endpoint_is_rejected() {
        let app = test_router();
        let (status, body) = send_json(
            &app,
            "POST",
            "/api/network/pipes",
            Some(serde_json::json!({
                "id": "p-invalid",
                "from": "source",
                "to": "unknown",
                "kind": "pipe",
                "length_km": 2.0,
                "diameter_mm": 200.0
            })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().expect("error").contains("inconnu"));
    }
}
