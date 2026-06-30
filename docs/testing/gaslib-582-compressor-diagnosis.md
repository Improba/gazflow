# GasLib-582 — diagnostic compresseur (mild_618, juin 2026)

## Décision : Option 1

**Ratio d'exploitation** = catalogue `.cs` (carte / étages), **plafonné** par les bornes pression `.net`.

Options écartées :
- **Option 2** (lift chaîne) : CS1–3 ne sont pas en série mais en branches parallèles convergent vers `innode_14`.
- **Option 3** (abandon 582) : reportée ; on teste d'abord la sémantique ratio corrigée.

## Conclusion sur 4,09 / station

| | Transport CS1–3 | Sud CS4–5 |
|--|-----------------|-----------|
| Plafond `.net` (p_out/p_in) | **4,09** | **2,10** |
| Ratio carte à Q≈18 m³/s | **~1,11** | 1,3–1,6 |
| Faisable enveloppe `.cs` | oui | oui |
| Lift carte suffisant pour 4,09 ? | **non** | non |

Le ratio **4,09 n'est pas un degré de liberté d'exploitation** : c'est une borne d'équipement. L'imposer comme `compressor_ratio_max` créait un conflit hydraulique (résidu ~5 m³/s).

## Topologie (mild_618)

- Livraison hors slack : **90,13 m³/s** norm.
- CS2 et CS3 → hub `innode_14` (parallèle) ; CS1 lift final 14 → 389.
- Q estimé split égal (5 CS) : **18 m³/s** / station (approximation v7–v9).
- **v11 topologie** : CS1 **90 m³/s**, CS2/CS3 **45 m³/s** chacun (branches parallèles → hub `innode_14`).

## Changement code (unique)

`compressor_nominal_ratio` / `compressor_ratio_max` ← `.cs` (~1,08)
`compressor_pressure_cap_ratio` ← bornes `.net` (4,09 transport)

Carte : `effective_ratio = clamp(map(Q), operating, cap)`.

## Bench post Option 1 (juin 2026)

Après séparation operating / pressure cap (`compressor_ratio_max` ← `.cs`, `compressor_pressure_cap_ratio` ← `.net`) :

| Mode | Résidu | `ratio_max` / `map_target` (st. 1–3) |
|------|--------|--------------------------------------|
| legacy | **8,22 m³/s** | **1,08** / 1,08 |
| measurement | **8,22 m³/s** | 1,08 / 1,08 |
| biquadratic | **8,22 m³/s** | 1,08 / 1,08 |

Interprétation :

1. **4,09 n'était pas le bon DOF** : le résidu « baseline » 5 m³/s reposait sur un plafond r² artificiel (9) avec ratio nominal `.net` incohérent avec la carte.
2. Avec la sémantique corrigée, le solveur n'atteint plus ce faux équilibre ; le résidu remonte à **8,22 m³/s** (proche du no-cap v1–v4).
3. `map_target` reste au catalogue **1,08** tant que Q compresseur ≈ 0 (échec Newton avant recouplage carte).

Prochaine étape (post v12) : bilan massique nodal (identifier le nœud à ~5 m³/s), hydraulique tête/vitesse dans Newton, ou nomination / slack.
