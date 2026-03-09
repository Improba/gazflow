# Plan d'implémentation — OpenGasSim MVP

## Objectif

Simuler l'écoulement en régime permanent sur un petit réseau GasLib (GasLib-11, 11 nœuds)
et visualiser les résultats (pressions, débits) sur un globe CesiumJS.

---

## Phase 0 : Bootstrap (jour 1)

### Tâches

- [x] Créer la structure monorepo (`back/`, `front/`, `docs/`)
- [x] Initialiser le projet Rust (Cargo.toml, modules)
- [x] Initialiser le projet Quasar + CesiumJS
- [x] Écrire AGENTS.md
- [ ] Télécharger GasLib-11 dans `back/dat/`
- [ ] Premier `cargo build` sans erreur
- [ ] Premier `npm install` + `quasar dev` sans erreur

### Tests automatiques

```bash
# T0-1 : Le backend compile
cd back && cargo check

# T0-2 : Le frontend s'installe
cd front && npm install && npx quasar build
```

### Critères de passage

- `cargo check` → 0 erreurs
- `npm install` → 0 erreurs critiques
- Les fichiers GasLib-11 (.net, .scn) sont présents dans `back/dat/`

---

## Phase 1 : Parseur GasLib + Graphe (jours 2-4)

### Données source

- **GasLib-11** : 11 nœuds, ~12 tuyaux, 1 station de compression, coordonnées GPS.
- Téléchargement : <https://gaslib.zib.de/testData.html>
- Format : XML avec namespaces `framework:`, conforme aux XSD GasLib.

### Tâches

| # | Tâche | Agent | Fichier(s) |
|---|-------|-------|------------|
| 1.1 | Script de téléchargement GasLib | DevOps | `scripts/fetch_gaslib.sh` |
| 1.2 | Parseur XML : nœuds (source, sink, innode) | Backend | `gaslib/parser.rs` |
| 1.3 | Parseur XML : connexions (pipe, compressorStation, valve) | Backend | `gaslib/parser.rs` |
| 1.4 | Parseur XML : scénarios (.scn) — demandes aux nœuds | Backend | `gaslib/parser.rs` |
| 1.5 | Construction du GasNetwork depuis les données parsées | Backend | `graph/mod.rs` |
| 1.6 | Snapshot tests insta du parseur | Backend | `gaslib/parser.rs` |

### Tests automatiques

```bash
# T1-1 : Le parseur charge GasLib-11 sans panique
cargo test test_parse_gaslib_11

# T1-2 : Le graphe a le bon nombre de nœuds/arêtes
cargo test test_gaslib_11_topology -- --nocapture
# Attendu : 11 nœuds, ~12 connexions

# T1-3 : Snapshot test (structure JSON du réseau parsé)
cargo test test_gaslib_11_snapshot
# Utilise insta::assert_yaml_snapshot!

# T1-4 : Chaque nœud a des coordonnées GPS valides
cargo test test_all_nodes_have_gps
```

### Critères de passage

- GasLib-11.net parsé → 11 nœuds avec coordonnées GPS
- Aucun nœud orphelin
- Snapshot insta validé

---

## Phase 2 : Solveur régime permanent (jours 5-9)

### Fondements mathématiques

Voir `docs/science/equations.md` pour le détail.

**Équation de Darcy-Weisbach pour le gaz :**

```
P_in² - P_out² = K · Q · |Q|
```

où `K = f · L · ρ_n · T · Z / (D⁵ · π²/16)` (simplifié).

**Algorithme Newton-Raphson nodal :**

1. Initialiser les pressions (ex: 70 bar partout).
2. Calculer les débits dans chaque tuyau en fonction de ΔP².
3. Calculer le résidu (bilan de masse à chaque nœud).
4. Construire le Jacobien et résoudre le système linéaire.
5. Mettre à jour les pressions.
6. Répéter jusqu'à convergence (résidu < tolérance).

### Tâches

| # | Tâche | Agent | Fichier(s) |
|---|-------|-------|------------|
| 2.1 | Friction de Darcy (Swamee-Jain) | Backend | `solver/steady_state.rs` |
| 2.2 | Résistance hydraulique d'un tuyau | Backend | `solver/steady_state.rs` |
| 2.3 | Boucle Newton-Raphson (Picard simplifié d'abord) | Backend | `solver/steady_state.rs` |
| 2.4 | Upgrade vers vrai Newton-Raphson avec Jacobien (faer) | Backend | `solver/steady_state.rs` |
| 2.5 | Validation analytique : réseau 2 nœuds | Science | `docs/science/validation.md` |
| 2.6 | Validation : réseau en Y (3 branches) | Science | `docs/science/validation.md` |
| 2.7 | Exécution sur GasLib-11 complet | Backend | `main.rs` |

### Tests automatiques

```bash
# T2-1 : Friction de Darcy dans la plage réaliste
cargo test darcy_friction_turbulent

# T2-2 : Réseau 2 nœuds converge, pression source correcte
cargo test steady_state_two_nodes

# T2-3 : Réseau en Y — conservation de masse
cargo test steady_state_y_network
# ΣQ_in = ΣQ_out à chaque nœud (tolérance 1e-6)

# T2-4 : GasLib-11 — le solveur converge
cargo test test_solve_gaslib_11
# Converge en < 100 itérations, toutes pressions > 0

# T2-5 : Benchmark
cargo bench -- steady_state
```

### Critères de passage

- Réseau 2 nœuds : erreur pression < 0.1 bar vs analytique
- GasLib-11 : convergence, toutes pressions ∈ [1, 100] bar
- Résidu < 1e-4

---

## Phase 3 : API REST + Frontend CesiumJS (jours 10-14)

### Tâches

| # | Tâche | Agent | Fichier(s) |
|---|-------|-------|------------|
| 3.1 | Endpoint `/api/health` | Backend | `api/mod.rs` |
| 3.2 | Endpoint `/api/network` (JSON nœuds + tuyaux) | Backend | `api/mod.rs` |
| 3.3 | Endpoint `/api/simulate` (exécuter le solveur) | Backend | `api/mod.rs` |
| 3.4 | Tests d'intégration API (reqwest) | Backend | `tests/api_test.rs` |
| 3.5 | `npm install` + résoudre les dépendances | Frontend | `package.json` |
| 3.6 | Boot CesiumJS (globe vide, assets copiés) | Frontend | `boot/cesium.ts` |
| 3.7 | CesiumViewer : afficher les nœuds GasLib sur le globe | Frontend | `CesiumViewer.vue` |
| 3.8 | CesiumViewer : tracer les tuyaux entre nœuds | Frontend | `CesiumViewer.vue` |
| 3.9 | SimulationPanel : appel API + affichage résultats | Frontend | `SimulationPanel.vue` |
| 3.10 | Coloration dynamique des tuyaux par débit | Frontend | `CesiumViewer.vue` |

### Tests automatiques

```bash
# T3-1 : API health check
cargo test test_api_health

# T3-2 : API network retourne le bon nombre de nœuds
cargo test test_api_network_count

# T3-3 : Frontend build sans erreur
cd front && npm run build

# T3-4 : Frontend unit tests (stores)
cd front && npx vitest run
```

### Critères de passage

- Le globe CesiumJS affiche les 11 nœuds de GasLib-11 aux bonnes coordonnées GPS
- Les tuyaux sont tracés entre les nœuds connectés
- Un clic sur "Lancer la simulation" affiche les pressions/débits
- Les tuyaux changent de couleur selon le débit

---

## Phase 4 : Intégration complète + polish (jours 15-18)

### Tâches

| # | Tâche | Agent | Fichier(s) |
|---|-------|-------|------------|
| 4.1 | Sliders de demande aux nœuds puits | Frontend | `DemandControls.vue` |
| 4.2 | POST `/api/simulate` avec demandes custom | Backend | `api/mod.rs` |
| 4.3 | Légende de couleurs (pression / débit) | Frontend | `components/Legend.vue` |
| 4.4 | Thème sombre SCADA | Frontend | `css/app.scss` |
| 4.5 | Script CI complet | DevOps | `scripts/ci.sh` |
| 4.6 | Documentation architecture finale | Science | `docs/architecture/` |
| 4.7 | Support GasLib-24 (24 nœuds, réseau plus complexe) | Backend | `gaslib/parser.rs` |

### Tests automatiques

```bash
# T4-1 : CI complète
./scripts/ci.sh
# cargo test + npm run build + npm run test

# T4-2 : GasLib-24 converge
cargo test test_solve_gaslib_24

# T4-3 : API avec demandes custom
cargo test test_api_simulate_with_demands
```

---

## Résumé des jalons

| Jalon | Livrable | Vérification |
|-------|----------|-------------|
| M0 | Monorepo compilable | `cargo check` + `npm install` |
| M1 | GasLib-11 parsé en graphe | 11 nœuds, snapshot insta |
| M2 | Simulation régime permanent | Convergence GasLib-11 |
| M3 | Globe CesiumJS + résultats | Tuyaux colorés, panel résultats |
| M4 | MVP complet | CI verte, demandes interactives |

---

## Risques et mitigations

| Risque | Impact | Mitigation |
|--------|--------|------------|
| Parsing XML GasLib complexe (namespaces) | Bloquant P1 | Commencer par GasLib-11 (le plus simple), tests insta |
| Solveur ne converge pas | Bloquant P2 | Picard d'abord (simple), Newton-Raphson ensuite |
| CesiumJS lourd (bundle > 50 MB) | Lenteur frontend | Static copy, lazy loading, tree-shaking widgets |
| Coordonnées GPS absentes dans GasLib | Pas de carte | Utiliser les coordonnées `x`, `y` comme fallback projeté |
| Stabilité numérique grandes matrices | Résultats faux | Non-dimensionnalisation, faer pour les matrices creuses |
