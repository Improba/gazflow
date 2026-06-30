# Testing — How to run tests

This document complements `README.md` with a focus on test execution. Environment setup (Docker, service startup, scripts) remains in `README.md`.

## Operational test corpus (P6–P13)

Fixtures for the post-MVP roadmap live in `docs/testing/corpus/`:

```bash
./scripts/fetch_test_corpus.sh   # GasLib-39, TRR154 transient, SciGRID FR snippet
./scripts/verify_test_corpus.sh
```

Synthetic fixtures (GeoJSON, CSV, mapping YAML, SCADA) are versioned under `corpus/synthetic/`. Downloaded assets go to `corpus/external/` (gitignored). See `corpus/README.md` and `corpus/manifest.yaml`.

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
./scripts/ci.sh            # Full build + tests (+ verify_test_corpus.sh)
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

Current baseline (2026-06-30): **~270** Rust lib tests, **64** frontend tests (`vitest`).

Current frontend coverage includes:
- `src/services/ws.spec.ts`, `apiContracts.spec.ts`, `gas-presets.spec.ts`
- `src/stores/network.spec.ts`, `simulate.spec.ts`, `scenarios.spec.ts`, `editor.spec.ts`, `demandProfiles.spec.ts`
- `src/utils/*` (demand profiles, weather CSV, import errors, equipment labels)
- `src/config/dev-integration.spec.ts` (Pinia boot + WS proxy)

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

GasLib-582 smoke test (`test_solve_gaslib_582`) runs in **robust mode** when `GAZFLOW_ENABLE_LARGE_DATASET_TESTS=1`: the solver must not panic; convergence to tolerance is logged but not required. Set `GAZFLOW_REQUIRE_FULL_CONVERGENCE=1` to enforce residual < tolerance and demand scale ≥ 0.999 (expected to fail on the MVP compressor model until `.cs` maps). Automatic `.cdf` routing is skipped when it would fragment the graph or not beat the default open topology.

Transport `.cdf` routing (optional env):

- `GAZFLOW_SKIP_CDF_ROUTING` / `GAZFLOW_SKIP_CDF`: disable automatic combined-decision selection.
- `GAZFLOW_FORCE_CDF_ROUTING=1`: run CDF screening on large connected networks (default: skip when baseline has no floating components and N > 500).
- `GAZFLOW_CDF_MAX_COMBINATIONS` (default 512 for N > 500): cap for exhaustive routing search.
- `GAZFLOW_CDF_SCREEN_MAX_ITER`, `GAZFLOW_CDF_SCREEN_TOL`, `GAZFLOW_CDF_SCREEN_SCALE`, `GAZFLOW_CDF_SCREEN_TIMEOUT_MS`: fast screening preset.
- `GAZFLOW_CDF_SCREEN_SCALES` (default `0.15,0.4` for N > 500): multi-scale routing screening.
- `GAZFLOW_CDF_FULL_SOLVE_CANDIDATES` (default 5): number of top routing candidates validated with the robust preset.
- `GAZFLOW_SKIP_COMPRESSOR_OUTER` / `GAZFLOW_COMPRESSOR_OUTER`: control post-continuation compressor blend fallback.
- `GAZFLOW_REQUIRE_FULL_CONVERGENCE=1`: strict large-dataset smoke (residual < tolerance, scale ≥ 0.999); default is robust mode (log only).

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

Defaults (Large tier, e.g. GasLib-582 with `preset_robust`):
- `max_iter=400`, `tol=3e-3`, `scales=0.05,0.1,0.2,0.4,0.7,1.0`, continuation timeout `180s`, auto bridges `6`;
- intermediate continuation tiers use relaxed tolerance (0.3 for 582);
- GasLib-4197: very short smoke profile `max_iter=12`, `tol=1e-2`, `scales=0.05,0.1,0.2,0.4,0.7,1.0`, global timeout `240s` (explicit non-convergence accepted in smoke mode).

## GasLib-582 compressor diagnostic (I-A0)

Manual diagnostic for transport compressor behaviour on GasLib-582. This is **not** run in CI (full `preset_robust` solve takes ~6 min on a dev machine).

### Protocol (frozen)

| Step | Setting |
|------|---------|
| Network | `back/dat/GasLib-582.net` (symlink from `fetch_gaslib.sh`) |
| Scenario | `nomination_mild_618.scn` if present under `back/dat/` (nominations archive), else `GasLib-582.scn` |
| Demands | `demands_without_pressure_slack` (pressure slack node flow removed, e.g. `sink_109`) |
| CDF routing | **off** — baseline connected topology (`GAZFLOW_SKIP_CDF_ROUTING=1`, set by the binary) |
| Solver | `solve_steady_state_with_preset` + `preset_robust` |

Download data first:

```bash
./scripts/fetch_gaslib.sh GasLib-582
```

Run diagnostic:

```bash
cd back
cargo run --bin compressor_diag -- GasLib-582
```

Options:

```bash
cargo run --bin compressor_diag -- GasLib-582 --no-r2-cap
cargo run --bin compressor_diag -- GasLib-582 --map-mode measurement --json /tmp/582-map.json
cargo run --bin compressor_diag -- GasLib-582 --json /tmp/582-diag.json --csv /tmp/582-stations.csv
```

If `dat/GasLib-582.net` or a scenario file is missing, the binary exits gracefully with `status: "skipped"` JSON (no solve).

Output JSON fields: `residual`, `demand_scale`, `compressor_stations` (per-station `flow_m3s`, `ratio_max`, `effective_r2`), and `flags` used.

Bench results (I-A0, juin 2026) : [gaslib-582-compressor-bench.md](./gaslib-582-compressor-bench.md).

### GAZFLOW_* flags (compressor / large transport)

| Variable | Role | Default |
|----------|------|---------|
| `GAZFLOW_DISABLE_R2_CAP` | Disable MVP $r^2 \leq 9$ attenuation for `ratio > 3` (H2 diagnostic; `--no-r2-cap` on `compressor_diag`) | off |
| `GAZFLOW_SKIP_COMPRESSOR_OUTER` / `GAZFLOW_COMPRESSOR_OUTER` | Post-continuation compressor blend fallback | outer on for networks $\geq$ 200 nodes |
| `GAZFLOW_COMPRESSOR_MAP_MODE` | `legacy` (blend) \| `measurement` (carte `.cs` + outer loop) \| `biquadratic` (alias measurement, coeffs GasLib à venir) | `legacy` |
| `GAZFLOW_COMPRESSOR_OUTER_MAX_ITERS` | Plafond boucle externe ratio | 12 |
| `GAZFLOW_COMPRESSOR_RELAX` | Relaxation $\omega$ pour mises à jour ratio | 0.5 |
| `GAZFLOW_DISABLE_R2_CAP` | Désactive le plafond MVP $r^2 \leq 9$ ; aussi désactivé automatiquement en mode `measurement` | off |
| `GAZFLOW_SKIP_CDF_ROUTING` / `GAZFLOW_SKIP_CDF` | Disable automatic `.cdf` routing | off (forced on by `compressor_diag`) |
| `GAZFLOW_FORCE_CDF_ROUTING` | Run CDF screening on large connected baselines | off when baseline connected and N > 500 |
| `GAZFLOW_ENABLE_LARGE_DATASET_TESTS` | Enable `test_solve_gaslib_582` / 4197 in `cargo test` | off in CI |
| `GAZFLOW_REQUIRE_FULL_CONVERGENCE` | Strict large-dataset smoke (residual < tol, scale $\geq$ 0.999) | off (robust log-only) |

See also the transport `.cdf` routing variables in the [GasLib datasets](#gaslib-datasets-smokescaling) section above.

## Validation pack (backend)

Single script to run T1→T10 in sequence:

```bash
./scripts/validation-pack.sh
```

Useful options:
- `GAZFLOW_REGEN_REFERENCE=1`: regenerate `GasLib-11.reference.internal.csv` before T9.
- `GAZFLOW_RUN_LARGE_SMOKE=1`: add large-dataset smoke tests.
