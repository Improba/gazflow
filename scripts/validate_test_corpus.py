#!/usr/bin/env python3
"""Validation sémantique du corpus docs/testing/corpus/."""

from __future__ import annotations

import csv
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
CORPUS = SCRIPT_DIR.parent / "docs" / "testing" / "corpus"


def main() -> int:
    issues: list[str] = []

    def err(msg: str) -> None:
        issues.append(msg)
        print(f"  ERREUR  {msg}")

    # minimal-line
    nodes = json.loads((CORPUS / "synthetic/minimal-line/nodes.geojson").read_text())
    pipes = json.loads((CORPUS / "synthetic/minimal-line/pipes.geojson").read_text())
    node_ids = {f["properties"]["ID_NOEUD"] for f in nodes["features"]}
    if len(node_ids) != 3 or len(pipes["features"]) != 2:
        err("minimal-line: attendu 3 nœuds et 2 pipes")
    for pf in pipes["features"]:
        p = pf["properties"]
        if p["NOEUD_AMONT"] not in node_ids or p["NOEUD_AVAL"] not in node_ids:
            err(f"minimal-line: pipe {p['ID_CANA']} référence un nœud inconnu")

    # gravity altitudes (UP → DOWN)
    for name, expected_dz in [
        ("nodes.csv", 150),
        ("nodes-flat.csv", 0),
        ("nodes-downhill.csv", -150),
    ]:
        path = CORPUS / "synthetic/gravity-pipe" / name
        rows = list(csv.DictReader(path.open()))
        dz = float(rows[1]["altitude_m"]) - float(rows[0]["altitude_m"])
        if abs(dz - expected_dz) > 1e-6:
            err(f"gravity {name}: Δz={dz}, attendu {expected_dz}")

    # topo-errors
    def load_mixed(path: Path) -> tuple[set[str], set[str]]:
        data = json.loads(path.read_text())
        points = {f["properties"].get("ID_NOEUD") for f in data["features"] if f["geometry"]["type"] == "Point"}
        connected: set[str] = set()
        for f in data["features"]:
            if f["geometry"]["type"] != "LineString":
                continue
            p = f["properties"]
            connected.add(p["NOEUD_AMONT"])
            connected.add(p["NOEUD_AVAL"])
        return points, connected

    pts, conn = load_mixed(CORPUS / "synthetic/topo-errors/orphan-node.geojson")
    if pts - conn != {"ORPHAN"}:
        err(f"orphan-node: orphelins attendus {{ORPHAN}}, got {pts - conn}")

    data = json.loads((CORPUS / "synthetic/topo-errors/no-slack.geojson").read_text())
    has_slack = any(
        f["geometry"]["type"] == "Point"
        and (f["properties"].get("P_CONSIGNE_BAR") or f["properties"].get("TYPE") == "ALIM")
        for f in data["features"]
    )
    if has_slack:
        err("no-slack: contient une source ou pression fixée")

    data = json.loads((CORPUS / "synthetic/topo-errors/disconnected.geojson").read_text())
    adj: dict[str, set[str]] = defaultdict(set)
    all_nodes: set[str] = set()
    for f in data["features"]:
        if f["geometry"]["type"] == "Point":
            all_nodes.add(f["properties"]["ID_NOEUD"])
        elif f["geometry"]["type"] == "LineString":
            a, b = f["properties"]["NOEUD_AMONT"], f["properties"]["NOEUD_AVAL"]
            adj[a].add(b)
            adj[b].add(a)
    visited: set[str] = set()
    components = 0
    for n in all_nodes:
        if n in visited:
            continue
        components += 1
        stack = [n]
        while stack:
            u = stack.pop()
            if u in visited:
                continue
            visited.add(u)
            stack.extend(adj[u] - visited)
    if components != 2:
        err(f"disconnected: {components} composantes, attendu 2")

    # scada ↔ minimal-line
    scada_ids = {r["element_id"] for r in csv.DictReader((CORPUS / "synthetic/scada/measurements.csv").open())}
    if not {"SRC01", "JNC01", "LVR01", "P01"} <= scada_ids:
        err("scada: IDs non alignés avec minimal-line")

    # daily profiles (Σ w_h = 24 par preset)
    profiles_path = CORPUS / "synthetic/demand/daily-profiles.yaml"
    if profiles_path.exists():
        text = profiles_path.read_text()
        for preset in re.findall(r"^([a-z_]+):\s*$", text, re.M):
            block = re.search(rf"^{preset}:(.*?)(?:\n[a-z_]+:|$)", text, re.S)
            if not block:
                continue
            weights = [float(v) for _, v in re.findall(r"(\d+):\s*([\d.]+)", block.group(1))]
            if len(weights) != 24:
                err(f"daily-profiles {preset}: attendu 24 poids, got {len(weights)}")
            total = sum(weights)
            if abs(total - 24.0) > 1e-6:
                err(f"daily-profiles {preset}: Σ w_h = {total}, attendu 24")

    # SciGRID snippet
    meta = json.loads((CORPUS / "external/scigrid/fr-snippet/snippet-meta.json").read_text())
    if meta.get("pipe_count", 0) < 50:
        err(f"SciGRID snippet trop petit: {meta.get('pipe_count')} pipes")
    sg = json.loads((CORPUS / "external/scigrid/fr-snippet/IGGIELGN_PipeSegments.geojson").read_text())
    if any("_derived_from" not in f["properties"] for f in sg["features"]):
        err("SciGRID: pipes sans _derived_from/_derived_to")

    # GasLib-39
    net = (CORPUS / "external/gaslib-39/GasLib-39-v1-20231119.net").read_text()
    if "controlValve" not in net:
        err("GasLib-39: pas de controlValve dans .net")
    if len(list((CORPUS / "external/gaslib-39").glob("*.scn"))) != 10:
        err("GasLib-39: attendu 10 fichiers .scn")

    # TRR154
    bcd_path = CORPUS / "external/transient/gaslib-11/GasLib-11-sinus-InputData.bcd"
    state_path = CORPUS / "external/transient/gaslib-11/GasLib-11-sinus_5000_60-initial.state"
    bcd_head = bcd_path.read_text(errors="replace")[:2000]
    state_head = state_path.read_text(errors="replace")[:2000]
    if bcd_path.stat().st_size < 1000 or "<?xml" not in bcd_head or "<boundarydata" not in bcd_head:
        err("TRR154 bcd: XML boundarydata invalide ou trop petit")
    if "GasLib-11.net" not in bcd_head:
        err("TRR154 bcd: réseau GasLib-11.net non référencé")
    if state_path.stat().st_size < 1000 or "<?xml" not in state_head or "<initialdata" not in state_head:
        err("TRR154 state: XML initialdata invalide ou trop petit")
    if "GasLib-11.net" not in state_head:
        err("TRR154 state: réseau GasLib-11.net non référencé")

    if issues:
        return 1

    print("  OK  validation sémantique")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
