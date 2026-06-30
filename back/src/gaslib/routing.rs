//! Sélection automatique du routage transport GasLib (`.cdf`) avant résolution.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use rayon::prelude::*;
use tracing::{debug, info};

use crate::graph::GasNetwork;
use crate::solver::presets::{SolverPreset, preset_for_node_count, preset_robust};
use crate::solver::{
    ContinuationStepEvent, GasComposition, SolverControl, SolverResult, solve_steady_state_with_preset,
};

use super::cdf::{CombinedDecisions, CdfDecision, apply_cdf_decisions, cdf_path_for_network, load_combined_decisions};
use super::connectivity::{
    active_component_stats, demands_span_multiple_active_components, routing_supports_demands,
};

/// Routage `.cdf` retenu pour une résolution (traçabilité UI / logs).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedCdfRouting {
    pub group_ids: Vec<String>,
    pub decision_ids: Vec<String>,
    pub screen_score: f64,
}

#[derive(Debug, Clone)]
pub struct CdfRoutingConfig {
    pub max_exhaustive_combinations: usize,
    pub screen_max_iter: usize,
    pub screen_tolerance: f64,
    pub screen_scales: Vec<f64>,
    pub screen_timeout_ms: u64,
}

impl CdfRoutingConfig {
    pub fn from_env(node_count: usize) -> Self {
        let default_combos = if node_count > 500 { 512 } else { 256 };
        let default_scales = if node_count > 500 {
            "0.15,0.4"
        } else {
            "0.15"
        };
        Self {
            max_exhaustive_combinations: env_usize("GAZFLOW_CDF_MAX_COMBINATIONS", default_combos),
            screen_max_iter: env_usize("GAZFLOW_CDF_SCREEN_MAX_ITER", 35),
            screen_tolerance: env_f64("GAZFLOW_CDF_SCREEN_TOL", 0.05),
            screen_scales: env_f64_list("GAZFLOW_CDF_SCREEN_SCALES", default_scales),
            screen_timeout_ms: env_usize("GAZFLOW_CDF_SCREEN_TIMEOUT_MS", 45_000) as u64,
        }
    }

    fn screen_preset(&self, node_count: usize, scale: f64) -> SolverPreset {
        let mut preset = preset_for_node_count(node_count);
        preset.max_iter = self.screen_max_iter.max(10);
        preset.tolerance = self.screen_tolerance;
        preset.continuation_scales = vec![scale.clamp(0.05, 1.0)];
        preset.continuation_auto_bridges = 0;
        preset.continuation_max_seconds = Some((self.screen_timeout_ms / 1000).max(5));
        preset.timeout_ms = self.screen_timeout_ms.max(5_000);
        preset
    }
}

/// Résultat complet de la résolution de routage (évite un second solve si déjà convergé).
#[derive(Debug, Clone)]
pub struct CdfRoutingOutcome {
    pub routing: ResolvedCdfRouting,
    pub full_solve: Option<SolverResult>,
}

/// Si un `.cdf` est présent, sélectionne et applique le routage adapté aux demandes.
pub fn resolve_and_apply_cdf_routing(
    network: &mut GasNetwork,
    net_path: &Path,
    demands: &HashMap<String, f64>,
    solve_preset: &SolverPreset,
) -> Result<Option<CdfRoutingOutcome>> {
    if skip_cdf_routing() {
        return Ok(None);
    }
    let Some(cdf_path) = cdf_path_for_network(net_path) else {
        return Ok(None);
    };
    let cdf = load_combined_decisions(&cdf_path)
        .with_context(|| format!("lecture routage transport {:?}", cdf_path))?;
    if cdf.groups.is_empty() {
        return Ok(None);
    }

    let config = CdfRoutingConfig::from_env(network.node_count());
    let topology = network.clone();
    let node_count = network.node_count();
    let baseline_rank = active_connectivity_rank(&topology);

    if node_count > 500 && baseline_rank.0 == 0 && !force_cdf_routing() {
        info!(
            baseline_rank = ?baseline_rank,
            "Routage `.cdf` ignoré : baseline connectée (grands réseaux, sans GAZFLOW_FORCE_CDF_ROUTING=1)"
        );
        return Ok(None);
    }

    let greedy = greedy_routing(&topology, &cdf, demands, &config, node_count);
    let mut greedy_trial = topology.clone();
    apply_cdf_decisions(&mut greedy_trial, &greedy.decisions);
    let greedy_rank = active_connectivity_rank(&greedy_trial);

    if connectivity_rank_worse(greedy_rank, baseline_rank) {
        info!(
            baseline_rank = ?baseline_rank,
            greedy_rank = ?greedy_rank,
            greedy_decisions = ?greedy.routing.decision_ids,
            "Routage `.cdf` ignoré : le greedy dégrade la connectivité active"
        );
        return Ok(None);
    }

    let candidates =
        select_cdf_routing_candidates(&topology, &cdf, demands, &config, Some(&greedy));
    let Some(best_candidate) = candidates.first().cloned() else {
        return Ok(None);
    };

    let decisions = decisions_for_routing(&cdf, &best_candidate);
    let mut best_trial = topology.clone();
    apply_cdf_decisions(&mut best_trial, &decisions);
    let best_rank = active_connectivity_rank(&best_trial);

    if connectivity_rank_worse(best_rank, baseline_rank) {
        info!(
            baseline_rank = ?baseline_rank,
            best_rank = ?best_rank,
            decisions = ?best_candidate.decision_ids,
            "Routage `.cdf` ignoré : meilleur candidat dégrade la connectivité"
        );
        return Ok(None);
    }

    let needs_score_comparison = best_rank == baseline_rank;
    let baseline_score = if needs_score_comparison {
        score_routing(&topology, demands, &config, node_count)
    } else {
        f64::INFINITY
    };

    if needs_score_comparison
        && !routing_topology_beats(
            &topology,
            &best_trial,
            baseline_score,
            best_candidate.screen_score,
        )
    {
        info!(
            baseline_score,
            best_score = best_candidate.screen_score,
            "Routage `.cdf` ignoré : baseline préférée au screening"
        );
        return Ok(None);
    }

    let (chosen, full_solve) = if needs_score_comparison {
        pick_routing_with_full_solve(&topology, &cdf, demands, &candidates, solve_preset)
            .unwrap_or_else(|| (best_candidate.clone(), None))
    } else {
        (best_candidate.clone(), None)
    };

    let final_decisions = decisions_for_routing(&cdf, &chosen);
    let mut final_trial = topology.clone();
    apply_cdf_decisions(&mut final_trial, &final_decisions);
    if !routing_topology_beats(
        &topology,
        &final_trial,
        baseline_score,
        chosen.screen_score,
    ) {
        info!(
            baseline_score,
            chosen_score = chosen.screen_score,
            baseline_rank = ?baseline_rank,
            chosen_rank = ?active_connectivity_rank(&final_trial),
            "Routage `.cdf` ignoré : topologie par défaut préférée après validation"
        );
        return Ok(None);
    }

    apply_cdf_decisions(network, &final_decisions);
    info!(
        groups = ?chosen.group_ids,
        decisions = ?chosen.decision_ids,
        screen_score = chosen.screen_score,
        converged = full_solve.is_some(),
        "Routage transport `.cdf` sélectionné"
    );
    Ok(Some(CdfRoutingOutcome {
        routing: chosen,
        full_solve,
    }))
}

/// Applique un routage explicite (override utilisateur futur).
pub fn apply_cdf_routing_by_id(
    network: &mut GasNetwork,
    cdf: &CombinedDecisions,
    decision_ids: &[&str],
) -> Result<ResolvedCdfRouting> {
    let decisions = decision_ids
        .iter()
        .filter_map(|id| {
            cdf.groups
                .iter()
                .flat_map(|g| g.decisions.iter())
                .find(|d| d.id == *id)
        })
        .collect::<Vec<_>>();
    if decisions.len() != cdf.groups.len() {
        bail!(
            "expected {} decision ids (one per group), got {}",
            cdf.groups.len(),
            decisions.len()
        );
    }
    apply_cdf_decisions(network, &decisions);
    Ok(ResolvedCdfRouting {
        group_ids: cdf.groups.iter().map(|g| g.id.clone()).collect(),
        decision_ids: decisions.iter().map(|d| d.id.clone()).collect(),
        screen_score: 0.0,
    })
}

struct RoutingSelection<'a> {
    routing: ResolvedCdfRouting,
    decisions: Vec<&'a CdfDecision>,
    #[allow(dead_code)]
    strategy: &'static str,
}

fn decisions_for_routing<'a>(cdf: &'a CombinedDecisions, routing: &ResolvedCdfRouting) -> Vec<&'a CdfDecision> {
    let map: HashMap<&str, &CdfDecision> = cdf
        .groups
        .iter()
        .flat_map(|g| g.decisions.iter())
        .map(|d| (d.id.as_str(), d))
        .collect();
    routing
        .decision_ids
        .iter()
        .filter_map(|id| map.get(id.as_str()).copied())
        .collect()
}

fn pick_routing_with_full_solve(
    topology: &GasNetwork,
    cdf: &CombinedDecisions,
    demands: &HashMap<String, f64>,
    candidates: &[ResolvedCdfRouting],
    solve_preset: &SolverPreset,
) -> Option<(ResolvedCdfRouting, Option<SolverResult>)> {
    if candidates.is_empty() {
        return None;
    }
    let top_k = env_usize("GAZFLOW_CDF_FULL_SOLVE_CANDIDATES", 5);
    let validation_preset = if topology.node_count() > 500 {
        preset_robust(topology.node_count())
    } else {
        solve_preset.clone()
    };
    let mut best: Option<(ResolvedCdfRouting, f64, Option<SolverResult>)> = None;

    for candidate in candidates.iter().take(top_k) {
        let decisions = decisions_for_routing(cdf, candidate);
        if decisions.len() != cdf.groups.len() {
            continue;
        }
        let mut trial = topology.clone();
        apply_cdf_decisions(&mut trial, &decisions);
        if !routing_supports_demands(&trial, demands) {
            continue;
        }
        match solve_steady_state_with_preset(
            &trial,
            demands,
            None,
            &validation_preset,
            GasComposition::pure_ch4(),
            |_| SolverControl::Continue,
            None::<fn(ContinuationStepEvent)>,
        ) {
            Ok(result) => {
                let scale = result.demand_scale_achieved.unwrap_or(1.0);
                if result.residual <= validation_preset.tolerance && scale >= 0.999 {
                    info!(
                        decisions = ?candidate.decision_ids,
                        residual = result.residual,
                        "Routage `.cdf` validé en solveur complet"
                    );
                    return Some((candidate.clone(), Some(result)));
                }
                let score = routing_score_from_result(
                    &result,
                    validation_preset.tolerance,
                    active_component_stats(&trial).1,
                );
                if best.as_ref().is_none_or(|(_, s, _)| score < *s) {
                    best = Some((candidate.clone(), score, None));
                }
            }
            Err(_) => {}
        }
    }

    best.map(|(routing, _, _)| (routing, None))
}

fn select_cdf_routing_candidates<'a>(
    topology: &GasNetwork,
    cdf: &CombinedDecisions,
    demands: &HashMap<String, f64>,
    config: &CdfRoutingConfig,
    precomputed_greedy: Option<&RoutingSelection<'a>>,
) -> Vec<ResolvedCdfRouting> {
    let node_count = topology.node_count();
    let total = total_combinations(cdf);
    let mut scored: Vec<ResolvedCdfRouting> = Vec::new();

    if total <= config.max_exhaustive_combinations {
        scored = exhaustive_routing_scores(topology, cdf, demands, config, node_count);
    } else if let Some(greedy) = precomputed_greedy {
        scored.push(greedy.routing.clone());
        if let Some(refined) = local_refine(topology, cdf, demands, config, node_count, greedy) {
            scored.push(refined.routing);
        }
    } else {
        let greedy = greedy_routing(topology, cdf, demands, config, node_count);
        scored.push(greedy.routing.clone());
        if let Some(refined) = local_refine(topology, cdf, demands, config, node_count, &greedy) {
            scored.push(refined.routing);
        }
    }

    sort_routing_candidates_by_connectivity(topology, cdf, &mut scored);
    scored.dedup_by(|a, b| a.decision_ids == b.decision_ids);
    let has_finite_score = scored.iter().any(|r| r.screen_score.is_finite());
    if has_finite_score {
        scored.retain(|r| r.screen_score.is_finite());
    }
    if scored.is_empty() {
        scored.push(
            first_decision_fallback(cdf, topology, demands, config, node_count)
                .routing,
        );
    }
    scored
}

fn active_connectivity_rank(network: &GasNetwork) -> (usize, usize) {
    let (total, without_fixed) = active_component_stats(network);
    (without_fixed, total)
}

fn connectivity_rank_strictly_better(candidate: (usize, usize), baseline: (usize, usize)) -> bool {
    candidate.0 < baseline.0 || (candidate.0 == baseline.0 && candidate.1 < baseline.1)
}

fn connectivity_rank_worse(candidate: (usize, usize), baseline: (usize, usize)) -> bool {
    connectivity_rank_strictly_better(baseline, candidate)
}

fn sort_routing_candidates_by_connectivity(
    topology: &GasNetwork,
    cdf: &CombinedDecisions,
    scored: &mut Vec<ResolvedCdfRouting>,
) {
    let mut ranked: Vec<(ResolvedCdfRouting, (usize, usize))> = scored
        .drain(..)
        .map(|routing| {
            let rank = candidate_connectivity_rank(topology, cdf, &routing);
            (routing, rank)
        })
        .collect();
    ranked.sort_by(|(a, rank_a), (b, rank_b)| {
        rank_a
            .0
            .cmp(&rank_b.0)
            .then_with(|| rank_a.1.cmp(&rank_b.1))
            .then_with(|| {
                a.screen_score
                    .partial_cmp(&b.screen_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    scored.extend(ranked.into_iter().map(|(routing, _)| routing));
}

fn candidate_connectivity_rank(
    topology: &GasNetwork,
    cdf: &CombinedDecisions,
    routing: &ResolvedCdfRouting,
) -> (usize, usize) {
    let mut trial = topology.clone();
    apply_cdf_decisions(&mut trial, &decisions_for_routing(cdf, routing));
    let (total, without_fixed) = active_component_stats(&trial);
    (without_fixed, total)
}

/// Vrai si la topologie candidate améliore la baseline (connectivité d'abord, puis score).
fn routing_topology_beats(
    baseline: &GasNetwork,
    candidate: &GasNetwork,
    baseline_score: f64,
    candidate_score: f64,
) -> bool {
    let (b_unfixed, b_total) = active_component_stats(baseline);
    let (c_unfixed, c_total) = active_component_stats(candidate);
    if c_unfixed != b_unfixed {
        return c_unfixed < b_unfixed;
    }
    if c_total != b_total {
        return c_total < b_total;
    }
    match (
        baseline_score.is_finite(),
        candidate_score.is_finite(),
    ) {
        (true, true) => candidate_score < baseline_score,
        (_, true) => true,
        (true, false) => false,
        (false, false) => false,
    }
}

fn greedy_routing<'a>(
    topology: &GasNetwork,
    cdf: &'a CombinedDecisions,
    demands: &HashMap<String, f64>,
    config: &CdfRoutingConfig,
    node_count: usize,
) -> RoutingSelection<'a> {
    let mut picked_indices = Vec::new();
    let mut picked_decisions: Vec<&CdfDecision> = Vec::new();

    for group in &cdf.groups {
        let mut best_idx = 0usize;
        let mut best_score = f64::INFINITY;
        for (idx, decision) in group.decisions.iter().enumerate() {
            let mut trial = topology.clone();
            let mut trial_decisions = picked_decisions.clone();
            trial_decisions.push(decision);
            apply_cdf_decisions(&mut trial, &trial_decisions);
            let score = score_routing(&trial, demands, config, node_count);
            if score < best_score {
                best_score = score;
                best_idx = idx;
            }
        }
        picked_indices.push(best_idx);
        picked_decisions.push(&group.decisions[best_idx]);
    }

    let final_score = {
        let mut trial = topology.clone();
        apply_cdf_decisions(&mut trial, &picked_decisions);
        score_routing(&trial, demands, config, node_count)
    };

    build_selection(cdf, picked_indices, final_score, "greedy")
}

fn exhaustive_routing_scores(
    topology: &GasNetwork,
    cdf: &CombinedDecisions,
    demands: &HashMap<String, f64>,
    config: &CdfRoutingConfig,
    node_count: usize,
) -> Vec<ResolvedCdfRouting> {
    let combos = enumerate_index_combinations(cdf);
    let evaluated = AtomicUsize::new(0);
    let started = Instant::now();

    let mut scored: Vec<(Vec<usize>, f64)> = combos
        .par_iter()
        .map(|indices| {
            evaluated.fetch_add(1, Ordering::Relaxed);
            let mut trial = topology.clone();
            let decisions = indices
                .iter()
                .enumerate()
                .map(|(g, &i)| &cdf.groups[g].decisions[i])
                .collect::<Vec<_>>();
            apply_cdf_decisions(&mut trial, &decisions);
            let score = score_routing(&trial, demands, config, node_count);
            (indices.clone(), score)
        })
        .collect();

    scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    debug!(
        trials = evaluated.load(Ordering::Relaxed),
        elapsed_ms = started.elapsed().as_millis(),
        "Recherche exhaustive `.cdf` terminée"
    );

    scored
        .into_iter()
        .map(|(indices, score)| build_selection(cdf, indices, score, "exhaustive").routing)
        .collect()
}

fn local_refine<'a>(
    topology: &GasNetwork,
    cdf: &'a CombinedDecisions,
    demands: &HashMap<String, f64>,
    config: &CdfRoutingConfig,
    node_count: usize,
    seed: &RoutingSelection<'a>,
) -> Option<RoutingSelection<'a>> {
    let mut indices = seed
        .decisions
        .iter()
        .enumerate()
        .map(|(g, d)| {
            cdf.groups[g]
                .decisions
                .iter()
                .position(|x| x.id == d.id)
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    let mut best_score = seed.routing.screen_score;
    let mut improved = false;

    for g in 0..cdf.groups.len() {
        for alt in 0..cdf.groups[g].decisions.len() {
            if alt == indices[g] {
                continue;
            }
            let mut trial_indices = indices.clone();
            trial_indices[g] = alt;
            let mut trial = topology.clone();
            let decisions = trial_indices
                .iter()
                .enumerate()
                .map(|(gi, &i)| &cdf.groups[gi].decisions[i])
                .collect::<Vec<_>>();
            apply_cdf_decisions(&mut trial, &decisions);
            let score = score_routing(&trial, demands, config, node_count);
            if score < best_score {
                best_score = score;
                indices = trial_indices;
                improved = true;
            }
        }
    }

    improved.then(|| build_selection(cdf, indices, best_score, "local_refine"))
}

fn first_decision_fallback<'a>(
    cdf: &'a CombinedDecisions,
    topology: &GasNetwork,
    demands: &HashMap<String, f64>,
    config: &CdfRoutingConfig,
    node_count: usize,
) -> RoutingSelection<'a> {
    let indices = vec![0; cdf.groups.len()];
    let mut trial = topology.clone();
    let decisions: Vec<&CdfDecision> = cdf
        .groups
        .iter()
        .filter_map(|g| g.decisions.first())
        .collect();
    apply_cdf_decisions(&mut trial, &decisions);
    let score = score_routing(&trial, demands, config, node_count);
    build_selection(cdf, indices, score, "fallback_first")
}

fn build_selection<'a>(
    cdf: &'a CombinedDecisions,
    indices: Vec<usize>,
    score: f64,
    strategy: &'static str,
) -> RoutingSelection<'a> {
    let decisions = indices
        .iter()
        .enumerate()
        .map(|(g, &i)| &cdf.groups[g].decisions[i])
        .collect::<Vec<_>>();
    RoutingSelection {
        routing: ResolvedCdfRouting {
            group_ids: cdf.groups.iter().map(|g| g.id.clone()).collect(),
            decision_ids: decisions.iter().map(|d| d.id.clone()).collect(),
            screen_score: score,
        },
        decisions,
        strategy,
    }
}

fn routing_fragmentation_penalty(network: &GasNetwork, demands: &HashMap<String, f64>) -> f64 {
    let (total, without_fixed) = active_component_stats(network);
    let large = network.node_count() > 500;

    if without_fixed > 1 {
        if large {
            return f64::INFINITY;
        }
        return 1.0e6 * (without_fixed - 1) as f64;
    }

    if total > 1 && demands_span_multiple_active_components(network, demands) {
        if large {
            return f64::INFINITY;
        }
        return 1.0e6;
    }

    0.0
}

fn score_routing(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    config: &CdfRoutingConfig,
    node_count: usize,
) -> f64 {
    if !routing_supports_demands(network, demands) {
        return f64::INFINITY;
    }

    let fragmentation = routing_fragmentation_penalty(network, demands);
    if !fragmentation.is_finite() {
        return f64::INFINITY;
    }

    let scales = if config.screen_scales.is_empty() {
        vec![0.15]
    } else {
        config.screen_scales.clone()
    };

    let (_, components_without_fixed) = active_component_stats(network);
    let mut best_score = f64::INFINITY;

    for scale in scales {
        let preset = config.screen_preset(node_count, scale);
        let score = score_routing_at_scale(network, demands, &preset, components_without_fixed);
        if score < best_score {
            best_score = score;
        }
    }

    if !best_score.is_finite() {
        f64::INFINITY
    } else {
        best_score + fragmentation
    }
}

fn score_routing_at_scale(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    preset: &SolverPreset,
    components_without_fixed: usize,
) -> f64 {
    match solve_steady_state_with_preset(
        network,
        demands,
        None,
        preset,
        GasComposition::pure_ch4(),
        |_| SolverControl::Continue,
        None::<fn(ContinuationStepEvent)>,
    ) {
        Ok(result) => {
            routing_score_from_result(&result, preset.tolerance, components_without_fixed)
        }
        Err(_) => f64::INFINITY,
    }
}

fn routing_score_from_result(
    result: &SolverResult,
    tolerance: f64,
    components_without_fixed: usize,
) -> f64 {
    if !result.residual.is_finite() || result.residual > 1.0e6 {
        return f64::INFINITY;
    }
    let scale = result.demand_scale_achieved.unwrap_or(1.0);
    let scale_gap = (1.0 - scale).max(0.0);
    // Pénalité forte si le palier cible n'est pas atteint ; priorité à la convergence.
    let scale_penalty = scale_gap * 1.0e4;
    let converged_bonus = if result.residual <= tolerance && scale >= 0.999 {
        -1.0
    } else {
        0.0
    };
    let component_penalty = if components_without_fixed > 0 {
        1.0e3 * components_without_fixed as f64
    } else {
        0.0
    };
    result.residual + scale_penalty + converged_bonus + component_penalty
}

fn total_combinations(cdf: &CombinedDecisions) -> usize {
    cdf.groups
        .iter()
        .map(|g| g.decisions.len().max(1))
        .product()
}

fn enumerate_index_combinations(cdf: &CombinedDecisions) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    let sizes: Vec<usize> = cdf.groups.iter().map(|g| g.decisions.len()).collect();
    let mut current = vec![0usize; sizes.len()];
    loop {
        out.push(current.clone());
        let mut carry = true;
        for i in (0..current.len()).rev() {
            if sizes[i] == 0 {
                continue;
            }
            current[i] += 1;
            if current[i] < sizes[i] {
                carry = false;
                break;
            }
            current[i] = 0;
        }
        if carry {
            break;
        }
    }
    out
}

fn skip_cdf_routing() -> bool {
    std::env::var("GAZFLOW_SKIP_CDF_ROUTING")
        .or_else(|_| std::env::var("GAZFLOW_SKIP_CDF"))
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

fn force_cdf_routing() -> bool {
    std::env::var("GAZFLOW_FORCE_CDF_ROUTING")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_f64_list(key: &str, default: &str) -> Vec<f64> {
    let raw = std::env::var(key).unwrap_or_else(|_| default.to_string());
    raw.split(',')
        .filter_map(|part| part.trim().parse::<f64>().ok())
        .map(|s| s.clamp(0.05, 1.0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{ConnectionKind, Node, Pipe};

    fn toy_cdf() -> CombinedDecisions {
        CombinedDecisions {
            groups: vec![
                super::super::cdf::CdfDecisionGroup {
                    id: "g1".into(),
                    decisions: vec![
                        super::super::cdf::CdfDecision {
                            id: "a1".into(),
                            elements: vec![],
                        },
                        super::super::cdf::CdfDecision {
                            id: "a2".into(),
                            elements: vec![],
                        },
                    ],
                },
                super::super::cdf::CdfDecisionGroup {
                    id: "g2".into(),
                    decisions: vec![
                        super::super::cdf::CdfDecision {
                            id: "b1".into(),
                            elements: vec![],
                        },
                        super::super::cdf::CdfDecision {
                            id: "b2".into(),
                            elements: vec![],
                        },
                    ],
                },
            ],
        }
    }

    #[test]
    fn enumerate_combinations_count() {
        let cdf = toy_cdf();
        assert_eq!(total_combinations(&cdf), 4);
        assert_eq!(enumerate_index_combinations(&cdf).len(), 4);
    }

    #[test]
    fn routing_score_prefers_convergence() {
        let ok = routing_score_from_result(
            &SolverResult {
                residual: 1e-4,
                demand_scale_achieved: Some(1.0),
                ..Default::default()
            },
            1e-3,
            0,
        );
        let bad = routing_score_from_result(
            &SolverResult {
                residual: 5.0,
                demand_scale_achieved: Some(0.5),
                ..Default::default()
            },
            1e-3,
            0,
        );
        assert!(ok < bad);
    }

    #[test]
    fn cdf_config_default_scales_by_size() {
        let small = CdfRoutingConfig::from_env(100);
        assert_eq!(small.screen_scales, vec![0.15]);
        let large = CdfRoutingConfig::from_env(600);
        assert_eq!(large.screen_scales, vec![0.15, 0.4]);
    }

    #[test]
    fn routing_fragmentation_penalty_rejects_large_multi_component() {
        let mut net = GasNetwork::new();
        for i in 0..600 {
            net.add_node(Node {
                id: format!("N{i}"),
                ..Default::default()
            });
        }
        net.add_pipe(Pipe {
            id: "P1".into(),
            from: "N0".into(),
            to: "N1".into(),
            kind: ConnectionKind::Pipe,
            ..Default::default()
        });
        net.add_pipe(Pipe {
            id: "P2".into(),
            from: "N2".into(),
            to: "N3".into(),
            kind: ConnectionKind::Pipe,
            ..Default::default()
        });
        let demands = HashMap::new();
        assert!(!routing_fragmentation_penalty(&net, &demands).is_finite());
    }

    #[test]
    fn routing_topology_beats_prefers_connected_baseline() {
        let mut baseline = GasNetwork::new();
        for id in ["A", "B", "C"] {
            baseline.add_node(Node {
                id: id.into(),
                ..Default::default()
            });
        }
        baseline.add_pipe(Pipe {
            id: "P1".into(),
            from: "A".into(),
            to: "B".into(),
            kind: ConnectionKind::Pipe,
            ..Default::default()
        });
        baseline.add_pipe(Pipe {
            id: "P2".into(),
            from: "B".into(),
            to: "C".into(),
            kind: ConnectionKind::Pipe,
            ..Default::default()
        });

        let mut fragmented = GasNetwork::new();
        for id in ["A", "B", "C", "D"] {
            fragmented.add_node(Node {
                id: id.into(),
                ..Default::default()
            });
        }
        fragmented.add_pipe(Pipe {
            id: "P1".into(),
            from: "A".into(),
            to: "B".into(),
            kind: ConnectionKind::Pipe,
            ..Default::default()
        });
        fragmented.add_pipe(Pipe {
            id: "P2".into(),
            from: "C".into(),
            to: "D".into(),
            kind: ConnectionKind::Pipe,
            ..Default::default()
        });

        assert!(!routing_topology_beats(
            &baseline,
            &fragmented,
            f64::INFINITY,
            f64::INFINITY,
        ));
        assert!(routing_topology_beats(
            &fragmented,
            &baseline,
            f64::INFINITY,
            10.0,
        ));
    }

    #[test]
    fn connectivity_rank_helpers() {
        let baseline = (0usize, 1usize);
        let worse = (2, 5);
        let better = (0, 1);
        assert!(connectivity_rank_worse(worse, baseline));
        assert!(!connectivity_rank_strictly_better(worse, baseline));
        assert!(!connectivity_rank_strictly_better(better, baseline));
        assert!(connectivity_rank_strictly_better((0, 1), (0, 2)));
    }

    #[test]
    fn apply_routing_by_id_requires_all_groups() {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "A".into(),
            ..Default::default()
        });
        net.add_node(Node {
            id: "B".into(),
            ..Default::default()
        });
        net.add_pipe(Pipe {
            id: "P".into(),
            from: "A".into(),
            to: "B".into(),
            kind: ConnectionKind::Pipe,
            ..Default::default()
        });
        let cdf = toy_cdf();
        let err = apply_cdf_routing_by_id(&mut net, &cdf, &["a1"]).expect_err("missing group");
        assert!(err.to_string().contains("expected 2 decision ids"));
    }
}
