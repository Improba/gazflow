#!/usr/bin/env bash
set -euo pipefail

# Validation pack scientifique backend (T1 -> T16)
# - Exécute les tests ciblés du protocole + invariants endurcissement
# - Optionnellement régénère la référence interne GasLib-11
# - Optionnellement lance les smoke tests grands réseaux
# - GAZFLOW_REQUIRE_GASLIB_DATA=1 par défaut (fail si dat/ GasLib absent)

export GAZFLOW_REQUIRE_GASLIB_DATA="${GAZFLOW_REQUIRE_GASLIB_DATA:-1}"

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

run_step "T1 darcy" cargo test -p gazflow-back --lib darcy_friction_turbulent
run_step "T2 resistance" cargo test -p gazflow-back --lib pipe_resistance_positive
run_step "T3 two-nodes" cargo test -p gazflow-back --lib steady_state_two_nodes
run_step "T3b closed-form-P2" cargo test -p gazflow-back --lib test_two_node_closed_form_p_squared
run_step "T4 y-network" cargo test -p gazflow-back --lib steady_state_y_network_mass_conservation
run_step "T5 newton-vs-jacobi" cargo test -p gazflow-back --lib test_newton_vs_jacobi_same_result
run_step "T6 gaslib-11 smoke" cargo test -p gazflow-back --lib test_solve_gaslib_11
run_step "T7 units" cargo test -p gazflow-back --lib test_units_scn_to_si
run_step "T8 dimensional-consistency" cargo test -p gazflow-back --lib test_pressure_drop_dimension_consistency
run_step "T9 quarantined (ignored)" cargo test -p gazflow-back --lib test_gaslib_11_vs_reference_solution
run_step "T10 sensitivity" cargo test -p gazflow-back --lib test_sensitivity_physical_trends
run_step "T11 mass-balance-GasLib" env GAZFLOW_REQUIRE_GASLIB_DATA=1 cargo test -p gazflow-back --lib mass_balance_gaslib
run_step "T12 linepack-capacitance" cargo test -p gazflow-back --lib linepack_capacitance
run_step "T13 pde-mass-balance" cargo test -p gazflow-back --lib test_pde_
run_step "T14 eos-h2" cargo test -p gazflow-back --lib eos_
run_step "T15 conductance-pde" cargo test -p gazflow-back --lib segment_conductance
run_step "T16 gravity" cargo test -p gazflow-back --lib test_gravity_

if [[ "${GAZFLOW_RUN_LARGE_SMOKE:-0}" == "1" ]]; then
  run_step "large-smoke" env GAZFLOW_ENABLE_LARGE_DATASET_TESTS=1 cargo test -p gazflow-back --lib test_solve_gaslib_ -- --nocapture
fi

echo ""
echo "========================================="
echo " Validation pack terminé avec succès"
echo "========================================="
