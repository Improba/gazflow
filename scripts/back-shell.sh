#!/usr/bin/env bash
set -euo pipefail

# Ouvre un shell bash dans le conteneur back (pour cargo add, cargo test, etc.).

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."

cd "$ROOT"
docker compose exec back bash
