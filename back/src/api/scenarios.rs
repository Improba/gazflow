//! CRUD scénarios topologiques par jeu de données (P12).
//!
//! Persistance SQLite via `ScenarioRepo`. La baseline (snapshot du réseau de base du
//! dataset) reste en RAM, recalculée à la volée depuis le dataset actif — elle n'a pas
//! vocation à être persistée (un dataset rechargé retrouve sa baseline naturellement).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::graph::GasNetwork;
use crate::graph::scenarios::{
    NetworkDiff, NetworkSnapshot, apply_diff, compute_snapshot_diff, validate_diff,
};
use crate::store::ScenarioRepo;

use super::{
    ApiError, NodeDto, PipeDto, SharedState, active_dataset_id, active_default_demands,
    active_gas_composition, active_network, clone_network,
};

/// Cache en RAM des baselines par dataset (snapshot du réseau de base non modifié).
pub(crate) type ScenarioBaselines = Arc<RwLock<HashMap<String, NetworkSnapshot>>>;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ScenarioSummary {
    pub id: String,
    pub name: String,
    pub created_at_ms: u64,
    pub node_delta: usize,
    pub pipe_delta: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ScenarioDetail {
    pub id: String,
    pub name: String,
    pub created_at_ms: u64,
    pub diff: NetworkDiff,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateScenarioRequest {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ApplyScenarioResponse {
    pub scenario_id: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub nodes: Vec<NodeDto>,
    pub pipes: Vec<PipeDto>,
}

fn api_error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: message.into(),
        }),
    )
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn diff_entity_count(diff: &NetworkDiff) -> (usize, usize) {
    let node_delta = diff.nodes.added.len()
        + diff.nodes.updated.len()
        + diff.nodes.removed.len();
    let pipe_delta = diff.pipes.added.len()
        + diff.pipes.updated.len()
        + diff.pipes.removed.len();
    (node_delta, pipe_delta)
}

fn network_to_dtos(network: &GasNetwork) -> (Vec<NodeDto>, Vec<PipeDto>) {
    let nodes = network
        .nodes()
        .map(|n| NodeDto {
            id: n.id.clone(),
            x: n.x,
            y: n.y,
            lon: n.lon,
            lat: n.lat,
            height_m: n.height_m,
            pressure_fixed_bar: n.pressure_fixed_bar,
            flow_min_m3s: n.flow_min_m3s,
            flow_max_m3s: n.flow_max_m3s,
        })
        .collect();
    let pipes = network
        .pipes()
        .map(|p| PipeDto {
            id: p.id.clone(),
            from: p.from.clone(),
            to: p.to.clone(),
            kind: p.kind,
            length_km: p.length_km,
            diameter_mm: p.diameter_mm,
            equipment: p.equipment.clone(),
        })
        .collect();
    (nodes, pipes)
}

pub(crate) fn ensure_baseline(
    baselines: &ScenarioBaselines,
    dataset_id: &str,
    network: &GasNetwork,
) {
    let mut guard = baselines
        .write()
        .expect("scenario baselines lock should not be poisoned");
    guard
        .entry(dataset_id.to_string())
        .or_insert_with(|| NetworkSnapshot::from_network(network));
}

fn baseline_network(
    baselines: &ScenarioBaselines,
    dataset_id: &str,
    fallback: &GasNetwork,
) -> Result<GasNetwork, (StatusCode, Json<ApiError>)> {
    let guard = baselines
        .read()
        .expect("scenario baselines lock should not be poisoned");
    match guard.get(dataset_id) {
        Some(snapshot) => snapshot
            .clone()
            .to_network()
            .map_err(|err| api_error(StatusCode::UNPROCESSABLE_ENTITY, err.to_string())),
        None => Ok(clone_network(fallback)),
    }
}

pub(super) async fn list_scenarios(
    State(state): State<SharedState>,
) -> Json<Vec<ScenarioSummary>> {
    let dataset_id = active_dataset_id(&state);
    let records = state
        .scenario_repo
        .list_topological_scenarios(&dataset_id)
        .unwrap_or_default();
    let summaries = records
        .into_iter()
        .filter_map(|rec| {
            let diff: NetworkDiff = serde_json::from_str(&rec.diff_json).ok()?;
            let (node_delta, pipe_delta) = diff_entity_count(&diff);
            Some(ScenarioSummary {
                id: rec.id,
                name: rec.name,
                created_at_ms: rec.created_at_ms,
                node_delta,
                pipe_delta,
            })
        })
        .collect();
    Json(summaries)
}

pub(super) async fn create_scenario(
    State(state): State<SharedState>,
    Json(payload): Json<CreateScenarioRequest>,
) -> Result<Json<ScenarioDetail>, (StatusCode, Json<ApiError>)> {
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "scenario name must not be empty",
        ));
    }

    let dataset_id = active_dataset_id(&state);
    let current = active_network(&state);
    let current_snapshot = NetworkSnapshot::from_network(&current);

    ensure_baseline(&state.scenario_baselines, &dataset_id, &current);
    let baseline = {
        let guard = state
            .scenario_baselines
            .read()
            .expect("scenario baselines lock should not be poisoned");
        guard
            .get(&dataset_id)
            .cloned()
            .expect("baseline just ensured")
    };
    let diff = compute_snapshot_diff(&baseline, &current_snapshot);
    validate_diff(&diff).map_err(|err| api_error(StatusCode::BAD_REQUEST, err.to_string()))?;

    let id = format!("scn-{}", now_ms());
    let created_at_ms = now_ms();
    let diff_json = serde_json::to_string(&diff)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    let record = crate::store::TopologicalScenarioRecord {
        id: id.clone(),
        dataset_id: dataset_id.clone(),
        name: name.to_string(),
        created_at_ms,
        diff_json,
    };
    state
        .scenario_repo
        .insert_topological_scenario(&record)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    Ok(Json(ScenarioDetail {
        id,
        name: name.to_string(),
        created_at_ms,
        diff,
    }))
}

pub(super) async fn get_scenario(
    State(state): State<SharedState>,
    Path(scenario_id): Path<String>,
) -> Result<Json<ScenarioDetail>, (StatusCode, Json<ApiError>)> {
    let record = state
        .scenario_repo
        .get_topological_scenario(&scenario_id)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("scenario not found: {scenario_id}")))?;
    let diff: NetworkDiff = serde_json::from_str(&record.diff_json)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    Ok(Json(ScenarioDetail {
        id: record.id,
        name: record.name,
        created_at_ms: record.created_at_ms,
        diff,
    }))
}

pub(super) async fn delete_scenario(
    State(state): State<SharedState>,
    Path(scenario_id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    let removed = state
        .scenario_repo
        .delete_topological_scenario(&scenario_id)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    if !removed {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("scenario not found: {scenario_id}"),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn apply_scenario(
    State(state): State<SharedState>,
    Path(scenario_id): Path<String>,
) -> Result<Json<ApplyScenarioResponse>, (StatusCode, Json<ApiError>)> {
    let dataset_id = active_dataset_id(&state);
    let active = active_network(&state);
    let record = state
        .scenario_repo
        .get_topological_scenario(&scenario_id)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("scenario not found: {scenario_id}")))?;
    let diff: NetworkDiff = serde_json::from_str(&record.diff_json)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    let base = baseline_network(&state.scenario_baselines, &dataset_id, &active)?;
    let network = apply_diff(&base, &diff)
        .map_err(|err| api_error(StatusCode::UNPROCESSABLE_ENTITY, err.to_string()))?;
    let (nodes, pipes) = network_to_dtos(&network);

    Ok(Json(ApplyScenarioResponse {
        scenario_id,
        node_count: network.node_count(),
        edge_count: network.edge_count(),
        nodes,
        pipes,
    }))
}

pub(crate) fn resolve_scenario_network(
    state: &SharedState,
    scenario_id: Option<&str>,
) -> Result<GasNetwork, (StatusCode, Json<ApiError>)> {
    match scenario_id {
        None => Ok(clone_network(&active_network(state))),
        Some(id) => {
            let dataset_id = active_dataset_id(state);
            let active = active_network(state);
            let record = state
                .scenario_repo
                .get_topological_scenario(id)
                .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
                .ok_or_else(|| {
                    api_error(StatusCode::NOT_FOUND, format!("scenario not found: {id}"))
                })?;
            let diff: NetworkDiff = serde_json::from_str(&record.diff_json)
                .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
            let base = baseline_network(&state.scenario_baselines, &dataset_id, &active)?;
            apply_diff(&base, &diff)
                .map_err(|err| api_error(StatusCode::UNPROCESSABLE_ENTITY, err.to_string()))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct CompareScenariosRequest {
    #[serde(default)]
    pub scenario_a_id: Option<String>,
    #[serde(default)]
    pub scenario_b_id: Option<String>,
    #[serde(default)]
    pub demands: Option<HashMap<String, f64>>,
}

#[derive(Debug, Serialize)]
pub(super) struct CompareScenariosResponse {
    pub scenario_a_id: Option<String>,
    pub scenario_b_id: Option<String>,
    pub pressures_a: HashMap<String, f64>,
    pub pressures_b: HashMap<String, f64>,
    pub flows_a: HashMap<String, f64>,
    pub flows_b: HashMap<String, f64>,
    pub delta_pressures: HashMap<String, f64>,
    pub delta_flows: HashMap<String, f64>,
    pub summary: CompareSummary,
}

#[derive(Debug, Serialize)]
pub(super) struct CompareSummary {
    pub max_abs_delta_p_bar: f64,
    pub max_abs_delta_q_m3s: f64,
    pub nodes_compared: usize,
    pub pipes_compared: usize,
}

pub(super) async fn compare_scenarios(
    State(state): State<SharedState>,
    Json(payload): Json<CompareScenariosRequest>,
) -> Result<Json<CompareScenariosResponse>, (StatusCode, Json<ApiError>)> {
    let demands = payload
        .demands
        .unwrap_or_else(|| (*active_default_demands(&state)).clone());

    let network_a = resolve_scenario_network(&state, payload.scenario_a_id.as_deref())?;
    let network_b = resolve_scenario_network(&state, payload.scenario_b_id.as_deref())?;

    let gas_composition = active_gas_composition(&state);
    let result_a = crate::solver::solve_steady_state_with_composition(
        &network_a,
        &demands,
        gas_composition,
        1000,
        5e-4,
    )
    .map_err(|err| api_error(StatusCode::UNPROCESSABLE_ENTITY, err.to_string()))?;
    let result_b = crate::solver::solve_steady_state_with_composition(
        &network_b,
        &demands,
        gas_composition,
        1000,
        5e-4,
    )
    .map_err(|err| api_error(StatusCode::UNPROCESSABLE_ENTITY, err.to_string()))?;

    let mut delta_pressures = HashMap::new();
    let all_pressure_ids: HashSet<String> = result_a
        .pressures
        .keys()
        .chain(result_b.pressures.keys())
        .cloned()
        .collect();
    let mut max_abs_delta_p = 0.0_f64;
    for id in &all_pressure_ids {
        let pa = result_a.pressures.get(id).copied().unwrap_or(0.0);
        let pb = result_b.pressures.get(id).copied().unwrap_or(0.0);
        let delta = pb - pa;
        max_abs_delta_p = max_abs_delta_p.max(delta.abs());
        delta_pressures.insert(id.clone(), delta);
    }

    let mut delta_flows = HashMap::new();
    let all_flow_ids: HashSet<String> = result_a
        .flows
        .keys()
        .chain(result_b.flows.keys())
        .cloned()
        .collect();
    let mut max_abs_delta_q = 0.0_f64;
    for id in &all_flow_ids {
        let qa = result_a.flows.get(id).copied().unwrap_or(0.0);
        let qb = result_b.flows.get(id).copied().unwrap_or(0.0);
        let delta = qb - qa;
        max_abs_delta_q = max_abs_delta_q.max(delta.abs());
        delta_flows.insert(id.clone(), delta);
    }

    Ok(Json(CompareScenariosResponse {
        scenario_a_id: payload.scenario_a_id,
        scenario_b_id: payload.scenario_b_id,
        pressures_a: result_a.pressures,
        pressures_b: result_b.pressures,
        flows_a: result_a.flows,
        flows_b: result_b.flows,
        delta_pressures,
        delta_flows,
        summary: CompareSummary {
            max_abs_delta_p_bar: max_abs_delta_p,
            max_abs_delta_q_m3s: max_abs_delta_q,
            nodes_compared: all_pressure_ids.len(),
            pipes_compared: all_flow_ids.len(),
        },
    }))
}
