//! API REST exposée via Axum.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use rayon::{ThreadPool, ThreadPoolBuilder};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tower_http::cors::CorsLayer;

use crate::graph::GasNetwork;
use crate::solver;

mod export;
mod ws;

#[derive(Clone)]
struct AppState {
    network: Arc<GasNetwork>,
    default_demands: Arc<HashMap<String, f64>>,
    simulation_slots: Arc<Semaphore>,
    rayon_pool: Arc<ThreadPool>,
    exports: Arc<RwLock<HashMap<String, export::ExportRecord>>>,
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
    create_router_with_runtime_limits(
        network,
        default_demands,
        max_concurrent_simulations,
        rayon_threads_from_env(max_concurrent_simulations),
    )
}

pub fn create_router_with_runtime_limits(
    network: GasNetwork,
    default_demands: HashMap<String, f64>,
    max_concurrent_simulations: usize,
    rayon_threads: usize,
) -> Router {
    let simulation_capacity = max_concurrent_simulations.max(1);
    let rayon_threads = rayon_threads.max(1);
    let rayon_pool = ThreadPoolBuilder::new()
        .num_threads(rayon_threads)
        .thread_name(|idx| format!("solver-rayon-{idx}"))
        .build()
        .expect("build solver rayon pool");

    tracing::info!(
        simulation_capacity,
        rayon_threads,
        "Configured solver runtime limits"
    );

    let shared: SharedState = Arc::new(AppState {
        network: Arc::new(network),
        default_demands: Arc::new(default_demands),
        simulation_slots: Arc::new(Semaphore::new(simulation_capacity)),
        rayon_pool: Arc::new(rayon_pool),
        exports: Arc::new(RwLock::new(HashMap::new())),
    });

    Router::new()
        .route("/api/health", get(health))
        .route("/api/network", get(get_network))
        .route("/api/export/{simulation_id}", get(export::get_export))
        .route("/api/ws/sim", get(ws::ws_simulation_handler))
        .route(
            "/api/simulate",
            get(run_simulation_default).post(run_simulation_custom),
        )
        .layer(CorsLayer::permissive())
        .with_state(shared)
}

fn max_concurrent_simulations_from_env() -> usize {
    std::env::var("GAZFLOW_MAX_CONCURRENT_SIMULATIONS")
        .ok()
        .or_else(|| std::env::var("GAZSIM_MAX_CONCURRENT_SIMULATIONS").ok())
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(2)
}

fn rayon_threads_from_env(max_concurrent_simulations: usize) -> usize {
    if let Some(value) = std::env::var("GAZFLOW_RAYON_THREADS")
        .ok()
        .or_else(|| std::env::var("GAZSIM_RAYON_THREADS").ok())
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|&n| n > 0)
    {
        return value;
    }

    let cpu = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    // Heuristique anti-contention: répartir les cores selon le nombre max de solves concurrents.
    (cpu / max_concurrent_simulations.max(1)).max(1)
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
    x: f64,
    y: f64,
    lon: Option<f64>,
    lat: Option<f64>,
    height_m: f64,
    pressure_fixed_bar: Option<f64>,
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
            x: n.x,
            y: n.y,
            lon: n.lon,
            lat: n.lat,
            height_m: n.height_m,
            pressure_fixed_bar: n.pressure_fixed_bar,
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
    run_simulation_with_demands(&state, demands).await
}

async fn run_simulation_custom(
    State(state): State<SharedState>,
    Json(payload): Json<SimulateRequest>,
) -> ApiResult<solver::SolverResult> {
    run_simulation_with_demands(&state, payload.demands).await
}

async fn run_simulation_with_demands(
    state: &SharedState,
    demands: HashMap<String, f64>,
) -> ApiResult<solver::SolverResult> {
    let demands_for_export = demands.clone();
    let network = state.network.clone();
    let pool = state.rayon_pool.clone();
    let result = tokio::task::spawn_blocking(move || {
        pool.install(|| solver::solve_steady_state(&network, &demands, 1000, 5e-4))
    })
    .await
    .map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: format!("simulation task join error: {err}"),
            }),
        )
    })?
    .map_err(|err| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError {
                error: err.to_string(),
            }),
        )
    })?;

    let export_id = format!(
        "rest-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    export::store_export_record(
        state,
        export::new_export_record(export_id, demands_for_export, result.clone(), 0),
    );

    Ok(Json(result))
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
            is_open: true,
            length_km: 10.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
        });

        let mut defaults = HashMap::new();
        defaults.insert("sink".to_string(), -5.0);
        create_router_with_runtime_limits(net, defaults, 4, 2)
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
