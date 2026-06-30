# GasLib-582 — diagnostic compresseur (mild_618, juin 2026)

Document de référence architecture et décisions. Bench chiffré : [gaslib-582-compressor-bench.md](./gaslib-582-compressor-bench.md).

## État actuel (v17)

| | |
|--|--|
| Résidu measurement | **~2,0 m³/s** |
| Tolérance cible | 3×10⁻³ m³/s |
| Pire nœud | `sink_24` (exit, Q=17,9 m³/s imposé) |
| Statut solve | partial accept (`accept_partial_solution` en outer loop) |

## Décision structurante : Option 1

**Ratio d'exploitation** = catalogue `.cs` (carte / étages), **plafonné** par les bornes pression `.net`.

```text
compressor_ratio_max          ← .cs (~1,08 par étage)
compressor_pressure_cap_ratio ← .net (4,09 transport, 2,10 sud)
effective_ratio = clamp(map(Q, p_in), operating, cap)
```

Options écartées :

- **Option 2** (lift chaîne) : CS1–3 ne sont pas en série mais en branches parallèles vers `innode_14`.
- **Option 3** (abandon 582) : reportée.

Le ratio **4,09 n'est pas un DOF d'exploitation** : borne d'équipement. L'imposer comme `compressor_ratio_max` créait un faux équilibre (résidu artificiel ~5 m³/s avec cap r²).

### Lift carte vs plafond `.net`

| | Transport CS1–3 | Sud CS4–5 |
|--|-----------------|-----------|
| Plafond `.net` (p_out/p_in) | **4,09** | **2,10** |
| Ratio carte à Q nominal | **~1,46** (v11) | ~1,31–1,46 |
| Lift carte suffisant pour 4,09 ? | **non** | non |

## Topologie transport (mild_618)

- Livraison hors slack `sink_109` : **90,13 m³/s** norm.
- CS2 et CS3 → hub `innode_14` (parallèle) ; CS1 lift final `innode_14` → `innode_389`.
- Débit carte (v11, `flow_topology.rs`) : CS1 **90 m³/s**, CS2/CS3 **45 m³/s** chacun ; CS4/CS5 ~10,4 m³/s (zone distribution locale, v12).

## Architecture solveur compresseur

```text
Continuation (demand ramp 0→1)
    ↓
Newton-hybrid (MVP P² + gravité + régulateurs)
    ↓  [v17: recouplage carte in-Newton si measurement/biquadratic]
Outer loop compresseur (apply_compressor_map_updates)
    ↓
Partial accept si residual > tol
```

Newton : variable π = P² ; compresseur = coefficient multiplicatif amont (`pressure_from_coeff` = ratio²). Pas de tête/vitesse explicite in-Newton sauf v17 (coefficient dynamique semi-implicite).

Outer loop : `guarded_compressor_ratio_step`, relaxation ω, `find_operating_point_for_mode`, débit estimé topologique si Q solver ≈ 0.

## Ancrages pression scénario (v13–v16)

Enrichissement via `enrich_scenario_with_balance_hub` + raffinement massique optionnel.

| Type | Détection | Exemple mild_618 | Max |
|------|-----------|------------------|-----|
| `pressure_slack` | exit gros Q + borne basse seule | `sink_109` | 1 |
| `balance_hubs` | boundaries Q≈0, degré topologique | `sink_2`, `sink_96` | 2 |
| `boundary_spine_anchors` | source/sink Q≈0, degré ≥4, mix voisins | `source_17` | 1 |
| `junction_anchors` | `innode_*`, mix entry+exit Q≈0 ou exit-hub | `innode_381`, `innode_315` | 2+1 |
| `mass_balance_anchors` | pire `innode_*` post-solve si Δresidual > 0 | `innode_420` | 4 passes |

Règles :

- Pression ancrée = borne basse scénario ou `.net` (2,01 bar typique).
- Exclure extrémités compresseur (`innode_402`, …).
- Sur-ancrage (>2–3 junctions) **dégrade** le résidu (3,6 m³/s observé).
- Raffinement massique : revert si pas d'amélioration (gate v16).

Fichiers : `back/src/gaslib/scenario.rs`, `back/src/solver/steady_state.rs` (`solve_with_mass_balance_refinement`).

## Diagnostic massique (v13+)

`mass_balance_report` dans JSON diag : pour chaque nœud libre, imbalance = demand + Σ flux.

Finding v13 : `sink_2` portait −5 m³/s (hub degré 8, Q=0 scénario, pression libre) → ancrage → résidu 5 → 3 m³/s.

Finding v14–v17 : cluster `sink_24` / innodes ±2 m³/s ; nombreux nœuds à exactement ±2 m³/s (partial accept, tolérance preset non atteinte).

## Couplage in-Newton (v17)

`GAZFLOW_NEWTON_COMPRESSOR_MAP=1` (défaut en measurement/biquadratic) :

1. Bootstrap Q depuis coefficient nominal courant.
2. `find_operating_point_for_mode` → (n, H_ad, Q).
3. `had_to_pressure_ratio` → ratio, plafonné r² si transport.
4. `effective_compressor_pressure_from_coeff` adoucit si ratio > pression atteinte.

Résultat mild_618 : **identique** à outer loop seul (2,0 m³/s) — les ratios outer loop pré-calibrent déjà le coefficient avant convergence partielle.

## Historique résidu (synthèse)

| Étape | Résidu | Cause / levier |
|-------|--------|----------------|
| v1 baseline | 5,0 | cap r² + ratio incohérent |
| v6 Option 1 | 8,22 | sémantique ratio corrigée |
| v4 | 5,0 | garde outer loop |
| v13 | 3,0 | hub `sink_2` |
| v14 | 2,0 | junction `innode_381` |
| v15–v17 | 2,0 | spine, mass refine, in-Newton map |

## Prochaines étapes (v18+)

1. **Assouplissement P/Q contractuel** : entries/exits GasLib avec P_min + Q (ex. `source_20`, `sink_24`) — retirer Q ou fixer P depuis scénario, pas les deux.
2. **Modèle compresseur complet** : tête adiabatique + rendement in-Newton avec Jacobian couplé (au-delà MVP ratio P²).
3. **Convergence stricte** : investiguer partial accept à 2 m³/s (2400 iter preset robust) vs relâcher tolérance smoke pour bench.

Objectif Phase I : **2 → 3×10⁻³ m³/s** sur mild_618.
