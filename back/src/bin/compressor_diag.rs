//! Diagnostic compresseurs transport (palier I-A0 / I-A, GasLib-582).
//!
//! **Chemin bench** (hors prod API) : `solve_with_mass_balance_refinement` + refinement
//! itératif (ancrages `innode_*`, abandon partiel Q opt-in via
//! `GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT=1`).
//!
//! **Prod** (`main`, API REST) : `prepare_transport_scenario` + `effective_solver_demands`
//! sans refinement itératif — nomination GasLib préservée (sauf slack pression).
//!
//! Protocole figé : réseau baseline connecté, CDF désactivé. Hors CI.
//! Prérequis : `./scripts/fetch_gaslib.sh GasLib-582`

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use gazflow_back::compressor::{CompressorOperatingContext, effective_ratio_with_nominal};
use gazflow_back::gaslib::{
    compressor_decision_variables_enabled, compressor_hard_coupling_enabled,
    detect_shortpipe_boundary_pairs, effective_solver_demands_for_network,
    enrich_scenario_with_balance_hub,
    load_network, load_scenario_demands, network_with_scenario_boundaries,
    scenario_boundary_active_envelopes_enabled, scenario_boundary_partial_accept_enabled,
    scenario_pressure_envelopes_enabled, scenario_pressure_floor_anchor_enabled,
    scenario_pressure_in_newton_enabled, shortpipe_coupled_envelopes_enabled,
    shortpipe_merge_boundaries_enabled, entry_transport_anchor_enabled,
    entry_transport_anchored_ids,
    transport_minimal_anchors_enabled, ShortPipeBoundaryPair,
};
use gazflow_back::graph::{ConnectionKind, GasNetwork};
use gazflow_back::solver::{
    ContinuationStepEvent, GasComposition, MassBalanceReport, SolverResult,
    apply_compressor_decision_updates,
    apply_map_ratios_after_continuation_step, boundary_nomination_slips,
    compressor_accept_partial_enabled, compressor_map_mode,
    compressor_pressure_from_coeff, estimated_compressor_map_flow_m3s, mass_balance_report,
    preset_robust, scenario_pressure_slips, solve_with_mass_balance_refinement,
    boundary_pressure_supply_reports, upstream_pressure_trace, BoundaryPressureSupplyReport,
    BoundaryNominationSlip, CompressorMapMode, ScenarioPressureSlip,
};
use serde::Serialize;

const DEFAULT_GAS_TEMPERATURE_K: f64 = 288.15;
const FLOW_EVAL_THRESHOLD_M3S: f64 = 0.01;
const MAX_PLAUSIBLE_COMPRESSOR_FLOW_M3S: f64 = 250.0;
const HANDOFF_PREFER_ESTIMATED_RESIDUAL: f64 = 7.0;
/// Pression amont indicative transport quand le solve échoue avant convergence (582 mild_618).
const TRANSPORT_FALLBACK_INLET_BAR: f64 = 40.0;
const DECISION_MIN_FED_P_IN_BAR: f64 = 5.0;

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn diag_env_enthalpic() -> bool {
    env_flag("GAZFLOW_COMPRESSOR_ENTHALPIC")
        || env_flag("GAZFLOW_COMPRESSOR_ENERGY_CLOSURE")
        || env_flag("GAZFLOW_COMPRESSOR_ENERGY_EQUATION")
}

fn diag_compressor_relax() -> f64 {
    std::env::var("GAZFLOW_COMPRESSOR_RELAX")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.5)
        .clamp(0.0, 1.0)
}

fn compressor_decision_report(
    network: &GasNetwork,
    result: &SolverResult,
) -> Option<Vec<CompressorDecisionReportEntry>> {
    if !compressor_decision_variables_enabled() {
        return None;
    }
    let mut preview_network = network.clone();
    let updates = apply_compressor_decision_updates(
        &mut preview_network,
        result,
        diag_compressor_relax(),
        DECISION_MIN_FED_P_IN_BAR,
    );
    let total_slack_before = updates.total_slack_before;
    let total_slack_after = updates.total_slack_after;
    let mut report = updates
        .updates
        .into_iter()
        .map(|entry| CompressorDecisionReportEntry {
            cs_id: entry.cs_id,
            from_node: entry.from_node,
            to_node: entry.to_node,
            ratio_before: entry.ratio_before,
            ratio_after: entry.ratio_after,
            p_in_bar: entry.p_in_bar,
            downstream_deficits: entry
                .downstream_deficits
                .into_iter()
                .map(|d| CompressorDecisionSinkDeficit {
                    sink_id: d.sink_id,
                    lower_bar: d.lower_bar,
                    p_resolved: d.p_resolved_bar,
                    deficit_bar: d.deficit_bar,
                })
                .collect(),
            total_slack_before,
            total_slack_after,
        })
        .collect::<Vec<_>>();
    report.sort_by(|a, b| a.cs_id.cmp(&b.cs_id));
    Some(report)
}

fn compressor_hard_coupling_report(
    network: &GasNetwork,
    result: &SolverResult,
) -> Option<Vec<CompressorHardCouplingEntry>> {
    if !compressor_hard_coupling_enabled() || !compressor_decision_variables_enabled() {
        return None;
    }
    let mut entries = Vec::new();
    for pipe in network.pipes() {
        if pipe.kind != ConnectionKind::CompressorStation || !pipe.hydraulically_active() {
            continue;
        }
        let r = pipe
            .compressor_ratio_max
            .or(pipe.equipment.compressor_nominal_ratio)
            .unwrap_or(1.08)
            .max(1.0);
        let p_in = result.pressures.get(&pipe.from).copied().unwrap_or(0.0);
        let p_out = result.pressures.get(&pipe.to).copied().unwrap_or(0.0);
        let p_out_expected = r * p_in;
        let achieved = if p_in > 1e-6 { p_out / p_in } else { 0.0 };
        entries.push(CompressorHardCouplingEntry {
            cs_id: pipe.id.clone(),
            from_node: pipe.from.clone(),
            to_node: pipe.to.clone(),
            declared_ratio: r,
            achieved_ratio: achieved,
            p_in_bar: p_in,
            p_out_bar: p_out,
            p_out_expected_bar: p_out_expected,
            coupling_residual_bar: p_out - p_out_expected,
        });
    }
    entries.sort_by(|a, b| a.cs_id.cmp(&b.cs_id));
    Some(entries)
}

#[derive(Debug)]
struct CliArgs {
    dataset: String,
    no_r2_cap: bool,
    map_mode: Option<String>,
    json_out: Option<PathBuf>,
    csv_out: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct DiagFlags {
    skip_cdf_routing: bool,
    disable_r2_cap: bool,
    map_mode: String,
    catalog_stations: usize,
    preset: &'static str,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    compressor_enthalpic: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    compressor_energy_closure: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    compressor_energy_equation: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    scenario_pressure_envelopes: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    transport_minimal_anchors: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    scenario_pressure_in_newton: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    compressor_strict_newton: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    scenario_shortpipe_coupled_envelopes: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    scenario_pressure_floor_anchor: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    scenario_boundary_active_envelopes: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    scenario_boundary_partial_accept: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    scenario_shortpipe_merge_boundaries: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    entry_transport_anchor: bool,
}

#[derive(Debug, Serialize)]
struct UpstreamPressureHop {
    node_id: String,
    pressure_bar: f64,
}

#[derive(Debug, Serialize)]
struct CompressorStationDiag {
    pipe_id: String,
    from: String,
    to: String,
    flow_m3s: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    map_eval_q_m3s: Option<f64>,
    ratio_max: f64,
    effective_r2: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    map_target_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inlet_pressure_bar: Option<f64>,
}

#[derive(Debug, Serialize)]
struct DiagOutput {
    status: &'static str,
    dataset: String,
    network: String,
    scenario: Option<String>,
    residual: Option<f64>,
    demand_scale: Option<f64>,
    iterations: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    continuation_scales: Vec<f64>,
    flags: DiagFlags,
    compressor_stations: Vec<CompressorStationDiag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compressor_decision_report: Option<Vec<CompressorDecisionReportEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compressor_hard_coupling_report: Option<Vec<CompressorHardCouplingEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mass_balance: Option<MassBalanceReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mass_balance_refinement_passes: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    mass_balance_anchors: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    contract_flow_relaxed: Vec<String>,
    /// Bilan massique avec demandes **nominales** du `.scn`.
    #[serde(skip_serializing_if = "Option::is_none")]
    nomination_mass_balance: Option<MassBalanceReport>,
    /// Écarts débit aux points frontière nominés (entry/exit à Q≠0).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    boundary_nomination_slips: Vec<BoundaryNominationSlip>,
    /// Violations enveloppe pression (`.scn` + `.net`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    scenario_pressure_slips: Vec<ScenarioPressureSlip>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    shortpipe_boundary_couplings: Vec<ShortPipeBoundaryPair>,
    /// Trace amont depuis le pire `scenario_pressure_slip` (6 sauts max).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    worst_pressure_upstream_trace: Vec<UpstreamPressureHop>,
    /// Alimentation pression amont vs plancher contractuel (Phase II diag).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    boundary_pressure_supply: Vec<BoundaryPressureSupplyReport>,
    /// Sondes pression/débit sur nœuds/arcs demandés (GAZFLOW_DIAG_PROBE_NODES).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    probe_nodes: Vec<ProbeNode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    entry_transport_anchored_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct CompressorDecisionSinkDeficit {
    sink_id: String,
    lower_bar: f64,
    p_resolved: f64,
    deficit_bar: f64,
}

#[derive(Debug, Serialize)]
struct CompressorDecisionReportEntry {
    cs_id: String,
    from_node: String,
    to_node: String,
    ratio_before: f64,
    ratio_after: f64,
    p_in_bar: f64,
    downstream_deficits: Vec<CompressorDecisionSinkDeficit>,
    total_slack_before: f64,
    total_slack_after: f64,
}

#[derive(Debug, Serialize)]
struct CompressorHardCouplingEntry {
    cs_id: String,
    from_node: String,
    to_node: String,
    /// Ratio déclaré sur le réseau à l'instant du diagnostic (peut différer du
    /// ratio effectif si l'outer-loop de décision a modifié r sur un clone).
    declared_ratio: f64,
    /// Ratio réalisé P_out / P_in (reflète le r réellement appliqué au solve).
    achieved_ratio: f64,
    p_in_bar: f64,
    p_out_bar: f64,
    /// P_out attendu = declared_ratio · P_in.
    p_out_expected_bar: f64,
    /// P_out − P_out_attendu (vs ratio déclaré).
    coupling_residual_bar: f64,
}

#[derive(Debug, Serialize)]
struct ProbeNode {
    node_id: String,
    pressure_bar: Option<f64>,
    fixed_bar: Option<f64>,
    lower_bar: Option<f64>,
    upper_bar: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    incident_flow_m3s: Option<f64>,
}

fn probe_node_reports(network: &GasNetwork, result: &SolverResult) -> Vec<ProbeNode> {
    let raw = std::env::var("GAZFLOW_DIAG_PROBE_NODES").unwrap_or_default();
    let ids: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if ids.is_empty() {
        return Vec::new();
    }
    ids.iter()
        .filter_map(|id| {
            let node = network.nodes().find(|n| n.id == *id)?;
            // débit net incident (somme signée selon orientation from->to).
            let mut net = 0.0;
            let mut found = false;
            for pipe in network.pipes() {
                if pipe.from == *id {
                    net += result.flows.get(&pipe.id).copied().unwrap_or(0.0);
                    found = true;
                } else if pipe.to == *id {
                    net -= result.flows.get(&pipe.id).copied().unwrap_or(0.0);
                    found = true;
                }
            }
            Some(ProbeNode {
                node_id: id.clone(),
                pressure_bar: result.pressures.get(id).copied(),
                fixed_bar: node.pressure_fixed_bar,
                lower_bar: node.pressure_lower_bar,
                upper_bar: node.pressure_upper_bar,
                incident_flow_m3s: found.then_some(net),
            })
        })
        .collect()
}

fn parse_residual_from_error(err: &str) -> Option<f64> {
    let marker = "residual=";
    let start = err.find(marker)? + marker.len();
    let tail = &err[start..];
    let end = tail.find(|c: char| c == ',' || c.is_whitespace())?;
    tail[..end].trim().parse().ok()
}

fn parse_args() -> CliArgs {
    let mut args = std::env::args().skip(1);
    let mut dataset = "GasLib-582".to_string();
    let mut no_r2_cap = false;
    let mut map_mode = None;
    let mut json_out = None;
    let mut csv_out = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--no-r2-cap" => no_r2_cap = true,
            "--map-mode" => {
                if let Some(mode) = args.next() {
                    map_mode = Some(mode);
                }
            }
            "--json" => {
                if let Some(path) = args.next() {
                    json_out = Some(PathBuf::from(path));
                }
            }
            "--csv" => {
                if let Some(path) = args.next() {
                    csv_out = Some(PathBuf::from(path));
                }
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other if other.starts_with('-') => {
                eprintln!("option inconnue : {other}");
                print_help();
                std::process::exit(2);
            }
            other => dataset = other.to_string(),
        }
    }

    CliArgs {
        dataset,
        no_r2_cap,
        map_mode,
        json_out,
        csv_out,
    }
}

fn print_help() {
    eprintln!(
        "Usage: compressor_diag [DATASET] [OPTIONS]\n\
         \n\
         Diagnostic compresseurs transport (protocole GasLib-582, hors CI).\n\
         DATASET défaut : GasLib-582\n\
         --no-r2-cap           GAZFLOW_DISABLE_R2_CAP=1 (hypothèse H2)\n\
         --map-mode MODE       legacy | measurement | biquadratic (GAZFLOW_COMPRESSOR_MAP_MODE)\n\
         --json PATH           écrit la sortie JSON (sinon stdout)\n\
         --csv PATH            exporte les débits compresseur en CSV"
    );
}

fn resolve_scenario_path(dat_dir: &Path, dataset: &str) -> Option<PathBuf> {
    let mild_name = "nomination_mild_618.scn";
    if let Ok(entries) = fs::read_dir(dat_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.file_name().is_some_and(|n| n == mild_name) {
                return Some(path);
            }
            if path.is_dir() {
                let candidate = path.join(mild_name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    let fallback = dat_dir.join(format!("{dataset}.scn"));
    fallback.is_file().then_some(fallback)
}

fn apply_map_mode_env(mode: &str) -> Result<()> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "legacy" | "measurement" | "biquadratic" => {
            unsafe { std::env::set_var("GAZFLOW_COMPRESSOR_MAP_MODE", mode.trim()) };
            Ok(())
        }
        other => bail!("map-mode invalide: {other} (legacy|measurement|biquadratic)"),
    }
}

fn map_eval_inlet_pressure_bar(pipe: &gazflow_back::graph::Pipe, inlet: Option<f64>) -> f64 {
    let transport = pipe
        .equipment
        .compressor_pressure_cap_ratio
        .unwrap_or(1.0)
        >= 2.0;
    inlet.filter(|p| *p > 1.5).unwrap_or(if transport {
        TRANSPORT_FALLBACK_INLET_BAR
    } else {
        1.01325
    })
}

fn synthetic_result_for_map_preview(
    network: &GasNetwork,
    residual: f64,
) -> SolverResult {
    let mut result = SolverResult::default();
    result.residual = residual;
    for pipe in network.pipes() {
        if pipe.kind != ConnectionKind::CompressorStation || !pipe.hydraulically_active() {
            continue;
        }
        result.pressures.insert(
            pipe.from.clone(),
            map_eval_inlet_pressure_bar(pipe, None),
        );
    }
    result
}

fn map_mode_label(cli: &CliArgs) -> String {
    cli.map_mode.clone().unwrap_or_else(|| {
        std::env::var("GAZFLOW_COMPRESSOR_MAP_MODE").unwrap_or_else(|_| "legacy".into())
    })
}

fn compressor_station_rows(
    network: &GasNetwork,
    result: &SolverResult,
    demands: &std::collections::HashMap<String, f64>,
    demand_scale: f64,
    tolerance: f64,
) -> Vec<CompressorStationDiag> {
    let catalog = network.compressor_catalog.as_ref();
    let prefer_estimated = result.residual > tolerance.max(HANDOFF_PREFER_ESTIMATED_RESIDUAL);
    let mut rows: Vec<CompressorStationDiag> = network
        .pipes()
        .filter(|pipe| {
            pipe.kind == ConnectionKind::CompressorStation && pipe.hydraulically_active()
        })
        .map(|pipe| {
            let ratio_max = pipe.compressor_ratio_max.unwrap_or(1.08);
            let flow_m3s = result.flows.get(&pipe.id).copied().unwrap_or(0.0);
            let solver_q = flow_m3s.abs();
            let estimated_q =
                estimated_compressor_map_flow_m3s(network, pipe, demands, demand_scale);
            let map_eval_q = if prefer_estimated && estimated_q >= FLOW_EVAL_THRESHOLD_M3S {
                estimated_q
            } else if solver_q >= FLOW_EVAL_THRESHOLD_M3S
                && solver_q <= MAX_PLAUSIBLE_COMPRESSOR_FLOW_M3S
            {
                solver_q
            } else if estimated_q >= FLOW_EVAL_THRESHOLD_M3S {
                estimated_q
            } else {
                solver_q
            };
            let inlet_pressure_bar = result.pressures.get(&pipe.from).copied();
            let p_in = map_eval_inlet_pressure_bar(pipe, inlet_pressure_bar);
            let map_target_ratio = catalog.and_then(|cat| {
                let station = cat.station(&pipe.id)?;
                Some(effective_ratio_with_nominal(
                    station,
                    &CompressorOperatingContext {
                        q_m3s_norm: map_eval_q,
                        p_in_bar: p_in,
                        t_in_k: DEFAULT_GAS_TEMPERATURE_K,
                    },
                    pipe.equipment.compressor_nominal_ratio,
                    pipe.equipment.compressor_pressure_cap_ratio,
                ))
            });
            CompressorStationDiag {
                pipe_id: pipe.id.clone(),
                from: pipe.from.clone(),
                to: pipe.to.clone(),
                flow_m3s,
                map_eval_q_m3s: ((map_eval_q - solver_q).abs() > 1e-9).then_some(map_eval_q),
                ratio_max,
                effective_r2: compressor_pressure_from_coeff(pipe),
                map_target_ratio,
                inlet_pressure_bar: Some(p_in),
            }
        })
        .collect();
    rows.sort_by(|a, b| a.pipe_id.cmp(&b.pipe_id));
    rows
}

fn write_csv(path: &Path, stations: &[CompressorStationDiag]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create parent directory for {:?}", path))?;
    }
    let mut lines = vec![
        "pipe_id,from,to,flow_m3s,ratio_max,effective_r2,map_target_ratio,inlet_pressure_bar"
            .to_string(),
    ];
    for s in stations {
        let map = s
            .map_target_ratio
            .map(|v| format!("{v:.6}"))
            .unwrap_or_default();
        let p_in = s
            .inlet_pressure_bar
            .map(|v| format!("{v:.6}"))
            .unwrap_or_default();
        lines.push(format!(
            "{},{},{},{:.10},{:.6},{:.6},{map},{p_in}",
            s.pipe_id, s.from, s.to, s.flow_m3s, s.ratio_max, s.effective_r2
        ));
    }
    fs::write(path, lines.join("\n") + "\n").with_context(|| format!("write CSV {:?}", path))?;
    Ok(())
}

fn emit_json(output: &DiagOutput, path: Option<&Path>) -> Result<()> {
    let json = serde_json::to_string_pretty(output).context("serialize diagnostic JSON")?;
    if let Some(path) = path {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create parent directory for {:?}", path))?;
        }
        fs::write(path, json + "\n").with_context(|| format!("write JSON {:?}", path))?;
        eprintln!("wrote JSON: {}", path.display());
    } else {
        println!("{json}");
    }
    Ok(())
}

fn skipped_output(
    dataset: String,
    network: String,
    scenario: Option<String>,
    flags: DiagFlags,
    reason: String,
) -> DiagOutput {
    DiagOutput {
        status: "skipped",
        dataset,
        network,
        scenario,
        residual: None,
        demand_scale: None,
        iterations: None,
        continuation_scales: Vec::new(),
        flags,
        compressor_stations: Vec::new(),
        compressor_decision_report: None,
        compressor_hard_coupling_report: None,
        reason: Some(reason),
        error: None,
        mass_balance: None,
        mass_balance_refinement_passes: None,
        mass_balance_anchors: Vec::new(),
        contract_flow_relaxed: Vec::new(),
        nomination_mass_balance: None,
        boundary_nomination_slips: Vec::new(),
        scenario_pressure_slips: Vec::new(),
        shortpipe_boundary_couplings: Vec::new(),
        worst_pressure_upstream_trace: Vec::new(),
        boundary_pressure_supply: Vec::new(),
        probe_nodes: Vec::new(),
        entry_transport_anchored_ids: Vec::new(),
    }
}

fn main() -> Result<()> {
    let cli = parse_args();
    let back_root = PathBuf::from(".");
    let dat_dir = back_root.join("dat");
    let network_path = dat_dir.join(format!("{}.net", cli.dataset));

    if cli.no_r2_cap {
        unsafe { std::env::set_var("GAZFLOW_DISABLE_R2_CAP", "1") };
    }
    if let Some(ref mode) = cli.map_mode {
        apply_map_mode_env(mode)?;
    }
    unsafe { std::env::set_var("GAZFLOW_SKIP_CDF_ROUTING", "1") };

    let map_mode = map_mode_label(&cli);
    let scenario_path = resolve_scenario_path(&dat_dir, &cli.dataset);
    let network_display = network_path.display().to_string();
    let scenario_display = scenario_path.as_ref().map(|p| p.display().to_string());

    if !network_path.is_file() {
        let reason = format!(
            "network not found at {network_display} (run ./scripts/fetch_gaslib.sh {})",
            cli.dataset
        );
        eprintln!("skip: {reason}");
        let flags = DiagFlags {
            skip_cdf_routing: true,
            disable_r2_cap: cli.no_r2_cap,
            map_mode,
            catalog_stations: 0,
            preset: "robust",
            compressor_enthalpic: diag_env_enthalpic(),
            compressor_energy_closure: env_flag("GAZFLOW_COMPRESSOR_ENERGY_CLOSURE"),
            compressor_energy_equation: env_flag("GAZFLOW_COMPRESSOR_ENERGY_EQUATION"),
            scenario_pressure_envelopes: scenario_pressure_envelopes_enabled(),
            transport_minimal_anchors: transport_minimal_anchors_enabled(),
            scenario_pressure_in_newton: scenario_pressure_in_newton_enabled(),
            compressor_strict_newton: !compressor_accept_partial_enabled(),
            scenario_shortpipe_coupled_envelopes: shortpipe_coupled_envelopes_enabled(),
            scenario_pressure_floor_anchor: scenario_pressure_floor_anchor_enabled(),
            scenario_boundary_active_envelopes: scenario_boundary_active_envelopes_enabled(),
            scenario_boundary_partial_accept: scenario_boundary_partial_accept_enabled(),
            scenario_shortpipe_merge_boundaries: shortpipe_merge_boundaries_enabled(),
            entry_transport_anchor: entry_transport_anchor_enabled(),
        };
        emit_json(
            &skipped_output(
                cli.dataset.clone(),
                network_display,
                scenario_display,
                flags,
                reason,
            ),
            cli.json_out.as_deref(),
        )?;
        return Ok(());
    }

    let Some(scenario_path) = scenario_path else {
        let reason = format!(
            "scenario not found (expected nomination_mild_618.scn or {}.scn under dat/)",
            cli.dataset
        );
        eprintln!("skip: {reason}");
        let flags = DiagFlags {
            skip_cdf_routing: true,
            disable_r2_cap: cli.no_r2_cap,
            map_mode,
            catalog_stations: 0,
            preset: "robust",
            compressor_enthalpic: diag_env_enthalpic(),
            compressor_energy_closure: env_flag("GAZFLOW_COMPRESSOR_ENERGY_CLOSURE"),
            compressor_energy_equation: env_flag("GAZFLOW_COMPRESSOR_ENERGY_EQUATION"),
            scenario_pressure_envelopes: scenario_pressure_envelopes_enabled(),
            transport_minimal_anchors: transport_minimal_anchors_enabled(),
            scenario_pressure_in_newton: scenario_pressure_in_newton_enabled(),
            compressor_strict_newton: !compressor_accept_partial_enabled(),
            scenario_shortpipe_coupled_envelopes: shortpipe_coupled_envelopes_enabled(),
            scenario_pressure_floor_anchor: scenario_pressure_floor_anchor_enabled(),
            scenario_boundary_active_envelopes: scenario_boundary_active_envelopes_enabled(),
            scenario_boundary_partial_accept: scenario_boundary_partial_accept_enabled(),
            scenario_shortpipe_merge_boundaries: shortpipe_merge_boundaries_enabled(),
            entry_transport_anchor: entry_transport_anchor_enabled(),
        };
        emit_json(
            &skipped_output(cli.dataset.clone(), network_display, None, flags, reason),
            cli.json_out.as_deref(),
        )?;
        return Ok(());
    };

    eprintln!(
        "compressor_diag: dataset={} map_mode={map_mode} network={} scenario={}",
        cli.dataset,
        network_path.display(),
        scenario_path.display()
    );

    let base_network = load_network(&network_path).context("load network")?;
    let catalog_stations = base_network
        .compressor_catalog
        .as_ref()
        .map(|c| c.stations.len())
        .unwrap_or(0);
    let flags = DiagFlags {
        skip_cdf_routing: true,
        disable_r2_cap: cli.no_r2_cap,
        map_mode: map_mode.clone(),
        catalog_stations,
        preset: "robust",
        compressor_enthalpic: diag_env_enthalpic(),
        compressor_energy_closure: env_flag("GAZFLOW_COMPRESSOR_ENERGY_CLOSURE"),
        compressor_energy_equation: env_flag("GAZFLOW_COMPRESSOR_ENERGY_EQUATION"),
        scenario_pressure_envelopes: scenario_pressure_envelopes_enabled(),
        transport_minimal_anchors: transport_minimal_anchors_enabled(),
        scenario_pressure_in_newton: scenario_pressure_in_newton_enabled(),
        compressor_strict_newton: !compressor_accept_partial_enabled(),
        scenario_shortpipe_coupled_envelopes: shortpipe_coupled_envelopes_enabled(),
        scenario_pressure_floor_anchor: scenario_pressure_floor_anchor_enabled(),
        scenario_boundary_active_envelopes: scenario_boundary_active_envelopes_enabled(),
        scenario_boundary_partial_accept: scenario_boundary_partial_accept_enabled(),
        scenario_shortpipe_merge_boundaries: shortpipe_merge_boundaries_enabled(),
        entry_transport_anchor: entry_transport_anchor_enabled(),
    };

    let mut scenario = load_scenario_demands(&scenario_path).context("load scenario")?;
    enrich_scenario_with_balance_hub(&base_network, &mut scenario);

    let preset = preset_robust(base_network.node_count());
    let mut continuation_scales = Vec::new();
    let refinement_outcome = solve_with_mass_balance_refinement(
        &base_network,
        &mut scenario,
        &preset,
        GasComposition::pure_ch4(),
        Some(|ev: ContinuationStepEvent| continuation_scales.push(ev.scale)),
    );
    let (network, solve_result, refinement_passes) = match refinement_outcome {
        Ok(outcome) => (
            outcome.network,
            Ok(outcome.result),
            outcome.refinement_passes,
        ),
        Err(err) => {
            let net = network_with_scenario_boundaries(&base_network, &scenario);
            (net, Err(err), 0)
        }
    };
    let demands = effective_solver_demands_for_network(&network, &scenario.demands, &scenario);
    let mass_balance_anchor_ids: Vec<String> = scenario
        .mass_balance_anchors
        .iter()
        .map(|a| a.node_id.clone())
        .collect();
    let contract_flow_relaxed: Vec<String> = scenario.contract_flow_relaxed.clone();

    let stations = match &solve_result {
        Ok(result) => {
            let mut report_network = network.clone();
            let demand_scale = result.demand_scale_achieved.unwrap_or(1.0);
            let mode = compressor_map_mode();
            if matches!(
                mode,
                CompressorMapMode::Measurement | CompressorMapMode::Biquadratic
            ) && !compressor_decision_variables_enabled()
            {
                apply_map_ratios_after_continuation_step(
                    &mut report_network,
                    &demands,
                    demand_scale,
                    result,
                    mode,
                    preset.tolerance,
                );
            }
            compressor_station_rows(&report_network, result, &demands, demand_scale, preset.tolerance)
        }
        Err(err) => {
            let err_text = format!("{err:#}");
            let residual = parse_residual_from_error(&err_text).unwrap_or(8.0);
            let mut report_network = network.clone();
            let preview = synthetic_result_for_map_preview(&report_network, residual);
            let mode = compressor_map_mode();
            if matches!(
                mode,
                CompressorMapMode::Measurement | CompressorMapMode::Biquadratic
            ) && !compressor_decision_variables_enabled()
            {
                apply_map_ratios_after_continuation_step(
                    &mut report_network,
                    &demands,
                    1.0,
                    &preview,
                    mode,
                    preset.tolerance,
                );
            }
            compressor_station_rows(&report_network, &preview, &demands, 1.0, preset.tolerance)
        }
    };

    let output = match solve_result {
        Ok(result) => {
            let mass_balance = Some(mass_balance_report(&network, &demands, &result));
            let nomination_mass_balance = Some(mass_balance_report(
                &network,
                &scenario.demands,
                &result,
            ));
            let mut excluded: Vec<String> = scenario.contract_flow_relaxed.clone();
            if let Some(slack) = scenario.pressure_slack.as_ref() {
                excluded.push(slack.node_id.clone());
            }
            let boundary_nomination_slips = boundary_nomination_slips(
                &network,
                &scenario.demands,
                &result,
                &excluded,
            );
            let scenario_pressure_slips = scenario_pressure_slips(&network, &result);
            let boundary_pressure_supply =
                boundary_pressure_supply_reports(&network, &result, &scenario_pressure_slips, 12);
            let probe_nodes = probe_node_reports(&network, &result);
            let entry_transport_anchored_ids = entry_transport_anchored_ids(&scenario);
            let shortpipe_boundary_couplings = detect_shortpipe_boundary_pairs(&network);
            let worst_pressure_upstream_trace = scenario_pressure_slips
                .first()
                .map(|slip| {
                    upstream_pressure_trace(&network, &result, &slip.node_id, 6)
                        .into_iter()
                        .map(|(node_id, pressure_bar)| UpstreamPressureHop {
                            node_id,
                            pressure_bar,
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let contract_active = scenario_boundary_active_envelopes_enabled();
            let contract_violated =
                contract_active && result.residual > preset.tolerance;
            let compressor_decision_report = compressor_decision_report(&network, &result);
            let compressor_hard_coupling_report =
                compressor_hard_coupling_report(&network, &result);
            DiagOutput {
                status: if contract_violated {
                    "contract_violation"
                } else {
                    "ok"
                },
                dataset: cli.dataset.clone(),
                network: network_display,
                scenario: Some(scenario_path.display().to_string()),
                residual: Some(result.residual),
                demand_scale: result.demand_scale_achieved,
                iterations: Some(result.iterations),
                continuation_scales,
                flags,
                compressor_stations: stations,
                compressor_decision_report,
                compressor_hard_coupling_report,
                reason: if contract_violated {
                    Some(format!(
                        "residual {:.4e} m³/s exceeds tolerance {:.4e} (contract Q+P)",
                        result.residual, preset.tolerance
                    ))
                } else {
                    None
                },
                error: None,
                mass_balance,
                mass_balance_refinement_passes: Some(refinement_passes),
                mass_balance_anchors: mass_balance_anchor_ids.clone(),
                contract_flow_relaxed: contract_flow_relaxed.clone(),
                nomination_mass_balance,
                boundary_nomination_slips,
                scenario_pressure_slips,
                shortpipe_boundary_couplings,
                worst_pressure_upstream_trace,
                boundary_pressure_supply,
                probe_nodes,
                entry_transport_anchored_ids,
            }
        }
        Err(err) => {
            eprintln!("solve failed: {err:#}");
            let err_text = format!("{err:#}");
            let residual = parse_residual_from_error(&err_text).unwrap_or(8.0);
            let preview = synthetic_result_for_map_preview(&network, residual);
            DiagOutput {
                status: "error",
                dataset: cli.dataset.clone(),
                network: network_display,
                scenario: Some(scenario_path.display().to_string()),
                residual: Some(residual),
                demand_scale: None,
                iterations: None,
                continuation_scales,
                flags,
                compressor_stations: stations,
                compressor_decision_report: compressor_decision_report(&network, &preview),
                compressor_hard_coupling_report: compressor_hard_coupling_report(&network, &preview),
                reason: None,
                error: Some(err_text),
                mass_balance: None,
                mass_balance_refinement_passes: Some(refinement_passes),
                mass_balance_anchors: mass_balance_anchor_ids,
                contract_flow_relaxed,
                nomination_mass_balance: None,
                boundary_nomination_slips: Vec::new(),
                scenario_pressure_slips: Vec::new(),
                shortpipe_boundary_couplings: detect_shortpipe_boundary_pairs(&network),
                worst_pressure_upstream_trace: Vec::new(),
                boundary_pressure_supply: Vec::new(),
                probe_nodes: Vec::new(),
                entry_transport_anchored_ids: Vec::new(),
            }
        }
    };

    if let Some(csv_path) = cli.csv_out.as_deref() {
        write_csv(csv_path, &output.compressor_stations)?;
        eprintln!("wrote CSV: {}", csv_path.display());
    }

    emit_json(&output, cli.json_out.as_deref())?;

    if output.status == "error" || output.status == "contract_violation" {
        std::process::exit(1);
    }
    Ok(())
}
