//! Modèles d'équation d'état (EOS) pour mélanges gaz.

pub mod gerg;
pub mod pr78;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EosModel {
    #[default]
    PapayKay,
    /// Peng-Robinson 1978 (recommandé H₂ ≥ 25 %, ou dominant dès 15 % en diagnostics).
    Pr78,
    /// GERG-2008 (précision élevée, H₂ élevé / calage).
    Gerg2008,
}

impl EosModel {
    /// Bande de transition douce Papay ↔ PR-78 (évite le saut de densité à 20 % H₂).
    pub const H2_BLEND_LO: f64 = 0.15;
    pub const H2_BLEND_HI: f64 = 0.25;
    pub const H2_GERG_THRESHOLD: f64 = 0.50;

    /// Sélection « dominante » pour diagnostics / warnings :
    /// - GERG si H₂ > 50 %
    /// - PR-78 si H₂ > 15 % (y compris dans la bande de blend où Z est encore mixte)
    /// - Papay si H₂ ≤ 15 %
    ///
    /// Attention : dans [15 %, 25 %], `compressibility()` blend Papay↔PR-78 ;
    /// ce label n'implique pas Z = Z_PR78 pur avant 25 %.
    pub fn auto_for_composition(h2_fraction: f64) -> Self {
        if h2_fraction > Self::H2_GERG_THRESHOLD + 1e-9 {
            Self::Gerg2008
        } else if h2_fraction > Self::H2_BLEND_LO + 1e-9 {
            Self::Pr78
        } else {
            Self::PapayKay
        }
    }

    /// Poids PR-78 dans le blend Papay↔PR-78 (smoothstep sur [15 %, 25 %] H₂).
    pub fn pr78_blend_weight(h2_fraction: f64) -> f64 {
        if h2_fraction <= Self::H2_BLEND_LO {
            0.0
        } else if h2_fraction >= Self::H2_BLEND_HI {
            1.0
        } else {
            let t = (h2_fraction - Self::H2_BLEND_LO)
                / (Self::H2_BLEND_HI - Self::H2_BLEND_LO);
            // smoothstep hermite : C¹, dérivée nulle aux bords
            t * t * (3.0 - 2.0 * t)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EosModel;

    #[test]
    fn test_pr78_blend_weight_boundaries_and_c1() {
        assert_eq!(EosModel::pr78_blend_weight(0.10), 0.0);
        assert_eq!(EosModel::pr78_blend_weight(0.15), 0.0);
        assert_eq!(EosModel::pr78_blend_weight(0.25), 1.0);
        assert_eq!(EosModel::pr78_blend_weight(0.40), 1.0);
        // Milieu de bande : smoothstep(0.5) = 0.5
        assert!((EosModel::pr78_blend_weight(0.20) - 0.5).abs() < 1e-12);
        // Dérivée numérique ≈ 0 aux bords (C¹)
        let eps = 1e-6;
        let d_lo = (EosModel::pr78_blend_weight(0.15 + eps) - EosModel::pr78_blend_weight(0.15))
            / eps;
        let d_hi = (EosModel::pr78_blend_weight(0.25) - EosModel::pr78_blend_weight(0.25 - eps))
            / eps;
        assert!(d_lo.abs() < 1e-3, "dW/dH₂ at 15% should be ~0, got {d_lo}");
        assert!(d_hi.abs() < 1e-3, "dW/dH₂ at 25% should be ~0, got {d_hi}");
        // Monotonie
        let mut prev = -1.0;
        for i in 0..=20 {
            let h = 0.15 + 0.005 * i as f64;
            let w = EosModel::pr78_blend_weight(h);
            assert!(w + 1e-15 >= prev, "blend weight not monotone at H₂={h}");
            prev = w;
        }
    }
}
