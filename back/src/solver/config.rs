//! Paramètres de résolution steady-state (évite les signatures à 8+ arguments).

use crate::solver::gas_properties::GasComposition;

/// Configuration d'une résolution en régime permanent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SteadyStateConfig {
    pub gas_composition: GasComposition,
    pub max_iter: usize,
    pub tolerance: f64,
    pub snapshot_every: usize,
    /// Désactivé pendant les paliers de continuation (le ramping compresseur y est géré à part).
    pub enable_compressor_outer_loop: bool,
    /// Désactive le plafond MVP r²≤9 (mode carte compresseur ou bench H2).
    pub disable_compressor_r2_cap: bool,
    /// Retourne le dernier itéré Newton même si le résidu dépasse la tolérance (boucle carte).
    pub accept_partial_solution: bool,
}

impl Default for SteadyStateConfig {
    fn default() -> Self {
        Self {
            gas_composition: GasComposition::default(),
            max_iter: 500,
            tolerance: 1e-6,
            snapshot_every: 0,
            enable_compressor_outer_loop: true,
            disable_compressor_r2_cap: false,
            accept_partial_solution: false,
        }
    }
}

impl SteadyStateConfig {
    pub fn with_composition(mut self, gas_composition: GasComposition) -> Self {
        self.gas_composition = gas_composition;
        self
    }

    pub fn with_max_iter(mut self, max_iter: usize) -> Self {
        self.max_iter = max_iter;
        self
    }

    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance;
        self
    }

    pub fn with_snapshot_every(mut self, snapshot_every: usize) -> Self {
        self.snapshot_every = snapshot_every;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_matches_solver_defaults() {
        let cfg = SteadyStateConfig::default();
        assert_eq!(cfg.max_iter, 500);
        assert!((cfg.tolerance - 1e-6).abs() < f64::EPSILON);
        assert_eq!(cfg.snapshot_every, 0);
        assert!(cfg.enable_compressor_outer_loop);
        assert_eq!(cfg.gas_composition, GasComposition::g20_nominal());
    }

    #[test]
    fn builder_methods_override_fields() {
        let cfg = SteadyStateConfig::default()
            .with_composition(GasComposition::g20_nominal())
            .with_max_iter(42)
            .with_tolerance(1e-4)
            .with_snapshot_every(10);
        assert_eq!(cfg.max_iter, 42);
        assert!((cfg.tolerance - 1e-4).abs() < f64::EPSILON);
        assert_eq!(cfg.snapshot_every, 10);
        assert_eq!(cfg.gas_composition, GasComposition::g20_nominal());
    }
}
