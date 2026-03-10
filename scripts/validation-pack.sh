#!/usr/bin/env bash
set -euo pipefail

# Validation pack scientifique backend (T1 -> T10)
# - Exécute les tests ciblés du protocole
# - Optionnellement régénère la référence interne GasLib-11
# - Optionnellement lance les smoke tests grands réseaux

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."
BACK="$ROOT/back"

run_step() {
  local label="$1"
  shift
  echo ""
  echo ">>> [$label]"
  "$@"
}

echo "========================================="
echo " GazFlow validation pack (backend)"
echo "========================================="
echo "Root: $ROOT"

cd "$BACK"

if [[ "${GAZFLOW_REGEN_REFERENCE:-0}" == "1" ]]; then
  run_step "regen-reference" cargo run --bin generate_gaslib11_reference
fi

run_step "T1 darcy" cargo test darcy_friction_turbulent
run_step "T2 resistance" cargo test pipe_resistance_positive
run_step "T3 two-nodes" cargo test steady_state_two_nodes
run_step "T4 y-network" cargo test steady_state_y_network_mass_conservation
run_step "T5 newton-vs-jacobi" cargo test test_newton_vs_jacobi_same_result
run_step "T6 gaslib-11 smoke" cargo test test_solve_gaslib_11
run_step "T7 units" cargo test test_units_scn_to_si
run_step "T8 dimensional-consistency" cargo test test_pressure_drop_dimension_consistency
run_step "T9 reference-comparison" cargo test test_gaslib_11_vs_reference_solution -- --nocapture
run_step "T10 sensitivity" cargo test test_sensitivity_physical_trends

if [[ "${GAZFLOW_RUN_LARGE_SMOKE:-0}" == "1" ]]; then
  run_step "large-smoke" env GAZFLOW_ENABLE_LARGE_DATASET_TESTS=1 cargo test test_solve_gaslib_ -- --nocapture
fi

echo ""
echo "========================================="
echo " Validation pack terminé avec succès"
echo "========================================="
