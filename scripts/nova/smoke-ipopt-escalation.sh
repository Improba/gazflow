#!/usr/bin/env bash
# Smoke escalade IPOPT NoVa (feature nlp-ipopt) — sans installer coinor-libipopt-dev sur l'hôte.
#
# Prérequis :
#   - Docker
#   - Image `gazflow-ipopt:latest` (sinon : docker build -t gazflow-ipopt -f docker/Dockerfile.ipopt docker/)
#
# Ce que ce script vérifie :
#   1. `cargo check --features nlp-ipopt` (link pkg-config ipopt dans l'image)
#   2. Gates `GAZFLOW_NOVA_IPOPT_ESCALATION` (On / OnNotSolved / Off) via unit tests
#   3. Path FFI IPOPT in-repo : `ipopt_solves_two_node_feasible`
#
# Hors scope (volontaire) :
#   - Escalade mild_618 de bout en bout via `compressor_diag` : ce binaire n'appelle pas
#     `finalize_nova_verdict` (API REST/WS uniquement). Pour mild_618 + signature
#     IpoptEscalation, lancer le backend buildé avec `--features nlp-ipopt` et
#     `GAZFLOW_NOVA_IPOPT_ESCALATION=on-notsolved`.
#   - Pas d'env `GAZFLOW_NOVA_IPOPT` : seule `GAZFLOW_NOVA_IPOPT_ESCALATION` pilote l'escalade.
#
# Usage (depuis gazsim/) :
#   ./scripts/nova/smoke-ipopt-escalation.sh

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
IMAGE="${GAZFLOW_IPOPT_IMAGE:-gazflow-ipopt:latest}"
CARGO_VOL="${GAZFLOW_IPOPT_CARGO_VOL:-gazflow-ipopt-cargo}"
TARGET_VOL="${GAZFLOW_IPOPT_TARGET_VOL:-gazflow-ipopt-target}"

if ! command -v docker >/dev/null 2>&1; then
  echo "BLOCKER: docker absent. Build hôte nécessite coinor-libipopt-dev + libblas/lapack (voir docker/Dockerfile.ipopt)." >&2
  exit 2
fi

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "BLOCKER: image $IMAGE absente." >&2
  echo "  docker build -t gazflow-ipopt -f \"$ROOT/docker/Dockerfile.ipopt\" \"$ROOT/docker\"" >&2
  exit 2
fi

if ! pkg-config --exists ipopt 2>/dev/null; then
  echo "note: IPOPT absent sur l'hôte (pkg-config ipopt) — smoke via Docker uniquement (OK)."
fi

# Ne pas utiliser bash -l : le login shell peut dropper /usr/local/cargo/bin du PATH.
run() {
  docker run --rm \
    -v "$ROOT:/app" \
    -v "$CARGO_VOL:/usr/local/cargo/registry" \
    -v "$TARGET_VOL:/app/back/target" \
    -w /app/back \
    -e OMP_NUM_THREADS=1 \
    -e PATH="/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    "$IMAGE" \
    bash -c "$1"
}

echo "==> [1/3] cargo check --features nlp-ipopt"
run 'set -e; cargo check --features nlp-ipopt --lib 2>&1 | tee /tmp/check.log | tail -8; grep -q "Finished" /tmp/check.log'

echo "==> [2/3] unit tests api::nova_finalize (escalation gates)"
run 'set -e; cargo test --features nlp-ipopt --lib nova_finalize -- --nocapture 2>&1 | tee /tmp/nf.log | tail -30; grep -q "9 passed" /tmp/nf.log'

echo "==> [3/3] FFI IPOPT two_node (solve_nova_with_ipopt)"
run 'set -e; cargo test --features nlp-ipopt --lib ipopt_solves_two_node_feasible -- --nocapture 2>&1 | tee /tmp/ip.log | tail -40; grep -q "1 passed" /tmp/ip.log'

echo
echo "OK smoke IPOPT NoVa (gates + FFI)."
echo "Escalade mild_618 (signature IpoptEscalation) : backend --features nlp-ipopt +"
echo "  GAZFLOW_NOVA_IPOPT_ESCALATION=on-notsolved  (pas compressor_diag)."
