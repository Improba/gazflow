# Testing — Comment exécuter les tests

Ce dossier centralise la procédure de test du projet GazSim.

## Prérequis

- Docker et Docker Compose installés.
- Services lancés via les scripts du projet.

## Démarrage rapide

Depuis la racine `gazsim/` :

```bash
./scripts/dev.sh
```

Cette commande lance les services backend et frontend dans les conteneurs.

## Exécuter les tests backend

Commande recommandée :

```bash
./scripts/back-test.sh
```

Alternative en shell conteneur backend :

```bash
./scripts/back-shell.sh
cargo test
```

Pour un test ciblé :

```bash
cargo test steady_state_two_nodes
```

## Exécuter les tests frontend

Commande recommandée :

```bash
./scripts/front-test.sh
```

Alternative en shell conteneur frontend :

```bash
./scripts/front-shell.sh
npm test
```

Selon la configuration du front, vous pouvez aussi utiliser :

```bash
npx vitest run
```

## Exécuter la CI locale complète

Pour valider build + tests en une seule commande :

```bash
./scripts/ci.sh
```

## Bonnes pratiques

- Lancer les tests backend après chaque modification dans `back/`.
- Lancer les tests frontend après chaque modification dans `front/`.
- Exécuter `./scripts/ci.sh` avant de finaliser une livraison.
- Éviter d'exécuter `cargo`/`npm` directement sur l'hôte : utiliser les conteneurs.
