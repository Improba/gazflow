#!/usr/bin/env bash
set -euo pipefail

# Ouvre un shell bash dans le conteneur front (pour npm install, npx, etc.).

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."

cd "$ROOT"
docker compose exec front bash
