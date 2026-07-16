# NoVa feasibility — external NLP verification (Phase VIII-bis)

Independent bounded NoVa feasibility NLP, built directly from GasLib `.net`/`.scn` files and
solved with an external solver. Used to settle the feasibility question of
`GasLib-582` `nomination_mild_618` without the circularity of GazFlow checking its own model.

## Files

- `nova_pyomo.py` — Pyomo model (isothermal P², smooth `Q·sqrt(Q²+ε²)` reformulation,
  compressor ratio, CV reducer, mass conservation, gauge fixed at the scenario slack) and
  Phase-1 feasibility solve. Pins `OMP_NUM_THREADS=1` for reproducibility.
- `Dockerfile` — conda-forge image with Pyomo + IPOPT.
- `results/mild_618_ipopt_FEASIBLE.log` — canonical feasible solve for `mild_618`.

## Run

```bash
# build the solver image (one-off)
docker build -t gazflow-nova scripts/nova

# solve mild_618 (paths are relative to the gazsim/ mount)
docker run --rm -v "$(pwd)":/work gazflow-nova /work/scripts/nova/nova_pyomo.py \
    --net  /work/back/dat/GasLib-582.net \
    --scn  /work/back/dat/Nominations-582-v2-20211129/nomination_mild_618.scn \
    --solver ipopt

# multistart (robustness on other / harder nominations)
docker run --rm -v "$(pwd)":/work gazflow-nova /work/scripts/nova/nova_pyomo.py \
    --net /work/back/dat/GasLib-582.net \
    --scn /work/back/dat/Nominations-582-v2-20211129/nomination_mild_618.scn \
    --multistart 8
```

## Verdict for `nomination_mild_618`

**FEASIBLE.** IPOPT exhibits a feasible point under the full nomination (255.6 Nm³/s to the
slack sink_109): mass-conservation violation ≤ 2.6e-12, max nodal slack 3.4e-7 Nm³/s, 0 bound
violations, all marginal sinks in contract bounds (sink_88 40.99 bar [26,51], sink_83 41.01
[21,71], sink_108 40.99 [16,51], sink_122 74.01 [74,81], sink_125 63.47 [41,84]).

The NLP is non-convex: multithreaded IPOPT reaches the feasible point only ~20% of runs from a
naive uniform start (others stop at non-feasible local minima of the Phase-1 slack objective);
`OMP_NUM_THREADS=1` makes it reliable (5/5). This is the same phenomenon that makes GazFlow's
in-repo penalty-Newton report `NotSolvedLocal`. Feasibility itself is settled by exhibiting a
point; no global solver (Couenne/BARON) is required. See `docs/science/validation.md`
(Phase VIII-bis) and `docs/testing/gaslib-582-compressor-diagnosis.md`.

## Model notes / approximations

- Pipe resistance `K = f·L·ρ_eff/(2·D·A²·1e10)` bar²·s²/m⁶ with `ρ_eff = 50` kg/m³ (~70 bar
  CH₄) and Swamee-Jain `f` at `Re = 1e7` — matches GazFlow's MVP `pipe_resistance()` in
  `solver/steady_state.rs`. The dynamic `ρ(P_moy)`/Re refinement is not replicated here; this
  is a standard HP-gas approximation sufficient for a feasibility verdict.
- Resistor arcs are approximated as quasi-transparent short pipes (drag factor not converted
  to an exact K); few (8) and small effect.
- Compressor fuel gas not modeled (flow continuity `Q_in = Q_out`).
- Control valve modeled as a bounded reducer (`P_out ≤ P_in`, `P_out ≤ pressureOutMax`,
  `P_out ≥ pressureInMin`); the active-setpoint/bypass complementarity is relaxed — sufficient
  to find a feasible point.
- Valves assumed open (NLP relaxation; the MINLP binary decision is not exercised).

## Options

- `--dump-pressures PATH` — write the solved pressures as JSON `{node_id: bar}` (all nodes).
  Used to feed the in-repo solver's warm-start hook.
- `--multistart N` — run N starts (seed 0 = uniform 70 bar; seeds 1..N = random within
  bounds); stop at the first feasible point. The NoVa NLP is non-convex, so multistart
  improves the chance of finding a feasible point.
- `--rho-eff RHO` — override the effective density (default 50; GazFlow's dynamic ρ is ≈ 54
  at 70 bar). Note: higher ρ makes the naive-start solve harder (the feasible manifold
  shifts); combine with `--multistart`.
- `--per-pipe-rho-from FILE` — compute per-pipe `ρ(P_moy)` (Papay, pure CH₄, matching
  GazFlow's `pipe_resistance_at_pressure`) from a prior pressures JSON, to align K with the
  in-repo dynamic-rho linearization. Used for the warm-start isolation experiment
  (Phase VIII-ter, `docs/science/validation.md`).

## Warm-starting the in-repo solver (Phase VIII-ter)

The in-repo `compressor_diag` accepts `GAZFLOW_INITIAL_PRESSURES_FILE=PATH` (JSON
`{node_id: pressure_bar}`) to seed its Newton from an external point. Combined with
`GAZFLOW_CONTINUATION_SCALES=1.0` (direct solve) and the `phase-viii-ter-warmstart` bench tag,
this tests whether the in-repo penalty-Newton can converge from the IPOPT feasible point. It
cannot (it diverges to residual ≈ 69.8 then errors) — the penalty-Newton is not robust enough
on this non-convex NLP; a trust-region/SQP or IPOPT backend upgrade is needed. See
`docs/science/validation.md` (Phase VIII-ter).

## Smoke in-repo IPOPT escalation (`nlp-ipopt`)

Le feature Cargo `nlp-ipopt` lie la lib C Ipopt (`coinor-libipopt-dev`). **Pas installé sur l'hôte par défaut** — utiliser l'image `gazflow-ipopt` :

```bash
# one-off image (si absente)
docker build -t gazflow-ipopt -f docker/Dockerfile.ipopt docker/

# micro-smoke : check + gates GAZFLOW_NOVA_IPOPT_ESCALATION + FFI two_node
./scripts/nova/smoke-ipopt-escalation.sh
```

Variables :

| Env | Rôle |
|-----|------|
| `GAZFLOW_NOVA_IPOPT_ESCALATION` | `off` (défaut) / `on`\|`1`\|`true` / `on-notsolved` |
| *(pas de `GAZFLOW_NOVA_IPOPT`)* | n'existe pas ; ne pas confondre |

`finalize_nova_verdict` (API) tente IPOPT seulement si signature `Unresolved` et mode On / OnNotSolved. `compressor_diag` **ne branche pas** cette escalade.
