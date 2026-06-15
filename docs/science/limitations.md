# Model limitations — GazFlow

This document describes the known limits of the solver in its current state. It complements `docs/science/equations.md` (model) and `docs/science/validation.md` (tests).

## 1. Physical limits

- Steady state by default; transient MVP re-solves quasi-steady steps (no wave PDE yet).
- Isothermal assumption (uniform temperature 288 K by default).
- Gas properties: Kay pseudo-criticals + Papay Z, Lee-Gonzalez-Eakin viscosity, G20 or custom composition; runtime warning when H₂ > 20 %.
- Gravity included (`ρ g Δz` term in the P² equation); altitude from import/GasLib.
- Reynolds dynamic in `pipe_resistance_hydraulic` when $|Q|>0$; Newton Jacobian uses $Re=10^7$ for stability.
- Compressors: simplified pressure-lift MVP (not enthalpic).
- H₂ blends > ~20 %: Papay + Kay may be inaccurate; prefer PR-78 / GERG-2008 for such cases.
- Flow variable: normal volumetric flow (Nm³/s) at **15 °C / 1,01325 bar**; PCS/PCI/Wobbe use **ISO 6976 component tables at 0 °C / 101,325 kPa** (ideal mixing) — do not confuse the two reference states when comparing energy vs hydraulic quantities.
- Pressure regulators / delivery stations (P8): outer loop + downstream slack when active; bypass reference for mode switching; static head in activation threshold; isothermal expansion (no Joule–Thomson); control valves use effective diameter from $C_v$ (not full ISA gas choking).
- Demand profiles (P9): quasi-steady hourly sequence (no linepack coupling between hourly steps in timeseries); Nm³ at standard conditions; scalar $T_{\mathrm{ext}}$ per step (no spatial weather map); $T_{\mathrm{ext}}$ does not change gas $T$ in pipes; linear HDD with optional $Q_{\mathrm{chauff,max}}$ cap on presets; same diurnal $m_h$ on base and heating per category; preset categories are delivery-point orders of magnitude; tertiary uses distinct weekday/weekend profiles; industrial preset has no thermosensitivity ($\alpha=0$); weather CSV must have unique hours; $\bar m_h = 1$ when all $w_h \ge 0$.
- SCADA calibration (P13): residuals $r_i = y_i - \hat y_i$; global roughness via Levenberg–Marquardt on $\frac12\sum (r_i/\sigma_i)^2$ (1 parameter, FD Jacobian); per-pipe strategy remains grid search.
- Transient MVP (P11): linepack $M = \sum \rho(P)\, A\, L$ on active pipes only (isothermal $\rho$, no inventory–flow dynamics between quasi-steady steps).

## 2. Numerical limits

- Convergence depends on initialisation, line search, and optional Jacobi fallback.
- Very large networks may require continuation strategies and warm-start.
- Convergence criterion sensitive to input data scaling.
- Some configurations may end in explicit non-convergence.

## 3. Data and validation limits

- GasLib-11 pressure validation: max relative error < 5 % (`test_gaslib_11_vs_reference_solution`).
- Flow comparison against external `.sol` references: not yet systematic.
- Internal reference useful for non-regression but does not replace independent validation.
- Result quality depends on scenario and topology file quality.

## 4. Impact on usage

- The solver is suited for technical and comparative pipeline studies.
- Results must not be interpreted as a guarantee for industrial operation without business calibration.
- Critical decisions (safety, contracts, real-time control) require additional verification.

## 5. Recommended evolutions

- Extend external reference validation (pressure and flow) on a representative set of cases.
- Outer-loop Re–Q coupling in Newton (beyond P7.7 plateau) if sub-1 % accuracy is required.
- GERG-2008 / PR-78 EOS for high-H₂ blends (replace Papay + Kay beyond ~20 % H₂).
- Transient simulation and thermal profiles.
- Consolidate numerical robustness for very large networks.
