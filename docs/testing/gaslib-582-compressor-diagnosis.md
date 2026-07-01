# GasLib-582 — diagnostic compresseur (mild_618, juin–juillet 2026)

Document de référence architecture et décisions. Bench chiffré : [gaslib-582-compressor-bench.md](./gaslib-582-compressor-bench.md).

## Sémantique GasLib (mild_618)

| Condition scénario | Métier GasLib | MVP GazFlow solveur |
|--------------------|---------------|---------------------|
| Entry/exit à Q nominé | Égalité sur Q ; enveloppe P (min/max) = **inégalité**, non imposée au Newton | Q en égalité ; P libre (bornes `.net` vérifiées a posteriori dans `pressure_violations`) |
| Slack pression (`sink_109`) | P référence fixe ; Q **inconnue** | P fixe ; Q nominal **retiré** (`effective_solver_demands`) |
| Ancrages `innode_*` / hubs | Pas une condition GasLib standard | Fermeture numérique DOF (refinement bench) |
| v18 `contract_flow_relaxed` | **Violation** nomination Q | Retrait Q sur pires boundaries (opt-in bench) |

Le champ JSON `boundary_nomination_slips` liste les écarts débit sur `source_*` / `sink_*` à Q≠0 (hors slack et boundaries assouplies) : utile pour quantifier la fidélité nomination au partial accept.

## État actuel (juillet 2026)

| | |
|--|--|
| Résidu effectif (nomination intacte, v17) | **2,045 m³/s** |
| Résidu avec abandon Q v18 (run historique) | ~2,000 m³/s |
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
Refinement post-solve (v16–v18): ancrages innode + abandon Q opt-in (bench)
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
| v20 | 2,159 | cap in-Newton assoupli (opt-in, pas de gain) |
| v21 | **2,045** | fermeture H_map ↔ H_req (opt-in, = baseline) |
| v22 | **2,045** | équation H explicite + T_out aval (opt-in, = baseline) |

## Abandon nomination Q (v18) — limites scientifiques

`try_relax_contract_boundary` retire le Q nominatif sur les pires `source_*`/`sink_*` (seuil 1,5 m³/s, max 3/passe). **Désactivé par défaut** (`GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT=0`) ; activer uniquement pour expériences bench. Ce n'est **pas** un assouplissement P/Q contractuel GasLib : les enveloppes pression du `.scn` ne sont de toute façon pas imposées au solveur.

**Limites** :

1. **Violation nomination** : les débits mild_618 ne sont plus imposés sur ces nœuds → toujours lire `nomination_mass_balance` dans le JSON diag.
2. **Seuil 1,5 m³/s** : proche du plancher partial accept (~2) — risque de confondre symptôme et levier.
3. **Gate d'amélioration** (juillet 2026) : revert si la passe n'améliore pas le résidu (comme les ancrages `innode_*`). Les runs « 2,000 » antérieurs utilisaient une variante qui conservait les relaxations sans gain.

`GAZFLOW_RELAX_DUAL_PRESSURE_CONTRACTS=1` (29 nœuds) : dégrade (~2,11 m³/s) — rejeté.

## Couplage in-Newton (v17) et Jacobian (v19)

**v17** (`GAZFLOW_NEWTON_COMPRESSOR_MAP=1`) : bootstrap Q → `find_operating_point_for_mode` → `had_to_pressure_ratio` → coeff P², semi-implicite (coeff gelé par évaluation Jacobian). Résultat mild_618 : **identique** à outer loop seul (2,045 m³/s).

**v19** (`GAZFLOW_NEWTON_COMPRESSOR_HEAD_JAC=1`) : dérivées numériques ∂coeff/∂Q et ∂coeff/∂P_in avec correction Picard sur ∂dp/∂π. **Pas un modèle enthalpique** ; défaut off (régression légère ON). Pas de gain au plancher.

**v20** (`GAZFLOW_COMPRESSOR_ENTHALPIC=1`) : recouplage carte in-Newton avec cap achieved-ratio configurable (`GAZFLOW_COMPRESSOR_ENTHALPIC_OVERSHOOT`, défaut 1,08) et dérivées tête implicites. `head_required_from_pressures` disponible pour v21. Bench unique : **2,159 m³/s** (légère régression vs 2,045 ; retirer entièrement le cap → ~3 m³/s). Opt-in, défaut off.

**v21** (`GAZFLOW_COMPRESSOR_ENERGY_CLOSURE=1`) : fermeture explicite `H_eff = (H_map + H_req)/2` avec `H_req(P_in,P_out)` et dérivées ∂coeff/∂P_aval. Bench unique : **2,045 m³/s** (= baseline v17, pas de gain). Opt-in, défaut off.

**v22** (`GAZFLOW_COMPRESSOR_ENERGY_EQUATION=1`) : pénalité explicite `H_map − H_req` dans Δ(P²) + T_sortie isentrope aval. Bench unique : **2,045 m³/s** (= baseline). Opt-in, défaut off.

## Phase I-bis — enveloppes pression (juillet 2026)

| Bench | Résidu | Violations P | Notes |
|-------|--------|--------------|-------|
| partial accept + enveloppes post-check | **2,159** | **11** | hubs standard, nomination intacte |
| in-Newton soft (w=0,01) | **2,045** | **11** | = baseline massique, enveloppes visibles |
| strict Newton | **échec @ 3,0** | n/a | partial accept off → pas de faux « ok » à 2,045 |

### Analyse des 11 violations (mild_618, in-newton w=0,01)

**10/11** : `sink_*` **nommés** (Q imposé, slip \|Q\| ≪ 2 m³/s) dont la pression résolue est **sous la borne basse scénario** :

| Nœud | P résolu | lower scénario | Écart | Q nominal (m³/s) |
|------|----------|----------------|-------|------------------|
| `sink_122` | 4,2 bar | **74,0 bar** | −69,8 bar | −10,4 |
| `sink_125` | 10,7 bar | 41,0 bar | −30,3 bar | −8,0 |
| `sink_88` | 2,5 bar | 26,0 bar | −23,5 bar | −7,2 |
| `sink_83` | 2,0 bar | 21,0 bar | −19,0 bar | −4,0 |
| `sink_42`, `47`, `55` | ~4 bar | 5,5–7 bar | ~1–3 bar | faible |

**1/11** : `innode_3` (interne, borne `.net` 61,9 bar, hors enveloppe `.scn`).

**Interprétation métier** : ce n'est pas un artefact « sinks Q≈0 à 51 bar ». Le solveur trouve un état **basse pression locale** (~2–11 bar) sur des sorties contractuelles qui exigent des planchers **5–74 bar**, tout en imposant Q. Cas extrême : `sink_122` (branche `innode_49` → `shortPipe` → `source_10`) à 4 bar vs contrat 74–81 bar.

Le JSON diag expose désormais `scenario_pressure_slips` (tri par shortfall, flag `from_scenario_envelope`).

**Conclusion Phase I-bis** : le plancher **~2 m³/s** coexiste avec **violation systématique des enveloppes P** sur les contrats dual Q+P. Ce n'est pas résolu par post-check seul ni par pénalité soft faible (w=0,01).

## Prochaines étapes (Phase I-c)

1. **Contrat dual Q+P** : modèle frontière (P dans enveloppe + Q imposé) vs ancrage P sur lower bound (changement de problème).
2. **Prioriser `sink_122`** : topologie branche haute pression / shortPipe bidirectionnel.
3. **Strict Newton + diag** : `scenario_pressure_slips` sur dernier état partiel (si solve échoue).
4. Bench : `./scripts/bench-gaslib-582.sh phase-ibis-in-newton` (w=0,01 défaut).

```bash
# Enveloppes + critères honnêtes
./scripts/bench-gaslib-582.sh phase-ibis-nominal-anchors
./scripts/bench-gaslib-582.sh strict-newton-envelopes
```

Objectif Phase I : convergence nomination intacte vers **3×10⁻³ m³/s** sur mild_618 (non atteint).
