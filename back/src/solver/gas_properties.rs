/// Température par défaut (isotherme) utilisée par le MVP [K].
pub const DEFAULT_GAS_TEMPERATURE_K: f64 = 288.15;

// Hypothèses gaz naturel type CH4.
const MOLAR_MASS_KG_PER_MOL: f64 = 0.016_04;
const UNIVERSAL_GAS_CONSTANT: f64 = 8.314_462_618; // J/(mol·K)
const CRITICAL_PRESSURE_BAR: f64 = 46.0;
const CRITICAL_TEMPERATURE_K: f64 = 190.6;

/// Facteur de compressibilité Z via corrélation de Papay.
///
/// Pression en bar, température en K.
pub fn compressibility_factor_papay(pressure_bar: f64, temperature_k: f64) -> f64 {
    let pr = pressure_bar.max(0.0) / CRITICAL_PRESSURE_BAR;
    let tr = (temperature_k / CRITICAL_TEMPERATURE_K).max(0.1);
    let z = 1.0 - 3.52 * pr / 10_f64.powf(0.9813 * tr) + 0.274 * pr * pr / 10_f64.powf(0.8157 * tr);
    // Bornes de sécurité numérique pour éviter les valeurs non physiques.
    z.clamp(0.2, 1.5)
}

/// Densité du gaz [kg/m³] en fonction de P(bar), T(K) et Z(P,T).
pub fn gas_density_kg_per_m3(pressure_bar: f64, temperature_k: f64) -> f64 {
    let p_pa = pressure_bar.max(0.0) * 1e5;
    let z = compressibility_factor_papay(pressure_bar, temperature_k);
    p_pa * MOLAR_MASS_KG_PER_MOL / (z * UNIVERSAL_GAS_CONSTANT * temperature_k.max(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
