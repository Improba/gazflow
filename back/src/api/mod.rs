//! API REST exposée via Axum.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Router,
    Json,
    extract::State,
    routing::get,
};
use serde::Serialize;
use tower_http::cors::CorsLayer;

use crate::graph::GasNetwork;
use crate::solver;

type SharedNetwork = Arc<GasNetwork>;

pub fn create_router(network: GasNetwork) -> Router {
    let shared: SharedNetwork = Arc::new(network);

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

async fn get_network(State(net): State<SharedNetwork>) -> Json<NetworkResponse> {
    let nodes: Vec<NodeDto> = net
        .nodes()
        .map(|n| NodeDto {
            id: n.id.clone(),
            lon: n.lon,
            lat: n.lat,
            height_m: n.height_m,
        })
        .collect();

    let pipes: Vec<PipeDto> = net
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
        node_count: net.node_count(),
        edge_count: net.edge_count(),
        nodes,
        pipes,
    })
}

async fn run_simulation(State(net): State<SharedNetwork>) -> Json<solver::SolverResult> {
    let demands = HashMap::new();
    let result = solver::solve_steady_state(&net, &demands, 200, 1e-4)
        .unwrap_or_else(|_| solver::SolverResult {
            pressures: HashMap::new(),
            flows: HashMap::new(),
            iterations: 0,
            residual: f64::MAX,
        });

    Json(result)
}
