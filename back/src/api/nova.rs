//! Endpoints NoVa : liste des nominations scénario `.scn` disponibles pour le dataset actif.
//! Point d'entrée du picker de scénario (interface Natran) — l'id renvoyé correspond au
//! `scenario_id` attendu par la simulation WS pour activer les diagnostics pression.

use std::path::Path;

use axum::{Json, extract::State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use super::{ApiError, SharedState, active_dataset_id, active_gas_composition, active_network};

#[derive(Debug, Clone, Serialize)]
pub(super) struct NovaScenarioSummary {
    /// Identifiant à passer comme `options.scenario_id` au démarrage WS.
    pub id: String,
    /// Nom du fichier (ex. `nomination_mild_618.scn`).
    pub filename: String,
    /// Chemin relatif au dat_dir.
    pub relative_path: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct NovaCapacityRequest {
    /// Identifiant du scénario `.scn` (ex. `nomination_mild_618`).
    pub scenario_id: String,
    /// Sinks à étudier. Si absent → sinks marginaux par défaut présents dans le scénario.
    #[serde(default)]
    pub sink_ids: Option<Vec<String>>,
    /// Pas de dichotomie (défaut 6). Coût = (1 + pas) × solves par sink.
    #[serde(default)]
    pub bisection_steps: Option<usize>,
    /// Mode robuste (continuation de charge) — recommandé sur grands réseaux transport.
    #[serde(default)]
    pub robust_mode: Option<bool>,
    /// Itérations Newton max par solve.
    #[serde(default)]
    pub max_iter: Option<usize>,
}

fn api_error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: message.into(),
        }),
    )
}

const MAX_SINKS: usize = 12;
const DEFAULT_BISECTION_STEPS: usize = 6;
const MAX_BISECTION_STEPS: usize = 16;

/// Étude capacité NoVa : débit max faisable par sink (dichotomie sous bornes pression).
/// Coûteuse (plusieurs solves) — gardée par le sémaphore de slots simulation.
pub(super) async fn post_nova_capacity(
    State(state): State<SharedState>,
    Json(payload): Json<NovaCapacityRequest>,
) -> Result<Json<Vec<crate::solver::SinkCapacityReport>>, (StatusCode, Json<ApiError>)> {
    let dataset_id = active_dataset_id(&state);
    let scenario_path = crate::gaslib::resolve_scenario_path(
        &state.data_dir,
        &dataset_id,
        &payload.scenario_id,
    )
    .ok_or_else(|| {
        api_error(
            StatusCode::NOT_FOUND,
            format!(
                "scénario {} introuvable pour le dataset {}",
                payload.scenario_id, dataset_id
            ),
        )
    })?;

    let network = active_network(&state);
    let gas = active_gas_composition(&state);
    let bisection_steps = payload
        .bisection_steps
        .filter(|n| *n > 0)
        .map(|n| n.min(MAX_BISECTION_STEPS))
        .unwrap_or(DEFAULT_BISECTION_STEPS);
    let robust = payload.robust_mode.unwrap_or(true);
    let max_iter = payload.max_iter.unwrap_or(400);

    let sink_ids: Vec<String> = payload
        .sink_ids
        .map(|mut ids| {
            ids.truncate(MAX_SINKS);
            ids
        })
        .unwrap_or_else(|| {
            crate::solver::nova_capacity::default_marginal_sink_ids()
                .into_iter()
                .map(|s| s.to_string())
                .collect()
        });

    let preset = crate::solver::preset_from_request(
        network.node_count(),
        robust,
        max_iter,
        1e-3,
        120_000,
        1,
        None,
    );

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

    let pool = state.rayon_pool.clone();
    let join_result: Result<
        Result<Vec<crate::solver::SinkCapacityReport>, (StatusCode, Json<ApiError>)>,
        tokio::task::JoinError,
    > = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut scenario = crate::gaslib::load_scenario_demands(&scenario_path)
            .map_err(|err| api_error(StatusCode::UNPROCESSABLE_ENTITY, format!("{err:#}")))?;
        crate::gaslib::enrich_scenario_with_balance_hub(&network, &mut scenario);
        pool.install(|| {
            crate::solver::study_sinks_capacity(
                &network,
                &scenario,
                &sink_ids,
                &preset,
                gas,
                bisection_steps,
            )
        })
        .map_err(|err| api_error(StatusCode::UNPROCESSABLE_ENTITY, format!("{err:#}")))
    })
    .await;

    let inner = join_result
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("join: {err}")))?;
    Ok(Json(inner?))
}

/// Liste récursivement les `.scn` sous le dat_dir du dataset actif.
pub(super) async fn list_nova_scenarios(
    State(state): State<SharedState>,
) -> Json<Vec<NovaScenarioSummary>> {
    let dataset_id = super::active_dataset_id(&state);
    let dat_dir = state.data_dir.clone();
    let mut out: Vec<NovaScenarioSummary> = Vec::new();
    collect_scenarios(&dat_dir, &dat_dir, &mut out);
    // Tri déterministe : nom de fichier.
    out.sort_by(|a, b| a.filename.cmp(&b.filename));
    // Dédup par id (stem) : deux `.scn` de même stem dans des sous-dossiers différents
    // produiraient deux options de même valeur pour le `q-select` (emit-value). On garde
    // le premier après tri (chemin le plus court / nom le plus simple en premier).
    out.dedup_by(|a, b| a.id == b.id);
    // Marqueur implicite : on n'exclut rien, mais on s'assure que le scénario du dataset
    // (ex. GasLib-582.scn) reste sélectionnable. L'id = stem du fichier.
    let _ = dataset_id; // réservé pour un futur filtrage par dataset.
    Json(out)
}

fn collect_scenarios(base: &Path, dir: &Path, out: &mut Vec<NovaScenarioSummary>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_scenarios(base, &path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("scn") {
            continue;
        }
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let id = path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let relative_path = path
            .strip_prefix(base)
            .ok()
            .and_then(|p| p.to_str())
            .unwrap_or(&filename)
            .to_string();
        out.push(NovaScenarioSummary {
            id,
            filename,
            relative_path,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch_dir() -> PathBuf {
        scratch_dir_for("common")
    }

    fn scratch_dir_for(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gazflow-nova-test-{}-{suffix}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn collect_scenarios_walks_subdirs() {
        let tmp = scratch_dir();
        std::fs::create_dir_all(tmp.join("Nominations-582")).unwrap();
        std::fs::write(
            tmp.join("Nominations-582").join("nomination_mild_618.scn"),
            "<scenario/>",
        )
        .unwrap();
        std::fs::write(tmp.join("GasLib-582.scn"), "<scenario/>").unwrap();
        std::fs::write(tmp.join("not-a-scenario.txt"), "x").unwrap();

        let mut out = Vec::new();
        collect_scenarios(&tmp, &tmp, &mut out);
        let ids: Vec<&str> = out.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"nomination_mild_618"));
        assert!(ids.contains(&"GasLib-582"));
        assert!(!ids.contains(&"not-a-scenario"));
        let mild = out.iter().find(|s| s.id == "nomination_mild_618").unwrap();
        assert!(mild.relative_path.contains("Nominations-582"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn post_nova_capacity_returns_404_for_unknown_scenario() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use std::collections::HashMap;
        use tower::ServiceExt;

        let tmp = scratch_dir_for("http404");
        let mut net = crate::graph::GasNetwork::new();
        net.add_node(crate::graph::Node {
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
        let defaults: HashMap<String, f64> = HashMap::new();
        let app = crate::api::create_router_with_runtime_limits_and_datasets(
            net,
            defaults,
            "test".to_string(),
            vec!["test".to_string()],
            tmp.clone(),
            2,
            1,
        );

        let body = serde_json::json!({ "scenario_id": "does_not_exist" });
        let req = Request::builder()
            .method("POST")
            .uri("/api/nova/capacity")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn list_nova_scenarios_dedupes_by_id_across_subdirs() {
        use axum::body::Body;
        use axum::http::Request;
        use std::collections::HashMap;
        use tower::ServiceExt;

        let tmp = scratch_dir_for("dedup");
        std::fs::create_dir_all(tmp.join("a")).unwrap();
        std::fs::create_dir_all(tmp.join("b")).unwrap();
        std::fs::write(tmp.join("a").join("dup.scn"), "<scenario/>").unwrap();
        std::fs::write(tmp.join("b").join("dup.scn"), "<scenario/>").unwrap();
        std::fs::write(tmp.join("unique.scn"), "<scenario/>").unwrap();

        let net = crate::graph::GasNetwork::new();
        let defaults: HashMap<String, f64> = HashMap::new();
        let app = crate::api::create_router_with_runtime_limits_and_datasets(
            net,
            defaults,
            "test".to_string(),
            vec!["test".to_string()],
            tmp.clone(),
            2,
            1,
        );

        let req = Request::builder()
            .method("GET")
            .uri("/api/nova/scenarios")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let ids: Vec<&str> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids.iter().filter(|id| **id == "dup").count(), 1);
        assert!(ids.contains(&"unique"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
