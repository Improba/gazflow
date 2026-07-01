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

echo "bench: tag=$TAG map=$GAZFLOW_COMPRESSOR_MAP_MODE contract_refine=$GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT head_jac=$GAZFLOW_NEWTON_COMPRESSOR_HEAD_JAC enthalpic=$GAZFLOW_COMPRESSOR_ENTHALPIC"
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
PY
