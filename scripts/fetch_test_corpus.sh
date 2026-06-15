#!/usr/bin/env bash
# Télécharge les jeux externes du corpus opérationnel (P6–P13).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CORPUS_DIR="$SCRIPT_DIR/../docs/testing/corpus"
EXTERNAL_DIR="$CORPUS_DIR/external"
TMP_DIR="${TMPDIR:-/tmp}/gazflow-corpus-$$"
mkdir -p "$TMP_DIR" "$EXTERNAL_DIR/gaslib-39" "$EXTERNAL_DIR/transient/gaslib-11" "$EXTERNAL_DIR/scigrid/fr-snippet"
trap 'rm -rf "$TMP_DIR"' EXIT

echo "==> GasLib-39 (control valves, scénarios)"
G39_URL="https://gaslib.zib.de/download/testData/GasLib-39-v1-20231119.zip"
G39_ZIP="$TMP_DIR/GasLib-39.zip"
curl -fSL -o "$G39_ZIP" "$G39_URL"
unzip -o -q "$G39_ZIP" -d "$EXTERNAL_DIR/gaslib-39"
echo "    → $EXTERNAL_DIR/gaslib-39/"

echo "==> TRR154 transitoire GasLib-11 (.bcd + .state)"
TRR_BCD="https://www.trr154.fau.de/shared-files/6818/?GasLib-11-sinus-InputData-1.bcd-1.bcd"
TRR_STATE="https://www.trr154.fau.de/shared-files/6817/?GasLib-11-sinus_5000_60-initial-1.state-1.state"
# Fallback si l'ID shared-files change : page https://www.trr154.fau.de/transient-data/
if ! curl -fSL -o "$EXTERNAL_DIR/transient/gaslib-11/GasLib-11-sinus-InputData.bcd" "$TRR_BCD"; then
  echo "    Avertissement: échec téléchargement .bcd — vérifier les URLs sur trr154.fau.de/transient-data/"
fi
if ! curl -fSL -o "$EXTERNAL_DIR/transient/gaslib-11/GasLib-11-sinus_5000_60-initial.state" "$TRR_STATE"; then
  echo "    Avertissement: échec téléchargement .state — vérifier les URLs sur trr154.fau.de/transient-data/"
fi
echo "    → $EXTERNAL_DIR/transient/gaslib-11/"

echo "==> SciGRID_gas IGGIELGN (extrait France)"
SCIGRID_URL="https://zenodo.org/records/4767098/files/IGGIELGN.zip"
SCIGRID_ZIP="$TMP_DIR/IGGIELGN.zip"
curl -fSL -o "$SCIGRID_ZIP" "$SCIGRID_URL"
python3 "$SCRIPT_DIR/extract_scigrid_snippet.py" "$SCIGRID_ZIP" "$EXTERNAL_DIR/scigrid/fr-snippet"
echo "    → $EXTERNAL_DIR/scigrid/fr-snippet/"

echo ""
echo "Corpus externe prêt."
echo "Fixtures synthétiques (déjà versionnées) : $CORPUS_DIR/synthetic/"
echo "Inventaire : $CORPUS_DIR/manifest.yaml"
