# GasLib-582 — diagnostic compresseur (mild_618, juin 2026)

Document de référence architecture et décisions. Bench chiffré : [gaslib-582-compressor-bench.md](./gaslib-582-compressor-bench.md).

## État actuel (v19)

| | |
|--|--|
| Résidu measurement | **~2,0 m³/s** |
| Tolérance cible | 3×10⁻³ m³/s |
| Pire nœud | cluster ±2 m³/s (`innode_402`, junctions) |
| Statut solve | partial accept (sauf `GAZFLOW_COMPRESSOR_STRICT_NEWTON=1`) |

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
| v15–v17 | 2,045 | spine, mass refine, in-Newton map |
| v18 | **~2,000** | assouplissement Q contractuel (4 boundaries) |
| v19 | ~2,0 | head-Jacobian opt-in (pas de gain ; ON légèrement pire) |

## Assouplissement contractuel (v18)

`effective_solver_demands` retire le Q imposé pour :

- slack pression (`sink_109`, déjà v13+)
- boundaries listées dans `contract_flow_relaxed` (ajoutées itérativement par `try_relax_contract_boundary`)

Heuristique : pires `source_*`/`sink_*` avec |imbalance| ≥ 1,5 m³/s, max 3/passe, jusqu'à `GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES` (défaut 4). Les assouplissements contractuels sont conservés même si le résidu ne baisse pas immédiatement ; les ancrages `innode_*` sont revertés si pas d'amélioration.

Option expérimentale : `GAZFLOW_RELAX_DUAL_PRESSURE_CONTRACTS=1` retire le Q de toutes les entries/exits à enveloppe pression lower+upper (29 nœuds mild_618) — **dégrade** le résidu (~2,11 m³/s).

Env : `GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT=0` désactive v18 ; `GAZFLOW_CONTRACT_FIX_PRESSURE=1` fixe P à la pression résolue lors de l'assouplissement (défaut : Q seul).

## Couplage Jacobian tête (v19)

`GAZFLOW_NEWTON_COMPRESSOR_HEAD_JAC=1` active `pipe_flow_derivatives_head_jac` : résidu compresseur avec coefficient carte `c(Q, P_in)` et dérivées implicites (Picard sur ∂c/∂Q). Défaut **off** : bench mild_618 montre 2,045 m³/s avec ON vs ~2,0 avec OFF.

`GAZFLOW_COMPRESSOR_STRICT_NEWTON=1` : outer loop sans partial accept (Newton doit converger à tol ou échouer).

## Prochaines étapes (v20+)

1. **Modèle compresseur enthalpique** : bilan énergétique nodal ou défaut enthalpique in-Newton (au-delà du Jacobian ratio P²).
2. **Convergence stricte** : outer loop sans partial accept + plus d'itérations ; investiguer plancher ~2 m³/s comme attracteur numérique.

Objectif Phase I : **2 → 3×10⁻³ m³/s** sur mild_618.
