#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DAT_DIR="$SCRIPT_DIR/../back/dat"

mkdir -p "$DAT_DIR"

GASLIB_BASE="https://gaslib.zib.de"

declare -A DATASETS=(
    ["GasLib-11"]="GasLib-11.zip"
    ["GasLib-24"]="GasLib-24.zip"
    ["GasLib-40"]="GasLib-40.zip"
)

REQUESTED="${1:-GasLib-11}"

if [[ -z "${DATASETS[$REQUESTED]+x}" ]]; then
    echo "Dataset inconnu : $REQUESTED"
    echo "Disponibles : ${!DATASETS[*]}"
    exit 1
fi

ZIP_FILE="${DATASETS[$REQUESTED]}"
URL="$GASLIB_BASE/download/$ZIP_FILE"
DEST="$DAT_DIR/$ZIP_FILE"

echo "Téléchargement de $REQUESTED depuis $URL …"
curl -fSL -o "$DEST" "$URL"

echo "Extraction dans $DAT_DIR …"
unzip -o "$DEST" -d "$DAT_DIR"
rm -f "$DEST"

echo "$REQUESTED installé dans $DAT_DIR/"
ls -la "$DAT_DIR/"
