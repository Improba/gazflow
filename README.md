# GazFlow

Simulateur d'écoulement de gaz naturel en réseau, inspiré de SIMONE.

![GazFlow Screenshot](docs/assets/screenshot.png)

## Ce que fait GazFlow (vision métier)

GazFlow simule l'écoulement du gaz dans un réseau de transport à partir d'une topologie
GasLib et d'un scénario de demande. L'outil calcule un point de fonctionnement
hydraulique en régime permanent (pressions nodales et débits par conduite), puis le
restitue en lecture opérationnelle : carte 3D, suivi de convergence et exports exploitables.

### Pour quoi faire

- Étudier le comportement hydraulique d'un réseau selon différents niveaux de soutirage/injection
- Visualiser rapidement les zones de pression forte/faible et les conduites les plus sollicitées
- Comparer des scénarios et documenter les résultats (JSON/CSV/ZIP)

### Ce que l'outil n'est pas

GazFlow est un prototype de simulation et de visualisation inspiré des outils industriels.
Il ne remplace pas un simulateur certifié d'exploitation réseau.

## Architecture

- **back/** — Backend Rust : moteur de calcul (Darcy-Weisbach, Newton-Raphson) + API REST (Axum)
- **front/** — Frontend Vue 3 / QuasarJS / CesiumJS : visualisation géospatiale 3D
- **docker/** — Dockerfiles pour les services back et front
- **docs/** — Documentation (architecture, science, plans)

## Prérequis

- Docker & Docker Compose

C'est tout. Les toolchains Rust et Node vivent dans les conteneurs.

## Quickstart

```bash
# 1. Télécharger les données GasLib
./scripts/fetch_gaslib.sh GasLib-11

# 2. Lancer l'environnement de développement
./scripts/dev.sh
```

- Backend (API Rust) : `http://localhost:3001`
- Frontend (Quasar/CesiumJS) : `http://localhost:9000`

## Scripts

| Script | Description |
|--------|-------------|
| `./scripts/dev.sh` | Lance back + front via Docker Compose |
| `./scripts/stop.sh` | Arrête tous les conteneurs |
| `./scripts/back-shell.sh` | Shell dans le conteneur back (`cargo add`, etc.) |
| `./scripts/front-shell.sh` | Shell dans le conteneur front (`npm install`, etc.) |
| `./scripts/back-test.sh` | Lance `cargo test` dans le conteneur |
| `./scripts/front-test.sh` | Lance `npm test` dans le conteneur |
| `./scripts/ci.sh` | CI complète (build + tests back & front) |
| `./scripts/fetch_gaslib.sh` | Télécharge les données GasLib |

## Ajouter une dépendance

Toujours passer par le conteneur :

```bash
# Rust
./scripts/back-shell.sh
cargo add ma-crate

# Node
./scripts/front-shell.sh
npm install mon-package
```

Les fichiers `Cargo.toml` et `package.json` sont sur le volume partagé : les modifications
sont visibles sur l'hôte et versionnées par git.

## Tests

```bash
./scripts/back-test.sh     # Tests Rust
./scripts/front-test.sh    # Tests frontend
./scripts/ci.sh            # CI complète
```

## Documentation

- [Quickstart](docs/quickstart.md)
- [Architecture](docs/architecture/overview.md)
- [Contrat d'export des résultats](docs/architecture/export-contract.md)
- [Équations physiques](docs/science/equations.md)
- [Plan d'implémentation (partagé)](docs/plans/implementation-plan.md)
- [Fonctionnalités MVP](docs/features/mvp.md)
