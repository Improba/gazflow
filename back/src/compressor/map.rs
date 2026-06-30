use std::cmp::Ordering;

use super::station::{StationModel, TurboMeasurement};

const NORMAL_PRESSURE_BAR: f64 = 1.01325;
const STANDARD_TEMPERATURE_K: f64 = 288.15;
const DEFAULT_GAMMA: f64 = 1.3;
const DEFAULT_CP_J_PER_KG_K: f64 = 2_180.0;
const DEFAULT_EFFICIENCY: f64 = 0.85;

/// Contexte d'exploitation d'une station (débit normal + aspiration réelle).
#[derive(Debug, Clone, Copy)]
pub struct CompressorOperatingContext {
    /// Débit solver [m³/s] aux conditions normales (288,15 K, 1,01325 bar).
    pub q_m3s_norm: f64,
    /// Pression amont réelle [bar].
    pub p_in_bar: f64,
    /// Température gaz amont [K].
    pub t_in_k: f64,
}

impl CompressorOperatingContext {
    pub fn suction_volumetric_flow_m3s(&self) -> f64 {
        if self.q_m3s_norm <= 0.0 || !self.q_m3s_norm.is_finite() {
            return 0.0;
        }
        let p_in = self.p_in_bar.max(1e-3);
        let t_in = self.t_in_k.max(1.0);
        self.q_m3s_norm * (NORMAL_PRESSURE_BAR / p_in) * (t_in / STANDARD_TEMPERATURE_K)
    }
}

pub fn interpolate_head(
    measurements: &[TurboMeasurement],
    q_m3_s: f64,
    speed_rpm: f64,
) -> Option<f64> {
    if measurements.is_empty() || !q_m3_s.is_finite() || !speed_rpm.is_finite() {
        return None;
    }

    let mut sorted = measurements.to_vec();
    sorted.sort_by(|a, b| {
        a.speed_rpm
            .total_cmp(&b.speed_rpm)
            .then(a.flow_m3_s.total_cmp(&b.flow_m3_s))
    });

    let mut isolines: Vec<(f64, Vec<TurboMeasurement>)> = Vec::new();
    for measurement in sorted {
        if let Some((speed, points)) = isolines.last_mut() {
            if (measurement.speed_rpm - *speed).abs() <= 1e-6 {
                points.push(measurement);
                continue;
            }
        }
        isolines.push((measurement.speed_rpm, vec![measurement]));
    }

    let lower_idx = isolines
        .iter()
        .rposition(|(speed, _)| *speed <= speed_rpm)
        .unwrap_or(0);
    let upper_idx = isolines
        .iter()
        .position(|(speed, _)| *speed >= speed_rpm)
        .unwrap_or_else(|| isolines.len().saturating_sub(1));

    let (lower_speed, lower_points) = &isolines[lower_idx];
    let lower_head = interpolate_head_on_isoline(lower_points, q_m3_s)?;

    if lower_idx == upper_idx {
        return Some(lower_head);
    }

    let (upper_speed, upper_points) = &isolines[upper_idx];
    let upper_head = interpolate_head_on_isoline(upper_points, q_m3_s)?;
    Some(linear_interp(
        *lower_speed,
        lower_head,
        *upper_speed,
        upper_head,
        speed_rpm,
    ))
}

fn interpolate_head_on_isoline(points: &[TurboMeasurement], q_m3_s: f64) -> Option<f64> {
    if points.is_empty() {
        return None;
    }
    if points.len() == 1 {
        return Some(points[0].head_kj_per_kg);
    }

    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| match a.flow_m3_s.total_cmp(&b.flow_m3_s) {
        Ordering::Equal => a.head_kj_per_kg.total_cmp(&b.head_kj_per_kg),
        ord => ord,
    });

    if q_m3_s <= sorted[0].flow_m3_s {
        return Some(sorted[0].head_kj_per_kg);
    }
    if q_m3_s >= sorted[sorted.len() - 1].flow_m3_s {
        return Some(sorted[sorted.len() - 1].head_kj_per_kg);
    }

    for window in sorted.windows(2) {
        let left = &window[0];
        let right = &window[1];
        if q_m3_s >= left.flow_m3_s && q_m3_s <= right.flow_m3_s {
            return Some(linear_interp(
                left.flow_m3_s,
                left.head_kj_per_kg,
                right.flow_m3_s,
                right.head_kj_per_kg,
                q_m3_s,
            ));
        }
    }

    None
}

fn linear_interp(x0: f64, y0: f64, x1: f64, y1: f64, x: f64) -> f64 {
    if (x1 - x0).abs() <= f64::EPSILON {
        return y0;
    }
    let alpha = ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
    y0 + alpha * (y1 - y0)
}

pub fn had_to_pressure_ratio(
    head_kj_per_kg: f64,
    p_in_bar: f64,
    t_in_k: f64,
    gamma: f64,
    cp_j_per_kg_k: f64,
    eta: f64,
) -> f64 {
    if !head_kj_per_kg.is_finite()
        || !p_in_bar.is_finite()
        || !t_in_k.is_finite()
        || !gamma.is_finite()
        || !cp_j_per_kg_k.is_finite()
        || !eta.is_finite()
        || head_kj_per_kg <= 0.0
        || p_in_bar <= 0.0
        || t_in_k <= 0.0
        || cp_j_per_kg_k <= 0.0
        || eta <= 0.0
        || gamma <= 1.0
    {
        return 1.0;
    }

    let head_j_per_kg = head_kj_per_kg * 1_000.0;
    let base = 1.0 + (head_j_per_kg * eta) / (cp_j_per_kg_k * t_in_k);
    if base <= 0.0 {
        return 1.0;
    }
    base.powf(gamma / (gamma - 1.0)).max(1.0)
}

fn representative_speed_rpm(station: &StationModel, q_m3_s: f64) -> Option<f64> {
    let measurements = station.map_measurements();
    if measurements.is_empty() {
        return station.speed_bounds().map(|(min, max)| 0.5 * (min + max));
    }

    let mut speeds: Vec<f64> = measurements
        .iter()
        .map(|m| m.speed_rpm)
        .filter(|s| s.is_finite())
        .collect();
    speeds.sort_by(|a, b| a.total_cmp(b));
    speeds.dedup_by(|a, b| (*a - *b).abs() <= 1e-6);

    if speeds.is_empty() {
        return None;
    }

    let mut best: Option<(f64, f64)> = None;
    for speed in speeds {
        let isoline: Vec<_> = measurements
            .iter()
            .filter(|m| (m.speed_rpm - speed).abs() <= 1e-6)
            .cloned()
            .collect();
        let Some(head) = interpolate_head_on_isoline(&isoline, q_m3_s) else {
            continue;
        };
        let margin = head.max(1e-6);
        if best.is_none_or(|(_, m)| margin > m) {
            best = Some((speed, margin));
        }
    }

    best.map(|(speed, _)| speed)
        .or_else(|| station.speed_bounds().map(|(min, max)| 0.5 * (min + max)))
}

fn stage_ratio_heuristic(stages: usize) -> f64 {
    (1.08_f64).powi(stages.max(1) as i32).clamp(1.0, 5.0)
}

pub fn effective_ratio_from_operating_point(
    station: &StationModel,
    ctx: &CompressorOperatingContext,
) -> f64 {
    let stages = station.max_serial_stages().max(1);
    let fallback = stage_ratio_heuristic(stages);
    let q_m3_s = ctx.suction_volumetric_flow_m3s();
    if q_m3_s <= 0.0 || !q_m3_s.is_finite() {
        return fallback;
    }

    let measurements = station.map_measurements();
    if measurements.is_empty() {
        return fallback;
    }

    let Some(target_speed) = representative_speed_rpm(station, q_m3_s) else {
        return fallback;
    };

    let Some(head_kj_per_kg) = interpolate_head(measurements, q_m3_s, target_speed) else {
        return fallback;
    };

    let single_stage_ratio = had_to_pressure_ratio(
        head_kj_per_kg,
        ctx.p_in_bar.max(1e-3),
        ctx.t_in_k.max(1.0),
        DEFAULT_GAMMA,
        DEFAULT_CP_J_PER_KG_K,
        DEFAULT_EFFICIENCY,
    )
    .clamp(1.0, 5.0);

    single_stage_ratio.powi(stages as i32).clamp(1.0, 5.0)
}

/// Compatibilité tests / appels legacy (pression et température par défaut).
pub fn effective_ratio_from_flow(station: &StationModel, q_m3s_norm: f64) -> f64 {
    effective_ratio_from_operating_point(
        station,
        &CompressorOperatingContext {
            q_m3s_norm: q_m3s_norm.abs(),
            p_in_bar: NORMAL_PRESSURE_BAR,
            t_in_k: STANDARD_TEMPERATURE_K,
        },
    )
}

#[cfg(test)]
mod tests {
    use approx::assert_abs_diff_eq;

    use super::{
        CompressorOperatingContext, effective_ratio_from_flow, effective_ratio_from_operating_point,
        had_to_pressure_ratio, interpolate_head,
    };
    use crate::compressor::station::{StationModel, TurboMeasurement};

    #[test]
    fn test_interpolate_head_bilinear_across_speed_and_flow() {
        let measurements = vec![
            TurboMeasurement {
                speed_rpm: 4_700.0,
                flow_m3_s: 0.20,
                head_kj_per_kg: 60.0,
            },
            TurboMeasurement {
                speed_rpm: 4_700.0,
                flow_m3_s: 0.40,
                head_kj_per_kg: 80.0,
            },
            TurboMeasurement {
                speed_rpm: 6_500.0,
                flow_m3_s: 0.20,
                head_kj_per_kg: 90.0,
            },
            TurboMeasurement {
                speed_rpm: 6_500.0,
                flow_m3_s: 0.40,
                head_kj_per_kg: 110.0,
            },
        ];

        let head = interpolate_head(&measurements, 0.30, 5_600.0).expect("interpolated head");
        assert_abs_diff_eq!(head, 85.0, epsilon = 1e-9);
    }

    #[test]
    fn test_had_to_pressure_ratio_increases_with_inlet_pressure_context() {
        let ratio_low_p = had_to_pressure_ratio(80.0, 1.01325, 288.15, 1.3, 2_180.0, 0.85);
        let ratio_high_p = had_to_pressure_ratio(80.0, 50.0, 288.15, 1.3, 2_180.0, 0.85);
        assert!(ratio_low_p > 1.0);
        assert!(ratio_high_p > 1.0);
        assert!((ratio_low_p - ratio_high_p).abs() < 1e-9);
    }

    #[test]
    fn test_operating_point_uses_suction_volumetric_flow() {
        let mut station = StationModel::default();
        station.push_configuration(1);
        station.update_speed_min(5_600.0);
        station.update_speed_max(5_600.0);
        station.push_characteristic_measurement(TurboMeasurement {
            speed_rpm: 5_600.0,
            flow_m3_s: 0.30,
            head_kj_per_kg: 85.0,
        });
        station.push_characteristic_measurement(TurboMeasurement {
            speed_rpm: 5_600.0,
            flow_m3_s: 0.50,
            head_kj_per_kg: 82.0,
        });

        let ctx_low_p = CompressorOperatingContext {
            q_m3s_norm: 0.40,
            p_in_bar: 1.01325,
            t_in_k: 288.15,
        };
        let ctx_high_p = CompressorOperatingContext {
            q_m3s_norm: 0.40,
            p_in_bar: 40.0,
            t_in_k: 288.15,
        };
        let r_low = effective_ratio_from_operating_point(&station, &ctx_low_p);
        let r_high = effective_ratio_from_operating_point(&station, &ctx_high_p);
        assert!(r_low > 1.0);
        assert!(r_high > 1.0);
        assert_ne!(r_low, r_high);
    }

    #[test]
    fn test_effective_ratio_from_flow_uses_stage_count() {
        let mut station = StationModel::default();
        station.push_configuration(2);
        station.update_speed_min(6_500.0);
        station.update_speed_max(6_500.0);
        station.push_characteristic_measurement(TurboMeasurement {
            speed_rpm: 6_500.0,
            flow_m3_s: 0.38,
            head_kj_per_kg: 88.0,
        });
        station.push_characteristic_measurement(TurboMeasurement {
            speed_rpm: 6_500.0,
            flow_m3_s: 0.90,
            head_kj_per_kg: 84.0,
        });

        let ratio = effective_ratio_from_flow(&station, 0.5);
        assert!(ratio > 1.0);
        assert!(ratio <= 5.0);
    }
}
