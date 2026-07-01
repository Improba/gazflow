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
export GAZFLOW_SCENARIO_PRESSURE_CLAMP="${GAZFLOW_SCENARIO_PRESSURE_CLAMP:-0}"

case "$TAG" in
  strict-newton)
    export GAZFLOW_COMPRESSOR_STRICT_NEWTON=1
    export GAZFLOW_SCENARIO_PRESSURE_ENVELOPES="${GAZFLOW_SCENARIO_PRESSURE_ENVELOPES:-0}"
    export GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON="${GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON:-0}"
    export GAZFLOW_TRANSPORT_MINIMAL_ANCHORS="${GAZFLOW_TRANSPORT_MINIMAL_ANCHORS:-0}"
    export GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES="${GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES:-4}"
    ;;
  strict-newton-envelopes)
    export GAZFLOW_COMPRESSOR_STRICT_NEWTON=1
    export GAZFLOW_SCENARIO_PRESSURE_ENVELOPES=1
    export GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON=0
    export GAZFLOW_TRANSPORT_MINIMAL_ANCHORS=0
    export GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES=4
    ;;
  phase-ibis-in-newton)
    export GAZFLOW_COMPRESSOR_STRICT_NEWTON=0
    export GAZFLOW_SCENARIO_PRESSURE_ENVELOPES="${GAZFLOW_SCENARIO_PRESSURE_ENVELOPES:-1}"
    export GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON="${GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON:-1}"
    export GAZFLOW_TRANSPORT_MINIMAL_ANCHORS="${GAZFLOW_TRANSPORT_MINIMAL_ANCHORS:-0}"
    export GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES="${GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES:-4}"
    ;;
  phase-ibis-nominal-anchors)
    export GAZFLOW_SCENARIO_PRESSURE_ENVELOPES="${GAZFLOW_SCENARIO_PRESSURE_ENVELOPES:-1}"
    export GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON="${GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON:-0}"
    export GAZFLOW_TRANSPORT_MINIMAL_ANCHORS="${GAZFLOW_TRANSPORT_MINIMAL_ANCHORS:-0}"
    export GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES="${GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES:-4}"
    ;;
  phase-ibis)
    export GAZFLOW_SCENARIO_PRESSURE_ENVELOPES="${GAZFLOW_SCENARIO_PRESSURE_ENVELOPES:-1}"
    export GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON="${GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON:-0}"
    export GAZFLOW_TRANSPORT_MINIMAL_ANCHORS="${GAZFLOW_TRANSPORT_MINIMAL_ANCHORS:-1}"
    export GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES="${GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES:-0}"
    ;;
  *)
    export GAZFLOW_COMPRESSOR_STRICT_NEWTON="${GAZFLOW_COMPRESSOR_STRICT_NEWTON:-0}"
    export GAZFLOW_SCENARIO_PRESSURE_ENVELOPES="${GAZFLOW_SCENARIO_PRESSURE_ENVELOPES:-0}"
    export GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON="${GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON:-0}"
    export GAZFLOW_TRANSPORT_MINIMAL_ANCHORS="${GAZFLOW_TRANSPORT_MINIMAL_ANCHORS:-0}"
    export GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES="${GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES:-4}"
    ;;
esac

echo "bench: tag=$TAG map=$GAZFLOW_COMPRESSOR_MAP_MODE strict=$GAZFLOW_COMPRESSOR_STRICT_NEWTON envelopes=$GAZFLOW_SCENARIO_PRESSURE_ENVELOPES in_newton=$GAZFLOW_SCENARIO_PRESSURE_IN_NEWTON clamp=$GAZFLOW_SCENARIO_PRESSURE_CLAMP minimal_anchors=$GAZFLOW_TRANSPORT_MINIMAL_ANCHORS refine_passes=$GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES"
echo "out: $OUT"

./target/release/compressor_diag GasLib-582 --json "$OUT" || true

python3 - <<PY
import json, sys
d = json.load(open("$OUT"))
mb = d.get("nomination_mass_balance") or d.get("mass_balance") or {}
print("status:", d.get("status"))
print("residual:", d.get("residual"))
print("error:", d.get("error"))
print("nomination_worst:", mb.get("worst_free_node"), mb.get("max_free_imbalance_m3s"))
print("contract_relaxed:", d.get("contract_flow_relaxed"))
flags = d.get("flags") or {}
print("strict:", flags.get("compressor_strict_newton"))
print("envelopes:", flags.get("scenario_pressure_envelopes"))
print("in_newton:", flags.get("scenario_pressure_in_newton"))
pv = mb.get("pressure_violations") or []
print("pressure_violations:", len(pv), pv[:3] if pv else [])
sps = d.get("scenario_pressure_slips") or []
print("scenario_pressure_slips:", len(sps))
for s in sps[:5]:
    print(" ", s["node_id"], "P=", round(s["solved_pressure_bar"],3),
          "shortfall=", round(s["shortfall_bar"],2), "scn=", s.get("from_scenario_envelope"))
PY
