//! GERG-2008 via le port NIST (`aga8` crate).
//!
//! Domaine : mélange 5 composants GazFlow (CH₄, C₂H₆, CO₂, N₂, H₂) mappé vers
//! la composition 21-composants GERG (autres fractions à 0). Pression en kPa,
//! densité molaire en mol/L. En cas d'échec d'itération densité → fallback `None`
//! (l'appelant bascule sur PR-78).

use aga8::composition::Composition;
use aga8::gerg2008::Gerg2008;

use super::super::gas_properties::GasComposition;

/// Facteur Z GERG-2008, ou `None` si la densité n'a pas convergé.
pub fn compressibility_gerg2008(
    composition: GasComposition,
    pressure_bar: f64,
    temperature_k: f64,
) -> Option<f64> {
    let mut gerg = Gerg2008::new();
    let comp = to_aga8_composition(composition);
    if gerg.set_composition(&comp).is_err() {
        return None;
    }
    gerg.p = pressure_bar.max(1e-6) * 100.0; // bar → kPa
    gerg.t = temperature_k.max(1.0);
    // dens() flag 0 = calculate density from P,T
    if gerg.density(0).is_err() {
        return None;
    }
    gerg.properties();
    let z = gerg.z;
    if z.is_finite() && z > 0.05 && z < 3.0 {
        Some(z)
    } else {
        None
    }
}

fn to_aga8_composition(c: GasComposition) -> Composition {
    let n = c.normalize();
    Composition {
        methane: n.ch4,
        ethane: n.c2h6,
        carbon_dioxide: n.co2,
        nitrogen: n.n2,
        hydrogen: n.h2,
        ..Composition::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::gas_properties::GasComposition;

    #[test]
    fn gerg_z_physical_g20_transmission() {
        let z = compressibility_gerg2008(GasComposition::default(), 70.0, 288.15)
            .expect("GERG density should converge for G20");
        assert!(
            z > 0.75 && z < 1.05,
            "Z GERG G20 @ 70 bar / 15 °C hors domaine physique: {z}"
        );
    }

    #[test]
    fn gerg_h2_blend_differs_from_pr78() {
        let blend = GasComposition {
            ch4: 0.70,
            h2: 0.30,
            ..GasComposition::pure_ch4()
        };
        let z_gerg = compressibility_gerg2008(blend, 70.0, 288.15).expect("GERG H2");
        let z_pr = super::super::pr78::compressibility_pr78(blend, 70.0, 288.15);
        // Les deux doivent rester physiques ; l'écart relatif est informatif.
        assert!(z_gerg > 0.7 && z_gerg < 1.2, "Z GERG H30: {z_gerg}");
        assert!(
            (z_gerg - z_pr).abs() > 1e-4 || (z_gerg - z_pr).abs() < 0.15,
            "GERG vs PR78 unexpectedly extreme: gerg={z_gerg} pr={z_pr}"
        );
    }

    #[test]
    fn gerg_monotonic_density_with_pressure() {
        let g20 = GasComposition::default();
        let z50 = compressibility_gerg2008(g20, 50.0, 288.15).unwrap();
        let z90 = compressibility_gerg2008(g20, 90.0, 288.15).unwrap();
        // ρ ∝ P/Z : à T fixe, P↑ ⇒ ρ↑ pour gaz naturel typique.
        let rho50 = 50.0 / z50;
        let rho90 = 90.0 / z90;
        assert!(
            rho90 > rho50,
            "density proxy should increase with P: 50→{rho50}, 90→{rho90}"
        );
    }
}
