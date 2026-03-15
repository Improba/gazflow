# Testing — How to run tests

This document complements `README.md` with a focus on test execution. Environment setup (Docker, service startup, scripts) remains in `README.md`.

## Scientific validation

The detailed scientific validation protocol is maintained in `docs/plans/implementation-plan.md` (Phase 2).

For the GasLib-11 reference comparison test (`test_gaslib_11_vs_reference_solution`), a versioned internal reference is provided in `docs/testing/references/GasLib-11.reference.internal.csv`.

Regenerating the internal reference (after model/solver change):

```bash
cd back
cargo run --bin generate_gaslib11_reference
```

You can also provide an external reference with:

```bash
GAZFLOW_REFERENCE_SOLUTION_PATH=/path/to/reference.sol cargo test test_gaslib_11_vs_reference_solution
```

## Recommended commands

From the project root:

```bash
./scripts/back-test.sh     # Rust backend tests
./scripts/front-test.sh    # Frontend tests
./scripts/ci.sh            # Full build + tests
./scripts/validation-pack.sh # Backend scientific protocol T1→T10
```

## Backend tests

Full run:

```bash
./scripts/back-shell.sh
cargo test
```

Targeted test:

```bash
cargo test steady_state_two_nodes
```

## Frontend tests

Full run:

```bash
./scripts/front-shell.sh
npm test
```

Common alternative:

```bash
npx vitest run
```

Current minimal frontend coverage:
- `src/services/ws.spec.ts` (WS URL mapping)
- `src/stores/network.spec.ts` (network load + error handling)
- `src/stores/simulate.spec.ts` (warm-start + export)
- `src/config/dev-integration.spec.ts` (dev config safeguards: Pinia boot + WS proxy `/api`)

Interface/websocket non-regression:
- this test guards against a blank screen (Pinia store used without booting `pinia`);
- it also guards against `websocket failed to open` in dev (proxy `/api` without `ws: true`).

## Good practices

- Run at least the targeted tests for the modified scope.
- Run `./scripts/ci.sh` before merge/release.
- Keep `cargo`/`npm` commands inside containers.

## Backend profiling (flamegraph)

Dedicated script:

```bash
./scripts/profile.sh
```

Optional benchmark filter:

```bash
./scripts/profile.sh steady_state_newton_parallel_n_threads
```

The script prefers `cargo flamegraph`, otherwise falls back to `perf + inferno-flamegraph`. Outputs are written to `back/target/profile/`.

## GasLib datasets (smoke/scaling)

Download a dataset:

```bash
./scripts/fetch_gaslib.sh GasLib-24
./scripts/fetch_gaslib.sh GasLib-582
```

Notes:
- the script supports GasLib-11, GasLib-24, GasLib-40, GasLib-135, GasLib-582, GasLib-4197;
- for 582/4197 it also fetches nomination archives (`.scn`) and creates stable aliases in `back/dat/`.

Large network smoke tests (optional):

```bash
GAZFLOW_ENABLE_LARGE_DATASET_TESTS=1 cargo test test_solve_gaslib_582
GAZFLOW_ENABLE_LARGE_DATASET_TESTS=1 cargo test test_solve_gaslib_4197
```

Advanced (optional) parameters for large smoke tuning:
- `GAZFLOW_LARGE_TEST_MAX_ITER` (e.g. `300`)
- `GAZFLOW_LARGE_TEST_TOL` (e.g. `1e-2`)
- `GAZFLOW_LARGE_TEST_SCALES` (CSV list, e.g. `0.3,0.1,0.05`)
- `GAZFLOW_LARGE_TEST_MAX_SECONDS` (global large smoke timeout, e.g. `60`)
- `GAZFLOW_CONTINUATION_AUTO_BRIDGES` (auto-insert intermediate tiers, e.g. `1`)
- `GAZFLOW_CONTINUATION_MIN_GAP` (min gap for auto-bridge, e.g. `0.02`)
- `GAZFLOW_CONTINUATION_MAX_SECONDS` (global continuation timeout, e.g. `120`)
- `GAZFLOW_CONTINUATION_SNAPSHOT_EVERY` (snapshot/warm-start frequency in continuation, e.g. `3`)
- `GAZFLOW_CONTINUATION_ITER_SCHEDULE` (iteration budget per tier, CSV, e.g. `1,1,4`)
- `GAZFLOW_DISABLE_JACOBI_FALLBACK` (debug: disable Jacobi fallback in Newton, e.g. `1`)
- `GAZFLOW_GMRES_MAX_ITERS` / `GAZFLOW_GMRES_RESTART` (GMRES iterative solver tuning)
- `GAZFLOW_PHYSICAL_INIT_ITERS` (number of physical init sweeps before Newton; `0` to disable)
- `GAZFLOW_GUARD_JACOBI_FALLBACK` (accept Jacobi fallback only if it reduces residual; default on for >2000 nodes)

Defaults:
- GasLib-582: `max_iter=180`, `tol=2e-3`, `scales=0.1,0.3`, global timeout `120s`;
- GasLib-4197: very short smoke profile `max_iter=6`, `tol=1e-2`, `scales=0.05,0.1,0.1`, global timeout `40s` (iters split by default `1,1,4` between tiers, short physical init default `2` sweeps for >2000 nodes, default GMRES cap `220` iters on free systems >1200 unknowns, guarded Jacobi fallback default on >2000 nodes, explicit non-convergence accepted in smoke mode).

## Validation pack (backend)

Single script to run T1→T10 in sequence:

```bash
./scripts/validation-pack.sh
```

Useful options:
- `GAZFLOW_REGEN_REFERENCE=1`: regenerate `GasLib-11.reference.internal.csv` before T9.
- `GAZFLOW_RUN_LARGE_SMOKE=1`: add large-dataset smoke tests.
