//! Presets solveur adaptés à la taille du réseau (démo → transport).

use serde::{Deserialize, Serialize};

/// Catégorie exploitant pour l'UI et les métadonnées API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkTier {
    /// Réseau de démo (GasLib-11, petits imports).
    Demo,
    /// Réseau d'étude standard (< 200 nœuds).
    Standard,
    /// Réseau transport moyen (200–2000 nœuds).
    Large,
    /// Très grand réseau (> 2000 nœuds).
    XLarge,
}

/// Paramètres de résolution recommandés pour un réseau.
#[derive(Debug, Clone, PartialEq)]
pub struct SolverPreset {
    pub tier: NetworkTier,
    pub max_iter: usize,
    pub tolerance: f64,
    pub timeout_ms: u64,
    /// Paliers de continuation de charge (1.0 = demande cible).
    pub continuation_scales: Vec<f64>,
    pub continuation_max_seconds: Option<u64>,
    /// Paliers intermédiaires auto-insérés en cas d'échec.
    pub continuation_auto_bridges: usize,
    pub snapshot_every: usize,
}

impl SolverPreset {
    pub fn uses_continuation(&self) -> bool {
        self.continuation_scales.len() > 1
            || self
                .continuation_scales
                .first()
                .is_some_and(|s| (*s - 1.0).abs() > 1e-9)
    }
}

/// Construit le preset effectif à partir des options client et de la taille réseau.
pub fn preset_from_request(
    node_count: usize,
    robust_mode: bool,
    max_iter: usize,
    tolerance: f64,
    timeout_ms: u64,
    snapshot_every: usize,
    continuation_scales: Option<Vec<f64>>,
) -> SolverPreset {
    let mut preset = if robust_mode {
        preset_robust(node_count)
    } else {
        preset_for_node_count(node_count)
    };
    preset.max_iter = max_iter;
    preset.tolerance = tolerance;
    if timeout_ms > 0 {
        preset.timeout_ms = timeout_ms;
    }
    preset.snapshot_every = snapshot_every.max(1);
    if let Some(scales) = continuation_scales {
        let filtered: Vec<f64> = scales.into_iter().filter(|s| *s > 0.0).collect();
        if !filtered.is_empty() {
            preset.continuation_scales = filtered;
        }
    }
    preset
}

pub fn tier_for_node_count(node_count: usize) -> NetworkTier {
    match node_count {
        0..=50 => NetworkTier::Demo,
        51..=199 => NetworkTier::Standard,
        200..=2000 => NetworkTier::Large,
        _ => NetworkTier::XLarge,
    }
}

pub fn tier_for_dataset(dataset_id: &str, node_count: usize) -> NetworkTier {
    if dataset_id == "GasLib-11" {
        return NetworkTier::Demo;
    }
    tier_for_node_count(node_count)
}

pub fn recommended_demo_for_dataset(dataset_id: &str) -> bool {
    matches!(dataset_id, "GasLib-11" | "GasLib-135" | "GasLib-582")
}

pub fn preset_for_node_count(node_count: usize) -> SolverPreset {
    let tier = tier_for_node_count(node_count);
    match tier {
        NetworkTier::Demo => SolverPreset {
            tier,
            max_iter: 1000,
            tolerance: 5e-4,
            timeout_ms: 30_000,
            continuation_scales: vec![1.0],
            continuation_max_seconds: None,
            continuation_auto_bridges: 0,
            snapshot_every: 3,
        },
        NetworkTier::Standard => SolverPreset {
            tier,
            max_iter: 1000,
            tolerance: 1e-3,
            timeout_ms: 60_000,
            continuation_scales: vec![0.5, 1.0],
            continuation_max_seconds: Some(90),
            continuation_auto_bridges: 2,
            snapshot_every: 3,
        },
        NetworkTier::Large => SolverPreset {
            tier,
            max_iter: 400,
            tolerance: 3e-3,
            timeout_ms: 180_000,
            continuation_scales: vec![0.05, 0.1, 0.2, 0.4, 0.7, 1.0],
            continuation_max_seconds: Some(180),
            continuation_auto_bridges: 6,
            snapshot_every: 3,
        },
        NetworkTier::XLarge => SolverPreset {
            tier,
            max_iter: 12,
            tolerance: 1e-2,
            timeout_ms: 240_000,
            continuation_scales: vec![0.05, 0.1, 0.2, 0.4, 0.7, 1.0],
            continuation_max_seconds: Some(240),
            continuation_auto_bridges: 8,
            snapshot_every: 1,
        },
    }
}

/// Preset « robuste » : force continuation même sur réseaux moyens.
pub fn preset_robust(node_count: usize) -> SolverPreset {
    let mut preset = preset_for_node_count(node_count);
    if !preset.uses_continuation() {
        preset.continuation_scales = vec![0.3, 0.6, 1.0];
        preset.continuation_max_seconds = Some(120);
        preset.tolerance = preset.tolerance.max(1e-3);
    }
    preset.timeout_ms = preset.timeout_ms.max(120_000);
    preset.max_iter = preset.max_iter.max(400);
    preset.continuation_auto_bridges = preset.continuation_auto_bridges.max(4);
    preset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_preset_no_continuation() {
        let p = preset_for_node_count(11);
        assert_eq!(p.tier, NetworkTier::Demo);
        assert!(!p.uses_continuation());
    }

    #[test]
    fn large_preset_uses_continuation() {
        let p = preset_for_node_count(582);
        assert_eq!(p.tier, NetworkTier::Large);
        assert!(p.uses_continuation());
        assert!(p.continuation_auto_bridges >= 4);
        assert!(p.max_iter >= 400);
    }

    #[test]
    fn gaslib_11_tier_is_demo() {
        assert_eq!(tier_for_dataset("GasLib-11", 11), NetworkTier::Demo);
    }

    #[test]
    fn robust_upgrades_small_network() {
        let p = preset_robust(20);
        assert!(p.uses_continuation());
    }

    #[test]
    fn recommended_demo_datasets() {
        assert!(recommended_demo_for_dataset("GasLib-11"));
        assert!(recommended_demo_for_dataset("GasLib-135"));
        assert!(recommended_demo_for_dataset("GasLib-582"));
    }
}
