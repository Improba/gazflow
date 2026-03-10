use std::collections::HashMap;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{
        HeaderValue, StatusCode,
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;

use crate::solver::SolverResult;

#[derive(Debug, Clone)]
pub(super) struct ExportRecord {
    pub simulation_id: String,
    pub created_at: String,
    pub status: String,
    pub network_id: String,
    pub scenario_id: String,
    pub demands: HashMap<String, f64>,
    pub solver_method: String,
    pub result: SolverResult,
    pub elapsed_ms: u64,
}

#[derive(Debug, Deserialize)]
pub(super) struct ExportQuery {
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub include_logs: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ExportPayload {
    schema_version: String,
    simulation: SimulationSection,
    units: UnitsSection,
    results: ResultsSection,
    stats: StatsSection,
}

#[derive(Debug, Serialize)]
struct SimulationSection {
    id: String,
    created_at: String,
    status: String,
    network_id: String,
    scenario_id: String,
    demands: HashMap<String, f64>,
    solver: SolverSection,
}

#[derive(Debug, Serialize)]
struct SolverSection {
    method: String,
    iterations: usize,
    residual: f64,
    elapsed_ms: u64,
}

#[derive(Debug, Serialize)]
struct UnitsSection {
    pressure: &'static str,
    flow: &'static str,
}

#[derive(Debug, Serialize)]
struct ResultsSection {
    pressures: Vec<PressureRow>,
    flows: Vec<FlowRow>,
}

#[derive(Debug, Serialize)]
struct PressureRow {
    node_id: String,
    pressure: f64,
}

#[derive(Debug, Serialize)]
struct FlowRow {
    pipe_id: String,
    from: String,
    to: String,
    flow: f64,
    abs_flow: f64,
    direction: &'static str,
}

#[derive(Debug, Serialize)]
struct StatsSection {
    node_count: usize,
    pipe_count: usize,
    min_pressure: f64,
    max_pressure: f64,
    max_abs_flow: f64,
}

pub(super) async fn get_export(
    Path(simulation_id): Path<String>,
    Query(query): Query<ExportQuery>,
    State(state): State<super::SharedState>,
) -> Result<Response, (StatusCode, Json<super::ApiError>)> {
    let format = query
        .format
        .unwrap_or_else(|| "json".to_string())
        .to_ascii_lowercase();

    let record = {
        let guard = state.exports.read().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(super::ApiError {
                    error: "EXPORT_INTERNAL_ERROR".to_string(),
                }),
            )
        })?;
        guard.get(&simulation_id).cloned().ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(super::ApiError {
                    error: format!("EXPORT_NOT_FOUND: {simulation_id}"),
                }),
            )
        })?
    };

    match format.as_str() {
        "json" => {
            let payload = build_json_payload(&state, &record);
            Ok(Json(payload).into_response())
        }
        "csv" => {
            let csv = build_csv(&state, &record);
            let mut response = csv.into_response();
            response.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_static("text/csv; charset=utf-8"),
            );
            response.headers_mut().insert(
                CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!(
                    "attachment; filename=\"{}-export.csv\"",
                    record.simulation_id
                ))
                .unwrap_or_else(|_| {
                    HeaderValue::from_static("attachment; filename=\"export.csv\"")
                }),
            );
            Ok(response)
        }
        "zip" => {
            let include_logs = query.include_logs.unwrap_or(false);
            let archive = build_zip_bundle(&state, &record, include_logs).map_err(|err| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(super::ApiError {
                        error: format!("EXPORT_INTERNAL_ERROR: {err}"),
                    }),
                )
            })?;
            let mut response = archive.into_response();
            response
                .headers_mut()
                .insert(CONTENT_TYPE, HeaderValue::from_static("application/zip"));
            response.headers_mut().insert(
                CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!(
                    "attachment; filename=\"{}-export.zip\"",
                    record.simulation_id
                ))
                .unwrap_or_else(|_| {
                    HeaderValue::from_static("attachment; filename=\"export.zip\"")
                }),
            );
            Ok(response)
        }
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(super::ApiError {
                error: format!("EXPORT_FORMAT_UNSUPPORTED: {format}"),
            }),
        )),
    }
}

pub(super) fn store_export_record(state: &super::SharedState, record: ExportRecord) {
    if let Ok(mut guard) = state.exports.write() {
        guard.insert(record.simulation_id.clone(), record);
    }
}

pub(super) fn new_export_record(
    simulation_id: String,
    demands: HashMap<String, f64>,
    result: SolverResult,
    elapsed_ms: u64,
) -> ExportRecord {
    ExportRecord {
        simulation_id,
        created_at: now_iso8601_approx(),
        status: "converged".to_string(),
        network_id: "GasLib-11".to_string(),
        scenario_id: "default".to_string(),
        demands,
        solver_method: "newton_hybrid_dense".to_string(),
        result,
        elapsed_ms,
    }
}

fn build_json_payload(state: &super::SharedState, record: &ExportRecord) -> ExportPayload {
    let mut pressures: Vec<_> = record
        .result
        .pressures
        .iter()
        .map(|(node_id, pressure)| PressureRow {
            node_id: node_id.clone(),
            pressure: *pressure,
        })
        .collect();
    pressures.sort_by(|a, b| a.node_id.cmp(&b.node_id));

    let mut pipe_meta = HashMap::new();
    for pipe in state.network.pipes() {
        pipe_meta.insert(pipe.id.clone(), (pipe.from.clone(), pipe.to.clone()));
    }

    let mut flows: Vec<_> = record
        .result
        .flows
        .iter()
        .map(|(pipe_id, flow)| {
            let (from, to) = pipe_meta
                .get(pipe_id)
                .cloned()
                .unwrap_or_else(|| (String::new(), String::new()));
            FlowRow {
                pipe_id: pipe_id.clone(),
                from,
                to,
                flow: *flow,
                abs_flow: flow.abs(),
                direction: if *flow >= 0.0 { "forward" } else { "reverse" },
            }
        })
        .collect();
    flows.sort_by(|a, b| a.pipe_id.cmp(&b.pipe_id));

    let min_pressure = pressures
        .iter()
        .map(|p| p.pressure)
        .fold(f64::INFINITY, f64::min);
    let max_pressure = pressures
        .iter()
        .map(|p| p.pressure)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_abs_flow = flows.iter().map(|f| f.abs_flow).fold(0.0_f64, f64::max);

    ExportPayload {
        schema_version: "gazflow-export/v1".to_string(),
        simulation: SimulationSection {
            id: record.simulation_id.clone(),
            created_at: record.created_at.clone(),
            status: record.status.clone(),
            network_id: record.network_id.clone(),
            scenario_id: record.scenario_id.clone(),
            demands: record.demands.clone(),
            solver: SolverSection {
                method: record.solver_method.clone(),
                iterations: record.result.iterations,
                residual: record.result.residual,
                elapsed_ms: record.elapsed_ms,
            },
        },
        units: UnitsSection {
            pressure: "bar",
            flow: "m3/s",
        },
        results: ResultsSection { pressures, flows },
        stats: StatsSection {
            node_count: state.network.node_count(),
            pipe_count: state.network.edge_count(),
            min_pressure: if min_pressure.is_finite() {
                min_pressure
            } else {
                0.0
            },
            max_pressure: if max_pressure.is_finite() {
                max_pressure
            } else {
                0.0
            },
            max_abs_flow,
        },
    }
}

fn build_csv(state: &super::SharedState, record: &ExportRecord) -> String {
    let mut lines = Vec::new();
    lines.push("kind,id,from,to,value,abs_value,unit,direction".to_string());

    let mut pressure_rows: Vec<_> = record.result.pressures.iter().collect();
    pressure_rows.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (node_id, pressure) in pressure_rows {
        lines.push(format!(
            "pressure,{node_id},,,{pressure},{},bar,",
            pressure.abs()
        ));
    }

    let mut pipe_meta = HashMap::new();
    for pipe in state.network.pipes() {
        pipe_meta.insert(pipe.id.clone(), (pipe.from.clone(), pipe.to.clone()));
    }

    let mut flow_rows: Vec<_> = record.result.flows.iter().collect();
    flow_rows.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (pipe_id, flow) in flow_rows {
        let (from, to) = pipe_meta
            .get(pipe_id)
            .cloned()
            .unwrap_or_else(|| (String::new(), String::new()));
        let direction = if *flow >= 0.0 { "forward" } else { "reverse" };
        lines.push(format!(
            "flow,{pipe_id},{from},{to},{flow},{},m3/s,{direction}",
            flow.abs()
        ));
    }

    lines.join("\n")
}

fn build_zip_bundle(
    state: &super::SharedState,
    record: &ExportRecord,
    include_logs: bool,
) -> Result<Vec<u8>, String> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut cursor);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let json_payload = build_json_payload(state, record);
    let json_content =
        serde_json::to_vec_pretty(&json_payload).map_err(|err| format!("json serialize: {err}"))?;
    zip.start_file("result.json", options)
        .map_err(|err| format!("zip result.json: {err}"))?;
    zip.write_all(&json_content)
        .map_err(|err| format!("write result.json: {err}"))?;

    let csv_content = build_csv(state, record);
    zip.start_file("result.csv", options)
        .map_err(|err| format!("zip result.csv: {err}"))?;
    zip.write_all(csv_content.as_bytes())
        .map_err(|err| format!("write result.csv: {err}"))?;

    let context = serde_json::json!({
        "simulation_id": record.simulation_id,
        "network_id": record.network_id,
        "scenario_id": record.scenario_id,
        "exported_at": now_iso8601_approx(),
        "include_logs": include_logs
    });
    let context_content =
        serde_json::to_vec_pretty(&context).map_err(|err| format!("context serialize: {err}"))?;
    zip.start_file("context.json", options)
        .map_err(|err| format!("zip context.json: {err}"))?;
    zip.write_all(&context_content)
        .map_err(|err| format!("write context.json: {err}"))?;

    if include_logs {
        zip.start_file("logs.ndjson", options)
            .map_err(|err| format!("zip logs.ndjson: {err}"))?;
        zip.write_all(b"{\"message\":\"logs not persisted in backend v1\"}\n")
            .map_err(|err| format!("write logs.ndjson: {err}"))?;
    }

    zip.finish().map_err(|err| format!("zip finish: {err}"))?;
    Ok(cursor.into_inner())
}

fn now_iso8601_approx() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("unix-ms-{ms}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{ConnectionKind, GasNetwork, Node, Pipe};
    use rayon::ThreadPoolBuilder;
    use std::sync::{Arc, RwLock};
    use tokio::sync::Semaphore;

    fn fake_state() -> super::super::SharedState {
        let mut network = GasNetwork::new();
        network.add_node(Node {
            id: "A".into(),
            x: 0.0,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
        });
        network.add_node(Node {
            id: "B".into(),
            x: 1.0,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
        });
        network.add_pipe(Pipe {
            id: "P1".into(),
            from: "A".into(),
            to: "B".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 1.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
        });

        Arc::new(super::super::AppState {
            network: Arc::new(network),
            default_demands: Arc::new(HashMap::new()),
            simulation_slots: Arc::new(Semaphore::new(1)),
            rayon_pool: Arc::new(
                ThreadPoolBuilder::new()
                    .num_threads(1)
                    .build()
                    .expect("pool"),
            ),
            exports: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    #[test]
    fn test_export_result_json_schema() {
        let state = fake_state();
        let mut pressures = HashMap::new();
        pressures.insert("A".to_string(), 70.0);
        pressures.insert("B".to_string(), 65.0);
        let mut flows = HashMap::new();
        flows.insert("P1".to_string(), 5.5);
        let record = new_export_record(
            "sim-test".to_string(),
            HashMap::new(),
            SolverResult {
                pressures,
                flows,
                iterations: 7,
                residual: 1e-4,
            },
            42,
        );

        let payload = build_json_payload(&state, &record);
        assert_eq!(payload.schema_version, "gazflow-export/v1");
        assert_eq!(payload.units.pressure, "bar");
        assert_eq!(payload.units.flow, "m3/s");
        assert_eq!(payload.results.pressures.len(), 2);
        assert_eq!(payload.results.flows.len(), 1);
    }

    #[test]
    fn test_export_result_csv_headers() {
        let state = fake_state();
        let mut pressures = HashMap::new();
        pressures.insert("A".to_string(), 70.0);
        let mut flows = HashMap::new();
        flows.insert("P1".to_string(), 1.0);
        let record = new_export_record(
            "sim-test".to_string(),
            HashMap::new(),
            SolverResult {
                pressures,
                flows,
                iterations: 3,
                residual: 1e-4,
            },
            12,
        );
        let csv = build_csv(&state, &record);
        let mut lines = csv.lines();
        assert_eq!(
            lines.next(),
            Some("kind,id,from,to,value,abs_value,unit,direction")
        );
        assert!(csv.contains("pressure,A,,,70"));
        assert!(csv.contains("flow,P1,A,B,1"));
    }
}
