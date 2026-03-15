# Implementation plan — GazFlow MVP

> Convention note: local working version of plans is in `docs/temps/plans/` (unversioned). `docs/plans/` is for shared plans.

## Objective

Simulate steady-state flow on a small GasLib network (GasLib-11, 11 nodes) and visualise results (pressures, flows) **in real time** on a CesiumJS globe, with solver log streaming and progressive 3D map updates. The MVP must also allow **result export** (JSON/CSV) and guarantee a **smooth experience** (map interaction + panel without perceptible lag).

## Cross-cutting requirements (non-negotiable)

- **Result export:** every converged simulation must be exportable with pressures, flows, metadata (timestamp, scenario/demands, units, iterations, residual).
- **Smooth UX:** Cesium navigation (pan/zoom/rotate) and live updates must stay smooth (no UI freezes, no visible WS backlog to the user).
- **Operational readability:** legends, units and states (running/converged/cancelled/error) must remain visible even under load.

---

## Phase 0: Bootstrap (day 1) ✅

### Tasks

- [x] Create monorepo structure (`back/`, `front/`, `docs/`)
- [x] Initialise Rust project (Cargo.toml, modules)
- [x] Initialise Quasar + CesiumJS project
- [x] Write AGENTS.md
- [x] Docker Compose (back + front, shared volumes)
- [x] First `cargo check` without error
- [x] First `npm install` + `quasar build` without error
- [ ] Download GasLib-11 into `back/dat/`

### Automated tests

```bash
# T0-1: Backend compiles
cd back && cargo check

# T0-2: Frontend build
cd front && npm install && npx quasar build
```

---

## Phase 1: GasLib parser + graph (days 2–4)

### Source data

- **GasLib-11:** 11 nodes, ~12 pipes, 1 compressor station, GPS coordinates.
- Download: <https://gaslib.zib.de/testData.html>
- Format: XML with `framework:` namespaces, GasLib XSD compliant.

### Tasks

| # | Task | Agent | File(s) | Status |
|---|------|-------|---------|--------|
| 1.1 | GasLib download script | DevOps | `scripts/fetch_gaslib.sh` | ✅ |
| 1.2 | XML parser: nodes (source, sink, innode) | Backend | `gaslib/parser.rs` | ✅ |
| 1.3 | XML parser: connections (pipe, valve, shortPipe) | Backend | `gaslib/parser.rs` | ✅ |
| 1.4 | XML parser: compressorStation | Backend | `gaslib/parser.rs` | ✅ |
| 1.5 | XML parser: scenarios (.scn) — node demands | Backend | `gaslib/scenario.rs` | ✅ |
| 1.6 | Build GasNetwork from parsed data | Backend | `graph/mod.rs` | ✅ |
| 1.7 | Parser insta snapshot tests | Backend | `gaslib/parser.rs`, `gaslib/snapshots/` | ✅ |

### Automated tests

```bash
cargo test test_parse_gaslib_11        # T1-1: loads without panic
cargo test test_gaslib_11_topology     # T1-2: 11 nodes, ~12 connections
cargo test test_gaslib_11_snapshot      # T1-3 ✅: insta::assert_yaml_snapshot!
cargo test test_all_nodes_have_gps     # T1-4 ✅: coordinates present if available + x/y/GPS validation
cargo test test_parse_scenario_scn     # T1-5: demands parsed
cargo test test_parse_gaslib_24_extended_connection_kinds  # T1-6 ✅: resistor/controlValve support on real dataset
```

---

## Phase 2: Steady-state solver (days 5–9)

### Mathematical foundations

See `docs/science/equations.md`. The detailed scientific validation protocol is defined in this phase (section "Detailed scientific validation protocol (v1)").

> **⚠️ Scaling: task 2.4 (full Newton with sparse Jacobian) is a prerequisite for Phase 3.** The diagonal Jacobi solver (2.3) converges on GasLib-11 but will diverge on larger or more coupled networks. Do not move to Phase 3 without a working Newton on GasLib-11.

### Tasks

| # | Task | Agent | File(s) | Status |
|---|------|-------|---------|--------|
| 2.1 | Darcy friction (Swamee-Jain) | Backend | `solver/steady_state.rs` | ✅ |
| 2.2 | Pipe hydraulic resistance | Backend | `solver/steady_state.rs` | ✅ |
| 2.3 | Diagonal Newton-Raphson (Jacobi) | Backend | `solver/steady_state.rs` | ✅ |
| 2.4 | **🔴 CRITICAL: Full Newton-Raphson + sparse Jacobian (faer)** | Backend | `solver/newton.rs` | ✅ |
| 2.5 | **Gas equation of state (density = f(P, T))** | Backend | `solver/gas_properties.rs`, `solver/steady_state.rs`, `solver/newton.rs` | ✅ |
| 2.6 | **Variable non-dimensionalisation** | Backend | `solver/steady_state.rs`, `solver/newton.rs` | ✅ |
| 2.7 | Analytical validation: 2-node network | Science | `docs/science/validation.md` | ✅ |
| 2.8 | Validation: Y network (mass conservation) | Science | `docs/science/validation.md` | ✅ |
| 2.9 | Run on full GasLib-11 | Backend | `main.rs` | ✅ |
| 2.10 | **Validation against GasLib-11 reference solutions (.sol)** | Science | `solver/steady_state.rs`, `docs/science/validation.md` | 🟨 partial (versioned internal reference run; official `.sol` reference still absent) |
| 2.11 | **Line search (backtracking) + Newton/Jacobi hybrid fallback** | Backend | `solver/newton.rs` | ✅ |
| 2.12 | **Document unit conversions (Pa²→bar², ρ_eff) in equations.md** | Science | `docs/science/equations.md` | ✅ |
| 2.13 | **Warm-start: initialise Newton from previous solution** | Backend | `solver/steady_state.rs` | ✅ |
| 2.14 | **Valve modelling (K≈0 open, arc removed when closed) and shortPipes** | Backend | `solver/steady_state.rs`, `solver/newton.rs`, `gaslib/parser.rs`, `graph/mod.rs` | ✅ |
| 2.15 | **Compressors: MVP directional model (compression ratio on \(P^2\) via `.cs`)** | Backend | `solver/steady_state.rs`, `solver/newton.rs`, `gaslib/compressor.rs`, `gaslib/parser.rs` | ✅ |
| 2.16 | **Run scientific validation protocol v1 (T1→T10) and publish Go/No-Go report** | Science + Backend | `docs/plans/implementation-plan.md`, `docs/science/validation.md`, `scripts/validation-pack.sh` | 🟨 partial (conditional final report published + pack execution script; strict scientific validation pending official reference) |

### Automated tests

```bash
cargo test darcy_friction_turbulent                  # T2-1 ✅
cargo test steady_state_two_nodes                    # T2-2 ✅
cargo test steady_state_y_network_mass_conservation  # T2-3 ✅
cargo test pipe_resistance_positive                  # T2-4 ✅
cargo test test_solve_gaslib_11                      # T2-5 ✅
cargo test test_newton_vs_jacobi_same_result         # T2-6 ✅
cargo bench -- steady_state                          # T2-7 ✅ (Criterion bench runs without panic)
cargo test test_gaslib_11_vs_reference_solution      # T2-8 ✅ (versioned internal ref) / 🟨 (external official ref absent)
cargo test test_newton_line_search_convergence       # T2-9 ✅ (Newton converges even with far init)
cargo test test_newton_jacobi_hybrid_fallback        # T2-10 ✅ (Jacobi fallback if line search fails)
cargo test test_warm_start_fewer_iterations          # T2-11 ✅ (warm-start converges in ≤ 5 iter vs ~20 cold)
cargo test test_valve_open_zero_resistance            # T2-12 ✅ (open valve: ΔP ≈ 0)
cargo test test_compressor_applies_pressure_lift_mvp  # T2-13 ✅ (MVP compressor uplift)
cargo test test_compressor_higher_ratio_increases_downstream_pressure # T2-13bis ✅
cargo test test_units_scn_to_si                       # T2-14 ✅ (scenario unit conversion to SI)
cargo test test_pressure_drop_dimension_consistency   # T2-15 ✅ (SI ↔ bar² dimensional consistency)
# T2-16 🟨: conditional final report published (strict scientific Go/No-Go pending official reference)
cargo test test_sensitivity_physical_trends           # T2-17 ✅ (monotonic physical trends)
cargo test test_pipe_resistance_at_pressure_increases_with_pressure  # T2-18 ✅ (ρ(P,T) affects K)
cargo test test_nondimensionalized_flow_matches_physical_formula     # T2-19 ✅ (non-dim vs physical equivalence)
cargo test test_valve_closed_removes_arc_and_blocks_flow             # T2-20 ✅ (closed valve → inactive arc)
```

### Detailed scientific validation protocol (v1)

**Objective:** qualify the scientific robustness of the steady-state solver before moving to UI/perf phases.

#### Preconditions

- `./scripts/dev.sh`
- `./scripts/back-shell.sh`
- GasLib data present in `back/dat/`

#### Tests, criteria and status

| ID | Test | Command | Acceptance criterion | Status |
|---|---|---|---|---|
| T1 | Darcy friction in turbulent | `cargo test darcy_friction_turbulent` | Test passes, friction factor in realistic physical range | ✅ |
| T2 | Positive/finite pipe resistance | `cargo test pipe_resistance_positive` | Test passes, K > 0 and finite | ✅ |
| T3 | 2-node analytical case | `cargo test steady_state_two_nodes` | Source pressure ~fixed, downstream pressure positive and < upstream | ✅ |
| T4 | Y network: local conservation | `cargo test steady_state_y_network_mass_conservation` | \|Q_SJ - Q_JA - Q_JB\| < 1e-4 | ✅ |
| T5 | Hybrid vs Jacobi | `cargo test test_newton_vs_jacobi_same_result` | Pressures close, hybrid iters ≤ Jacobi on test case | ✅ |
| T6 | GasLib-11 sanity check | `cargo test test_solve_gaslib_11` | Convergence, finite/positive pressures, consistent cardinalities | ✅ |
| T7 | Scenario units → SI | `cargo test test_units_scn_to_si` | Conversion relative error < 1e-6 | ✅ |
| T8 | Pressure drop dimensional consistency | `cargo test test_pressure_drop_dimension_consistency` | SI ↔ bar² equivalence within numerical tolerance | ✅ |
| T9 | Validation vs GasLib `.sol` reference | `cargo test test_gaslib_11_vs_reference_solution` | MVP: max pressure error < 5%; post-upgrade: < 1% | 🟨 (test ready, `.sol` dataset missing locally) |
| T10 | Physical sensitivity (roughness, Z, T) | `cargo test test_sensitivity_physical_trends` | Monotonic physically consistent trends | ✅ |

#### Recommended execution order

1. **Equation base:** T1 → T4
2. **Solver:** T5 → T6
3. **Scientific quality:** T7 → T10

#### Go/No-Go gate

- **Immediate No-Go** if any test T1–T6 fails.
- **MVP scientific Go** if T1–T8 + T9(MVP) pass (threshold < 5%).
- **Robust Go** if T1–T10 + T9(post-upgrade) pass (threshold < 1%).

#### Expected deliverable (task 2.16)

Publish a short report in `docs/science/validation.md` with:

- date and commit tested;
- Pass/Fail status for T1..T10;
- T9 metrics (max error, mean, worst node);
- explicit decision: **Go** or **No-Go** for Phase 2 exit.

---

## Phase 3: WebSocket + live interface (days 10–16)

### Communication architecture

```
Frontend                          Backend
┌──────────┐  WS /api/ws/sim   ┌─────────────┐
│ SimPanel  │◄═══════════════►│ Axum WS     │
│ LogPanel  │  { type, data }  │ handler     │
│ CesiumMap │                  └──────┬──────┘
└──────────┘                         │ mpsc channel
                               ┌─────▼──────┐
                               │ Solver      │
                               │ (spawn_blocking + rayon) │
                               └─────────────┘
```

**WebSocket protocol (JSON):**

```jsonc
// Client → Server: start simulation
{ "type": "start_simulation", "demands": { "sink_1": -10.0 } }

// Client → Server: cancel running simulation
{ "type": "cancel_simulation" }

// Server → Client: progress each iteration
{ "type": "iteration", "iter": 5, "residual": 0.0023, "elapsed_ms": 12 }

// Server → Client: intermediate results (every N iterations)
{ "type": "snapshot", "pressures": {...}, "flows": {...} }

// Server → Client: convergence reached
{ "type": "converged", "result": {...}, "total_ms": 45 }

// Server → Client: simulation cancelled (by client or timeout)
{ "type": "cancelled", "reason": "client_request" | "timeout" | "diverged" }

// Server → Client: error (fatal=true → connection closed, fatal=false → can retry)
{ "type": "error", "message": "...", "fatal": false }
```

### Tasks

| # | Task | Agent | File(s) | Status |
|---|------|-------|---------|--------|
| 3.1 | Axum WebSocket handler | Backend | `api/ws.rs` | ✅ |
| 3.2 | Solver with progress callback | Backend | `solver/steady_state.rs` | ✅ |
| 3.3 | `tokio::spawn_blocking` for solver | Backend | `api/ws.rs` | ✅ |
| 3.4 | `mpsc` channel: solver → WS → client | Backend | `api/ws.rs` | ✅ |
| 3.5 | REST endpoint `/api/network` | Backend | `api/mod.rs` | ✅ |
| 3.6 | API integration tests (reqwest + WS) | Backend | `tests/api_test.rs` | ✅ |
| 3.7 | WebSocket client (Vue composable) | Frontend | `services/ws.ts` | ✅ |
| 3.8 | LogPanel: real-time solver logs | Frontend | `components/LogPanel.vue` | ✅ |
| 3.9 | CesiumViewer: display nodes + pipes | Frontend | `CesiumViewer.vue` | ✅ |
| 3.10 | CesiumViewer: live colour updates | Frontend | `CesiumViewer.vue` | ✅ |
| 3.11 | SimulationPanel: start/stop via WebSocket | Frontend | `SimulationPanel.vue` | ✅ |
| 3.12 | Progress bar + residual indicator | Frontend | `components/ProgressBar.vue` | ✅ |
| 3.13 | **Simulation cancellation (CancellationToken + timeout)** | Backend | `api/ws.rs`, `solver/steady_state.rs` | ✅ |
| 3.14 | **WS backpressure: bounded buffer + drop intermediate snapshots** | Backend | `api/ws.rs` | ✅ |
| 3.15 | **Live smoothness: UI snapshot throttling (max ~10 Hz) + message coalescing** | Frontend | `services/ws.ts`, `stores/simulate.ts` | ✅ |
| 3.16 | **UI perf metrics: map FPS + update render time (optional debug overlay)** | Frontend | `CesiumViewer.vue` | ✅ |

### Target interface

```
┌────────────────────────────────────────────────────────┐
│ GazFlow                                 [▶ Start] [⏹] │
├──────────────────────────────────┬─────────────────────┤
│                                  │ Simulation           │
│                                  │ ████████░░ 80%       │
│       CesiumJS Globe              │ Iter: 42 / 100       │
│   (pipes coloured live,          │ Residual: 2.3e-4      │
│    nodes with pressure)          │ Time: 34ms           │
│                                  ├─────────────────────┤
│                                  │ Logs                 │
│                                  │ [42] res=2.3e-4      │
│                                  │ [41] res=5.1e-4      │
│                                  │ [40] res=1.2e-3      │
│                                  │ ...                  │
│                                  ├─────────────────────┤
│                                  │ Pressures (bar)      │
│                                  │ S: 70.00  J: 68.45   │
│                                  │ A: 65.12  B: 66.30   │
│                                  ├─────────────────────┤
│                                  │ Flows (m³/s)         │
│                                  │ SJ: 10.0  JA: 5.2    │
│                                  │ JB: 4.8              │
└──────────────────────────────────┴─────────────────────┘
```

### Automated tests

```bash
cargo test test_ws_start_simulation    # T3-1 ✅: WS connects and receives iterations
cargo test test_ws_start_simulation    # T3-2 ✅: "converged" message received
cargo test test_api_network_count      # T3-3 ✅: REST network OK
# (Integration tests also in back/tests/api_test.rs)
cd front && npx vitest run             # T3-4 ✅: ws + stores (network/simulate) covered
cd front && npx quasar build           # T3-5 ✅: build without error
cargo test test_ws_cancel_simulation   # T3-6 ✅: cancel mid-solve, receive "cancelled"
cargo test test_ws_timeout_diverged    # T3-7 ✅: diverging solver → auto timeout
cd front && npx vitest run src/config/dev-integration.spec.ts  # T3-8 ✅: dev config safeguards (Pinia boot + WS proxy /api)
```

---

## Phase 4: Multi-threading + performance + scaling (days 17–22)

### Multi-thread architecture

```
┌───────────────────────────────────────────────────────────┐
│                    tokio runtime                           │
│  ┌─────────────┐  ┌─────────────┐  ┌───────────┐         │
│  │ Axum HTTP   │  │ Axum WS     │  │ Axum WS   │         │
│  │ /api/network│  │ client #1   │  │ client #2 │         │
│  └─────────────┘  └──────┬──────┘  └─────┬─────┘         │
│                          │                │               │
│            ┌─────────────▼────────────────▼─────┐         │
│            │       spawn_blocking pool           │         │
│            │    (bounded by Semaphore, max N)    │         │
│            │  ┌──────────────────────────────┐  │         │
│            │  │     Solver (1 per simulation) │  │         │
│            │  │  ┌────────┐ ┌────────┐       │  │         │
│            │  │  │ Rayon  │ │ Rayon  │ ...   │  │         │
│            │  │  │ thread │ │ thread │       │  │         │
│            │  │  └────────┘ └────────┘       │  │         │
│            │  └──────────────────────────────┘  │         │
│            └────────────────────────────────────┘         │
└───────────────────────────────────────────────────────────┘
```

### Scaling strategy by network size

| Network size | Recommended solver | Parallelism |
|---|---|---|
| ≤ 50 nodes | Newton + direct sparse LU (faer) | Sequential (Rayon overhead > gain) |
| 50–2000 nodes | Newton + direct sparse LU (faer) | Rayon `par_iter` on residual/Jacobian assembly |
| 2000–5000 nodes | Newton + ILU-preconditioned GMRES (stretch) | Rayon + iterative solver |
| > 5000 nodes | Beyond MVP scope — requires domain decomposition | — |

> **Note:** Sparse LU on a gas network Jacobian (~3 non-zeros per row) has effective complexity O(N^{1.2} to N^{1.5}) thanks to AMD ordering, well below dense LU O(N³). The GMRES threshold only applies if profiling (task 4.9) shows LU factorisation as the bottleneck.

### Tasks

| # | Task | Agent | File(s) | Status |
|---|------|-------|---------|--------|
| 4.1 | Verify `spawn_blocking` (3.3) + Rayon do not cause contention | Backend | `api/mod.rs`, `api/ws.rs` | ✅ |
| 4.2 | Rayon `par_iter` on pipes (residual + Jacobian), threshold ≥ 50 pipes | Backend | `solver/newton.rs` | ✅ |
| 4.3 | Parallel sparse Jacobian assembly (faer) | Backend | `solver/newton.rs` | ✅ |
| 4.4 | Criterion benchmark: Jacobi vs Newton, 1 thread vs N | Backend | `benches/solver_bench.rs` | ✅ |
| 4.5 | Concurrent simulations (multiple WS clients) | Backend | `api/ws.rs`, `tests/api_test.rs` | ✅ |
| 4.6 | GasLib-24 + GasLib-40 support | Backend | `gaslib/parser.rs` | ✅ |
| 4.7 | Benchmark on GasLib-135 (stress test) | Backend | `benches/solver_bench.rs` | ✅ |
| 4.8 | **Semaphore: limit concurrent simulations (configurable max N)** | Backend | `api/ws.rs` | ✅ |
| 4.9 | **Integrated flamegraph profiling (tracing + inferno or perf)** | Backend | `benches/`, `scripts/profile.sh` | ✅ |
| 4.10 | **GasLib-582 + GasLib-4197 support (scaling targets)** | Backend | `gaslib/parser.rs`, `scripts/fetch_gaslib.sh`, `solver/steady_state.rs`, `solver/newton.rs` | 🟨 partial (download + naming + parse OK; MVP directional compressor model integrated; continuation warm-start/snapshots + auto bridges + timeout budget; short physical init + GMRES cap for large cases; smoke perf target 4197 <5e5 in <15s met on dev machine, full physical convergence of very large cases not guaranteed) |
| 4.11 | **🔵 STRETCH: GMRES iterative solver + ILU preconditioner (if sparse LU insufficient beyond ~2000 nodes)** | Backend | `solver/iterative.rs` | ✅ |
| 4.12 | **Scaling benchmark: time vs N nodes (11, 24, 40, 135, 582, 4197)** | Backend | `benches/scaling_bench.rs` | ✅ |

### Automated tests

```bash
cargo test test_parallel_solver_same_result    # T4-1 ✅: same result 1 vs N threads
cargo test test_concurrent_simulations         # T4-2 ✅: 2 WS clients simultaneous
cargo bench -- steady_state                    # T4-3 ✅: Jacobi vs Newton + 1 thread vs N
cargo test test_solve_gaslib_24                # T4-4 🟨: smoke test in place (skip if dataset absent)
cargo test test_solve_gaslib_40                # T4-5 🟨: smoke test in place (skip if dataset absent)
cargo test test_semaphore_rejects_overflow     # T4-6 ✅: (N+1)th simulation gets explicit reject
cargo test test_solve_gaslib_582               # T4-7 🟨: robust large smoke (converge or explicit non-convergence), env-guarded
cargo test test_solve_gaslib_4197              # T4-8 🟨: robust large smoke (converge or explicit non-convergence), env-guarded
# continuation knobs: GAZFLOW_CONTINUATION_AUTO_BRIDGES, GAZFLOW_CONTINUATION_MIN_GAP, GAZFLOW_CONTINUATION_MAX_SECONDS, GAZFLOW_CONTINUATION_SNAPSHOT_EVERY, GAZFLOW_CONTINUATION_ITER_SCHEDULE
# large smoke knobs: GAZFLOW_LARGE_TEST_MAX_SECONDS
# large solver knobs: GAZFLOW_PHYSICAL_INIT_ITERS, GAZFLOW_GMRES_MAX_ITERS, GAZFLOW_GMRES_RESTART, GAZFLOW_GUARD_JACOBI_FALLBACK
# default 4197 profile: max_iter=6, scales=0.05,0.1,0.1, schedule iters=1,1,4, init_phys=2 (>2000 nodes), gmres_cap=220, jacobi_guard=on (>2000 nodes) (~15s, residual ~2.52e5 on recent dev machine)
cargo bench -- scaling                         # T4-9 ✅: time vs N nodes curve (synthetic bench)
cargo test test_ws_concurrent_with_single_rayon_thread_no_deadlock  # T4-10 ✅: no deadlock with rayon=1
./scripts/profile.sh                           # T4-11 ✅: flamegraph generation (tools available)
cargo test test_sparse_linear_solver_matches_dense  # T4-12 ✅: faer sparse solver consistent with dense
cargo test test_gmres_ilu0_solves_small_system # T4-13 ✅: GMRES+ILU0 fallback works on reference system
```

---

## Phase 5: Full integration + polish (days 23–28)

Export contract reference: `docs/architecture/export-contract.md` (API/format source of truth).

### Tasks

| # | Task | Agent | File(s) | Status |
|---|------|-------|---------|--------|
| 5.1 | Demand sliders at sink nodes | Frontend | `components/DemandControls.vue`, `components/SimulationPanel.vue` | ✅ |
| 5.2 | POST `/api/simulate` with custom demands (REST fallback) | Backend | `api/mod.rs` | ✅ |
| 5.3 | Colour legend (pressure / flow gradient) | Frontend | `components/Legend.vue`, `pages/MapPage.vue` | ✅ |
| 5.4 | Node selection → popup with pressure, neighbors | Frontend | `components/CesiumViewer.vue` | ✅ |
| 5.5 | Dark SCADA theme (industrial palette) | Frontend | `css/app.scss` | ✅ |
| 5.6 | Full CI script via Docker | DevOps | `scripts/ci.sh` | ✅ |
| 5.7 | Final architecture documentation | Science | `docs/architecture/overview.md`, `docs/architecture/export-contract.md` | ✅ |
| 5.8 | **CesiumJS LOD: node clustering at low zoom (> 200 entities)** | Frontend | `components/CesiumViewer.vue` | ✅ |
| 5.9 | **WebGL primitives for large networks (PolylineCollection instead of entities)** | Frontend | `components/CesiumViewer.vue` | ✅ |
| 5.10 | **Warm-start via slider: reuse previous solution when demand changes** | Frontend + Backend | `components/DemandControls.vue`, `stores/simulate.ts`, `api/ws.rs` | ✅ |
| 5.11 | **Backend export: result export endpoint (`json`, `csv`) with metadata (v1 contract compliant)** | Backend | `api/mod.rs`, `api/export.rs` | ✅ |
| 5.12 | **Frontend export: "Export JSON/CSV" buttons in simulation panel (non-blocking `exporting` state)** | Frontend | `components/SimulationPanel.vue`, `stores/simulate.ts`, `services/api.ts` | ✅ |
| 5.13 | **Full export: optional `.zip` bundle (results + logs + simulation context, v1 contract)** | Frontend + Backend | `components/SimulationPanel.vue`, `stores/simulate.ts`, `api/export.rs` | ✅ |
| 5.14 | **UI smoothness under load: list virtualisation + slider debounce + frame-time budget** | Frontend | `components/LogPanel.vue`, `components/DemandControls.vue`, `components/CesiumViewer.vue` | ✅ |

### Automated tests

```bash
cargo test test_export_result_json_schema      # T5-1 ✅: JSON export contains data + metadata + units
cargo test test_export_result_csv_headers     # T5-2 ✅: CSV export stable parseable columns
cd front && npx vitest run                     # T5-3: export button visible/active per simulation state
cd front && npx playwright test                # T5-4: E2E export scenario + map navigation smoothness
```

---

## Milestone summary

| Milestone | Deliverable | Verification |
|-----------|-------------|--------------|
| M0 | Compilable monorepo, Docker | `cargo check` + `quasar build` | ✅ |
| M1 | GasLib-11 parsed to graph | 11 nodes, insta snapshot | ✅ |
| M2 | Steady-state simulation + full Newton + reference validation | Tests T2-1..T2-13 + scientific protocol v1 (Go/No-Go), error < 5% vs .sol | ✅ partial |
| M3 | **Live WebSocket + logs + real-time map + cancellation** | Simulation visible live, cancel works |
| M4 | **Multi-threading + scaling verified** | GasLib-135 < 100ms, GasLib-582 converges, scaling curve documented |
| M4+ | **Iterative solver (stretch goal)** | GasLib-4197 converges with GMRES+ILU |
| M5 | Full MVP + LOD + result export | Green CI, interactive demands, JSON/CSV export, 4000 entities without lag |

---

## Risks and mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Complex GasLib XML parsing (namespaces) | Blocks P1 | ✅ Resolved: `alias` serde |
| Solver does not converge | Blocks P2 | ✅ Jacobi converges (8 tests) |
| Full Newton unstable (Jacobian singularity) | P2/P4 | Jacobi fallback, regularisation + **line search backtracking + hybrid** (task 2.11) |
| Gap vs GasLib reference solutions > 5% | P2 | Upgrade ρ(P,T) and Z (task 2.5), then target < 1% |
| CesiumJS heavy (bundle > 50 MB) | Frontend slowness | Static copy, lazy loading |
| WebSocket disconnect during simulation | P3 | Auto-reconnect + result cache |
| **Frontend config regression (Pinia boot / WS proxy dev)** | P3 | Non-regression test `src/config/dev-integration.spec.ts` + review `quasar.config.ts` |
| Rayon in spawn_blocking: contention | P4 | Systematic benchmark, pool sizing |
| GasLib-135+: slow solver | P4 | faer sparse matrices, profiling |
| **Divergent simulation blocks slot indefinitely** | P3/P4 | Configurable timeout + CancellationToken (task 3.13) |
| **Concurrent simulations saturate memory / CPU** | P4 | Bounded semaphore (task 4.8), graceful reject when full |
| **Sparse LU too slow for N > ~2000 (fill-in, memory)** | P4 | Profiling (4.9) then GMRES+ILU fallback if needed (task 4.11) |
| **CesiumJS lag with > 1000 individual entities** | P5 | LOD + clustering (5.8) + PolylineCollection (5.9) |
| **No warm-start: each slider re-solves from scratch** | P5 | Warm-start (2.13) + WS protocol with initial solution (5.10) |
| **Incomplete/inconsistent exports (units, missing metadata)** | P5 | Versioned export contract + tests T5-1/T5-2 + documented examples |
| **Non-smooth UX under real conditions (snapshot backlog, UI jank)** | P3/P5 | Throttling/coalescing (3.15), LOD/primitives (5.8/5.9), frame-time budget (5.14) |
