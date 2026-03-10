# Testing — Comment exécuter les tests

Ce document complète le `README.md` avec un focus exécution des tests.
Le setup de l'environnement (Docker, lancement des services, scripts) reste documenté dans `README.md`.

## Validation scientifique

Le protocole de validation scientifique détaillé est maintenu dans
`docs/plans/implementation-plan.md` (Phase 2).

Pour le test de comparaison référence GasLib-11 (`test_gaslib_11_vs_reference_solution`),
une référence interne versionnée est fournie dans
`docs/testing/references/GasLib-11.reference.internal.csv`.

Régénération de la référence interne (après changement modèle/solveur):

```bash
cd back
cargo run --bin generate_gaslib11_reference
```

Vous pouvez aussi fournir une référence externe avec:

```bash
GAZFLOW_REFERENCE_SOLUTION_PATH=/chemin/vers/reference.sol cargo test test_gaslib_11_vs_reference_solution
```

## Commandes recommandées

Depuis la racine du projet :

```bash
./scripts/back-test.sh     # tests backend Rust
./scripts/front-test.sh    # tests frontend
./scripts/ci.sh            # build + tests complets
./scripts/validation-pack.sh # protocole scientifique backend T1->T10
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

Couverture actuelle minimale frontend:
- `src/services/ws.spec.ts` (mapping URL WS)
- `src/stores/network.spec.ts` (chargement réseau + gestion erreur)
- `src/stores/simulate.spec.ts` (warm-start + export)
- `src/config/dev-integration.spec.ts` (garde-fous config dev: boot Pinia + proxy WS `/api`)

Non-regression interface/websocket:
- ce test protege contre l'ecran vide (store Pinia utilise sans boot `pinia`);
- il protege aussi contre `websocket failed to open` en dev (proxy `/api` sans `ws: true`).

## Bonnes pratiques

- Exécuter au minimum les tests ciblés du scope modifié.
- Exécuter `./scripts/ci.sh` avant merge/livraison.
- Garder les commandes `cargo`/`npm` dans les conteneurs.

## Profiling backend (flamegraph)

Script dédié:

```bash
./scripts/profile.sh
```

Filtre benchmark optionnel:

```bash
./scripts/profile.sh steady_state_newton_parallel_n_threads
```

Le script utilise en priorité `cargo flamegraph`, sinon fallback `perf + inferno-flamegraph`.
Les sorties sont écrites dans `back/target/profile/`.

## Datasets GasLib (smoke/scaling)

Télécharger un dataset:

```bash
./scripts/fetch_gaslib.sh GasLib-24
./scripts/fetch_gaslib.sh GasLib-582
```

Notes:
- le script supporte `GasLib-11`, `GasLib-24`, `GasLib-40`, `GasLib-135`, `GasLib-582`, `GasLib-4197`;
- pour `582/4197`, il récupère aussi les archives de nominations (`.scn`) et crée des alias stables dans `back/dat/`.

Tests smoke grands réseaux (optionnels):

```bash
GAZFLOW_ENABLE_LARGE_DATASET_TESTS=1 cargo test test_solve_gaslib_582
GAZFLOW_ENABLE_LARGE_DATASET_TESTS=1 cargo test test_solve_gaslib_4197
```

Paramètres avancés (optionnels) pour ajuster les smoke tests large:
- `GAZFLOW_LARGE_TEST_MAX_ITER` (ex: `300`)
- `GAZFLOW_LARGE_TEST_TOL` (ex: `1e-2`)
- `GAZFLOW_LARGE_TEST_SCALES` (liste CSV, ex: `0.3,0.1,0.05`)
- `GAZFLOW_LARGE_TEST_MAX_SECONDS` (timeout global smoke large, ex: `60`)
- `GAZFLOW_CONTINUATION_AUTO_BRIDGES` (insère des paliers intermédiaires auto, ex: `1`)
- `GAZFLOW_CONTINUATION_MIN_GAP` (écart mini pour auto-bridge, ex: `0.02`)
- `GAZFLOW_CONTINUATION_MAX_SECONDS` (timeout global continuation, ex: `120`)
- `GAZFLOW_CONTINUATION_SNAPSHOT_EVERY` (fréquence snapshot/warm-start en continuation, ex: `3`)
- `GAZFLOW_CONTINUATION_ITER_SCHEDULE` (budget itérations par palier, CSV, ex: `1,1,4`)
- `GAZFLOW_DISABLE_JACOBI_FALLBACK` (debug: désactive fallback Jacobi dans Newton, ex: `1`)
- `GAZFLOW_GMRES_MAX_ITERS` / `GAZFLOW_GMRES_RESTART` (tuning solveur itératif GMRES)
- `GAZFLOW_PHYSICAL_INIT_ITERS` (nombre de sweeps d'initialisation physique avant Newton; `0` pour désactiver)

Valeurs par défaut:
- `GasLib-582` : `max_iter=180`, `tol=2e-3`, `scales=0.1,0.3`, timeout global `120s`;
- `GasLib-4197` : profil smoke très court `max_iter=6`, `tol=1e-2`, `scales=0.05,0.1,0.1`, timeout global `40s` (itérations réparties par défaut `1,1,4` entre paliers, initialisation physique courte par défaut `2` sweeps pour `>2000` nœuds, cap GMRES par défaut `220` itérations sur systèmes libres `>1200` inconnues, non-convergence explicite acceptée en mode smoke).

## Validation pack (backend)

Script unique pour exécuter T1->T10 en séquence:

```bash
./scripts/validation-pack.sh
```

Options utiles:
- `GAZFLOW_REGEN_REFERENCE=1` : régénère `GasLib-11.reference.internal.csv` avant T9.
- `GAZFLOW_RUN_LARGE_SMOKE=1` : ajoute les smoke tests grands datasets.
