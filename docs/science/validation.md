# Validation du solveur — GazSim

## Cas de test analytiques

### Test 1 : Tuyau unique (2 nœuds)

**Configuration :**
- Nœud A (source) : pression fixée à 70 bar
- Nœud B (puits) : débit soutiré de 10 m³/s (conditions normales)
- Tuyau : L = 100 km, D = 500 mm, ε = 0.012 mm

**Solution analytique :**

$$
P_B = \sqrt{P_A^2 - K \cdot Q \cdot |Q|}
$$

Avec $K = f \cdot L / (2 \cdot D \cdot A^2)$ et $f$ calculé par Swamee-Jain.

**Critère :** Écart solveur vs analytique < 0.1 bar.

---

### Test 2 : Réseau en Y (3 branches)

**Configuration :**
- Nœud S (source) : P = 70 bar
- Nœud J (jonction) : libre
- Nœud A (puits) : Q = -5 m³/s
- Nœud B (puits) : Q = -5 m³/s
- Tuyau S→J : L = 50 km, D = 600 mm
- Tuyau J→A : L = 30 km, D = 400 mm
- Tuyau J→B : L = 40 km, D = 400 mm

**Critère :** Conservation de masse à J (|Q_SJ - Q_JA - Q_JB| < 1e-6).

---

### Test 3 : GasLib-11

**Configuration :** Réseau complet GasLib-11 avec scénario .scn.

**Critères :**
- Convergence en < 100 itérations
- Toutes les pressions ∈ [1, 100] bar
- Conservation de masse globale (|ΣQ_sources + ΣQ_puits| < 1e-4)

---

## Comparaison avec la littérature

Les résultats GasLib sont documentés dans :
> Schmidt, M. et al. (2017). "GasLib — A Library of Gas Network Instances." *Data*, 2(4), 40.

Des solutions de référence seront comparées lorsque disponibles.
