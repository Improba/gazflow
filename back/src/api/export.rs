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

use crate::graph::GasNetwork;
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
    pub node_count: usize,
    pub pipe_count: usize,
    pub pipe_meta: HashMap<String, (String, String)>,
    pub capacity_violations: Vec<crate::solver::CapacityViolation>,
    pub adjusted_demands: Option<HashMap<String, f64>>,
    pub active_bounds: Vec<String>,
    pub objective_value: Option<f64>,
    pub outer_iterations: Option<usize>,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    capacity_violations: Vec<crate::solver::CapacityViolation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    adjusted_demands: Option<HashMap<String, f64>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    active_bounds: Vec<String>,
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
        "xlsx" | "excel" => {
            let xlsx = build_xlsx(&state, &record).map_err(|err| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(super::ApiError {
                        error: format!("EXPORT_INTERNAL_ERROR: {err}"),
                    }),
                )
            })?;
            let mut response = xlsx.into_response();
            response.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_static(
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                ),
            );
            response.headers_mut().insert(
                CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!(
                    "attachment; filename=\"{}-export.xlsx\"",
                    record.simulation_id
                ))
                .unwrap_or_else(|_| {
                    HeaderValue::from_static("attachment; filename=\"export.xlsx\"")
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
    network_id: String,
    network: &GasNetwork,
    demands: HashMap<String, f64>,
    result: SolverResult,
    elapsed_ms: u64,
) -> ExportRecord {
    let mut pipe_meta = HashMap::new();
    for pipe in network.pipes() {
        pipe_meta.insert(pipe.id.clone(), (pipe.from.clone(), pipe.to.clone()));
    }

    ExportRecord {
        simulation_id,
        created_at: now_iso8601_approx(),
        status: "converged".to_string(),
        network_id,
        scenario_id: "default".to_string(),
        demands,
        solver_method: "newton_hybrid_dense".to_string(),
        result,
        elapsed_ms,
        node_count: network.node_count(),
        pipe_count: network.edge_count(),
        pipe_meta,
        capacity_violations: Vec::new(),
        adjusted_demands: None,
        active_bounds: Vec::new(),
        objective_value: None,
        outer_iterations: None,
    }
}

pub(super) fn new_constrained_export_record(
    simulation_id: String,
    network_id: String,
    network: &GasNetwork,
    target_demands: HashMap<String, f64>,
    result: crate::solver::ConstrainedSolverResult,
    elapsed_ms: u64,
) -> ExportRecord {
    let mut pipe_meta = HashMap::new();
    for pipe in network.pipes() {
        pipe_meta.insert(pipe.id.clone(), (pipe.from.clone(), pipe.to.clone()));
    }
    ExportRecord {
        simulation_id,
        created_at: now_iso8601_approx(),
        status: if result.infeasibility_diagnostic.is_some() {
            "infeasible".to_string()
        } else {
            "converged".to_string()
        },
        network_id,
        scenario_id: "default".to_string(),
        demands: target_demands,
        solver_method: "constrained_projection".to_string(),
        result: SolverResult {
            pressures: result.pressures,
            flows: result.flows,
            iterations: result.iterations,
            residual: result.residual,
        },
        elapsed_ms,
        node_count: network.node_count(),
        pipe_count: network.edge_count(),
        pipe_meta,
        capacity_violations: result.capacity_violations,
        adjusted_demands: Some(result.adjusted_demands),
        active_bounds: result.active_bounds,
        objective_value: Some(result.objective_value),
        outer_iterations: Some(result.outer_iterations),
    }
}

fn build_json_payload(_state: &super::SharedState, record: &ExportRecord) -> ExportPayload {
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

    let mut flows: Vec<_> = record
        .result
        .flows
        .iter()
        .map(|(pipe_id, flow)| {
            let (from, to) = record
                .pipe_meta
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
            node_count: record.node_count,
            pipe_count: record.pipe_count,
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
        capacity_violations: record.capacity_violations.clone(),
        adjusted_demands: record.adjusted_demands.clone(),
        active_bounds: record.active_bounds.clone(),
    }
}

fn csv_escape(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

fn build_csv(_state: &super::SharedState, record: &ExportRecord) -> String {
    let mut lines = Vec::new();

    lines.push("# GazFlow Export".to_string());
    lines.push(format!("# Simulation: {}", record.simulation_id));
    lines.push(format!("# Network: {}", record.network_id));
    lines.push(format!("# Status: {}", record.status));
    lines.push(format!(
        "# Solver: {} | Iterations: {} | Residual: {:.3e} | Time: {} ms",
        record.solver_method, record.result.iterations, record.result.residual, record.elapsed_ms
    ));
    if let Some(outer) = record.outer_iterations {
        lines.push(format!("# Outer iterations: {}", outer));
    }
    if let Some(obj) = record.objective_value {
        lines.push(format!("# Objective value: {:.6e}", obj));
    }
    lines.push(String::new());

    lines.push("## Pressures".to_string());
    lines.push("node_id,pressure_bar".to_string());
    let mut pressure_rows: Vec<_> = record.result.pressures.iter().collect();
    pressure_rows.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (node_id, pressure) in pressure_rows {
        lines.push(format!("{},{pressure:.6}", csv_escape(node_id)));
    }
    lines.push(String::new());

    lines.push("## Flows".to_string());
    lines.push("pipe_id,from,to,flow_m3s,abs_flow_m3s,direction".to_string());
    let mut flow_rows: Vec<_> = record.result.flows.iter().collect();
    flow_rows.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (pipe_id, flow) in flow_rows {
        let (from, to) = record
            .pipe_meta
            .get(pipe_id)
            .cloned()
            .unwrap_or_else(|| (String::new(), String::new()));
        let direction = if *flow >= 0.0 { "forward" } else { "reverse" };
        lines.push(format!(
            "{},{},{},{flow:.6},{:.6},{direction}",
            csv_escape(pipe_id),
            csv_escape(&from),
            csv_escape(&to),
            flow.abs()
        ));
    }

    if record.adjusted_demands.is_some() || !record.demands.is_empty() {
        lines.push(String::new());
        lines.push("## Demands".to_string());
        lines.push("node_id,target_demand_m3s,adjusted_demand_m3s,active_bound".to_string());
        let mut demand_ids: Vec<_> = record.demands.keys().collect();
        demand_ids.sort();
        for node_id in demand_ids {
            let target = record.demands.get(node_id).copied().unwrap_or(0.0);
            let adjusted = record
                .adjusted_demands
                .as_ref()
                .and_then(|m| m.get(node_id))
                .copied();
            let active = if record.active_bounds.contains(node_id) {
                "yes"
            } else {
                "no"
            };
            match adjusted {
                Some(adj) => lines.push(format!("{node_id},{target:.6},{adj:.6},{active}")),
                None => lines.push(format!("{node_id},{target:.6},,{active}")),
            }
        }
    }

    if !record.capacity_violations.is_empty() {
        lines.push(String::new());
        lines.push("## Capacity Violations".to_string());
        lines.push("element_id,element_type,bound_type,limit_m3s,actual_m3s,margin_m3s".to_string());
        for v in &record.capacity_violations {
            let etype = match v.element_type {
                crate::solver::capacity::ViolationElementType::Node => "node",
                crate::solver::capacity::ViolationElementType::Pipe => "pipe",
            };
            let btype = match v.bound_type {
                crate::solver::capacity::BoundType::Min => "min",
                crate::solver::capacity::BoundType::Max => "max",
            };
            lines.push(format!(
                "{},{etype},{btype},{:.6},{:.6},{:.6}",
                v.element_id, v.limit, v.actual, v.margin
            ));
        }
    }

    lines.join("\n")
}

fn build_xlsx(_state: &super::SharedState, record: &ExportRecord) -> Result<Vec<u8>, String> {
    use rust_xlsxwriter::{Color, Format, FormatBorder, Workbook};

    let mut workbook = Workbook::new();

    let header_fmt = Format::new()
        .set_bold()
        .set_background_color(Color::RGB(0x2C3E50))
        .set_font_color(Color::White)
        .set_border(FormatBorder::Thin);
    let number_fmt = Format::new().set_num_format("0.000000");
    let title_fmt = Format::new().set_bold().set_font_size(14);
    let meta_fmt = Format::new().set_italic();

    // === Sheet 1: Pressures ===
    let sheet = workbook.add_worksheet();
    sheet.set_name("Pressions").map_err(|e| e.to_string())?;
    sheet.set_column_width(0, 20).map_err(|e| e.to_string())?;
    sheet.set_column_width(1, 15).map_err(|e| e.to_string())?;

    sheet
        .write_string_with_format(0, 0, "Pressions nodales", &title_fmt)
        .map_err(|e| e.to_string())?;
    sheet
        .write_string_with_format(
            1,
            0,
            &format!(
                "Réseau: {} | Solveur: {} | Itérations: {} | Résidu: {:.3e}",
                record.network_id,
                record.solver_method,
                record.result.iterations,
                record.result.residual
            ),
            &meta_fmt,
        )
        .map_err(|e| e.to_string())?;

    let hrow = 3u32;
    sheet
        .write_string_with_format(hrow, 0, "Nœud", &header_fmt)
        .map_err(|e| e.to_string())?;
    sheet
        .write_string_with_format(hrow, 1, "Pression (bar)", &header_fmt)
        .map_err(|e| e.to_string())?;

    let mut pressure_rows: Vec<_> = record.result.pressures.iter().collect();
    pressure_rows.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (i, (node_id, pressure)) in pressure_rows.into_iter().enumerate() {
        let row = hrow + 1 + i as u32;
        sheet
            .write_string(row, 0, node_id.as_str())
            .map_err(|e| e.to_string())?;
        sheet
            .write_number_with_format(row, 1, *pressure, &number_fmt)
            .map_err(|e| e.to_string())?;
    }

    // === Sheet 2: Flows ===
    let sheet = workbook.add_worksheet();
    sheet.set_name("Débits").map_err(|e| e.to_string())?;
    for col in 0..6u16 {
        sheet
            .set_column_width(col, 18)
            .map_err(|e| e.to_string())?;
    }

    sheet
        .write_string_with_format(0, 0, "Débits dans les conduites", &title_fmt)
        .map_err(|e| e.to_string())?;

    let hrow = 2u32;
    let headers = [
        "Conduite",
        "De",
        "Vers",
        "Débit (m³/s)",
        "|Débit| (m³/s)",
        "Direction",
    ];
    for (c, h) in headers.iter().enumerate() {
        sheet
            .write_string_with_format(hrow, c as u16, *h, &header_fmt)
            .map_err(|e| e.to_string())?;
    }

    let mut flow_rows: Vec<_> = record.result.flows.iter().collect();
    flow_rows.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (i, (pipe_id, flow)) in flow_rows.into_iter().enumerate() {
        let row = hrow + 1 + i as u32;
        let (from, to) = record
            .pipe_meta
            .get(pipe_id)
            .cloned()
            .unwrap_or_else(|| (String::new(), String::new()));
        sheet
            .write_string(row, 0, pipe_id.as_str())
            .map_err(|e| e.to_string())?;
        sheet
            .write_string(row, 1, &from)
            .map_err(|e| e.to_string())?;
        sheet
            .write_string(row, 2, &to)
            .map_err(|e| e.to_string())?;
        sheet
            .write_number_with_format(row, 3, *flow, &number_fmt)
            .map_err(|e| e.to_string())?;
        sheet
            .write_number_with_format(row, 4, flow.abs(), &number_fmt)
            .map_err(|e| e.to_string())?;
        sheet
            .write_string(row, 5, if *flow >= 0.0 { "→" } else { "←" })
            .map_err(|e| e.to_string())?;
    }

    // === Sheet 3: Demands (if constrained) ===
    if record.adjusted_demands.is_some() || !record.demands.is_empty() {
        let sheet = workbook.add_worksheet();
        sheet.set_name("Demandes").map_err(|e| e.to_string())?;
        for col in 0..4u16 {
            sheet
                .set_column_width(col, 22)
                .map_err(|e| e.to_string())?;
        }

        sheet
            .write_string_with_format(0, 0, "Demandes (cible vs ajusté)", &title_fmt)
            .map_err(|e| e.to_string())?;

        let hrow = 2u32;
        let headers = [
            "Nœud",
            "Demande cible (m³/s)",
            "Demande ajustée (m³/s)",
            "Borne active",
        ];
        for (c, h) in headers.iter().enumerate() {
            sheet
                .write_string_with_format(hrow, c as u16, *h, &header_fmt)
                .map_err(|e| e.to_string())?;
        }

        let mut demand_ids: Vec<_> = record.demands.keys().collect();
        demand_ids.sort();
        for (i, node_id) in demand_ids.into_iter().enumerate() {
            let row = hrow + 1 + i as u32;
            let target = record.demands.get(node_id).copied().unwrap_or(0.0);
            let adjusted = record
                .adjusted_demands
                .as_ref()
                .and_then(|m| m.get(node_id))
                .copied();
            let active = record.active_bounds.contains(node_id);

            sheet
                .write_string(row, 0, node_id.as_str())
                .map_err(|e| e.to_string())?;
            sheet
                .write_number_with_format(row, 1, target, &number_fmt)
                .map_err(|e| e.to_string())?;
            if let Some(adj) = adjusted {
                sheet
                    .write_number_with_format(row, 2, adj, &number_fmt)
                    .map_err(|e| e.to_string())?;
            }
            sheet
                .write_string(row, 3, if active { "oui" } else { "non" })
                .map_err(|e| e.to_string())?;
        }
    }

    // === Sheet 4: Violations (if any) ===
    if !record.capacity_violations.is_empty() {
        let sheet = workbook.add_worksheet();
        sheet.set_name("Violations").map_err(|e| e.to_string())?;
        for col in 0..6u16 {
            sheet
                .set_column_width(col, 18)
                .map_err(|e| e.to_string())?;
        }

        let warn_fmt = Format::new()
            .set_bold()
            .set_font_color(Color::RGB(0xE74C3C))
            .set_font_size(14);
        sheet
            .write_string_with_format(
                0,
                0,
                &format!(
                    "⚠ {} violation(s) de capacité",
                    record.capacity_violations.len()
                ),
                &warn_fmt,
            )
            .map_err(|e| e.to_string())?;

        let hrow = 2u32;
        let headers = [
            "Élément",
            "Type",
            "Borne",
            "Limite (m³/s)",
            "Réel (m³/s)",
            "Marge (m³/s)",
        ];
        for (c, h) in headers.iter().enumerate() {
            sheet
                .write_string_with_format(hrow, c as u16, *h, &header_fmt)
                .map_err(|e| e.to_string())?;
        }

        let violation_fmt = Format::new().set_background_color(Color::RGB(0xFDEDEC));

        for (i, v) in record.capacity_violations.iter().enumerate() {
            let row = hrow + 1 + i as u32;
            let etype = match v.element_type {
                crate::solver::capacity::ViolationElementType::Node => "nœud",
                crate::solver::capacity::ViolationElementType::Pipe => "conduite",
            };
            let btype = match v.bound_type {
                crate::solver::capacity::BoundType::Min => "min",
                crate::solver::capacity::BoundType::Max => "max",
            };
            sheet
                .write_string_with_format(row, 0, &v.element_id, &violation_fmt)
                .map_err(|e| e.to_string())?;
            sheet
                .write_string_with_format(row, 1, etype, &violation_fmt)
                .map_err(|e| e.to_string())?;
            sheet
                .write_string_with_format(row, 2, btype, &violation_fmt)
                .map_err(|e| e.to_string())?;
            sheet
                .write_number_with_format(row, 3, v.limit, &number_fmt)
                .map_err(|e| e.to_string())?;
            sheet
                .write_number_with_format(row, 4, v.actual, &number_fmt)
                .map_err(|e| e.to_string())?;
            sheet
                .write_number_with_format(row, 5, v.margin, &number_fmt)
                .map_err(|e| e.to_string())?;
        }
    }

    workbook.save_to_buffer().map_err(|e| e.to_string())
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

    if let Ok(xlsx_content) = build_xlsx(state, record) {
        zip.start_file("result.xlsx", options)
            .map_err(|err| format!("zip result.xlsx: {err}"))?;
        zip.write_all(&xlsx_content)
            .map_err(|err| format!("write result.xlsx: {err}"))?;
    }

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
            flow_min_m3s: None,
            flow_max_m3s: None,
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
            flow_min_m3s: None,
            flow_max_m3s: None,
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
            flow_min_m3s: None,
            flow_max_m3s: None,
        });

        Arc::new(super::super::AppState {
            network: Arc::new(RwLock::new(Arc::new(network))),
            default_demands: Arc::new(RwLock::new(Arc::new(HashMap::new()))),
            active_dataset: Arc::new(RwLock::new("custom".to_string())),
            available_datasets: Arc::new(vec!["custom".to_string()]),
            data_dir: Arc::new(std::path::PathBuf::from("dat")),
            simulation_slots: Arc::new(Semaphore::new(1)),
            simulation_capacity: 1,
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
            "custom".to_string(),
            &state
                .network
                .read()
                .expect("network lock should not be poisoned"),
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
            "custom".to_string(),
            &state
                .network
                .read()
                .expect("network lock should not be poisoned"),
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
        assert!(csv.contains("# GazFlow Export"));
        assert!(csv.contains("## Pressures"));
        assert!(csv.contains("## Flows"));
        assert!(csv.contains("node_id,pressure_bar"));
        assert!(csv.contains("pipe_id,from,to,flow_m3s"));
    }

    #[test]
    fn test_export_csv_includes_sections() {
        let state = fake_state();
        let mut pressures = HashMap::new();
        pressures.insert("A".to_string(), 70.0);
        let mut flows = HashMap::new();
        flows.insert("P1".to_string(), 1.0);
        let record = new_export_record(
            "sim-test".to_string(),
            "custom".to_string(),
            &state
                .network
                .read()
                .expect("network lock should not be poisoned"),
            [("A".to_string(), 0.0), ("B".to_string(), -10.0)]
                .into_iter()
                .collect(),
            SolverResult {
                pressures,
                flows,
                iterations: 3,
                residual: 1e-4,
            },
            12,
        );
        let csv = build_csv(&state, &record);
        assert!(csv.contains("## Pressures"), "should have pressures section");
        assert!(csv.contains("## Flows"), "should have flows section");
        assert!(csv.contains("## Demands"), "should have demands section");
        assert!(csv.contains("node_id,pressure_bar"), "pressure header");
        assert!(
            csv.contains("pipe_id,from,to,flow_m3s"),
            "flow header"
        );
    }

    #[test]
    fn test_export_result_xlsx_generates_valid_buffer() {
        let state = fake_state();
        let mut pressures = HashMap::new();
        pressures.insert("A".to_string(), 70.0);
        pressures.insert("B".to_string(), 65.0);
        let mut flows = HashMap::new();
        flows.insert("P1".to_string(), 5.5);
        let record = new_export_record(
            "sim-test".to_string(),
            "custom".to_string(),
            &state.network.read().expect("lock"),
            HashMap::new(),
            SolverResult {
                pressures,
                flows,
                iterations: 7,
                residual: 1e-4,
            },
            42,
        );
        let xlsx = build_xlsx(&state, &record).expect("xlsx generation should succeed");
        assert!(
            xlsx.len() > 100,
            "xlsx should have content, got {} bytes",
            xlsx.len()
        );
        assert_eq!(
            &xlsx[0..2],
            b"PK",
            "xlsx should start with PK (zip signature)"
        );
    }
}
