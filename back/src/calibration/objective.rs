use std::collections::HashMap;

use crate::{graph::GasNetwork, solver};

use super::{
    CalibrationParameter, MeasurementType, ScadaMeasurement, apply_calibration_parameter_set,
};

const CALIBRATION_SOLVE_MAX_ITER: usize = 1000;
const CALIBRATION_SOLVE_TOLERANCE: f64 = 5e-4;

pub fn compute_residuals(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    params: &CalibrationParameter,
) -> Vec<f64> {
    compute_residuals_with_params(network, demands, measurements, std::slice::from_ref(params))
}

pub fn compute_residuals_with_params(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    params: &[CalibrationParameter],
) -> Vec<f64> {
    compute_measurement_predictions_with_params(network, demands, measurements, params)
        .into_iter()
        .map(|(observed, predicted)| observed - predicted)
        .collect()
}

pub(crate) fn compute_measurement_predictions(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    params: &CalibrationParameter,
) -> Vec<(f64, f64)> {
    compute_measurement_predictions_with_params(network, demands, measurements, std::slice::from_ref(params))
}

pub(crate) fn compute_measurement_predictions_with_params(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    params: &[CalibrationParameter],
) -> Vec<(f64, f64)> {
    let (calibrated_network, scaled_demands) =
        apply_calibration_parameter_set(network, demands, params);
    let solved = solver::solve_steady_state_with_composition(
        &calibrated_network,
        &scaled_demands,
        solver::GasComposition::default(),
        CALIBRATION_SOLVE_MAX_ITER,
        CALIBRATION_SOLVE_TOLERANCE,
    );
    let Ok(solution) = solved else {
        return Vec::new();
    };

    measurements
        .iter()
        .filter_map(|measurement| {
            let predicted = match measurement.measurement_type {
                MeasurementType::Pressure => solution.pressures.get(&measurement.id).copied(),
                MeasurementType::Flow => solution.flows.get(&measurement.id).copied(),
            }?;
            if !predicted.is_finite() {
                return None;
            }
            Some((measurement.value, predicted))
        })
        .collect()
}

/// Incertitudes σ alignées sur l'ordre de `compute_measurement_predictions`.
pub(crate) fn aligned_uncertainties(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    params: &CalibrationParameter,
) -> Vec<f64> {
    aligned_uncertainties_with_params(network, demands, measurements, std::slice::from_ref(params))
}

pub(crate) fn aligned_uncertainties_with_params(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    params: &[CalibrationParameter],
) -> Vec<f64> {
    let (calibrated_network, scaled_demands) =
        apply_calibration_parameter_set(network, demands, params);
    let solved = solver::solve_steady_state_with_composition(
        &calibrated_network,
        &scaled_demands,
        solver::GasComposition::default(),
        CALIBRATION_SOLVE_MAX_ITER,
        CALIBRATION_SOLVE_TOLERANCE,
    );
    let Ok(solution) = solved else {
        return Vec::new();
    };

    measurements
        .iter()
        .filter_map(|measurement| {
            let predicted = match measurement.measurement_type {
                MeasurementType::Pressure => solution.pressures.get(&measurement.id).copied(),
                MeasurementType::Flow => solution.flows.get(&measurement.id).copied(),
            }?;
            if !predicted.is_finite() {
                return None;
            }
            Some(measurement.uncertainty.unwrap_or(0.0))
        })
        .collect()
}
