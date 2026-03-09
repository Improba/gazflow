# Testing — Comment exécuter les tests

Ce document complète le `README.md` avec un focus exécution des tests.
Le setup de l'environnement (Docker, lancement des services, scripts) reste documenté dans `README.md`.

## Validation scientifique

Le protocole de validation scientifique détaillé est maintenu dans
`docs/plans/implementation-plan.md` (Phase 2).

## Commandes recommandées

Depuis la racine du projet :

```bash
./scripts/back-test.sh     # tests backend Rust
./scripts/front-test.sh    # tests frontend
./scripts/ci.sh            # build + tests complets
```

## Tests backend

Exécution complète :

```bash
./scripts/back-shell.sh
cargo test
```

Test ciblé :

```bash
cargo test steady_state_two_nodes
```

## Tests frontend

Exécution complète :

```bash
./scripts/front-shell.sh
npm test
```

Alternative fréquente :

```bash
npx vitest run
```

## Bonnes pratiques

- Exécuter au minimum les tests ciblés du scope modifié.
- Exécuter `./scripts/ci.sh` avant merge/livraison.
- Garder les commandes `cargo`/`npm` dans les conteneurs.
