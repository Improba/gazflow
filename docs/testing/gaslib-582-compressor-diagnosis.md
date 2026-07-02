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

## Phase I-c — contrat dual Q+P actif (juillet 2026)

Flag `GAZFLOW_SCENARIO_BOUNDARY_ACTIVE_ENVELOPES=1` :
- active enveloppes P + pénalité in-Newton (défaut **1 m³/s par bar**)
- bloque partial accept si violation P scénario > tolérance
- opt-out partial : `GAZFLOW_SCENARIO_BOUNDARY_PARTIAL_ACCEPT=1`

### Résultats smoke (refinement=0, ~3 min en parallèle)

| Tag | Status | Résidu massique | Violations P scénario |
|-----|--------|-----------------|------------------------|
| `nominal-smoke` | ok (partial) | **2,045** m³/s | 1 (`innode_3`) |
| `phase-ibis-in-newton-smoke` | ok (partial) | **2,11** m³/s | **11** (pire `sink_122` −69,8 bar) |
| `phase-ic-dual-contract-smoke` | **error** | — | **69,8 m³/s** (critère contractuel) |

**Interprétation** : le dual contract ne masque plus l'infaisabilité. Le partial accept à 2,045 m³/s cachait 11 violations P ; le mode actif échoue honnêtement avec un résidu contractuel dominé par `sink_122` (4,2 bar vs 74–81 bar, Q=−10,4 m³/s imposé).

Ce n'est pas un bug numérique : le MVP P² impose Q mais ne couple pas la pression amont aux planchers contractuels. Les ancrages `innode_*` ou floor-anchor ne règlent pas la sémantique GasLib.

### Bench

```bash
./scripts/bench-gaslib-582.sh phase-ic-dual-contract        # ~2,5 min (échec honnête)
./scripts/bench-gaslib-582-parallel.sh 3 nominal-smoke phase-ibis-in-newton-smoke phase-ic-dual-contract-smoke
```

Artefacts : `/tmp/582-phase-ic-dual-contract.json`, `/tmp/582-phase-ic-dual-contract-smoke.json`.

### Phase II — fusion shortPipe + diagnostic alimentation (juillet 2026)

Flag `GAZFLOW_SCENARIO_SHORTPIPE_MERGE_BOUNDARIES=1` (auto avec dual contract) :
- alias pression : `source_*` esclave → `sink_*` maître dans Newton
- Q net sur le maître ; shortPipe retiré du graphe hydraulique
- pipes amont du `source_*` recollés au `sink_*`

JSON diag : `status="contract_violation"` (résolu honnête, exit 1), `boundary_pressure_supply` (déficit amont par violation).

### Résultats Phase II smoke (refinement=0)

| Config | Status | Résidu | sink_122 P | gap amont |
|--------|--------|--------|------------|-----------|
| dual-contract | contract_violation | **69,32** m³/s | 4,69 bar | 69,3 bar |
| enthalpic (`ENTHALPIC=1`) | contract_violation | **69,32** m³/s | 4,69 bar | 69,3 bar |

**Compresseur enthalpic : aucun effet** (résidu identique). Diagnostic `boundary_pressure_supply` :

```
sink_122  need=74.0  max_up=4.69   gap=69.3 bar
innode_3  need=61.9  max_up=9.58   gap=52.3 bar
sink_125  need=41.0  max_up=10.91  gap=30.1 bar
sink_88   need=26.0  max_up=2.48   gap=23.5 bar
sink_83   need=21.0  max_up=4.81   gap=16.2 bar
```

**Cause racine Phase II (modèle, pas numerique)** : quelle que soit la continuation (`1.0`, `0.3,0.6,1.0`, `0.05…1.0`), le solveur converge vers le **même** état basse-pression (sink_122 = 4,69 bar, noyau ~9 bar). Ce n'est pas un effondrement continuation — c'est la solution du modèle P².

- Sources GasLib-582 : `pressureMin=1,01 bar`, `pressureMax=121 bar`, mais le MVP impose **Q seul** aux entries et laisse P libre.
- Une seule référence P : slack `sink_109` à 51 bar.
- Le solveur choisit le P minimal satisfaisant le bilan massique → entries à ~4 bar, exits à 4–11 bar.
- `sink_122` exige 74 bar > slack 51 bar : **physiquement infaisable** sans compresseur sur la branche (aucun compresseur sur innode_49→…→sink_122).
- Compresseurs : `flow_m3s=0` (aucun sur les branches violantes ; enthalpic n'aide pas).

Le dual contract identifie honnêtement cette infeasibilité contractuelle. Ce n'est pas un bug : c'est une limite du modèle MVP Q-seul.

## Phase II — test décisif : ancrage entries en régime transport (juillet 2026)

Flag `GAZFLOW_ENTRY_TRANSPORT_ANCHOR=1` (`GAZFLOW_ENTRY_TRANSPORT_ANCHOR_BAR=70`) : fixe P aux entries nominées (régime transport), libère leur Q. Test scientifique : distingue l'artefact basse-pression (référence exit-slack unique) d'une infeasibilité réelle.

### Résultats (entries ancrées, dual contract actif)

| Config | Résidu | sink_122 | sink_125 | sink_88 | Notes |
|--------|--------|----------|----------|---------|-------|
| dual contract (entries libres) | **69,3** m³/s | 4,7 bar (viol 69) | 10,9 bar | 2,5 bar | entries à 4 bar (non physique) |
| entry-anchor 70 bar | **23,5** m³/s | **résolu** | 34,6 bar | 2,5 bar | −66 % ; régime transport partiel |
| entry-anchor 80 bar | **23,5** m³/s | résolu | 34,6 bar | 2,5 bar | 80 bar n'aide pas sink_88 |
| entry-anchor + enthalpic | **23,5** m³/s | résolu | 34,6 bar | 2,5 bar | enthalpic n'aide pas (comp. off) |
| entry-anchor + refinement=4 | **23,5** m³/s | résolu | 34,6 bar | 2,5 bar | refinement n'aide pas |

**Conclusion scientifique** :
1. L'état basse-pression (entries à 4 bar) était **en partie un artefact** de la référence exit-slack unique. Ancrer les entries à 70 bar résout le flagship `sink_122` (74 bar) via le merge shortPipe et atteint un régime transport partiel.
2. Le résidu chute de 69,3 à **23,5 m³/s** (−66 %), stable across toutes les variantes (80 bar, enthalpic, refinement).
3. Deux catégories de violations résiduelles :
   - **Branches alimentées mais à grosse chute** (`innode_3` max_up=70 bar / nœud 9 bar, `sink_125` max_up=70 / 34,6 bar) : compresseur OFF sur le chemin → activation compresseur requise.
   - **Branches non alimentées** (`sink_88` max_up=2,5 bar, `sink_83` max_up=4,95) : aucune entry haute pression n'atteint ces branches → **infeasibilité réelle** sans nouveau compresseur/infrastructure.

C'est exactement le problème de **validation of nominations** de la littérature GasLib (Pfetsch, Geißler et al.) : la nomination est feasible ssi il existe des réglages d'éléments actifs (compresseurs) satisfaisant les bornes. Le résidu résiduel (23,5 m³/s) est la violation minimale que les compresseurs doivent annuler.

## Prochaines étapes (Phase II suite)

1. **Activation compresseurs sur branches alimentées** : `innode_3`, `sink_125` ont de la pression amont (70 bar) mais grosse chute. Vérifier pourquoi les compresseurs sont OFF (r² cap, map, outer loop) et les activer.
2. **Branches non alimentées** (`sink_88`, `sink_83`) : confirmer l'infeasibilité topologique (aucun compresseur sur le chemin). Résultat négatif valide = la nomination nécessite des investissements.
3. **Modèle validation of nominations** : compresseurs en variables de décision, minimise la violation P (NLP feasibility).

Objectif Phase I : convergence nomination intacte vers **3×10⁻³ m³/s** sur mild_618 (**non atteint** ; cause racine identifiée et quantifiée : 23,5 m³/s de violation P résiduelle nécessitant activation compresseurs / infeasibilité topologique sur sink_88).

## Phase II — correction alias shortPipe + sonde pression (juillet 2026)

### Bug racine : l'alias shortPipe écrasait la pression transport des entries ancrées

Investigation compresseur : les stations `compressorStation_1/2/3` (inlet 9 bar, ratio 1,08) sont **choked** (débit volumique réel ~10 m³/s >> choke line ~2 m³/s de la carte turbo) → ratio dégénéré. La cause n'est pas le compresseur mais l'inlet à 9 bar.

Sonde `GAZFLOW_DIAG_PROBE_NODES` (commit 2886fc1) sur la branche `innode_3` :

| Nœud | fixed_bar | P avant fix | P après fix |
|------|-----------|-------------|-------------|
| `source_22` | 70 | **9,16** | **70,00** |
| `source_26` | 70 | 64,97 | 70,00 |
| `source_27` | 70 | 64,98 | 70,00 |
| `source_28` | 70 | 70,00 (non couplé) | 70,00 |
| `innode_3` | — | 9,16 | **69,97** |
| `innode_14` | — | 9,32 | 71,48 |

**Racine** : `source_22` est couplée par shortPipe à `sink_114` (`detect_shortpipe_boundary_pairs`). Le merging shortPipe traitait la source comme **esclave** du sink ; `sync_pressures` copiait alors la pression basse du sink (~9 bar) vers la source, **écrasant** son `pressure_fixed_bar=70`. `source_28` (non couplée) restait à 70 — d'où le régime transport partiel.

**Fix** (`back/src/solver/newton.rs`, `ShortPipeAliasContext::from_network`) : on saute les paires dont la source a `pressure_fixed_bar.is_some()`. La source ancrée reste maître à pression fixée ; le shortPipe haute conductance propage 70 bar au sink et à l'innode aval. Après fix, `innode_3` passe de 9,16 à **69,97 bar** — le flagship `innode_3` (shortfall 52,8 bar) est **résolu**.

### Résidu résiduel = infeasibilité topologique réelle (sink_88, sink_83)

Après la correction, le résidu 23,5 m³/s est **dominé à 100 %** par `sink_88` (P=2,52, lower=26,01, gap=23,50) :

| Nœud | P (bar) | lower (bar) | gap (bar) | max_upstream | hops | diagnostic |
|------|---------|-------------|-----------|--------------|------|------------|
| `sink_88` | 2,52 | 26,01 | **23,50** | 2,52 | 3 | infeasible |
| `sink_83` | 2,01 | 21,01 | 15,96 | 5,06 | 12 | infeasible |
| `sink_108` | 7,18 | 16,01 | 7,48 | 8,54 | 14 | infeasible (partiel) |
| `sink_125` | 34,88 | 41,01 | 0 | 70,00 | 14 | alimenté, friction |
| `sink_122` | 69,98 | 74,01 | 4,04 | 69,98 | 5 | entry anchor 70 < bound 74 |

**Trace topologique** (`.net`) : `sink_88` et `sink_83` sont connectées aux sources (source_22, source_28) mais via **43 à 57 hops** de tuyaux. Les control valves sont modélisées quasi-transparentes (pas de throttling, `steady_state.rs` l.235), donc la chute de 70 → 2,5 bar est **pure friction** sur un long trajet de distribution. Aucun compresseur n'est sur ces chemins.

**Conclusion** : `nomination_mild_618` est **hydrauliquement infeasible** pour `sink_88`/`sink_83` — le réseau ne peut pas délivrer 16 m³/s à 26 bar à ces points sans compression supplémentaire sur la branche de distribution. C'est le résultat négatif valide du problème de **validation of nominations** (Pfetsch et al.) : la nomination nécessite des investissements (nouveau compresseur ou renforcement tuyau) pour être satisfiable.

### Conclusion Phase II

- L'artefact basse-pression (entries à 4 bar) est **éliminé** par l'ancrage entry transport + la correction alias shortPipe. Le régime transport (70 bar) se propage désormais jusqu'aux branches alimentées.
- Le résidu résiduel 23,5 m³/s n'est plus un artefact numérique : c'est l'**infeasibilité hydraulique réelle** de `sink_88`/`sink_83` (branches de distribution longues sans compresseur, bornes transport non atteignables).
- `sink_125` (alimenté, friction 70→35) et `sink_122` (entry 70 < bound 74) sont marginaux : activation compresseur sur le chemin `innode_63→innode_53` (CS4, ratio 1,226) pourrait combler.
- La nomination mild_618 est **infeasible** sous le modèle MVP ; la feasibility exigerait de traiter les compresseurs comme variables de décision (NLP validation of nominations).



1. **Modèle frontière GasLib** : égalités/inégalités dual Q+P au niveau physique (pas pénalité soft seule).
2. **Couplage shortPipe** : même nœud physique `sink_*` ↔ `source_*` — pression unique, Q net.
3. **Compresseur / enthalpie** : lever le plancher massique si le réseau peut physiquement alimenter les planchers P.

Objectif Phase I : convergence nomination intacte vers **3×10⁻³ m³/s** sur mild_618 (**non atteint** ; cause racine identifiée : infaisabilité contractuelle P sous MVP Q-seul).
