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

---

## Rapport protocole scientifique v1 (intermédiaire)

- Date: 2026-03-09
- Portée: backend Rust (`back/`) sur branche locale de travail
- Commandes exécutées: suite T1..T8 + T10 (T9 dépend d'un fichier `.sol` non présent localement)

### Statut T1..T10

| ID | Test | Statut | Note |
|---|---|---|---|
| T1 | Friction Darcy en turbulent | ✅ Pass | `darcy_friction_turbulent` OK |
| T2 | Résistance de tuyau positive/finie | ✅ Pass | `pipe_resistance_positive` OK |
| T3 | Cas analytique 2 nœuds | ✅ Pass | `steady_state_two_nodes` OK |
| T4 | Réseau en Y: conservation locale | ✅ Pass | `steady_state_y_network_mass_conservation` OK |
| T5 | Hybride vs Jacobi | ✅ Pass | `test_newton_vs_jacobi_same_result` OK |
| T6 | Sanity check GasLib-11 | ✅ Pass | `test_solve_gaslib_11` OK (si données présentes) |
| T7 | Conversion unités scénario -> SI | ✅ Pass | `test_units_scn_to_si` OK |
| T8 | Cohérence dimensionnelle chute de pression | ✅ Pass | `test_pressure_drop_dimension_consistency` OK |
| T9 | Validation vs référence `.sol` | ⏸️ Bloqué données | test ajouté (`test_gaslib_11_vs_reference_solution`) avec skip propre si `.sol` absent |
| T10 | Sensibilité physique (rugosité, Z, T) | ✅ Pass | `test_sensitivity_physical_trends` OK |

### Métriques T9 (référence `.sol`)

- Max erreur pression: N/A (fichier `.sol` absent localement)
- Erreur moyenne: N/A
- Nœud le plus en écart: N/A

### Décision Go/No-Go

- **No-Go strict sortie Phase 2 complète** tant que T9 ne peut pas être exécuté.
- **Go technique conditionnel MVP** sur la robustesse interne (T1-T8 + T10 passants), en attente de jeu de référence `.sol` pour qualification scientifique complète.
