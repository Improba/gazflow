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
- Compressors: simplified pressure-lift MVP (not enthalpic).
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
- **Floating connected components**: if the hydraulically active subgraph splits into several components without a fixed-pressure node, the Newton solver anchors one pressure reference per component (upper bound, lower bound, largest |demand|, or lowest index) to avoid singular Laplacian blocks and faer LU failures. Distribution networks with a single connected active graph are unaffected.
- PDE transient: fixed time step; no adaptive CFL yet; junction coupling simplified.

## 2.1 Transport GasLib (`.cdf`, `.scn`)

- **Default topology**: valves and control valves are **open** after parsing; combined decisions (`.cdf`) and scenario boundaries close or activate equipment explicitly.
- **`.cdf` routing**: when a `.cdf` file exists next to the loaded `.net` (including versioned symlinks such as `GasLib-582.net` → `GasLib-582-v2-….net`), the solver selects combined routing decisions before the steady-state solve (screening + optional full validation of top candidates). Connectivity of the active subgraph to all demand and fixed-pressure nodes is required.
- **Transport scenarios**: a single pressure slack node is detected heuristically (e.g. `sink_109` on GasLib-582); its imposed flow is removed before solve so pressure and flow are not over-constrained.
- Disable automatic routing with `GAZFLOW_SKIP_CDF_ROUTING=1` (or `GAZFLOW_SKIP_CDF=1`).
- **Compressor outer loop (fallback)**: after continuation failure on transport networks (≥200 nodes with high-ratio compressors), a progressive blend schedule ramps `compressor_ratio_max` toward nominal (`GAZFLOW_SKIP_COMPRESSOR_OUTER=1` to disable; `GAZFLOW_COMPRESSOR_OUTER=1` to force on smaller networks).
- **CDF screening**: multi-scale evaluation via `GAZFLOW_CDF_SCREEN_SCALES` (default `0.15,0.4` for N>500); routings that fragment the active graph (multiple components without fixed pressure) are rejected on large networks.

## 3. Data and validation limits

- GasLib-11 pressure validation: max relative error < 5 % (`test_gaslib_11_vs_reference_solution`).
- GasLib-135 (135 nodes): recommended transport demo with continuation preset; steady-state smoke test passes (faer LU stable with component anchoring on fragmented subgraphs).
- GasLib-582 (582 nodes): structural solver issues (singular Jacobian, faer LU panics) resolved via component anchoring and open-by-default topology; **full steady-state convergence to preset tolerance (3e-3) not yet reached** with the simplified compressor MVP (observed residuals ~5–9 m³/s on continuation tiers, June 2026). Scenario applies a pressure slack (`sink_109`) and strips its imposed flow before solve; optional `.cdf` routing selects valve/compressor decisions (e.g. `d1` / `d1_1`).
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
- Transport compressors: adaptive ratio outer loop or `.cs` performance maps (required for GasLib-582 full convergence).
