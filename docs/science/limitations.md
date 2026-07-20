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
- Compressors: simplified pressure-lift MVP (ratio P² coefficient, not full enthalpic balance). Optional **`.cs` performance maps** (measurement / biquadratic modes): outer loop updates `compressor_ratio_max` from head/speed search; **in-Newton recoupling** (`GAZFLOW_NEWTON_COMPRESSOR_MAP`, default on in map modes) evaluates map ratio each Newton iteration (semi-implicit Jacobian). Operating ratio from catalogue (~1,08/stage); pressure cap from `.net` (e.g. 4,09 transport) stored separately (`compressor_pressure_cap_ratio`); **dynamic outlet cap** `pressureOutMax / P_in` (Phase VI) bounds the map target so hard coupling `P_out = r·P_in` respects the physical outlet limit. Legacy blend fallback remains for networks without `.cs`.
- Flow variable: normal volumetric flow (Nm³/s) at **15 °C / 1,01325 bar**; PCS/PCI/Wobbe at **ISO 6976 0 °C** basis — see `equations.md` §2.4.
- **P8 regulators**: outer loop + downstream slack; bypass with hysteresis; isothermal expansion (no Joule–Thomson). Hydrostatic threshold uses $\rho(P_{\text{consigne}}, T_{\text{défaut}})$ from gas composition (not a fixed $\rho = 50\ \text{kg/m}^3$). Control valves: **effective diameter** from $C_v$ and opening (not full ISA gas choking). Regulator Jacobian: finite-difference row coupling (not fully analytic).
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
- **PDE boundary mass balance (transient)**: schéma volumes finis conservatif (BC pression amont sur le bord, pas Dirichlet sur cellule 0) ; bilan cumulatif $|ΔM − ρ_n ∫(Q_{in}−Q_{out})\\,dt| / |ΔM|$ vérifié à **5 %** par `test_pde_mass_balance_integrated` ; régime stationnaire : $|Q_{in} − Q_{out}| < 10^{-4}$ Nm³/s et $|ΔM| < 10^{-3}$ kg/pas.
- **PDE segment conductance**: $G = 2 P_{\mathrm{ref}} / (R \sqrt{Q_{\mathrm{prev}}^2 + \varepsilon^2})$ with $\varepsilon = 10^{-3}$ Nm³/s. Chord consistent with $P_1^2 - P_2^2 = R Q|Q|$ and $\Delta\pi \approx 2 P_{\mathrm{ref}} \Delta P$ ($\Delta P = R Q^2 / (2 P_{\mathrm{ref}})$), regularized at zero flow. Coupling $Q \approx G\,\Delta P$ (bar) in the implicit Euler step. **$G$ is lagged** at $Q_{\mathrm{prev}}$ (previous-step interface flow), not the implicit $Q$ of the current step: quasi-Newton chord linearization. The factor **2** in $2 P_{\mathrm{ref}}$ is intentional to recover steady $\Delta P = R Q^2 / (2 P_{\mathrm{ref}})$.
- **PDE storage capacitance** (corrected): $C = A L\, (\partial\rho/\partial P) / \rho_n$ in Nm³/bar, consistent with $Q$ in Nm³/s and linepack $M = \rho A L$ aggregated in Nm³ via $\rho_n$.

## 2.1 Transport GasLib (`.cdf`, `.scn`)

- **Default topology**: valves and control valves are **open** after parsing; combined decisions (`.cdf`) and scenario boundaries close or activate equipment explicitly.
- **`.cdf` routing**: when a `.cdf` file exists next to the loaded `.net` (including versioned symlinks such as `GasLib-582.net` → `GasLib-582-v2-….net`), the solver selects combined routing decisions before the steady-state solve (screening + optional full validation of top candidates). A routing is applied **only if it improves the default open topology** (connectivity first, then screening score); otherwise the baseline is kept. Connectivity of the active subgraph to all demand and fixed-pressure nodes is required.
- **Transport scenarios**: un nœud slack pression est détecté (ex. `sink_109` sur GasLib-582) ; son débit nominé est retiré avant solve car P fixe + Q imposé serait sur-contraint. Les autres entries/exits gardent Q nominé (égalité) ; les enveloppes pression du `.scn` ne sont pas imposées au Newton (bornes `.net` en post-contrôle seulement).
- **Scenario pressure anchors (transport, GasLib-582+)**: fermeture numérique DOF via `enrich_scenario_with_balance_hub` et refinement itératif bench. Gain réel nomination intacte : 5 → **2,045 m³/s** (v13–v17). v18 abandon Q sur boundaries abaisse le résidu effectif (~2,0) mais **viole la nomination** — comparer `nomination_mass_balance`, `boundary_nomination_slips` et `mass_balance` dans le JSON `compressor_diag`. Sur-ancrage dégrade le résidu.
- Disable automatic routing with `GAZFLOW_SKIP_CDF_ROUTING=1` (or `GAZFLOW_SKIP_CDF=1`).
- **Compressor map modes** (`GAZFLOW_COMPRESSOR_MAP_MODE`): `legacy`, `measurement`, `biquadratic`. In-Newton map (v17), head Jacobian (v19), enthalpic cap (v20), energy closure H_map↔H_req (v21) — all opt-in except v17 in map modes. See [GasLib-582 bench](../testing/gaslib-582-compressor-bench.md).
- **Compressor outer loop (fallback)**: after continuation failure on transport networks (≥200 nodes with high-ratio compressors), a progressive blend schedule ramps `compressor_ratio_max` toward nominal (`GAZFLOW_SKIP_COMPRESSOR_OUTER=1` to disable; `GAZFLOW_COMPRESSOR_OUTER=1` to force on smaller networks). With map modes, outer loop applies `find_operating_point_for_mode` and guarded ratio steps until residual settles or partial accept.
- **CDF screening**: multi-scale evaluation via `GAZFLOW_CDF_SCREEN_SCALES` (default `0.15,0.4` for N>500); routings that fragment the active graph (multiple components without fixed pressure) are rejected on large networks. On large connected baselines (no floating components, N>500), CDF screening is **skipped by default**; set `GAZFLOW_FORCE_CDF_ROUTING=1` to run it anyway.

## 2.2 NoVa product path (UI / API)

- **Solve with `scenario_id`** uses `resolve_simulation_demands`: nominal Q from the `.scn` plus partial client overrides merged before the WS solve.
- **Pressure diagnostics** are post-hoc envelope checks on the converged result (except capacity study and N-1, which use `network_with_scenario_boundaries_for_nova`).
- **IPOPT escalation** is never the default; enable via `GAZFLOW_NOVA_IPOPT_ESCALATION` (`on`, `on-notsolved`, `maybe`).
- **Reduced nomination** (`POST /api/nova/nominations/reduced`): mass-balance entries at fixed flow; not a substitute for certification without re-validation.
- **GasLib-582 `mild_618`**: feasible with external IPOPT NLP; the in-repo Newton solver may return `NotSolvedLocal`.
- **No systematic `.sol` validation** against external reference solutions.

## 3. Data and validation limits

- GasLib-11 pressure validation: max relative error < 5 % (`test_gaslib_11_vs_reference_solution`).
- GasLib-135 (135 nodes): recommended transport demo with continuation preset; steady-state smoke test passes (faer LU stable with component anchoring on fragmented subgraphs).
- GasLib-582 (582 nodes): `nomination_mild_618` is **feasible** (proven constructively in Phase VIII-bis by an independent external IPOPT NLP solve — see §3.1 below and [diagnosis](../testing/gaslib-582-compressor-diagnosis.md)). Earlier phases concluded "topological infeasibility" for sink_88/83/108; a zero-demand reachability probe (single anchor source_14 at 86 bar) shows these sinks reach ~86 bar at zero flow, far above their contractual floors (26/21/16 bar). The earlier "capacity = 0 even at zero flow" was an artifact of multiple conflicting pressure anchors (slack 51 bar + sources 70-121 bar + non-convergence), not a real topological infeasibility. The in-repo local Newton solver still reports `NotSolvedLocal` under the full nomination flow because the NoVa NLP is non-convex and the penalty-Newton is weaker than IPOPT (which finds the feasible point reliably when single-threaded); this is a local-solver weakness, not evidence against feasibility.
- Flow comparison against external `.sol` references: not yet systematic. **GasLib-11 ZIP (ZIB)** does not ship a `.sol` file ; oracle externe indisponible pour ce réseau (voir `docs/testing/gaslib-11-quarantine.md`).
- PDE transient: monotonicity tests on single pipe; full wave validation pending. **PDE chain junction mass conservation not yet tested** (explicit junction coupling only).

### 3.2 EOS H₂ — discontinuité au seuil 20 % (juillet 2026)

Le basculement automatique Papay+Kay → PR-78 à H₂ > 20 % (`gas_properties.rs`, `EosModel::auto_for_composition`) introduit un **saut de densité** à la frontière 19,9 % / 20,1 % H₂. Le test `test_eos_h2_continuity_at_20_percent_threshold` :

- vérifie la continuité **intra-régime** (Papay sous 20 %, PR-78 au-dessus) ;
- mesure le saut au seuil (~4,7 % à 70 bar, ~1,7 % à 30 bar, juillet 2026) ;
- borne le saut à **< 15 %** (garde-fou de régression) ;
- confirme l'avertissement `physics_warnings` au-delà de 20 %.

Ce n'est pas masqué : la discontinuité est un artefact du switch EOS. Pour des mélanges proches de 20 % H₂, interpréter les résultats avec prudence ou forcer une EOS unique. Cible post-MVP : EOS unique PR-78 sur toute la plage.

### 3.1 GasLib-582 `nomination_mild_618` — corrected NoVa status (Phase VIII, July 2026)

The earlier verdicts in `gaslib-582-compressor-diagnosis.md` (Phases II-VII-bis) stated that sink_88/83/108 are "topologiquement infeasible" / "hydrauliquement non alimentés" with "capacity = 0 même à débit nul". **These verdicts are retracted**: they were artifacts of the solver's boundary handling, not real infeasibility.

Evidence (`scripts/trace_sink_reachability.py` + `compressor_diag --reachability-probe`):

1. **Topological reachability (static, no solver):** all 5 marginal sinks are reachable from high-pressure sources via conductive paths (pipes + shortPipes + passive CVs + compressor bypass). sink_88 is connected to source_14 (pressureMax 86 bar) via 49 hops crossing 2 control valves (CV_17, CV_7); sink_83 via CV_7; sink_108 via CV_16 + CV_7. sink_125 via 1 shortPipe from source_13; sink_122 via 1 shortPipe from source_10.
2. **Zero-demand reachability probe (single anchor source_14 = 86 bar, CVs passive):** sink_88 = 86.10 bar, sink_83 = 86.36 bar, sink_108 = 86.04 bar — all far above their contractual floors (26/21/16 bar). Control valves in passive mode pass pressure (ΔP ≈ 0 at zero flow).
3. **Entry-anchor sensitivity:** anchoring sources at their per-node `pressureMax` (.net, 51-121 bar) instead of a uniform 70 bar flips sink_122 and sink_125 to feasible (85-86 bar vs needs 74/41). The prior "infeasibility" of those two sinks was an anchoring artifact.

**Root cause of the earlier false verdict:** the capacity study anchored multiple pressure nodes simultaneously at conflicting values (slack sink_109 at 51 bar + all sources at 70 bar + scenario hubs) and ran at non-convergence; the resulting low-pressure iterates were misread as a reachability limit. With a single consistent anchor, pressure propagates correctly through the passive CVs.

**Correct scientific status:** `nomination_mild_618` is **feasible** — proven constructively in
Phase VIII-bis by an independent external NLP solver. At zero flow it is feasible with large
margins (all sinks reach their floors). Under the **full** nomination flow (≈256 m³/s to
sink_109), a bounded NoVa feasibility NLP built independently in Pyomo from the GasLib
`.net`/`.scn` (`scripts/nova/nova_pyomo.py`) and solved with IPOPT (COIN-OR interior-point)
**exhibits a feasible point**: mass-conservation violation `≤ 2.6e-12`, max nodal slack
`3.4e-7 Nm³/s`, 0 bound violations, all marginal sinks in contract bounds (sink_88 40.99 bar
[26,51], sink_83 41.01 [21,71], sink_108 40.99 [16,51], sink_122 74.01 [74,81], sink_125
63.47 [41,84]). Log: `scripts/nova/results/mild_618_ipopt_FEASIBLE.log`.

The NoVa NLP is genuinely **non-convex**: from a naive uniform start, multithreaded IPOPT
reaches the feasible point only ~20% of runs (others stop at non-feasible local minima of the
Phase-1 slack objective); pinning `OMP_NUM_THREADS=1` makes IPOPT reach it reliably (5/5).
This is the phenomenon ZIB reports for local solvers on hard NoVa instances and the reason
GazFlow's weaker penalty-Newton reports `NotSolvedLocal`. The feasibility itself is settled;
the remaining engineering work is to make the in-repo local solver's non-convex convergence
match the external one (multistart, continuation, or SQP/IPOPT backend).

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
- **NoVa feasibility** (Phase VIII + VIII-bis): the bounded local feasibility search
  (`GAZFLOW_NOVA_FEASIBILITY=1`, `equations.md` §4.8) reports `NotSolvedLocal` on `mild_618`.
  Feasibility of `mild_618` is nonetheless **proven** by an independent external IPOPT NLP
  solve (`scripts/nova/nova_pyomo.py` + `scripts/nova/Dockerfile`), which exhibits a feasible
  point under the full nomination. The NoVa NLP is non-convex; remaining work is to bring the
  in-repo local solver's non-convex convergence to the external one (multistart, continuation,
  or an SQP/IPOPT backend) so the local verdict matches. A global solver (Couenne/BARON) is
  no longer needed for `mild_618` (feasibility is settled) but remains the tool to prove
  infeasibility of other nominations.
