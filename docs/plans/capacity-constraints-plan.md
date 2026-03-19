# Plan d'implémentation — Capacités d'entrée/sortie

> Extension du solveur GazFlow pour intégrer les contraintes de capacité (min/max débit) aux nœuds d'entrée/sortie, via un solveur interior point spécialisé.

## Vue d'ensemble

### Problème actuel

Le solveur prend des **débits fixes** par nœud et calcule les pressions résultantes. Si un scénario viole les capacités physiques ou contractuelles d'un point d'entrée/sortie, l'utilisateur ne le sait pas.

### Objectif

Intégrer les bornes de capacité $Q_i^{\min} \leq d_i \leq Q_i^{\max}$ dans le modèle, avec deux modes :

1. **Vérification** : résoudre normalement, puis diagnostiquer les violations.
2. **Optimisation** : ajuster automatiquement les débits pour respecter les bornes tout en restant le plus proche possible du scénario demandé.

### Formulation mathématique (mode optimisation)

$$
\min_{\boldsymbol{\pi},\, \mathbf{d}} \quad \sum_{i \in \mathcal{B}} (d_i - d_i^{\text{cible}})^2
$$

$$
\text{s.c.} \quad F_i(\boldsymbol{\pi}, \mathbf{d}) = 0 \quad \forall \text{ nœud libre } i
$$

$$
d_i^{\min} \leq d_i \leq d_i^{\max} \quad \forall i \in \mathcal{B}
$$

Résolu par **méthode du point intérieur** (barrière log) avec élimination du complément de Schur, réduisant le système KKT au Jacobien existant + termes diagonaux.

### Références

- Wächter & Biegler (2006). Interior-point filter line-search for large-scale NLP. *Math. Prog.*, 106(1).
- Nocedal & Wright (2006). *Numerical Optimization*, 2e éd. Springer. Chap. 17–19.
- Koch et al. (2015). *Evaluating Gas Network Capacities*. SIAM MOS.
- Ríos-Mercado & Borraz-Sánchez (2015). Optimization in gas transport. *Applied Energy*, 147.

---

## Architecture des phases

| Phase | Objectif | Dépendances |
|-------|----------|-------------|
| **P0** | Modèle de données + parser | Aucune |
| **P1** | Vérification post-simulation | P0 |
| **P2** | Solveur interior point | P0 |
| **P3** | Intégration API + WebSocket | P1, P2 |
| **P4** | Frontend | P3 |
| **P5** | Tests d'intégration + validation | P4 |

---

## Phase 0 — Modèle de données et parser

### Objectif

Enrichir `Node` avec les bornes de débit et parser les capacités GasLib.

### Tâches

| # | Tâche | Fichiers | Entrée | Sortie | Tests |
|---|-------|----------|--------|--------|-------|
| **P0-1** | Ajouter `flow_min_m3s: Option<f64>` et `flow_max_m3s: Option<f64>` à `Node` | `back/src/graph/mod.rs` | — | Compilation OK, sérialisation JSON inclut les nouveaux champs | `cargo check` |
| **P0-2** | Parser les balises `<flowMin>` et `<flowMax>` des nœuds GasLib `.net` | `back/src/gaslib/parser.rs` | Fichiers `.net` GasLib-11/24/582 | `flow_min_m3s` / `flow_max_m3s` remplis quand présents dans le XML | T0-1 |
| **P0-3** | Créer `CapacityBounds` struct | `back/src/solver/capacity.rs` (nouveau) | `GasNetwork` + `demands` | `HashMap<usize, (f64, f64)>` des nœuds bornés | T0-2 |
| **P0-4** | Mettre à jour les snapshots insta | `back/src/gaslib/snapshots/` | — | Snapshots à jour avec nouveaux champs | `cargo test` insta |

#### Tests Phase 0

| ID | Test | Type | Description |
|----|------|------|-------------|
| T0-1 | `test_parser_reads_flow_capacity_bounds` | Unitaire | Vérifie que le parser extrait `flow_min_m3s` / `flow_max_m3s` des nœuds source/sink GasLib-11 |
| T0-2 | `test_capacity_bounds_from_network` | Unitaire | Construit un `CapacityBounds` depuis un réseau 2-nœuds avec bornes, vérifie les valeurs |

#### Détails techniques — P0-2

Le XML GasLib peut contenir sur les nœuds `<source>` et `<sink>` :

```xml
<source id="entry01" x="0" y="0">
  <flowMin value="0" unit="1000m_cube_per_hour"/>
  <flowMax value="500" unit="1000m_cube_per_hour"/>
</source>
```

Le parser doit :
1. Ajouter `flow_min` et `flow_max` à `XmlNode` (même pattern que `pressure_min` / `pressure_max`).
2. Convertir les unités en m³/s (réutiliser la logique de `scenario.rs`).
3. Mapper vers `Node.flow_min_m3s` / `Node.flow_max_m3s` dans `load_network`.

---

## Phase 1 — Vérification post-simulation

### Objectif

Après convergence du solveur existant, vérifier les bornes de capacité et produire un diagnostic.

### Tâches

| # | Tâche | Fichiers | Entrée | Sortie | Tests |
|---|-------|----------|--------|--------|-------|
| **P1-1** | Créer `CapacityViolation` struct | `back/src/solver/capacity.rs` | — | `{ node_id, bound_type, limit, actual }` | — |
| **P1-2** | Implémenter `check_capacity_violations()` | `back/src/solver/capacity.rs` | `SolverResult` + `CapacityBounds` | `Vec<CapacityViolation>` | T1-1, T1-2 |
| **P1-3** | Ajouter `capacity_violations: Vec<CapacityViolation>` à `SolverResult` | `back/src/solver/steady_state.rs` | — | Champ optionnel, `#[serde(skip_serializing_if = "Vec::is_empty")]` | T1-3 |

#### Tests Phase 1

| ID | Test | Type | Description |
|----|------|------|-------------|
| T1-1 | `test_no_violation_when_within_bounds` | Unitaire | Scénario dans les bornes → `violations.is_empty()` |
| T1-2 | `test_detects_overflow_violation` | Unitaire | Débit > max → violation détectée avec valeurs correctes |
| T1-3 | `test_solver_result_includes_violations` | Unitaire | `SolverResult` sérialisé en JSON contient le champ quand non vide |

---

## Phase 2 — Solveur interior point spécialisé

### Objectif

Implémenter le cœur du solveur contraint : méthode de barrière logarithmique avec réduction de Schur, réutilisant l'infrastructure Newton existante.

### Architecture

```
solver/
├── mod.rs              ← ajouter `pub(crate) mod capacity;`
├── capacity.rs         ← NEW : CapacityBounds, vérification, solveur contraint
├── newton.rs           ← extraire `evaluate_state` et helpers en pub(crate)
├── steady_state.rs     ← inchangé (fonctions publiques)
├── gas_properties.rs   ← inchangé
└── iterative.rs        ← inchangé
```

### Tâches

| # | Tâche | Fichiers | Entrée | Sortie | Tests |
|---|-------|----------|--------|--------|-------|
| **P2-1** | Extraire `evaluate_state`, `IndexedPipe`, `build_*_map`, `solve_sparse_linear` en `pub(crate)` | `back/src/solver/newton.rs` | — | Fonctions accessibles depuis `capacity.rs` | Tests existants passent |
| **P2-2** | Implémenter la boucle barrière extérieure | `back/src/solver/capacity.rs` | `μ₀`, facteur de réduction, tolérance barrière | Séquence de résolutions Newton avec μ décroissant | T2-1 |
| **P2-3** | Implémenter le calcul des termes barrière diagonaux `Σ_d` | `back/src/solver/capacity.rs` | `d_i`, bornes, `μ` | Vecteur `σ_i` pour chaque nœud borné | T2-2 |
| **P2-4** | Modifier l'assemblage Jacobien pour intégrer les termes `σ_i` | `back/src/solver/capacity.rs` | Triplets Jacobien + `σ_i` | Système augmenté `(J - diag(σ⁻¹))·Δπ = -rhs` | T2-3 |
| **P2-5** | Implémenter le fraction-to-boundary (line search adaptée) | `back/src/solver/capacity.rs` | `Δd`, bornes | `α_max` tel que `d + α·Δd` reste dans les bornes | T2-4 |
| **P2-6** | Implémenter la mise à jour des débits variables `d_i` | `back/src/solver/capacity.rs` | `Δπ`, `σ_i`, gradient objectif | `Δd_i` par substitution Schur | T2-5 |
| **P2-7** | Implémenter la fonction objectif quadratique et son gradient | `back/src/solver/capacity.rs` | `d`, `d_cible` | `f(d)`, `∇f(d)` | T2-6 |
| **P2-8** | Exposer `solve_steady_state_constrained` | `back/src/solver/capacity.rs`, `back/src/solver/mod.rs` | Signature publique complète | Fonction appelable depuis l'API | T2-7 |
| **P2-9** | Ajouter `ConstrainedSolverResult` avec multiplicateurs et marges | `back/src/solver/capacity.rs` | — | Résultat enrichi : `active_bounds`, `multipliers`, `objective_value` | T2-8 |

#### Détails mathématiques — P2-3 et P2-4

Pour chaque nœud borné $i$ avec $d_i^{\min} \leq d_i \leq d_i^{\max}$ :

$$
\sigma_i = \frac{\mu}{(d_i - d_i^{\min})^2} + \frac{\mu}{(d_i^{\max} - d_i)^2} + 2
$$

Le terme $+2$ vient du Hessien de l'objectif quadratique $\|d - d^{\text{cible}}\|^2$.

Le gradient barrière pour le RHS :

$$
b_i = \frac{\mu}{d_i - d_i^{\min}} - \frac{\mu}{d_i^{\max} - d_i} + 2(d_i - d_i^{\text{cible}})
$$

Le système réduit (après élimination de $\Delta d$) :

$$
\bigl(J_{ii} - \sigma_i^{-1}\bigr) \cdot \Delta\pi_i = -F_i + \sigma_i^{-1} \cdot b_i \quad \text{(nœuds bornés)}
$$

$$
J_{ij} \cdot \Delta\pi_j = -F_i \quad \text{(nœuds non bornés, inchangé)}
$$

Puis la récupération de $\Delta d$ :

$$
\Delta d_i = -\sigma_i^{-1} \cdot (\lambda_i + b_i) = -\sigma_i^{-1} \cdot (\Delta\pi_i \cdot J_{ii} + F_i + b_i)
$$

#### Détails techniques — P2-5

Fraction-to-boundary rule : pour garder $d_i$ strictement dans les bornes :

$$
\alpha_{\max} = \min_i \begin{cases}
\frac{\tau \cdot (d_i^{\max} - d_i)}{\Delta d_i} & \text{si } \Delta d_i > 0 \\
\frac{\tau \cdot (d_i - d_i^{\min})}{-\Delta d_i} & \text{si } \Delta d_i < 0
\end{cases}
$$

avec $\tau = 0.995$ (marge de sécurité classique). Le pas final est $\alpha = \min(\alpha_{\max}, \alpha_{\text{backtrack}})$.

#### Signature cible — P2-8

```rust
pub fn solve_steady_state_constrained<F>(
    network: &GasNetwork,
    target_demands: &HashMap<String, f64>,
    capacity_bounds: &HashMap<String, (f64, f64)>,  // node_id → (min, max)
    initial_pressures_bar: Option<&HashMap<String, f64>>,
    max_iter: usize,
    tolerance: f64,
    snapshot_every: usize,
    on_progress: F,
) -> Result<ConstrainedSolverResult>
where
    F: FnMut(SolverProgress) -> SolverControl;
```

#### Tests Phase 2

| ID | Test | Type | Description |
|----|------|------|-------------|
| T2-1 | `test_barrier_loop_reduces_mu` | Unitaire | Vérifie que μ décroît et que la boucle extérieure termine |
| T2-2 | `test_barrier_diagonal_positive` | Unitaire | `σ_i > 0` pour tout `d_i` strictement dans les bornes |
| T2-3 | `test_augmented_jacobian_matches_manual` | Unitaire | Comparer assemblage Jacobien augmenté à un calcul manuel sur réseau 2-nœuds |
| T2-4 | `test_fraction_to_boundary_respects_bounds` | Unitaire | `d + α·Δd` reste dans `[min, max]` pour cas limites |
| T2-5 | `test_demand_update_consistent_with_schur` | Unitaire | `Δd` récupéré par Schur correspond au calcul direct |
| T2-6 | `test_objective_and_gradient` | Unitaire | `f(d_cible) = 0`, `∇f(d_cible) = 0` |
| T2-7 | `test_constrained_solver_two_nodes_within_bounds` | Intégration | Réseau 2-nœuds, bornes larges → résultat ≈ solveur non contraint |
| T2-8 | `test_constrained_solver_two_nodes_clamps_demand` | Intégration | Réseau 2-nœuds, demande > max → débit clampé à max |
| T2-9 | `test_constrained_solver_y_network` | Intégration | Y-network, une sortie bornée → débit redistribué |
| T2-10 | `test_constrained_vs_unconstrained_gaslib11` | Intégration | GasLib-11, bornes larges → résultats quasi-identiques |
| T2-11 | `test_constrained_gaslib11_tight_bounds` | Intégration | GasLib-11, bornes serrées → au moins un nœud actif |

---

## Phase 3 — Intégration API et WebSocket

### Objectif

Exposer les capacités et le solveur contraint via l'API REST et le WebSocket.

### Tâches

| # | Tâche | Fichiers | Entrée | Sortie | Tests |
|---|-------|----------|--------|--------|-------|
| **P3-1** | Enrichir `GET /api/network` avec les bornes de capacité par nœud | `back/src/api/mod.rs` | `Node.flow_min_m3s/flow_max_m3s` | JSON `nodes[].flow_min_m3s`, `flow_max_m3s` | T3-1 |
| **P3-2** | Ajouter `capacity_bounds` à `SimulateRequest` (optionnel) | `back/src/api/mod.rs` | — | `Option<HashMap<String, (f64, f64)>>` | T3-2 |
| **P3-3** | Ajouter `capacity_bounds` à `ClientMessage::StartSimulation` | `back/src/api/ws.rs` | — | Champ optionnel dans le message WS | T3-3 |
| **P3-4** | Router vers `solve_steady_state_constrained` quand `capacity_bounds` est fourni | `back/src/api/mod.rs`, `back/src/api/ws.rs` | Présence de `capacity_bounds` | Appel solveur contraint ou classique | T3-4 |
| **P3-5** | Enrichir les messages WS `converged` avec `capacity_violations` et `active_bounds` | `back/src/api/ws.rs` | `ConstrainedSolverResult` | Nouveaux champs JSON dans le message | T3-5 |
| **P3-6** | Ajouter mode `"check_only"` : résoudre classique + vérifier | `back/src/api/mod.rs`, `back/src/api/ws.rs` | Option `mode: "check" \| "optimize"` | Check → violations, Optimize → solveur contraint | T3-6 |

#### Tests Phase 3

| ID | Test | Type | Description |
|----|------|------|-------------|
| T3-1 | `test_api_network_includes_capacity_bounds` | API | `GET /api/network` retourne `flow_min_m3s` et `flow_max_m3s` |
| T3-2 | `test_api_simulate_with_capacity_bounds` | API | `POST /api/simulate` avec bornes → résultat contraint |
| T3-3 | `test_ws_start_with_capacity_bounds` | WS | Message `start_simulation` avec `capacity_bounds` → simulation démarre |
| T3-4 | `test_ws_constrained_converged_includes_violations` | WS | Message `converged` contient `capacity_violations` et `active_bounds` |
| T3-5 | `test_api_simulate_without_bounds_unchanged` | API | `POST /api/simulate` sans bornes → comportement identique à aujourd'hui |
| T3-6 | `test_ws_check_mode_returns_violations_only` | WS | Mode `check` → résolution classique + violations sans optimisation |

---

## Phase 4 — Frontend

### Objectif

Afficher les capacités dans l'UI, permettre de configurer le mode (vérification / optimisation), et visualiser les résultats contraints.

### Tâches

| # | Tâche | Fichiers | Entrée | Sortie | Tests |
|---|-------|----------|--------|--------|-------|
| **P4-1** | Enrichir `NodeDto` avec `flow_min_m3s`, `flow_max_m3s` | `front/src/stores/network.ts` | API response | Champs disponibles dans le store | T4-1 |
| **P4-2** | Borner les sliders de `DemandControls` aux capacités | `front/src/components/DemandControls.vue` | `node.flow_min_m3s`, `node.flow_max_m3s` | `:min` et `:max` du slider reflètent les bornes | — |
| **P4-3** | Ajouter sélecteur de mode (Vérifier / Optimiser / Libre) | `front/src/components/SimulationPanel.vue` | — | `q-btn-toggle` qui envoie `mode` avec la simulation | — |
| **P4-4** | Enrichir `WsServerMessage` avec `capacity_violations` et `active_bounds` | `front/src/services/ws.ts` | Message serveur | Types TypeScript mis à jour | T4-2 |
| **P4-5** | Enrichir `SimulationResult` avec violations et bornes actives | `front/src/stores/simulate.ts` | Message `converged` | `capacityViolations` et `activeBounds` dans le store | T4-3 |
| **P4-6** | Afficher les violations dans le panneau résultat | `front/src/components/SimulationPanel.vue` | `capacityViolations` | Bannière d'alerte avec liste des nœuds en violation | — |
| **P4-7** | Colorer les nœuds en violation sur la carte 3D | `front/src/components/CesiumMap.vue` (ou équivalent) | `capacityViolations` | Nœuds en rouge/orange quand violation | — |
| **P4-8** | Afficher les bornes actives dans le panneau résultat | `front/src/components/SimulationPanel.vue` | `activeBounds` | Icône 🔒 à côté des nœuds dont la borne est active | — |

#### Tests Phase 4

| ID | Test | Type | Description |
|----|------|------|-------------|
| T4-1 | `test_network_store_loads_capacity_bounds` | Vitest | Store charge `flow_min_m3s` / `flow_max_m3s` depuis l'API |
| T4-2 | `test_ws_types_include_violations` | Vitest | Types WS compilent et gèrent le nouveau champ |
| T4-3 | `test_simulate_store_handles_constrained_result` | Vitest | Store `simulate` expose `capacityViolations` après convergence |

---

## Phase 5 — Tests d'intégration et validation scientifique

### Objectif

Valider le solveur contraint de bout en bout, y compris la cohérence physique et la comparaison avec le solveur non contraint.

### Tâches

| # | Tâche | Fichiers | Entrée | Sortie | Tests |
|---|-------|----------|--------|--------|-------|
| **P5-1** | Test E2E : simulation contrainte via WebSocket | `back/tests/api_test.rs` | GasLib-11 + bornes | Résultat convergé avec violations/bornes actives | T5-1 |
| **P5-2** | Test de régression : bornes larges ≈ non contraint | `back/src/solver/capacity.rs` | GasLib-11, bornes à ±∞ | `\|P_contraint - P_libre\| < ε` | T5-2 |
| **P5-3** | Test de conservation de masse avec contraintes | `back/src/solver/capacity.rs` | Y-network borné | `\|Σ Q_in - Σ Q_out\| < ε` | T5-3 |
| **P5-4** | Test de monotonie de l'objectif | `back/src/solver/capacity.rs` | GasLib-11, bornes serrées | Objectif décroît à chaque itération barrière | T5-4 |
| **P5-5** | Test de performance : pas de régression sur le solveur classique | `back/src/solver/capacity.rs` | GasLib-11 sans bornes | Temps ≈ solveur non contraint (±20%) | T5-5 |
| **P5-6** | Mettre à jour `docs/science/equations.md` section 7 | `docs/science/equations.md` | — | Formulation complète du problème contraint | — |
| **P5-7** | Mettre à jour `docs/testing/README.md` | `docs/testing/README.md` | — | Commandes pour les tests contraints | — |

#### Tests Phase 5

| ID | Test | Type | Description |
|----|------|------|-------------|
| T5-1 | `test_ws_constrained_e2e_gaslib11` | E2E | WS simulation avec bornes, vérifie le message `converged` complet |
| T5-2 | `test_constrained_matches_unconstrained_wide_bounds` | Intégration | Bornes larges → résultats identiques à ±ε |
| T5-3 | `test_constrained_mass_conservation` | Intégration | Conservation de masse < 1e-4 avec bornes actives |
| T5-4 | `test_barrier_objective_monotone` | Intégration | Objectif décroît à chaque pas μ |
| T5-5 | `test_constrained_no_perf_regression` | Bench | Temps de résolution sans bornes ≈ solveur actuel |

---

## Matrice des tâches par sous-agent

Chaque tâche est conçue pour être exécutable par un sous-agent autonome avec un périmètre clair.

### Légende

- **Complexité** : S (small, <50 lignes), M (medium, 50–200 lignes), L (large, >200 lignes)
- **Prérequis** : tâches qui doivent être terminées avant

### Tableau complet

| Tâche | Phase | Domaine | Complexité | Prérequis | Fichiers principaux | Critère de succès |
|-------|-------|---------|------------|-----------|--------------------|--------------------|
| P0-1 | P0 | Backend/Modèle | S | — | `graph/mod.rs` | Compile, `flow_min_m3s`/`flow_max_m3s` dans JSON |
| P0-2 | P0 | Backend/Parser | M | P0-1 | `gaslib/parser.rs` | T0-1 passe |
| P0-3 | P0 | Backend/Solver | S | P0-1 | `solver/capacity.rs` | T0-2 passe |
| P0-4 | P0 | Backend/Test | S | P0-2 | `gaslib/snapshots/` | `cargo test` insta OK |
| P1-1 | P1 | Backend/Solver | S | P0-3 | `solver/capacity.rs` | Struct `CapacityViolation` compilée |
| P1-2 | P1 | Backend/Solver | S | P1-1 | `solver/capacity.rs` | T1-1, T1-2 passent |
| P1-3 | P1 | Backend/Solver | S | P1-2 | `solver/steady_state.rs` | T1-3 passe |
| P2-1 | P2 | Backend/Refactor | M | — | `solver/newton.rs` | Tests existants passent, fonctions `pub(crate)` |
| P2-2 | P2 | Backend/Solver | M | P2-1, P0-3 | `solver/capacity.rs` | T2-1 passe |
| P2-3 | P2 | Backend/Solver | S | P2-2 | `solver/capacity.rs` | T2-2 passe |
| P2-4 | P2 | Backend/Solver | M | P2-3 | `solver/capacity.rs` | T2-3 passe |
| P2-5 | P2 | Backend/Solver | S | P2-2 | `solver/capacity.rs` | T2-4 passe |
| P2-6 | P2 | Backend/Solver | M | P2-4, P2-5 | `solver/capacity.rs` | T2-5 passe |
| P2-7 | P2 | Backend/Solver | S | — | `solver/capacity.rs` | T2-6 passe |
| P2-8 | P2 | Backend/Solver | L | P2-6, P2-7, P1-2 | `solver/capacity.rs`, `solver/mod.rs` | T2-7 à T2-11 passent |
| P2-9 | P2 | Backend/Solver | S | P2-8 | `solver/capacity.rs` | T2-8 compile et sérialise |
| P3-1 | P3 | Backend/API | S | P0-1 | `api/mod.rs` | T3-1 passe |
| P3-2 | P3 | Backend/API | S | P2-8 | `api/mod.rs` | T3-2 passe |
| P3-3 | P3 | Backend/WS | S | P2-8 | `api/ws.rs` | T3-3 passe |
| P3-4 | P3 | Backend/API | M | P3-2, P3-3 | `api/mod.rs`, `api/ws.rs` | T3-4, T3-5 passent |
| P3-5 | P3 | Backend/WS | S | P3-4, P2-9 | `api/ws.rs` | T3-5 passe |
| P3-6 | P3 | Backend/API | M | P3-4, P1-2 | `api/mod.rs`, `api/ws.rs` | T3-6 passe |
| P4-1 | P4 | Frontend/Store | S | P3-1 | `stores/network.ts` | T4-1 passe |
| P4-2 | P4 | Frontend/UI | S | P4-1 | `components/DemandControls.vue` | Slider borné visuellement |
| P4-3 | P4 | Frontend/UI | S | — | `components/SimulationPanel.vue` | Toggle visible et fonctionnel |
| P4-4 | P4 | Frontend/WS | S | P3-5 | `services/ws.ts` | T4-2 passe |
| P4-5 | P4 | Frontend/Store | S | P4-4 | `stores/simulate.ts` | T4-3 passe |
| P4-6 | P4 | Frontend/UI | M | P4-5 | `components/SimulationPanel.vue` | Violations affichées |
| P4-7 | P4 | Frontend/UI | M | P4-5 | `components/CesiumMap.vue` | Nœuds en violation colorés |
| P4-8 | P4 | Frontend/UI | S | P4-5 | `components/SimulationPanel.vue` | Icône bornes actives |
| P5-1 | P5 | Test E2E | M | P3-5 | `tests/api_test.rs` | T5-1 passe |
| P5-2 | P5 | Test validation | S | P2-8 | `solver/capacity.rs` | T5-2 passe |
| P5-3 | P5 | Test validation | S | P2-8 | `solver/capacity.rs` | T5-3 passe |
| P5-4 | P5 | Test validation | S | P2-8 | `solver/capacity.rs` | T5-4 passe |
| P5-5 | P5 | Test perf | S | P2-8 | `solver/capacity.rs` | T5-5 passe |
| P5-6 | P5 | Documentation | M | P2-8 | `docs/science/equations.md` | Section 7 complète |
| P5-7 | P5 | Documentation | S | P5-1 | `docs/testing/README.md` | Commandes documentées |

### Graphe de parallélisme

```
P0-1 ──┬── P0-2 ── P0-4
       ├── P0-3 ── P1-1 ── P1-2 ── P1-3
       └── P3-1 ── P4-1 ── P4-2

P2-1 ──┬── P2-2 ──┬── P2-3 ── P2-4 ──┐
       │          └── P2-5 ───────────┤
       │                              ├── P2-6 ──┐
       │   P2-7 ──────────────────────┘          │
       │                                         ├── P2-8 ── P2-9 ──┬── P3-2 ──┐
       │                                         │                  ├── P3-3 ──┤
       │                                         │                  │          ├── P3-4 ── P3-5 ── P3-6
       │                                         │                  │          │
       │              P4-3 (indépendant)         │                  │          │
       │                                         │                  │          │
       │                                         └──────── P5-2 ──┐│          │
       │                                                    P5-3 ──┤│          │
       │                                                    P5-4 ──┤│          │
       │                                                    P5-5 ──┘│          │
       │                                                            │          │
       └────────────────────────────────────────────────────────────┘          │
                                                                               │
                                              P4-4 ── P4-5 ──┬── P4-6        │
                                                              ├── P4-7        │
                                                              └── P4-8        │
                                                                               │
                                              P5-1 (après P3-5) ── P5-6 ── P5-7
```

### Chemins critiques

1. **Chemin le plus long** : P0-1 → P0-3 → P1-1 → P1-2 → P2-2 → P2-3 → P2-4 → P2-6 → P2-8 → P3-4 → P3-5 → P5-1 → P5-7
2. **Goulot** : P2-8 (assemblage du solveur complet) — c'est la tâche la plus lourde (~200–300 lignes).

### Parallélisme maximal par vague

| Vague | Tâches parallèles | Domaines |
|-------|-------------------|----------|
| **V1** | P0-1, P2-1, P2-7, P4-3 | Modèle, refactor, objectif, UI |
| **V2** | P0-2, P0-3, P3-1 | Parser, bornes, API |
| **V3** | P0-4, P1-1, P4-1 | Snapshots, violation struct, store |
| **V4** | P1-2, P2-2, P4-2 | Vérification, boucle barrière, sliders |
| **V5** | P1-3, P2-3, P2-5 | Résultat enrichi, diagonale, fraction-to-boundary |
| **V6** | P2-4 | Jacobien augmenté |
| **V7** | P2-6 | Mise à jour Schur |
| **V8** | P2-8, P2-9 | Assemblage solveur contraint |
| **V9** | P3-2, P3-3, P5-2, P5-3, P5-4, P5-5 | API, WS, tests validation |
| **V10** | P3-4, P4-4 | Routage, types WS |
| **V11** | P3-5, P3-6, P4-5 | Messages, modes, store |
| **V12** | P4-6, P4-7, P4-8, P5-1 | UI résultats, test E2E |
| **V13** | P5-6, P5-7 | Documentation |

---

## Conventions

### Nommage

- Module : `solver/capacity.rs`
- Struct publique : `CapacityBounds`, `CapacityViolation`, `ConstrainedSolverResult`
- Fonction publique : `solve_steady_state_constrained`
- Fonction interne : `barrier_diagonal`, `fraction_to_boundary`, `schur_demand_update`

### Convention de signes (inchangée)

- `d > 0` : injection (source)
- `d < 0` : soutirage (sink)
- Bornes : `flow_min_m3s ≤ d ≤ flow_max_m3s` (respectent la convention de signe du scénario)

### Paramètres par défaut du solveur contraint

| Paramètre | Valeur | Justification |
|-----------|--------|---------------|
| `μ₀` | 0.1 | Standard pour NLP de petite taille |
| Facteur réduction μ | 0.2 (÷5) | Convergence en ~5–8 passes barrière |
| `τ` (fraction-to-boundary) | 0.995 | Standard (Nocedal & Wright) |
| Tolérance barrière | `tolerance × 10` | Cohérence avec le Newton sous-jacent |
| Max passes barrière | 15 | Sécurité |

### Rétro-compatibilité

- Sans `capacity_bounds` → comportement identique à aujourd'hui.
- `SolverResult.capacity_violations` est `Vec::new()` par défaut → JSON inchangé.
- `ClientMessage::StartSimulation.capacity_bounds` est `Option<…>` → messages WS existants fonctionnent.
