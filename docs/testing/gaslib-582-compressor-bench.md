# GasLib-582 — bench compresseur (Phase I, juin 2026)

Protocole figé : `compressor_diag`, réseau baseline connecté, CDF off, scénario `nomination_mild_618.scn`, slack pression retiré des demandes, preset `robust` (release).

## Synthèse (état au commit `3e05662`, v17)

| Indicateur | Valeur |
|------------|--------|
| Résidu measurement (mild_618) | **~2,0 m³/s** |
| Tolérance preset robust | **3×10⁻³ m³/s** |
| Convergence stricte | Non (partial accept) |
| Pire nœud libre | `sink_24` (−2 m³/s, Q imposé) |
| Objectif Phase I | 3×10⁻³ m³/s |

Progression du plancher :

```
8,2 → 5,0 (v4) → 3,0 (v13) → 2,0 (v14–v17)
```

Leviers épuisés pour le plancher 2 m³/s : ancrages pression (v13–v16), couplage carte in-Newton (v17). Goulot restant : boundaries avec **Q imposé** (`sink_24`, `source_20`, …) et modèle MVP P² compresseur.

Référence architecture : [gaslib-582-compressor-diagnosis.md](./gaslib-582-compressor-diagnosis.md).

## Commandes

```bash
cd back && cargo build --release --bin compressor_diag

# Baseline legacy (cap r² actif)
GAZFLOW_COMPRESSOR_MAP_MODE=legacy ./target/release/compressor_diag GasLib-582 --json /tmp/582-legacy.json

# Mode measurement (défaut Phase I)
GAZFLOW_COMPRESSOR_MAP_MODE=measurement ./target/release/compressor_diag GasLib-582 --json /tmp/582-measurement.json

# Diagnostic H2 (sans cap r²)
./target/release/compressor_diag GasLib-582 --no-r2-cap --json /tmp/582-no-r2-cap.json

# Biquadratique GasLib (coeffs n_isoline)
GAZFLOW_COMPRESSOR_MAP_MODE=biquadratic ./target/release/compressor_diag GasLib-582 --json /tmp/582-biquad.json
```

Durée typique : ~60–90 s/run (582, release).

## Variables d'environnement (Phase I)

| Variable | Rôle | Défaut measurement |
|----------|------|---------------------|
| `GAZFLOW_COMPRESSOR_MAP_MODE` | `legacy` \| `measurement` \| `biquadratic` | — |
| `GAZFLOW_NEWTON_COMPRESSOR_MAP` | Carte tête/vitesse recouplée à chaque itération Newton | `1` |
| `GAZFLOW_COMPRESSOR_R2_CAP_UNTIL_CONVERGED` | Plafond r²≤9 jusqu'à convergence | `1` |
| `GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES` | Passes post-solve d'ancrage massique | `4` |
| `GAZFLOW_DISABLE_R2_CAP` | Désactive atténuation r² transport (H2) | off |
| `GAZFLOW_COMPRESSOR_OUTER_MAX_ITERS` | Itérations boucle externe ratio | 12 |
| `GAZFLOW_COMPRESSOR_RELAX` | Relaxation ω mise à jour ratio | 0.5 |

Voir aussi [README testing](./README.md#gasflow_-flags-compressor--large-transport).

## Champs JSON `compressor_diag`

| Champ | Description |
|-------|-------------|
| `residual` | Résidu Newton final (max \|f_node\| nœuds libres) |
| `demand_scale` | Palier continuation atteint (1,0 = nominal) |
| `continuation_scales` | Historique des paliers continuation |
| `mass_balance` | Bilan massique post-solve (`worst_free_node`, `top_free_imbalances`) |
| `mass_balance_refinement_passes` | Passes d'ancrage massique (v16+) |
| `mass_balance_anchors` | Innodes ancrés dynamiquement (v16+) |
| `compressor_stations[]` | `flow_m3s`, `ratio_max`, `effective_r2`, `map_target_ratio`, `map_eval_q_m3s` |
| `flags` | `map_mode`, `disable_r2_cap`, `catalog_stations`, `preset` |

Artefact de référence v17 : `/tmp/582-v17.json`.

## Progression chronologique (résidu mild_618)

| v | Résidu | Levier principal |
|---|--------|------------------|
| v1 | 5,0 / 8,2 | cap r² MVP ; diag sans outer loop |
| v2–v3 | 5,0 / 8,2 | outer loop continuation ; recherche 1D vitesse |
| v4 | **5,0** | garde outer loop + r² hybride ; fin régression 8,22 |
| v6 | 8,22 | Option 1 : ratio `.cs` vs cap `.net` séparés |
| v7–v8 | 8,22 | recouplage Q estimé ; fix confId turbo |
| v9–v10 | **5,0** | handoff carte ; split hub naïf |
| v11 | 5,0 | débit carte topologique hub/branche (CS1=90, CS2/3=45) |
| v12 | 5,0 | distribution sud locale ; Newton P² adaptatif |
| **v13** | **3,0** | `mass_balance_report` + balance hubs (`sink_2`, `sink_96`) |
| **v14** | **2,0** | boundaries Q≈0 entries+exits ; junctions mixtes |
| v15 | 2,0 | spine `source_17` ; junctions degré 3 ; exclusion CS endpoints |
| v16 | 2,0 | raffinement massique itératif + exit-hub (`innode_315`) |
| **v17** | **2,0** | carte compresseur in-Newton (tête/vitesse → ratio P²) |

## Détail par version

### v17 — carte compresseur in-Newton

| Mode | Résidu |
|------|--------|
| measurement, `GAZFLOW_NEWTON_COMPRESSOR_MAP=1` (défaut) | 2,0 m³/s |
| measurement, `GAZFLOW_NEWTON_COMPRESSOR_MAP=0` | 2,0 m³/s |

`NewtonMapContext` dans `newton.rs` : à chaque évaluation pipe compresseur, bootstrap Q → `find_operating_point_for_mode` → `had_to_pressure_ratio` → coefficient P² semi-implicite (gelé par itération Jacobian).

### v16 — raffinement massique itératif

Boucle post-solve : `solve_with_mass_balance_refinement` ajoute au plus un ancrage `innode_*` par passe si le résidu baisse (pression résolue, gate d'amélioration). Résultat mild_618 : **1 passe**, ancrage `innode_420`, plancher inchangé.

### v15 — spine boundary + junctions degré 3

Ancrages statiques : 2 balance hubs + 1 spine (`source_17`) + 2 junctions mixtes + 1 exit-hub (`innode_315`). Sur-ancrage (3+ junctions) dégrade à 3,6 m³/s.

### v14 — junction anchors mixtes

Entries et exits Q≈0 ; junctions entry+exit (`innode_381`). Résidu 3 → 2 m³/s.

### v13 — diagnostic massique + balance hubs

Cause racine plancher 5 m³/s : `sink_2` portait −5 m³/s (hub junction sans DOF pression). Top 2 hubs par degré topologique. Résidu 5 → 3 m³/s.

### v12 — distribution sud + couplage pression/ratio

eval_q CS4/CS5 corrigé (~10,4 m³/s vs 45). Résidu plancher 5 m³/s inchangé.

### v11 — débit carte topologique

BFS hub/branche dans `flow_topology.rs` : CS1 ≈ 90 m³/s, CS2/CS3 ≈ 45 m³/s. Ratios carte ~1,46–1,50.

### v4 — turbo/biquadratique + garde outer loop

Fin de la régression measurement 8,22 → retour à 5,0 m³/s baseline.

### v1–v3 — fondations

Cap r²≤9 stabilise Newton (5 vs 8,2) mais empêche ratio nominal `.net`. Outer loop seul ne compense pas.

## Interprétation globale

1. **H2 (cap MVP dominant)** : non confirmée comme cause unique ; le cap aide la stabilité (5 vs 8,2) mais limite le lift transport.
2. **Option 1 ratio** : `compressor_ratio_max` ← `.cs` (~1,08), `compressor_pressure_cap_ratio` ← `.net` (4,09). Le 4,09 n'est pas un DOF d'exploitation.
3. **Ancrages pression (v13–v16)** : corrigent les junctions sous-déterminées Q≈0 ; gain 5 → 2 m³/s ; sur-ancrage dégrade.
4. **Couplage Q–ratio (outer + in-Newton)** : ratios carte cohérents (~1,46 transport) ; plancher 2 m³/s persiste car partial accept + boundaries Q imposées.
5. **Prochain levier (v18+)** : assouplissement P/Q contractuel GasLib ou modèle compresseur au-delà du MVP P².

## Test intégration

`test_solve_gaslib_582` (`.scn` défaut, tol smoke 0,3, ~14 min) : OK avec résidu ~8,59 m³/s au nominal (non strict). Activer via `GAZFLOW_ENABLE_LARGE_DATASET_TESTS=1`.
