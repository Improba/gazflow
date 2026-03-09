#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DAT_DIR="$SCRIPT_DIR/../back/dat"

mkdir -p "$DAT_DIR"

GASLIB_BASE="https://gaslib.zib.de"

declare -A DATASETS=(
    ["GasLib-11"]="GasLib-11-v1-20211130.zip"
    ["GasLib-24"]="GasLib-24-v1-20211130.zip"
    ["GasLib-40"]="GasLib-40-v1-20211130.zip"
)

REQUESTED="${1:-GasLib-11}"

if [[ -z "${DATASETS[$REQUESTED]+x}" ]]; then
    echo "Dataset inconnu : $REQUESTED"
    echo "Disponibles : ${!DATASETS[*]}"
    exit 1
fi

ZIP_FILE="${DATASETS[$REQUESTED]}"
URL="$GASLIB_BASE/download/testData/$ZIP_FILE"
DEST="$DAT_DIR/$ZIP_FILE"

echo "Téléchargement de $REQUESTED depuis $URL …"
if ! curl -fSL -o "$DEST" "$URL"; then
    # Fallback vers les anciennes archives non versionnées.
    LEGACY_URL="$GASLIB_BASE/download/testData/old-versions/$REQUESTED.zip"
    echo "URL principale indisponible, tentative fallback: $LEGACY_URL"
    curl -fSL -o "$DEST" "$LEGACY_URL"
fi

echo "Extraction dans $DAT_DIR …"
unzip -o "$DEST" -d "$DAT_DIR"
rm -f "$DEST"

# Crée des alias stables attendus par le backend (ex: GasLib-11.net, GasLib-11.scn).
for ext in net scn cs svg; do
    target=""
    for candidate in "$DAT_DIR/${REQUESTED}-v1-"*."$ext"; do
        if [[ -f "$candidate" ]]; then
            target="$candidate"
            break
        fi
    done
    if [[ -z "$target" ]]; then
        legacy_candidate="$DAT_DIR/$REQUESTED.$ext"
        if [[ -f "$legacy_candidate" ]]; then
            target="$legacy_candidate"
        fi
    fi
    if [[ -n "$target" ]]; then
        ln -sfn "$(basename "$target")" "$DAT_DIR/$REQUESTED.$ext"
    fi
done

echo "$REQUESTED installé dans $DAT_DIR/"
ls -la "$DAT_DIR/"
