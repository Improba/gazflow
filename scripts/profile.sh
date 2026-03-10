#!/usr/bin/env bash
set -euo pipefail

# Profiling helper for backend solver (flamegraph-first).
# Usage:
#   ./scripts/profile.sh [bench_filter]
# Example:
#   ./scripts/profile.sh steady_state_newton_parallel_n_threads

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."
BACK="$ROOT/back"
BENCH_FILTER="${1:-steady_state_newton_parallel_n_threads}"
OUT_DIR="$BACK/target/profile"
mkdir -p "$OUT_DIR"

echo "Profiling benchmark filter: $BENCH_FILTER"
cd "$BACK"

if cargo flamegraph --help >/dev/null 2>&1; then
  OUT_FILE="$OUT_DIR/flamegraph-${BENCH_FILTER}-$(date +%Y%m%d-%H%M%S).svg"
  echo "Using cargo flamegraph -> $OUT_FILE"
  cargo flamegraph --bench solver_bench --output "$OUT_FILE" -- "$BENCH_FILTER"
  echo "Done: $OUT_FILE"
  exit 0
fi

if command -v perf >/dev/null 2>&1 && command -v inferno-flamegraph >/dev/null 2>&1; then
  TS="$(date +%Y%m%d-%H%M%S)"
  PERF_DATA="$OUT_DIR/perf-${BENCH_FILTER}-${TS}.data"
  PERF_SCRIPT="$OUT_DIR/perf-${BENCH_FILTER}-${TS}.script"
  OUT_FILE="$OUT_DIR/flamegraph-${BENCH_FILTER}-${TS}.svg"

  echo "Using perf + inferno-flamegraph"
  cargo bench --bench solver_bench --no-run

  BENCH_BIN=""
  for candidate in target/release/deps/solver_bench-*; do
    if [[ -x "$candidate" && ! "$candidate" =~ \.d$ ]]; then
      BENCH_BIN="$candidate"
      break
    fi
  done
  if [[ -z "$BENCH_BIN" ]]; then
    echo "Unable to locate solver_bench executable in target/release/deps"
    exit 1
  fi

  perf record -F 99 -g -o "$PERF_DATA" -- "$BENCH_BIN" "$BENCH_FILTER"
  perf script -i "$PERF_DATA" > "$PERF_SCRIPT"
  inferno-flamegraph < "$PERF_SCRIPT" > "$OUT_FILE"
  echo "Done: $OUT_FILE"
  exit 0
fi

echo "No flamegraph toolchain found."
echo "Install either:"
echo "  - cargo flamegraph (recommended), or"
echo "  - perf + inferno-flamegraph"
echo
echo "Fallback benchmark command:"
echo "  cd back && cargo bench --bench solver_bench -- \"$BENCH_FILTER\""
