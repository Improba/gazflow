#!/usr/bin/env bash
set -euo pipefail

# Arrête tous les conteneurs du projet.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."

cd "$ROOT"
docker compose down
