#!/usr/bin/env bash
set -euo pipefail

# CI complète : build + tests back et front, via Docker Compose.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."

cd "$ROOT"

echo "========================================="
echo "  GazSim CI (Docker)"
echo "========================================="

echo ""
echo "--- [1/5] Build des images ---"
docker compose build

echo ""
echo "--- [2/5] Rust : cargo check ---"
docker compose run --rm back cargo check

echo ""
echo "--- [3/5] Rust : cargo test ---"
docker compose run --rm back cargo test

echo ""
echo "--- [4/5] Frontend : npm install ---"
docker compose run --rm front npm install

echo ""
echo "--- [5/5] Frontend : build ---"
docker compose run --rm front npx quasar build

echo ""
echo "========================================="
echo "  CI terminée avec succès"
echo "========================================="
