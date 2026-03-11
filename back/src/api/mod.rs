//! API REST exposée via Axum.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use rayon::{ThreadPool, ThreadPoolBuilder};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tower_http::cors::CorsLayer;

use crate::gaslib;
use crate::graph::GasNetwork;
use crate::solver;

mod export;
mod ws;

#[derive(Clone)]
struct AppState {
    network: Arc<RwLock<Arc<GasNetwork>>>,
    default_demands: Arc<RwLock<Arc<HashMap<String, f64>>>>,
    active_dataset: Arc<RwLock<String>>,
    available_datasets: Arc<Vec<String>>,
    data_dir: Arc<PathBuf>,
    simulation_slots: Arc<Semaphore>,
    simulation_capacity: usize,
    rayon_pool: Arc<ThreadPool>,
    exports: Arc<RwLock<HashMap<String, export::ExportRecord>>>,
}

type SharedState = Arc<AppState>;
type ApiResult<T> = Result<Json<T>, (StatusCode, Json<ApiError>)>;

pub fn create_router(network: GasNetwork, default_demands: HashMap<String, f64>) -> Router {
    create_router_with_datasets(
        network,
        default_demands,
        "custom".to_string(),
        vec!["custom".to_string()],
        PathBuf::from("dat"),
    )
}

pub fn create_router_with_datasets(
    network: GasNetwork,
    default_demands: HashMap<String, f64>,
    active_dataset: String,
    available_datasets: Vec<String>,
    data_dir: PathBuf,
) -> Router {
    create_router_with_limits_and_datasets(
        network,
        default_demands,
        active_dataset,
        available_datasets,
        data_dir,
        max_concurrent_simulations_from_env(),
    )
}

pub fn create_router_with_limits_and_datasets(
    network: GasNetwork,
    default_demands: HashMap<String, f64>,
    active_dataset: String,
    available_datasets: Vec<String>,
    data_dir: PathBuf,
    max_concurrent_simulations: usize,
) -> Router {
    create_router_with_runtime_limits_and_datasets(
        network,
        default_demands,
        active_dataset,
        available_datasets,
        data_dir,
        max_concurrent_simulations,
        rayon_threads_from_env(max_concurrent_simulations),
    )
}

pub fn create_router_with_limits(
    network: GasNetwork,
    default_demands: HashMap<String, f64>,
    max_concurrent_simulations: usize,
) -> Router {
    create_router_with_limits_and_datasets(
        network,
        default_demands,
        "custom".to_string(),
        vec!["custom".to_string()],
        PathBuf::from("dat"),
        max_concurrent_simulations,
    )
}

pub fn create_router_with_runtime_limits(
    network: GasNetwork,
    default_demands: HashMap<String, f64>,
    max_concurrent_simulations: usize,
    rayon_threads: usize,
) -> Router {
    create_router_with_runtime_limits_and_datasets(
        network,
        default_demands,
        "custom".to_string(),
        vec!["custom".to_string()],
        PathBuf::from("dat"),
        max_concurrent_simulations,
        rayon_threads,
    )
}

pub fn create_router_with_runtime_limits_and_datasets(
    network: GasNetwork,
    default_demands: HashMap<String, f64>,
    active_dataset: String,
    available_datasets: Vec<String>,
    data_dir: PathBuf,
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
        network: Arc::new(RwLock::new(Arc::new(network))),
        default_demands: Arc::new(RwLock::new(Arc::new(default_demands))),
        active_dataset: Arc::new(RwLock::new(active_dataset)),
        available_datasets: Arc::new(available_datasets),
        data_dir: Arc::new(data_dir),
        simulation_slots: Arc::new(Semaphore::new(simulation_capacity)),
        simulation_capacity,
        rayon_pool: Arc::new(rayon_pool),
        exports: Arc::new(RwLock::new(HashMap::new())),
    });

    Router::new()
        .route("/api/health", get(health))
        .route("/api/networks", get(list_networks))
        .route("/api/network", get(get_network).post(select_network))
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
    active_dataset: String,
    node_count: usize,
    edge_count: usize,
    nodes: Vec<NodeDto>,
    pipes: Vec<PipeDto>,
}

#[derive(Serialize)]
struct NetworksResponse {
    available: Vec<String>,
    active: String,
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

#[derive(Debug, Deserialize)]
struct SelectNetworkRequest {
    dataset_id: String,
}

#[derive(Debug, Serialize)]
struct SelectNetworkResponse {
    active: String,
    node_count: usize,
    edge_count: usize,
}

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
}

async fn list_networks(State(state): State<SharedState>) -> Json<NetworksResponse> {
    Json(NetworksResponse {
        available: state.available_datasets.as_ref().clone(),
        active: active_dataset_id(&state),
    })
}

async fn get_network(State(state): State<SharedState>) -> Json<NetworkResponse> {
    let network = active_network(&state);
    let active_dataset = active_dataset_id(&state);
    let nodes: Vec<NodeDto> = network
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

    let pipes: Vec<PipeDto> = network
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
        active_dataset,
        node_count: network.node_count(),
        edge_count: network.edge_count(),
        nodes,
        pipes,
    })
}

async fn select_network(
    State(state): State<SharedState>,
    Json(payload): Json<SelectNetworkRequest>,
) -> Result<Json<SelectNetworkResponse>, (StatusCode, Json<ApiError>)> {
    if !state.available_datasets.iter().any(|id| id == &payload.dataset_id) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: format!("unknown dataset id: {}", payload.dataset_id),
            }),
        ));
    }

    if state.simulation_slots.available_permits() != state.simulation_capacity {
        return Err((
            StatusCode::CONFLICT,
            Json(ApiError {
                error: "cannot switch dataset while simulations are running".to_string(),
            }),
        ));
    }

    let (network, default_demands) = load_dataset_from_disk(&state.data_dir, &payload.dataset_id)
        .map_err(|err| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ApiError { error: err }),
            )
        })?;
    let node_count = network.node_count();
    let edge_count = network.edge_count();

    {
        let mut guard = state
            .network
            .write()
            .expect("network lock should not be poisoned");
        *guard = Arc::new(network);
    }
    {
        let mut guard = state
            .default_demands
            .write()
            .expect("default demands lock should not be poisoned");
        *guard = Arc::new(default_demands);
    }
    {
        let mut guard = state
            .active_dataset
            .write()
            .expect("active dataset lock should not be poisoned");
        *guard = payload.dataset_id.clone();
    }

    Ok(Json(SelectNetworkResponse {
        active: payload.dataset_id,
        node_count,
        edge_count,
    }))
}

async fn run_simulation_default(
    State(state): State<SharedState>,
) -> ApiResult<solver::SolverResult> {
    let demands = (*active_default_demands(&state)).clone();
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
    let network = active_network(state);
    let network_for_solve = network.clone();
    let network_id = active_dataset_id(state);
    let permit = state
        .simulation_slots
        .clone()
        .acquire_owned()
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: format!("simulation capacity semaphore closed: {err}"),
                }),
            )
        })?;
    let pool = state.rayon_pool.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        pool.install(|| solver::solve_steady_state(&network_for_solve, &demands, 1000, 5e-4))
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
        export::new_export_record(
            export_id,
            network_id,
            &network,
            demands_for_export,
            result.clone(),
            0,
        ),
    );

    Ok(Json(result))
}

fn active_network(state: &SharedState) -> Arc<GasNetwork> {
    state
        .network
        .read()
        .expect("network lock should not be poisoned")
        .clone()
}

fn active_default_demands(state: &SharedState) -> Arc<HashMap<String, f64>> {
    state
        .default_demands
        .read()
        .expect("default demands lock should not be poisoned")
        .clone()
}

fn active_dataset_id(state: &SharedState) -> String {
    state
        .active_dataset
        .read()
        .expect("active dataset lock should not be poisoned")
        .clone()
}

fn load_dataset_from_disk(
    data_dir: &Path,
    dataset_id: &str,
) -> Result<(GasNetwork, HashMap<String, f64>), String> {
    let network_path = data_dir.join(format!("{dataset_id}.net"));
    let network = gaslib::load_network(&network_path)
        .map_err(|err| format!("failed to load network {:?}: {err:#}", network_path))?;

    let scenario_path = data_dir.join(format!("{dataset_id}.scn"));
    let default_demands = if scenario_path.exists() {
        match gaslib::load_scenario_demands(&scenario_path) {
            Ok(parsed) => {
                let scenario: gaslib::ScenarioDemands = parsed;
                scenario.demands
            }
            Err(err) => {
                tracing::warn!("Impossible de charger {:?}: {err:#}", scenario_path);
                HashMap::new()
            }
        }
    } else {
        HashMap::new()
    };

    Ok((network, default_demands))
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
