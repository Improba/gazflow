#!/usr/bin/env python3
"""Trace topological pressure-reachability from sources to marginal sinks.

Builds an undirected graph of pressure-conductive arcs and checks whether each
marginal sink is reachable from at least one source, and via which arc types.

Arc conductivity model (matches GazFlow MVP assumptions):
  - pipe, shortPipe, resistor: always conductive (passive)
  - valve: conductive if open (GasFlow opens valves by default; .cdf may close)
  - controlValve: conductive in bypass/quasi-transparent mode (GazFlow MVP)
  - compressorStation: conductive via internal bypass (bypassRequired=0 path)

This is a STATIC reachability test (no friction, no flow). It isolates the
topological question from the friction/flow question:
  * reachable  -> "infeasibility" is friction/flow-driven (capacity question)
  * unreachable-> real topological infeasibility (no open pressure path)
"""

import xml.etree.ElementTree as ET
import sys
from collections import defaultdict, deque

NS = {"g": "http://gaslib.zib.de/Gas"}


def localname(tag):
    return tag.split("}", 1)[-1]


def main(path="back/dat/GasLib-582.net", sinks=("sink_88", "sink_83", "sink_108", "sink_125", "sink_122")):
    tree = ET.parse(path)
    root = tree.getroot()

    # Collect sources with their pressureMax
    sources = {}  # id -> pressureMax bar
    for node in root.iter():
        if localname(node.tag) == "source":
            sid = node.get("id")
            pmax = None
            pmin = None
            for child in node:
                ln = localname(child.tag)
                if ln == "pressureMax":
                    pmax = float(child.get("value"))
                if ln == "pressureMin":
                    pmin = float(child.get("value"))
            sources[sid] = (pmin, pmax)

    # Build adjacency with arc type
    adj = defaultdict(list)  # node -> [(neighbor, arc_type, arc_id)]
    arc_types = defaultdict(int)
    for node in root.iter():
        ln = localname(node.tag)
        if ln in ("pipe", "shortPipe", "valve", "controlValve", "resistor", "compressorStation"):
            frm = node.get("from")
            to = node.get("to")
            aid = node.get("id")
            adj[frm].append((to, ln, aid))
            adj[to].append((frm, ln, aid))
            arc_types[ln] += 1

    print(f"Sources: {len(sources)}  Arcs: {dict(arc_types)}")
    print()

    # BFS from ALL sources simultaneously over the conductive graph.
    # All arc types here are treated conductive (valves open, CV bypass, compressor bypass).
    dist = {}
    parent = {}
    q = deque()
    for s in sources:
        dist[s] = 0
        parent[s] = (None, None, None)
        q.append(s)
    while q:
        u = q.popleft()
        for (v, atype, aid) in adj[u]:
            if v not in dist:
                dist[v] = dist[u] + 1
                parent[v] = (u, atype, aid)
                q.append(v)

    # Per-sink analysis
    for sink in sinks:
        if sink not in dist:
            print(f"{sink}: UNREACHABLE from any source (no conductive path).")
            # which sources are in the same component as nothing -> report component
            continue
        # reconstruct path, note arc types used
        path = []
        cur = sink
        while parent[cur][0] is not None:
            p, atype, aid = parent[cur]
            path.append((p, cur, atype, aid))
            cur = p
        arc_type_counts = defaultdict(int)
        for (p, c, atype, aid) in path:
            arc_type_counts[atype] += 1
        # find the source it connects to and its pressureMax
        src = cur
        pmax = sources[src][1]
        print(f"{sink}: reachable from {src} (pressureMax={pmax} bar), hops={len(path)}, "
              f"arc_types={dict(arc_type_counts)}")
        # print first few hops and any controlValve/valve/compressor on path
        special = [(p, c, at, aid) for (p, c, at, aid) in path if at in ("controlValve", "valve", "compressorStation")]
        if special:
            print(f"  active elements on path ({len(special)}):")
            for (p, c, at, aid) in special:
                print(f"    {at} {aid}: {p} -> {c}")
        else:
            print(f"  no valve/controlValve/compressor on path (pure passive pipes/shortPipes/resistors)")
    print()

    # Also: per source, which marginal sinks it can reach (to spot close sources)
    print("Per-source reachability to marginal sinks (hops):")
    for s in sorted(sources, key=lambda x: int(x.split("_")[1])):
        # BFS from this single source
        d = {s: 0}
        q2 = deque([s])
        while q2:
            u = q2.popleft()
            for (v, atype, aid) in adj[u]:
                if v not in d:
                    d[v] = d[u] + 1
                    q2.append(v)
        reach = {sk: d[sk] for sk in sinks if sk in d}
        if reach:
            pmax = sources[s][1]
            print(f"  {s} (pMax={pmax}): {reach}")


if __name__ == "__main__":
    p = sys.argv[1] if len(sys.argv) > 1 else "back/dat/GasLib-582.net"
    main(p)
