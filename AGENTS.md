# AGENTS.md — GazSim

## Vue d'ensemble du projet

GazSim est un simulateur d'écoulement de gaz naturel en réseau, inspiré de SIMONE.
Architecture monorepo : backend Rust (Axum + petgraph + faer) + frontend Vue 3 / QuasarJS / CesiumJS.

Le plan d'implémentation détaillé (phases, tâches, tests) est par défaut dans `temp/plans/implementation-plan.md`.
Si un plan doit être partagé, il est alors copié dans `docs/plans/`.
Le guide d'exécution des tests est dans `docs/testing/README.md`.

---

## Structure du monorepo

```
gazsim/
├── back/              # Backend Rust — moteur de calcul + API
│   ├── src/
│   │   ├── gaslib/        # Parseur XML GasLib
│   │   ├── graph/         # Structure de données réseau (petgraph)
│   │   ├── solver/        # Solveur Newton-Raphson (régime permanent)
│   │   └── api/           # Endpoints REST + WebSocket Axum
│   ├── dat/               # Données GasLib (NON versionné)
│   └── benches/           # Benchmarks Criterion
├── front/             # Frontend QuasarJS + CesiumJS
│   └── src/
│       ├── components/    # CesiumViewer, SimulationPanel, LogPanel
│       ├── stores/        # Pinia stores (network, simulate)
│       ├── services/      # Client API axios + WebSocket
│       └── pages/         # MapPage
├── docker/            # Dockerfiles
│   ├── Dockerfile.back    # Image Rust + cargo-watch
│   └── Dockerfile.front   # Image Node 20 + @quasar/cli
├── scripts/           # Scripts utilitaires
├── docker-compose.yml # Orchestration des services
├── docs/              # Documentation du projet (partageable)
│   ├── architecture/      # Diagrammes, stratégie multi-thread
│   ├── science/           # Équations, validation
│   ├── features/          # Spécifications fonctionnelles
│   ├── reviews/           # Résumés de phase
│   ├── testing/           # Guide pour exécuter les tests
│   └── plans/             # Plans partagés uniquement
├── temp/              # Documents de travail locaux (par défaut)
│   └── plans/             # Plans en cours (source de vérité locale)
└── AGENTS.md          # Ce fichier
```

---

## Environnement Docker (OBLIGATOIRE)

> **Règle absolue : tout passe par les conteneurs Docker.**
> Ne jamais exécuter `cargo`, `npm`, `npx` ou `quasar` directement sur la machine hôte.
> Les volumes partagés synchronisent le code source automatiquement.

### Commandes courantes

```bash
./scripts/dev.sh           # lance back + front (docker compose up --build)
./scripts/stop.sh          # arrête tout

./scripts/back-shell.sh    # shell dans le conteneur back
./scripts/front-shell.sh   # shell dans le conteneur front

./scripts/back-test.sh     # cargo test dans le conteneur
./scripts/front-test.sh    # npm test dans le conteneur
./scripts/ci.sh            # CI complète (build + tests)
```

### Ajouter une dépendance

```bash
# Backend : ouvrir un shell conteneur, puis :
cargo add nom_du_crate

# Frontend : ouvrir un shell conteneur, puis :
npm install nom_du_package
```

Les fichiers `Cargo.toml` / `package.json` sont sur le volume partagé :
les modifications dans le conteneur sont visibles sur l'hôte et versionnées normalement.

### Volumes Docker

| Volume | Rôle | Partagé hôte ? |
|--------|------|----------------|
| `./back` → `/app` | Code source Rust | Oui |
| `./front` → `/app` | Code source front | Oui |
| `back-target` | Cache compilation (`target/`) | Non |
| `cargo-registry` | Cache crates | Non |
| `cargo-git` | Cache git Cargo | Non |
| `front-node-modules` | `node_modules/` | Non |

Purger les volumes : `docker compose down -v`.

---

## Conventions de code

### Rust (back/)

- Edition 2024.
- `anyhow` pour les erreurs applicatives, `thiserror` pour les erreurs de bibliothèque.
- Chaque module doit avoir un doc-comment `//!` en en-tête.
- Les fonctions publiques doivent être documentées.
- Chaque module doit avoir un bloc `#[cfg(test)]` avec des tests unitaires.
- Utiliser `insta` pour les snapshot tests du parseur XML.
- Le solveur ne doit jamais bloquer le runtime tokio : utiliser `spawn_blocking`.
- Paralléliser les boucles sur les pipes via `rayon::par_iter` quand le réseau dépasse ~50 tuyaux.

### TypeScript / Vue (front/)

- TypeScript strict.
- Composition API uniquement (pas d'Options API).
- Utiliser `vitest` pour les tests unitaires.
- Communication simulation via WebSocket (pas de polling REST).

### Données

- Les fichiers GasLib (.net, .scn, .cs) vont dans `back/dat/`. Ne jamais les committer.

---

## Workflow de développement

1. **Avant chaque modification** : lire les fichiers concernés, comprendre le contexte.
2. **Après chaque modification** : lancer les tests via les scripts Docker.
3. **Pour ajouter une lib** : passer par le shell conteneur.
4. **Après chaque phase** : mettre à jour `docs/reviews/` avec un résumé.
5. **Résolution de problèmes** : documenter dans `temp/` puis synthétiser dans `docs/features/`.
6. **Référence** : par défaut, le plan d'implémentation fait foi dans `temp/plans/implementation-plan.md`.
   Si le plan est destiné au partage, publier une copie dans `docs/plans/`.

---

## Dépendances externes

| Composant | Crate / Package | Rôle |
|-----------|----------------|------|
| Graphes | `petgraph` | Représentation du réseau |
| Algèbre linéaire | `faer` | Matrices creuses, Newton complet |
| Parallélisme | `rayon` | Itération parallèle sur les pipes |
| XML parsing | `quick-xml` | Lecture GasLib |
| Web server | `axum` | API REST + WebSocket |
| Async runtime | `tokio` | spawn_blocking, mpsc channels |
| Globe 3D | `cesium` | Visualisation géospatiale |
| UI framework | `quasar` | Composants Vue 3 |
