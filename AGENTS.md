# AGENTS.md — OpenGasSim

## Vue d'ensemble du projet

OpenGasSim est un simulateur d'écoulement de gaz naturel en réseau, inspiré de SIMONE.
Architecture monorepo : backend Rust (Axum + petgraph + faer) + frontend Vue 3 / QuasarJS / CesiumJS.

---

## Structure du monorepo

```
gazsim/
├── back/              # Backend Rust — moteur de calcul + API
│   ├── src/
│   │   ├── gaslib/        # Parseur XML GasLib
│   │   ├── graph/         # Structure de données réseau (petgraph)
│   │   ├── solver/        # Solveur Newton-Raphson (régime permanent)
│   │   └── api/           # Endpoints REST Axum
│   ├── dat/               # Données GasLib (NON versionné)
│   └── benches/           # Benchmarks Criterion
├── front/             # Frontend QuasarJS + CesiumJS
│   └── src/
│       ├── components/    # CesiumViewer, SimulationPanel
│       ├── stores/        # Pinia stores (network, simulate)
│       ├── services/      # Client API axios
│       └── pages/         # MapPage
├── docker/            # Dockerfiles
│   ├── Dockerfile.back    # Image Rust + cargo-watch
│   └── Dockerfile.front   # Image Node 20 + @quasar/cli
├── scripts/           # Scripts utilitaires
│   ├── dev.sh             # Lance docker compose up --build
│   ├── stop.sh            # Arrête les conteneurs
│   ├── back-shell.sh      # Shell dans le conteneur back
│   ├── front-shell.sh     # Shell dans le conteneur front
│   ├── back-test.sh       # cargo test dans le conteneur
│   ├── front-test.sh      # npm test dans le conteneur
│   ├── ci.sh              # CI complète via Docker
│   └── fetch_gaslib.sh    # Télécharge les données GasLib
├── docker-compose.yml # Orchestration des services
├── docs/              # Documentation du projet
│   ├── architecture/
│   ├── science/
│   ├── features/
│   ├── reviews/
│   ├── plans/
│   └── temp/              # Brouillons (NON versionné)
└── AGENTS.md          # Ce fichier
```

---

## Environnement Docker (OBLIGATOIRE)

> **Règle absolue : tout passe par les conteneurs Docker.**
> Ne jamais exécuter `cargo`, `npm`, `npx` ou `quasar` directement sur la machine hôte.
> Les volumes partagés synchronisent le code source automatiquement.

### Démarrage

```bash
./scripts/dev.sh          # lance back + front (docker compose up --build)
./scripts/stop.sh         # arrête tout
```

### Ajouter une dépendance

```bash
# Backend (Rust)
./scripts/back-shell.sh
cargo add nom_du_crate    # dans le shell conteneur

# Frontend (Node)
./scripts/front-shell.sh
npm install nom_du_package  # dans le shell conteneur
```

Les fichiers `Cargo.toml` / `Cargo.lock` et `package.json` / `package-lock.json` sont
sur le volume partagé : les modifications faites dans le conteneur sont visibles sur l'hôte
et versionnées par git normalement.

### Tests

```bash
./scripts/back-test.sh    # cargo test dans le conteneur
./scripts/front-test.sh   # npm test dans le conteneur
./scripts/ci.sh           # CI complète (build + tests back & front)
```

### Volumes Docker

| Volume | Rôle | Partagé avec l'hôte ? |
|--------|------|-----------------------|
| `./back` → `/app` | Code source Rust | Oui (bind mount) |
| `./front` → `/app` | Code source front | Oui (bind mount) |
| `back-target` | Cache de compilation Rust (`target/`) | Non (volume nommé) |
| `cargo-registry` | Cache des crates téléchargés | Non (volume nommé) |
| `cargo-git` | Cache git de Cargo | Non (volume nommé) |
| `front-node-modules` | `node_modules/` | Non (volume nommé) |

Les volumes nommés (`back-target`, `front-node-modules`, etc.) persistent entre les
redémarrages de conteneurs. Pour les purger : `docker compose down -v`.

---

## Conventions

- **Langage backend** : Rust edition 2024. Utiliser `anyhow` pour les erreurs applicatives, `thiserror` pour les erreurs de bibliothèque.
- **Langage frontend** : TypeScript strict. Composition API uniquement (pas d'Options API).
- **Tests** : Chaque module Rust doit avoir un bloc `#[cfg(test)]` avec des tests unitaires. Utiliser `insta` pour les snapshot tests du parseur XML. Utiliser `vitest` côté frontend.
- **Documentation** : Les modules Rust doivent avoir un doc-comment `//!` en en-tête. Les fonctions publiques doivent être documentées.
- **Données** : Les fichiers GasLib (.net, .scn, .cs) vont dans `back/dat/`. Ne jamais les committer.
- **Exécution** : Toujours passer par les conteneurs Docker. Ne jamais installer de dépendances sur la machine hôte.

---

## Matrices de tâches par agent

### Agent "Backend Rust" (back/)

| Priorité | Tâche | Fichiers | Critère de succès |
|----------|-------|----------|-------------------|
| P0 | Parseur GasLib XML complet | `gaslib/parser.rs` | Parse GasLib-11.net sans erreur ; snapshot test insta |
| P0 | Structure GasNetwork robuste | `graph/mod.rs` | Tests unitaires : ajout nœuds/arêtes, requêtes voisins |
| P1 | Solveur steady-state Newton-Raphson | `solver/steady_state.rs` | Réseau 2 nœuds converge ; pression source = fixée |
| P1 | API REST `/api/network` + `/api/simulate` | `api/mod.rs` | Réponse JSON valide, test d'intégration reqwest |
| P2 | Support GasLib-24 et GasLib-40 | `gaslib/parser.rs` | Parse sans erreur, cohérence topologique |
| P2 | Solveur : support compresseurs | `solver/` | Station de compression augmente la pression |
| P3 | Benchmarks Criterion | `benches/solver_bench.rs` | GasLib-135 résolu en < 100ms |

### Agent "Frontend Vue/Quasar" (front/)

| Priorité | Tâche | Fichiers | Critère de succès |
|----------|-------|----------|-------------------|
| P0 | CesiumViewer : afficher nœuds + tuyaux | `components/CesiumViewer.vue` | Nœuds visibles sur le globe, tuyaux connectés |
| P0 | Store network : fetch et cache | `stores/network.ts` | Données chargées au mount, réactives |
| P1 | SimulationPanel : lancer simulation | `components/SimulationPanel.vue` | Bouton déclenche /api/simulate, résultats affichés |
| P1 | Coloration dynamique des tuyaux | `components/CesiumViewer.vue` | Couleur = f(débit), légende visible |
| P2 | Sélection d'un nœud → détails | `components/` | Click sur nœud → popup pression |
| P2 | Contrôles de demande (sliders) | `components/DemandControls.vue` | Modifier les demandes, relancer la simulation |
| P3 | Mode sombre / thème industriel | `css/app.scss` | Thème cohérent avec l'esthétique SCADA |

### Agent "Science & Validation" (docs/science/)

| Priorité | Tâche | Fichiers | Critère de succès |
|----------|-------|----------|-------------------|
| P0 | Documenter les équations (Darcy-Weisbach, Newton-Raphson) | `docs/science/equations.md` | Formules LaTeX vérifiées |
| P1 | Valider le solveur vs solution analytique 2 nœuds | `docs/science/validation.md` | Écart < 1% |
| P2 | Comparer résultats GasLib-11 avec littérature | `docs/science/validation.md` | Résultats cohérents |

### Agent "DevOps & Tests" (racine)

| Priorité | Tâche | Fichiers | Critère de succès |
|----------|-------|----------|-------------------|
| P0 | `cargo test` passe (tous les tests Rust) | `back/` | Exit code 0 |
| P0 | `vitest run` passe (tests frontend) | `front/` | Exit code 0 |
| P1 | Script CI : build back + front | `scripts/ci.sh` | Script exécutable, retour 0 |
| P2 | Téléchargement automatique GasLib | `scripts/fetch_gaslib.sh` | GasLib-11 dans back/dat/ |

---

## Workflow de développement

1. **Avant chaque modification** : lire les fichiers concernés, comprendre le contexte.
2. **Après chaque modification** : lancer les tests via les scripts Docker (`./scripts/back-test.sh` ou `./scripts/front-test.sh`).
3. **Pour ajouter une lib** : ouvrir un shell dans le conteneur concerné (`./scripts/back-shell.sh` ou `./scripts/front-shell.sh`), puis utiliser `cargo add` ou `npm install`.
4. **Après chaque phase** : mettre à jour `docs/reviews/` avec un résumé des changements.
5. **Résolution de problèmes** : documenter dans `docs/temp/` (non versionné) puis synthétiser dans `docs/features/`.

---

## Dépendances externes critiques

| Composant | Crate / Package | Version | Rôle |
|-----------|----------------|---------|------|
| Graphes | `petgraph` | 0.8 | Représentation du réseau |
| Algèbre linéaire | `faer` | 0.22 | Matrices creuses, Newton-Raphson |
| XML parsing | `quick-xml` | 0.37 | Lecture GasLib |
| Web server | `axum` | 0.8 | API REST |
| Globe 3D | `cesium` | 1.138 | Visualisation géospatiale |
| UI framework | `quasar` | 2.17 | Composants Vue 3 |
