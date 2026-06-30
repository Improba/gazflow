# GasLib-582 — bench compresseur (I-A0, juin 2026)

Protocole : `compressor_diag`, réseau baseline, CDF off, `nomination_mild_618.scn`, slack retiré, `preset_robust` (release).

Commandes :

```bash
cd back && cargo build --release --bin compressor_diag
./target/release/compressor_diag GasLib-582 --json /tmp/baseline.json
./target/release/compressor_diag GasLib-582 --no-r2-cap --json /tmp/no-r2-cap.json
GAZFLOW_COMPRESSOR_MAP_MODE=measurement \
  ./target/release/compressor_diag GasLib-582 --json /tmp/measurement.json
```

## Résultats (build release, ~48 s par run)

| Variante | Résidu (dernier Newton) | Tolérance preset | Convergence | effective r² (st. 1–3) | Verdict H2 |
|----------|-------------------------|------------------|-------------|-------------------------|------------|
| **Baseline** (cap r²≤9 actif) | **5,0 m³/s** | 3×10⁻³ | Non | 9,0 (~ratio eff. 3) | — |
| **`--no-r2-cap`** | **8,22 m³/s** | 3×10⁻³ | Non | 16,75 (~ratio eff. 4,09) | Cap **aide** la stabilité numérique |
| **`measurement`** (env) | **8,22 m³/s** | 3×10⁻³ | Non | 16,75 | Identique au no-cap sur ce binaire* |

\* `compressor_diag` appelle `solve_steady_state_with_preset` sans boucle externe carte ; le mode `measurement` désactive le cap r² via config/env mais n’exécute pas encore la boucle `compressor_loop` dans ce binaire. Pour tester la boucle carte complète : solve API / test avec continuation + outer loop.

## Interprétation

1. **H2 (cap MVP dominant)** : **non confirmée** comme cause unique de l’échec. Retirer le cap **dégrade** le résidu (5 → 8,2 m³/s) : le plafond stabilise Newton mais empêche le ratio nominal `.net` sur les stations transport.
2. **Cause dominante actuelle** : modèle compresseur + couplage Q–ratio encore insuffisant ; résidu massique O(1–8 m³/s) au nominal.
3. **Prochaine mesure** : bench via solve complet avec `GAZFLOW_COMPRESSOR_MAP_MODE=measurement` et continuation (outer loop), pas seulement `compressor_diag`.

Artefacts JSON : `/tmp/gazflow-582-bench/{baseline,no-r2-cap,measurement}.json` (machine locale).
