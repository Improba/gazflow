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

## Résultats v5 (couplage Q–ratio continuation + débit estimé, juin 2026)

Leviers : `apply_map_ratios_after_continuation_step` (scale ≥ 0.5), débit nominal estimé depuis les sinks quand Q solver ≈ 0, salvage si outer loop échoue après continuation convergée.

Artefacts : `/tmp/gazflow-582-bench-v5/{baseline,measurement,biquadratic}.json`.

| Variante | Résidu | Notes |
|----------|--------|-------|
| Baseline (mild_618) | 5,0 m³/s | inchangé vs v4 |
| measurement | **8,95 m³/s** | régression v5 (cible carte sur paliers partiels) — **corrigé** : `allow_map_target` uniquement au nominal |
| biquadratic | **5,0 m³/s** | = baseline |

Test intégration `test_solve_gaslib_582` (`.scn` défaut, tol smoke 0,3, ~14 min) : **OK** avec résidu **8,59 m³/s** au nominal (non strict).
