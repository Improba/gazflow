//! Lanceur paramétrique de batchs : produit cartésien d'axes (échelles de demande,
//! variantes topologiques) sur une nomination NoVa de base, et persiste les résultats.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use super::{
    ApiError, SharedState, active_dataset_id, active_gas_composition, active_network,
    resolve_scenario_xml, scenarios, sync_compressor_map_mode_for_solve,
};

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

#[derive(Debug, Deserialize)]
pub(super) struct CreateBatchRequest {
    /// Nom optionnel du batch.
    #[serde(default)]
    pub name: Option<String>,
    /// Nomination `.scn` de base (id bundlé ou importé).
    pub base_scenario_id: String,
    /// Axes du sweep. Chaque axe est une liste de valeurs ; le produit cartésien
    /// définit les cas. `demand_scales` = multiplicateurs des demandes effectives ;
    /// `topology_scenario_ids` = variantes topologiques à appliquer (`null` = baseline).
    #[serde(default)]
    pub demand_scales: Option<Vec<f64>>,
    #[serde(default)]
    pub topology_scenario_ids: Option<Vec<Option<String>>>,
    #[serde(default)]
    pub max_iter: Option<usize>,
    #[serde(default)]
    pub tolerance: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct BatchCaseOutcome {
    pub label: String,
    pub demand_scale: f64,
    pub topology_scenario_id: Option<String>,
    pub feasible: bool,
    pub cause: String,
    pub deficit_sinks: Vec<String>,
    pub max_shortfall_bar: f64,
    pub iterations: usize,
    pub residual: f64,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct BatchRunSummary {
    pub id: String,
    pub name: String,
    pub created_at_ms: u64,
    pub status: String,
    pub case_count: usize,
    pub feasible_count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct BatchRunDetail {
    pub id: String,
    pub dataset_id: String,
    pub name: String,
    pub created_at_ms: u64,
    pub status: String,
    pub base_scenario_id: String,
    pub cases: Vec<BatchCaseOutcome>,
}

fn cause_str(c: crate::solver::NovaCause) -> &'static str {
    match c {
        crate::solver::NovaCause::Feasible => "Feasible",
        crate::solver::NovaCause::PressureDeficit => "PressureDeficit",
        crate::solver::NovaCause::PressureExcess => "PressureExcess",
        crate::solver::NovaCause::PressureReachability => "PressureReachability",
        crate::solver::NovaCause::NotSolvedLocal => "NotSolvedLocal",
        crate::solver::NovaCause::ScaleNotAchieved => "ScaleNotAchieved",
    }
}

fn run_one_case(
    network: &crate::graph::GasNetwork,
    scenario: &crate::gaslib::ScenarioDemands,
    gas: crate::solver::GasComposition,
    demand_scale: f64,
    topology_scenario_id: Option<&str>,
    state: &SharedState,
    max_iter: usize,
    tolerance: f64,
) -> BatchCaseOutcome {
    let label = format!(
        "x{demand_scale:.2} | topo={}",
        topology_scenario_id.unwrap_or("baseline")
    );

    // Réseau potentiellement modifié par variante topologique.
    let resolved = scenarios::resolve_scenario_network(state, topology_scenario_id);
    let net = match resolved {
        Ok(n) => n,
        Err(_) => {
            return BatchCaseOutcome {
                label,
                demand_scale,
                topology_scenario_id: topology_scenario_id.map(|s| s.to_string()),
                feasible: false,
                cause: "Error".to_string(),
                deficit_sinks: vec![],
                max_shortfall_bar: 0.0,
                iterations: 0,
                residual: 0.0,
                error: Some(format!(
                    "variante topologique introuvable: {}",
                    topology_scenario_id.unwrap_or("baseline")
                )),
            };
        }
    };

    let mut demands =
        crate::gaslib::effective_solver_demands_for_network(&net, &scenario.demands, scenario);
    if (demand_scale - 1.0).abs() > f64::EPSILON {
        for v in demands.values_mut() {
            *v *= demand_scale;
        }
    }

    sync_compressor_map_mode_for_solve(state);

    let preset = crate::solver::preset_from_request(
        net.node_count(),
        true,
        max_iter,
        tolerance,
        120_000,
        1,
        None,
    );
    let solve = crate::solver::solve_steady_state_with_preset(
        &net,
        &demands,
        None,
        &preset,
        gas,
        |_| crate::solver::SolverControl::Continue,
        None::<fn(crate::solver::ContinuationStepEvent)>,
    );

    match solve {
        Ok(result) => {
            let diag = crate::solver::compute_nova_diagnostics(&net, scenario, &result);
            let converged = result.residual <= preset.tolerance;
            let verdict = super::nova_finalize::finalize_nova_verdict(
                &net,
                Some(scenario),
                &demands,
                gas,
                &diag,
                converged,
                preset.tolerance,
                &result,
            );
            let max_shortfall = diag
                .pressure_slips
                .iter()
                .map(|s| s.shortfall_bar.max(0.0))
                .fold(0.0_f64, f64::max);
            BatchCaseOutcome {
                label,
                demand_scale,
                topology_scenario_id: topology_scenario_id.map(|s| s.to_string()),
                feasible: verdict.feasible,
                cause: cause_str(verdict.cause).to_string(),
                deficit_sinks: verdict.deficit_sinks,
                max_shortfall_bar: max_shortfall,
                iterations: result.iterations,
                residual: result.residual,
                error: None,
            }
        }
        Err(err) => BatchCaseOutcome {
            label,
            demand_scale,
            topology_scenario_id: topology_scenario_id.map(|s| s.to_string()),
            feasible: false,
            cause: "Error".to_string(),
            deficit_sinks: vec![],
            max_shortfall_bar: 0.0,
            iterations: 0,
            residual: 0.0,
            error: Some(format!("{err:#}")),
        },
    }
}

pub(super) async fn post_batch_run(
    State(state): State<SharedState>,
    Json(payload): Json<CreateBatchRequest>,
) -> Result<Json<BatchRunDetail>, (StatusCode, Json<ApiError>)> {
    let dataset_id = active_dataset_id(&state);
    let gas = active_gas_composition(&state);
    let max_iter = payload.max_iter.unwrap_or(400);
    let tolerance = payload.tolerance.unwrap_or(1e-3);

    // Construit la liste des cas (produit cartésien).
    let scales: Vec<f64> = payload.demand_scales.unwrap_or_else(|| vec![1.0]);
    let topo: Vec<Option<String>> = payload
        .topology_scenario_ids
        .unwrap_or_else(|| vec![None]);
    if scales.is_empty() || topo.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "axes must not be empty",
        ));
    }

    // Charge la nomination de base.
    let xml = resolve_scenario_xml(&state, &dataset_id, &payload.base_scenario_id).ok_or_else(|| {
        api_error(
            StatusCode::NOT_FOUND,
            format!(
                "scénario {} introuvable pour le dataset {}",
                payload.base_scenario_id, dataset_id
            ),
        )
    })?;

    let permit = state
        .simulation_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "simulation capacity reached, retry later",
            )
        })?;

    let network = active_network(&state);
    let base_scenario_id = payload.base_scenario_id.clone();
    let batch_id = format!("batch-{}", now_ms());
    let batch_name = payload
        .name
        .clone()
        .unwrap_or_else(|| format!("Batch {batch_id}"));
    let scales_for_task = scales.clone();
    let topo_for_task = topo.clone();
    let state_for_task = state.clone();
    let pool = state.rayon_pool.clone();

    let cases: Vec<BatchCaseOutcome> = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        pool.install(|| {
            let mut scenario = match crate::gaslib::parse_scenario_demands_from_str(&xml) {
                Ok(s) => s,
                Err(err) => {
                    return vec![BatchCaseOutcome {
                        label: "parse error".to_string(),
                        demand_scale: 1.0,
                        topology_scenario_id: None,
                        feasible: false,
                        cause: "Error".to_string(),
                        deficit_sinks: vec![],
                        max_shortfall_bar: 0.0,
                        iterations: 0,
                        residual: 0.0,
                        error: Some(format!("{err:#}")),
                    }];
                }
            };
            crate::gaslib::enrich_scenario_with_balance_hub(&network, &mut scenario);

            let mut out = Vec::new();
            for topo_id in &topo_for_task {
                for &scale in &scales_for_task {
                    out.push(run_one_case(
                        &network,
                        &scenario,
                        gas,
                        scale,
                        topo_id.as_deref(),
                        &state_for_task,
                        max_iter,
                        tolerance,
                    ));
                }
            }
            out
        })
    })
    .await
    .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("join: {err}")))?;

    let feasible_count = cases.iter().filter(|c| c.feasible).count();
    let results_json = serde_json::to_string(&cases)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let config_json = serde_json::to_string(&CreateBatchConfigSnapshot {
        base_scenario_id: base_scenario_id.clone(),
        demand_scales: scales,
        topology_scenario_ids: topo,
    })
    .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    let record = crate::store::BatchRunRecord {
        id: batch_id.clone(),
        dataset_id: dataset_id.clone(),
        name: batch_name.clone(),
        created_at_ms: now_ms(),
        config_json,
        status: "done".to_string(),
        results_json,
    };
    state
        .scenario_repo
        .insert_batch_run(&record)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    Ok(Json(BatchRunDetail {
        id: batch_id,
        dataset_id,
        name: batch_name,
        created_at_ms: record.created_at_ms,
        status: record.status,
        base_scenario_id,
        cases,
    }))
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateBatchConfigSnapshot {
    base_scenario_id: String,
    demand_scales: Vec<f64>,
    topology_scenario_ids: Vec<Option<String>>,
}

pub(super) async fn list_batch_runs(
    State(state): State<SharedState>,
) -> Result<Json<Vec<BatchRunSummary>>, (StatusCode, Json<ApiError>)> {
    let dataset_id = active_dataset_id(&state);
    let records = state
        .scenario_repo
        .list_batch_runs(&dataset_id)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let summaries = records
        .into_iter()
        .filter_map(|rec| {
            let cases: Vec<BatchCaseOutcome> = serde_json::from_str(&rec.results_json).ok()?;
            let feasible_count = cases.iter().filter(|c| c.feasible).count();
            Some(BatchRunSummary {
                id: rec.id,
                name: rec.name,
                created_at_ms: rec.created_at_ms,
                status: rec.status,
                case_count: cases.len(),
                feasible_count,
            })
        })
        .collect();
    Ok(Json(summaries))
}

pub(super) async fn get_batch_run(
    State(state): State<SharedState>,
    Path(batch_id): Path<String>,
) -> Result<Json<BatchRunDetail>, (StatusCode, Json<ApiError>)> {
    let rec = state
        .scenario_repo
        .get_batch_run(&batch_id)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("batch not found: {batch_id}")))?;
    let config: CreateBatchConfigSnapshot = serde_json::from_str(&rec.config_json)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let cases: Vec<BatchCaseOutcome> = serde_json::from_str(&rec.results_json)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(BatchRunDetail {
        id: rec.id,
        dataset_id: rec.dataset_id,
        name: rec.name,
        created_at_ms: rec.created_at_ms,
        status: rec.status,
        base_scenario_id: config.base_scenario_id,
        cases,
    }))
}

pub(super) async fn delete_batch_run(
    State(state): State<SharedState>,
    Path(batch_id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    let removed = state
        .scenario_repo
        .delete_batch_run(&batch_id)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    if !removed {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("batch not found: {batch_id}"),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use std::collections::HashMap;
    use tower::ServiceExt;

    fn scratch_dir(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gazflow-batch-test-{suffix}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn scn_xml(flow: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<boundaryValue xmlns="http://gaslib.zib.de/Gas" xmlns:framework="http://gaslib.zib.de/Framework">
  <scenario id="batch">
    <node type="entry" id="entry01">
      <flow bound="lower" value="{flow}" unit="1000m_cube_per_hour"/>
      <flow bound="upper" value="{flow}" unit="1000m_cube_per_hour"/>
    </node>
    <node type="exit" id="exit01">
      <flow bound="lower" value="{flow}" unit="1000m_cube_per_hour"/>
      <flow bound="upper" value="{flow}" unit="1000m_cube_per_hour"/>
    </node>
  </scenario>
</boundaryValue>"#
        )
    }

    #[tokio::test]
    async fn batch_run_sweep_demand_scales() {
        let tmp = scratch_dir("sweep");
        use crate::graph::{ConnectionKind, EquipmentSpec, GasNetwork, Node, Pipe};
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "entry01".into(), x: 0.0, y: 0.0, lon: Some(10.0), lat: Some(50.0),
            height_m: 0.0, pressure_lower_bar: None, pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0), flow_min_m3s: None, flow_max_m3s: None,
        });
        net.add_node(Node {
            id: "exit01".into(), x: 1.0, y: 0.0, lon: Some(11.0), lat: Some(50.0),
            height_m: 0.0, pressure_lower_bar: None, pressure_upper_bar: None,
            pressure_fixed_bar: None, flow_min_m3s: None, flow_max_m3s: None,
        });
        net.add_pipe(Pipe {
            id: "p1".into(), from: "entry01".into(), to: "exit01".into(),
            kind: ConnectionKind::Pipe, is_open: true, length_km: 5.0, diameter_mm: 500.0,
            roughness_mm: 0.05, compressor_ratio_max: None, flow_min_m3s: None,
            flow_max_m3s: None, equipment: EquipmentSpec::default(),
        });
        let defaults: HashMap<String, f64> = HashMap::new();
        let app = crate::api::create_router_with_runtime_limits_and_datasets(
            net, defaults, "test".to_string(), vec!["test".to_string()],
            tmp.clone(), 2, 1,
        );

        // Importe une nomination.
        let body = serde_json::json!({ "filename": "batch.scn", "xml": scn_xml("100.00") });
        let req = Request::builder().method("POST").uri("/api/nova/nominations")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())).unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v: serde_json::Value =
            serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.unwrap()).unwrap();
        let nom_id = v["id"].as_str().unwrap().to_string();

        // Lance un sweep 3 échelles.
        let body = serde_json::json!({
            "name": "Sweep test",
            "base_scenario_id": nom_id,
            "demand_scales": [0.5, 1.0, 1.5],
        });
        let req = Request::builder().method("POST").uri("/api/batch/runs")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())).unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v: serde_json::Value =
            serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(v["cases"].as_array().unwrap().len(), 3);
        assert_eq!(v["status"].as_str(), Some("done"));
        assert!(v["base_scenario_id"].as_str().unwrap().starts_with("imported-batch-"));
        // Le label du 2e cas contient x1.00.
        assert!(v["cases"][1]["label"].as_str().unwrap().contains("x1.00"));

        // Listing.
        let req = Request::builder().method("GET").uri("/api/batch/runs")
            .body(Body::empty()).unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v: serde_json::Value =
            serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["case_count"].as_u64(), Some(3));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
