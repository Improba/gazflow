#!/usr/bin/env bash
# Vérifie la présence et la cohérence du corpus de test (synthetic + external).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CORPUS="$SCRIPT_DIR/../docs/testing/corpus"
FAIL=0

check() {
  if [[ -e "$1" ]]; then
    echo "  OK  $2"
  else
    echo "  MANQUANT  $2 ($1)"
    FAIL=1
  fi
}

echo "Fixtures synthétiques (versionnées)"
check "$CORPUS/synthetic/minimal-line/nodes.geojson" "minimal-line nodes"
check "$CORPUS/synthetic/minimal-line/pipes.geojson" "minimal-line pipes"
check "$CORPUS/synthetic/minimal-line/mapping.yaml" "minimal-line mapping"
check "$CORPUS/synthetic/gravity-pipe/nodes.csv" "gravity uphill"
check "$CORPUS/synthetic/gravity-pipe/nodes-flat.csv" "gravity flat"
check "$CORPUS/synthetic/gravity-pipe/nodes-downhill.csv" "gravity downhill"
check "$CORPUS/synthetic/gravity-pipe/pipes.csv" "gravity pipes"
check "$CORPUS/synthetic/topo-errors/orphan-node.geojson" "topo orphan"
check "$CORPUS/synthetic/topo-errors/no-slack.geojson" "topo no-slack"
check "$CORPUS/synthetic/topo-errors/disconnected.geojson" "topo disconnected"
check "$CORPUS/synthetic/demand/profiles.csv" "demand profiles"
check "$CORPUS/synthetic/demand/daily-profiles.yaml" "demand daily profiles"
check "$CORPUS/synthetic/scada/measurements.csv" "scada measurements"
check "$CORPUS/mapping/scigrid-fr.mapping.yaml" "scigrid mapping"

echo ""
echo "Jeux externes (./scripts/fetch_test_corpus.sh)"
check "$CORPUS/external/gaslib-39/GasLib-39-v1-20231119.net" "GasLib-39 net"
check "$CORPUS/external/gaslib-39/GasLib-39-v1-20231119.cs" "GasLib-39 compressors"
check "$CORPUS/external/transient/gaslib-11/GasLib-11-sinus-InputData.bcd" "TRR154 bcd"
check "$CORPUS/external/transient/gaslib-11/GasLib-11-sinus_5000_60-initial.state" "TRR154 state"
check "$CORPUS/external/scigrid/fr-snippet/IGGIELGN_PipeSegments.geojson" "SciGRID FR pipes"
check "$CORPUS/external/scigrid/fr-snippet/IGGIELGN_Nodes.geojson" "SciGRID FR nodes"
check "$CORPUS/external/scigrid/fr-snippet/snippet-meta.json" "SciGRID meta"

echo ""
echo "Validation sémantique (Python)"
if ! python3 "$SCRIPT_DIR/validate_test_corpus.py"; then
  FAIL=1
fi

if [[ "$FAIL" -ne 0 ]]; then
  echo ""
  echo "Corpus incomplet ou invalide. Lancer : ./scripts/fetch_test_corpus.sh"
  exit 1
fi

echo ""
echo "Corpus OK."
