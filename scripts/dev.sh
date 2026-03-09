#!/usr/bin/env bash
set -euo pipefail

# Lance l'environnement de développement complet (back + front) via Docker Compose.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."

cd "$ROOT"
docker compose up --build
