#!/usr/bin/env python3
"""Extrait un sous-ensemble SciGRID_gas IGGIELGN (pipes FR) pour le corpus de tests."""

from __future__ import annotations

import json
import math
import shutil
import sys
import zipfile
from pathlib import Path

MAX_PIPES = 80
ENDPOINT_PRECISION = 4  # arrondi deg → fusion nœuds proches


def round_coord(lon: float, lat: float) -> tuple[float, float]:
    return round(lon, ENDPOINT_PRECISION), round(lat, ENDPOINT_PRECISION)


def haversine_km(lon1: float, lat1: float, lon2: float, lat2: float) -> float:
    r = 6371.0
    p1, p2 = math.radians(lat1), math.radians(lat2)
    dphi = math.radians(lat2 - lat1)
    dl = math.radians(lon2 - lon1)
    a = math.sin(dphi / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(dl / 2) ** 2
    return 2 * r * math.asin(min(1.0, math.sqrt(a)))


def country_has_fr(country_code) -> bool:
    if country_code is None:
        return False
    if isinstance(country_code, str):
        return country_code == "FR"
    return "FR" in country_code


def main() -> int:
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <IGGIELGN.zip> <output_dir>", file=sys.stderr)
        return 1

    zip_path = Path(sys.argv[1])
    out_dir = Path(sys.argv[2])
    out_dir.mkdir(parents=True, exist_ok=True)

    with zipfile.ZipFile(zip_path) as zf:
        pipe_name = "data/IGGIELGN_PipeSegments.geojson"
        with zf.open(pipe_name) as f:
            pipes_fc = json.load(f)

        for name in ("data/LICENSE", "data/README", "data/AUTHORS"):
            if name in zf.namelist():
                with zf.open(name) as src, open(out_dir / Path(name).name, "wb") as dst:
                    shutil.copyfileobj(src, dst)

    selected: list[dict] = []
    for feat in pipes_fc.get("features", []):
        props = feat.get("properties") or {}
        if not country_has_fr(props.get("country_code")):
            continue
        geom = feat.get("geometry") or {}
        if geom.get("type") != "LineString":
            continue
        coords = geom.get("coordinates") or []
        if len(coords) < 2:
            continue
        selected.append(feat)
        if len(selected) >= MAX_PIPES:
            break

    if not selected:
        print("Aucun pipe FR trouvé dans SciGRID", file=sys.stderr)
        return 1

    node_index: dict[tuple[float, float], str] = {}
    node_features: list[dict] = []
    out_pipes: list[dict] = []

    def node_id_for(lon: float, lat: float) -> str:
        key = round_coord(lon, lat)
        if key not in node_index:
            nid = f"N_{len(node_index):04d}"
            node_index[key] = nid
            node_features.append(
                {
                    "type": "Feature",
                    "properties": {"id": nid, "lon": key[0], "lat": key[1]},
                    "geometry": {"type": "Point", "coordinates": [key[0], key[1]]},
                }
            )
        return node_index[key]

    for feat in selected:
        props = dict(feat.get("properties") or {})
        coords = feat["geometry"]["coordinates"]
        lon_a, lat_a = coords[0][0], coords[0][1]
        lon_b, lat_b = coords[-1][0], coords[-1][1]
        from_id = node_id_for(lon_a, lat_a)
        to_id = node_id_for(lon_b, lat_b)
        param = props.get("param") or {}
        length_km = param.get("length_km")
        if not length_km or length_km <= 0:
            length_km = haversine_km(lon_a, lat_a, lon_b, lat_b)
        props["_derived_from"] = from_id
        props["_derived_to"] = to_id
        if "param" in props and isinstance(props["param"], dict):
            props["param"] = dict(props["param"])
            props["param"]["length_km"] = length_km
        out_pipes.append(
            {
                "type": "Feature",
                "properties": props,
                "geometry": feat["geometry"],
            }
        )

    pipes_out = {"type": "FeatureCollection", "name": "IGGIELGN_PipeSegments_fr_snippet", "features": out_pipes}
    nodes_out = {"type": "FeatureCollection", "name": "IGGIELGN_Nodes_fr_snippet", "features": node_features}

    (out_dir / "IGGIELGN_PipeSegments.geojson").write_text(
        json.dumps(pipes_out, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    (out_dir / "IGGIELGN_Nodes.geojson").write_text(
        json.dumps(nodes_out, ensure_ascii=False, indent=2), encoding="utf-8"
    )

    meta = {
        "source": "SciGRID_gas IGGIELGN (Zenodo 4767098)",
        "filter": "country_code contains FR",
        "max_pipes": MAX_PIPES,
        "pipe_count": len(out_pipes),
        "node_count": len(node_features),
    }
    (out_dir / "snippet-meta.json").write_text(json.dumps(meta, indent=2), encoding="utf-8")

    print(f"SciGRID extrait : {len(out_pipes)} pipes, {len(node_features)} nœuds → {out_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
