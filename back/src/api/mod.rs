//! API REST exposée via Axum.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;
use tower_http::cors::CorsLayer;

use crate::graph::GasNetwork;
use crate::solver;

#[derive(Clone)]
struct AppState {
    network: Arc<GasNetwork>,
    default_demands: Arc<HashMap<String, f64>>,
}

type SharedState = Arc<AppState>;

pub fn create_router(network: GasNetwork, default_demands: HashMap<String, f64>) -> Router {
    let shared: SharedState = Arc::new(AppState {
        network: Arc::new(network),
        default_demands: Arc::new(default_demands),
    });

    Router::new()
        .route("/api/health", get(health))
        .route("/api/network", get(get_network))
        .route("/api/simulate", get(run_simulation))
        .layer(CorsLayer::permissive())
        .with_state(shared)
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

async fn run_simulation(State(state): State<SharedState>) -> Json<solver::SolverResult> {
    let demands = (*state.default_demands).clone();
    let result =
        solver::solve_steady_state(&state.network, &demands, 200, 1e-4).unwrap_or_else(|_| {
            solver::SolverResult {
                pressures: HashMap::new(),
                flows: HashMap::new(),
                iterations: 0,
                residual: f64::MAX,
            }
        });

    Json(result)
}
