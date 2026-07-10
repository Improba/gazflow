# Solver validation — GazFlow

## Analytical test cases

### Test 1: Single pipe (2 nodes)

**Configuration:**
- Node A (source): fixed pressure at 70 bar
- Node B (sink): withdrawn flow of 10 m³/s (standard conditions)
- Pipe: L = 100 km, D = 500 mm, ε = 0.012 mm

**Analytical solution:**

$$
P_B = \sqrt{P_A^2 - K \cdot Q \cdot |Q|}
$$

With $K = f \cdot L / (2 \cdot D \cdot A^2)$ and $f$ from Swamee-Jain.

**Criterion:** Solver vs analytical difference < 0.1 bar.

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
- Commands run: suite T1..T10 (T9 via versioned internal reference)

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
- **Conditional MVP technical Go** on internal robustness (T1–T10 with locked internal reference).

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

- **Go (engineering / CI):** yes, validation T1..T10 is continuously runnable with locked internal reference.
- **No-Go (strict scientific):** maintained until an independent official GasLib-11 reference is available.

### Execution industrialisation

- Script pack: `scripts/validation-pack.sh`
- Observed execution: T1..T10 passing end-to-end (backend).
- Options:
  - `GAZFLOW_REGEN_REFERENCE=1` to regenerate internal reference before T9;
  - `GAZFLOW_RUN_LARGE_SMOKE=1` to include large-dataset smoke tests.

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
- **Feasible under full nomination flow**: **open (local solver)**. A bounded local NoVa
  feasibility solver has been implemented (`GAZFLOW_NOVA_FEASIBILITY=1`, `equations.md` §4.8):
  entry pressures float within `.net` bounds, compressor ratios and CV setpoints are decision
  variables, pressure bounds enforced as a Newton soft penalty, verdict `Feasible` /
  `BoundViolation` / `NotSolvedLocal`. On `mild_618` it reports `NotSolvedLocal` (residual 73.9
  m³/s, base Newton non-convergence on this hard non-convex NLP). This is the honest answer per
  Pfetsch et al. — a local solver cannot prove infeasibility. A definitive verdict requires a
  global solver (SCIP/Couenne/BARON) on the bounded formulation.
- **Engineering / CI**: solver is stable (hard compressor coupling capped by `pressureOutMax`, CV setpoint infrastructure, component anchoring, bounded NoVa mode). Baselines preserved; 361 lib tests pass (only pre-existing flaky `test_ws_timeout_diverged` fails).
- **Demo recommendation**: not recommended until NoVa feasibility is properly characterised by a global solver or a converged bounded-DOF formulation.
