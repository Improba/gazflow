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
- $Q_n$: volumetric flow at standard conditions (m³/s)

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

### 1.2b Unit conversion in code — `pipe_resistance()`

In the code (`solver/steady_state.rs`), resistance $K$ is expressed in **bar²·s²/m⁶** (so that $P_1^2 - P_2^2$ is directly in bar²). The conversion from SI form is:

$$
K_{\text{bar}^2} = \frac{f \cdot L \cdot \rho_{\text{eff}}}{2 \cdot D \cdot A^2 \cdot 10^{10}}
$$

Detail of the $10^{10}$ factor: $1\ \text{bar}^2 = (10^5\ \text{Pa})^2 = 10^{10}\ \text{Pa}^2$.

The term $\rho_{\text{eff}}$ replaces the product $\rho_n \cdot Z \cdot T / T_n$ from §1.1. With MVP assumptions ($Z = 1$, $T = 288$ K, natural gas at ~70 bar):

$$
\rho_{\text{eff}} \approx \frac{P \cdot M}{Z \cdot R \cdot T} \approx 50\ \text{kg/m}^3
$$

**Implementation update:** The Newton/Jacobi solver now uses dynamic $\rho(P,T)$ (Papay + ideal gas correction) via the mean pressure of each pipe segment. The fixed $\rho_{\text{eff}}$ form is kept as a baseline for compatibility/testing.

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

For high-pressure natural gas ($P \approx 70$ bar), $Re \approx 10^6 - 10^7$ (fully turbulent). The MVP uses fixed $Re = 10^7$.

**TODO:** Compute Re dynamically from flow and gas properties.

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

With $P_r = P / P_c$ and $T_r = T / T_c$ (reduced pressures/temperatures). For CH₄: $P_c = 46.0$ bar, $T_c = 190.6$ K.

At 70 bar and 288 K: $Z \approx 0.87$.

### 2.3 Dynamic viscosity

Lee-Gonzalez-Eakin correlation:

$$
\mu = 10^{-4} \cdot K_\mu \cdot \exp\left(X_\mu \cdot \rho^{Y_\mu}\right)
$$

For the MVP, constant value: $\mu = 1.1 \times 10^{-5}$ Pa·s.

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

### 4.3 Method B: Full Newton-Raphson (faer) ⬜

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

**Line search (backtracking) ⬜:** Essential for robustness of full Newton. Algorithm:

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

---

## 5. MVP assumptions and roadmap

| MVP assumption                    | Value        | Planned upgrade        |
| --------------------------------- | ------------ | ---------------------- |
| Ideal gas ($Z = 1$)               | ✅ implemented | Papay (§2.2)           |
| Fixed $\rho_{eff}$ (50 kg/m³)     | ✅ implemented | Eq. of state (§2.1)    |
| Fixed $Re$ ($10^7$)               | ✅ implemented | Dynamic Re (§1.4)      |
| Uniform temperature 288 K         | ✅ implemented | Thermal profile        |
| Compressors (MVP directional uplift) | ✅ implemented | Enthalpic model        |
| No gravity effects                | ⬜            | Term $\rho g \Delta h$  |
| Steady state                      | ✅ implemented | Transient (PDE)         |
| Diagonal Jacobi solver            | ✅ implemented | Sparse Newton (§4.3)   |

---

## 5b. Validation against reference solutions ⬜

GasLib provides `.sol` files with reference pressures and flows for each instance. Validation is done in two tiers:

| Tier                         | Max relative error (pressure) | Conditions                                   |
| ---------------------------- | ----------------------------- | ------------------------------------------- |
| MVP (fixed ρ, Z=1)           | < 5%                          | Acceptable for nodes at ~50–80 bar pressure  |
| Post-upgrade (ρ(P,T), Papay) | < 1%                          | Target after tasks 2.5 + 2.11                 |

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
