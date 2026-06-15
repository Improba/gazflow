#!/usr/bin/env bash
set -euo pipefail

# CI complète : build + tests back et front, via Docker Compose.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."

cd "$ROOT"

echo "========================================="
echo "  GazFlow CI (Docker)"
echo "========================================="

echo ""
echo "--- [0/8] Corpus de test (local) ---"
if [[ -x "$SCRIPT_DIR/verify_test_corpus.sh" ]]; then
  "$SCRIPT_DIR/verify_test_corpus.sh"
else
  echo "  skip verify_test_corpus.sh (absent ou non exécutable)"
fi

echo ""
echo "--- [1/8] Build des images ---"
docker compose build

echo ""
echo "--- [2/8] Rust : cargo check ---"
docker compose run --rm back cargo check

echo ""
echo "--- [3/8] Rust : clippy ---"
docker compose run --rm back cargo clippy -- -D warnings

echo ""
echo "--- [4/8] Rust : cargo test ---"
docker compose run --rm back cargo test

echo ""
echo "--- [5/8] Frontend : npm install ---"
docker compose run --rm front npm install

echo ""
echo "--- [6/8] Frontend : npm test ---"
docker compose run --rm front npm test

echo ""
echo "--- [7/8] Frontend : build ---"
docker compose run --rm front npx quasar build

echo ""
echo "========================================="
echo "  CI terminée avec succès"
echo "========================================="
