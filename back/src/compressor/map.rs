use std::cmp::Ordering;

use super::station::{
    BiquadraticCoeffs, QuadraticCurve, StationModel, TurboCompressorModel, TurboMeasurement,
};

/// Point de fonctionnement $(Q, H_{\mathrm{ad}}, n)$ sur la carte.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OperatingPoint {
    pub speed_rpm: f64,
    pub head_kj_per_kg: f64,
    pub flow_m3_s: f64,
}

const NORMAL_PRESSURE_BAR: f64 = 1.01325;
const STANDARD_TEMPERATURE_K: f64 = 288.15;
const DEFAULT_GAMMA: f64 = 1.3;
const DEFAULT_CP_J_PER_KG_K: f64 = 2_180.0;
const DEFAULT_EFFICIENCY: f64 = 0.85;
const SPEED_SEARCH_STEPS: usize = 24;
const TRANSPORT_LIFT_THRESHOLD: f64 = 2.0;

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

pub fn eval_biquadratic_head(coeffs: &BiquadraticCoeffs, q: f64, n: f64) -> f64 {
    let q_terms = [1.0, q, q * q];
    let n_terms = [1.0, n, n * n];
    let mut head = 0.0;
    for (i, q_term) in q_terms.iter().enumerate() {
        for (j, n_term) in n_terms.iter().enumerate() {
            // GasLib order: [a00, a01, a02, a10, a11, a12, a20, a21, a22].
            head += coeffs[i * 3 + j] * q_term * n_term;
        }
    }
    head
}

pub fn eval_quadratic(curve: &QuadraticCurve, q: f64) -> f64 {
    curve[0] + curve[1] * q + curve[2] * q * q
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

fn selected_turbo(station: &StationModel) -> Option<&TurboCompressorModel> {
    station.preferred_turbo()
}

fn selected_map_measurements<'a>(
    station: &'a StationModel,
    turbo: Option<&'a TurboCompressorModel>,
) -> &'a [TurboMeasurement] {
    if let Some(turbo) = turbo {
        let measurements = turbo.map_measurements();
        if !measurements.is_empty() {
            return measurements;
        }
    }
    station.map_measurements()
}

fn selected_surgeline_measurements<'a>(
    station: &'a StationModel,
    turbo: Option<&'a TurboCompressorModel>,
) -> &'a [TurboMeasurement] {
    if let Some(turbo) = turbo
        && !turbo.surgeline_measurements.is_empty()
    {
        return turbo.surgeline_measurements.as_slice();
    }
    station.surgeline_measurements.as_slice()
}

fn selected_speed_bounds(
    station: &StationModel,
    turbo: Option<&TurboCompressorModel>,
) -> Option<(f64, f64)> {
    turbo
        .and_then(TurboCompressorModel::speed_bounds)
        .or_else(|| station.speed_bounds())
}

fn eval_head_for_speed(
    station: &StationModel,
    turbo: Option<&TurboCompressorModel>,
    q_m3_s: f64,
    speed_rpm: f64,
    prefer_biquadratic: bool,
) -> Option<f64> {
    if prefer_biquadratic
        && let Some(coeffs) = turbo.and_then(|t| t.biquadratic_head_coeffs.as_ref())
    {
        return Some(eval_biquadratic_head(coeffs, q_m3_s, speed_rpm));
    }
    interpolate_head(selected_map_measurements(station, turbo), q_m3_s, speed_rpm)
}

fn surge_flow_at_speed(
    station: &StationModel,
    turbo: Option<&TurboCompressorModel>,
    speed_rpm: f64,
) -> Option<f64> {
    let surgeline_measurements = selected_surgeline_measurements(station, turbo);
    if !surgeline_measurements.is_empty() {
        let flows: Vec<(f64, f64)> = surgeline_measurements
            .iter()
            .filter(|m| m.speed_rpm.is_finite() && m.flow_m3_s.is_finite())
            .map(|m| (m.speed_rpm, m.flow_m3_s))
            .collect();
        return interpolate_scalar_on_speed(&flows, speed_rpm);
    }

    let characteristic = selected_map_measurements(station, turbo);
    if characteristic.is_empty() {
        return None;
    }
    let min_flow = characteristic
        .iter()
        .filter(|m| (m.speed_rpm - speed_rpm).abs() <= 1e-6)
        .map(|m| m.flow_m3_s)
        .fold(f64::INFINITY, f64::min);
    if min_flow.is_finite() {
        Some(min_flow * 0.85)
    } else {
        None
    }
}

fn choke_flow_at_speed(
    station: &StationModel,
    turbo: Option<&TurboCompressorModel>,
    speed_rpm: f64,
) -> Option<f64> {
    let characteristic = selected_map_measurements(station, turbo);
    if characteristic.is_empty() {
        return None;
    }
    let max_flow = characteristic
        .iter()
        .filter(|m| (m.speed_rpm - speed_rpm).abs() <= 1e-6)
        .map(|m| m.flow_m3_s)
        .fold(f64::NEG_INFINITY, f64::max);
    if max_flow.is_finite() {
        Some(max_flow * 1.05)
    } else {
        None
    }
}

fn interpolate_scalar_on_speed(samples: &[(f64, f64)], speed_rpm: f64) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    if speed_rpm <= sorted[0].0 {
        return Some(sorted[0].1);
    }
    if speed_rpm >= sorted[sorted.len() - 1].0 {
        return Some(sorted[sorted.len() - 1].1);
    }
    for window in sorted.windows(2) {
        let (s0, v0) = window[0];
        let (s1, v1) = window[1];
        if speed_rpm >= s0 && speed_rpm <= s1 {
            return Some(linear_interp(s0, v0, s1, v1, speed_rpm));
        }
    }
    None
}

fn is_flow_feasible(
    station: &StationModel,
    turbo: Option<&TurboCompressorModel>,
    speed_rpm: f64,
    q_m3_s: f64,
) -> bool {
    let Some(q_surge) = surge_flow_at_speed(station, turbo, speed_rpm) else {
        return true;
    };
    let q_choke = choke_flow_at_speed(station, turbo, speed_rpm).unwrap_or(f64::INFINITY);
    q_m3_s + 1e-9 >= q_surge && q_m3_s <= q_choke + 1e-9
}

fn is_head_within_bounds(
    turbo: Option<&TurboCompressorModel>,
    q_m3_s: f64,
    head_kj_per_kg: f64,
) -> bool {
    let Some(turbo) = turbo else {
        return true;
    };

    if let Some(surge) = turbo.surgeline_curve.as_ref()
        && head_kj_per_kg > eval_quadratic(surge, q_m3_s) + 1e-9
    {
        return false;
    }
    if let Some(choke) = turbo.chokeline_curve.as_ref()
        && head_kj_per_kg + 1e-9 < eval_quadratic(choke, q_m3_s)
    {
        return false;
    }
    true
}

/// Recherche 1D de la vitesse $n$ pour un débit d'aspiration donné, dans l'enveloppe surge/choke.
pub fn find_operating_point_for_mode(
    station: &StationModel,
    ctx: &CompressorOperatingContext,
    prefer_biquadratic: bool,
) -> Option<OperatingPoint> {
    let q_m3_s = ctx.suction_volumetric_flow_m3s();
    if q_m3_s <= 0.0 || !q_m3_s.is_finite() {
        return None;
    }

    let turbo = selected_turbo(station);
    let characteristic = selected_map_measurements(station, turbo);
    let has_biquadratic = prefer_biquadratic
        && turbo
            .and_then(|t| t.biquadratic_head_coeffs.as_ref())
            .is_some();
    if characteristic.is_empty() && !has_biquadratic {
        return None;
    }

    let (n_min, n_max) = selected_speed_bounds(station, turbo)?;
    if (n_max - n_min).abs() <= f64::EPSILON {
        let head = eval_head_for_speed(station, turbo, q_m3_s, n_min, prefer_biquadratic)?;
        if !is_head_within_bounds(turbo, q_m3_s, head) {
            return None;
        }
        return Some(OperatingPoint {
            speed_rpm: n_min,
            head_kj_per_kg: head,
            flow_m3_s: q_m3_s,
        });
    }

    let mut best: Option<OperatingPoint> = None;
    for i in 0..SPEED_SEARCH_STEPS {
        let alpha = i as f64 / (SPEED_SEARCH_STEPS.saturating_sub(1).max(1) as f64);
        let speed = n_min + alpha * (n_max - n_min);
        if !is_flow_feasible(station, turbo, speed, q_m3_s) {
            continue;
        }
        let Some(head) = eval_head_for_speed(station, turbo, q_m3_s, speed, prefer_biquadratic)
        else {
            continue;
        };
        if !is_head_within_bounds(turbo, q_m3_s, head) {
            continue;
        }
        let candidate = OperatingPoint {
            speed_rpm: speed,
            head_kj_per_kg: head,
            flow_m3_s: q_m3_s,
        };
        if best.as_ref().is_none_or(|b| head > b.head_kj_per_kg) {
            best = Some(candidate);
        }
    }

    if best.is_some() {
        return best;
    }

    let target_speed = representative_speed_rpm(
        characteristic,
        selected_speed_bounds(station, turbo),
        q_m3_s,
    )?;
    let head = eval_head_for_speed(station, turbo, q_m3_s, target_speed, prefer_biquadratic)?;
    if !is_head_within_bounds(turbo, q_m3_s, head) {
        return None;
    }
    Some(OperatingPoint {
        speed_rpm: target_speed,
        head_kj_per_kg: head,
        flow_m3_s: q_m3_s,
    })
}

pub fn find_operating_point(
    station: &StationModel,
    ctx: &CompressorOperatingContext,
) -> Option<OperatingPoint> {
    find_operating_point_for_mode(station, ctx, false)
}

fn ratio_from_head(head_kj_per_kg: f64, ctx: &CompressorOperatingContext, stages: usize) -> f64 {
    let single_stage_ratio = had_to_pressure_ratio(
        head_kj_per_kg,
        ctx.p_in_bar.max(1e-3),
        ctx.t_in_k.max(1.0),
        DEFAULT_GAMMA,
        DEFAULT_CP_J_PER_KG_K,
        DEFAULT_EFFICIENCY,
    )
    .clamp(1.0, 5.0);
    single_stage_ratio
        .powi(stages.max(1) as i32)
        .clamp(1.0, 5.0)
}

/// Ratio effectif avec garde sur le nominal `.net` pour les stations transport.
pub fn effective_ratio_with_nominal_for_mode(
    station: &StationModel,
    ctx: &CompressorOperatingContext,
    nominal_ratio: Option<f64>,
    prefer_biquadratic: bool,
) -> f64 {
    let stages = station
        .serial_stages_for_conf(station.default_conf_id())
        .max(1);
    let fallback = nominal_ratio
        .unwrap_or_else(|| stage_ratio_heuristic(stages))
        .clamp(1.0, 5.0);

    let q_m3_s = ctx.suction_volumetric_flow_m3s();
    if q_m3_s <= 0.0 || !q_m3_s.is_finite() {
        return fallback;
    }

    let Some(point) = find_operating_point_for_mode(station, ctx, prefer_biquadratic) else {
        return fallback;
    };

    let map_ratio = ratio_from_head(point.head_kj_per_kg, ctx, stages);
    let Some(nominal) = nominal_ratio.filter(|r| *r >= TRANSPORT_LIFT_THRESHOLD) else {
        return map_ratio;
    };

    if map_ratio < nominal * 0.5 {
        (0.35 * map_ratio + 0.65 * nominal).clamp(1.0, 5.0)
    } else {
        map_ratio.clamp(1.0, nominal.max(map_ratio))
    }
}

pub fn effective_ratio_with_nominal(
    station: &StationModel,
    ctx: &CompressorOperatingContext,
    nominal_ratio: Option<f64>,
) -> f64 {
    effective_ratio_with_nominal_for_mode(station, ctx, nominal_ratio, false)
}

fn representative_speed_rpm(
    measurements: &[TurboMeasurement],
    speed_bounds: Option<(f64, f64)>,
    q_m3_s: f64,
) -> Option<f64> {
    if measurements.is_empty() {
        return speed_bounds.map(|(min, max)| 0.5 * (min + max));
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
        .or_else(|| speed_bounds.map(|(min, max)| 0.5 * (min + max)))
}

fn stage_ratio_heuristic(stages: usize) -> f64 {
    (1.08_f64).powi(stages.max(1) as i32).clamp(1.0, 5.0)
}

pub fn effective_ratio_from_operating_point(
    station: &StationModel,
    ctx: &CompressorOperatingContext,
) -> f64 {
    effective_ratio_with_nominal_for_mode(station, ctx, None, false)
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
        CompressorOperatingContext, effective_ratio_from_flow,
        effective_ratio_from_operating_point, effective_ratio_with_nominal,
        effective_ratio_with_nominal_for_mode, eval_biquadratic_head, eval_quadratic,
        find_operating_point, find_operating_point_for_mode, had_to_pressure_ratio,
        interpolate_head,
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
    fn test_eval_biquadratic_head_uses_gaslib_matrix_order() {
        let coeffs = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let head = eval_biquadratic_head(&coeffs, 2.0, 3.0);
        assert_abs_diff_eq!(head, 628.0, epsilon = 1e-9);
    }

    #[test]
    fn test_eval_quadratic_curve() {
        let curve = [1.0, 2.0, 3.0];
        let value = eval_quadratic(&curve, 4.0);
        assert_abs_diff_eq!(value, 57.0, epsilon = 1e-9);
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
        station.push_configuration(None, 1);
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
    fn test_find_operating_point_respects_surgeline_flow() {
        let mut station = StationModel::default();
        station.update_speed_min(4_700.0);
        station.update_speed_max(6_500.0);
        for (speed, flow, head) in [(4_700.0, 0.20, 60.0), (6_500.0, 0.40, 90.0)] {
            station.push_surgeline_measurement(TurboMeasurement {
                speed_rpm: speed,
                flow_m3_s: flow,
                head_kj_per_kg: head,
            });
        }
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

        let ctx = CompressorOperatingContext {
            q_m3s_norm: 10.0,
            p_in_bar: 21.0,
            t_in_k: 288.15,
        };
        let point = find_operating_point(&station, &ctx).expect("operating point");
        assert!(point.speed_rpm >= 4_700.0 && point.speed_rpm <= 6_500.0);
        assert!(point.head_kj_per_kg > 80.0);
    }

    #[test]
    fn test_effective_ratio_with_nominal_preserves_transport_lift() {
        let mut station = StationModel::default();
        station.push_configuration(Some("config_2".into()), 1);
        station.update_speed_min(5_600.0);
        station.update_speed_max(5_600.0);
        station.push_characteristic_measurement(TurboMeasurement {
            speed_rpm: 5_600.0,
            flow_m3_s: 0.30,
            head_kj_per_kg: 70.0,
        });

        let ctx = CompressorOperatingContext {
            q_m3s_norm: 0.35,
            p_in_bar: 21.0,
            t_in_k: 288.15,
        };
        let ratio = effective_ratio_with_nominal(&station, &ctx, Some(4.09));
        assert!(
            ratio > 2.0,
            "transport nominal should dominate low map ratio"
        );
    }

    #[test]
    fn test_effective_ratio_from_flow_uses_stage_count() {
        let mut station = StationModel::default();
        station.push_configuration(None, 2);
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

    #[test]
    fn test_biquadratic_mode_prefers_default_conf_turbo() {
        let mut station = StationModel::default();
        station.push_configuration(Some("config_1".into()), 1);
        station.push_configuration(Some("config_2".into()), 1);

        {
            let turbo = station.turbo_mut("config_1");
            turbo.update_speed_min(5_600.0);
            turbo.update_speed_max(5_600.0);
            turbo.set_biquadratic_coeff(0, 10.0);
        }
        {
            let turbo = station.turbo_mut("config_2");
            turbo.update_speed_min(5_600.0);
            turbo.update_speed_max(5_600.0);
            turbo.set_biquadratic_coeff(0, 90.0);
        }

        let ctx = CompressorOperatingContext {
            q_m3s_norm: 1.0,
            p_in_bar: 1.01325,
            t_in_k: 288.15,
        };
        let point = find_operating_point_for_mode(&station, &ctx, true).expect("biquadratic point");
        assert_abs_diff_eq!(point.head_kj_per_kg, 90.0, epsilon = 1e-9);
    }

    #[test]
    fn test_biquadratic_mode_falls_back_to_measurements_without_coeffs() {
        let mut station = StationModel::default();
        station.push_configuration(Some("config_2".into()), 1);
        {
            let turbo = station.turbo_mut("config_2");
            turbo.update_speed_min(5_600.0);
            turbo.update_speed_max(5_600.0);
            turbo.push_characteristic_measurement(TurboMeasurement {
                speed_rpm: 5_600.0,
                flow_m3_s: 0.20,
                head_kj_per_kg: 70.0,
            });
            turbo.push_characteristic_measurement(TurboMeasurement {
                speed_rpm: 5_600.0,
                flow_m3_s: 0.40,
                head_kj_per_kg: 90.0,
            });
        }

        let ctx = CompressorOperatingContext {
            q_m3s_norm: 0.30,
            p_in_bar: 1.01325,
            t_in_k: 288.15,
        };
        let point = find_operating_point_for_mode(&station, &ctx, true).expect("fallback point");
        assert_abs_diff_eq!(point.head_kj_per_kg, 80.0, epsilon = 1e-9);

        let ratio = effective_ratio_with_nominal_for_mode(&station, &ctx, Some(1.2), true);
        assert!(ratio >= 1.0);
    }
}
