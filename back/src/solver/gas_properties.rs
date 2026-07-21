//! Propriétés thermodynamiques du gaz (CH₄ pur ou mélange multi-composant).
//!
//! ## Hypothèses et limites (revue métier)
//! - **Kay + Papay** : pseudo-critiques de Kay (moyenne molaire) pour Z de Papay ;
//!   acceptable gaz naturel classique (CH₄ dominant, H₂ < 10 %). Au-delà de 15 % H₂,
//!   blend Papay↔PR-78 puis PR-78 ; GERG-2008 au-delà de 50 %.
//! - **PCS / PCI** : mélange idéal des composants à 0 °C, 101,325 kPa (MJ/Nm³, ISO 6976).
//! - **Wobbe** : WI = PCS / √d avec d = M_gaz / M_air (EN 437, gaz parfait à 15 °C).
//! - La composition par défaut des simulations opérationnelles est le **G20** (EN 437).
//!   Le CH₄ pur reste disponible via [`GasComposition::pure_ch4`] (référence GasLib / comparaisons).

/// Température par défaut (isotherme) utilisée par le MVP [K] (15 °C).
pub const DEFAULT_GAS_TEMPERATURE_K: f64 = 288.15;

/// Conditions standard EN 437 / ISO 6976 pour PCS et Wobbe.
pub const STANDARD_PRESSURE_BAR: f64 = 1.013_25;
pub const STANDARD_TEMPERATURE_K: f64 = 288.15; // 15 °C

const UNIVERSAL_GAS_CONSTANT: f64 = 8.314_462_618; // J/(mol·K)
const MOLAR_MASS_AIR_KG_PER_MOL: f64 = 0.028_964_7; // ISO 6976

// Constantes héritées du MVP (CH₄ pur) — conservées pour compatibilité binaire.
const LEGACY_MOLAR_MASS_KG_PER_MOL: f64 = 0.016_04;
const LEGACY_CRITICAL_PRESSURE_BAR: f64 = 46.0;
const LEGACY_CRITICAL_TEMPERATURE_K: f64 = 190.6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GasComponent {
    pub molar_mass_kg_per_mol: f64,
    pub critical_pressure_bar: f64,
    pub critical_temperature_k: f64,
    /// PCS volumique de référence [MJ/Nm³] à 0 °C, 101,325 kPa (ISO 6976, composants combustibles).
    pub pcs_mj_per_nm3: f64,
    /// PCI volumique inférieur de référence [MJ/Nm³] (ISO 6976, composants combustibles).
    pub pci_mj_per_nm3: f64,
}

impl GasComponent {
    const CH4: Self = Self {
        molar_mass_kg_per_mol: LEGACY_MOLAR_MASS_KG_PER_MOL,
        critical_pressure_bar: LEGACY_CRITICAL_PRESSURE_BAR,
        critical_temperature_k: LEGACY_CRITICAL_TEMPERATURE_K,
        pcs_mj_per_nm3: 39.82,
        pci_mj_per_nm3: 35.81,
    };
    const C2H6: Self = Self {
        molar_mass_kg_per_mol: 0.030_07,
        critical_pressure_bar: 48.72,
        critical_temperature_k: 305.32,
        pcs_mj_per_nm3: 68.4,
        pci_mj_per_nm3: 62.90,
    };
    const CO2: Self = Self {
        molar_mass_kg_per_mol: 0.044_01,
        critical_pressure_bar: 73.77,
        critical_temperature_k: 304.13,
        pcs_mj_per_nm3: 0.0,
        pci_mj_per_nm3: 0.0,
    };
    const N2: Self = Self {
        molar_mass_kg_per_mol: 0.028_01,
        critical_pressure_bar: 33.96,
        critical_temperature_k: 126.20,
        pcs_mj_per_nm3: 0.0,
        pci_mj_per_nm3: 0.0,
    };
    const H2: Self = Self {
        molar_mass_kg_per_mol: 0.002_016,
        critical_pressure_bar: 12.96,
        critical_temperature_k: 33.19,
        pcs_mj_per_nm3: 12.74,
        pci_mj_per_nm3: 10.76,
    };
}

use serde::{Deserialize, Serialize};

/// Fractions molaires normalisées (Σ = 1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GasComposition {
    pub ch4: f64,
    pub c2h6: f64,
    pub co2: f64,
    pub n2: f64,
    pub h2: f64,
}

impl Default for GasComposition {
    /// G20 nominal pour les réseaux français (distribution / transport gaz naturel).
    fn default() -> Self {
        Self::g20_nominal()
    }
}

impl GasComposition {
    pub fn pure_ch4() -> Self {
        Self {
            ch4: 1.0,
            c2h6: 0.0,
            co2: 0.0,
            n2: 0.0,
            h2: 0.0,
        }
    }

    /// Gaz de ville G20 (France, EN 437) — fractions molaires indicatives.
    ///
    /// Réf. GRDF / EN 437 famille H : ~78 % CH₄, ~11,5 % C₂H₆ (C₃+ inclus),
    /// ~2,5 % CO₂, ~8 % N₂ → PCS ~39 MJ/Nm³, Wobbe ~46–48 MJ/Nm³.
    pub fn g20_nominal() -> Self {
        Self {
            ch4: 0.780,
            c2h6: 0.115,
            co2: 0.025,
            n2: 0.080,
            h2: 0.0,
        }
        .normalize()
    }

    /// Alias rétrocompat tests / docs.
    pub fn typical_distribution() -> Self {
        Self::g20_nominal()
    }

    /// Normalise les fractions (ignore les valeurs négatives).
    pub fn normalize(self) -> Self {
        let parts = [
            self.ch4.max(0.0),
            self.c2h6.max(0.0),
            self.co2.max(0.0),
            self.n2.max(0.0),
            self.h2.max(0.0),
        ];
        let sum: f64 = parts.iter().sum();
        if sum <= 1e-12 {
            return Self::pure_ch4();
        }
        Self {
            ch4: parts[0] / sum,
            c2h6: parts[1] / sum,
            co2: parts[2] / sum,
            n2: parts[3] / sum,
            h2: parts[4] / sum,
        }
    }

    fn fractions(&self) -> [(GasComponent, f64); 5] {
        [
            (GasComponent::CH4, self.ch4),
            (GasComponent::C2H6, self.c2h6),
            (GasComponent::CO2, self.co2),
            (GasComponent::N2, self.n2),
            (GasComponent::H2, self.h2),
        ]
    }

    /// Masse molaire du mélange [kg/mol] (moyenne molaire).
    pub fn molar_mass_kg_per_mol(&self) -> f64 {
        self.fractions()
            .iter()
            .map(|(c, y)| y * c.molar_mass_kg_per_mol)
            .sum()
    }

    /// Pression critique pseudo [bar] (Kay, moyenne molaire — couplage Papay standard).
    pub fn pseudo_critical_pressure_bar(&self) -> f64 {
        self.fractions()
            .iter()
            .map(|(c, y)| y * c.critical_pressure_bar)
            .sum::<f64>()
            .max(1e-6)
    }

    /// Température critique pseudo [K] (Kay, moyenne molaire).
    pub fn pseudo_critical_temperature_k(&self) -> f64 {
        self.fractions()
            .iter()
            .map(|(c, y)| y * c.critical_temperature_k)
            .sum::<f64>()
            .max(1e-6)
    }

    /// EOS dominante pour diagnostics (GERG >50 %, PR-78 au-delà de 15 % H₂, sinon Papay).
    pub fn effective_eos(&self) -> super::eos::EosModel {
        super::eos::EosModel::auto_for_composition(self.h2)
    }

    /// Facteur Z.
    ///
    /// Sous 50 % H₂ : **blend d'ingénierie** C¹ de $Z$ Papay↔PR-78 sur [15 %, 25 %]
    /// (smoothstep sur le poids). Ce n'est **pas** une règle de mélange thermodynamique ;
    /// le but est la continuité de $\rho(H_2)$ pour éviter un saut artificiel au basculement EOS.
    /// Au-delà de 50 % : GERG-2008 (fallback PR-78).
    pub fn compressibility(&self, pressure_bar: f64, temperature_k: f64) -> f64 {
        use super::eos::EosModel;
        if self.h2 > EosModel::H2_GERG_THRESHOLD + 1e-9 {
            return super::eos::gerg::compressibility_gerg2008(*self, pressure_bar, temperature_k)
                .unwrap_or_else(|| {
                    super::eos::pr78::compressibility_pr78(*self, pressure_bar, temperature_k)
                });
        }
        let w = EosModel::pr78_blend_weight(self.h2);
        if w <= 1e-15 {
            return self.compressibility_papay(pressure_bar, temperature_k);
        }
        let z_pr = super::eos::pr78::compressibility_pr78(*self, pressure_bar, temperature_k);
        if w >= 1.0 - 1e-15 {
            return z_pr;
        }
        let z_papay = self.compressibility_papay(pressure_bar, temperature_k);
        (1.0 - w) * z_papay + w * z_pr
    }

    /// Facteur de compressibilité Z (Papay, pseudo-Pr/Tr).
    pub fn compressibility_papay(&self, pressure_bar: f64, temperature_k: f64) -> f64 {
        compressibility_factor_papay_for_criticals(
            pressure_bar,
            temperature_k,
            self.pseudo_critical_pressure_bar(),
            self.pseudo_critical_temperature_k(),
        )
    }

    /// Avertissements physiques (EOS, domaine de validité).
    pub fn physics_warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        use super::eos::EosModel;
        if self.h2 > EosModel::H2_GERG_THRESHOLD + 1e-9 {
            out.push(
                "Fraction H₂ > 50 % : EOS GERG-2008 activée (fallback PR-78 si itération densité échoue)."
                    .to_string(),
            );
        } else if self.h2 >= EosModel::H2_BLEND_HI - 1e-12 {
            out.push(
                "Fraction H₂ ≥ 25 % : EOS PR-78 (après bande de transition Papay↔PR-78)."
                    .to_string(),
            );
        } else if self.h2 > EosModel::H2_BLEND_LO + 1e-9 {
            out.push(format!(
                "Fraction H₂ dans la bande de transition Papay↔PR-78 ([{:.0} %, {:.0} %]) : Z blendé (smoothstep).",
                EosModel::H2_BLEND_LO * 100.0,
                EosModel::H2_BLEND_HI * 100.0
            ));
        }
        out
    }

    /// Densité [kg/m³] à P(bar), T(K).
    pub fn density_kg_per_m3(&self, pressure_bar: f64, temperature_k: f64) -> f64 {
        let p_pa = pressure_bar.max(0.0) * 1e5;
        let z = self.compressibility(pressure_bar, temperature_k);
        p_pa * self.molar_mass_kg_per_mol() / (z * UNIVERSAL_GAS_CONSTANT * temperature_k.max(1.0))
    }

    /// PCS volumique supérieur du mélange [MJ/Nm³] (ISO 6976, mélange idéal).
    pub fn pcs_mj_per_nm3(&self) -> f64 {
        self.fractions()
            .iter()
            .map(|(c, y)| y * c.pcs_mj_per_nm3)
            .sum()
    }

    /// PCI volumique inférieur du mélange [MJ/Nm³] (ISO 6976, mélange idéal).
    pub fn pci_mj_per_nm3(&self) -> f64 {
        self.fractions()
            .iter()
            .map(|(c, y)| y * c.pci_mj_per_nm3)
            .sum()
    }

    /// Densité relative d = M_gaz / M_air (EN 437, gaz parfait à 15 °C / 1,01325 bar).
    pub fn relative_density_at_standard(&self) -> f64 {
        self.molar_mass_kg_per_mol() / MOLAR_MASS_AIR_KG_PER_MOL
    }

    /// Indice de Wobbe supérieur [MJ/Nm³] (EN 437): PCS / √d.
    pub fn wobbe_mj_per_nm3(&self) -> f64 {
        let d = self.relative_density_at_standard();
        if d <= 1e-12 {
            return 0.0;
        }
        self.pcs_mj_per_nm3() / d.sqrt()
    }

    /// Viscosité dynamique Lee-Gonzalez-Eakin [Pa·s] à (P,T) du mélange.
    ///
    /// Corrélation SPE-1340-PA : μ [cP] = 10⁻⁴ · K · exp(X · ρ^Y),
    /// ρ [g/cm³], T [°R], M [g/mol], Y = 2,4 − 0,2X.
    pub fn dynamic_viscosity_pa_s(&self, pressure_bar: f64, temperature_k: f64) -> f64 {
        lee_gonzalez_eakin_viscosity_pa_s(
            self.molar_mass_kg_per_mol(),
            self.density_kg_per_m3(pressure_bar.max(0.1), temperature_k),
            temperature_k,
        )
    }
}

/// Viscosité Lee-Gonzalez-Eakin [Pa·s] (Lee et al. 1966, SPE-1340-PA).
///
/// μ [cP] = 10⁻⁴ · K · exp(X · ρ^Y) avec ρ [g/cm³], T [°R], M [g/mol].
pub fn lee_gonzalez_eakin_viscosity_pa_s(
    molar_mass_kg_per_mol: f64,
    density_kg_per_m3: f64,
    temperature_k: f64,
) -> f64 {
    let m_g_mol = (molar_mass_kg_per_mol * 1000.0).max(1.0);
    let t_rankine = (temperature_k * 9.0 / 5.0).max(200.0);
    let rho_g_cm3 = density_kg_per_m3 / 1000.0;
    if rho_g_cm3 < 1e-4 {
        // LGE hors domaine (ρ trop faible) : retour à la limite diluée K/(209+19M+T).
        let k = (9.4 + 0.02 * m_g_mol) * t_rankine.powf(1.5) / (209.0 + 19.0 * m_g_mol + t_rankine);
        return ((1e-4 * k) * 1e-3).max(1e-7);
    }
    let rho_g_cm3 = rho_g_cm3.max(1e-6);

    let k = (9.4 + 0.02 * m_g_mol) * t_rankine.powf(1.5) / (209.0 + 19.0 * m_g_mol + t_rankine);
    let x = 3.5 + 986.0 / t_rankine + 0.01 * m_g_mol;
    let y = 2.4 - 0.2 * x;
    let mu_cp = 1e-4 * k * (x * rho_g_cm3.powf(y)).exp();
    (mu_cp * 1e-3).max(1e-7)
}

/// Reynolds pour un tuyau circulaire avec débit **aux conditions normales** (Nm³/s).
///
/// Convention du solveur : Q est en m³/s à 15 °C / 1,01325 bar (voir `SolverResult::flows`).
/// ṁ = ρ_std · Q ⇒ Re = 4 · ρ_std · |Q| / (π · D · μ).
pub fn reynolds_from_standard_flow(
    standard_density_kg_per_m3: f64,
    flow_m3s_at_standard: f64,
    diameter_mm: f64,
    viscosity_pa_s: f64,
) -> f64 {
    let d = (diameter_mm * 1e-3).max(1e-6);
    let mu = viscosity_pa_s.max(1e-7);
    let rho_std = standard_density_kg_per_m3.max(1e-6);
    4.0 * rho_std * flow_m3s_at_standard.abs() / (std::f64::consts::PI * d * mu)
}

/// Densité de l'air sec à 15 °C, 1,01325 bar [kg/m³] (gaz parfait — référence EN 437).
#[allow(dead_code)]
pub fn air_density_kg_per_m3_at_standard() -> f64 {
    let p_pa = STANDARD_PRESSURE_BAR * 1e5;
    p_pa * MOLAR_MASS_AIR_KG_PER_MOL / (UNIVERSAL_GAS_CONSTANT * STANDARD_TEMPERATURE_K)
}

fn compressibility_factor_papay_for_criticals(
    pressure_bar: f64,
    temperature_k: f64,
    critical_pressure_bar: f64,
    critical_temperature_k: f64,
) -> f64 {
    let pr = pressure_bar.max(0.0) / critical_pressure_bar.max(1e-6);
    let tr = (temperature_k / critical_temperature_k.max(1e-6)).max(0.1);
    let z = 1.0 - 3.52 * pr / 10_f64.powf(0.9813 * tr) + 0.274 * pr * pr / 10_f64.powf(0.8157 * tr);
    z.clamp(0.2, 1.5)
}

/// Facteur Z Papay — CH₄ pur (API historique MVP).
pub fn compressibility_factor_papay(pressure_bar: f64, temperature_k: f64) -> f64 {
    GasComposition::pure_ch4().compressibility_papay(pressure_bar, temperature_k)
}

/// Densité CH₄ pur (API historique MVP).
pub fn gas_density_kg_per_m3(pressure_bar: f64, temperature_k: f64) -> f64 {
    gas_density_kg_per_m3_with_composition(pressure_bar, temperature_k, &GasComposition::pure_ch4())
}

/// Densité avec composition explicite.
pub fn gas_density_kg_per_m3_with_composition(
    pressure_bar: f64,
    temperature_k: f64,
    composition: &GasComposition,
) -> f64 {
    composition.density_kg_per_m3(pressure_bar, temperature_k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gas_composition_pure_ch4_matches_legacy() {
        let comp = GasComposition::pure_ch4();
        assert!((comp.molar_mass_kg_per_mol() - LEGACY_MOLAR_MASS_KG_PER_MOL).abs() < 1e-9);
        assert!((comp.pseudo_critical_pressure_bar() - LEGACY_CRITICAL_PRESSURE_BAR).abs() < 1e-9);
        assert!(
            (comp.pseudo_critical_temperature_k() - LEGACY_CRITICAL_TEMPERATURE_K).abs() < 1e-9
        );

        let z_legacy = compressibility_factor_papay(70.0, DEFAULT_GAS_TEMPERATURE_K);
        let z_comp = comp.compressibility_papay(70.0, DEFAULT_GAS_TEMPERATURE_K);
        assert!((z_legacy - z_comp).abs() < 1e-12);

        let rho_legacy = gas_density_kg_per_m3(70.0, DEFAULT_GAS_TEMPERATURE_K);
        let rho_comp = comp.density_kg_per_m3(70.0, DEFAULT_GAS_TEMPERATURE_K);
        assert!((rho_legacy - rho_comp).abs() < 1e-9);
    }

    #[test]
    fn test_h2_blend_reduces_density() {
        let pure = GasComposition::pure_ch4();
        let with_h2 = GasComposition {
            ch4: 0.80,
            h2: 0.20,
            ..GasComposition::pure_ch4()
        }
        .normalize();

        let rho_pure = pure.density_kg_per_m3(70.0, DEFAULT_GAS_TEMPERATURE_K);
        let rho_blend = with_h2.density_kg_per_m3(70.0, DEFAULT_GAS_TEMPERATURE_K);
        assert!(
            rho_blend < rho_pure,
            "20 % H₂ doit réduire la densité: pure={rho_pure}, blend={rho_blend}"
        );
    }

    #[test]
    fn test_h2_blend_reduces_pcs() {
        let pure = GasComposition::pure_ch4();
        let with_h2 = GasComposition {
            ch4: 0.80,
            h2: 0.20,
            ..GasComposition::pure_ch4()
        }
        .normalize();

        assert!(
            with_h2.pcs_mj_per_nm3() < pure.pcs_mj_per_nm3(),
            "20 % H₂ doit réduire le PCS"
        );
        let expected = 0.80 * 39.82 + 0.20 * 12.74;
        assert!((with_h2.pcs_mj_per_nm3() - expected).abs() < 0.01);
    }

    #[test]
    fn test_papay_h2_blend_z_stays_physical_at_transmission_pressure() {
        let with_h2 = GasComposition {
            ch4: 0.80,
            h2: 0.20,
            ..GasComposition::pure_ch4()
        }
        .normalize();
        let z = with_h2.compressibility_papay(70.0, DEFAULT_GAS_TEMPERATURE_K);
        assert!(
            (0.65..=1.15).contains(&z),
            "Z Papay+Kay à 70 bar pour 20 % H₂ hors bornes physiques: {z}"
        );
    }

    #[test]
    fn test_kay_pseudo_critical_pure_ch4_matches_component() {
        let ch4 = GasComposition::pure_ch4();
        assert!(
            (ch4.pseudo_critical_pressure_bar() - GasComponent::CH4.critical_pressure_bar).abs()
                < 1e-9
        );
        assert!(
            (ch4.pseudo_critical_temperature_k() - GasComponent::CH4.critical_temperature_k).abs()
                < 1e-9
        );
    }

    #[test]
    fn test_kay_pseudo_critical_g20_order_of_magnitude() {
        let g20 = GasComposition::g20_nominal();
        // Kay sur G20 : Ppc ~46 bar, Tpc ~200 K (ordre de grandeur mélange gaz de ville).
        assert!(
            (44.0..=48.0).contains(&g20.pseudo_critical_pressure_bar()),
            "Ppc G20 Kay: {}",
            g20.pseudo_critical_pressure_bar()
        );
        assert!(
            (195.0..=210.0).contains(&g20.pseudo_critical_temperature_k()),
            "Tpc G20 Kay: {}",
            g20.pseudo_critical_temperature_k()
        );
    }

    #[test]
    fn test_g20_pci_matches_en437_order_of_magnitude() {
        let g20 = GasComposition::g20_nominal();
        let pci = g20.pci_mj_per_nm3();
        assert!(
            (34.5..=37.5).contains(&pci),
            "PCI G20 hors plage EN 437: {pci}"
        );
        assert!(pci < g20.pcs_mj_per_nm3());
    }

    #[test]
    fn test_g20_pcs_matches_en437_order_of_magnitude() {
        let g20 = GasComposition::g20_nominal();
        let pcs = g20.pcs_mj_per_nm3();
        // EN 437 / GRDF gaz de ville : PCS supérieur typ. 38,5–41 MJ/Nm³.
        assert!(
            (38.5..=41.0).contains(&pcs),
            "PCS G20 hors plage EN 437: {pcs}"
        );
    }

    #[test]
    fn test_g20_wobbe_matches_literature_order_of_magnitude() {
        let g20 = GasComposition::g20_nominal();
        let wobbe = g20.wobbe_mj_per_nm3();
        // EN 437 famille H (gaz de ville) : typ. 46–52 MJ/Nm³ (WI supérieur).
        assert!(
            (45.0..=52.0).contains(&wobbe),
            "Wobbe G20 attendu ~46–52 MJ/Nm³, got {wobbe}"
        );
        // Valeur de référence GRDF/EN 437 ~46 MJ/Nm³ à ±1.
        assert!(
            (45.0..=48.0).contains(&wobbe),
            "Wobbe G20 nominal hors plage GRDF: {wobbe}"
        );
    }

    #[test]
    fn test_wobbe_uses_en437_ideal_relative_density() {
        let g20 = GasComposition::g20_nominal();
        let d = g20.relative_density_at_standard();
        let d_molar = g20.molar_mass_kg_per_mol() / MOLAR_MASS_AIR_KG_PER_MOL;
        assert!(
            (d - d_molar).abs() < 1e-12,
            "EN 437 : d = M_gaz/M_air, got d={d}, molar={d_molar}"
        );
        let wobbe = g20.wobbe_mj_per_nm3();
        let wobbe_en437 = g20.pcs_mj_per_nm3() / d.sqrt();
        assert!(
            (wobbe - wobbe_en437).abs() < 1e-9,
            "Wobbe = PCS/sqrt(d) EN 437"
        );
    }

    #[test]
    fn test_papay_z_reasonable_at_nominal_conditions() {
        let z = compressibility_factor_papay(70.0, DEFAULT_GAS_TEMPERATURE_K);
        assert!(
            (0.7..=1.1).contains(&z),
            "expected Papay Z in realistic range, got {z}"
        );
    }

    #[test]
    fn test_density_increases_with_pressure() {
        let rho_low = gas_density_kg_per_m3(30.0, DEFAULT_GAS_TEMPERATURE_K);
        let rho_high = gas_density_kg_per_m3(70.0, DEFAULT_GAS_TEMPERATURE_K);
        assert!(
            rho_high > rho_low,
            "density should increase with pressure: rho70={rho_high}, rho30={rho_low}"
        );
    }

    #[test]
    fn test_density_nominal_order_of_magnitude() {
        let rho = gas_density_kg_per_m3(70.0, DEFAULT_GAS_TEMPERATURE_K);
        assert!(
            (30.0..=80.0).contains(&rho),
            "unexpected nominal density: {rho}"
        );
    }

    #[test]
    fn test_h2_fraction_monotonically_reduces_pcs_and_density() {
        let mut prev_pcs = f64::MAX;
        let mut prev_rho = f64::MAX;
        for h2_pct in [0.0, 0.05, 0.10, 0.15, 0.20] {
            let comp = GasComposition {
                ch4: 1.0 - h2_pct,
                h2: h2_pct,
                ..GasComposition::pure_ch4()
            }
            .normalize();
            let pcs = comp.pcs_mj_per_nm3();
            let rho = comp.density_kg_per_m3(70.0, DEFAULT_GAS_TEMPERATURE_K);
            assert!(pcs < prev_pcs, "PCS must decrease with H₂ at {h2_pct}");
            assert!(rho < prev_rho, "density must decrease with H₂ at {h2_pct}");
            prev_pcs = pcs;
            prev_rho = rho;
        }
    }

    #[test]
    fn test_composition_normalize_recovers_from_raw_fractions() {
        let raw = GasComposition {
            ch4: 8.0,
            c2h6: 1.0,
            co2: 0.5,
            n2: 0.5,
            h2: 0.0,
        }
        .normalize();
        let sum = raw.ch4 + raw.c2h6 + raw.co2 + raw.n2 + raw.h2;
        assert!((sum - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_dynamic_viscosity_lee_gonzalez() {
        let ch4 = GasComposition::pure_ch4();
        let mu = ch4.dynamic_viscosity_pa_s(70.0, DEFAULT_GAS_TEMPERATURE_K);
        // CH4 à 70 bar, 15 °C : ~1.1e-5 Pa·s (ordre de grandeur pipeline)
        assert!(
            (0.8e-5..=2.5e-5).contains(&mu),
            "viscosité CH4 hors plage attendue: {mu}"
        );
        let g20 = GasComposition::g20_nominal();
        let mu_g20 = g20.dynamic_viscosity_pa_s(70.0, DEFAULT_GAS_TEMPERATURE_K);
        assert!(mu_g20 > 0.0);
    }

    #[test]
    fn test_dynamic_reynolds_varies_with_flow() {
        let g20 = GasComposition::g20_nominal();
        let rho_std = g20.density_kg_per_m3(STANDARD_PRESSURE_BAR, STANDARD_TEMPERATURE_K);
        let mu = g20.dynamic_viscosity_pa_s(70.0, DEFAULT_GAS_TEMPERATURE_K);
        let d_mm = 500.0;
        let re_low = reynolds_from_standard_flow(rho_std, 1.0, d_mm, mu);
        let re_high = reynolds_from_standard_flow(rho_std, 10.0, d_mm, mu);
        assert!(re_high > re_low);
        // Q=1 Nm³/s, DN500, G20 : Re ~ 10⁵ (turbulent), pas 10⁷.
        assert!(
            (5e4..=5e6).contains(&re_low),
            "Re G20 DN500 Q=1 Nm³/s: {re_low}"
        );
    }

    #[test]
    fn test_g20_matches_contract_json() {
        let raw = include_str!("../../../docs/contracts/gas-presets.json");
        let presets: serde_json::Value = serde_json::from_str(raw).expect("parse gas-presets.json");
        let g20_json = &presets["g20_nominal"];
        let contract = GasComposition {
            ch4: g20_json["ch4"].as_f64().expect("ch4"),
            c2h6: g20_json["c2h6"].as_f64().expect("c2h6"),
            co2: g20_json["co2"].as_f64().expect("co2"),
            n2: g20_json["n2"].as_f64().expect("n2"),
            h2: g20_json["h2"].as_f64().expect("h2"),
        };
        let backend = GasComposition::g20_nominal();
        for (a, b) in [
            (backend.ch4, contract.ch4),
            (backend.c2h6, contract.c2h6),
            (backend.co2, contract.co2),
            (backend.n2, contract.n2),
            (backend.h2, contract.h2),
        ] {
            assert!(
                (a - b).abs() < 1e-12,
                "G20 drift vs contract JSON: {a} vs {b}"
            );
        }
    }

    #[test]
    fn test_h2_blend_reduces_re_at_same_normal_flow() {
        let ch4 = GasComposition::pure_ch4();
        let h2_mix = GasComposition {
            ch4: 0.80,
            h2: 0.20,
            ..GasComposition::pure_ch4()
        }
        .normalize();
        let rho_ch4 = ch4.density_kg_per_m3(STANDARD_PRESSURE_BAR, STANDARD_TEMPERATURE_K);
        let rho_h2 = h2_mix.density_kg_per_m3(STANDARD_PRESSURE_BAR, STANDARD_TEMPERATURE_K);
        let mu_ch4 = ch4.dynamic_viscosity_pa_s(70.0, DEFAULT_GAS_TEMPERATURE_K);
        let mu_h2 = h2_mix.dynamic_viscosity_pa_s(70.0, DEFAULT_GAS_TEMPERATURE_K);
        let q = 5.0;
        let d_mm = 600.0;
        let re_ch4 = reynolds_from_standard_flow(rho_ch4, q, d_mm, mu_ch4);
        let re_h2 = reynolds_from_standard_flow(rho_h2, q, d_mm, mu_h2);
        assert!(
            re_h2 < re_ch4,
            "20 % H₂ : Re(Nm³/s) plus bas à Q constant (ρ_std↓ domine μ↓): Re_CH4={re_ch4}, Re_H2={re_h2}"
        );
        assert!(
            re_ch4 > 1e5 && re_h2 > 1e5,
            "régime turbulent maintenu pour les deux mélanges"
        );
    }

    #[test]
    fn test_physics_warnings_h2_above_20_percent() {
        let blend = GasComposition {
            ch4: 0.75,
            h2: 0.25,
            ..GasComposition::pure_ch4()
        }
        .normalize();
        let warnings = blend.physics_warnings();
        assert!(
            warnings.iter().any(|w| w.contains("PR-78") || w.contains("25 %")),
            "H₂=25 % doit signaler PR-78: {warnings:?}"
        );
        let mid = composition_with_h2(0.20);
        assert!(
            mid.physics_warnings()
                .iter()
                .any(|w| w.contains("transition") || w.contains("blend")),
            "H₂=20 % doit signaler la bande de transition: {:?}",
            mid.physics_warnings()
        );
    }

    #[test]
    fn test_gas_composition_serde_roundtrip() {
        let comp = GasComposition::g20_nominal();
        let json = serde_json::to_string(&comp).expect("serialize");
        let back: GasComposition = serde_json::from_str(&json).expect("deserialize");
        assert!((back.ch4 - comp.ch4).abs() < 1e-12);
        assert!((back.h2 - comp.h2).abs() < 1e-12);
    }

    fn composition_with_h2(h2: f64) -> GasComposition {
        GasComposition {
            ch4: 1.0 - h2,
            h2,
            ..GasComposition::pure_ch4()
        }
        .normalize()
    }

    /// T5 : continuité ρ(H₂) — bande de blend Papay↔PR-78 [15 %, 25 %] (smoothstep).
    #[test]
    fn test_eos_h2_continuity_at_20_percent_threshold() {
        // Continuité locale autour de 20 % : saut << 1 % grâce au blend.
        for p_bar in [70.0, 30.0] {
            let rho_lo = composition_with_h2(0.199).density_kg_per_m3(p_bar, DEFAULT_GAS_TEMPERATURE_K);
            let rho_hi = composition_with_h2(0.201).density_kg_per_m3(p_bar, DEFAULT_GAS_TEMPERATURE_K);
            let jump = (rho_hi - rho_lo).abs() / rho_lo.max(1e-6);
            eprintln!("H₂ blend P={p_bar} bar: ρ(19.9%)={rho_lo:.4}, ρ(20.1%)={rho_hi:.4}, jump={jump:.5}");
            assert!(
                jump < 0.01,
                "P={p_bar}: saut ρ sur ΔH₂=0,2 pt doit rester < 1 % avec blend (got {jump:.4})"
            );
        }

        // Continuité sur toute la bande [15 %, 25 %] : max saut local ≤ 2 %.
        for p_bar in [70.0, 30.0] {
            let samples: Vec<f64> = (0..=20).map(|i| 0.15 + 0.005 * i as f64).collect();
            let mut max_jump = 0.0_f64;
            for w in samples.windows(2) {
                let a = composition_with_h2(w[0]).density_kg_per_m3(p_bar, DEFAULT_GAS_TEMPERATURE_K);
                let b = composition_with_h2(w[1]).density_kg_per_m3(p_bar, DEFAULT_GAS_TEMPERATURE_K);
                max_jump = max_jump.max((b - a).abs() / a.max(1e-6));
            }
            eprintln!("H₂ blend band P={p_bar}: max local jump={max_jump:.5}");
            assert!(
                max_jump < 0.02,
                "P={p_bar}: max jump local sur bande [15,25] % doit être < 2 % (got {max_jump:.4})"
            );
        }

        // Hors bande : Papay pur (10 %) et PR-78 pur (30 %) sans warning de transition.
        assert!(composition_with_h2(0.10).physics_warnings().is_empty());
        assert!(
            composition_with_h2(0.30)
                .physics_warnings()
                .iter()
                .any(|w| w.contains("PR-78"))
        );
    }

    /// T5b : facteur Z Papay+Kay borné dans [0.2, 1.5] sur grille P×H₂.
    #[test]
    fn test_eos_z_clamp_on_pressure_h2_grid() {
        for p_bar in [1.0, 10.0, 30.0, 70.0, 100.0] {
            for h2_pct in [0.0, 0.05, 0.10, 0.15, 0.20, 0.25, 0.30] {
                let comp = composition_with_h2(h2_pct);
                let z = comp.compressibility_papay(p_bar, DEFAULT_GAS_TEMPERATURE_K);
                assert!(
                    (0.2..=1.5).contains(&z),
                    "Z out of clamp [0.2, 1.5] at P={p_bar} bar, H₂={h2_pct}: Z={z}"
                );
            }
        }
    }

    /// T5c : monotonie ρ(P) pour CH₄ pur.
    #[test]
    fn test_eos_ch4_density_monotone_in_pressure() {
        let comp = GasComposition::pure_ch4();
        let pressures = [1.0, 10.0, 30.0, 50.0, 70.0, 100.0];
        let mut prev_rho = 0.0;
        for &p in &pressures {
            let rho = comp.density_kg_per_m3(p, DEFAULT_GAS_TEMPERATURE_K);
            assert!(
                rho > prev_rho,
                "CH₄ density must increase with pressure: P={p} bar, ρ={rho}, prev={prev_rho}"
            );
            prev_rho = rho;
        }
    }
}
