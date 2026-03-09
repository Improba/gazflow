# Équations physiques — OpenGasSim

## 1. Écoulement de gaz en conduite

### 1.1 Équation de Darcy-Weisbach (forme gaz)

Pour un gaz compressible en régime permanent isotherme, la relation entre les pressions
amont et aval d'un tuyau est :

$$
P_1^2 - P_2^2 = \frac{f \cdot L \cdot \rho_n \cdot Z \cdot T}{D \cdot A^2 \cdot 2} \cdot Q_n \cdot |Q_n|
$$

Où :
- $P_1$, $P_2$ : pressions amont et aval (Pa)
- $f$ : coefficient de friction de Darcy (sans dimension)
- $L$ : longueur du tuyau (m)
- $D$ : diamètre intérieur (m)
- $A = \pi D^2 / 4$ : section (m²)
- $\rho_n$ : masse volumique du gaz à conditions normales (~0.73 kg/m³ pour CH₄)
- $Z$ : facteur de compressibilité (≈ 1 pour le MVP)
- $T$ : température absolue (K), typiquement 288.15 K
- $Q_n$ : débit volumique à conditions normales (m³/s)

### 1.2 Forme simplifiée

En posant la **résistance hydraulique** du tuyau :

$$
K = \frac{f \cdot L}{2 \cdot D \cdot A^2}
$$

On obtient :

$$
P_1^2 - P_2^2 = K \cdot Q \cdot |Q|
$$

D'où le débit :

$$
Q = \text{sign}(P_1^2 - P_2^2) \cdot \sqrt{\frac{|P_1^2 - P_2^2|}{K}}
$$

### 1.3 Coefficient de friction — Approximation de Swamee-Jain

Pour éviter la résolution implicite de Colebrook-White :

$$
f = \frac{0.25}{\left[\log_{10}\left(\frac{\varepsilon/D}{3.7} + \frac{5.74}{Re^{0.9}}\right)\right]^2}
$$

Valable pour $5000 < Re < 10^8$ et $10^{-6} < \varepsilon/D < 10^{-2}$.

Pour le gaz naturel haute pression, $Re \approx 10^6 - 10^7$ (turbulent pleinement développé).

---

## 2. Conservation de la masse aux nœuds

À chaque nœud $i$ du réseau, la somme algébrique des débits est nulle :

$$
\sum_{j \in \text{voisins}(i)} Q_{ij} + d_i = 0
$$

Où $d_i$ est le débit injecté (source > 0) ou soutiré (puits < 0) au nœud $i$.

---

## 3. Système d'équations et résolution

### 3.1 Formulation nodale

Variables : pressions au carré $\pi_i = P_i^2$ à chaque nœud.

Pour chaque nœud $i$ non fixé en pression, l'équation résiduelle est :

$$
F_i(\boldsymbol{\pi}) = \sum_{j \in \text{voisins}(i)} \text{sign}(\pi_i - \pi_j) \cdot \sqrt{\frac{|\pi_i - \pi_j|}{K_{ij}}} + d_i = 0
$$

### 3.2 Newton-Raphson

Le Jacobien $\mathbf{J}$ a pour éléments :

$$
J_{ij} = \frac{\partial F_i}{\partial \pi_j}
$$

Pour un tuyau $(i, j)$ :

$$
\frac{\partial Q_{ij}}{\partial \pi_i} = \frac{1}{2 \sqrt{K_{ij} \cdot |\pi_i - \pi_j|}}
$$

Itération :

$$
\boldsymbol{\pi}^{(k+1)} = \boldsymbol{\pi}^{(k)} - \mathbf{J}^{-1} \cdot \mathbf{F}(\boldsymbol{\pi}^{(k)})
$$

Le système linéaire $\mathbf{J} \cdot \Delta\boldsymbol{\pi} = -\mathbf{F}$ est résolu
avec `faer` (décomposition LU ou Cholesky pour matrices creuses).

### 3.3 Convergence

- Critère d'arrêt : $\|\mathbf{F}\|_\infty < \varepsilon$ (ex: $\varepsilon = 10^{-4}$ bar).
- Relaxation sous-relaxée possible si oscillations : $\boldsymbol{\pi}^{(k+1)} = \boldsymbol{\pi}^{(k)} + \alpha \cdot \Delta\boldsymbol{\pi}$ avec $\alpha \in (0, 1]$.
- Initialisation : pressions uniformes (ex: 70 bar), puis raffinée par le résultat précédent pour les simulations successives.

---

## 4. Hypothèses du MVP

| Hypothèse | Justification |
|-----------|---------------|
| Gaz parfait ($Z = 1$) | Simplification ; $Z \approx 0.9$ en pratique |
| Température uniforme 15 °C | Isotherme, pas de modèle thermique |
| Pas de compresseurs | Phase ultérieure |
| Pas d'effets gravitaires | Terrain plat pour GasLib-11 |
| Régime permanent uniquement | Pas de transitoire |

---

## 5. Références

- Osiadacz, A.J. (1987). *Simulation and Analysis of Gas Networks*. Gulf Publishing.
- Ríos-Mercado, R.Z., Borraz-Sánchez, C. (2015). Optimization problems in natural gas transportation systems: A state-of-the-art review. *Applied Energy*, 147, 536-555.
- Schmidt, M. et al. (2017). GasLib — A Library of Gas Network Instances. *Data*, 2(4), 40.
- Swamee, P.K., Jain, A.K. (1976). Explicit equations for pipe-flow problems. *ASCE J. Hydraulic Division*, 102(5), 657-664.
