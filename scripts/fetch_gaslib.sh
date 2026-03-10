#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DAT_DIR="$SCRIPT_DIR/../back/dat"

mkdir -p "$DAT_DIR"

GASLIB_BASE="https://gaslib.zib.de"

declare -A DATASET_URLS=(
    ["GasLib-11"]="$GASLIB_BASE/download/testData/GasLib-11-v1-20211130.zip"
    ["GasLib-24"]="$GASLIB_BASE/download/testData/GasLib-24-v1-20211130.zip"
    ["GasLib-40"]="$GASLIB_BASE/download/testData/GasLib-40-v1-20211130.zip"
    ["GasLib-135"]="$GASLIB_BASE/download/testData/GasLib-135-v1-20211130.zip"
    ["GasLib-582"]="$GASLIB_BASE/download/data/GasLib-582-v2-20211129.zip"
    ["GasLib-4197"]="$GASLIB_BASE/download/data/GasLib-4197-v1-20220119.zip"
)

declare -A NOMINATIONS_URLS=(
    ["GasLib-582"]="$GASLIB_BASE/download/data/Nominations-582-v2-20211129.zip"
    ["GasLib-4197"]="$GASLIB_BASE/download/data/Nominations-4197-v1-20220119.zip"
)

REQUESTED="${1:-GasLib-11}"

if [[ -z "${DATASET_URLS[$REQUESTED]+x}" ]]; then
    echo "Dataset inconnu : $REQUESTED"
    echo "Disponibles : ${!DATASET_URLS[*]}"
    exit 1
fi

URL="${DATASET_URLS[$REQUESTED]}"
ZIP_FILE="$(basename "$URL")"
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

if [[ -n "${NOMINATIONS_URLS[$REQUESTED]:-}" ]]; then
    NOM_URL="${NOMINATIONS_URLS[$REQUESTED]}"
    NOM_ZIP="$DAT_DIR/$(basename "$NOM_URL")"
    echo "Téléchargement nominations pour $REQUESTED depuis $NOM_URL …"
    if curl -fSL -o "$NOM_ZIP" "$NOM_URL"; then
        unzip -o "$NOM_ZIP" -d "$DAT_DIR"
        rm -f "$NOM_ZIP"
    else
        echo "Avertissement: nominations indisponibles pour $REQUESTED (continue)."
        rm -f "$NOM_ZIP"
    fi
fi

# Crée des alias stables attendus par le backend (ex: GasLib-11.net, GasLib-11.scn).
for ext in net scn cs svg; do
    target=""
    dataset_code="${REQUESTED#GasLib-}"
    for candidate in "$DAT_DIR/${REQUESTED}-v1-"*."$ext"; do
        if [[ -f "$candidate" ]]; then
            target="$candidate"
            break
        fi
    done
    if [[ -z "$target" ]]; then
        for candidate in "$DAT_DIR/${REQUESTED}-v2-"*."$ext"; do
            if [[ -f "$candidate" ]]; then
                target="$candidate"
                break
            fi
        done
    fi
    if [[ -z "$target" && "$ext" == "scn" ]]; then
        for candidate in "$DAT_DIR/Nominations-${dataset_code}"*."$ext" "$DAT_DIR/Nominations-${dataset_code}"*/*."$ext"; do
            if [[ -f "$candidate" ]]; then
                target="$candidate"
                break
            fi
        done
    fi
    if [[ -z "$target" ]]; then
        legacy_candidate="$DAT_DIR/$REQUESTED.$ext"
        if [[ -f "$legacy_candidate" ]]; then
            target="$legacy_candidate"
        fi
    fi
    if [[ -n "$target" ]]; then
        rel_target="${target#$DAT_DIR/}"
        ln -sfn "$rel_target" "$DAT_DIR/$REQUESTED.$ext"
    fi
done

echo "$REQUESTED installé dans $DAT_DIR/"
ls -la "$DAT_DIR/"
