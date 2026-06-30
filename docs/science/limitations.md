# Model limitations — GazFlow

This document describes the known limits of the solver in its current state. It complements `docs/science/equations.md` (model) and `docs/science/validation.md` (tests).

## 1. Physical limits

- **Steady state** is the default operational mode; **transient** supports two modes:
  - `quasi_steady`: re-solves steady-state at each step (MVP, no wave propagation between steps).
  - `pde`: 1D isothermal implicit Euler on **series pipes**; **branched networks fall back** to quasi-steady.
- Isothermal assumption (uniform gas temperature 288.15 K in pipes; outdoor $T_{\mathrm{ext}}$ affects demand only).
- **EOS**: Kay pseudo-criticals + Papay Z by default; **PR-78 auto-selected** when H₂ > 20 % (`solver/eos/pr78.rs`). GERG-2008 not implemented.
- Lee-Gonzalez-Eakin viscosity; G20 or custom composition via `PATCH /api/network/gas-composition`.
- Gravity included (`ρ g Δz` in the P² equation); altitude from import/GasLib.
- Reynolds dynamic in `pipe_resistance_hydraulic` when $|Q|>0$; Newton Jacobian uses $Re=10^7$ for stability (optional Re–Q coupling not enabled).
- Compressors: simplified pressure-lift MVP (ratio P² coefficient, not full enthalpic balance). Optional **`.cs` performance maps** (measurement / biquadratic modes): outer loop updates `compressor_ratio_max` from head/speed search; **in-Newton recoupling** (`GAZFLOW_NEWTON_COMPRESSOR_MAP`, default on in map modes) evaluates map ratio each Newton iteration (semi-implicit Jacobian). Operating ratio from catalogue (~1,08/stage); pressure cap from `.net` (e.g. 4,09 transport) stored separately (`compressor_pressure_cap_ratio`). Legacy blend fallback remains for networks without `.cs`.
- Flow variable: normal volumetric flow (Nm³/s) at **15 °C / 1,01325 bar**; PCS/PCI/Wobbe at **ISO 6976 0 °C** basis — see `equations.md` §2.4.
- **P8 regulators**: outer loop + downstream slack; bypass with hysteresis; isothermal expansion (no Joule–Thomson). Control valves: **effective diameter** from $C_v$ and opening (not full ISA gas choking). Regulator Jacobian: finite-difference row coupling (not fully analytic).
- **P9 demand**: quasi-steady hourly timeseries (no linepack coupling between hourly steps); scalar $T_{\mathrm{ext}}$ per step; weather CSV with unique hours; weekday/weekend profiles; $\bar m_h = 1$ when all $w_h \ge 0$.
- **P13 calibration**: residuals $r_i = y_i - \hat y_i$; LM on global roughness and up to **5 parameters** (roughness + `DemandScale`); per-pipe strategy uses grid search for many pipes.
- **P11 linepack**: $M = \sum \rho(P_{\mathrm{moy}})\, A\, L$ on active pipes (aggregated, not spatially resolved except PDE MVP on single pipe / chain).

## 2. Numerical limits

- Convergence depends on initialisation, line search, and optional Jacobi fallback.
- Very large networks may require continuation strategies and warm-start.
- **Partial continuation**: when charge ramping stops before 100 % demand, the solver may return a converged state at a lower scale (`demand_scale_achieved` < 1); results are valid only for that fraction of nominal demand.
- **Continuation tiers**: intermediate demand scales use a relaxed residual tolerance (max(0.05, 100× final tolerance)); only the final scale at 100 % demand uses the preset tolerance. Compressor pressure uplift is ramped with the current continuation scale (`network_with_scaled_compressor_lift`).
- **Floating connected components**: if the hydraulically active subgraph splits into several components without a fixed-pressure node, the Newton solver anchors **one numerical pressure reference per component** (largest |demand| node at the current iterate, else lowest index, else 70 bar). This is a **Jacobian regularisation**, not a GasLib boundary condition; `pressureMin`/`pressureMax` from `.net` are operational bounds, not used as anchors. Distribution networks with a single connected active graph are unaffected.
- PDE transient: fixed time step; no adaptive CFL yet; junction coupling simplified.

## 2.1 Transport GasLib (`.cdf`, `.scn`)

- **Default topology**: valves and control valves are **open** after parsing; combined decisions (`.cdf`) and scenario boundaries close or activate equipment explicitly.
- **`.cdf` routing**: when a `.cdf` file exists next to the loaded `.net` (including versioned symlinks such as `GasLib-582.net` → `GasLib-582-v2-….net`), the solver selects combined routing decisions before the steady-state solve (screening + optional full validation of top candidates). A routing is applied **only if it improves the default open topology** (connectivity first, then screening score); otherwise the baseline is kept. Connectivity of the active subgraph to all demand and fixed-pressure nodes is required.
- **Transport scenarios**: a single pressure slack node is detected heuristically (e.g. `sink_109` on GasLib-582); its imposed flow is removed before solve so pressure and flow are not over-constrained.
- **Scenario pressure anchors (transport, GasLib-582+)**: optional enrichment via `enrich_scenario_with_balance_hub` — balance hubs on high-degree Q≈0 boundaries, junction/spine anchors on under-determined innodes, and iterative mass-balance refinement (`GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES`). Reduces mass imbalance on junctions but **cannot fully fix contract entries/exits with imposed Q**; v18 iteratively relaxes flow on worst boundaries (`GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT`, marginal gain 2.045→2.0 m³/s mild_618). Over-anchoring degrades residuals.
- Disable automatic routing with `GAZFLOW_SKIP_CDF_ROUTING=1` (or `GAZFLOW_SKIP_CDF=1`).
- **Compressor map modes** (`GAZFLOW_COMPRESSOR_MAP_MODE`): `legacy` (progressive ratio blend), `measurement` (`.cs` map + outer loop), `biquadratic` (GasLib `n_isoline` coeffs). See [GasLib-582 bench](../testing/gaslib-582-compressor-bench.md).
- **Compressor outer loop (fallback)**: after continuation failure on transport networks (≥200 nodes with high-ratio compressors), a progressive blend schedule ramps `compressor_ratio_max` toward nominal (`GAZFLOW_SKIP_COMPRESSOR_OUTER=1` to disable; `GAZFLOW_COMPRESSOR_OUTER=1` to force on smaller networks). With map modes, outer loop applies `find_operating_point_for_mode` and guarded ratio steps until residual settles or partial accept.
- **CDF screening**: multi-scale evaluation via `GAZFLOW_CDF_SCREEN_SCALES` (default `0.15,0.4` for N>500); routings that fragment the active graph (multiple components without fixed pressure) are rejected on large networks. On large connected baselines (no floating components, N>500), CDF screening is **skipped by default**; set `GAZFLOW_FORCE_CDF_ROUTING=1` to run it anyway.

## 3. Data and validation limits

- GasLib-11 pressure validation: max relative error < 5 % (`test_gaslib_11_vs_reference_solution`).
- GasLib-135 (135 nodes): recommended transport demo with continuation preset; steady-state smoke test passes (faer LU stable with component anchoring on fragmented subgraphs).
- GasLib-582 (582 nodes): structural solver issues (singular Jacobian, faer LU panics) resolved via numerical component anchoring and open-by-default topology. **Full steady-state convergence to preset tolerance (3×10⁻³) not yet reached** on transport scenario `nomination_mild_618.scn` (June 2026): observed residual **~2.0 m³/s** in measurement mode with partial accept (v18 contract flow relaxation: 2.045→2.0; down from ~5 m³/s before scenario pressure anchors). Dominant remaining gap: partial accept floor and MVP P² compressor model vs enthalpic operation. Not recommended as demo dataset until residual < 0.01 m³/s or enthalpic compressor in Newton. Smoke test runs in **robust mode** by default (no panic, logs residual); set `GAZFLOW_REQUIRE_FULL_CONVERGENCE=1` for strict check. Manual bench: `compressor_diag GasLib-582` — see [bench](../testing/gaslib-582-compressor-bench.md).
- Flow comparison against external `.sol` references: not yet systematic.
- PDE transient: monotonicity tests on single pipe; full wave validation pending.

## 4. Impact on usage

- Suited for **comparative** pipeline and distribution studies on imported or GasLib networks.
- Not a certified real-time operations simulator.
- Critical decisions (safety, contracts, live control) require additional verification and field calibration.

## 5. Recommended evolutions

- Full Saint-Venant PDE on branched networks + transient WebSocket streaming.
- Cv ISA gas choking in Newton; analytic regulator Jacobian.
- GERG-2008 for high-H₂ blends (beyond PR-78).
- Thermal profiles in pipes (soil coupling).
- Outer-loop Re–Q in Newton Jacobian for sub-1 % accuracy.
- Systematic external reference validation (pressure and flow).
- Export edited network as GeoJSON/CSV from UI.
- GasLib-582 full convergence: enthalpic compressor in Newton or strict convergence without partial accept (contract Q relaxation v18: marginal; floor ~2 m³/s on mild_618, June 2026).
