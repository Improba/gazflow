#!/usr/bin/env bash
# Lance plusieurs tags bench-gaslib-582.sh en parallèle (processus indépendants).
#
# Usage:
#   ./scripts/bench-gaslib-582-parallel.sh [jobs] tag1 tag2 ...
#   ./scripts/bench-gaslib-582-parallel.sh 3 phase-ic-dual-contract-smoke nominal phase-ibis
#
# Chaque run utilise déjà Rayon in-process (~6–8 threads). Règle empirique :
#   jobs ≈ floor(nproc / 6)  (ex. 22 cœurs → 3 jobs).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
JOBS="${1:-2}"
if [[ "$JOBS" =~ ^[0-9]+$ ]]; then
  shift
else
  JOBS=2
fi

if [[ $# -eq 0 ]]; then
  echo "usage: $0 [jobs] tag1 tag2 ..." >&2
  echo "ex:    $0 3 phase-ic-dual-contract-smoke nominal phase-ibis-nominal-anchors" >&2
  exit 1
fi

TAGS=("$@")
NPROC="$(nproc 2>/dev/null || echo 4)"
REC=$(( NPROC / 6 ))
REC=$(( REC < 1 ? 1 : REC ))
if (( JOBS > REC )); then
  echo "warn: jobs=$JOBS > recommandé ~$REC (nproc=$NPROC, ~6 threads/run)" >&2
fi

echo "parallel bench: jobs=$JOBS tags=${TAGS[*]} nproc=$NPROC"

cd "$ROOT/back"
cargo build --release --bin compressor_diag

export GAZFLOW_582_SKIP_BUILD=1
export GAZFLOW_RAYON_THREADS="${GAZFLOW_RAYON_THREADS:-6}"

run_one() {
  local tag="$1"
  local log="/tmp/582-${tag}.parallel.log"
  echo "start: $tag → log $log"
  if "$ROOT/scripts/bench-gaslib-582.sh" "$tag" >"$log" 2>&1; then
    echo "done:  $tag"
    tail -n 12 "$log"
  else
    echo "fail:  $tag (voir $log)" >&2
    tail -n 20 "$log" >&2
    return 1
  fi
}
export -f run_one
export ROOT GAZFLOW_582_SKIP_BUILD GAZFLOW_RAYON_THREADS

printf '%s\n' "${TAGS[@]}" | xargs -P "$JOBS" -I{} bash -c 'run_one "$@"' _ {}

echo "parallel bench: terminé (${#TAGS[@]} tags)"
