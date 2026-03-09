//! API REST exposée via Axum.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tower_http::cors::CorsLayer;

use crate::graph::GasNetwork;
use crate::solver;

mod ws;

#[derive(Clone)]
struct AppState {
    network: Arc<GasNetwork>,
    default_demands: Arc<HashMap<String, f64>>,
    simulation_slots: Arc<Semaphore>,
}

type SharedState = Arc<AppState>;
type ApiResult<T> = Result<Json<T>, (StatusCode, Json<ApiError>)>;

pub fn create_router(network: GasNetwork, default_demands: HashMap<String, f64>) -> Router {
    create_router_with_limits(
        network,
        default_demands,
        max_concurrent_simulations_from_env(),
    )
}

pub fn create_router_with_limits(
    network: GasNetwork,
    default_demands: HashMap<String, f64>,
    max_concurrent_simulations: usize,
) -> Router {
    let shared: SharedState = Arc::new(AppState {
        network: Arc::new(network),
        default_demands: Arc::new(default_demands),
        simulation_slots: Arc::new(Semaphore::new(max_concurrent_simulations.max(1))),
    });

    Router::new()
        .route("/api/health", get(health))
        .route("/api/network", get(get_network))
        .route("/api/ws/sim", get(ws::ws_simulation_handler))
        .route(
            "/api/simulate",
            get(run_simulation_default).post(run_simulation_custom),
        )
        .layer(CorsLayer::permissive())
        .with_state(shared)
}

fn max_concurrent_simulations_from_env() -> usize {
    std::env::var("GAZSIM_MAX_CONCURRENT_SIMULATIONS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(2)
}

async fn health() -> &'static str {
    "ok"
}

#[derive(Serialize)]
struct NetworkResponse {
    node_count: usize,
    edge_count: usize,
    nodes: Vec<NodeDto>,
    pipes: Vec<PipeDto>,
}

#[derive(Serialize)]
struct NodeDto {
    id: String,
    lon: Option<f64>,
    lat: Option<f64>,
    height_m: f64,
}

#[derive(Serialize)]
struct PipeDto {
    id: String,
    from: String,
    to: String,
    length_km: f64,
    diameter_mm: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct SimulateRequest {
    demands: HashMap<String, f64>,
}

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
}

async fn get_network(State(state): State<SharedState>) -> Json<NetworkResponse> {
    let nodes: Vec<NodeDto> = state
        .network
        .nodes()
        .map(|n| NodeDto {
            id: n.id.clone(),
            lon: n.lon,
            lat: n.lat,
            height_m: n.height_m,
        })
        .collect();

    let pipes: Vec<PipeDto> = state
        .network
        .pipes()
        .map(|p| PipeDto {
            id: p.id.clone(),
            from: p.from.clone(),
            to: p.to.clone(),
            length_km: p.length_km,
            diameter_mm: p.diameter_mm,
        })
        .collect();

    Json(NetworkResponse {
        node_count: state.network.node_count(),
        edge_count: state.network.edge_count(),
        nodes,
        pipes,
    })
}

async fn run_simulation_default(
    State(state): State<SharedState>,
) -> ApiResult<solver::SolverResult> {
    let demands = (*state.default_demands).clone();
    run_simulation_with_demands(&state, demands)
}

async fn run_simulation_custom(
    State(state): State<SharedState>,
    Json(payload): Json<SimulateRequest>,
) -> ApiResult<solver::SolverResult> {
    run_simulation_with_demands(&state, payload.demands)
}

fn run_simulation_with_demands(
    state: &SharedState,
    demands: HashMap<String, f64>,
) -> ApiResult<solver::SolverResult> {
    solver::solve_steady_state(&state.network, &demands, 1000, 5e-4)
        .map(Json)
        .map_err(|err| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ApiError {
                    error: err.to_string(),
                }),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use serde_json::Value;
    use tower::ServiceExt;

    use crate::graph::{ConnectionKind, Node, Pipe};

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
        });
        net.add_pipe(Pipe {
            id: "p1".into(),
            from: "source".into(),
            to: "sink".into(),
            kind: ConnectionKind::Pipe,
            length_km: 10.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
        });

        let mut defaults = HashMap::new();
        defaults.insert("sink".to_string(), -5.0);
        create_router_with_limits(net, defaults, 4)
    }

    #[tokio::test]
    async fn test_api_network_count() {
        let app = test_router();
        let req = Request::builder()
            .method("GET")
            .uri("/api/network")
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("read body");
        let json: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(json.get("node_count").and_then(Value::as_u64), Some(2));
        assert_eq!(json.get("edge_count").and_then(Value::as_u64), Some(1));
    }

    #[tokio::test]
    async fn test_api_simulate_post_custom_demands_returns_result() {
        let app = test_router();
        let payload = serde_json::json!({
            "demands": {
                "sink": -8.0
            }
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/simulate")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("read body");
        let json: Value = serde_json::from_slice(&body).expect("json body");
        assert!(json.get("pressures").is_some(), "pressures field missing");
        assert!(json.get("flows").is_some(), "flows field missing");
    }

    #[tokio::test]
    async fn test_api_simulate_invalid_demands_returns_422() {
        let app = test_router();
        let payload = serde_json::json!({
            "demands": {
                "UNKNOWN": -1.0
            }
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/simulate")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let body = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("read body");
        let json: Value = serde_json::from_slice(&body).expect("json body");
        let err = json
            .get("error")
            .and_then(Value::as_str)
            .expect("error message");
        assert!(
            err.contains("unknown demand node id"),
            "unexpected error message: {err}"
        );
    }
}
