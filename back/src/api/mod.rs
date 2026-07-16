//! API REST exposée via Axum.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, patch, post, put},
};
use rayon::{ThreadPool, ThreadPoolBuilder};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tower_http::cors::CorsLayer;

use crate::calibration;
use crate::gaslib;
use crate::graph::{ConnectionKind, EquipmentSpec, GasNetwork};
use crate::solver;
use crate::store::ScenarioRepo;

mod export;
mod import;
mod batch;
mod compressor;
mod network_edit;
mod nova;
mod nova_finalize;
mod scenarios;
mod ws;

pub(crate) struct ImportedDataset {
    pub network: GasNetwork,
    pub default_demands: HashMap<String, f64>,
    pub gas_composition: solver::GasComposition,
}

#[derive(Clone, Debug, Serialize)]
pub struct GasPropertiesDto {
    pub composition: solver::GasComposition,
    pub pcs_mj_per_nm3: f64,
    pub pci_mj_per_nm3: f64,
    pub wobbe_mj_per_nm3: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl GasPropertiesDto {
    pub fn from_composition(composition: solver::GasComposition) -> Self {
        Self {
            pcs_mj_per_nm3: composition.pcs_mj_per_nm3(),
            pci_mj_per_nm3: composition.pci_mj_per_nm3(),
            wobbe_mj_per_nm3: composition.wobbe_mj_per_nm3(),
            warnings: composition.physics_warnings(),
            composition,
        }
    }
}

#[derive(Clone)]
pub(crate) struct LastSimulationSnapshot {
    pub demands: HashMap<String, f64>,
    pub result: solver::SolverResult,
}

#[derive(Clone)]
pub(crate) struct AppState {
    network: Arc<RwLock<Arc<GasNetwork>>>,
    default_demands: Arc<RwLock<Arc<HashMap<String, f64>>>>,
    active_dataset: Arc<RwLock<String>>,
    available_datasets: Arc<RwLock<Vec<String>>>,
    imported: Arc<RwLock<HashMap<String, ImportedDataset>>>,
    gas_composition: Arc<RwLock<solver::GasComposition>>,
    data_dir: Arc<PathBuf>,
    simulation_slots: Arc<Semaphore>,
    simulation_capacity: usize,
    rayon_pool: Arc<ThreadPool>,
    exports: Arc<RwLock<HashMap<String, export::ExportRecord>>>,
    scenario_repo: ScenarioRepo,
    scenario_baselines: scenarios::ScenarioBaselines,
    compressor_map_mode_override: Arc<RwLock<Option<solver::CompressorMapMode>>>,
    last_simulation: Arc<RwLock<Option<LastSimulationSnapshot>>>,
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

/// Construit le routeur avec un dépôt SQLite déjà ouvert (production : fichier disque ;
/// tests : en mémoire). Tous les builders ci-dessus délèuent ici avec un repo en mémoire.
pub fn create_router_with_repo_and_datasets(
    network: GasNetwork,
    default_demands: HashMap<String, f64>,
    active_dataset: String,
    available_datasets: Vec<String>,
    data_dir: PathBuf,
    max_concurrent_simulations: usize,
    rayon_threads: usize,
    scenario_repo: ScenarioRepo,
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
        active_dataset: Arc::new(RwLock::new(active_dataset.clone())),
        available_datasets: Arc::new(RwLock::new(available_datasets)),
        imported: Arc::new(RwLock::new(HashMap::new())),
        gas_composition: Arc::new(RwLock::new(solver::GasComposition::default())),
        data_dir: Arc::new(data_dir),
        simulation_slots: Arc::new(Semaphore::new(simulation_capacity)),
        simulation_capacity,
        rayon_pool: Arc::new(rayon_pool),
        exports: Arc::new(RwLock::new(HashMap::new())),
        scenario_repo,
        scenario_baselines: Arc::new(RwLock::new(HashMap::new())),
        compressor_map_mode_override: Arc::new(RwLock::new(None)),
        last_simulation: Arc::new(RwLock::new(None)),
    });

    let initial_network = shared
        .network
        .read()
        .expect("network lock should not be poisoned")
        .clone();
    init_dataset_baseline(&shared, &active_dataset, initial_network.as_ref());

    Router::new()
        .route("/api/health", get(health))
        .route("/api/networks", get(list_networks))
        .route("/api/network", get(get_network).post(select_network))
        .route(
            "/api/network/gas-composition",
            patch(update_gas_composition),
        )
        .route("/api/network/nodes", post(network_edit::post_node))
        .route(
            "/api/network/nodes/{id}",
            put(network_edit::put_node).delete(network_edit::delete_node),
        )
        .route("/api/network/pipes", post(network_edit::post_pipe))
        .route(
            "/api/network/pipes/{id}",
            put(network_edit::put_pipe).delete(network_edit::delete_pipe),
        )
        .route("/api/import", post(import::post_import_network))
        .route("/api/export/{simulation_id}", get(export::get_export))
        .route("/api/exports", get(export::get_exports_list))
        .route("/api/exports/{id}/download", get(export::download_export))
        .route(
            "/api/contingency/export",
            post(export::post_contingency_export),
        )
        .route("/api/ws/sim", get(ws::ws_simulation_handler))
        .route(
            "/api/simulate",
            get(run_simulation_default).post(run_simulation_custom),
        )
        .route("/api/calibrate", post(post_calibrate))
        .route("/api/simulate/timeseries", post(run_timeseries_simulation))
        .route("/api/simulate/transient", post(run_transient_simulation))
        .route("/api/contingency", post(run_contingency))
        .route("/api/scenarios", get(scenarios::list_scenarios).post(scenarios::create_scenario))
        .route(
            "/api/scenarios/{id}",
            get(scenarios::get_scenario).delete(scenarios::delete_scenario),
        )
        .route(
            "/api/scenarios/{id}/apply",
            post(scenarios::apply_scenario),
        )
        .route("/api/simulate/compare", post(scenarios::compare_scenarios))
        .route("/api/nova/scenarios", get(nova::list_nova_scenarios))
        .route("/api/nova/capacity", post(nova::post_nova_capacity))
        .route("/api/nova/compare", post(nova::post_compare_nominations))
        .route(
            "/api/batch/runs",
            get(batch::list_batch_runs).post(batch::post_batch_run),
        )
        .route(
            "/api/batch/runs/{id}",
            get(batch::get_batch_run).delete(batch::delete_batch_run),
        )
        .route("/api/compressor/map-mode", get(compressor::get_map_mode).put(compressor::put_map_mode))
        .route(
            "/api/compressor/operating-points",
            get(compressor::get_operating_points),
        )
        .route(
            "/api/nova/nominations/reduced",
            post(nova::post_reduced_nomination),
        )
        .route(
            "/api/nova/nominations",
            post(nova::post_import_nomination),
        )
        .route(
            "/api/nova/nominations/{id}",
            axum::routing::delete(nova::delete_import_nomination),
        )
        .layer(CorsLayer::permissive())
        .with_state(shared)
}

/// Convenience wrapper ouvrant un dépôt SQLite en mémoire (tests / builders sans DB
/// explicite). La production doit passer par `create_router_with_repo_and_datasets`.
pub fn create_router_with_runtime_limits_and_datasets(
    network: GasNetwork,
    default_demands: HashMap<String, f64>,
    active_dataset: String,
    available_datasets: Vec<String>,
    data_dir: PathBuf,
    max_concurrent_simulations: usize,
    rayon_threads: usize,
) -> Router {
    let repo = ScenarioRepo::open(None).expect("open in-memory scenario repo");
    create_router_with_repo_and_datasets(
        network,
        default_demands,
        active_dataset,
        available_datasets,
        data_dir,
        max_concurrent_simulations,
        rayon_threads,
        repo,
    )
}

pub fn max_concurrent_simulations_from_env() -> usize {
    std::env::var("GAZFLOW_MAX_CONCURRENT_SIMULATIONS")
        .ok()
        .or_else(|| std::env::var("GAZSIM_MAX_CONCURRENT_SIMULATIONS").ok())
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(2)
}

pub fn rayon_threads_from_env(max_concurrent_simulations: usize) -> usize {
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
    gas: GasPropertiesDto,
    nodes: Vec<NodeDto>,
    pipes: Vec<PipeDto>,
}

#[derive(Serialize)]
struct NetworksResponse {
    networks: Vec<NetworkInfoDto>,
    active: String,
}

#[derive(Serialize)]
struct NetworkInfoDto {
    id: String,
    tier: solver::NetworkTier,
    node_count: usize,
    recommended_demo: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct NodeDto {
    id: String,
    x: f64,
    y: f64,
    lon: Option<f64>,
    lat: Option<f64>,
    height_m: f64,
    pressure_fixed_bar: Option<f64>,
    flow_min_m3s: Option<f64>,
    flow_max_m3s: Option<f64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PipeDto {
    id: String,
    from: String,
    to: String,
    kind: ConnectionKind,
    length_km: f64,
    diameter_mm: f64,
    #[serde(skip_serializing_if = "EquipmentSpec::is_empty")]
    equipment: EquipmentSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CapacityBoundDto {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SimulationMode {
    Check,
    Optimize,
}

#[derive(Debug, Deserialize)]
struct SimulateRequest {
    demands: HashMap<String, f64>,
    #[serde(default)]
    capacity_bounds: Option<HashMap<String, CapacityBoundDto>>,
    #[serde(default)]
    mode: Option<SimulationMode>,
}

#[derive(Debug, Deserialize)]
struct TimeseriesRequest {
    profiles: HashMap<String, solver::DemandProfile>,
    weather: Vec<solver::WeatherStep>,
    #[serde(default = "default_timeseries_max_iter")]
    max_iter: usize,
    #[serde(default = "default_timeseries_tolerance")]
    tolerance: f64,
    #[serde(default = "default_true")]
    warm_start: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TransientApiMode {
    QuasiSteady,
    Pde,
}

impl Default for TransientApiMode {
    fn default() -> Self {
        Self::QuasiSteady
    }
}

impl From<TransientApiMode> for solver::TransientMode {
    fn from(mode: TransientApiMode) -> Self {
        match mode {
            TransientApiMode::QuasiSteady => Self::QuasiSteady,
            TransientApiMode::Pde => Self::Pde,
        }
    }
}

#[derive(Debug, Deserialize)]
struct TransientRequest {
    #[serde(default)]
    initial_demands: Option<HashMap<String, f64>>,
    #[serde(default)]
    events: Vec<solver::TransientEvent>,
    #[serde(default = "default_transient_duration_s")]
    duration_s: f64,
    #[serde(default = "default_transient_dt_s")]
    dt_s: f64,
    #[serde(default)]
    gas_composition: Option<solver::GasComposition>,
    #[serde(default)]
    mode: TransientApiMode,
    #[serde(default)]
    n_cells_per_pipe: Option<usize>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ContingencyScope {
    All,
    SourcesOnly,
    Custom,
}

#[derive(Debug, Clone, Deserialize)]
struct ContingencyRequest {
    #[serde(default)]
    demands: Option<HashMap<String, f64>>,
    /// Identifiant de nomination NoVa (ex. `nomination_mild_618`) : charge les demandes du
    /// scénario sans modifier la topologie active.
    #[serde(default)]
    scenario_id: Option<String>,
    scope: ContingencyScope,
    #[serde(default)]
    custom_cases: Option<Vec<solver::ContingencyCase>>,
}

fn default_timeseries_max_iter() -> usize {
    800
}

fn default_timeseries_tolerance() -> f64 {
    1e-3
}

fn default_transient_duration_s() -> f64 {
    3600.0
}

fn default_transient_dt_s() -> f64 {
    300.0
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
struct TimeseriesResponse {
    steps: Vec<solver::TimeseriesStepResult>,
    total_iterations: usize,
    failed_hours: Vec<u8>,
}

#[derive(Debug, Serialize)]
struct TransientResponse {
    steps: Vec<solver::TransientStepResult>,
    total_iterations: usize,
    limitation: String,
}

#[derive(Debug, Serialize)]
struct SimulationResponse {
    pressures: HashMap<String, f64>,
    flows: HashMap<String, f64>,
    iterations: usize,
    residual: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    capacity_violations: Vec<solver::CapacityViolation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    adjusted_demands: Option<HashMap<String, f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_bounds: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    objective_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outer_iterations: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    infeasibility_diagnostic: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    equipment_states: Vec<solver::EquipmentState>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    demand_scale_achieved: Option<f64>,
}

impl From<solver::SolverResult> for SimulationResponse {
    fn from(r: solver::SolverResult) -> Self {
        Self {
            pressures: r.pressures,
            flows: r.flows,
            iterations: r.iterations,
            residual: r.residual,
            capacity_violations: Vec::new(),
            adjusted_demands: None,
            active_bounds: None,
            objective_value: None,
            outer_iterations: None,
            infeasibility_diagnostic: None,
            equipment_states: r.equipment_states,
            warnings: r.warnings,
            demand_scale_achieved: r.demand_scale_achieved,
        }
    }
}

impl From<solver::ConstrainedSolverResult> for SimulationResponse {
    fn from(r: solver::ConstrainedSolverResult) -> Self {
        Self {
            pressures: r.pressures,
            flows: r.flows,
            iterations: r.iterations,
            residual: r.residual,
            capacity_violations: r.capacity_violations,
            adjusted_demands: Some(r.adjusted_demands),
            active_bounds: Some(r.active_bounds),
            objective_value: Some(r.objective_value),
            outer_iterations: Some(r.outer_iterations),
            infeasibility_diagnostic: r.infeasibility_diagnostic,
            equipment_states: Vec::new(),
            warnings: Vec::new(),
            demand_scale_achieved: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct SelectNetworkRequest {
    dataset_id: String,
}

#[derive(Debug, Deserialize)]
struct UpdateGasCompositionRequest {
    gas_composition: solver::GasComposition,
}

#[derive(Debug, Serialize)]
struct SelectNetworkResponse {
    active: String,
    node_count: usize,
    edge_count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct ApiError {
    error: String,
}

async fn list_networks(State(state): State<SharedState>) -> Json<NetworksResponse> {
    let available = state
        .available_datasets
        .read()
        .expect("available datasets lock should not be poisoned")
        .clone();
    let networks: Vec<NetworkInfoDto> = available
        .iter()
        .filter_map(|id| dataset_network_info(&state, id))
        .collect();
    Json(NetworksResponse {
        networks,
        active: active_dataset_id(&state),
    })
}

fn dataset_network_info(state: &SharedState, dataset_id: &str) -> Option<NetworkInfoDto> {
    let node_count = dataset_node_count(state, dataset_id)?;
    Some(NetworkInfoDto {
        id: dataset_id.to_string(),
        tier: solver::tier_for_dataset(dataset_id, node_count),
        node_count,
        recommended_demo: solver::recommended_demo_for_dataset(dataset_id),
    })
}

fn dataset_node_count(state: &SharedState, dataset_id: &str) -> Option<usize> {
    let net_path = state.data_dir.join(format!("{dataset_id}.net"));
    if net_path.exists() {
        return gaslib::load_network(&net_path)
            .ok()
            .map(|network| network.node_count());
    }
    state
        .imported
        .read()
        .expect("imported lock should not be poisoned")
        .get(dataset_id)
        .map(|dataset| dataset.network.node_count())
}

fn solve_rest_steady(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    gas_composition: solver::GasComposition,
) -> anyhow::Result<solver::SolverResult> {
    let preset = solver::preset_for_node_count(network.node_count());
    solver::solve_steady_state_with_preset(
        network,
        demands,
        None,
        &preset,
        gas_composition,
        |_| solver::SolverControl::Continue,
        None::<fn(solver::ContinuationStepEvent)>,
    )
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
            flow_min_m3s: n.flow_min_m3s,
            flow_max_m3s: n.flow_max_m3s,
        })
        .collect();

    let pipes: Vec<PipeDto> = network
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

    Json(NetworkResponse {
        active_dataset,
        node_count: network.node_count(),
        edge_count: network.edge_count(),
        gas: GasPropertiesDto::from_composition(active_gas_composition(&state)),
        nodes,
        pipes,
    })
}

async fn select_network(
    State(state): State<SharedState>,
    Json(payload): Json<SelectNetworkRequest>,
) -> Result<Json<SelectNetworkResponse>, (StatusCode, Json<ApiError>)> {
    let known = state
        .available_datasets
        .read()
        .expect("available datasets lock should not be poisoned")
        .iter()
        .any(|id| id == &payload.dataset_id);
    if !known {
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

    if payload.dataset_id.starts_with("import-") {
        let imported = state
            .imported
            .read()
            .expect("imported lock should not be poisoned");
        let dataset = imported.get(&payload.dataset_id).ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: format!("imported dataset not found: {}", payload.dataset_id),
                }),
            )
        })?;
        let node_count = dataset.network.node_count();
        let edge_count = dataset.network.edge_count();
        activate_imported_dataset(&state, &payload.dataset_id, dataset);
        return Ok(Json(SelectNetworkResponse {
            active: payload.dataset_id,
            node_count,
            edge_count,
        }));
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
    set_active_gas_composition(
        &state,
        if payload.dataset_id.starts_with("GasLib") {
            solver::GasComposition::pure_ch4()
        } else {
            solver::GasComposition::default()
        },
    );
    init_dataset_baseline(
        &state,
        &payload.dataset_id,
        state
            .network
            .read()
            .expect("network lock should not be poisoned")
            .as_ref(),
    );

    Ok(Json(SelectNetworkResponse {
        active: payload.dataset_id,
        node_count,
        edge_count,
    }))
}

pub(crate) fn activate_imported_dataset(
    state: &SharedState,
    network_id: &str,
    dataset: &ImportedDataset,
) {
    {
        let mut guard = state
            .network
            .write()
            .expect("network lock should not be poisoned");
        *guard = Arc::new(clone_network(&dataset.network));
    }
    {
        let mut guard = state
            .default_demands
            .write()
            .expect("default demands lock should not be poisoned");
        *guard = Arc::new(dataset.default_demands.clone());
    }
    set_active_gas_composition(state, dataset.gas_composition);
    {
        let mut guard = state
            .active_dataset
            .write()
            .expect("active dataset lock should not be poisoned");
        *guard = network_id.to_string();
    }
    init_dataset_baseline(
        state,
        network_id,
        state
            .network
            .read()
            .expect("network lock should not be poisoned")
            .as_ref(),
    );
}

pub(crate) fn active_gas_composition(state: &SharedState) -> solver::GasComposition {
    *state
        .gas_composition
        .read()
        .expect("gas composition lock should not be poisoned")
}

pub(crate) fn set_active_gas_composition(state: &SharedState, composition: solver::GasComposition) {
    *state
        .gas_composition
        .write()
        .expect("gas composition lock should not be poisoned") = composition.normalize();
}

async fn update_gas_composition(
    State(state): State<SharedState>,
    Json(payload): Json<UpdateGasCompositionRequest>,
) -> Result<Json<GasPropertiesDto>, (StatusCode, Json<ApiError>)> {
    if state.simulation_slots.available_permits() != state.simulation_capacity {
        return Err((
            StatusCode::CONFLICT,
            Json(ApiError {
                error: "cannot update gas composition while simulations are running".to_string(),
            }),
        ));
    }

    let composition = payload.gas_composition.normalize();
    set_active_gas_composition(&state, composition);

    let active_id = active_dataset_id(&state);
    if active_id.starts_with("import-") {
        let mut imported = state
            .imported
            .write()
            .expect("imported lock should not be poisoned");
        if let Some(dataset) = imported.get_mut(&active_id) {
            dataset.gas_composition = composition;
        }
    }

    Ok(Json(GasPropertiesDto::from_composition(composition)))
}

fn clone_network(network: &GasNetwork) -> GasNetwork {
    let mut cloned = GasNetwork::new();
    for node in network.nodes() {
        cloned.add_node(node.clone());
    }
    for pipe in network.pipes() {
        cloned.add_pipe(pipe.clone());
    }
    cloned
}

fn init_dataset_baseline(state: &SharedState, dataset_id: &str, network: &GasNetwork) {
    scenarios::ensure_baseline(&state.scenario_baselines, dataset_id, network);
}

fn api_bounds_to_solver(
    api_bounds: &HashMap<String, CapacityBoundDto>,
    network: &GasNetwork,
) -> solver::CapacityBounds {
    let node_bounds = api_bounds
        .iter()
        .map(|(id, b)| (id.clone(), (b.min, b.max)))
        .collect();
    let pipe_bounds = network
        .pipes()
        .filter_map(|p| match (p.flow_min_m3s, p.flow_max_m3s) {
            (Some(min), Some(max)) => Some((p.id.clone(), (min, max))),
            _ => None,
        })
        .collect();
    solver::CapacityBounds {
        node_bounds,
        pipe_bounds,
    }
}

async fn run_simulation_default(State(state): State<SharedState>) -> ApiResult<SimulationResponse> {
    let demands = (*active_default_demands(&state)).clone();
    run_simulation_with_demands(&state, demands, None, None).await
}

async fn run_simulation_custom(
    State(state): State<SharedState>,
    Json(payload): Json<SimulateRequest>,
) -> ApiResult<SimulationResponse> {
    run_simulation_with_demands(
        &state,
        payload.demands,
        payload.capacity_bounds,
        payload.mode,
    )
    .await
}

async fn post_calibrate(
    State(state): State<SharedState>,
    Json(payload): Json<calibration::CalibrationRequest>,
) -> ApiResult<calibration::CalibrationReport> {
    let demands = payload
        .demands
        .unwrap_or_else(|| (*active_default_demands(&state)).clone());
    let network = active_network(&state);
    let network_for_calibration = network.clone();
    let strategy = payload.strategy;
    let measurements_csv = payload.measurements_csv;

    let permit = state
        .simulation_slots
        .clone()
        .acquire_owned()
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: format!("calibration capacity semaphore closed: {err}"),
                }),
            )
        })?;
    let pool = state.rayon_pool.clone();

    let report = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        pool.install(|| {
            calibration::calibrate_from_csv(
                &network_for_calibration,
                &demands,
                &measurements_csv,
                strategy,
            )
        })
    })
    .await
    .map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: format!("calibration task join error: {err}"),
            }),
        )
    })?
    .map_err(|err| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError { error: err }),
        )
    })?;

    Ok(Json(report))
}

async fn run_timeseries_simulation(
    State(state): State<SharedState>,
    Json(payload): Json<TimeseriesRequest>,
) -> ApiResult<TimeseriesResponse> {
    if payload.profiles.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "profiles must not be empty".to_string(),
            }),
        ));
    }
    if payload.weather.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "weather must not be empty".to_string(),
            }),
        ));
    }

    let network = active_network(&state);
    let network_for_solve = network.clone();
    let profiles = payload.profiles;
    let weather = payload.weather;
    let config = solver::TimeseriesConfig {
        gas_composition: active_gas_composition(&state),
        max_iter: payload.max_iter,
        tolerance: payload.tolerance,
        warm_start: payload.warm_start,
        warm_start_max_demand_rel_change: 3.0,
        robust_solver: network.node_count() > 199,
    };

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
        pool.install(|| {
            solver::simulate_timeseries(&network_for_solve, &profiles, &weather, &config)
        })
    })
    .await
    .map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: format!("timeseries task join error: {err}"),
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

    Ok(Json(TimeseriesResponse {
        steps: result.steps,
        total_iterations: result.total_iterations,
        failed_hours: result.failed_hours,
    }))
}

async fn run_transient_simulation(
    State(state): State<SharedState>,
    Json(payload): Json<TransientRequest>,
) -> ApiResult<TransientResponse> {
    let demands = payload
        .initial_demands
        .unwrap_or_else(|| (*active_default_demands(&state)).clone());
    let network = active_network(&state);
    let network_for_solve = network.clone();
    let events = payload.events;
    let config = solver::TransientConfig {
        duration_s: payload.duration_s,
        dt_s: payload.dt_s,
        gas_composition: payload
            .gas_composition
            .unwrap_or_else(|| active_gas_composition(&state)),
        n_cells_per_pipe: payload.n_cells_per_pipe,
    };
    let mode = solver::TransientMode::from(payload.mode);

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
        pool.install(|| {
            solver::simulate_transient_with_mode(
                &network_for_solve,
                &demands,
                &events,
                &config,
                mode,
            )
        })
    })
    .await
    .map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: format!("transient task join error: {err}"),
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

    Ok(Json(TransientResponse {
        steps: result.steps,
        total_iterations: result.total_iterations,
        limitation: result.limitation,
    }))
}

async fn run_contingency(
    State(state): State<SharedState>,
    Json(payload): Json<ContingencyRequest>,
) -> ApiResult<solver::ContingencyReport> {
    let report = compute_contingency_report(&state, payload).await?;
    Ok(Json(report))
}

fn resolve_contingency_cases(
    network: &GasNetwork,
    scope: ContingencyScope,
    custom_cases: Option<Vec<solver::ContingencyCase>>,
) -> Result<Vec<solver::ContingencyCase>, (StatusCode, Json<ApiError>)> {
    let cases = match scope {
        ContingencyScope::All => solver::generate_n_minus_1_cases(network),
        ContingencyScope::SourcesOnly => solver::generate_n_minus_1_cases(network)
            .into_iter()
            .filter(|case| case.element_type == solver::ContingencyElementType::Source)
            .collect(),
        ContingencyScope::Custom => {
            let Some(custom) = custom_cases else {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        error: "custom_cases is required when scope=custom".to_string(),
                    }),
                ));
            };
            custom
        }
    };
    Ok(cases)
}

pub(crate) fn resolve_contingency_demands(
    state: &SharedState,
    network: &GasNetwork,
    scenario_id: Option<&str>,
    body_demands: Option<&HashMap<String, f64>>,
) -> Result<HashMap<String, f64>, (StatusCode, Json<ApiError>)> {
    if let Some(scenario_id) = scenario_id {
        let dataset_id = active_dataset_id(state);
        let mut scenario = load_scenario_demands_by_id(state, &dataset_id, scenario_id).map_err(
            |err| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError { error: err }),
                )
            },
        )?;
        gaslib::enrich_scenario_with_balance_hub(network, &mut scenario);
        return Ok(gaslib::effective_solver_demands_for_network(
            network,
            &scenario.demands,
            &scenario,
        ));
    }
    if let Some(demands) = body_demands {
        return Ok(demands.clone());
    }
    Ok((*active_default_demands(state)).clone())
}

async fn compute_contingency_report(
    state: &SharedState,
    payload: ContingencyRequest,
) -> Result<solver::ContingencyReport, (StatusCode, Json<ApiError>)> {
    let network = active_network(state);
    let demands = resolve_contingency_demands(
        state,
        &network,
        payload.scenario_id.as_deref(),
        payload.demands.as_ref(),
    )?;
    let network_for_solve = network.clone();
    let cases = resolve_contingency_cases(&network, payload.scope, payload.custom_cases)?;

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
    let gas_composition = active_gas_composition(state);

    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        pool.install(|| {
            solver::run_contingency_analysis(
                &network_for_solve,
                &demands,
                &cases,
                solver::SteadyStateConfig {
                    gas_composition,
                    max_iter: 1000,
                    tolerance: 5e-4,
                    ..solver::SteadyStateConfig::default()
                },
            )
        })
    })
    .await
    .map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: format!("contingency task join error: {err}"),
            }),
        )
    })
}

async fn run_simulation_with_demands(
    state: &SharedState,
    demands: HashMap<String, f64>,
    capacity_bounds: Option<HashMap<String, CapacityBoundDto>>,
    mode: Option<SimulationMode>,
) -> ApiResult<SimulationResponse> {
    let demands_for_export = demands.clone();
    let demands_for_snapshot = demands.clone();
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
    let gas_composition = active_gas_composition(state);
    let state_for_store = state.clone();
    let started = std::time::Instant::now();

    let mut export_stored = false;
    let response: SimulationResponse = match capacity_bounds {
        Some(ref api_bounds) if matches!(mode, Some(SimulationMode::Optimize)) => {
            let bounds = api_bounds_to_solver(api_bounds, &network_for_solve);
            let demands_clone = demands.clone();
            let state_for_mode = state_for_store.clone();
            let constrained_result = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                pool.install(|| {
                    sync_compressor_map_mode_for_solve(&state_for_mode);
                    solver::capacity::solve_steady_state_constrained(
                        &network_for_solve,
                        &demands_clone,
                        &bounds,
                        None,
                        solver::capacity::ConstrainedSolverConfig {
                            inner_gas_composition: gas_composition,
                            ..Default::default()
                        },
                        |_| solver::SolverControl::Continue,
                    )
                })
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
            let resp: SimulationResponse = constrained_result.clone().into();
            export::store_export_record(
                state,
                export::new_constrained_export_record(
                    export_id,
                    network_id.clone(),
                    &network,
                    demands_for_export.clone(),
                    constrained_result,
                    started.elapsed().as_millis() as u64,
                ),
            );
            export_stored = true;
            resp
        }
        Some(ref api_bounds) => {
            let bounds = api_bounds_to_solver(api_bounds, &network_for_solve);
            let demands_for_check = demands.clone();
            let state_for_mode = state_for_store.clone();
            let result = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                pool.install(|| {
                    sync_compressor_map_mode_for_solve(&state_for_mode);
                    solve_rest_steady(&network_for_solve, &demands_for_check, gas_composition)
                })
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
            let violations =
                solver::capacity::check_capacity_violations(&network, &result, &demands, &bounds);
            let mut resp: SimulationResponse = result.into();
            resp.capacity_violations = violations;
            resp
        }
        None => {
            let state_for_mode = state_for_store.clone();
            let result = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                pool.install(|| {
                    sync_compressor_map_mode_for_solve(&state_for_mode);
                    solve_rest_steady(&network_for_solve, &demands, gas_composition)
                })
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
            result.into()
        }
    };

    if !export_stored {
        let export_result = solver::SolverResult::from_core(
            response.pressures.clone(),
            response.flows.clone(),
            response.iterations,
            response.residual,
        );
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
                export_result,
                0,
            ),
        );
    }

    let snapshot_result = solver::SolverResult::from_core(
        response.pressures.clone(),
        response.flows.clone(),
        response.iterations,
        response.residual,
    );
    store_last_simulation(state, demands_for_snapshot, snapshot_result);

    Ok(Json(response))
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

pub(crate) fn sync_compressor_map_mode_for_solve(state: &SharedState) {
    let override_mode = state
        .compressor_map_mode_override
        .read()
        .expect("compressor map mode lock should not be poisoned")
        .clone();
    solver::set_thread_compressor_map_mode_override(override_mode);
}

pub(crate) fn store_last_simulation(
    state: &SharedState,
    demands: HashMap<String, f64>,
    result: solver::SolverResult,
) {
    let mut guard = state
        .last_simulation
        .write()
        .expect("last simulation lock should not be poisoned");
    *guard = Some(LastSimulationSnapshot { demands, result });
}

/// Résolution unifiée d'un scénario `.scn` par id : d'abord les nominations importées
/// (SQLite), puis le filesystem bundlé (`resolve_scenario_path`). Retourne le XML brut.
pub(crate) fn resolve_scenario_xml(
    state: &SharedState,
    dataset_id: &str,
    scenario_id: &str,
) -> Option<String> {
    if let Ok(Some(rec)) = state.scenario_repo.find_imported_nomination(scenario_id) {
        return Some(rec.xml);
    }
    gaslib::resolve_scenario_path(&state.data_dir, dataset_id, scenario_id)
        .and_then(|path| std::fs::read_to_string(&path).ok())
}

/// Charge et parse un scénario `.scn` par id (résolution unifiée importée + filesystem).
pub(crate) fn load_scenario_demands_by_id(
    state: &SharedState,
    dataset_id: &str,
    scenario_id: &str,
) -> Result<gaslib::ScenarioDemands, String> {
    let xml = resolve_scenario_xml(state, dataset_id, scenario_id)
        .ok_or_else(|| format!("scénario {scenario_id} introuvable pour le dataset {dataset_id}"))?;
    gaslib::parse_scenario_demands_from_str(&xml).map_err(|err| format!("{err:#}"))
}

fn load_dataset_from_disk(
    data_dir: &Path,
    dataset_id: &str,
) -> Result<(GasNetwork, HashMap<String, f64>), String> {
    let network_path = data_dir.join(format!("{dataset_id}.net"));
    let mut network = gaslib::load_network(&network_path)
        .map_err(|err| format!("failed to load network {:?}: {err:#}", network_path))?;

    let scenario_path = data_dir.join(format!("{dataset_id}.scn"));
    let default_demands = if scenario_path.exists() {
        match gaslib::load_scenario_demands(&scenario_path) {
            Ok(mut scenario) => {
                network = gaslib::prepare_transport_scenario(&network, &mut scenario);
                gaslib::effective_solver_demands_for_network(
                    &network,
                    &scenario.demands,
                    &scenario,
                )
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

    use crate::graph::{ConnectionKind, EquipmentSpec, Node, Pipe};

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
    async fn test_api_list_networks_returns_tier() {
        let app = create_router_with_runtime_limits_and_datasets(
            GasNetwork::new(),
            HashMap::new(),
            "GasLib-11".to_string(),
            vec!["GasLib-11".to_string()],
            PathBuf::from("dat"),
            4,
            2,
        );
        let req = Request::builder()
            .method("GET")
            .uri("/api/networks")
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("read body");
        let json: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            json.get("active").and_then(Value::as_str),
            Some("GasLib-11")
        );
        let networks = json
            .get("networks")
            .and_then(Value::as_array)
            .expect("networks array");
        assert_eq!(networks.len(), 1);
        let entry = &networks[0];
        assert_eq!(
            entry.get("id").and_then(Value::as_str),
            Some("GasLib-11")
        );
        assert_eq!(entry.get("tier").and_then(Value::as_str), Some("demo"));
        assert_eq!(entry.get("node_count").and_then(Value::as_u64), Some(11));
        assert_eq!(
            entry.get("recommended_demo").and_then(Value::as_bool),
            Some(true)
        );
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

    #[tokio::test]
    async fn test_api_timeseries_returns_24_steps() {
        let app = test_router();
        let weather: Vec<_> = (0u8..24)
            .map(|hour| serde_json::json!({ "hour": hour, "t_ext_c": -3.0 }))
            .collect();
        let payload = serde_json::json!({
            "profiles": {
                "sink": {
                    "q0_m3h": 45.0,
                    "alpha_m3h_per_c": 7.5,
                    "t_threshold_c": 17.0,
                    "category": "residential"
                }
            },
            "weather": weather,
            "warm_start": true
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/simulate/timeseries")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("read body");
        let json: Value = serde_json::from_slice(&body).expect("json body");
        let steps = json.get("steps").and_then(Value::as_array).expect("steps");
        assert_eq!(steps.len(), 24);
        assert!(
            json.get("failed_hours")
                .and_then(Value::as_array)
                .is_some_and(|a| a.is_empty())
        );
    }

    #[tokio::test]
    async fn test_api_transient_returns_steps_with_linepack() {
        let app = test_router();
        let payload = serde_json::json!({
            "duration_s": 1800.0,
            "dt_s": 600.0,
            "initial_demands": {
                "sink": -6.0
            },
            "events": [
                {
                    "type": "demand_change",
                    "time_s": 600.0,
                    "node_id": "sink",
                    "demand_m3s": -8.0
                }
            ]
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/simulate/transient")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("read body");
        let json: Value = serde_json::from_slice(&body).expect("json body");
        let steps = json.get("steps").and_then(Value::as_array).expect("steps");
        assert_eq!(steps.len(), 4);
        assert!(steps.iter().all(|step| {
            step.get("linepack_kg")
                .and_then(Value::as_f64)
                .is_some_and(|v| v > 0.0)
        }));
    }

    #[tokio::test]
    async fn test_api_contingency_scope_all_returns_report() {
        let app = test_router();
        let payload = serde_json::json!({
            "scope": "all"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/contingency")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("read body");
        let json: Value = serde_json::from_slice(&body).expect("json body");
        assert!(json.get("results").is_some(), "results field missing");
        assert!(json.get("red_cases").is_some(), "red_cases field missing");
        assert!(
            json.get("green_cases").is_some(),
            "green_cases field missing"
        );
    }

    fn contingency_scratch_dir(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gazflow-contingency-test-{suffix}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn contingency_test_network() -> GasNetwork {
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
        net
    }

    fn contingency_test_state(
        network: GasNetwork,
        defaults: HashMap<String, f64>,
        data_dir: PathBuf,
    ) -> SharedState {
        Arc::new(AppState {
            network: Arc::new(RwLock::new(Arc::new(network))),
            default_demands: Arc::new(RwLock::new(Arc::new(defaults))),
            active_dataset: Arc::new(RwLock::new("test".to_string())),
            available_datasets: Arc::new(RwLock::new(vec!["test".to_string()])),
            imported: Arc::new(RwLock::new(HashMap::new())),
            gas_composition: Arc::new(RwLock::new(solver::GasComposition::default())),
            data_dir: Arc::new(data_dir),
            simulation_slots: Arc::new(Semaphore::new(2)),
            simulation_capacity: 2,
            rayon_pool: Arc::new(
                ThreadPoolBuilder::new()
                    .num_threads(1)
                    .build()
                    .expect("pool"),
            ),
            exports: Arc::new(RwLock::new(HashMap::new())),
            scenario_repo: ScenarioRepo::open(None).expect("repo"),
            scenario_baselines: Arc::new(RwLock::new(HashMap::new())),
            compressor_map_mode_override: Arc::new(RwLock::new(None)),
            last_simulation: Arc::new(RwLock::new(None)),
        })
    }

    #[test]
    fn resolve_contingency_demands_without_scenario_id_uses_body_or_defaults() {
        let tmp = contingency_scratch_dir("defaults");
        let mut defaults = HashMap::new();
        defaults.insert("sink".to_string(), -5.0);
        let network = contingency_test_network();
        let state = contingency_test_state(network.clone(), defaults, tmp.clone());

        let resolved = resolve_contingency_demands(&state, &network, None, None)
            .expect("defaults");
        assert_eq!(resolved.get("sink").copied(), Some(-5.0));

        let mut body = HashMap::new();
        body.insert("sink".to_string(), -12.0);
        let resolved = resolve_contingency_demands(&state, &network, None, Some(&body))
            .expect("body demands");
        assert_eq!(resolved.get("sink").copied(), Some(-12.0));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_contingency_demands_with_scenario_id_loads_scenario() {
        let tmp = contingency_scratch_dir("scenario");
        let scenario_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<boundaryValue>
  <scenario id="ct_nom">
    <node type="sink" id="sink">
      <flow value="-42.0"/>
    </node>
  </scenario>
</boundaryValue>"#;
        std::fs::write(tmp.join("ct_nom.scn"), scenario_xml).unwrap();

        let mut defaults = HashMap::new();
        defaults.insert("sink".to_string(), -5.0);
        let network = contingency_test_network();
        let state = contingency_test_state(network.clone(), defaults, tmp.clone());

        let resolved =
            resolve_contingency_demands(&state, &network, Some("ct_nom"), None).expect("scenario");
        assert_eq!(resolved.get("sink").copied(), Some(-42.0));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_api_contingency_scenario_id_uses_scenario_demands() {
        let tmp = contingency_scratch_dir("http");
        let scenario_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<boundaryValue>
  <scenario id="ct_nom">
    <node type="sink" id="sink">
      <flow value="-42.0"/>
    </node>
  </scenario>
</boundaryValue>"#;
        std::fs::write(tmp.join("ct_nom.scn"), scenario_xml).unwrap();

        let mut defaults = HashMap::new();
        defaults.insert("sink".to_string(), -5.0);
        let app = create_router_with_runtime_limits_and_datasets(
            contingency_test_network(),
            defaults,
            "test".to_string(),
            vec!["test".to_string()],
            tmp.clone(),
            4,
            2,
        );

        let payload = serde_json::json!({
            "scope": "all",
            "scenario_id": "ct_nom"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/contingency")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
