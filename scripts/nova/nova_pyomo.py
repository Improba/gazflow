#!/usr/bin/env python3
"""Independent bounded NoVa feasibility NLP for GasLib-582 / nomination_mild_618.

Builds the standard isothermal steady-state P² NoVa NLP directly from the GasLib `.net`
and `.scn` files (independent re-implementation of the documented model, see
`docs/science/equations.md` §1.2b and §4.8), and solves it with an external solver:

  - IPOPT (local, robust interior-point NLP): finds a feasible point if one exists
    locally. A "feasible" verdict is a definitive YES. A "not solved" verdict is NOT a
    proof of infeasibility (IPOPT is local).
  - Couenne / BARON (global MINLP): would prove infeasibility — not run here by default.

Model (smooth NLP, pressures in bar, flows in Nm³/s = 1000m³/h ÷ 3.6):

  Variables:
    P_i in [Pmin_i, Pmax_i]  (bar) for every node; entries float within `.net` bounds
                              (Q fixed by nomination); slack (sink_109) P fixed = 51.01325.
    Q_a (signed) for every passive arc (pipe/shortPipe/resistor/valve-open).
    r_k in [1, pressureOutMax/pressureInMin] for each compressor station.

  Constraints:
    Passive arc a=(u,v):  P_u² - P_v² = K_a · Q_a · sqrt(Q_a² + ε²)   (smoothed Q|Q|)
    Compressor k:         P_out = r_k · P_in ; r_k in [1, rmax_k] ;
                          P_out <= pressureOutMax_k ; Q_in = Q_out (no fuel gas).
    Control valve j:      P_out <= P_in ; P_out <= pressureOutMax_j ;
                          P_out >= pressureInMin_j ; Q_in = Q_out.
    Mass conservation:    Σ_{a: to=i} Q_a - Σ_{a: from=i} Q_a = d_i
                          (d_i = +flow/3.6 for entries, -flow/3.6 for exits).
    Gauge:                P_sink109 = 51.01325 (50 barg, the scenario slack).

  Objective (Phase-1 feasibility): minimise Σ_i s_i² with mass conservation relaxed by
  slack s_i. If the optimum has Σ s_i² ≈ 0 (within tol) → feasible point found.

Pipe resistance K (bar²·s²/m⁶), matching GazFlow `pipe_resistance_with_density` MVP:
  K = f · L · rho_eff / (2 · D · A² · 1e10),  A = π D²/4,
  f = Swamee-Jain(roughness/D, Re=1e7), rho_eff = 50 kg/m³ (~70 bar CH₄),
  D in m, L in m. Effective geometry: pipe/resistor use net values; valve/shortPipe/
  compressor-bypass quasi-transparent (L=min(L,1e-3) km, D=max(D,1000) mm).

Usage:
  python3 nova_pyomo.py [--solver ipopt|couenne|bonmin] [--net PATH] [--scn PATH]
"""

import argparse
import math
import os
import sys
import xml.etree.ElementTree as ET
from collections import defaultdict

# IPOPT's default linear solver (MUMPS/HSL via OpenMP) is nondeterministic under
# multithreading, which on this non-convex NoVa NLP changes which local minimum is
# reached. Pin to a single thread so the feasibility verdict is reproducible.
# (Override by exporting OMP_NUM_THREADS before launching.)
os.environ.setdefault("OMP_NUM_THREADS", "1")


def ln(tag):
    return tag.split("}", 1)[-1]


# ---------- Parsing ----------

def parse_net(path):
    tree = ET.parse(path)
    root = tree.getroot()
    nodes = {}  # id -> {pmin, pmax, kind}
    for n in root.iter():
        if ln(n.tag) == "node" or ln(n.tag) in ("source", "sink", "innode"):
            nid = n.get("id")
            if nid is None:
                continue
            pmin = pmax = None
            for c in n:
                l = ln(c.tag)
                if l == "pressureMin":
                    pmin = float(c.get("value"))
                elif l == "pressureMax":
                    pmax = float(c.get("value"))
            kind = ln(n.tag)
            if nid not in nodes:
                nodes[nid] = {"pmin": pmin, "pmax": pmax, "kind": kind}
            else:
                if pmin is not None and nodes[nid]["pmin"] is None:
                    nodes[nid]["pmin"] = pmin
                if pmax is not None and nodes[nid]["pmax"] is None:
                    nodes[nid]["pmax"] = pmax
                if nodes[nid]["kind"] in ("node",):
                    nodes[nid]["kind"] = kind
    arcs = []
    for n in root.iter():
        l = ln(n.tag)
        if l not in ("pipe", "shortPipe", "valve", "controlValve", "resistor", "compressorStation"):
            continue
        aid = n.get("id")
        frm = n.get("from")
        to = n.get("to")
        length_km = diameter_mm = roughness_mm = None
        drag = None
        p_in_min = p_out_max = None
        for c in n:
            cl = ln(c.tag)
            v = c.get("value")
            if cl == "length":
                length_km = float(v)
            elif cl == "diameter":
                diameter_mm = float(v)
            elif cl == "roughness":
                roughness_mm = float(v)
            elif cl == "dragFactor":
                drag = float(v)
            elif cl == "pressureInMin":
                p_in_min = float(v)
            elif cl == "pressureOutMax":
                p_out_max = float(v)
        arcs.append({
            "id": aid, "kind": l, "from": frm, "to": to,
            "length_km": length_km, "diameter_mm": diameter_mm, "roughness_mm": roughness_mm,
            "drag": drag, "p_in_min": p_in_min, "p_out_max": p_out_max,
        })
    return nodes, arcs


def parse_scn(path):
    tree = ET.parse(path)
    root = tree.getroot()
    demands = {}      # id -> Nm³/s (signed: + entry, - exit)
    p_bounds = {}     # id -> (lower_bar_abs, upper_bar_abs)
    fixed_p = {}      # id -> bar abs (slack)
    for n in root.iter():
        if ln(n.tag) != "node":
            continue
        nid = n.get("id")
        ntype = n.get("type")
        if nid is None:
            continue
        flow_lo = flow_hi = None
        p_lo = p_hi = None
        p_lo_barg = False
        for c in n:
            cl = ln(c.tag)
            v = c.get("value")
            if cl == "flow":
                b = c.get("bound")
                u = c.get("unit", "")
                val = float(v)
                if "1000m" in u:
                    val = val / 3.6  # 1000m³/h -> Nm³/s
                if b == "both":
                    flow_lo = flow_hi = val
                elif b == "lower":
                    flow_lo = val if flow_lo is None else max(flow_lo, val)
                elif b == "upper":
                    flow_hi = val if flow_hi is None else min(flow_hi, val)
            elif cl == "pressure":
                b = c.get("bound")
                u = c.get("unit", "")
                val = float(v)
                if u == "barg":
                    val = val + 1.01325
                if b == "lower":
                    p_lo = val if p_lo is None else max(p_lo, val)
                    if u == "barg":
                        p_lo_barg = True
                elif b == "upper":
                    p_hi = val if p_hi is None else min(p_hi, val)
                elif b == "both" or b is None:
                    p_lo = val if p_lo is None else p_lo
                    p_hi = val if p_hi is None else p_hi
        # Demand sign: entry injects (+), exit withdraws (-).
        if flow_lo is not None and flow_hi is not None and flow_lo == flow_hi:
            d = flow_lo
            if ntype == "exit":
                d = -d
            demands[nid] = d
        elif flow_lo is not None or flow_hi is not None:
            # Range flow (not both): treat as bounded; for slack use the fixed-ish value.
            mid = ((flow_lo or 0.0) + (flow_hi or 0.0)) / 2.0 if (flow_lo is not None and flow_hi is not None) else (flow_lo if flow_lo is not None else flow_hi)
            if ntype == "exit":
                mid = -mid
            demands[nid] = mid  # approximation; slack handled below
        # Fixed pressure (slack): pressure bound lower with barg and flow both on exit sink_109.
        if p_lo is not None and p_hi is None and ntype == "exit" and p_lo_barg:
            fixed_p[nid] = p_lo
        if p_lo is not None or p_hi is not None:
            p_bounds[nid] = (p_lo, p_hi)
    return demands, p_bounds, fixed_p


# ---------- Pipe resistance (matches GazFlow MVP) ----------

def darcy_friction(roughness_mm, diameter_mm, re):
    e_d = roughness_mm / diameter_mm
    if re < 2300.0:
        return 64.0 / max(re, 1.0)
    return 0.25 / (math.log10(e_d / 3.7 + 5.74 / re ** 0.9) ** 2)


def pipe_resistance(length_km, diameter_mm, roughness_mm, rho_eff=50.0, re=1e7):
    d = diameter_mm * 1e-3
    L = length_km * 1e3
    re_c = min(max(re, 1000.0), 1e8)
    f = darcy_friction(roughness_mm, diameter_mm, re_c)
    A = math.pi * d * d / 4.0
    return max(f * L * rho_eff / (2.0 * d * A * A * 1e10), 1e-12)


# ---------- In-repo gas density (Papay, pure CH4) — to match GazFlow's rho(P_moy) ----------

_R = 8.314462618
_CH4_M = 0.01604
_CH4_PC_BAR = 46.0
_CH4_TC_K = 190.6
_GAS_T_K = 288.15


def papay_z(pressure_bar, temperature_k=_GAS_T_K):
    pr = max(pressure_bar, 0.0) / _CH4_PC_BAR
    tr = max(temperature_k / _CH4_TC_K, 0.1)
    z = 1.0 - 3.52 * pr / (10 ** (0.9813 * tr)) + 0.274 * pr * pr / (10 ** (0.8157 * tr))
    return min(max(z, 0.2), 1.5)


def gas_density_kg_per_m3(pressure_bar, temperature_k=_GAS_T_K):
    p_pa = max(pressure_bar, 0.0) * 1e5
    z = papay_z(pressure_bar, temperature_k)
    return p_pa * _CH4_M / (z * _R * max(temperature_k, 1.0))


def effective_geometry(arc):
    """Return (length_km, diameter_mm, roughness_mm) matching GazFlow effective_pipe_geometry."""
    k = arc["kind"]
    L = arc["length_km"] or 1.0
    D = arc["diameter_mm"] or 500.0
    r = arc["roughness_mm"] or 0.012
    if k in ("pipe", "resistor"):
        return L, D, r
    if k in ("valve", "shortPipe", "compressorStation"):
        return min(L, 0.001), max(D, 1000.0), r
    if k == "controlValve":
        # Passive/bypass quasi-transparent (GazFlow MVP, opening 100, Cv 100 -> scale 1).
        return min(L, 0.001), max(D, 1000.0), r
    return L, D, r


def compute_per_pipe_rho(net_path, pressures_file):
    """Per-pipe density rho(P_moy) matching GazFlow's pipe_resistance_at_pressure, evaluated
    at the mean of the endpoint pressures from a prior feasible-point JSON. Used to align the
    Pyomo K with the in-repo dynamic-rho K-linearization for warm-start isolation tests."""
    import json
    nodes, arcs = parse_net(net_path)
    pressures = json.load(open(pressures_file))
    out = {}
    for a in arcs:
        if a["kind"] not in ("pipe", "shortPipe", "resistor", "valve"):
            continue
        pf = pressures.get(a["from"])
        pt = pressures.get(a["to"])
        if pf is None or pt is None:
            continue
        pmoy = 0.5 * (pf + pt)
        out[a["id"]] = gas_density_kg_per_m3(pmoy)
    return out


# ---------- Model ----------

def build_and_solve(net_path, scn_path, solver="ipopt", eps=1e-3, tol=1e-4, seed=0,
                    dump_pressures=None, rho_eff=50.0, per_pipe_rho=None):
    import pyomo.environ as pyo
    import random

    nodes_raw, arcs_raw = parse_net(net_path)
    demands, p_bounds, fixed_p = parse_scn(scn_path)

    all_node_ids = list(nodes_raw.keys())
    # Node bounds: combine .net and scenario (tighter wins). Default [1.01325, 200] if none.
    def bounds_for(nid):
        npmin = nodes_raw[nid]["pmin"]
        npmax = nodes_raw[nid]["pmax"]
        spmin, spmax = p_bounds.get(nid, (None, None))
        lo = max(filter(lambda x: x is not None, [npmin, spmin, 1.01325]), default=1.01325)
        hi_candidates = [x for x in (npmax, spmax) if x is not None]
        hi = min(hi_candidates) if hi_candidates else 200.0
        if hi < lo:
            hi = lo
        return lo, hi

    m = pyo.ConcreteModel()
    m.NODES = pyo.Set(initialize=all_node_ids)
    m.P = pyo.Var(m.NODES, domain=pyo.NonNegativeReals, bounds=lambda m, i: bounds_for(i))

    # Gauge: fix slack(s).
    for nid, p in fixed_p.items():
        if nid in m.P:
            m.P[nid].fix(p)

    # Init pressures: uniform 70 bar for seed 0; random within bounds for multistart seeds.
    rng = random.Random(seed)
    for nid in all_node_ids:
        if m.P[nid].fixed:
            continue
        lo, hi = bounds_for(nid)
        base = 70.0 if seed == 0 else lo + (hi - lo) * rng.random()
        m.P[nid] = min(max(base, lo), hi)

    # Passive arcs (pipe/shortPipe/resistor/valve): P² law with flow var.
    passive = [a for a in arcs_raw if a["kind"] in ("pipe", "shortPipe", "resistor", "valve")]
    m.PASSIVE = pyo.Set(initialize=[a["id"] for a in passive])
    m.Q = pyo.Var(m.PASSIVE, domain=pyo.Reals, initialize=0.0)
    K = {}
    for a in passive:
        L, D, r = effective_geometry(a)
        if a["kind"] == "resistor" and a["drag"] is not None:
            # Resistor: model as a short pipe with drag-equivalent resistance.
            L, D, r = min(a["length_km"] or 0.001, 0.001), max(a["diameter_mm"] or 1000.0, 1000.0), r
        rho = rho_eff
        if per_pipe_rho is not None and a["id"] in per_pipe_rho:
            rho = per_pipe_rho[a["id"]]
        K[a["id"]] = pipe_resistance(L, D, r, rho_eff=rho)

    def pipe_law(m, a_id):
        a = next(x for x in passive if x["id"] == a_id)
        pu = m.P[a["from"]]
        pv = m.P[a["to"]]
        q = m.Q[a_id]
        return pu * pu - pv * pv == K[a_id] * q * pyo.sqrt(q * q + eps * eps)

    m.pipe_law = pyo.Constraint(m.PASSIVE, rule=pipe_law)

    # Compressor stations: P_out = r * P_in, r in [1, poutmax/pinmin], P_out <= poutmax.
    comps = [a for a in arcs_raw if a["kind"] == "compressorStation"
             and a["from"] in m.P and a["to"] in m.P]
    m.COMPS = pyo.Set(initialize=[a["id"] for a in comps])
    m.r = pyo.Var(m.COMPS, domain=pyo.NonNegativeReals, initialize=1.0)
    for a in comps:
        pinmin = a["p_in_min"] or 1.01325
        poutmax = a["p_out_max"] or 200.0
        rmax = poutmax / pinmin
        m.r[a["id"]].setlb(1.0)
        m.r[a["id"]].setub(max(rmax, 1.0))

    def comp_law(m, c_id):
        a = next(x for x in comps if x["id"] == c_id)
        return m.P[a["to"]] == m.r[c_id] * m.P[a["from"]]

    m.comp_law = pyo.Constraint(m.COMPS, rule=comp_law)

    def comp_cap(m, c_id):
        a = next(x for x in comps if x["id"] == c_id)
        poutmax = a["p_out_max"]
        if poutmax is None:
            return pyo.Constraint.Feasible
        return m.P[a["to"]] <= poutmax

    m.comp_cap = pyo.Constraint(m.COMPS, rule=comp_cap)

    # Control valves: reducer. P_out <= P_in, P_out <= poutmax, P_out >= pinmin.
    cvs = [a for a in arcs_raw if a["kind"] == "controlValve"
           and a["from"] in m.P and a["to"] in m.P]
    m.CVS = pyo.Set(initialize=[a["id"] for a in cvs])

    def cv_reduce(m, j_id):
        a = next(x for x in cvs if x["id"] == j_id)
        return m.P[a["to"]] <= m.P[a["from"]]

    m.cv_reduce = pyo.Constraint(m.CVS, rule=cv_reduce)

    def cv_cap(m, j_id):
        a = next(x for x in cvs if x["id"] == j_id)
        poutmax = a["p_out_max"]
        if poutmax is None:
            return pyo.Constraint.Feasible
        return m.P[a["to"]] <= poutmax

    m.cv_cap = pyo.Constraint(m.CVS, rule=cv_cap)

    def cv_floor(m, j_id):
        a = next(x for x in cvs if x["id"] == j_id)
        pinmin = a["p_in_min"]
        if pinmin is None:
            return pyo.Constraint.Feasible
        return m.P[a["to"]] >= pinmin

    m.cv_floor = pyo.Constraint(m.CVS, rule=cv_floor)

    # Mass conservation with slack (Phase-1 feasibility).
    # net injection at i = sum(Q into i) - sum(Q out of i) + (compressor/CV pass-through = 0).
    arcs_all = passive + comps + cvs
    in_arcs = defaultdict(list)
    out_arcs = defaultdict(list)
    for a in arcs_all:
        if a["from"] in m.P and a["to"] in m.P:
            out_arcs[a["from"]].append(a)
            in_arcs[a["to"]].append(a)

    # Flow vars for compressor/CV (pass-through, no fuel gas modeled).
    m.QC = pyo.Var(m.COMPS, domain=pyo.Reals, initialize=0.0)
    m.QV = pyo.Var(m.CVS, domain=pyo.Reals, initialize=0.0)
    m.S = pyo.Var(m.NODES, domain=pyo.Reals, initialize=0.0)

    def mass_balance2(m, i):
        net = 0.0
        for a in in_arcs[i]:
            if a["kind"] in ("pipe", "shortPipe", "resistor", "valve"):
                net += m.Q[a["id"]]
            elif a["kind"] == "compressorStation":
                net += m.QC[a["id"]]
            elif a["kind"] == "controlValve":
                net += m.QV[a["id"]]
        for a in out_arcs[i]:
            if a["kind"] in ("pipe", "shortPipe", "resistor", "valve"):
                net -= m.Q[a["id"]]
            elif a["kind"] == "compressorStation":
                net -= m.QC[a["id"]]
            elif a["kind"] == "controlValve":
                net -= m.QV[a["id"]]
        d = demands.get(i, 0.0)
        return net - d == m.S[i]

    m.mass_balance = pyo.Constraint(m.NODES, rule=mass_balance2)

    # Objective: Phase-1 feasibility = minimise sum of squared mass-conservation slacks.
    def obj(m):
        return sum(m.S[i] * m.S[i] for i in m.NODES)

    m.obj = pyo.Objective(rule=obj, sense=pyo.minimize)

    # Solve.
    opt = pyo.SolverFactory(solver)
    if not opt.available():
        print(f"ERROR: solver '{solver}' not available", file=sys.stderr)
        sys.exit(2)
    results = opt.solve(m, tee=True, load_solutions=True)

    # Evaluate feasibility.
    max_slack = max(abs(m.S[i]()) for i in m.NODES)
    sum_sq = sum(m.S[i]() ** 2 for i in m.NODES)
    # Bound violations.
    viol = []
    for nid in all_node_ids:
        if m.P[nid].fixed:
            continue
        p = m.P[nid]()
        lo, hi = bounds_for(nid)
        if p < lo - tol:
            viol.append((nid, p, lo, hi, lo - p, 0.0))
        elif p > hi + tol:
            viol.append((nid, p, lo, hi, 0.0, p - hi))
    feasible = (max_slack < tol) and (len(viol) == 0)
    print()
    print("=" * 60)
    print(f"solver: {solver}")
    print(f"max_mass_slack: {max_slack:.6e} Nm³/s")
    print(f"sum_sq_slack:   {sum_sq:.6e}")
    print(f"bound_violations: {len(viol)}")
    for v in viol[:10]:
        print(f"  {v[0]:12s} P={v[1]:.2f} [lo={v[2]:.2f}, hi={v[3]:.2f}] short={v[4]:.2f} excess={v[5]:.2f}")
    print(f"VERDICT: {'FEASIBLE (point found)' if feasible else 'NOT SOLVED (local)'}")
    print("=" * 60)
    # Print marginal sinks + entries.
    for nid in ("sink_88", "sink_83", "sink_108", "sink_122", "sink_125", "sink_109",
                "source_1", "source_14", "source_13", "source_10"):
        if nid in m.P:
            print(f"  P[{nid}] = {m.P[nid]():.3f} bar")
    if dump_pressures:
        import json
        pressures = {nid: float(m.P[nid]()) for nid in all_node_ids}
        with open(dump_pressures, "w") as fh:
            json.dump(pressures, fh, indent=0)
        print(f"  dumped {len(pressures)} node pressures -> {dump_pressures}")
    return m, feasible, max_slack


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--net", default="back/dat/GasLib-582.net")
    ap.add_argument("--scn", default="back/dat/Nominations-582-v2-20211129/nomination_mild_618.scn")
    ap.add_argument("--solver", default="ipopt")
    ap.add_argument("--eps", type=float, default=1e-3)
    ap.add_argument("--tol", type=float, default=1e-4)
    ap.add_argument("--multistart", type=int, default=1,
                    help="number of starts (seed 0 = uniform 70 bar; seeds 1..N = random). "
                         "Stops at first feasible point.")
    ap.add_argument("--dump-pressures", default=None,
                    help="write all node pressures as JSON {node_id: bar} to this path "
                         "(on the first feasible start, or the last start if none feasible).")
    ap.add_argument("--rho-eff", type=float, default=50.0,
                    help="effective gas density (kg/m³) for the P² resistance; "
                         "raise toward ~55 to match GazFlow's dynamic rho(P_moy) at ~70 bar.")
    ap.add_argument("--per-pipe-rho-from", default=None,
                    help="path to a pressures JSON {node_id: bar}; compute per-pipe rho(P_moy) "
                         "matching GazFlow's dynamic-rho K-linearization for warm-start isolation.")
    args = ap.parse_args()
    per_pipe_rho = None
    if args.per_pipe_rho_from:
        per_pipe_rho = compute_per_pipe_rho(args.net, args.per_pipe_rho_from)
        print(f"per-pipe rho: computed for {len(per_pipe_rho)} passive arcs "
              f"(min={min(per_pipe_rho.values()):.2f}, max={max(per_pipe_rho.values()):.2f} kg/m³)")
    if args.multistart <= 1:
        build_and_solve(args.net, args.scn, args.solver, args.eps, args.tol, seed=0,
                        dump_pressures=args.dump_pressures, rho_eff=args.rho_eff,
                        per_pipe_rho=per_pipe_rho)
        return
    best = None
    for s in range(args.multistart):
        print(f"\n##### multistart seed {s} #####")
        _, feas, slack = build_and_solve(args.net, args.scn, args.solver, args.eps, args.tol,
                                         seed=s, rho_eff=args.rho_eff, per_pipe_rho=per_pipe_rho)
        if best is None or slack < best[1]:
            best = (s, slack, feas)
        if feas:
            build_and_solve(args.net, args.scn, args.solver, args.eps, args.tol, seed=s,
                            dump_pressures=args.dump_pressures, rho_eff=args.rho_eff,
                            per_pipe_rho=per_pipe_rho)
            print(f"\n>>> FEASIBLE point found at seed {s}; stopping multistart.")
            return
    print(f"\n>>> No feasible point found in {args.multistart} starts. "
          f"Best: seed {best[0]} slack={best[1]:.3e} feasible={best[2]}")


if __name__ == "__main__":
    main()
