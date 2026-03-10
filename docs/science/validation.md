# Validation du solveur — GazFlow

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
- Commandes exécutées: suite T1..T10 (T9 via référence interne versionnée)

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
| T9 | Validation vs référence `.sol` | ✅ interne / ⏸️ externe | référence interne versionnée OK; référence officielle externe absente |
| T10 | Sensibilité physique (rugosité, Z, T) | ✅ Pass | `test_sensitivity_physical_trends` OK |

### Métriques T9 (référence `.sol`)

- Max erreur pression (référence interne): 0.000%
- Erreur moyenne (référence interne): 0.000%
- Nœud le plus en écart (référence interne): `entry01`
- Note exécution: le test accepte désormais une source de référence configurable via
  `GAZFLOW_REFERENCE_SOLUTION_PATH` (formats texte CSV-like ou XML), en plus de
  `dat/GasLib-11.sol`.

### Décision Go/No-Go

- **No-Go strict sortie Phase 2 complète** tant qu'une référence officielle externe n'est pas disponible pour T9.
- **Go technique conditionnel MVP** sur la robustesse interne (T1-T10 avec référence interne verrouillée).

---

## Mise à jour (backend) — 2026-03-10

- Intégration d'un modèle compresseur MVP avec **uplift directionnel sur \(P^2\)**:
  - parsing de `*.cs` pour estimer un ratio max par station (`nrOfSerialStages`);
  - injection d'un coefficient de compression sur la pression amont dans les équations de flux;
  - Jacobien Newtown/Jacobi ajusté avec pondération asymétrique amont/aval.
- Campagne smoke datasets forcée:
  - commande: `GAZFLOW_ENABLE_LARGE_DATASET_TESTS=1 cargo test test_solve_gaslib_ -- --nocapture`;
  - `GasLib-24` / `GasLib-40`: OK;
  - `GasLib-582`: exécution robuste, non-convergence explicite acceptée en mode smoke (résidu final observé: `5.000e0`);
  - `GasLib-4197`: exécution robuste, non-convergence explicite acceptée en mode smoke (profil très court, continuation + warm-start).
- Exploration complémentaire (continuation plus profonde, run interrompu après premier palier):
  - config: `GAZFLOW_LARGE_TEST_MAX_ITER=60`, `GAZFLOW_LARGE_TEST_SCALES=0.1,0.03,0.01`;
  - premier palier `0.1`: résidu `9.626e5` (amélioration vs profil smoke court), convergence non atteinte.
- Ajustements anti-run long:
  - profils smoke raccourcis (`4197`: `max_iter=6` avec `scales=0.05,0.1,0.1` et répartition `1,1,4`, `582`: `max_iter=180`);
  - timeout global smoke (`GAZFLOW_LARGE_TEST_MAX_SECONDS`) + timeout continuation (`GAZFLOW_CONTINUATION_MAX_SECONDS`);
  - snapshot warm-start en continuation (`GAZFLOW_CONTINUATION_SNAPSHOT_EVERY`);
  - initialisation physique courte avant Newton pour très grands réseaux (activée par défaut au-delà de `2000` nœuds, rejetée automatiquement si elle n'améliore pas le résidu initial);
  - cap GMRES par défaut réduit sur grands systèmes libres (`220` itérations pour `m > 1200`).
- Mesures récentes:
  - `GasLib-4197` smoke par défaut: ~14-15s sur runs récents (résidu observé ~`2.83e5` avec paliers `0.05 -> 0.1 -> 0.1`, budget itérations `1,1,4`);
  - `GasLib-582` smoke par défaut: ~25-33s selon run;
  - les deux restent robustes (non-convergence explicite acceptée en mode smoke).
- Note objectif perf court-terme:
  - cible exploratoire `<5e5` sous `~15s` atteinte sur profil smoke par défaut actuel;
  - meilleure configuration stable observée: résidu `~2.83e5` en `~14.9s` sur `GasLib-4197`.
- Tentatives complémentaires (rollback):
  - clamp dur des mises a jour de pression sur bornes nodales (`pressure_lower/upper`) teste puis retire;
  - effet observe sur `GasLib-4197`: degradation forte du residu (jusqu'a `~1.43e7`) et aucune convergence supplementaire utile;
  - initialisation "70 bar bornee par noeud" testee puis retiree;
  - effet observe sur `GasLib-4197`: runtime degrade (`~24s`) avec residu degrade (`~3.58e6`);
  - baseline conservee puis amelioree: continuation `0.05 -> 0.1 -> 0.1` + budget `1,1,4` + init physique courte + cap GMRES.
- État global inchangé sur la qualification scientifique:
  - T9 reste bloqué sans solution de référence fournie;
  - décision finale Go/No-Go scientifique complète toujours en attente de la référence.

---

## Rapport v1 final (conditionnel) — 2026-03-10

### Référence interne verrouillée (régression)

- Fichier: `docs/testing/references/GasLib-11.reference.internal.csv`
- Génération: `cargo run --bin generate_gaslib11_reference` (depuis `back/`)
- Exécution de contrôle: `cargo test test_gaslib_11_vs_reference_solution -- --nocapture`
- Résultat observé:
  - `n=11` nœuds comparés
  - `max_err=0.000%`
  - `mean_err=0.000%`
  - `worst_node=entry01`

### Interprétation

- Cette référence interne est utile comme **garde-fou de non-régression**.
- Elle ne remplace pas une référence indépendante externe (`.sol`) pour une validation scientifique stricte.

### Décision Go/No-Go (version finale conditionnelle)

- **Go (engineering / CI):** oui, la validation T1..T10 est exécutable en continu avec référence interne verrouillée.
- **No-Go (scientifique strict):** maintenu tant qu'une référence officielle indépendante GasLib-11 n'est pas disponible.

### Industrialisation de l'exécution

- Script pack: `scripts/validation-pack.sh`
- Exécution observée: T1..T10 passants de bout en bout (backend).
- Options:
  - `GAZFLOW_REGEN_REFERENCE=1` pour régénérer la référence interne avant T9;
  - `GAZFLOW_RUN_LARGE_SMOKE=1` pour inclure les smoke tests grands datasets.
