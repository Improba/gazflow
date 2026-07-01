#!/usr/bin/env bash
# Bench reproductible GasLib-582 / nomination_mild_618 (Phase I).
# Usage: ./scripts/bench-gaslib-582.sh [tag]   → écrit /tmp/582-<tag>.json
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TAG="${1:-nominal}"
OUT="${GAZFLOW_582_BENCH_OUT:-/tmp/582-${TAG}.json}"

cd "$ROOT/back"
cargo build --release --bin compressor_diag

export GAZFLOW_COMPRESSOR_MAP_MODE="${GAZFLOW_COMPRESSOR_MAP_MODE:-measurement}"
export GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT="${GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT:-0}"
export GAZFLOW_NEWTON_COMPRESSOR_HEAD_JAC="${GAZFLOW_NEWTON_COMPRESSOR_HEAD_JAC:-0}"
export GAZFLOW_COMPRESSOR_ENTHALPIC="${GAZFLOW_COMPRESSOR_ENTHALPIC:-0}"
export GAZFLOW_COMPRESSOR_ENERGY_CLOSURE="${GAZFLOW_COMPRESSOR_ENERGY_CLOSURE:-0}"
export GAZFLOW_COMPRESSOR_ENERGY_EQUATION="${GAZFLOW_COMPRESSOR_ENERGY_EQUATION:-0}"

if [[ "$TAG" == "phase-ibis" ]]; then
  export GAZFLOW_SCENARIO_PRESSURE_ENVELOPES="${GAZFLOW_SCENARIO_PRESSURE_ENVELOPES:-1}"
  export GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON="${GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON:-0}"
  export GAZFLOW_TRANSPORT_MINIMAL_ANCHORS="${GAZFLOW_TRANSPORT_MINIMAL_ANCHORS:-1}"
  export GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES="${GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES:-0}"
else
  export GAZFLOW_SCENARIO_PRESSURE_ENVELOPES="${GAZFLOW_SCENARIO_PRESSURE_ENVELOPES:-0}"
  export GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON="${GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON:-0}"
  export GAZFLOW_TRANSPORT_MINIMAL_ANCHORS="${GAZFLOW_TRANSPORT_MINIMAL_ANCHORS:-0}"
  export GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES="${GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES:-4}"
fi

echo "bench: tag=$TAG map=$GAZFLOW_COMPRESSOR_MAP_MODE contract_refine=$GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT head_jac=$GAZFLOW_NEWTON_COMPRESSOR_HEAD_JAC enthalpic=$GAZFLOW_COMPRESSOR_ENTHALPIC energy_closure=$GAZFLOW_COMPRESSOR_ENERGY_CLOSURE energy_equation=$GAZFLOW_COMPRESSOR_ENERGY_EQUATION envelopes=$GAZFLOW_SCENARIO_PRESSURE_ENVELOPES in_newton=$GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON minimal_anchors=$GAZFLOW_TRANSPORT_MINIMAL_ANCHORS refine_passes=$GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES"
echo "out: $OUT"

./target/release/compressor_diag GasLib-582 --json "$OUT"

python3 - <<PY
import json, sys
d = json.load(open("$OUT"))
mb = d.get("nomination_mass_balance") or d.get("mass_balance") or {}
print("status:", d.get("status"))
print("residual:", d.get("residual"))
print("nomination_worst:", mb.get("worst_free_node"), mb.get("max_free_imbalance_m3s"))
print("contract_relaxed:", d.get("contract_flow_relaxed"))
flags = d.get("flags") or {}
print("envelopes:", flags.get("scenario_pressure_envelopes"))
print("minimal_anchors:", flags.get("transport_minimal_anchors"))
pv = mb.get("pressure_violations") or []
print("pressure_violations:", len(pv), pv[:3] if pv else [])
PY
