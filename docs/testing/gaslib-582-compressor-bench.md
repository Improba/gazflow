# GasLib-582 — bench compresseur (I-A0, juin 2026)

Protocole : `compressor_diag`, réseau baseline, CDF off, `nomination_mild_618.scn`, slack retiré, `preset_robust` (release).

Commandes :

```bash
cd back && cargo build --release --bin compressor_diag
./target/release/compressor_diag GasLib-582 --json /tmp/baseline.json
./target/release/compressor_diag GasLib-582 --no-r2-cap --json /tmp/no-r2-cap.json
./target/release/compressor_diag GasLib-582 --map-mode measurement --json /tmp/measurement.json
```

## Résultats v1 (build release, ~48 s par run)

| Variante | Résidu (dernier Newton) | Tolérance preset | Convergence | effective r² (st. 1–3) | Verdict H2 |
|----------|-------------------------|------------------|-------------|-------------------------|------------|
| **Baseline** (cap r²≤9 actif) | **5,0 m³/s** | 3×10⁻³ | Non | 9,0 (~ratio eff. 3) | — |
| **`--no-r2-cap`** | **8,22 m³/s** | 3×10⁻³ | Non | 16,75 (~ratio eff. 4,09) | Cap **aide** la stabilité numérique |
| **`measurement`** (env v1) | **8,22 m³/s** | 3×10⁻³ | Non | 16,75 | Identique no-cap (diag sans boucle carte) |

## Résultats v2 (continuation + outer loop sur échec, diag enrichi, ~42–64 s/run)

Après intégration de `solve_with_compressor_loop` dans le chemin d’échec continuation (`continuation.rs`) et `--map-mode` sur `compressor_diag` (JSON : `continuation_scales`, `map_target_ratio`, `catalog_stations`).

| Variante | Résidu (dernier Newton) | Convergence | effective r² (st. 1–3) | map_target_ratio (st. 1–3) | Notes |
|----------|-------------------------|-------------|-------------------------|----------------------------|-------|
| **Baseline** (`legacy`) | **5,0 m³/s** | Non | 9,0 | ~1,08 | Continuation atteint scale=1,0 puis échec Newton nominal |
| **`--no-r2-cap`** | **8,22 m³/s** | Non | 16,75 | ~1,08 | Dégradation identique v1 |
| **`--map-mode measurement`** | **8,22 m³/s** | Non | 16,75 | ~1,08 | Outer loop sur échec continuation ; pas d’amélioration vs no-cap |

Observations v2 :

1. Les **18 paliers** de continuation sont identiques entre variantes (cf. `continuation_scales` dans le JSON).
2. **`map_target_ratio`** ~1,08 sur st. 1–3 indique que la carte vise un ratio bien en dessous du plafond `.net` (~4,09) au débit nul post-échec ; la boucle externe n’a pas le temps de recoupler Q–ratio au nominal.
3. Flux compresseurs **0 m³/s** dans le JSON car le solve échoue avant état convergé.

## Interprétation

1. **H2 (cap MVP dominant)** : **non confirmée** comme cause unique de l’échec. Retirer le cap **dégrade** le résidu (5 → 8,2 m³/s) : le plafond stabilise Newton mais empêche le ratio nominal `.net` sur les stations transport.
2. **Boucle carte v2** : l’outer loop sur échec continuation **ne compense pas** la perte de stabilité liée au cap r² ; résidu measurement = no-cap.
3. **Cause dominante actuelle** : modèle compresseur + couplage Q–ratio encore insuffisant ; résidu massique O(1–8 m³/s) au nominal.
4. **Prochaine mesure (I-A)** : coeffs biquadratiques GasLib, sélection `confId`, faisabilité surgeline/chokeline, vitesse $n$ libre ; objectif convergence 582 ou blocage documenté.

Artefacts JSON v2 : `/tmp/gazflow-582-bench-v3/{baseline,measurement}.json` (machine locale).

## Résultats v3 (recherche 1D vitesse + garde nominal transport, juin 2026)

| Variante | Résidu | map_target_ratio (st. 1–3, Q=0 post-échec) |
|----------|--------|-----------------------------------------------|
| Baseline | 5,0 m³/s | **4,09** (nominal `.net`, plus 1,08) |
| measurement | 8,22 m³/s | **4,09** |

La recherche 1D + `effective_ratio_with_nominal` aligne la cible carte sur le lift transport ; le résidu measurement reste dégradé vs baseline (outer loop sans débit convergé).

## Résultats v4 (turbo/biquadratique + garde outer loop + r² hybride, juin 2026)

| Variante | Résidu | effective r² (st. 1–3) | Notes |
|----------|--------|-------------------------|-------|
| Baseline | **5,0 m³/s** | 9,0 | inchangé |
| measurement | **5,0 m³/s** | 16,75 (post-échec diag) | **plus de régression 8,22** |
| biquadratic | **5,0 m³/s** | 9,0 | coeffs `n_isoline` actifs |

Leviers v4 : `guarded_compressor_ratio_step` (pas de baisse transport avant convergence), `GAZFLOW_COMPRESSOR_R2_CAP_UNTIL_CONVERGED=1` (défaut measurement/biquadratic), parsing turbo + eval biquadratique GasLib.

## Résultats v13 (diagnostic massique + hub balance sink_2, juin 2026)

| Mode | Résidu | Pire nœud libre | Notes |
|------|--------|-----------------|-------|
| measurement | **3,0 m³/s** (was **5,0**) | `innode_381` (+3) | `sink_2` ancré P=2,01 bar (hub Q=0 le plus connecté) |
| biquadratic | **3,0 m³/s** | idem | |

Leviers : `mass_balance_report` dans `compressor_diag` JSON ; détection `balance_hubs` (top 2 exits Q≈0 par degré topologique) ; ancrage pression locale.

Cause racine du plancher 5 m³/s : **`sink_2`** portait tout le déséquilibre (hub junction sans DOF pression).

Artefacts : `/tmp/582-v13.json`, `/tmp/582-v13-mass.json`.

## Résultats v14 (entries Q=0 + junction anchors mixtes, juin 2026)

| Mode | Résidu | Pire nœud libre | Notes |
|------|--------|-----------------|-------|
| measurement | **2,0 m³/s** (was **3,0**) | `innode_420` (−2) | `innode_381` ancré (junction entry+exit Q≈0) |

Leviers v14 :

1. Boundaries Q≈0 : **entries + exits** (pression depuis scénario ou `.net`).
2. `balance_hubs` : top 2 inchangé (`sink_2`, `sink_96`).
3. `junction_anchors` : innodes degré ≥4 avec voisins entry **et** exit Q≈0 (priorité mixte) ; top **2** (`innode_381`, …).

Sur-ancrage (8 hubs + 5 junctions) **dégrade** le résidu (3,6 m³/s) : trop de pressions fixées localement.

Artefact : `/tmp/582-v14c.json`.

## Résultats v15 (spine boundary + junctions degré 3, juin 2026)

| Mode | Résidu | Pire nœud libre | Notes |
|------|--------|-----------------|-------|
| measurement | **2,0 m³/s** (= v14) | `sink_24` (−2) | `source_17` spine + `innode_381`/`innode_385` |

Leviers v15 :

1. **`boundary_spine_anchors`** : boundaries Q≈0 source/sink degré ≥4 (`source_17`), séparées des junctions internes.
2. **Junctions `innode_*` degré ≥3** si mix entry+exit Q≈0, ou hub exit-only (≥2 exits Q≈0).
3. **Exclusion des extrémités compresseur** (`innode_402` évité).
4. Sur-ancrage confirmé : 3+ junctions → régression (3,6 m³/s).

Artefact : `/tmp/582-v15.json`.

## Résultats v12 (distribution sud + couplage pression/ratio, juin 2026)

| Mode | Résidu | eval_q CS4/CS5 | `map_target` CS4/CS5 |
|------|--------|----------------|----------------------|
| measurement | **5,0 m³/s** | **~10,4** (was 45) | ~1,31 / ~1,46 |

Leviers : zone locale distribution (hors voisinage CS transport), repli peer CS4→CS5, relaxation bidirectionnelle ratio si résidu bloqué, coefficient P² compresseur adouci vs ratio pression atteint (Newton).

Résidu inchangé au plancher **5 m³/s** ; les ratios sud étaient sur-estimés (Q=45) et sont corrigés.

Artefact : `/tmp/582-v12.json`.

## Résultats v11 (débit carte topologique hub/branche, juin 2026)

| Mode | Résidu | `map_target` CS1 | CS2–3 | eval_q CS1 / CS2 |
|------|--------|------------------|-------|------------------|
| measurement | **5,0 m³/s** | **~1,46** | ~1,50 | **90 / 45 m³/s** |

Leviers : BFS aval sans traverser compresseurs (`flow_topology.rs`) — CS1 détecté merger hub (≥2 branches), CS2/CS3 branches parallèles → Q = total / 2.

Comparaison v10 → v11 : eval_q transport **30 → 90** (CS1), ratios carte **~1,51 → ~1,46–1,50**. Résidu inchangé au plancher **5 m³/s** : la topologie corrige le débit carte mais le goulot reste hydraulique Newton, pas le split Q.

Artefact : `/tmp/582-v11.json`.

## Résultats v10 (débit hub transport + refine continuation, juin 2026)

| Mode | Résidu | `map_target` CS1–3 | eval_q transport |
|------|--------|--------------------|------------------|
| measurement | **5,0 m³/s** | **~1,51** | **30 m³/s** (90/3 hub) |
| biquadratic | **5,0 m³/s** | ~1,51 | 30 |

Leviers : split transport cap≥3 vs distribution, continuation refine [0,92→1,0], clamp Q solver absurdes, diag map cohérent.

**Plancher ~5 m³/s** avec ratios carte cohérents : prochain levier = débit **topologique** (CS1 ≈ flux hub ~90, pas 30) ou hydraulique MVP au-delà du ratio.

## Résultats v9 (Newton partiel + handoff carte Q estimé, juin 2026)

| Mode | Résidu | Statut | Notes |
|------|--------|--------|-------|
| measurement | **5,0 m³/s** | ok (partiel) | handoff relax=1,0, Q estimé si non convergé, r² cap off en outer |
| biquadratic | à bench | | idem pipeline |

Retour au plateau **5 m³/s** avec sémantique ratio correcte (~1,33–1,51 transport) vs faux 5 m³/s d’avant (cap r² + ratio `.net`).

## Résultats v8 (confId→turbo config_2, juin 2026)

Fix : `preferred_turbo` résout `config_2` → `compressor_6` (582 CS1) au lieu de `compressor_5` (min id).

| Mode | Résidu | `map_target` CS1–3 | `ratio_max` post-handoff |
|------|--------|--------------------|--------------------------|
| measurement | **8,22 m³/s** | **~1,51** | **~1,29** (relax 0,5 depuis 1,08) |
| biquadratic | à bench | ~1,51 | ~1,29 |

Le ratio carte transport passe de 1,08 à **~1,51** à Q≈18 m³/s ; le résidu Newton reste 8,22 (prochain levier : convergence avec lift ~1,3–1,5).

## Résultats v7 (recouplage Q estimé + garde carte upward, juin 2026)

| Mode | Résidu | `map_eval_q` | `map_target` (st. 1–3) | Notes |
|------|--------|--------------|------------------------|-------|
| measurement | **8,22 m³/s** | **~18 m³/s** | **1,08** | diag : p_in fallback 40 bar si solve échoue |
| biquadratic | **8,22 m³/s** | ~18 | 1,08 | idem |

Le recouplage Q fonctionne (`map_eval_q_m3s` ≈ livraison/5). Le ratio reste au catalogue car la carte 582 à Q≈18 norm / p_in≈40 bar donne **~1,08** (pas 1,11) : `find_operating_point` trouve un point mais la tête ne dépasse pas le operating `.cs`.

## Résultats v6 (Option 1 — sémantique ratio operating vs pressure cap, juin 2026)

Décision documentée : `docs/testing/gaslib-582-compressor-diagnosis.md`.

| Mode | Résidu | `compressor_ratio_max` (st. 1–3) | `map_target_ratio` |
|------|--------|----------------------------------|--------------------|
| legacy | **8,22 m³/s** | **1,08** (`.cs`) | 1,08 |
| measurement | **8,22 m³/s** | 1,08 | 1,08 |
| biquadratic | **8,22 m³/s** | 1,08 | 1,08 |

Le plafond pression `.net` (4,09) est conservé dans `compressor_pressure_cap_ratio` mais n'est plus confondu avec le ratio d'exploitation. Le faux plateau baseline 5 m³/s disparaît.

## Résultats v5 (couplage Q–ratio continuation + débit estimé, juin 2026)

Leviers : `apply_map_ratios_after_continuation_step` (scale ≥ 0.5), débit nominal estimé depuis les sinks quand Q solver ≈ 0, salvage si outer loop échoue après continuation convergée.

Artefacts : `/tmp/gazflow-582-bench-v5/{baseline,measurement,biquadratic}.json`.

| Variante | Résidu | Notes |
|----------|--------|-------|
| Baseline (mild_618) | 5,0 m³/s | inchangé vs v4 |
| measurement | **8,95 m³/s** | régression v5 (cible carte sur paliers partiels) — **corrigé** : `allow_map_target` uniquement au nominal |
| biquadratic | **5,0 m³/s** | = baseline |

Test intégration `test_solve_gaslib_582` (`.scn` défaut, tol smoke 0,3, ~14 min) : **OK** avec résidu **8,59 m³/s** au nominal (non strict).
