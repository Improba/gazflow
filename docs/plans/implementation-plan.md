# Plan d'implémentation — GazFlow MVP

> Note de convention : la version de travail locale des plans est dans `docs/temps/plans/` (non versionnée).
> `docs/plans/` est réservé aux plans partagés.

## Objectif

Simuler l'écoulement en régime permanent sur un petit réseau GasLib (GasLib-11, 11 nœuds)
et visualiser les résultats (pressions, débits) **en temps réel** sur un globe CesiumJS,
avec streaming des logs du solveur et mise à jour progressive de la carte 3D.
Le MVP doit aussi permettre **l'export des résultats** (JSON/CSV) et garantir une
**expérience fluide** (interaction carte + panneau sans lag perceptible).

## Exigences transverses (non négociables)

- **Export des résultats** : chaque simulation convergée doit pouvoir être exportée
  avec pressions, débits, métadonnées (timestamp, scénario/demandes, unités, itérations, résidu).
- **Fluidité UX** : navigation Cesium (pan/zoom/rotate) et updates live doivent rester fluides
  (pas de freezes UI, pas de backlog WS visible côté utilisateur).
- **Lisibilité opérationnelle** : légendes, unités et états (running/converged/cancelled/error)
  doivent rester visibles même en charge.

---

## Phase 0 : Bootstrap (jour 1) ✅

### Tâches

- [x] Créer la structure monorepo (`back/`, `front/`, `docs/`)
- [x] Initialiser le projet Rust (Cargo.toml, modules)
- [x] Initialiser le projet Quasar + CesiumJS
- [x] Écrire AGENTS.md
- [x] Docker Compose (back + front, volumes partagés)
- [x] Premier `cargo check` sans erreur
- [x] Premier `npm install` + `quasar build` sans erreur
- [ ] Télécharger GasLib-11 dans `back/dat/`

### Tests automatiques

```bash
# T0-1 : Le backend compile
cd back && cargo check

# T0-2 : Le frontend build
cd front && npm install && npx quasar build
```

---

## Phase 1 : Parseur GasLib + Graphe (jours 2-4)

### Données source

- **GasLib-11** : 11 nœuds, ~12 tuyaux, 1 station de compression, coordonnées GPS.
- Téléchargement : <https://gaslib.zib.de/testData.html>
- Format : XML avec namespaces `framework:`, conforme aux XSD GasLib.

### Tâches

| # | Tâche | Agent | Fichier(s) | Status |
|---|-------|-------|------------|--------|
| 1.1 | Script de téléchargement GasLib | DevOps | `scripts/fetch_gaslib.sh` | ✅ |
| 1.2 | Parseur XML : nœuds (source, sink, innode) | Backend | `gaslib/parser.rs` | ✅ |
| 1.3 | Parseur XML : connexions (pipe, valve, shortPipe) | Backend | `gaslib/parser.rs` | ✅ |
| 1.4 | Parseur XML : compressorStation | Backend | `gaslib/parser.rs` | ✅ |
| 1.5 | Parseur XML : scénarios (.scn) — demandes aux nœuds | Backend | `gaslib/scenario.rs` | ✅ |
| 1.6 | Construction du GasNetwork depuis les données parsées | Backend | `graph/mod.rs` | ✅ |
| 1.7 | Snapshot tests insta du parseur | Backend | `gaslib/parser.rs`, `gaslib/snapshots/` | ✅ |

### Tests automatiques

```bash
cargo test test_parse_gaslib_11        # T1-1 : charge sans panique
cargo test test_gaslib_11_topology     # T1-2 : 11 nœuds, ~12 connexions
cargo test test_gaslib_11_snapshot     # T1-3 ✅ : insta::assert_yaml_snapshot!
cargo test test_all_nodes_have_gps     # T1-4 : coordonnées GPS valides
cargo test test_parse_scenario_scn     # T1-5 : demandes parsées
```

---

## Phase 2 : Solveur régime permanent (jours 5-9)

### Fondements mathématiques

Voir `docs/science/equations.md`.
Le protocole de validation scientifique détaillé est défini dans cette phase
(section "Protocole de validation scientifique détaillé (v1)").

> **⚠️ Scaling : la tâche 2.4 (Newton complet avec Jacobien creux) est un prérequis
> pour la Phase 3.** Le solveur Jacobi diagonal (2.3) converge sur GasLib-11 mais
> divergera sur des réseaux plus grands ou plus couplés. Ne pas passer en Phase 3
> sans un Newton fonctionnel sur GasLib-11.

### Tâches

| # | Tâche | Agent | Fichier(s) | Status |
|---|-------|-------|------------|--------|
| 2.1 | Friction de Darcy (Swamee-Jain) | Backend | `solver/steady_state.rs` | ✅ |
| 2.2 | Résistance hydraulique d'un tuyau | Backend | `solver/steady_state.rs` | ✅ |
| 2.3 | Newton-Raphson diagonal (Jacobi) | Backend | `solver/steady_state.rs` | ✅ |
| 2.4 | **🔴 CRITIQUE : Newton-Raphson complet (MVP dense implémenté), migration Jacobien creux (faer) restante** | Backend | `solver/newton.rs` | 🟨 partiel |
| 2.5 | **Équation d'état du gaz (densité = f(P, T))** | Backend | `solver/gas_properties.rs` | ⬜ |
| 2.6 | **Non-dimensionnalisation des variables** | Backend | `solver/steady_state.rs` | ⬜ |
| 2.7 | Validation analytique : réseau 2 nœuds | Science | `docs/science/validation.md` | ✅ |
| 2.8 | Validation : réseau en Y (conservation de masse) | Science | `docs/science/validation.md` | ✅ |
| 2.9 | Exécution sur GasLib-11 complet | Backend | `main.rs` | ✅ |
| 2.10 | **Validation contre solutions de référence GasLib-11 (.sol)** | Science | `solver/steady_state.rs`, `docs/science/validation.md` | 🟨 partiel (test/scaffold prêt, `.sol` local absent) |
| 2.11 | **Line search (backtracking) + fallback hybride Newton/Jacobi** | Backend | `solver/newton.rs` | ✅ |
| 2.12 | **Documenter les conversions d'unités (Pa²→bar², ρ_eff) dans equations.md** | Science | `docs/science/equations.md` | ✅ |
| 2.13 | **Warm-start : initialiser Newton depuis la solution précédente** | Backend | `solver/steady_state.rs` | ✅ |
| 2.14 | **Modélisation valves (K≈0 ouvert, arc supprimé fermé) et shortPipes** | Backend | `solver/steady_state.rs`, `graph/mod.rs` | 🟨 partiel (ouvert/shortPipe OK, fermé TODO) |
| 2.15 | **Compresseurs : ignorer gracieusement (log warning, traiter comme pipe K≈0)** | Backend | `solver/steady_state.rs` | ✅ |
| 2.16 | **Exécuter le protocole de validation scientifique v1 (T1→T10) et publier un rapport Go/No-Go** | Science + Backend | `docs/plans/implementation-plan.md`, `docs/science/validation.md` | 🟨 partiel (rapport intermédiaire publié, T9 bloqué) |

### Tests automatiques

```bash
cargo test darcy_friction_turbulent                  # T2-1 ✅
cargo test steady_state_two_nodes                    # T2-2 ✅
cargo test steady_state_y_network_mass_conservation  # T2-3 ✅
cargo test pipe_resistance_positive                  # T2-4 ✅
cargo test test_solve_gaslib_11                      # T2-5 ✅
cargo test test_newton_vs_jacobi_same_result         # T2-6 ✅
cargo bench -- steady_state                          # T2-7 ⬜
cargo test test_gaslib_11_vs_reference_solution      # T2-8 🟨 (test prêt, skip si `.sol` absent ; cible < 5% MVP, < 1% post-upgrade)
cargo test test_newton_line_search_convergence       # T2-9 ✅ (Newton converge même avec init éloigné)
cargo test test_newton_jacobi_hybrid_fallback        # T2-10 ✅ (fallback Jacobi si line search échoue)
cargo test test_warm_start_fewer_iterations          # T2-11 ✅ (warm-start converge en ≤ 5 iter vs ~20 cold)
cargo test test_valve_open_zero_resistance            # T2-12 ✅ (valve ouverte : ΔP ≈ 0)
cargo test test_compressor_ignored_with_warning       # T2-13 ✅ (compresseur → warning + K≈0)
cargo test test_units_scn_to_si                       # T2-14 ✅ (conversion d'unités scénario vers SI)
cargo test test_pressure_drop_dimension_consistency   # T2-15 ✅ (cohérence dimensionnelle SI <-> bar²)
# T2-16 🟨 : rapport intermédiaire publié dans docs/science/validation.md (Go/No-Go final après T9)
cargo test test_sensitivity_physical_trends           # T2-17 ✅ (tendances physiques monotones)
```

### Protocole de validation scientifique détaillé (v1)

**Objectif :** qualifier la solidité scientifique du solveur stationnaire avant
de passer aux phases UI/perf.

#### Pré-conditions

- `./scripts/dev.sh`
- `./scripts/back-shell.sh`
- données GasLib présentes dans `back/dat/`

#### Tests, critères et statut

| ID | Test | Commande | Critère d'acceptation | Statut |
|---|---|---|---|---|
| T1 | Friction Darcy en turbulent | `cargo test darcy_friction_turbulent` | Test passe, facteur de friction dans une plage physique réaliste | ✅ |
| T2 | Résistance de tuyau positive/finie | `cargo test pipe_resistance_positive` | Test passe, `K > 0` et fini | ✅ |
| T3 | Cas analytique 2 nœuds | `cargo test steady_state_two_nodes` | Pression source ~fixe, pression aval positive et < amont | ✅ |
| T4 | Réseau en Y: conservation locale | `cargo test steady_state_y_network_mass_conservation` | `\|Q_SJ - Q_JA - Q_JB\| < 1e-4` | ✅ |
| T5 | Hybride vs Jacobi | `cargo test test_newton_vs_jacobi_same_result` | Pressions proches, itérations hybride <= Jacobi sur le cas test | ✅ |
| T6 | Sanity check GasLib-11 | `cargo test test_solve_gaslib_11` | Convergence, pressions finies/positives, cardinalités cohérentes | ✅ |
| T7 | Conversion unités scénario -> SI | `cargo test test_units_scn_to_si` | Erreur relative de conversion < `1e-6` | ✅ |
| T8 | Cohérence dimensionnelle chute de pression | `cargo test test_pressure_drop_dimension_consistency` | Équivalence SI <-> bar² dans la tolérance numérique | ✅ |
| T9 | Validation vs référence GasLib `.sol` | `cargo test test_gaslib_11_vs_reference_solution` | MVP: erreur max pression < 5%; post-upgrade: < 1% | 🟨 (test prêt, dataset `.sol` manquant localement) |
| T10 | Sensibilité physique (rugosité, Z, T) | `cargo test test_sensitivity_physical_trends` | Tendances monotones physiques cohérentes | ✅ |

#### Ordre d'exécution recommandé

1. **Base équations** : T1 -> T4
2. **Solveur** : T5 -> T6
3. **Qualité scientifique** : T7 -> T10

#### Gate Go/No-Go

- **No-Go immédiat** si un test T1-T6 échoue.
- **Go MVP scientifique** si T1-T8 + T9(MVP) passent (seuil `< 5%`).
- **Go robuste** si T1-T10 + T9(post-upgrade) passent (seuil `< 1%`).

#### Livrable attendu (tâche 2.16)

Publier un rapport court dans `docs/science/validation.md` contenant :

- date et commit testés;
- statut Pass/Fail T1..T10;
- métriques de T9 (erreur max, moyenne, nœud le plus en écart);
- décision explicite: **Go** ou **No-Go** pour sortie de Phase 2.

---

## Phase 3 : WebSocket + Interface live (jours 10-16)

### Architecture de communication

```
Frontend                          Backend
┌──────────┐  WS /api/ws/sim   ┌─────────────┐
│ SimPanel  │◄═══════════════►│ Axum WS     │
│ LogPanel  │  { type, data }  │ handler     │
│ CesiumMap │                  └──────┬──────┘
└──────────┘                         │ mpsc channel
                               ┌─────▼──────┐
                               │ Solver      │
                               │ (spawn_blocking + rayon) │
                               └─────────────┘
```

**Protocole WebSocket (JSON) :**

```jsonc
// Client → Serveur : lancer une simulation
{ "type": "start_simulation", "demands": { "sink_1": -10.0 } }

// Client → Serveur : annuler la simulation en cours
{ "type": "cancel_simulation" }

// Serveur → Client : progression à chaque itération
{ "type": "iteration", "iter": 5, "residual": 0.0023, "elapsed_ms": 12 }

// Serveur → Client : résultats intermédiaires (toutes les N itérations)
{ "type": "snapshot", "pressures": {...}, "flows": {...} }

// Serveur → Client : convergence atteinte
{ "type": "converged", "result": {...}, "total_ms": 45 }

// Serveur → Client : simulation annulée (par le client ou par timeout)
{ "type": "cancelled", "reason": "client_request" | "timeout" | "diverged" }

// Serveur → Client : erreur (fatal=true → connexion fermée, fatal=false → peut relancer)
{ "type": "error", "message": "...", "fatal": false }
```

### Tâches

| # | Tâche | Agent | Fichier(s) | Status |
|---|-------|-------|------------|--------|
| 3.1 | WebSocket handler Axum | Backend | `api/ws.rs` | ✅ |
| 3.2 | Solver avec callback de progression | Backend | `solver/steady_state.rs` | ✅ |
| 3.3 | `tokio::spawn_blocking` pour le solveur | Backend | `api/ws.rs` | ✅ |
| 3.4 | Channel `mpsc` : solver → WS → client | Backend | `api/ws.rs` | ✅ |
| 3.5 | Endpoint REST `/api/network` | Backend | `api/mod.rs` | ✅ |
| 3.6 | Tests d'intégration API (reqwest + WS) | Backend | `tests/api_test.rs` | ✅ |
| 3.7 | WebSocket client (composable Vue) | Frontend | `services/ws.ts` | ✅ |
| 3.8 | LogPanel : logs du solveur en temps réel | Frontend | `components/LogPanel.vue` | ✅ |
| 3.9 | CesiumViewer : afficher nœuds + tuyaux | Frontend | `CesiumViewer.vue` | ✅ |
| 3.10 | CesiumViewer : mise à jour live des couleurs | Frontend | `CesiumViewer.vue` | ✅ |
| 3.11 | SimulationPanel : start/stop via WebSocket | Frontend | `SimulationPanel.vue` | ✅ |
| 3.12 | Barre de progression + indicateur de résidu | Frontend | `components/ProgressBar.vue` | ✅ |
| 3.13 | **Annulation de simulation (CancellationToken + timeout)** | Backend | `api/ws.rs`, `solver/steady_state.rs` | ✅ |
| 3.14 | **Backpressure WS : buffer borné + drop de snapshots intermédiaires** | Backend | `api/ws.rs` | ✅ |
| 3.15 | **Fluidité live : throttling des snapshots UI (max ~10 Hz) + coalescing des messages** | Frontend | `services/ws.ts`, `stores/simulate.ts` | ✅ |
| 3.16 | **Mesures perf UI dev : FPS map + temps de rendu update (overlay debug activable)** | Frontend | `CesiumViewer.vue` | ✅ |

### Interface cible

```
┌────────────────────────────────────────────────────────┐
│ GazFlow                                 [▶ Start] [⏹] │
├──────────────────────────────────┬─────────────────────┤
│                                  │ Simulation           │
│                                  │ ████████░░ 80%       │
│       Globe CesiumJS             │ Iter: 42 / 100       │
│   (tuyaux colorés en live,       │ Résidu: 2.3e-4       │
│    nœuds avec pression)          │ Temps: 34ms          │
│                                  ├─────────────────────┤
│                                  │ Logs                 │
│                                  │ [42] res=2.3e-4      │
│                                  │ [41] res=5.1e-4      │
│                                  │ [40] res=1.2e-3      │
│                                  │ ...                  │
│                                  ├─────────────────────┤
│                                  │ Pressions (bar)      │
│                                  │ S: 70.00  J: 68.45   │
│                                  │ A: 65.12  B: 66.30   │
│                                  ├─────────────────────┤
│                                  │ Débits (m³/s)        │
│                                  │ SJ: 10.0  JA: 5.2    │
│                                  │ JB: 4.8              │
└──────────────────────────────────┴─────────────────────┘
```

### Tests automatiques

```bash
cargo test test_ws_start_simulation    # T3-1 ✅ : WS connecte et reçoit iterations
cargo test test_ws_start_simulation    # T3-2 ✅ : message "converged" reçu
cargo test test_api_network_count      # T3-3 ✅ : REST network OK
# (Tests d'intégration également dans back/tests/api_test.rs)
cd front && npx vitest run             # T3-4 🟨 : suite frontend initialisée (ws service)
cd front && npx quasar build           # T3-5 ✅ : build sans erreur
cargo test test_ws_cancel_simulation   # T3-6 ✅ : cancel mid-solve, reçoit "cancelled"
cargo test test_ws_timeout_diverged    # T3-7 ✅ : solveur qui diverge → timeout auto
```

---

## Phase 4 : Multi-threading + performance + scaling (jours 17-22)

### Architecture multi-thread

```
┌───────────────────────────────────────────────────────────┐
│                    tokio runtime                           │
│  ┌─────────────┐  ┌─────────────┐  ┌───────────┐         │
│  │ Axum HTTP   │  │ Axum WS     │  │ Axum WS   │         │
│  │ /api/network│  │ client #1   │  │ client #2 │         │
│  └─────────────┘  └──────┬──────┘  └─────┬─────┘         │
│                          │                │               │
│            ┌─────────────▼────────────────▼─────┐         │
│            │       spawn_blocking pool          │         │
│            │    (borné par Semaphore, max N)     │         │
│            │  ┌──────────────────────────────┐  │         │
│            │  │     Solver (1 par simulation) │  │         │
│            │  │  ┌────────┐ ┌────────┐       │  │         │
│            │  │  │ Rayon  │ │ Rayon  │ ...   │  │         │
│            │  │  │ thread │ │ thread │       │  │         │
│            │  │  └────────┘ └────────┘       │  │         │
│            │  └──────────────────────────────┘  │         │
│            └────────────────────────────────────┘         │
└───────────────────────────────────────────────────────────┘
```

### Stratégie de scaling par taille de réseau

| Taille réseau | Solveur recommandé | Parallélisme |
|---|---|---|
| ≤ 50 nœuds | Newton + LU direct creux (faer) | Séquentiel (overhead Rayon > gain) |
| 50–2000 nœuds | Newton + LU direct creux (faer) | Rayon `par_iter` sur assemblage résidu/Jacobien |
| 2000–5000 nœuds | Newton + GMRES préconditionné ILU (stretch) | Rayon + solveur itératif |
| > 5000 nœuds | Hors scope MVP — nécessite domain decomposition | — |

> **Note :** Le LU creux sur un Jacobien de réseau de gaz (~3 non-zéros par ligne)
> a une complexité effective O(N^{1.2} à N^{1.5}) grâce à l'ordering AMD, bien
> inférieure au O(N³) du LU dense. Le seuil GMRES ne s'applique que si le profiling
> (tâche 4.9) révèle que la factorisation LU devient le bottleneck.

### Tâches

| # | Tâche | Agent | Fichier(s) | Status |
|---|-------|-------|------------|--------|
| 4.1 | Vérifier que `spawn_blocking` (3.3) + Rayon ne causent pas de contention | Backend | `api/ws.rs` | ⬜ |
| 4.2 | Rayon `par_iter` sur les pipes (résidu + Jacobien), seuil ≥ 50 pipes | Backend | `solver/steady_state.rs` | ⬜ |
| 4.3 | Assemblage Jacobien creux parallèle (faer) | Backend | `solver/newton.rs` | ⬜ |
| 4.4 | Benchmark Criterion : Jacobi vs Newton, 1 thread vs N | Backend | `benches/solver_bench.rs` | ⬜ |
| 4.5 | Simulations concurrentes (plusieurs WS clients) | Backend | `api/ws.rs`, `tests/api_test.rs` | ✅ |
| 4.6 | Support GasLib-24 + GasLib-40 | Backend | `gaslib/parser.rs` | 🟨 partiel (parse + smoke tests, datasets optionnels) |
| 4.7 | Benchmark sur GasLib-135 (stress test) | Backend | `benches/solver_bench.rs` | ⬜ |
| 4.8 | **Sémaphore : limiter les simulations concurrentes (max N configurable)** | Backend | `api/ws.rs` | ✅ |
| 4.9 | **Profiling flamegraph intégré (tracing + inferno ou perf)** | Backend | `benches/`, `scripts/profile.sh` | ⬜ |
| 4.10 | **Support GasLib-582 + GasLib-4197 (cibles de scaling)** | Backend | `gaslib/parser.rs`, `scripts/fetch_gaslib.sh` | ⬜ |
| 4.11 | **🔵 STRETCH : Solveur itératif GMRES + préconditionneur ILU (si LU creux insuffisant au-delà de ~2000 nœuds)** | Backend | `solver/iterative.rs` | ⬜ |
| 4.12 | **Benchmark scaling : temps vs N nœuds (11, 24, 40, 135, 582, 4197)** | Backend | `benches/scaling_bench.rs` | ⬜ |

### Tests automatiques

```bash
cargo test test_parallel_solver_same_result    # T4-1 : même résultat 1 vs N threads
cargo test test_concurrent_simulations         # T4-2 ✅ : 2 WS clients simultanés
cargo bench -- steady_state                    # T4-3 : Jacobi vs Newton perf
cargo test test_solve_gaslib_24                # T4-4 🟨 : smoke test en place (skip si dataset absent)
cargo test test_solve_gaslib_40                # T4-5 🟨 : smoke test en place (skip si dataset absent)
cargo test test_semaphore_rejects_overflow     # T4-6 ✅ : N+1ème simulation reçoit un rejet explicite
cargo test test_solve_gaslib_582               # T4-7 ⬜ : convergence GasLib-582 (Newton+LU)
cargo test test_solve_gaslib_4197              # T4-8 ⬜ : convergence GasLib-4197 (GMRES+ILU)
cargo bench -- scaling                         # T4-9 ⬜ : courbe temps vs N nœuds
```

---

## Phase 5 : Intégration complète + polish (jours 23-28)

Référence contrat d'export : `docs/architecture/export-contract.md` (source de vérité API/format).

### Tâches

| # | Tâche | Agent | Fichier(s) | Status |
|---|-------|-------|------------|--------|
| 5.1 | Sliders de demande aux nœuds puits | Frontend | `DemandControls.vue` | ⬜ |
| 5.2 | POST `/api/simulate` avec demandes custom (REST fallback) | Backend | `api/mod.rs` | ⬜ |
| 5.3 | Légende de couleurs (gradient pression / débit) | Frontend | `components/Legend.vue` | ⬜ |
| 5.4 | Sélection d'un nœud → popup avec pression, voisins | Frontend | `CesiumViewer.vue` | ⬜ |
| 5.5 | Thème sombre SCADA (palette industrielle) | Frontend | `css/app.scss` | ⬜ |
| 5.6 | Script CI complet via Docker | DevOps | `scripts/ci.sh` | ✅ |
| 5.7 | Documentation architecture finale | Science | `docs/architecture/` | ⬜ |
| 5.8 | **LOD CesiumJS : clustering de nœuds à faible zoom (> 200 entités)** | Frontend | `CesiumViewer.vue` | ⬜ |
| 5.9 | **Primitives WebGL pour grands réseaux (PolylineCollection au lieu d'entités)** | Frontend | `CesiumViewer.vue` | ⬜ |
| 5.10 | **Warm-start via slider : réutiliser la solution précédente quand la demande change** | Frontend + Backend | `DemandControls.vue`, `api/ws.rs` | ⬜ |
| 5.11 | **Export backend : endpoint d'export des résultats (`json`, `csv`) avec métadonnées (conforme contrat v1)** | Backend | `api/mod.rs`, `api/export.rs` | ⬜ |
| 5.12 | **Export frontend : boutons "Exporter JSON/CSV" dans le panel simulation (état `exporting` non bloquant)** | Frontend | `components/SimulationPanel.vue` | ⬜ |
| 5.13 | **Export complet : bundle `.zip` optionnel (résultats + logs + contexte simulation, contrat v1)** | Frontend + Backend | `components/SimulationPanel.vue`, `api/export.rs` | ⬜ |
| 5.14 | **Fluidité UI en charge : virtualisation listes + debounce sliders + budget frame-time** | Frontend | `components/`, `stores/` | ⬜ |

### Tests automatiques

```bash
cargo test test_export_result_json_schema      # T5-1 : JSON export contient données + métadonnées + unités
cargo test test_export_result_csv_headers      # T5-2 : CSV export colonnes stables et parseables
cd front && npx vitest run                     # T5-3 : bouton export visible/actif selon état simulation
cd front && npx playwright test                # T5-4 : scénario E2E export + fluidité navigation map
```

---

## Résumé des jalons

| Jalon | Livrable | Vérification |
|-------|----------|-------------|
| M0 | Monorepo compilable, Docker | `cargo check` + `quasar build` | ✅ |
| M1 | GasLib-11 parsé en graphe | 11 nœuds, snapshot insta | ✅ |
| M2 | Simulation régime permanent + Newton complet + validation référence | Tests T2-1..T2-13 + protocole scientifique v1 (Go/No-Go), erreur < 5% vs .sol | ✅ partiel |
| M3 | **WebSocket live + logs + carte temps réel + annulation** | Simulation visible en live, cancel fonctionne |
| M4 | **Multi-threading + scaling vérifié** | GasLib-135 < 100ms, GasLib-582 converge, courbe scaling documentée |
| M4+ | **Solveur itératif (stretch goal)** | GasLib-4197 converge avec GMRES+ILU |
| M5 | MVP complet + LOD + export résultats | CI verte, demandes interactives, export JSON/CSV, 4000 entités sans lag |

---

## Risques et mitigations

| Risque | Impact | Mitigation |
|--------|--------|------------|
| Parsing XML GasLib complexe (namespaces) | Bloquant P1 | ✅ Résolu : `alias` serde |
| Solveur ne converge pas | Bloquant P2 | ✅ Jacobi converge (8 tests) |
| Newton complet instable (singularité Jacobien) | P2/P4 | Fallback Jacobi, régularisation + **line search backtracking + hybride** (tâche 2.11) |
| Écart vs solutions de référence GasLib > 5% | P2 | Upgrade ρ(P,T) et Z (tâche 2.5), puis viser < 1% |
| CesiumJS lourd (bundle > 50 MB) | Lenteur frontend | Static copy, lazy loading |
| WebSocket déconnexion pendant simulation | P3 | Reconnexion automatique + cache résultat |
| Rayon dans spawn_blocking : contention | P4 | Benchmark systématique, pool sizing |
| GasLib-135+ : solveur lent | P4 | Matrices creuses faer, profilage |
| **Simulation divergente bloque un slot indéfiniment** | P3/P4 | Timeout configurable + CancellationToken (tâche 3.13) |
| **Simulations concurrentes saturent la mémoire / CPU** | P4 | Sémaphore borné (tâche 4.8), rejet gracieux si plein |
| **LU creux trop lent pour N > ~2000 (fill-in, mémoire)** | P4 | Profiling (4.9) puis GMRES+ILU en fallback si nécessaire (tâche 4.11) |
| **CesiumJS lag avec > 1000 entités individuelles** | P5 | LOD + clustering (5.8) + PolylineCollection (5.9) |
| **Pas de warm-start : chaque slider re-solve from scratch** | P5 | Warm-start (2.13) + protocole WS avec solution initiale (5.10) |
| **Exports incomplets/incohérents (unités, métadonnées manquantes)** | P5 | Contrat d'export versionné + tests T5-1/T5-2 + exemples documentés |
| **UX non fluide en conditions réelles (backlog snapshots, jank UI)** | P3/P5 | Throttling/coalescing (3.15), LOD/primitives (5.8/5.9), budget frame-time (5.14) |
