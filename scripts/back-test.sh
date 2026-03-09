#!/usr/bin/env bash
set -euo pipefail

# Lance les tests Rust dans le conteneur back.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."

cd "$ROOT"
docker compose exec back cargo test "$@"
