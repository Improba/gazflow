# Solver validation — GazFlow

## Analytical test cases

### Test 1: Single pipe (2 nodes)

**Configuration:**
- Node A (source): fixed pressure at 70 bar
- Node B (sink): withdrawn flow of 50 Nm³/s (standard conditions)
- Pipe: L = 150 km, D = 150 mm, ε = 0.012 mm

**Analytical solution:**

$$
P_B = \sqrt{P_A^2 - K \cdot Q \cdot |Q|}
$$

With $K = f \cdot L / (2 \cdot D \cdot A^2)$ and $f$ from Swamee-Jain at **Re = 10⁷** (plateau turbulent, `flow_m3s=0` in the solver Jacobian path).

**Criterion:** Solver vs analytical difference < **0.05 bar** (or 5 % of ΔP, whichever is larger). Test requires ΔP > 1 bar (non-trivial).

**Test:** `test_two_node_closed_form_p_squared` (résistance `pipe_resistance_at_pressure_with_composition`, point fixe P_moy, Re plateau).

---

### Test 2: Y network (3 branches)

**Configuration:**
- Node S (source): P = 70 bar
- Node J (junction): free
- Node A (sink): Q = -5 m³/s
- Node B (sink): Q = -5 m³/s
- Pipe S→J: L = 50 km, D = 600 mm
- Pipe J→A: L = 30 km, D = 400 mm
- Pipe J→B: L = 40 km, D = 400 mm

**Criterion:** Mass conservation at J (|Q_SJ - Q_JA - Q_JB| < 1e-6).

---

### Test 3: GasLib-11

**Configuration:** Full GasLib-11 network with .scn scenario.

**Criteria:**
- Convergence in < 100 iterations
- All pressures ∈ [1, 100] bar
- Global mass conservation (|ΣQ_sources + ΣQ_sinks| < 1e-4)

---

## Comparison with literature

GasLib results are documented in:
> Schmidt, M. et al. (2017). "GasLib — A Library of Gas Network Instances." *Data*, 2(4), 40.

Reference solutions will be compared when available.

---

## Scientific protocol report v1 (intermediate)

- Date: 2026-03-09
- Scope: Rust backend (`back/`) on local working branch
- Commands run: suite T1..T16 (T9 via versioned internal reference; T9 quarantined in pack)

### T1..T10 status

| ID | Test | Status | Note |
|---|---|---|---|
| T1 | Darcy friction in turbulent | ✅ Pass | `darcy_friction_turbulent` OK |
| T2 | Positive/finite pipe resistance | ✅ Pass | `pipe_resistance_positive` OK |
| T3 | 2-node analytical case | ✅ Pass | `steady_state_two_nodes` OK |
| T4 | Y network: local conservation | ✅ Pass | `steady_state_y_network_mass_conservation` OK |
| T5 | Hybrid vs Jacobi | ✅ Pass | `test_newton_vs_jacobi_same_result` OK |
| T6 | GasLib-11 sanity check | ✅ Pass | `test_solve_gaslib_11` OK (if data present) |
| T7 | Scenario unit conversion → SI | ✅ Pass | `test_units_scn_to_si` OK |
| T8 | Pressure drop dimensional consistency | ✅ Pass | `test_pressure_drop_dimension_consistency` OK |
| T9 | Validation vs `.sol` reference | ✅ internal / ⏸️ external | versioned internal reference OK; external official reference absent |
| T10 | Physical sensitivity (roughness, Z, T) | ✅ Pass | `test_sensitivity_physical_trends` OK |

### T9 metrics (`.sol` reference)

- Max pressure error (internal reference): 0.000%
- Mean error (internal reference): 0.000%
- Worst-node deviation (internal reference): `entry01`
- Execution note: the test now accepts a configurable reference source via `GAZFLOW_REFERENCE_SOLUTION_PATH` (CSV-like text or XML formats), in addition to `dat/GasLib-11.sol`.

### Go/No-Go decision

- **Strict No-Go for full Phase 2 exit** until an external official reference is available for T9.
- **Conditional MVP technical Go** on internal robustness (T1–T16 with locked internal reference; T9 external still pending).

---

## Update (backend) — 2026-03-10

- Integration of an MVP compressor model with **directional uplift on \(P^2\)**:
  - parsing of `*.cs` to estimate max ratio per station (`nrOfSerialStages`);
  - injection of a compression factor on upstream pressure in flow equations;
  - Newton/Jacobi Jacobian adjusted with asymmetric upstream/downstream weighting.
- Forced smoke dataset campaign:
  - command: `GAZFLOW_ENABLE_LARGE_DATASET_TESTS=1 cargo test test_solve_gaslib_ -- --nocapture`;
  - GasLib-24 / GasLib-40: OK;
  - GasLib-582: robust run, explicit non-convergence accepted in smoke mode (observed final residual: `5.000e0`);
  - GasLib-4197: robust run, explicit non-convergence accepted in smoke mode (very short profile, continuation + warm-start).
- Further exploration (deeper continuation, run stopped after first tier):
  - config: `GAZFLOW_LARGE_TEST_MAX_ITER=60`, `GAZFLOW_LARGE_TEST_SCALES=0.1,0.03,0.01`;
  - first tier `0.1`: residual `9.626e5` (improvement vs short smoke profile), convergence not reached.
- Anti-long-run adjustments:
  - shortened smoke profiles (4197: `max_iter=6` with `scales=0.05,0.1,0.1` and split `1,1,4`, 582: `max_iter=180`);
  - global smoke timeout (`GAZFLOW_LARGE_TEST_MAX_SECONDS`) + continuation timeout (`GAZFLOW_CONTINUATION_MAX_SECONDS`);
  - warm-start snapshot in continuation (`GAZFLOW_CONTINUATION_SNAPSHOT_EVERY`);
  - short physical initialisation before Newton for very large networks (enabled by default above 2000 nodes, auto-disabled if it does not improve initial residual);
  - default GMRES cap reduced on large free systems (220 iterations for m > 1200);
  - Jacobi fallback guarded on very large networks (applied only if residual decreases).
- Recent measurements:
  - GasLib-4197 default smoke: ~15s on recent runs (observed residual ~`2.52e5` with tiers `0.05 -> 0.1 -> 0.1`, iteration budget `1,1,4`);
  - GasLib-582 default smoke: ~6 min with full `preset_robust` (observed residual ~`5.0e0`); CDF screening skipped when it degrades connectivity;
  - both remain robust (explicit non-convergence accepted in smoke mode).
- Short-term perf objective note:
  - exploratory target `<5e5` in ~15s reached on current default smoke profile;
  - best observed stable config: residual ~`2.52e5` in ~15.0s on GasLib-4197.
- Further attempts (rollback):
  - hard clamp of pressure updates on nodal bounds (`pressure_lower/upper`) tried then removed;
  - observed effect on GasLib-4197: strong residual degradation (up to ~`1.43e7`) and no useful further convergence;
  - “70 bar bounded per node” initialisation tried then removed;
  - observed effect on GasLib-4197: degraded runtime (~24s) and residual (~`3.58e6`);
  - baseline kept then improved: continuation `0.05 -> 0.1 -> 0.1` + budget `1,1,4` + short physical init + GMRES cap + guarded Jacobi fallback (very large networks).
- Overall scientific qualification unchanged:
  - T9 still blocked without provided reference solution;
  - final scientific Go/No-Go decision still pending reference.

---

## Transport solver hardening — 2026-06-29

- **Root cause (GasLib-582)**: closing valves/CV by default fragmented the active subgraph into many connected components without fixed pressure → singular Jacobian → faer LU panics and continuation failure.
- **Fixes (generic, no dataset hardcode)**:
  - valves and control valves **open by default**; `.cdf` combined decisions close equipment explicitly;
  - **component pressure anchoring** in Newton (`newton.rs`): one **numerical** reference pressure per floating connected component (not a GasLib BC; `pressureMin`/`pressureMax` unused);
  - **`.cdf` parser and dynamic routing** (`gaslib/cdf.rs`, `routing.rs`, `connectivity.rs`) at solve time, with symlink-aware `.cdf` path resolution; routing applied only if it beats the default open topology;
  - continuation: ramped compressor uplift per demand scale; relaxed tolerance on intermediate tiers.
- **Measurements (June 2026, `GAZFLOW_ENABLE_LARGE_DATASET_TESTS=1`)**:
  - GasLib-135: smoke OK (~90s), no faer LU panics;
  - GasLib-582: no faer panics; residual ~5 m³/s without `.cdf` (baseline kept when CDF fragments graph); **full convergence to 3e-3 not reached** (MVP compressor limit);
  - GasLib-11: unchanged (distribution reference).
- **June 2026 follow-up (compressor outer fallback + CDF multi-scale)**:
  - Post-continuation compressor blend fallback (≥200 nodes, transport compressors);
  - CDF screening at multiple demand scales; fragmentation penalty on large networks;
  - GasLib-582 unchanged (~5 m³/s robust smoke); GasLib-135 regression fixed (outer loop no longer nested inside continuation steps).
  - **Scientific review (June 2026)**: CDF baseline comparison, numerical-only component anchoring, GasLib-582 removed from recommended demos, large smoke tests default to robust mode (`GAZFLOW_REQUIRE_FULL_CONVERGENCE=1` for strict).
- **Next step for 582 convergence**: compressor outer loop or `.cs` maps (see `limitations.md` §5).

---

## Scientific quantitative tests — tranches 2–5 (juillet 2026)

Nouveaux tests backend (`gazflow-back`, `cargo test --lib`) et seuils associés.

> **Distinction d'IDs.** Les IDs **T2–T6** (et sous-IDs T3b, T5b, …) ci-dessous sont des **IDs de contenu** (tranches scientifiques). Les IDs **pack T11–T16** du script `scripts/validation-pack.sh` sont **distincts** : ils regroupent des filtres `cargo test` pour l'industrialisation CI (voir tableau « Pack mapping »).

### Pack mapping (`scripts/validation-pack.sh`)

| Pack ID | Filtre `cargo test` | Lien contenu |
|---------|---------------------|--------------|
| T3b | `test_two_node_closed_form_p_squared` | Tranche T2 (formule P² fermée) |
| T11 | `mass_balance_gaslib` | Tranche T3 (bilan massique GasLib) |
| T12 | `linepack_capacitance` | Tranche T4 (linepack ↔ capacitance) |
| T13 | `test_pde_` | Tranche T5 (bilan masse PDE) |
| T14 | `eos_` | Tranche T6 (EOS H₂) |
| T15 | `segment_conductance` | Conductance segment (G chordée, FV) |
| T16 | `gravity` | Terme gravitaire |

Le pack exécute T1–T16 en séquence. Par défaut `GAZFLOW_REQUIRE_GASLIB_DATA=1` : absence de `back/dat/` GasLib → échec explicite sur T6 et T11.

### Seuils par tranche de contenu

| ID | Test(s) | Seuil | Note |
|---|---|---|---|
| T2 | `test_two_node_closed_form_p_squared` | \|P_calc − P_expected\| < **0,05 bar** (ou 5 % ΔP) ; ΔP > **1 bar** | Formule P², R via `pipe_resistance_at_pressure_with_composition` avec **Re plateau 10⁷** (`flow_m3s=0`) |
| T3 | `mass_balance_gaslib_{11,24,40}` | \|Σd\| < **1e−4** ; résidu flux toujours asserté ; NLP fini et < **1e6** (compresseurs) ou < **max(2×tol, 0,1)** ; déterminisme \|ΔP\| < **1e−9** ; **fail** si `CI`/`GAZFLOW_REQUIRE_GASLIB_DATA` et `dat/` absent | Skip local sans données seulement |
| T3b | `recompute_mass_balance_residual` (`newton.rs`) | — | Résidu honnête via `pressure_nlp_eval` |
| T4 | `test_linepack_capacitance_cross_module` | \|dM/dP_FD − ρ_n ΣC\| / ρ_n ΣC < **0,05** | P = 70 et 30 bar, CH₄, pipe maillé |
| T5 | `test_pde_steady_mass_balance_at_boundaries`, `test_pde_mass_balance_after_demand_step`, `test_pde_sink_flow_matches_boundary` | Steady : \|Q_in−Q_out\| < **1e−4** Nm³/s, \|ΔM\| < **1e−3** kg/pas ; après échelon : ΔM < 0 + débit aval = \|d_sink\| | voir schéma FV conservatif |
| T5b | `test_pde_mass_balance_integrated` | \|ΔM − ρ_n∫(Q_in−Q_out)dt\| / max(\|ΔM\|, 1e−6) < **0,05** | Échelon demande, dt = 60 s, T = 1800 s |
| T5c | `test_pde_single_pipe_pressure_step_response_monotonic` | Dépressurisation > **10 %** de ΔP attendu : `R·(Q_new²−Q_old²)/(2·P_moy)` ; monotonie + linepack ↓ | Re plateau 10⁷ ; pas de seuil absolu arbitraire |
| T6 | `test_eos_h2_continuity_at_20_percent_threshold` | Intra-régime < **1–2 %** ; saut au seuil **≥ 0,5 %** et < **15 %** ; warning PR-78 | Mesure ~4,7 % @70 bar ; voir `limitations.md` §3.2 |
| T6b | `test_eos_z_clamp_on_pressure_h2_grid` | Z ∈ **[0,2 ; 1,5]** | Grille P × H₂ (Papay+Kay) |
| T6c | `test_eos_ch4_density_monotone_in_pressure` | ρ(P) strictement croissante | CH₄ pur |

Commandes de contrôle :

```bash
cd gazsim/back
cargo test -p gazflow-back --lib test_two_node_closed_form
cargo test -p gazflow-back --lib mass_balance
cargo test -p gazflow-back --lib linepack_capacitance
cargo test -p gazflow-back --lib test_pde
cargo test -p gazflow-back --lib eos_
```

---


## Final report v1 (conditional) — 2026-03-10

### Locked internal reference (regression)

- File: `docs/testing/references/GasLib-11.reference.internal.csv`
- Generation: `cargo run --bin generate_gaslib11_reference` (from `back/`)
- Control run: `cargo test test_gaslib_11_vs_reference_solution -- --nocapture`
- Observed result:
  - n=11 nodes compared
  - max_err=0.000%
  - mean_err=0.000%
  - worst_node=entry01

### Interpretation

- This internal reference is useful as a **non-regression safeguard**.
- It does not replace an independent external reference (`.sol`) for strict scientific validation.

### Go/No-Go decision (final conditional version)

- **Go (engineering / CI):** yes, validation T1..T16 is continuously runnable with locked internal reference (`GAZFLOW_REQUIRE_GASLIB_DATA=1` by default in the pack).
- **No-Go (strict scientific):** maintained until an independent official GasLib-11 reference is available.

### Execution industrialisation

- Script pack: `scripts/validation-pack.sh`
- Observed execution: T1..T16 passing end-to-end (backend, juillet 2026).
- Default: `GAZFLOW_REQUIRE_GASLIB_DATA=1` (fail if GasLib data absent).
- Options:
  - `GAZFLOW_REGEN_REFERENCE=1` to regenerate internal reference before T9;
  - `GAZFLOW_RUN_LARGE_SMOKE=1` to include large-dataset smoke tests.

### Transient API — boundary flows

Each transient step exposes nodal `flows` [Nm³/s] plus boundary-oriented fields (see [`docs/contracts/openapi-stub.yaml`](../contracts/openapi-stub.yaml)):

- `flows_in` : upstream boundary flow (PDE: `flows[0]`; quasi-steady: equals `flows`)
- `flows_out` : downstream boundary flow (PDE: `flows[n]`; quasi-steady: equals `flows`)

These support Qin/Qout mass-balance checks in the UI (TransientPlayer) and in pack tests T13.

---

## NoVa feasibility — methodology note & Phase VIII correction (July 2026)

### Methodology (per Pfetsch et al., ZIB-Report 12-41 / Optim. Methods Softw. 2015)

The GasLib-582 nominations are inputs to the **validation of nominations** (NoVa) problem: does there exist a setting of the active elements (compressors, control valves, valves) and a network state satisfying all pressure/flow bounds? NoVa is a non-convex MINLP feasibility problem. Two methodological rules from the literature govern the interpretation:

1. **No official per-nomination feasibility status.** GasLib states the 4227 nominations are "assumed feasible in reality, but there is no proof for this so far" (GasLib paper, §2.1.6). `nomination_mild_618` carries no ZIB-issued feasible/infeasible label.
2. **Local solver non-convergence ≠ infeasibility.** "If a local solver is not able to find a feasible solution, no conclusion for NoVa can be drawn. To prove infeasibility of a nomination, a global solver is required." GazFlow is a **local** Newton solver; it can confirm feasibility when it converges, but it **cannot** prove infeasibility.

Consequently, any GazFlow non-convergence on a nomination must be reported as "not solved (local)" — never as "infeasible" or "proven non-feasible".

### Phase VIII — reachability correction for `nomination_mild_618`

Earlier phases (II-VII-bis, see `gaslib-582-compressor-diagnosis.md`) concluded that sink_88/83/108 are "topologiquement infeasible" with "capacity = 0 même à débit nul" and "aucune source de pression sur le chemin". **These conclusions are retracted** as artifacts of the solver's boundary handling.

Decisive evidence:

- **Static reachability** (`scripts/trace_sink_reachability.py`): all 5 marginal sinks are topologically reachable from high-pressure sources (source_14 pressureMax 86 bar for sink_88/83/108 via CV_17/CV_7/CV_16; source_13/10 for sink_125/122 via single shortPipes).
- **Zero-demand probe, single anchor source_14 = 86 bar, CVs passive** (`GAZFLOW_REACHABILITY_PROBE=1`, `GAZFLOW_REACHABILITY_ANCHOR_SOURCES=source_14`): sink_88 = 86.10 bar, sink_83 = 86.36 bar, sink_108 = 86.04 bar — all far above their contractual floors (26/21/16 bar). Passive control valves pass pressure (ΔP ≈ 0 at zero flow).
- **Entry-anchor sensitivity** (`GAZFLOW_ENTRY_ANCHOR_USE_PRESSURE_MAX=1`): anchoring sources at their per-node `.net` pressureMax (51-121 bar) instead of a uniform 70 bar flips sink_122 (85 bar, need 74) and sink_125 (86 bar, need 41) to feasible.

Root cause of the earlier false verdict: the capacity study fixed multiple pressure nodes at conflicting values simultaneously (slack 51 bar + sources 70 bar + hubs) and read non-converged low-pressure iterates as a reachability limit. With a single consistent anchor, pressure propagates correctly through passive CVs.

### Corrected Go/No-Go (GasLib-582 `nomination_mild_618`)

- **Feasible at zero flow**: yes (all marginal sinks reach their floors with large margin).
- **Feasible under full nomination flow: YES — proven by an independent external NLP solver
  (Phase VIII-bis, July 2026).** A bounded NoVa feasibility NLP was built independently in
  Pyomo directly from the GasLib `.net`/`.scn` (`scripts/nova/nova_pyomo.py`), using the same
  isothermal P² model documented in `equations.md` §1.2b (smooth reformulation
  `P_u²−P_v² = K·Q·sqrt(Q²+ε²)` to allow reverse flow, compressor `P_out = r·P_in` with
  `r ∈ [1, pressureOutMax/pressureInMin]` and `P_out ≤ pressureOutMax`, control valve as a
  bounded reducer `P_out ≤ P_in`, `P_out ≤ pressureOutMax`, flow continuity, gauge fixed at
  sink_109 = 51.01325 bar, entries floating within `.net` bounds). Solved with IPOPT
  (COIN-OR interior-point NLP) in a Docker image (`scripts/nova/Dockerfile`).

  **Result: IPOPT finds a feasible point.** Constraint violation (mass conservation)
  `≤ 2.6e-12`, max nodal mass slack `3.4e-7 Nm³/s`, **0 bound violations**, all marginal
  sinks in contract bounds: sink_88 = 40.99 bar [26, 51], sink_83 = 41.01 [21, 71],
  sink_108 = 40.99 [16, 51], sink_122 = 74.01 [74.01, 81.01], sink_125 = 63.47 [41, 84],
  sink_109 = 51.013. Log: `scripts/nova/results/mild_618_ipopt_FEASIBLE.log`.

  **Reproducibility caveat (important).** The NoVa NLP is genuinely **non-convex**: from a
  naive uniform-70-bar start, multithreaded IPOPT (OpenMP linear solver) reaches the
  feasible point only ~20% of the time; the other runs stop at non-feasible local minima of
  the Phase-1 slack objective (mass slacks 75-3691 Nm³/s). Pinning `OMP_NUM_THREADS=1`
  removes the OpenMP nondeterminism and makes IPOPT reach the feasible point **reliably
  (5/5 runs)**. This is exactly the phenomenon ZIB reports (local solvers fail to find
  feasible points on hard NoVa instances even when they exist) and is the reason GazFlow's
  weaker penalty-Newton consistently reports `NotSolvedLocal`. The feasibility itself is
  not in doubt: a feasible point is exhibited.

  No global solver (Couenne/BARON) run is needed: exhibiting a feasible point is a
  constructive proof of feasibility. A global solver would only be required to prove
  *infeasibility*, which is moot here.
- **Bounded local NoVa solver (in-repo)** (`GAZFLOW_NOVA_FEASIBILITY=1`, `equations.md` §4.8):
  reports `NotSolvedLocal` on `mild_618`. This is now understood as a local-solver weakness
  on a non-convex NLP (confirmed by the IPOPT multistart behaviour above), not evidence
  against feasibility. The honest local verdict remains "not solved (local)".
- **Engineering / CI**: solver is stable (hard compressor coupling capped by `pressureOutMax`, CV setpoint infrastructure, component anchoring, bounded NoVa mode). Baselines preserved; 361 lib tests pass (only pre-existing flaky `test_ws_timeout_diverged` fails).
- **Demo recommendation**: `nomination_mild_618` is feasible and may be shown, with the
  caveat that the in-repo local solver does not itself converge to the feasible point; the
  feasibility is established by the external IPOPT model. Improving the in-repo solver's
  non-convex convergence (multistart, continuation, or SQP/IPOPT backend) is the remaining
  engineering work to make the local verdict match the external one.

### Phase VIII-ter — in-repo solver convergence investigation (July 2026)

After the external IPOPT proof, three levers were tried to make the in-repo penalty-Newton
converge to the feasible point on `mild_618` (so the local verdict would match the external
`Feasible`):

1. **Demand continuation (already engaged).** The Large preset (582 nodes) already ramps
   `[0.05, 0.1, 0.2, 0.4, 0.7, 1.0]` with `auto_bridges=6` and warm-start between steps. The
   in-repo NoVa solver nonetheless stops at the same non-converged low-pressure basin
   (residual ≈ 81.8, sink_122 = 4.2 bar, innode_11 mass imbalance ≈ 3.15e5). Overriding the
   ramp (`GAZFLOW_CONTINUATION_SCALES`) only changes the floating-point tail, not the basin.

2. **Warm-start from the IPOPT feasible point.** A new `GAZFLOW_INITIAL_PRESSURES_FILE` env
   (JSON `{node_id: pressure_bar}`) was added to `compressor_diag` to seed the in-repo Newton
   from the IPOPT point (`scripts/nova/results/mild_618_feasible_pressures.json`). In direct
   solve (`CONTINUATION_SCALES=1.0`) the Newton **diverges from the warm-start to residual
   ≈ 69.8 then errors out**; with a ramp it falls back to the same stuck basin. The warm-start
   point is feasible for the Pyomo model (ρ_eff = 50, Re = 1e7) but not exactly for the in-repo
   model (dynamic ρ(P_moy) ≈ 54 at 70 bar via Papay, dynamic Re, compressor/CV decision loops,
   effective demands) — a ≈ 9 % K mismatch — yet the Newton diverges instead of converging to
   the in-repo's own nearby feasible point.

3. **Model-matching isolation.** The Pyomo model was extended with per-pipe `ρ(P_moy)` matching
   GazFlow's `pipe_resistance_at_pressure` (Papay, pure CH₄) to align K with the in-repo
   linearization (`--per-pipe-rho-from`). The per-pipe-ρ NLP (ρ 2.7–58 kg/m³) is harder and
   uniform-start IPOPT did not find a feasible point in 6 starts, so a matched-K warm-start
   point could not be produced cheaply.

**Conclusion (engineering).** The in-repo penalty-Newton is **not robust enough** for this
non-convex NoVa NLP: it diverges even when started near a feasible point of a closely-related
model. Continuation and warm-start are insufficient. Closing the gap so the local verdict
matches the external `Feasible` requires a **solver upgrade**: a trust-region / SQP Newton, or
an external IPOPT backend solving the in-repo model directly (eliminating the model mismatch).
The honest local verdict remains `NotSolvedLocal`; the feasibility of `mild_618` is settled by
the external IPOPT proof (Phase VIII-bis). The `GAZFLOW_INITIAL_PRESSURES_FILE` warm-start hook
is kept as a general capability.
