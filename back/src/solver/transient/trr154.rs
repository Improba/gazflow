//! Lecteur minimal des fixtures TRR154 (GasLib-11 sinus) pour validation PDE.
//!
//! Formats XML TRR154 (`https://www.trr154.fau.de/transient-data`) :
//! - `.state` : pressions et massflows initiaux [bar], [kg/s]
//! - `.bcd` : séries temporelles de massflow aux boundary nodes
//!
//! Conversion vers GazFlow : $\dot V_n = \dot m / \rho$ [Nm³/s].
//! Préférer $\rho_*$ calée sur le `.scn` ([`rho_ref_calibrated_to_scn`]) plutôt que
//! $\rho_n^{\mathrm{G20}}$ brut (écart volumique ~1,25× sur GasLib-11).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::solver::gas_properties::{
    GasComposition, STANDARD_PRESSURE_BAR, STANDARD_TEMPERATURE_K,
};

use super::events::TransientEvent;
use super::mesh::PipeMesh;
use super::state::TransientPipeState;

/// Échantillon spatial TRR154 le long d'une arête (conduite).
#[derive(Debug, Clone)]
pub struct Trr154EdgeSample {
    pub space_m: f64,
    pub massflow_kg_s: f64,
    pub pressure_bar: f64,
}

/// État nodal TRR154.
#[derive(Debug, Clone)]
pub struct Trr154NodeState {
    pub pressure_bar: f64,
    /// Massflow [kg/s] (positif = injection, négatif = prélèvement).
    pub massflow_kg_s: f64,
}

#[derive(Debug, Clone)]
pub struct Trr154InitialState {
    pub network_name: String,
    pub speed_of_sound_m_s: f64,
    pub nodes: HashMap<String, Trr154NodeState>,
    /// Profils spatiaux le long des conduites (`type="pipe"` uniquement).
    pub edges: HashMap<String, Vec<Trr154EdgeSample>>,
}

#[derive(Debug, Clone)]
pub struct Trr154BoundarySeries {
    pub network_name: String,
    pub speed_of_sound_m_s: f64,
    pub horizon_s: f64,
    /// node_id → samples (t_s, massflow_kg_s)
    pub series: HashMap<String, Vec<(f64, f64)>>,
}

pub fn trr154_gaslib11_corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/testing/corpus/external/transient/gaslib-11")
}

pub fn trr154_gaslib11_paths() -> Option<(PathBuf, PathBuf)> {
    let dir = trr154_gaslib11_corpus_dir();
    let state = dir.join("GasLib-11-sinus_5000_60-initial.state");
    let bcd = dir.join("GasLib-11-sinus-InputData.bcd");
    if state.is_file() && bcd.is_file() {
        Some((state, bcd))
    } else {
        None
    }
}

/// ρ_n [kg/Nm³] GazFlow (15 °C / 1,01325 bar, composition courante).
///
/// Attention TRR154 : convertir `ṁ/ρ_n` avec cette densité donne ~1,25× le $Q$ du `.scn`
/// GasLib-11 (ρ implicite TRR154 ≈ 1,02 kg/Nm³). Préférer
/// [`rho_ref_calibrated_to_scn`] pour des BC volumiques alignées sur le scénario.
pub fn standard_density_kg_per_nm3(composition: &GasComposition) -> f64 {
    composition.density_kg_per_m3(STANDARD_PRESSURE_BAR, STANDARD_TEMPERATURE_K)
}

/// $Q_n = \dot m / \rho$ [Nm³/s]. Signe conservé (TRR154 : exit &lt; 0).
pub fn massflow_to_nm3s(massflow_kg_s: f64, rho_kg_per_nm3: f64) -> f64 {
    massflow_kg_s / rho_kg_per_nm3.max(1e-9)
}

/// Densité de référence pour convertir les massflows TRR154 en $Q_n$ GazFlow.
///
/// Calée pour que `exit01` reproduise le débit volumique du `.scn` GasLib :
/// $\rho_* = |\dot m_{\mathrm{exit01}}^{\mathrm{TRR}}| / |Q_{\mathrm{exit01}}^{\mathrm{scn}}|$.
/// Les ratios relatifs entre exits du `.state` / BCD sont alors conservés.
pub fn rho_ref_calibrated_to_scn(
    state: &Trr154InitialState,
    scn_demands: &HashMap<String, f64>,
    pivot_exit: &str,
) -> Result<f64> {
    let m = state
        .nodes
        .get(pivot_exit)
        .map(|n| n.massflow_kg_s.abs())
        .filter(|m| *m > 1e-9)
        .with_context(|| format!("TRR154 state missing massflow for {pivot_exit}"))?;
    let q = scn_demands
        .get(pivot_exit)
        .map(|q| q.abs())
        .filter(|q| *q > 1e-9)
        .with_context(|| format!("scn missing demand for {pivot_exit}"))?;
    Ok(m / q)
}

/// Demandes [Nm³/s] : massflows TRR154 / `rho_ref`.
/// Entries → 0 (débit libre sous P fixe) ; exits / innodes → $Q_n$.
pub fn demands_from_trr154_state(
    state: &Trr154InitialState,
    rho_ref: f64,
) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for (id, n) in &state.nodes {
        if id.starts_with("entry") {
            out.insert(id.clone(), 0.0);
        } else {
            out.insert(id.clone(), massflow_to_nm3s(n.massflow_kg_s, rho_ref));
        }
    }
    out
}

pub fn parse_trr154_initial_state(xml: &str) -> Result<Trr154InitialState> {
    let network_name = attr_after(xml, "<network>", "</network>")
        .unwrap_or_else(|| "unknown".to_string());
    let speed_of_sound_m_s = attr_value(xml, "speedOfSound", "value")
        .and_then(|s| s.parse().ok())
        .unwrap_or(340.0);

    let mut nodes = HashMap::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<node ") {
        let chunk = &rest[start..];
        let end = chunk
            .find("</node>")
            .map(|i| i + "</node>".len())
            .or_else(|| chunk.find("/>").map(|i| i + 2))
            .unwrap_or(chunk.len());
        let node_xml = &chunk[..end];
        if let Some(id) = attr_value(node_xml, "node", "id") {
            let pressure_bar = attr_value(node_xml, "pressure", "value")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let massflow_kg_s = attr_value(node_xml, "massflow", "value")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            nodes.insert(
                id,
                Trr154NodeState {
                    pressure_bar,
                    massflow_kg_s,
                },
            );
        }
        rest = &chunk[end.min(chunk.len()).max(1)..];
    }
    if nodes.is_empty() {
        bail!("TRR154 initial.state: no <node> parsed");
    }

    let mut edges = HashMap::new();
    let mut rest_edges = xml;
    while let Some(start) = rest_edges.find("<edge ") {
        let chunk = &rest_edges[start..];
        let end = chunk
            .find("</edge>")
            .map(|i| i + "</edge>".len())
            .or_else(|| chunk.find("/>").map(|i| i + 2))
            .unwrap_or(chunk.len());
        let edge_xml = &chunk[..end];
        let edge_type = attr_value(edge_xml, "edge", "type").unwrap_or_default();
        if edge_type == "pipe"
            && let Some(id) = attr_value(edge_xml, "edge", "id")
        {
            let mut samples = Vec::new();
            let mut inner = edge_xml;
            while let Some(ed) = inner.find("<edgedata>") {
                let ed_chunk = &inner[ed..];
                let ed_end = ed_chunk
                    .find("</edgedata>")
                    .map(|i| i + "</edgedata>".len())
                    .unwrap_or(ed_chunk.len());
                let block = &ed_chunk[..ed_end];
                let space_m = attr_value(block, "space", "value")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                let massflow_kg_s = attr_value(block, "massflow", "value")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                let pressure_bar = attr_value(block, "pressure", "value")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                samples.push(Trr154EdgeSample {
                    space_m,
                    massflow_kg_s,
                    pressure_bar,
                });
                inner = &ed_chunk[ed_end.min(ed_chunk.len()).max(1)..];
            }
            if !samples.is_empty() {
                edges.insert(id, samples);
            }
        }
        rest_edges = &chunk[end.min(chunk.len()).max(1)..];
    }

    Ok(Trr154InitialState {
        network_name,
        speed_of_sound_m_s,
        nodes,
        edges,
    })
}

pub fn load_trr154_initial_state(path: impl AsRef<Path>) -> Result<Trr154InitialState> {
    let xml = fs::read_to_string(path.as_ref())
        .with_context(|| format!("read {}", path.as_ref().display()))?;
    parse_trr154_initial_state(&xml)
}

/// Parse BCD ; conserve un échantillon tous les `stride` points (1 = tout garder).
pub fn parse_trr154_bcd(xml: &str, stride: usize) -> Result<Trr154BoundarySeries> {
    let stride = stride.max(1);
    let network_name = attr_after(xml, "<network>", "</network>")
        .unwrap_or_else(|| "unknown".to_string());
    let speed_of_sound_m_s = attr_value(xml, "speedOfSound", "value")
        .and_then(|s| s.parse().ok())
        .unwrap_or(340.0);
    let horizon_s = attr_value(xml, "timeInterval", "end")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    let mut series: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<node ") {
        let chunk = &rest[start..];
        let end = chunk
            .find("</node>")
            .map(|i| i + "</node>".len())
            .unwrap_or(chunk.len());
        let node_xml = &chunk[..end];
        let Some(id) = attr_value(node_xml, "node", "id") else {
            rest = &chunk[end.min(chunk.len()).max(1)..];
            continue;
        };
        let mut samples = Vec::new();
        let mut inner = node_xml;
        let mut idx = 0usize;
        while let Some(nd) = inner.find("<nodedata>") {
            let nd_chunk = &inner[nd..];
            let nd_end = nd_chunk
                .find("</nodedata>")
                .map(|i| i + "</nodedata>".len())
                .unwrap_or(nd_chunk.len());
            let block = &nd_chunk[..nd_end];
            if idx % stride == 0 {
                let t = attr_value(block, "time", "value").and_then(|s| s.parse().ok());
                let m = attr_value(block, "massflow", "value").and_then(|s| s.parse().ok());
                if let (Some(t), Some(m)) = (t, m) {
                    samples.push((t, m));
                }
            }
            idx += 1;
            inner = &nd_chunk[nd_end.min(nd_chunk.len()).max(1)..];
        }
        if !samples.is_empty() {
            series.insert(id, samples);
        }
        rest = &chunk[end.min(chunk.len()).max(1)..];
    }
    if series.is_empty() {
        bail!("TRR154 bcd: no boundary series parsed");
    }
    Ok(Trr154BoundarySeries {
        network_name,
        speed_of_sound_m_s,
        horizon_s,
        series,
    })
}

pub fn load_trr154_bcd(path: impl AsRef<Path>, stride: usize) -> Result<Trr154BoundarySeries> {
    let xml = fs::read_to_string(path.as_ref())
        .with_context(|| format!("read {}", path.as_ref().display()))?;
    parse_trr154_bcd(&xml, stride)
}

/// Demandes [Nm³/s] : massflows TRR154 convertis (y compris entries).
/// Préférer [`demands_from_trr154_state`] quand les entries sont ancrées en pression.
pub fn demands_from_state(state: &Trr154InitialState, rho_n: f64) -> HashMap<String, f64> {
    state
        .nodes
        .iter()
        .map(|(id, n)| (id.clone(), massflow_to_nm3s(n.massflow_kg_s, rho_n)))
        .collect()
}

/// Événements `DemandChange` pour les exits (massflow &lt; 0) à partir du BCD échantillonné.
pub fn demand_events_from_bcd(
    bcd: &Trr154BoundarySeries,
    rho_n: f64,
    max_events_per_node: usize,
) -> Vec<TransientEvent> {
    let mut events = Vec::new();
    for (node_id, samples) in &bcd.series {
        let mut count = 0usize;
        for &(t, m) in samples {
            if m >= -1e-12 {
                continue; // entries / injections : gérées via P fixe
            }
            events.push(TransientEvent::DemandChange {
                time_s: t,
                node_id: node_id.clone(),
                demand_m3s: massflow_to_nm3s(m, rho_n),
            });
            count += 1;
            if count >= max_events_per_node {
                break;
            }
        }
    }
    events.sort_by(|a, b| {
        a.time_s()
            .partial_cmp(&b.time_s())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    events
}

/// Interpolation linéaire 1D (xs croissants).
fn linear_interp_1d(x: f64, xs: &[f64], ys: &[f64]) -> f64 {
    if xs.is_empty() || ys.is_empty() {
        return 0.0;
    }
    if xs.len() == 1 {
        return ys[0];
    }
    if x <= xs[0] {
        return ys[0];
    }
    let last = xs.len() - 1;
    if x >= xs[last] {
        return ys[last];
    }
    for i in 0..last {
        let x0 = xs[i];
        let x1 = xs[i + 1];
        if x >= x0 && x <= x1 {
            let t = (x - x0) / (x1 - x0).max(1e-12);
            return ys[i] + t * (ys[i + 1] - ys[i]);
        }
    }
    ys[last]
}

/// Construit un [`TransientPipeState`] à partir du profil spatial TRR154 d'une conduite.
///
/// - P aux centres de cellule : interpolation linéaire sur `space_m`.
/// - Débits aux interfaces : `ṁ/ρ_*` ; si `ṁ` quasi constant le long de la conduite,
///   on utilise la moyenne (signe TRR154 = sens de l'arête, aligné GazFlow from→to).
pub fn pipe_state_from_trr154_edge(
    mesh: &PipeMesh,
    samples: &[Trr154EdgeSample],
    rho_ref: f64,
) -> TransientPipeState {
    let n = mesh.n_cells;
    let mut sorted: Vec<Trr154EdgeSample> = samples.to_vec();
    sorted.sort_by(|a, b| {
        a.space_m
            .partial_cmp(&b.space_m)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let spaces: Vec<f64> = sorted.iter().map(|s| s.space_m).collect();
    let pressures: Vec<f64> = sorted.iter().map(|s| s.pressure_bar).collect();
    let massflows: Vec<f64> = sorted.iter().map(|s| s.massflow_kg_s).collect();

    let m_mean = massflows.iter().sum::<f64>() / massflows.len().max(1) as f64;
    let m_spread = massflows
        .iter()
        .fold(0.0_f64, |acc, &m| acc.max((m - m_mean).abs()));
    let m_const = m_spread < 1e-6 * m_mean.abs().max(1e-9);

    let mut p_cells = Vec::with_capacity(n);
    for i in 0..n {
        let x = (i as f64 + 0.5) * mesh.dx;
        p_cells.push(linear_interp_1d(x, &spaces, &pressures));
    }

    let mut flows = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let x = i as f64 * mesh.dx;
        let m = if m_const {
            m_mean
        } else {
            linear_interp_1d(x, &spaces, &massflows)
        };
        flows.push(massflow_to_nm3s(m, rho_ref));
    }

    TransientPipeState {
        pressures: p_cells,
        flows,
    }
}

/// CI spatiales par conduite à partir des `edges` TRR154 et du maillage GazFlow.
pub fn spatial_pipe_states_from_trr154(
    network: &crate::graph::GasNetwork,
    state: &Trr154InitialState,
    rho_ref: f64,
    n_cells: Option<usize>,
) -> HashMap<String, TransientPipeState> {
    let mut out = HashMap::new();
    for pipe in network.pipes().filter(|p| p.hydraulically_active()) {
        if let Some(samples) = state.edges.get(&pipe.id) {
            let mesh = PipeMesh::from_pipe(pipe, n_cells);
            out.insert(
                pipe.id.clone(),
                pipe_state_from_trr154_edge(&mesh, samples, rho_ref),
            );
        }
    }
    out
}

fn attr_value(xml: &str, tag_hint: &str, attr: &str) -> Option<String> {
    // Cherche `attr="..."` près d'une balise contenant tag_hint, ou globalement.
    let needle = format!("{attr}=\"");
    let search_from = if tag_hint.is_empty() {
        0
    } else {
        xml.find(tag_hint).unwrap_or(0)
    };
    let window = &xml[search_from..];
    let start = window.find(&needle)? + needle.len();
    let rest = &window[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn attr_after(xml: &str, open: &str, close: &str) -> Option<String> {
    let start = xml.find(open)? + open.len();
    let rest = &xml[start..];
    let end = rest.find(close)?;
    Some(rest[..end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gaslib::load_network;
    use crate::solver::transient::{
        TransientConfig, TransientMode, TransientResult, simulate_transient_with_mode,
    };
    use std::path::Path;

    fn assert_pde_smoke_band(pde: &TransientResult, label: &str) -> (f64, f64) {
        assert!(
            !pde.limitation.to_lowercase().contains("fallback"),
            "{label}: unexpected fallback: {}",
            pde.limitation
        );
        let mut p_min = f64::INFINITY;
        let mut p_max = f64::NEG_INFINITY;
        for step in &pde.steps {
            assert!(
                step.residual.is_finite(),
                "{label} t={:.0}: residual not finite",
                step.time_s
            );
            assert!(
                step.linepack_kg.is_finite(),
                "{label} t={:.0}: linepack not finite",
                step.time_s
            );
            for (nid, &p) in &step.pressures {
                assert!(
                    p.is_finite() && (15.0..=120.0).contains(&p),
                    "{label} t={:.0} {nid}={p} out of band [15,120]",
                    step.time_s
                );
                p_min = p_min.min(p);
                p_max = p_max.max(p);
            }
        }
        (p_min, p_max)
    }

    fn trr154_gaslib11_pde_fixture(
    ) -> Option<(crate::graph::GasNetwork, HashMap<String, f64>, HashMap<String, f64>, HashMap<String, TransientPipeState>, Trr154InitialState, Trr154BoundarySeries, f64)> {
        let (state_path, bcd_path) = trr154_gaslib11_paths()?;
        let net_path = Path::new("dat/GasLib-11.net");
        if !net_path.is_file() {
            return None;
        }
        let state = load_trr154_initial_state(&state_path).ok()?;
        let bcd = load_trr154_bcd(&bcd_path, 60).ok()?;
        let scn = crate::gaslib::load_scenario_demands(Path::new("dat/GasLib-11.scn")).ok()?;
        let rho_star = rho_ref_calibrated_to_scn(&state, &scn.demands, "exit01").ok()?;

        let mut net = load_network(net_path).ok()?;
        for (id, n) in &state.nodes {
            if id.starts_with("entry")
                && let Some(node) = net.node_mut(id)
            {
                node.pressure_fixed_bar = Some(n.pressure_bar);
            }
        }

        let demands = demands_from_trr154_state(&state, rho_star);
        let warm: HashMap<String, f64> = state
            .nodes
            .iter()
            .map(|(id, n)| (id.clone(), n.pressure_bar))
            .collect();
        let pipe_states = spatial_pipe_states_from_trr154(&net, &state, rho_star, Some(4));
        Some((net, demands, warm, pipe_states, state, bcd, rho_star))
    }

    #[test]
    fn test_parse_trr154_gaslib11_initial_state() {
        let Some((state_path, _)) = trr154_gaslib11_paths() else {
            eprintln!("skip: TRR154 GasLib-11 corpus absent");
            return;
        };
        let state = load_trr154_initial_state(&state_path).expect("parse state");
        assert!(
            state.network_name.contains("GasLib-11"),
            "network={}",
            state.network_name
        );
        assert!((state.speed_of_sound_m_s - 340.0).abs() < 1e-6);
        assert!(
            state.nodes.len() >= 11,
            "expected ≥11 nodes, got {}",
            state.nodes.len()
        );
        let entry = state.nodes.get("entry01").expect("entry01");
        assert!((entry.pressure_bar - 53.0).abs() < 1e-6);
        assert!(entry.massflow_kg_s > 0.0);
        let exit = state.nodes.get("exit01").expect("exit01");
        assert!(exit.massflow_kg_s < 0.0);
        let sum_m: f64 = state.nodes.values().map(|n| n.massflow_kg_s).sum();
        assert!(
            sum_m.abs() < 1e-6,
            "TRR154 state massflows should balance, sum={sum_m}"
        );
        assert_eq!(
            state.edges.len(),
            8,
            "expected 8 pipe edges, got {}",
            state.edges.len()
        );
        for (id, samples) in &state.edges {
            assert_eq!(
                samples.len(),
                12,
                "edge {id}: expected 12 samples"
            );
        }
        let pipe = state
            .edges
            .get("pipe01_entry01_entry03")
            .expect("pipe01");
        assert!((pipe[0].space_m - 0.0).abs() < 1e-6);
        assert!((pipe[11].space_m - 55_000.0).abs() < 1.0);
        assert!(pipe[0].massflow_kg_s > 0.0);
        for s in pipe {
            assert!(
                (s.massflow_kg_s - pipe[0].massflow_kg_s).abs() < 1e-6,
                "massflow should be constant along pipe01"
            );
        }
    }

    #[test]
    fn test_parse_trr154_bcd_samples() {
        let Some((_, bcd_path)) = trr154_gaslib11_paths() else {
            eprintln!("skip: TRR154 GasLib-11 corpus absent");
            return;
        };
        // stride 60 → ~1 point / heure si Δt=60 s
        let bcd = load_trr154_bcd(&bcd_path, 60).expect("parse bcd");
        assert!(bcd.horizon_s >= 86_400.0 - 1.0);
        let exit = bcd.series.get("exit01").expect("exit01 series");
        assert!(
            exit.len() >= 20,
            "expected subsampled series, got {}",
            exit.len()
        );
        assert!((exit[0].0 - 0.0).abs() < 1e-9);
        assert!(exit[0].1 < 0.0);
    }

    /// Smoke corpus TRR154 (GasLib-11) — **pas** une validation de trajectoire.
    ///
    /// Vérifie uniquement :
    /// 1. parse `.state` / `.bcd` + métadonnée `speedOfSound≈340`
    /// 2. conversion `ṁ → Q_n` avec `ρ_n` GazFlow (écart ~1,25× vs scn documenté)
    /// 3. éligibilité PDE après ancrage P entries TRR154
    /// 4. steady solvable (demandes **scn**, P entries TRR154) — pas de match P vs `.state`
    /// 5. un pas PDE sans explosion numérique
    ///
    /// Voir aussi `test_trr154_gaslib11_consistent_bc_and_bcd` (ρ_* + BCD).
    #[test]
    fn test_trr154_gaslib11_pde_smoke_validation() {
        use crate::solver::steady_state::solve_steady_state;

        let Some((state_path, bcd_path)) = trr154_gaslib11_paths() else {
            eprintln!("skip: TRR154 GasLib-11 corpus absent");
            return;
        };
        let net_path = Path::new("dat/GasLib-11.net");
        if !net_path.is_file() {
            eprintln!("skip: dat/GasLib-11.net absent");
            return;
        }

        let state = load_trr154_initial_state(&state_path).expect("state");
        let bcd = load_trr154_bcd(&bcd_path, 120).expect("bcd");
        assert!(!bcd.series.is_empty(), "BCD series must be non-empty");
        assert!((state.speed_of_sound_m_s - 340.0).abs() < 1.0);

        let mut net = load_network(net_path).expect("GasLib-11");
        for (id, n) in &state.nodes {
            if id.starts_with("entry")
                && let Some(node) = net.node_mut(id)
            {
                node.pressure_fixed_bar = Some(n.pressure_bar);
            }
        }

        let composition = GasComposition::default();
        let rho_n = standard_density_kg_per_nm3(&composition);
        let scn = crate::gaslib::load_scenario_demands(Path::new("dat/GasLib-11.scn"))
            .expect("GasLib-11.scn");
        let q_scn = scn.demands.get("exit01").copied().unwrap_or(0.0).abs();
        let m_trr = state.nodes["exit01"].massflow_kg_s.abs();
        let q_trr = massflow_to_nm3s(state.nodes["exit01"].massflow_kg_s, rho_n).abs();
        // ρ implicite TRR154 si on imposait Q_scn : m/Q_scn ≈ 1.02 kg/Nm³ vs ρ_n G20 ≈ 0.69
        let rho_implied = m_trr / q_scn.max(1e-9);
        let ratio = q_trr / q_scn.max(1e-9);
        eprintln!(
            "TRR154 units: |m|={m_trr:.3} kg/s, Q_n(GazFlow ρ_n={rho_n:.3})={q_trr:.2}, \
             Q_scn={q_scn:.2}, ρ_implied={rho_implied:.3}, ratio Q_trr/Q_scn={ratio:.2}"
        );
        assert!(
            (1.2..=1.8).contains(&ratio),
            "expected ~1.25× offset GazFlow ρ_n vs TRR154/scn volumetric: ratio={ratio}"
        );

        let mut demands = scn.demands.clone();
        for id in state.nodes.keys() {
            if id.starts_with("entry") {
                demands.insert(id.clone(), 0.0);
            }
        }

        assert!(super::super::is_pde_eligible(&net));

        let steady = solve_steady_state(&net, &demands, 2000, 1e-3)
            .expect("TRR154 steady smoke");
        assert!(steady.residual.is_finite() && steady.residual <= 1e-3);

        // Entries must stay at TRR154 anchors (Dirichlet).
        for (id, n) in &state.nodes {
            if !id.starts_with("entry") {
                continue;
            }
            let p = steady.pressures.get(id).copied().unwrap_or(0.0);
            assert!(
                (p - n.pressure_bar).abs() < 0.5,
                "entry {id} should stay at TRR154 P: got {p}, want {}",
                n.pressure_bar
            );
        }

        // Explicit non-claim: exit pressure need not match TRR154 IC (different Q BCs).
        let p_exit_state = state.nodes["exit01"].pressure_bar;
        let p_exit_sol = steady.pressures.get("exit01").copied().unwrap_or(0.0);
        eprintln!(
            "TRR154 steady: exit01 P_sol={p_exit_sol:.1} bar vs P_state={p_exit_state:.1} \
             (no match required; scn Q ≠ TRR154 ṁ/ρ_n)"
        );
        assert!(
            (15.0..=120.0).contains(&p_exit_sol),
            "exit01 pressure out of operational band: {p_exit_sol}"
        );

        let cfg_pde = TransientConfig {
            duration_s: 300.0,
            dt_s: 300.0,
            gas_composition: composition,
            n_cells_per_pipe: Some(4),
            adaptive_dt: false,
            picard_relax: None,
        };
        let pde = simulate_transient_with_mode(
            &net,
            &demands,
            &[],
            &cfg_pde,
            TransientMode::Pde,
            None,
            None,
        )
        .expect("TRR154 PDE one-step");
        assert!(!pde.limitation.to_lowercase().contains("fallback"));
        let last = pde.steps.last().expect("pde steps");
        // Un pas depuis le steady : résidu Picard faible attendu, mais pas « validation dynamique ».
        assert!(
            last.converged || last.residual < 1.0,
            "PDE one-step degraded: converged={}, residual={}",
            last.converged,
            last.residual
        );
        for (nid, &p) in &last.pressures {
            assert!(
                p.is_finite() && p > 1.0 && p < 200.0,
                "PDE one-step pressure exploded: {nid}={p}"
            );
        }
        eprintln!(
            "TRR154 PDE one-step smoke: converged={}, residual={:.3e} (not a trajectory check)",
            last.converged, last.residual
        );
    }

    /// Cran scientifique : BC TRR154 volumiques cohérentes via $\rho_*$ + BCD.
    ///
    /// 1. $\rho_* = |m_{\mathrm{exit01}}| / |Q_{\mathrm{scn,exit01}}|$ (≠ `normDensity` GasLib 0,785
    ///    et ≠ $\rho_n^{\mathrm{G20}}$ ≈ 0,82 ; le corpus TRR154 implique ≈ 1,02)
    /// 2. $Q_n = m / \rho_*$ reproduit les débits `.scn` sur les exits
    /// 3. steady avec P entries TRR154 : **ancrages OK**, mais écart P vs `.state`
    ///    reste large (~30–40 %) car modèle P²/compresseurs ≠ Euler isotherme TRR154
    /// 4. snapshots BCD (0 / 1 / 2 h, même $\rho_*$) : pressions dans bande opérationnelle
    ///
    /// Ce n'est **pas** une validation de trajectoire / oracle `.sol`.
    #[test]
    fn test_trr154_gaslib11_consistent_bc_and_bcd() {
        use crate::solver::steady_state::solve_steady_state_with_initial_pressures;

        let Some((state_path, bcd_path)) = trr154_gaslib11_paths() else {
            eprintln!("skip: TRR154 GasLib-11 corpus absent");
            return;
        };
        let net_path = Path::new("dat/GasLib-11.net");
        if !net_path.is_file() {
            eprintln!("skip: dat/GasLib-11.net absent");
            return;
        }

        let state = load_trr154_initial_state(&state_path).expect("state");
        let scn = crate::gaslib::load_scenario_demands(Path::new("dat/GasLib-11.scn"))
            .expect("GasLib-11.scn");
        let composition = GasComposition::default();
        let rho_g20 = standard_density_kg_per_nm3(&composition);
        let rho_star = rho_ref_calibrated_to_scn(&state, &scn.demands, "exit01")
            .expect("rho*");
        assert!(
            (0.95..=1.10).contains(&rho_star),
            "unexpected ρ_* from TRR154/scn: {rho_star}"
        );
        // Écart documenté vs densités « naïves ».
        assert!(
            (rho_star / rho_g20 - 1.0).abs() > 0.15,
            "ρ_* should differ from GazFlow ρ_n: ρ_*={rho_star}, ρ_n={rho_g20}"
        );

        let mut net = load_network(net_path).expect("GasLib-11");
        for (id, n) in &state.nodes {
            if id.starts_with("entry")
                && let Some(node) = net.node_mut(id)
            {
                node.pressure_fixed_bar = Some(n.pressure_bar);
            }
        }

        let demands = demands_from_trr154_state(&state, rho_star);
        for exit in ["exit01", "exit02", "exit03"] {
            let q = demands[exit].abs();
            let q_scn = scn.demands[exit].abs();
            let rel = (q - q_scn).abs() / q_scn.max(1e-9);
            assert!(
                rel < 1e-6,
                "{exit}: ρ_* must recover scn Q (rel={rel}): got {q}, scn {q_scn}"
            );
        }
        // Contrôle négatif : ρ_n GazFlow gonfle les Q (~1,25×).
        let q_naive = massflow_to_nm3s(state.nodes["exit01"].massflow_kg_s, rho_g20).abs();
        assert!(
            (1.15..=1.40).contains(&(q_naive / scn.demands["exit01"].abs())),
            "naive ρ_n conversion should stay ~1.25× scn"
        );

        let warm: HashMap<String, f64> = state
            .nodes
            .iter()
            .map(|(id, n)| (id.clone(), n.pressure_bar))
            .collect();
        let steady = solve_steady_state_with_initial_pressures(
            &net,
            &demands,
            Some(&warm),
            3000,
            1e-3,
        )
        .expect("TRR154 consistent-BC steady");
        assert!(steady.residual.is_finite() && steady.residual <= 1e-3);

        for (id, n) in &state.nodes {
            if !id.starts_with("entry") {
                continue;
            }
            let p = steady.pressures[id];
            assert!(
                (p - n.pressure_bar).abs() < 0.25,
                "entry {id}: P={p}, want {}",
                n.pressure_bar
            );
        }

        let mut max_rel = 0.0_f64;
        let mut worst = String::new();
        for (id, n) in &state.nodes {
            if id.starts_with("entry") {
                continue;
            }
            let p = steady.pressures.get(id).copied().unwrap_or(f64::NAN);
            let rel = (p - n.pressure_bar).abs() / n.pressure_bar.max(1.0);
            eprintln!(
                "TRR154 P gap {id}: sol={p:.2} state={:.2} rel={:.1}%",
                n.pressure_bar,
                100.0 * rel
            );
            if rel > max_rel {
                max_rel = rel;
                worst = id.clone();
            }
            assert!(
                p.is_finite() && (15.0..=120.0).contains(&p),
                "{id} pressure out of band: {p}"
            );
        }
        // Garde-fou : écart modèle connu (aujourd'hui ~50 % max). Pas de borne basse
        // (une amélioration future ne doit pas faire échouer le test).
        assert!(
            max_rel < 0.55,
            "model gap vs TRR154 .state too large: {:.1}% at {worst} (limit 55 %)",
            100.0 * max_rel
        );
        assert!(
            max_rel > 0.05,
            "unexpected near-match vs TRR154 .state ({:.1}% at {worst}): check BC / units",
            100.0 * max_rel
        );
        eprintln!(
            "TRR154 consistent BC: ρ_*={rho_star:.4} (ρ_n G20={rho_g20:.4}), \
             max P gap={:.1}% at {worst} (model mismatch, not unit error)",
            100.0 * max_rel
        );

        let bcd = load_trr154_bcd(&bcd_path, 60).expect("bcd");
        let events = demand_events_from_bcd(&bcd, rho_star, 4);
        assert!(!events.is_empty(), "expected BCD demand events");
        // Snapshots BCD (évite le QS MVP dont la tolérance Newton fixe 1e-6 peut
        // échouer sur un pas alors que residual≈5e-6). Même ρ_*, bande physique.
        let mut demands_t = demands.clone();
        let sample_times = [0.0_f64, 3600.0, 7200.0];
        for &t_target in &sample_times {
            for (node_id, samples) in &bcd.series {
                if !node_id.starts_with("exit") {
                    continue;
                }
                let Some((_, m)) = samples
                    .iter()
                    .min_by(|a, b| (a.0 - t_target).abs().partial_cmp(&(b.0 - t_target).abs()).unwrap())
                else {
                    continue;
                };
                demands_t.insert(node_id.clone(), massflow_to_nm3s(*m, rho_star));
            }
            let snap = solve_steady_state_with_initial_pressures(
                &net,
                &demands_t,
                Some(&steady.pressures),
                3000,
                1e-3,
            )
            .unwrap_or_else(|e| panic!("TRR154 BCD snapshot t={t_target}: {e}"));
            assert!(snap.residual <= 1e-3);
            for (nid, &p) in &snap.pressures {
                assert!(
                    p.is_finite() && (15.0..=120.0).contains(&p),
                    "BCD snapshot t={t_target} {nid}={p}"
                );
            }
            eprintln!(
                "TRR154 BCD snapshot t={t_target:.0}s: residual={:.3e}, exit01 P={:.1}, events_built={}",
                snap.residual,
                snap.pressures.get("exit01").copied().unwrap_or(0.0),
                events.len()
            );
        }
    }

    /// PDE avec CI externe TRR154 `.state` : vérifie uniquement t=0 (pas de trajectoire).
    #[test]
    fn test_trr154_gaslib11_pde_ic_from_state() {
        let Some((net, demands, warm, pipe_states, state, _, _)) = trr154_gaslib11_pde_fixture()
        else {
            eprintln!("skip: TRR154 GasLib-11 corpus absent");
            return;
        };

        let composition = GasComposition::default();
        let cfg = TransientConfig {
            duration_s: 300.0,
            dt_s: 300.0,
            gas_composition: composition,
            n_cells_per_pipe: Some(4),
            adaptive_dt: false,
            picard_relax: None,
        };

        let pde = simulate_transient_with_mode(
            &net,
            &demands,
            &[],
            &cfg,
            TransientMode::Pde,
            Some(&warm),
            Some(&pipe_states),
        )
        .expect("TRR154 PDE IC");

        assert!(!pde.limitation.to_lowercase().contains("fallback"));
        let step0 = &pde.steps[0];
        // Sorties CS : projetées sur r_GazFlow (≠ P_state TRR154). Autres nœuds : CI.
        let cs_outlets = ["N01", "N05"];
        for (id, n) in &state.nodes {
            let p_sol = step0.pressures.get(id).copied().unwrap_or(f64::NAN);
            let tol = if cs_outlets.contains(&id.as_str()) {
                3.0
            } else {
                0.5
            };
            assert!(
                (p_sol - n.pressure_bar).abs() < tol,
                "t=0 {id}: P_sol={p_sol:.2} vs P_state={:.2} (tol={tol})",
                n.pressure_bar
            );
        }

        for step in pde.steps.iter().skip(1) {
            for (nid, &p) in &step.pressures {
                assert!(
                    p.is_finite() && (15.0..=120.0).contains(&p),
                    "t={:.0} {nid}={p} out of band",
                    step.time_s
                );
            }
        }

        eprintln!(
            "TRR154 PDE IC (spatial): {} pipe profiles, t_end={:.0}s exit01 P={:.2}",
            pipe_states.len(),
            pde.steps.last().map(|s| s.time_s).unwrap_or(0.0),
            pde
                .steps
                .last()
                .and_then(|s| s.pressures.get("exit01").copied())
                .unwrap_or(f64::NAN)
        );
    }

    /// Smoke PDE 900 s sous BC TRR154 : stabilité uniquement (pas d'oracle trajectoire).
    #[test]
    fn test_trr154_gaslib11_pde_smoke_900s() {
        let Some((net, demands, warm, pipe_states, _, bcd, rho_star)) =
            trr154_gaslib11_pde_fixture()
        else {
            eprintln!("skip: TRR154 GasLib-11 corpus absent");
            return;
        };

        let events = demand_events_from_bcd(&bcd, rho_star, 24)
            .into_iter()
            .filter(|ev| ev.time_s() > 1.0)
            .collect::<Vec<_>>();
        assert!(!events.is_empty(), "expected BCD demand events");

        let composition = GasComposition::default();
        let cfg = TransientConfig {
            duration_s: 900.0,
            dt_s: 60.0,
            gas_composition: composition,
            n_cells_per_pipe: Some(4),
            adaptive_dt: true,
            picard_relax: Some(0.25),
        };

        let pde = simulate_transient_with_mode(
            &net,
            &demands,
            &events,
            &cfg,
            TransientMode::Pde,
            Some(&warm),
            Some(&pipe_states),
        )
        .expect("TRR154 PDE smoke 900s");

        let (p_min, p_max) = assert_pde_smoke_band(&pde, "900s");
        let exit01_p = pde
            .steps
            .last()
            .and_then(|s| s.pressures.get("exit01").copied())
            .unwrap_or(f64::NAN);
        eprintln!(
            "TRR154 PDE smoke 900s: steps={}, P=[{p_min:.2},{p_max:.2}] bar, \
             exit01 P_final={exit01_p:.2}, events={}",
            pde.steps.len(),
            events.len()
        );
    }

    /// Smoke PDE 24 h sous BC TRR154 (mêmes réglages que le gate 900 s).
    ///
    /// Vert en release (~2 s, 28801 pas, P∈[40,68]). `#[ignore]` par défaut car
    /// debug ≈ 1–2 min. `dt_s=300` fixe diverge (P négatives) — ne pas « optimiser »
    /// le dt sans revalider.
    #[test]
    #[ignore = "TRR154 24h PDE smoke: run with --ignored (prefer --release)"]
    fn test_trr154_gaslib11_pde_smoke_24h() {
        let Some((net, demands, warm, pipe_states, _, bcd, rho_star)) =
            trr154_gaslib11_pde_fixture()
        else {
            eprintln!("skip: TRR154 GasLib-11 corpus absent");
            return;
        };

        let events = demand_events_from_bcd(&bcd, rho_star, 24)
            .into_iter()
            .filter(|ev| ev.time_s() > 1.0)
            .collect::<Vec<_>>();
        let composition = GasComposition::default();
        // Mêmes réglages que le gate 900 s (dt=300 fixe fait diverger P→négatif).
        let cfg = TransientConfig {
            duration_s: 86_400.0,
            dt_s: 60.0,
            gas_composition: composition,
            n_cells_per_pipe: Some(4),
            adaptive_dt: true,
            picard_relax: Some(0.25),
        };

        let pde = simulate_transient_with_mode(
            &net,
            &demands,
            &events,
            &cfg,
            TransientMode::Pde,
            Some(&warm),
            Some(&pipe_states),
        )
        .expect("TRR154 PDE smoke 24h");

        let (p_min, p_max) = assert_pde_smoke_band(&pde, "24h");
        let exit01_p = pde
            .steps
            .last()
            .and_then(|s| s.pressures.get("exit01").copied())
            .unwrap_or(f64::NAN);
        eprintln!(
            "TRR154 PDE smoke 24h: steps={}, P=[{p_min:.2},{p_max:.2}] bar, exit01 P_final={exit01_p:.2}",
            pde.steps.len()
        );
    }
}
