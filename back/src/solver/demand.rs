//! Profils de demande thermosensibles et journaliers (P9).
//!
//! ## Hypothèses physiques explicites
//!
//! - $T_{\mathrm{ext}}$ : température **air extérieur** pour la thermosensibilité client ;
//!   elle ne modifie pas $T$ du gaz dans le réseau (isotherme 288,15 K).
//! - Débits en Nm³/h et Nm³/s aux conditions normales (15 °C, 1,01325 bar).
//!
//! ## Modèle thermique (thermosensibilité distributeur)
//!
//! Socle $Q_0$ (ECS, cuisson, procédé continu) + part chauffage active si $T_{\mathrm{ext}} < T_{\mathrm{seuil}}$ :
//!
//! $$Q_{\mathrm{chauff}}(T_{\mathrm{ext}}) = \min\!\bigl(\alpha \max(0,\; T_{\mathrm{seuil}} - T_{\mathrm{ext}}),\; Q_{\mathrm{chauff,max}}\bigr)$$
//!
//! $$Q_{\mathrm{ref}}(T_{\mathrm{ext}}) = Q_0 + Q_{\mathrm{chauff}}(T_{\mathrm{ext}}) \quad [\mathrm{Nm}^3/\mathrm{h}]$$
//!
//! $Q_{\mathrm{ref}}$ est le **débit horaire moyen** sur la journée (avant profil journalier).
//!
//! ## Profil journalier
//!
//! $$Q_h = Q_{\mathrm{ref}}(T_{\mathrm{ext}})\, m_h$$
//!
//! avec $m_h = w_h / \bar w$, $\bar w = \frac{1}{24}\sum_k w_k$, donc $\frac{1}{24}\sum_h m_h = 1$.
//! Relation : $m_h = 24\, s_h$, $s_h = w_h / \sum_k w_k$.
//!
//! Si $T_{\mathrm{ext}} \ge T_{\mathrm{seuil}}$ : $Q_{\mathrm{ref}} = Q_0$ → seul le socle est modulé (pas de chauffage).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Catégorie client (profils types).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientCategory {
    Residential,
    Tertiary,
    Industrial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DayType {
    #[default]
    Weekday,
    Weekend,
}

/// Profil de demande paramétrique pour un nœud de soutirage (agrégé zone / poste).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandProfile {
    /// Socle hors chauffage [Nm³/h] (ECS, cuisson, procédé continu).
    pub q0_m3h: f64,
    /// Gradient chauffage [Nm³/h/°C] (degrés-jours linéaires).
    pub alpha_m3h_per_c: f64,
    /// Température extérieure de coupure chauffage [°C] (17 °C courant modèles distributeur, zones H1–H2).
    pub t_threshold_c: f64,
    /// Plafond optionnel de la part chauffage [Nm³/h] (saturation froid extrême).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_heating_m3h: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<ClientCategory>,
    #[serde(default)]
    pub day_type: DayType,
    /// Poids horaires relatifs (0–23). Seuls les rapports $w_h/\bar w$ comptent.
    #[serde(default)]
    pub daily_weights: Option<[f64; 24]>,
}

impl DemandProfile {
    pub fn new(q0_m3h: f64, alpha_m3h_per_c: f64, t_threshold_c: f64) -> Self {
        Self {
            q0_m3h,
            alpha_m3h_per_c,
            t_threshold_c,
            max_heating_m3h: None,
            category: None,
            day_type: DayType::Weekday,
            daily_weights: None,
        }
    }

    pub fn from_category(category: ClientCategory) -> Self {
        // Ordres de grandeur pour un point de livraison / nœud de soutirage agrégé (pas un logement).
        match category {
            ClientCategory::Residential => Self {
                q0_m3h: 45.0,
                alpha_m3h_per_c: 7.5,
                t_threshold_c: 17.0,
                max_heating_m3h: Some(220.0),
                category: Some(category),
                day_type: DayType::Weekday,
                daily_weights: None,
            },
            ClientCategory::Tertiary => Self {
                q0_m3h: 55.0,
                alpha_m3h_per_c: 3.0,
                t_threshold_c: 17.0,
                max_heating_m3h: Some(120.0),
                category: Some(category),
                day_type: DayType::Weekday,
                daily_weights: None,
            },
            ClientCategory::Industrial => Self {
                q0_m3h: 150.0,
                alpha_m3h_per_c: 0.0,
                t_threshold_c: 15.0,
                max_heating_m3h: None,
                category: Some(category),
                day_type: DayType::Weekday,
                daily_weights: None,
            },
        }
    }

    fn daily_weights(&self) -> [f64; 24] {
        if let Some(weights) = self.daily_weights {
            return weights;
        }
        match (self.category, self.day_type) {
            (Some(ClientCategory::Tertiary), DayType::Weekend) => weekend_winter_tertiary_weights(),
            (Some(ClientCategory::Tertiary), DayType::Weekday) => weekday_winter_tertiary_weights(),
            (Some(ClientCategory::Industrial), _) => flat_daily_weights(),
            (_, DayType::Weekend) => weekend_winter_weights(),
            (_, DayType::Weekday) => weekday_winter_weights(),
        }
    }

    /// Part chauffage seule [Nm³/h], nulle si $T_{\mathrm{ext}} \ge T_{\mathrm{seuil}}$.
    pub fn heating_demand_m3h(&self, t_ext_c: f64) -> f64 {
        let delta = (self.t_threshold_c - t_ext_c).max(0.0);
        let linear = (self.alpha_m3h_per_c * delta).max(0.0);
        match self.max_heating_m3h {
            Some(cap) if cap.is_finite() && cap >= 0.0 => linear.min(cap),
            _ => linear,
        }
    }

    /// Débit horaire moyen [Nm³/h] : $Q_0 + Q_{\mathrm{chauff}}(T_{\mathrm{ext}})$.
    pub fn reference_demand_m3h(&self, t_ext_c: f64) -> f64 {
        self.q0_m3h + self.heating_demand_m3h(t_ext_c)
    }

    /// Part chauffage (alias explicite).
    pub fn thermal_demand_m3h(&self, t_ext_c: f64) -> f64 {
        self.heating_demand_m3h(t_ext_c)
    }

    /// Part journalière $s_h = w_h^+ / \sum_k w_k^+$ avec $w_h^+=\max(0,w_h)$ (Σ s_h = 1 si Σ w_k^+ > 0).
    /// Alors $m_h = 24\, s_h$ lorsque $\sum_k w_k = \sum_k w_k^+$ (poids ≥ 0).
    pub fn daily_share(&self, hour: u8) -> f64 {
        let h = (hour as usize).min(23);
        let weights = self.daily_weights();
        let sum_pos: f64 = weights.iter().map(|w| w.max(0.0)).sum();
        if sum_pos <= 0.0 {
            return 1.0 / 24.0;
        }
        weights[h].max(0.0) / sum_pos
    }

    /// Multiplicateur horaire $m_h = w_h^+ / \bar w$ avec $\bar w = \frac{1}{24}\sum_k w_k$.
    /// Si $w_k \ge 0$ : $(1/24)\sum_h m_h = 1$ et $m_h = 24\, s_h$.
    pub fn hourly_multiplier(&self, hour: u8) -> f64 {
        let h = (hour as usize).min(23);
        let weights = self.daily_weights();
        let sum: f64 = weights.iter().sum();
        if sum <= 0.0 {
            return 1.0;
        }
        let mean = sum / 24.0;
        let w_h = weights[h].max(0.0);
        if mean <= 0.0 {
            return 1.0;
        }
        w_h / mean
    }

    /// Débit à l'heure $h$ [Nm³/h] : $Q_{\mathrm{ref}}(T_{\mathrm{ext}})\, m_h$.
    pub fn demand_m3h(&self, t_ext_c: f64, hour: u8) -> f64 {
        self.reference_demand_m3h(t_ext_c) * self.hourly_multiplier(hour)
    }

    /// Soutirage réseau [Nm³/s] (négatif).
    pub fn withdrawal_m3s(&self, t_ext_c: f64, hour: u8) -> f64 {
        -(self.demand_m3h(t_ext_c, hour) / 3600.0)
    }
}

/// Parts journalières $s_h$ avec $w_h^+=\max(0,w_h)$, $\sum_h s_h = 1$.
pub fn normalized_daily_shares(weights: &[f64; 24]) -> [f64; 24] {
    let sum: f64 = weights.iter().map(|w| w.max(0.0)).sum();
    if sum <= 0.0 {
        return [1.0 / 24.0; 24];
    }
    let mut out = [0.0; 24];
    for (i, w) in weights.iter().enumerate() {
        out[i] = w.max(0.0) / sum;
    }
    out
}

/// Poids horaires $w'_h$ renormalisés avec $\sum_h w'_h = 24$ (même $m_h$ qu'à l'origine).
pub fn normalize_daily_weights(weights: &[f64; 24]) -> [f64; 24] {
    let sum: f64 = weights.iter().map(|w| w.max(0.0)).sum();
    if sum <= 0.0 {
        return [1.0; 24];
    }
    let mut out = [0.0; 24];
    for (i, w) in weights.iter().enumerate() {
        out[i] = w.max(0.0) / sum * 24.0;
    }
    out
}

/// Résout les demandes nodales à partir des profils, météo et heure.
pub fn resolve_demands(
    profiles: &HashMap<String, DemandProfile>,
    t_ext_c: f64,
    hour: u8,
) -> anyhow::Result<HashMap<String, f64>> {
    if hour > 23 {
        anyhow::bail!("invalid hour {hour} (expected 0–23)");
    }
    if !t_ext_c.is_finite() {
        anyhow::bail!("non-finite T_ext");
    }
    let mut out = HashMap::with_capacity(profiles.len());
    for (node_id, profile) in profiles {
        let w = profile.withdrawal_m3s(t_ext_c, hour);
        if !w.is_finite() {
            anyhow::bail!("non-finite demand for node '{node_id}' at hour {hour}");
        }
        out.insert(node_id.clone(), w);
    }
    Ok(out)
}

/// Profil journalier hiver semaine résidentiel (corpus `daily-profiles.yaml`, Σ w_h = 24).
pub fn weekday_winter_weights() -> [f64; 24] {
    [
        0.67, 0.56, 0.45, 0.45, 0.56, 0.78, 1.12, 1.45, 1.23, 1.01, 0.89, 0.89, 1.01, 0.89, 0.89,
        1.01, 1.12, 1.34, 1.56, 1.67, 1.45, 1.23, 1.01, 0.76,
    ]
}

/// Profil journalier hiver semaine tertiaire (occupation 8h–19h, nuit atténuée, Σ w_h = 24).
pub fn weekday_winter_tertiary_weights() -> [f64; 24] {
    [
        0.42, 0.36, 0.31, 0.31, 0.36, 0.57, 0.94, 1.30, 1.61, 1.82, 1.82, 1.61, 1.51, 1.40, 1.51,
        1.51, 1.40, 1.30, 1.14, 0.88, 0.62, 0.52, 0.42, 0.36,
    ]
}

/// Profil journalier hiver week-end résidentiel (matin atténué, mi-journée renforcée, Σ w_h = 24).
pub fn weekend_winter_weights() -> [f64; 24] {
    [
        0.71, 0.61, 0.51, 0.51, 0.61, 0.81, 0.98, 1.13, 1.10, 1.03, 0.98, 1.00, 1.06, 1.03, 1.00,
        1.06, 1.16, 1.32, 1.49, 1.57, 1.40, 1.18, 0.96, 0.79,
    ]
}

/// Profil journalier hiver week-end tertiaire (même tendance, Σ w_h = 24).
pub fn weekend_winter_tertiary_weights() -> [f64; 24] {
    [
        0.45, 0.39, 0.34, 0.34, 0.39, 0.57, 0.92, 1.24, 1.48, 1.63, 1.68, 1.61, 1.56, 1.51, 1.53,
        1.51, 1.42, 1.27, 1.12, 0.92, 0.68, 0.57, 0.47, 0.40,
    ]
}

pub fn flat_daily_weights() -> [f64; 24] {
    [1.0; 24]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_unnormalized_daily_weights() -> [f64; 24] {
        [
            2.0, 4.0, 6.0, 8.0, 1.0, 3.0, 5.0, 7.0, 2.0, 4.0, 6.0, 8.0, 1.0, 3.0, 5.0, 7.0,
            2.0, 4.0, 6.0, 8.0, 1.0, 3.0, 5.0, 7.0,
        ]
    }

    #[test]
    fn test_summer_hourly_demand_modulates_base_only() {
        let p = DemandProfile::from_category(ClientCategory::Residential);
        let q_summer = p.demand_m3h(20.0, 12);
        let expected = p.q0_m3h * p.hourly_multiplier(12);
        assert!(
            (q_summer - expected).abs() < 1e-9,
            "été : Q_h = Q_0 m_h sans chauffage, got {q_summer}, expected {expected}"
        );
    }

    #[test]
    fn test_preset_residential_heating_capped_at_extreme_cold() {
        let p = DemandProfile::from_category(ClientCategory::Residential);
        assert!((p.heating_demand_m3h(-25.0) - 220.0).abs() < 1e-9);
    }

    #[test]
    fn test_negative_alpha_gives_no_heating() {
        let p = DemandProfile::new(10.0, -5.0, 17.0);
        assert!(p.heating_demand_m3h(5.0).abs() < 1e-12);
    }

    #[test]
    fn test_no_heating_at_or_above_threshold() {
        let p = DemandProfile::new(10.0, 80.0, 17.0);
        assert!(p.heating_demand_m3h(17.0).abs() < 1e-12);
        assert!(p.heating_demand_m3h(25.0).abs() < 1e-12);
        assert!((p.reference_demand_m3h(25.0) - 10.0).abs() < 1e-9);
        // En été, alpha élevé ne doit pas affecter la demande.
        let summer_h = p.demand_m3h(25.0, 12);
        let low_alpha = DemandProfile::new(10.0, 0.1, 17.0);
        assert!((summer_h - low_alpha.demand_m3h(25.0, 12)).abs() < 1e-9);
    }

    #[test]
    fn test_max_heating_caps_extreme_cold() {
        let mut p = DemandProfile::new(10.0, 100.0, 17.0);
        p.max_heating_m3h = Some(50.0);
        assert!((p.heating_demand_m3h(-20.0) - 50.0).abs() < 1e-9);
        assert!((p.reference_demand_m3h(-20.0) - 60.0).abs() < 1e-9);
    }

    #[test]
    fn test_demand_profile_zero_below_threshold() {
        let p = DemandProfile::new(10.0, 5.0, 17.0);
        assert!(p.heating_demand_m3h(20.0).abs() < 1e-9);
        let q = p.reference_demand_m3h(20.0);
        assert!(
            (q - 10.0).abs() < 1e-9,
            "T_ext > seuil → Q_ref = Q_0 uniquement, got {q}"
        );
    }

    #[test]
    fn test_demand_profile_linear_above_threshold() {
        let p = DemandProfile::new(10.0, 5.0, 17.0);
        let q = p.reference_demand_m3h(7.0);
        assert!(
            (q - 60.0).abs() < 1e-9,
            "T_ext = seuil - 10 → Q_ref = Q_0 + 10α, got {q}"
        );
    }

    #[test]
    fn test_normalize_daily_weights_sum_to_24() {
        let raw = sample_unnormalized_daily_weights();
        let normalized = normalize_daily_weights(&raw);
        let sum: f64 = normalized.iter().sum();
        assert!(
            (sum - 24.0).abs() < 1e-9,
            "normalize_daily_weights doit donner Σ w_h = 24, got {sum}"
        );
    }

    #[test]
    fn test_hourly_multiplier_equals_24_times_daily_share() {
        let p = DemandProfile::from_category(ClientCategory::Residential);
        for h in 0u8..24 {
            let share = p.daily_share(h);
            let mult = p.hourly_multiplier(h);
            assert!(
                (mult - 24.0 * share).abs() < 1e-9,
                "m_h = 24 s_h, h={h}, mult={mult}, share={share}"
            );
        }
    }

    #[test]
    fn test_hourly_multiplier_invariant_under_positive_scaling() {
        let base = weekday_winter_weights();
        let scaled: [f64; 24] = std::array::from_fn(|i| base[i] * 3.0);
        let profile_base = DemandProfile {
            q0_m3h: 10.0,
            alpha_m3h_per_c: 1.0,
            t_threshold_c: 17.0,
            max_heating_m3h: None,
            category: None,
            day_type: DayType::Weekday,
            daily_weights: Some(base),
        };
        let profile_scaled = DemandProfile {
            daily_weights: Some(scaled),
            ..profile_base.clone()
        };
        for h in 0u8..24 {
            assert!(
                (profile_base.hourly_multiplier(h) - profile_scaled.hourly_multiplier(h)).abs()
                    < 1e-9,
                "m_h(w) = m_h(c w) pour c > 0, h={h}"
            );
        }
    }

    #[test]
    fn test_negative_weight_clamped_in_hourly_multiplier() {
        let mut weights = [1.0; 24];
        weights[5] = -2.0;
        let profile = DemandProfile {
            q0_m3h: 10.0,
            alpha_m3h_per_c: 0.0,
            t_threshold_c: 17.0,
            max_heating_m3h: None,
            category: None,
            day_type: DayType::Weekday,
            daily_weights: Some(weights),
        };
        assert!(profile.hourly_multiplier(5).abs() < 1e-12);
        let sum: f64 = weights.iter().sum();
        assert!(
            (profile.hourly_multiplier(6) - 1.0 / (sum / 24.0)).abs() < 1e-9,
            "hors validation API : m_h = w_h^+ / (Σ w_k / 24)"
        );
        assert!(
            (profile.hourly_multiplier(6) - 24.0 * profile.daily_share(6)).abs() > 1e-6,
            "avec poids négatifs m_h ≠ 24 s_h (Σ w_k ≠ Σ w_k^+)"
        );
    }

    #[test]
    fn test_normalize_daily_weights_preserves_hourly_multipliers() {
        // Σ w_h = 108 ≠ 24 : vérifie l'invariance m_h, pas seulement le cas corpus.
        let raw = sample_unnormalized_daily_weights();
        let normalized = normalize_daily_weights(&raw);
        let profile_raw = DemandProfile {
            q0_m3h: 10.0,
            alpha_m3h_per_c: 1.0,
            t_threshold_c: 17.0,
            max_heating_m3h: None,
            category: None,
            day_type: DayType::Weekday,
            daily_weights: Some(raw),
        };
        let profile_norm = DemandProfile {
            daily_weights: Some(normalized),
            ..profile_raw.clone()
        };
        for h in 0u8..24 {
            assert!(
                (profile_raw.hourly_multiplier(h) - profile_norm.hourly_multiplier(h)).abs() < 1e-9,
                "renormalisation Σ=24 ne change pas m_h, h={h}"
            );
        }
    }

    #[test]
    fn test_daily_profile_normalization() {
        let shares = normalized_daily_shares(&weekday_winter_weights());
        let sum: f64 = shares.iter().sum();
        assert!(
            (sum - 1.0).abs() < 1e-9,
            "parts journalières s_h doivent sommer à 1.0, got {sum}"
        );
    }

    #[test]
    fn test_hourly_multipliers_average_to_one() {
        let p = DemandProfile::from_category(ClientCategory::Residential);
        let mean: f64 = (0u8..24).map(|h| p.hourly_multiplier(h)).sum::<f64>() / 24.0;
        assert!(
            (mean - 1.0).abs() < 1e-9,
            "moyenne des multiplicateurs horaires = 1, got {mean}"
        );
    }

    #[test]
    fn test_daily_profile_preserves_reference_mean() {
        let p = DemandProfile::from_category(ClientCategory::Residential);
        let q_ref = p.reference_demand_m3h(-5.0);
        let mean_hourly: f64 = (0u8..24).map(|h| p.demand_m3h(-5.0, h)).sum::<f64>() / 24.0;
        assert!(
            (mean_hourly - q_ref).abs() < 1e-6 * q_ref.max(1.0),
            "moyenne journalière = Q_ref(T), ref={q_ref:.4}, mean={mean_hourly:.4}"
        );
    }

    #[test]
    fn test_flat_profile_gives_constant_hourly_demand() {
        let mut p = DemandProfile::from_category(ClientCategory::Industrial);
        p.daily_weights = Some(flat_daily_weights());
        let q_ref = p.reference_demand_m3h(5.0);
        for h in 0u8..24 {
            assert!(
                (p.demand_m3h(5.0, h) - q_ref).abs() < 1e-9,
                "profil plat → Q_h constant, h={h}"
            );
        }
    }

    #[test]
    fn test_weekday_winter_tertiary_weights_sum_to_24() {
        let sum: f64 = weekday_winter_tertiary_weights().iter().sum();
        assert!(
            (sum - 24.0).abs() < 1e-6,
            "poids tertiaire Σ w_h = 24, got {sum}"
        );
    }

    #[test]
    fn test_tertiary_night_multiplier_below_residential() {
        let res = DemandProfile::from_category(ClientCategory::Residential);
        let ter = DemandProfile::from_category(ClientCategory::Tertiary);
        assert!(
            ter.hourly_multiplier(3) < res.hourly_multiplier(3),
            "tertiaire : charge nocturne plus faible que résidentiel"
        );
        assert!(
            ter.hourly_multiplier(11) > ter.hourly_multiplier(3),
            "tertiaire : pic en journée ouvrée"
        );
    }

    #[test]
    fn test_industrial_preset_has_no_weather_sensitivity() {
        let ind = DemandProfile::from_category(ClientCategory::Industrial);
        assert!(ind.alpha_m3h_per_c.abs() < 1e-12);
        let summer = ind.reference_demand_m3h(30.0);
        let winter = ind.reference_demand_m3h(-10.0);
        assert!((summer - winter).abs() < 1e-9);
    }

    #[test]
    fn test_weekday_winter_weights_sum_to_24() {
        let sum: f64 = weekday_winter_weights().iter().sum();
        assert!(
            (sum - 24.0).abs() < 1e-6,
            "poids corpus Σ w_h = 24 (moyenne unitaire), got {sum}"
        );
    }

    #[test]
    fn test_weekend_winter_weights_sum_to_24() {
        let sum: f64 = weekend_winter_weights().iter().sum();
        assert!(
            (sum - 24.0).abs() < 1e-6,
            "poids week-end Σ w_h = 24, got {sum}"
        );
    }

    #[test]
    fn test_weekend_tertiary_weights_sum_to_24() {
        let sum: f64 = weekend_winter_tertiary_weights().iter().sum();
        assert!(
            (sum - 24.0).abs() < 1e-6,
            "poids tertiaire week-end Σ w_h = 24, got {sum}"
        );
    }

    #[test]
    fn test_day_type_weekend_reduces_morning_peak_for_residential() {
        let weekday = DemandProfile::from_category(ClientCategory::Residential);
        let mut weekend = weekday.clone();
        weekend.day_type = DayType::Weekend;
        assert!(weekend.hourly_multiplier(7) < weekday.hourly_multiplier(7));
        assert!(weekend.hourly_multiplier(12) > weekday.hourly_multiplier(12));
    }

    #[test]
    fn test_day_type_weekend_applies_to_tertiary() {
        let weekday = DemandProfile::from_category(ClientCategory::Tertiary);
        let mut weekend = weekday.clone();
        weekend.day_type = DayType::Weekend;
        assert!(weekend.hourly_multiplier(8) < weekday.hourly_multiplier(8));
        assert!(weekend.hourly_multiplier(12) > weekday.hourly_multiplier(12));
    }

    #[test]
    fn test_m_h_equals_24_times_s_h_for_weekend_profile() {
        let mut p = DemandProfile::from_category(ClientCategory::Residential);
        p.day_type = DayType::Weekend;
        for h in 0u8..24 {
            let share = p.daily_share(h);
            let mult = p.hourly_multiplier(h);
            assert!(
                (mult - 24.0 * share).abs() < 1e-9,
                "week-end: m_h = 24 s_h, h={h}, mult={mult}, share={share}"
            );
        }
    }

    #[test]
    fn test_weekend_profile_preserves_reference_daily_mean() {
        let mut p = DemandProfile::from_category(ClientCategory::Tertiary);
        p.day_type = DayType::Weekend;
        let q_ref = p.reference_demand_m3h(-3.0);
        let mean_hourly: f64 = (0u8..24).map(|h| p.demand_m3h(-3.0, h)).sum::<f64>() / 24.0;
        assert!(
            (mean_hourly - q_ref).abs() < 1e-6 * q_ref.max(1.0),
            "week-end: moyenne journalière = Q_ref, ref={q_ref:.4}, mean={mean_hourly:.4}"
        );
    }

    #[test]
    fn test_resolve_demands_residential_winter() {
        let mut profiles = HashMap::new();
        profiles.insert(
            "SK".to_string(),
            DemandProfile::from_category(ClientCategory::Residential),
        );
        let morning = resolve_demands(&profiles, -5.0, 7).expect("morning");
        let night = resolve_demands(&profiles, -5.0, 2).expect("night");
        let q_morning = -morning["SK"];
        let q_night = -night["SK"];
        assert!(
            q_morning > q_night * 1.5,
            "7h hiver doit dépasser 2h (matin={q_morning:.4}, nuit={q_night:.4} Nm³/s)"
        );
        assert!(
            q_morning > 0.01,
            "demande résidentielle hiver significative"
        );
    }
}
