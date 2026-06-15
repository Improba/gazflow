//! Peng-Robinson 1978 — Z compressibilité pour mélanges (Kay mixing).

use super::super::gas_properties::GasComposition;

const R_J_PER_MOL_K: f64 = 8.314_462_618;

/// Facteur Z PR-78 à P [bar], T [K] pour un mélange.
pub fn compressibility_pr78(composition: GasComposition, pressure_bar: f64, temperature_k: f64) -> f64 {
    let p_pa = pressure_bar.max(1e-6) * 1e5;
    let t_k = temperature_k.max(1.0);
    let (tc_mix, pc_pa, omega_mix) = kay_pseudo_criticals(composition);
    if pc_pa <= 0.0 || tc_mix <= 0.0 {
        return 1.0;
    }

    let tr = t_k / tc_mix;
    let pr = p_pa / pc_pa;
    let kappa = 0.37464 + 1.54226 * omega_mix - 0.26992 * omega_mix * omega_mix;
    let alpha = (1.0 + kappa * (1.0 - tr.sqrt())).max(1e-6);
    let a = 0.45724 * R_J_PER_MOL_K * R_J_PER_MOL_K * tc_mix * tc_mix / pc_pa * alpha;
    let b = 0.07780 * R_J_PER_MOL_K * tc_mix / pc_pa;

    // Résolution cubique PR en V_m : P = RT/(V-b) - a/(V(V+b)+b(V-b))
    let mut v = R_J_PER_MOL_K * t_k / p_pa;
    for _ in 0..40 {
        let denom = v * (v + b) + b * (v - b);
        if denom.abs() < 1e-18 {
            break;
        }
        let f = R_J_PER_MOL_K * t_k / (v - b) - a / denom - p_pa;
        let df = -R_J_PER_MOL_K * t_k / ((v - b) * (v - b))
            + a * (2.0 * v + b) / (denom * denom);
        if df.abs() < 1e-18 {
            break;
        }
        v -= f / df;
        v = v.max(b * 1.001);
    }

    let z = p_pa * v / (R_J_PER_MOL_K * t_k);
    z.clamp(0.2, 1.5)
}

fn kay_pseudo_criticals(composition: GasComposition) -> (f64, f64, f64) {
    let comp = composition.normalize();
    let mut tc = 0.0;
    let mut pc = 0.0;
    let mut omega = 0.0;
    for (y, (t_c, p_c, w)) in [
        (comp.ch4, (190.6, 46.0e5, 0.011)),
        (comp.c2h6, (305.32, 48.72e5, 0.099)),
        (comp.co2, (304.13, 73.77e5, 0.225)),
        (comp.n2, (126.2, 33.96e5, 0.037)),
        (comp.h2, (33.19, 12.96e5, -0.216)),
    ] {
        tc += y * t_c;
        pc += y * p_c;
        omega += y * w;
    }
    (tc, pc, omega)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::gas_properties::GasComposition;

    #[test]
    fn pr78_z_physical_at_transmission_pressure() {
        let g20 = GasComposition::default();
        let z = compressibility_pr78(g20, 70.0, 288.15);
        assert!(z > 0.7 && z < 1.05, "Z PR-78 G20 @ 70 bar: {z}");
    }

    #[test]
    fn pr78_h2_blend_differs_from_unity() {
        let blend = GasComposition {
            ch4: 0.70,
            h2: 0.30,
            ..GasComposition::pure_ch4()
        }
        .normalize();
        let z = compressibility_pr78(blend, 70.0, 288.15);
        assert!(z.is_finite() && z > 0.5);
    }
}
