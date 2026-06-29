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
- PDE transient: fixed time step; no adaptive CFL yet; junction coupling simplified.

## 3. Data and validation limits

- GasLib-11 pressure validation: max relative error < 5 % (`test_gaslib_11_vs_reference_solution`).
- GasLib-135 (135 nodes): recommended transport demo with continuation preset.
- GasLib-582 (582 nodes): map/visualisation and load tests; **full steady-state convergence not guaranteed** with the simplified compressor MVP — scenario applies a pressure slack (`sink_109`) and strips its imposed flow before solve.
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
