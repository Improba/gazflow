# Équations physiques — GazSim

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
- $Z$ : facteur de compressibilité
- $T$ : température absolue (K)
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

### 1.2b Conversion d'unités dans le code — `pipe_resistance()`

Dans le code (`solver/steady_state.rs`), la résistance $K$ est exprimée en **bar²·s²/m⁶**
(pour que $P_1^2 - P_2^2$ soit directement en bar²). La transformation depuis la forme SI est :

$$
K_{\text{bar}^2} = \frac{f \cdot L \cdot \rho_{\text{eff}}}{2 \cdot D \cdot A^2 \cdot 10^{10}}
$$

Détail du facteur $10^{10}$ : $1\ \text{bar}^2 = (10^5\ \text{Pa})^2 = 10^{10}\ \text{Pa}^2$.

Le terme $\rho_{\text{eff}}$ remplace le produit $\rho_n \cdot Z \cdot T / T_n$ de §1.1.
Avec les hypothèses MVP ($Z = 1$, $T = 288$ K, gaz naturel à ~70 bar) :

$$
\rho_{\text{eff}} \approx \frac{P \cdot M}{Z \cdot R \cdot T} \approx 50\ \text{kg/m}^3
$$

**Limitation :** $\rho_{\text{eff}}$ fixe introduit une erreur croissante lorsque la pression
s'éloigne de 70 bar. Upgrade prévu : calculer $\rho(P, T)$ dynamiquement via §2.1 et §2.2.

### 1.3 Coefficient de friction — Approximation de Swamee-Jain

Pour éviter la résolution implicite de Colebrook-White :

$$
f = \frac{0.25}{\left[\log_{10}\left(\frac{\varepsilon/D}{3.7} + \frac{5.74}{Re^{0.9}}\right)\right]^2}
$$

Valable pour $5000 < Re < 10^8$ et $10^{-6} < \varepsilon/D < 10^{-2}$.

**Status implémentation :** ✅ `solver/steady_state.rs::darcy_friction()`

### 1.4 Nombre de Reynolds pour le gaz

$$
Re = \frac{\rho \cdot v \cdot D}{\mu} = \frac{4 \cdot \dot{m}}{\pi \cdot D \cdot \mu}
$$

Pour le gaz naturel haute pression ($P \approx 70$ bar), $Re \approx 10^6 - 10^7$
(turbulent pleinement développé). Le MVP utilise $Re = 10^7$ fixe.

**TODO :** Calculer Re dynamiquement à partir du débit et des propriétés du gaz.

---

## 2. Propriétés du gaz

### 2.1 Équation d'état (BWRS simplifiée)

**Status :** ⬜ Non implémenté. Le MVP utilise $\rho_{eff} = 50$ kg/m³ en dur.

Pour aller au-delà du gaz parfait, la densité à pression $P$ et température $T$ :

$$
\rho(P, T) = \frac{P \cdot M}{Z(P, T) \cdot R \cdot T}
$$

Où :

- $M$ = masse molaire du gaz (16.04 g/mol pour CH₄ pur)
- $R$ = 8.314 J/(mol·K)
- $Z(P, T)$ = facteur de compressibilité

### 2.2 Facteur de compressibilité Z

Approximation de Papay (simple, suffisante pour le MVP+) :

$$
Z = 1 - \frac{3.52 \cdot P_r}{10^{0.9813 \cdot T_r}} + \frac{0.274 \cdot P_r^2}{10^{0.8157 \cdot T_r}}
$$

Avec $P_r = P / P_c$ et $T_r = T / T_c$ (pressions/températures réduites).
Pour CH₄ : $P_c = 46.0$ bar, $T_c = 190.6$ K.

À 70 bar et 288 K : $Z \approx 0.87$.

### 2.3 Viscosité dynamique

Corrélation de Lee-Gonzalez-Eakin :

$$
\mu = 10^{-4} \cdot K_\mu \cdot \exp\left(X_\mu \cdot \rho^{Y_\mu}\right)
$$

Pour le MVP, valeur constante : $\mu = 1.1 \times 10^{-5}$ Pa·s.

---

## 3. Conservation de la masse aux nœuds

À chaque nœud $i$ du réseau, la somme algébrique des débits est nulle :

$$
\sum_{j \in \text{voisins}(i)} Q_{ij} + d_i = 0
$$

Où $d_i$ est le débit injecté (source > 0) ou soutiré (puits < 0) au nœud $i$.

**Convention d'implémentation :** dans le code, $Q_{ij} > 0$ signifie flux de $i$ vers $j$.
Le résidu $F_i = \sum Q_{\text{entrant}} - \sum Q_{\text{sortant}} + d_i$.

**Status :** ✅ Implémenté et testé (test Y-network : conservation < 1e-4).

---

## 4. Système d'équations et résolution

### 4.1 Formulation nodale

Variables : pressions au carré $\pi_i = P_i^2$ à chaque nœud.

Pour chaque nœud $i$ non fixé en pression, l'équation résiduelle est :

$$
F_i(\boldsymbol{\pi}) = \sum_{j \in \text{voisins}(i)} \text{sign}(\pi_i - \pi_j) \cdot \sqrt{\frac{|\pi_i - \pi_j|}{K_{ij}}} + d_i = 0
$$

### 4.2 Méthode A : Newton-Raphson diagonal (Jacobi) ✅

Approximation diagonale du Jacobien. Simple et robuste.

Conductance linéarisée :

$$
g_{ij} = \frac{1}{2\sqrt{K_{ij} \cdot |\pi_i - \pi_j|}}
$$

Jacobien diagonal :

$$
J_{ii} = -\sum_{j} g_{ij}
$$

Mise à jour :

$$
\Delta\pi_i = \frac{F_i}{\sum_j g_{ij}} \quad (\text{avec relaxation } \alpha = 0.8)
$$

**Avantages :** Pas de matrice à inverser, très rapide par itération.
**Inconvénients :** Convergence lente (linéaire), peut nécessiter beaucoup d'itérations.

**Status :** ✅ Implémenté, converge sur 2-nœuds et Y-network.

### 4.3 Méthode B : Newton-Raphson complet (faer) ⬜

Le Jacobien complet $\mathbf{J}$ est une matrice **creuse** (non-zéro seulement pour les
paires de nœuds connectés par un tuyau).

Éléments hors-diagonaux ($i \neq j$, tuyau entre $i$ et $j$) :

$$
J_{ij} = \frac{\partial F_i}{\partial \pi_j} = g_{ij}
$$

Éléments diagonaux :

$$
J_{ii} = \frac{\partial F_i}{\partial \pi_i} = -\sum_{j \in \text{voisins}(i)} g_{ij}
$$

Résolution via décomposition LU creuse (`faer::sparse::SparseLU`) :

$$
\mathbf{J} \cdot \Delta\boldsymbol{\pi} = -\mathbf{F}
$$

$$
\boldsymbol{\pi}^{(k+1)} = \boldsymbol{\pi}^{(k)} + \alpha \cdot \Delta\boldsymbol{\pi}
$$

**Avantages :** Convergence quadratique (très rapide, ~5-10 itérations).
**Inconvénients :** Assemblage et factorisation de la matrice à chaque itération.

**Parallélisation :** L'assemblage du Jacobien est parallélisable via Rayon.
La factorisation LU creuse est parallélisée en interne par faer.

**Line search (backtracking) ⬜ :** Indispensable pour la robustesse du Newton complet.
Algorithme :

1. Calculer la direction $\Delta\boldsymbol{\pi}$ via la résolution LU.
2. Essayer $\alpha = 1$. Si $\|\mathbf{F}^{(k+1)}\|_\infty > \|\mathbf{F}^{(k)}\|_\infty$,
  diviser $\alpha$ par 2 et réessayer (max 5 halvings).
3. Si aucun $\alpha$ ne réduit le résidu, fallback Jacobi pour cette itération.

Cela garantit la convergence globale même loin de la solution (initialisation
à pression uniforme, Jacobien mal conditionné aux premières itérations).

### 4.4 Non-dimensionnalisation ⬜

Pour la stabilité numérique sur les grands réseaux, adimensionner les variables :

$$
\hat{\pi}_i = \frac{\pi_i}{\pi_{ref}}, \quad \hat{Q} = \frac{Q}{Q_{ref}}, \quad \hat{K} = \frac{K \cdot Q_{ref}^2}{\pi_{ref}}
$$

Avec $\pi_{ref} = P_{source}^2$ et $Q_{ref} = \max(|d_i|)$.

### 4.5 Convergence

- Critère d'arrêt : $\mathbf{F}_\infty < \varepsilon$ (ex: $\varepsilon = 10^{-4}$).
- Relaxation sous-relaxée : $\alpha \in (0, 1]$, adaptatif si oscillations détectées.
- Initialisation : pressions uniformes, puis warm-start depuis le résultat précédent.
- **Line search** (Newton complet) : si $\mathbf{F}^{(k+1)} > \mathbf{F}^{(k)}$,
réduire $\alpha$ de moitié et réessayer.

---

## 5. Hypothèses du MVP et feuille de route


| Hypothèse MVP                | Valeur       | Upgrade prévu           |
| ---------------------------- | ------------ | ----------------------- |
| Gaz parfait ($Z = 1$)        | ✅ implémenté | Papay (§2.2)            |
| $\rho_{eff}$ fixe (50 kg/m³) | ✅ implémenté | Eq. d'état (§2.1)       |
| $Re$ fixe ($10^7$)           | ✅ implémenté | Re dynamique (§1.4)     |
| Température uniforme 288 K   | ✅ implémenté | Profil thermique        |
| Pas de compresseurs          | ⬜            | Modèle enthalpique      |
| Pas d'effets gravitaires     | ⬜            | Terme $\rho g \Delta h$ |
| Régime permanent             | ✅ implémenté | Transitoire (PDE)       |
| Solveur Jacobi diagonal      | ✅ implémenté | Newton creux (§4.3)     |


---

## 5b. Validation contre solutions de référence ⬜

GasLib fournit des fichiers `.sol` contenant les pressions et débits de référence pour
chaque instance. La validation se fait en deux paliers :


| Palier                       | Erreur relative max (pression) | Conditions                                      |
| ---------------------------- | ------------------------------ | ----------------------------------------------- |
| MVP (ρ fixe, Z=1)            | < 5%                           | Acceptable pour les nœuds à pression ~50-80 bar |
| Post-upgrade (ρ(P,T), Papay) | < 1%                           | Cible après tâches 2.5 + 2.11                   |


**Méthode :**

$$
e_i = \frac{|P_i^{\text{calc}} - P_i^{\text{ref}}|}{P_i^{\text{ref}}} \times 100
$$

Le test `test_gaslib_11_vs_reference_solution` (T2-8) chargera le `.sol`, lancera le solveur
avec les mêmes demandes, et vérifiera $\max_i(e_i) < \text{seuil}$.

---

## 6. Références

- Osiadacz, A.J. (1987). *Simulation and Analysis of Gas Networks*. Gulf Publishing.
- Ríos-Mercado, R.Z., Borraz-Sánchez, C. (2015). Optimization problems in natural gas
transportation systems. *Applied Energy*, 147, 536-555.
- Schmidt, M. et al. (2017). GasLib — A Library of Gas Network Instances. *Data*, 2(4), 40.
- Swamee, P.K., Jain, A.K. (1976). Explicit equations for pipe-flow problems.
*ASCE J. Hydraulic Division*, 102(5), 657-664.
- Papay, J. (1968). A termelestechnologiai parameterek valtozasa a gaztelepek muvelese soran.
*OGIL Muszaki Tudomanyos Kozlemenyek*, Budapest.
- Lee, A.L., Gonzalez, M.H., Eakin, B.E. (1966). The viscosity of natural gases.
*JPT*, 18(8), 997-1000.

