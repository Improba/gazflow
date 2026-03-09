#!/usr/bin/env bash
set -euo pipefail

# Lance les tests frontend dans le conteneur front.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."

cd "$ROOT"
docker compose exec front npm test "$@"
