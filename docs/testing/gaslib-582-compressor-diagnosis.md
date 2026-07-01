# GasLib-582 — diagnostic compresseur (mild_618, juin–juillet 2026)

Document de référence architecture et décisions. Bench chiffré : [gaslib-582-compressor-bench.md](./gaslib-582-compressor-bench.md).

## État actuel (juillet 2026)

| | |
|--|--|
| Résidu effectif (nomination intacte, v17) | **2,045 m³/s** |
| Résidu avec assouplissement Q (v18, run historique) | ~2,000 m³/s |
| Tolérance cible preset robust | 3×10⁻³ m³/s |
| Pire nœud (nomination intacte) | `sink_24` (Q −4,96 m³/s imposé, imbalance ≈ −résidu) |
| Statut solve | partial accept (~2 m³/s, pas convergence stricte) |

### Ce que signifie « ~2 m³/s »

Le résidu Newton rapporté est le **maximum des déséquilibres massiques nodaux** sur les nœuds à pression libre. Au partial accept, une quinzaine de nœuds affichent \|imbalance\| ≈ 2 m³/s : ce n'est pas une tolérance codée en dur, mais un **plateau de non-convergence** du MVP P² + outer loop carte.

## Décision structurante : Option 1

**Ratio d'exploitation** = catalogue `.cs` (carte / étages), **plafonné** par les bornes pression `.net`.

```text
compressor_ratio_max          ← .cs (~1,08 par étage)
compressor_pressure_cap_ratio ← .net (4,09 transport, 2,10 sud)
effective_ratio = clamp(map(Q, p_in), operating, cap)
```

Le ratio **4,09 n'est pas un DOF d'exploitation** : borne d'équipement.

## Architecture solveur

```text
Continuation (demand ramp 0→1)
    ↓
Newton-hybrid (MVP P² + gravité + régulateurs)
    ↓  [v17: coeff P² depuis carte, gelé par éval Jacobian]
    ↓  [v19 opt-in: ∂coeff/∂Q, ∂coeff/∂P_in — toujours P², pas enthalpie]
Outer loop compresseur (apply_compressor_map_updates)
    ↓
Partial accept si residual > tol  [désactivable: GAZFLOW_COMPRESSOR_STRICT_NEWTON=1]
    ↓
Refinement post-solve (v16–v18): ancrages innode + assouplissement Q
```

**Important** : v17/v19 ne couplent pas un bilan enthalpique nodal. La carte fournit une tête adiabatique convertie en ratio isentrope puis en coefficient P².

## Ancrages pression scénario (v13–v16)

| Type | Exemple mild_618 |
|------|------------------|
| `pressure_slack` | `sink_109` (Q retiré des demandes) |
| `balance_hubs` | `sink_2`, `sink_96` |
| `boundary_spine_anchors` | `source_17` |
| `junction_anchors` | `innode_381`, `innode_315` |
| `mass_balance_anchors` | `innode_420` |

Sur-ancrage (>2–3 junctions) dégrade le résidu (~3,6 m³/s observé).

## Historique résidu (nomination intacte sauf v18*)

| Étape | Résidu effectif | Commentaire |
|-------|-----------------|-------------|
| v4 | 5,0 | baseline Option 1 |
| v13 | 3,0 | hubs `sink_2` |
| v14–v17 | **2,045** | junctions ; in-Newton sans gain |
| v18* | ~2,000 | *Q retiré sur 4 boundaries — hors nomination |
| v19 | 2,045 | head-Jac off ; ON = 2,045 (run unique) |

## Assouplissement contractuel (v18) — limites scientifiques

`try_relax_contract_boundary` retire le Q nominatif sur les pires `source_*`/`sink_*` (seuil 1,5 m³/s, max 3/passe). **Désactivé par défaut** (`GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT=0`) ; activer uniquement pour expériences bench.

**Limites** :

1. **Violation nomination** : les débits mild_618 ne sont plus imposés sur ces nœuds → toujours lire `nomination_mass_balance` dans le JSON diag.
2. **Seuil 1,5 m³/s** : proche du plancher partial accept (~2) — risque de confondre symptôme et levier.
3. **Gate d'amélioration** (juillet 2026) : revert si la passe n'améliore pas le résidu (comme les ancrages `innode_*`). Les runs « 2,000 » antérieurs utilisaient une variante qui conservait les relaxations sans gain.

`GAZFLOW_RELAX_DUAL_PRESSURE_CONTRACTS=1` (29 nœuds) : dégrade (~2,11 m³/s) — rejeté.

## Couplage in-Newton (v17) et Jacobian (v19)

**v17** (`GAZFLOW_NEWTON_COMPRESSOR_MAP=1`) : bootstrap Q → `find_operating_point_for_mode` → `had_to_pressure_ratio` → coeff P², semi-implicite (coeff gelé par évaluation Jacobian). Résultat mild_618 : **identique** à outer loop seul (2,045 m³/s).

**v19** (`GAZFLOW_NEWTON_COMPRESSOR_HEAD_JAC=1`) : dérivées numériques ∂coeff/∂Q et ∂coeff/∂P_in avec correction Picard sur ∂dp/∂π. **Pas un modèle enthalpique** ; défaut off (régression légère ON). Pas de gain au plancher.

## Prochaines étapes (v20+)

1. **Modèle compresseur avec bilan énergétique** (hors MVP P²) ou défaut enthalpique explicite in-Newton.
2. **Convergence stricte** (`GAZFLOW_COMPRESSOR_STRICT_NEWTON=1`) + budget iter : qualifier si ~2 m³/s est attracteur physique ou purement numérique.
3. **Bench reproductible** : `./scripts/bench-gaslib-582.sh` (3 runs manuels recommandés, médiane).

Objectif Phase I : convergence nomination intacte vers **3×10⁻³ m³/s** sur mild_618 (non atteint).
