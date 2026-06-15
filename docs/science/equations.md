# Physical equations — GazFlow

See also: `docs/science/limitations.md` for known limits of the model.

## 1. Gas flow in pipes

### 1.1 Darcy-Weisbach equation (gas form)

For a compressible gas in steady-state isothermal flow, the relation between upstream and downstream pressures in a pipe is:

$$
P_1^2 - P_2^2 = \frac{f \cdot L \cdot \rho_n \cdot Z \cdot T}{D \cdot A^2 \cdot 2} \cdot Q_n \cdot |Q_n|
$$

Where:

- $P_1$, $P_2$: upstream and downstream pressures (Pa)
- $f$: Darcy friction factor (dimensionless)
- $L$: pipe length (m)
- $D$: inner diameter (m)
- $A = \pi D^2 / 4$: cross-section (m²)
- $\rho_n$: gas density at standard conditions (~0.73 kg/m³ for CH₄)
- $Z$: compressibility factor
- $T$: absolute temperature (K)
- $Q_n$: volumetric flow at standard conditions (Nm³/s)

### 1.2 Simplified form

Defining the **hydraulic resistance** of the pipe:

$$
K = \frac{f \cdot L}{2 \cdot D \cdot A^2}
$$

We get:

$$
P_1^2 - P_2^2 = K \cdot Q \cdot |Q|
$$

Hence the flow:

$$
Q = \text{sign}(P_1^2 - P_2^2) \cdot \sqrt{\frac{|P_1^2 - P_2^2|}{K}}
$$

With elevation change ($\Delta z = z_2 - z_1$), the code uses:

$$
P_1^2 - P_2^2 = K \cdot Q \cdot |Q| + \Delta G, \quad
\Delta G = \frac{\rho \cdot g \cdot \Delta z \cdot (P_1 + P_2)}{10^{10}} \quad [\text{bar}^2]
$$

($\rho$ at mean pipe pressure, $g = 9.80665$ m/s²). **Status:** ✅ `gravity_dp_sq_bar()`, `pipe_flow_with_gravity()`.

### 1.2b Unit conversion in code — `pipe_resistance()`

In the code (`solver/steady_state.rs`), resistance $K$ is expressed in **bar²·s²/m⁶** (so that $P_1^2 - P_2^2$ is directly in bar²). The conversion from SI form is:

$$
K_{\text{bar}^2} = \frac{f \cdot L \cdot \rho(P_{\text{moy}})}{2 \cdot D \cdot A^2 \cdot 10^{10}}
$$

Detail of the $10^{10}$ factor: $1\ \text{bar}^2 = (10^5\ \text{Pa})^2 = 10^{10}\ \text{Pa}^2$.

**Convention de débit (important)** : $Q$ dans le solveur et les scénarios GasLib est le **débit normal** (Nm³/s à 15 °C / 1,01325 bar). La résistance utilise $\rho(P_{\text{moy}})$ — densité réelle au col moyen du tronçon (Papay + composition), pas $\rho_n$ seul. Cette formulation P² avec densité in situ est la forme opérationnelle standard des solveurs réseau HP (Osiadacz, GasLib) : elle couple correctement pressions et débits normaux lorsque $\rho(P_{\text{moy}})$ est cohérent avec la loi d'état. La forme SI §1.1 avec $\rho_n$ explicite est équivalente après chaîne de conversion $Q_n \leftrightarrow \dot{m} \leftrightarrow Q_{\text{ligne}}$ ; le code la compresse en $\rho(P_{\text{moy}})$.

Historique MVP : $\rho_{\text{eff}} = 50\ \text{kg/m}^3$ fixe (~70 bar CH₄). **Implémentation actuelle** : $\rho(P,T)$ dynamique via Papay + composition ; Re dynamique via `pipe_resistance_hydraulic()` quand $|Q|>0$ ; Newton garde $Re = 10^7$ au Jacobian ($Q=0$) pour stabilité numérique.

### 1.3 Friction factor — Swamee-Jain approximation

To avoid implicit solution of Colebrook-White:

$$
f = \frac{0.25}{\left[\log_{10}\left(\frac{\varepsilon/D}{3.7} + \frac{5.74}{Re^{0.9}}\right)\right]^2}
$$

Valid for $5000 < Re < 10^8$ and $10^{-6} < \varepsilon/D < 10^{-2}$.

**Implementation status:** ✅ `solver/steady_state.rs::darcy_friction()`

### 1.4 Reynolds number for gas

$$
Re = \frac{\rho \cdot v \cdot D}{\mu} = \frac{4 \cdot \dot{m}}{\pi \cdot D \cdot \mu}
$$

For high-pressure natural gas ($P \approx 70$ bar), $Re \approx 10^6 - 10^7$ (fully turbulent) when using actual line velocity. With **normal volumetric flow** $Q_n$ (Nm³/s at 15 °C / 1,01325 bar) as in the solver:

$$
Re = \frac{4 \cdot \rho_n \cdot Q_n}{\pi \cdot D \cdot \mu}
$$

where $\rho_n$ is density at standard conditions (not line density).

**Implementation status:** ✅ `reynolds_from_standard_flow()` in `gas_properties.rs`; used in `pipe_resistance_hydraulic()` when $|Q| > 0$. The Newton Jacobian uses $Re = 10^7$ when $Q = 0$ (turbulent plateau, numerical stability).

---

## 2. Gas properties

### 2.1 Equation of state (simplified BWRS)

**Status:** ✅ Implemented (MVP+): $\rho(P,T)$ computed in `solver/gas_properties.rs` and injected into pipe resistances (Newton/Jacobi) via segment mean pressure.

Beyond ideal gas, density at pressure $P$ and temperature $T$:

$$
\rho(P, T) = \frac{P \cdot M}{Z(P, T) \cdot R \cdot T}
$$

Where:

- $M$ = molar mass of gas (16.04 g/mol for pure CH₄)
- $R$ = 8.314 J/(mol·K)
- $Z(P, T)$ = compressibility factor

### 2.2 Compressibility factor Z

Papay approximation (simple, sufficient for MVP+):

$$
Z = 1 - \frac{3.52 \cdot P_r}{10^{0.9813 \cdot T_r}} + \frac{0.274 \cdot P_r^2}{10^{0.8157 \cdot T_r}}
$$

With $P_r = P / P_c$ and $T_r = T / T_c$ (pseudo-criticals from **Kay's mixing rule** on les composants, puis Papay). Pour CH₄ pur : $P_c = 46.0$ bar, $T_c = 190.6$ K.

At 70 bar and 288 K: $Z \approx 0.87$.

### 2.3 Dynamic viscosity

Lee-Gonzalez-Eakin correlation:

$$
\mu = 10^{-4} \cdot K_\mu \cdot \exp\left(X_\mu \cdot \rho^{Y_\mu}\right)
$$

For the MVP, constant value: $\mu = 1.1 \times 10^{-5}$ Pa·s (legacy).

**Implementation status:** ✅ Lee-Gonzalez-Eakin (SPE-1340-PA) in `gas_properties.rs::lee_gonzalez_eakin_viscosity_pa_s()` — temperature in °R, $Y = 2.4 - 0.2X$, factor $10^{-4}$ ; repli limite diluée si $\rho < 0.1\ \text{kg/m}^3$.

### 2.4 Reference states (hydraulics vs calorific values)

Two reference states coexist in the solver; they must not be conflated:

| Quantity | Reference state | Use |
|----------|-----------------|-----|
| Normal flow $Q_n$ (Nm³/s, Nm³/h) | **15 °C**, 1,01325 bar | Hydraulic solver, GasLib, demand profiles |
| Line density $\rho(P_{\text{moy}})$ | Papay + Kay at **288,15 K** (isothermal network) | Pipe resistance, linepack $M = \sum \rho A L$ |
| PCS / PCI (MJ/Nm³) | **0 °C**, 101,325 kPa (ISO 6976 component tables, ideal mixing) | Energy indicators, interchangeability |
| Wobbe index | PCS at 0 °C; relative density $d = M_{\text{gas}}/M_{\text{air}}$ at **15 °C** (EN 437) | Burner interchangeability |

Hydraulic and calorific quantities are therefore reported at different standard temperatures by design. Comparing PCS (0 °C basis) with a volumetric delivery forecast (15 °C basis) requires an explicit conversion if energy balances are needed.

**H₂ blends:** Papay + Kay is validated for classical natural gas (H₂ ≲ 10–20 %). Above ~20 % H₂, the solver **automatically switches to PR-78** for $Z$ and $\rho$ (`GasComposition::compressibility()`); the API still returns an informational warning. GERG-2008 remains a future upgrade for highest accuracy.

At fixed normal flow $Q_n$, Reynolds based on $\rho_{\mathrm{std}}$ and $\mu(P_{\mathrm{ligne}})$ **decreases** when H₂ is added (lower standard density dominates over lower viscosity); turbulent regime is nevertheless maintained and total $\Delta P_{\mathrm{friction}}$ still drops because $\rho(P_{\mathrm{moy}})$ in the resistance term decreases faster than $f(Re)$ adjusts.

---

## 3. Mass conservation at nodes

At each node $i$ of the network, the algebraic sum of flows is zero:

$$
\sum_{j \in \text{neighbors}(i)} Q_{ij} + d_i = 0
$$

Where $d_i$ is the flow injected (source > 0) or withdrawn (sink < 0) at node $i$.

**Implementation convention:** In the code, $Q_{ij} > 0$ means flow from $i$ to $j$. The residual $F_i = \sum Q_{\text{in}} - \sum Q_{\text{out}} + d_i$.

**Status:** ✅ Implemented and tested (Y-network test: conservation < 1e-4).

---

## 4. Equation system and solution

### 4.1 Nodal formulation

Variables: squared pressures $\pi_i = P_i^2$ at each node.

For each node $i$ not fixed in pressure, the residual equation is:

$$
F_i(\boldsymbol{\pi}) = \sum_{j \in \text{neighbors}(i)} \text{sign}(\pi_i - \pi_j) \cdot \sqrt{\frac{|\pi_i - \pi_j|}{K_{ij}}} + d_i = 0
$$

### 4.2 Method A: Diagonal Newton-Raphson (Jacobi) ✅

Diagonal approximation of the Jacobian. Simple and robust.

Linearised conductance:

$$
g_{ij} = \frac{1}{2\sqrt{K_{ij} \cdot |\pi_i - \pi_j|}}
$$

Diagonal Jacobian:

$$
J_{ii} = -\sum_{j} g_{ij}
$$

Update:

$$
\Delta\pi_i = \frac{F_i}{\sum_j g_{ij}} \quad (\text{with relaxation } \alpha = 0.8)
$$

**Advantages:** No matrix inversion, very fast per iteration. **Drawbacks:** Slow convergence (linear), may need many iterations.

**Status:** ✅ Implemented, converges on 2-node and Y-network.

### 4.3 Method B: Full Newton-Raphson (faer) ✅

The full Jacobian $\mathbf{J}$ is a **sparse** matrix (non-zero only for node pairs connected by a pipe).

Off-diagonal elements ($i \neq j$, pipe between $i$ and $j$):

$$
J_{ij} = \frac{\partial F_i}{\partial \pi_j} = g_{ij}
$$

Diagonal elements:

$$
J_{ii} = \frac{\partial F_i}{\partial \pi_i} = -\sum_{j \in \text{neighbors}(i)} g_{ij}
$$

Solution via sparse LU decomposition (`faer::sparse::SparseLU`):

$$
\mathbf{J} \cdot \Delta\boldsymbol{\pi} = -\mathbf{F}
$$

$$
\boldsymbol{\pi}^{(k+1)} = \boldsymbol{\pi}^{(k)} + \alpha \cdot \Delta\boldsymbol{\pi}
$$

**Advantages:** Quadratic convergence (very fast, ~5–10 iterations). **Drawbacks:** Matrix assembly and factorisation at each iteration.

**Parallelisation:** Jacobian assembly is parallelisable via Rayon. Sparse LU factorisation is internally parallelised by faer.

**Line search (backtracking) ✅:** Essential for robustness of full Newton. Algorithm:

1. Compute direction $\Delta\boldsymbol{\pi}$ via LU solve.
2. Try $\alpha = 1$. If $\|\mathbf{F}^{(k+1)}\|_\infty > \|\mathbf{F}^{(k)}\|_\infty$, halve $\alpha$ and retry (max 5 halvings).
3. If no $\alpha$ reduces the residual, fallback to Jacobi for that iteration.

This ensures global convergence even far from the solution (uniform pressure initialisation, ill-conditioned Jacobian in early iterations).

### 4.4 Non-dimensionalisation ✅

For numerical stability on large networks, non-dimensionalise variables:

$$
\hat{\pi}_i = \frac{\pi_i}{\pi_{ref}}, \quad \hat{Q} = \frac{Q}{Q_{ref}}, \quad \hat{K} = \frac{K \cdot Q_{ref}^2}{\pi_{ref}}
$$

With $\pi_{ref} = P_{source}^2$ and $Q_{ref} = \max(|d_i|)$.

**Implementation:**

- common scaling `NondimScaling` in `solver/steady_state.rs`;
- flow/conductance computed via non-dimensional variables (`flow_and_conductance`);
- this path used in Jacobi and hybrid Newton solvers.

### 4.5 Convergence

- Stopping criterion: $\|\mathbf{F}\|_\infty < \varepsilon$ (e.g. $\varepsilon = 10^{-4}$).
- Under-relaxation: $\alpha \in (0, 1]$, adaptive if oscillations detected.
- Initialisation: uniform pressures, then warm-start from previous result.
- **Line search** (full Newton): if $\|\mathbf{F}^{(k+1)}\| > \|\mathbf{F}^{(k)}\|$, halve $\alpha$ and retry.

### 4.6 Organes de régulation (P8) ✅ MVP

**Détendeur / régulateur à consigne aval** — comportement cible :

$$
P_{\text{aval}} = P_{\text{consigne}} \quad \text{si} \quad P_{\text{amont}} \ge P_{\text{consigne}} + \Delta P_{\min}
$$

Sinon **bypass** (liaison quasi transparente, sans imposer la consigne).

**Commutation actif/bypass :** $P_{\text{amont}}$ est lu sur une **résolution de référence** où tous les régulateurs sont en bypass (liaisons à faible perte). En mode actif, la liaison reste quasi transparente et le nœud aval est traité comme **slack** à $P_{\text{consigne}}$ ; on ne peut donc pas déduire $P_{\text{amont}}$ de cette solution finale (sinon $P_{\text{amont}} \approx P_{\text{consigne}}$ et faux bypass).

Hystérésis : activation si $P_{\text{amont}} \ge P_{\text{requis}}$ ; désactivation depuis actif si $P_{\text{amont}} < P_{\text{requis}} - 0{,}05\,\Delta P_{\min}$, avec

$$
P_{\text{requis}} = P_{\text{consigne}} + \Delta P_{\min} + \rho\, g\, (z_{\text{aval}} - z_{\text{amont}})
$$

($\rho \approx 50\ \text{kg/m}^3$ au seuil ; pressions en bar manométriques aux nœuds). En descente, $P_{\text{requis}}$ peut être $< P_{\text{consigne}}$ ; borné inférieurement à $10^{-6}$ bar pour la stabilité numérique.

**Hypothèses physiques MVP :** détente isotherme (pas de Joule–Thomson) ; liaison active quasi sans perte (pas de $\Delta P_{\min}$ imposée sur l'arc en mode actif) ; $C_v$ ISA calibré via diamètre effectif, pas formule gaz complète.

**Vanne de régulation (Cv)** — cible ISA :

$$
Q = C_v \cdot N \cdot Y \cdot \sqrt{\frac{x \cdot P_1}{\rho_1}}, \quad x = \frac{P_1 - P_2}{P_1}
$$

**MVP implémenté :** conductance effective $\propto C_v \cdot (\text{ouverture}/100)$ ; dans la loi $K Q|Q| = \Delta\pi$, $K \propto 1/(C_v \cdot \text{ouverture})^2$ → diamètre effectif $D_{\text{eff}} \propto \sqrt{C_v \cdot \text{ouverture}}$. Ouverture 0 % : arc exclu du graphe.

**Poste de livraison :** consigne $P_{\text{consigne}}$ (régulation) distincte du minimum contractuel $P_{\min,\text{livr}}$ (vérification a posteriori, avertissement si $P_{\text{aval}} < P_{\min}$).

**Implementation status:** `solver/regulator.rs`, `effective_pipe_geometry()`, boucle externe dans `solve_steady_state_with_progress`.

---

## 4.7 Demand profiles (P9)

**Units:** normal volumetric flow (Nm³/h, Nm³/s) at standard conditions (15 °C, 1.01325 bar), consistent with the hydraulic solver and GasLib.

Thermosensitivity (French distributor linear model, as used for delivery-point forecasting):

$$
Q_{\mathrm{chauff}}(T_{\mathrm{ext}}) = \min\!\bigl(\alpha \max(0,\; T_{\mathrm{seuil}} - T_{\mathrm{ext}}),\; Q_{\mathrm{chauff,max}}\bigr)
$$

$$
Q_{\mathrm{ref}}(T_{\mathrm{ext}}) = Q_0 + Q_{\mathrm{chauff}}(T_{\mathrm{ext}})
\quad [\mathrm{Nm}^3/\mathrm{h}]
$$

$Q_{\mathrm{ref}}$ is the **mean hourly flow** at the given outdoor temperature (not the peak hour). $Q_0$ is base load (DHW, cooking, continuous process); heating is zero when $T_{\mathrm{ext}} \ge T_{\mathrm{seuil}}$ (typically 17 °C in distributor models, zones H1–H2). Preset categories target **aggregated delivery / offtake nodes** on a transmission or distribution network, not individual end customers.

**Category presets:** residential (thermosensitive, evening peaks); tertiary (lower $\alpha$, weekday occupation profile); industrial ($\alpha \approx 0$, flat profile).

Daily modulation:

$$
Q_h = Q_{\mathrm{ref}}(T_{\mathrm{ext}})\, m_h,
\qquad
m_h = \frac{w_h^+}{\bar w},
\quad
\bar w = \frac{1}{24}\sum_k w_k,
\quad
w_h^+ = \max(0, w_h),
$$

$$
s_h = \frac{w_h^+}{\sum_k w_k^+},
\qquad
m_h = 24\, s_h \quad \text{when } w_k \ge 0 \;\forall k.
$$

Then $\frac{1}{24}\sum_h Q_h = Q_{\mathrm{ref}}$ (conservation du débit horaire moyen).

$$
d_h = -\frac{Q_h}{3600} \quad [\mathrm{Nm}^3/\mathrm{s}] \text{ (withdrawal)}.
$$

Outdoor temperature $T_{\mathrm{ext}}$ does **not** modify gas temperature in pipes (isothermal network). One scalar $T_{\mathrm{ext}}$ applies to all profile nodes per step.

**Timeseries (quasi-steady):** each hour solves a full steady-state; transient pipe dynamics are ignored. Warm-start uses the previous converged pressure field when $\sum_i|d_i-d_i^{\mathrm{prev}}|/\sum_i|d_i^{\mathrm{prev}}| \le 3$; on Newton failure, a cold restart is attempted.

**Implementation:** `solver/demand.rs`, `resolve_demands`, `solver/timeseries.rs`.

---

## 5. MVP assumptions and current model status

| Feature                           | Status        | Notes |
| --------------------------------- | ------------- | ----- |
| Compressibility Z (Papay)         | ✅ implemented | Kay pseudo-criticals + Papay |
| Density $\rho(P,T)$               | ✅ implemented | Composition + Papay |
| Dynamic viscosity (Lee-Gonzalez)  | ✅ implemented | SPE-1340-PA |
| Dynamic Re in `pipe_resistance_hydraulic` | ✅ implemented | When $|Q|>0$ |
| Newton Jacobian Re                | ✅ $10^7$ plateau | Stabilité numérique ($Q=0$) |
| Uniform temperature 288 K         | ✅ implemented | Thermal profile ⬜ |
| Gravity $\rho g \Delta h$         | ✅ implemented | §1.2 |
| Multi-component gas (G20 default) | ✅ implemented | `GasComposition` |
| Compressors (MVP uplift)          | ✅ implemented | Enthalpic model ⬜ |
| Steady state                      | ✅ implemented | Transient (PDE) ⬜ |
| Hybrid Newton + Jacobi fallback   | ✅ implemented | `newton.rs` |
| Régulateurs / détendeurs (P8)     | ✅ MVP         | Boucle externe + slack aval |
| Vannes Cv (P8)                    | 🟨 MVP         | $D_{\text{eff}} \propto \sqrt{C_v \cdot \text{ouverture}}$ |
| Profils de demande (P9)           | ✅ MVP         | §4.7 thermo + profil journalier |
| Fixed $\rho_{\text{eff}}=50$ legacy | 🟨 test only | `pipe_resistance()` baseline |

---

## 5b. Validation against reference solutions ✅ (pressions)

GasLib provides `.sol` files with reference pressures and flows for each instance. Validation is done in two tiers:

| Tier                         | Max relative error (pressure) | Conditions                                   |
| ---------------------------- | ----------------------------- | ------------------------------------------- |
| MVP (fixed ρ, Z=1)           | < 5%                          | Acceptable for nodes at ~50–80 bar pressure  |
| Post-upgrade (ρ(P,T), Papay)  | < 5% (GasLib-11 OK)           | Cible < 1 % sur jeux élargis ⬜               |

**Method:**

$$
e_i = \frac{|P_i^{\text{calc}} - P_i^{\text{ref}}|}{P_i^{\text{ref}}} \times 100
$$

The test `test_gaslib_11_vs_reference_solution` (T2-8) will load the `.sol`, run the solver with the same demands, and check $\max_i(e_i) < \text{threshold}$.

---

## 6. References

- Osiadacz, A.J. (1987). *Simulation and Analysis of Gas Networks*. Gulf Publishing.
- Ríos-Mercado, R.Z., Borraz-Sánchez, C. (2015). Optimization problems in natural gas transportation systems. *Applied Energy*, 147, 536-555.
- Schmidt, M. et al. (2017). GasLib — A Library of Gas Network Instances. *Data*, 2(4), 40.
- Swamee, P.K., Jain, A.K. (1976). Explicit equations for pipe-flow problems. *ASCE J. Hydraulic Division*, 102(5), 657-664.
- Papay, J. (1968). A termelestechnologiai parameterek valtozasa a gaztelepek muvelese soran. *OGIL Muszaki Tudomanyos Kozlemenyek*, Budapest.
- Lee, A.L., Gonzalez, M.H., Eakin, B.E. (1966). The viscosity of natural gases. *JPT*, 18(8), 997-1000.
