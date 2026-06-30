//! Diagnostic compresseurs transport (palier I-A0 / I-A, GasLib-582).
//!
//! Protocole figé : réseau baseline connecté, CDF désactivé, slack pression retiré.
//! Hors CI ; utiliser localement après `./scripts/fetch_gaslib.sh GasLib-582`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use gazflow_back::compressor::{CompressorOperatingContext, effective_ratio_with_nominal};
use gazflow_back::gaslib::{
    apply_scenario_boundaries, demands_without_pressure_slack, load_network, load_scenario_demands,
};
use gazflow_back::graph::{ConnectionKind, GasNetwork};
use gazflow_back::solver::{
    ContinuationStepEvent, GasComposition, SolverControl, SolverResult,
    compressor_pressure_from_coeff, preset_robust, solve_steady_state_with_preset,
};
use serde::Serialize;

const DEFAULT_GAS_TEMPERATURE_K: f64 = 288.15;

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
}

#[derive(Debug, Serialize)]
struct CompressorStationDiag {
    pipe_id: String,
    from: String,
    to: String,
    flow_m3s: f64,
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
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
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

fn map_mode_label(cli: &CliArgs) -> String {
    cli.map_mode.clone().unwrap_or_else(|| {
        std::env::var("GAZFLOW_COMPRESSOR_MAP_MODE").unwrap_or_else(|_| "legacy".into())
    })
}

fn compressor_station_rows(
    network: &GasNetwork,
    result: &SolverResult,
) -> Vec<CompressorStationDiag> {
    let catalog = network.compressor_catalog.as_ref();
    let mut rows: Vec<CompressorStationDiag> = network
        .pipes()
        .filter(|pipe| {
            pipe.kind == ConnectionKind::CompressorStation && pipe.hydraulically_active()
        })
        .map(|pipe| {
            let ratio_max = pipe.compressor_ratio_max.unwrap_or(1.08);
            let flow_m3s = result.flows.get(&pipe.id).copied().unwrap_or(0.0);
            let inlet_pressure_bar = result.pressures.get(&pipe.from).copied();
            let map_target_ratio = catalog.and_then(|cat| {
                let station = cat.station(&pipe.id)?;
                let p_in = inlet_pressure_bar.unwrap_or(1.01325).max(1e-3);
                Some(effective_ratio_with_nominal(
                    station,
                    &CompressorOperatingContext {
                        q_m3s_norm: flow_m3s.abs(),
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
                ratio_max,
                effective_r2: compressor_pressure_from_coeff(pipe),
                map_target_ratio,
                inlet_pressure_bar,
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
        reason: Some(reason),
        error: None,
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

    let mut network = load_network(&network_path).context("load network")?;
    let catalog_stations = network
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
    };

    let scenario = load_scenario_demands(&scenario_path).context("load scenario")?;
    apply_scenario_boundaries(&mut network, &scenario);
    let demands = demands_without_pressure_slack(&scenario.demands, &scenario);

    let preset = preset_robust(network.node_count());
    let mut continuation_scales = Vec::new();
    let solve_result = solve_steady_state_with_preset(
        &network,
        &demands,
        None,
        &preset,
        GasComposition::pure_ch4(),
        |_| SolverControl::Continue,
        Some(|ev: ContinuationStepEvent| continuation_scales.push(ev.scale)),
    );

    let stations = match &solve_result {
        Ok(result) => compressor_station_rows(&network, result),
        Err(_) => compressor_station_rows(&network, &SolverResult::default()),
    };

    let output = match solve_result {
        Ok(result) => DiagOutput {
            status: "ok",
            dataset: cli.dataset.clone(),
            network: network_display,
            scenario: Some(scenario_path.display().to_string()),
            residual: Some(result.residual),
            demand_scale: result.demand_scale_achieved,
            iterations: Some(result.iterations),
            continuation_scales,
            flags,
            compressor_stations: stations,
            reason: None,
            error: None,
        },
        Err(err) => {
            eprintln!("solve failed: {err:#}");
            let err_text = format!("{err:#}");
            DiagOutput {
                status: "error",
                dataset: cli.dataset.clone(),
                network: network_display,
                scenario: Some(scenario_path.display().to_string()),
                residual: parse_residual_from_error(&err_text),
                demand_scale: None,
                iterations: None,
                continuation_scales,
                flags,
                compressor_stations: stations,
                reason: None,
                error: Some(err_text),
            }
        }
    };

    if let Some(csv_path) = cli.csv_out.as_deref() {
        write_csv(csv_path, &output.compressor_stations)?;
        eprintln!("wrote CSV: {}", csv_path.display());
    }

    emit_json(&output, cli.json_out.as_deref())?;

    if output.status == "error" {
        std::process::exit(1);
    }
    Ok(())
}
