# Corpus de validation — phases opérationnelles (P6–P13)

Jeux de données stables pour valider le plan `docs/plans/operational-roadmap.md` sans dépendre de données opérateur ni de téléchargements ad hoc.

## Structure

```
corpus/
├── README.md                 ← ce fichier
├── manifest.yaml             ← inventaire + mapping phase → fichier
├── synthetic/                ← versionné (petit, déterministe)
│   ├── minimal-line/         ← P6 : import GeoJSON 3 nœuds / 2 pipes
│   ├── gravity-pipe/         ← P7 : altitudes non nulles (CSV)
│   ├── topo-errors/          ← P6 : graphes invalides
│   ├── demand/               ← P9 : profils thermosensibles
│   └── scada/                ← P13 : mesures synthétiques
├── mapping/                  ← schémas YAML d'exemple
└── external/                 ← téléchargé (gitignored) via fetch_test_corpus.sh
    ├── gaslib-39/
    ├── transient/gaslib-11/
    └── scigrid/fr-snippet/
```

## Installation

Depuis la racine du dépôt :

```bash
./scripts/fetch_test_corpus.sh
```

Télécharge :

- **GasLib-39** (control valves, 10 scénarios) — P8, P10
- **TRR154** GasLib-11 transitoire (`.bcd` + `.state`) — P11
- **SciGRID IGGIELGN** extrait France (~80 tronçons) — P6

Les fixtures synthétiques dans `synthetic/` sont déjà versionnées ; aucune action requise.

GasLib classique (11, 582, …) reste dans `back/dat/` via `./scripts/fetch_gaslib.sh`.

## Usage prévu (quand P6+ seront codées)

```bash
# Chemin racine du corpus (depuis le backend Rust)
export GAZFLOW_TEST_CORPUS=../docs/testing/corpus

cargo test test_geojson_import_minimal
cargo test test_gravity_uphill_increases_pressure_drop
```

Constante recommandée côté Rust : lire `GAZFLOW_TEST_CORPUS` ou `CARGO_MANIFEST_DIR/../docs/testing/corpus`.

## Couverture par phase

| Phase | Jeu principal | Rôle |
|-------|---------------|------|
| P6 | `synthetic/minimal-line/`, `topo-errors/`, `external/scigrid/` | Import, mapping, validation topo |
| P7 | `synthetic/gravity-pipe/`, GasLib-11 (régression) | Gravité, régression physique |
| P8 | `external/gaslib-39/`, GasLib-24/582 | Control valves |
| P9 | `synthetic/demand/`, nominations GasLib | Profils / snapshots |
| P10 | GasLib-11, GasLib-39 | N-1 vert / rouge |
| P11 | `external/transient/gaslib-11/` | Benchmark transitoire TRR154 |
| P12 | tout réseau chargé | Édition (pas de données spécifiques) |
| P13 | `synthetic/scada/` | Calage sur mesures synthétiques |

## Licence / citation

- **GasLib** : CC BY 3.0 — citer Schmidt et al. (2017) Data 2(4):40.
- **SciGRID_gas IGGIELGN** : voir `external/scigrid/fr-snippet/LICENSE` après fetch.
- **TRR154 transient** : données académiques TRR154 / GasLib ; citer si publication.
