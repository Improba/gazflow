# Plan d'implémentation — Capacités d'entrée/sortie

> Extension du solveur GazFlow pour intégrer les contraintes de capacité (min/max débit) aux nœuds d'entrée/sortie, via un solveur à projection itérative avec barrière.

## Vue d'ensemble

### Problème actuel

Le solveur prend des **débits fixes** par nœud et calcule les pressions résultantes. Si un scénario viole les capacités physiques ou contractuelles d'un point d'entrée/sortie, l'utilisateur ne le sait pas.

### Objectif

Intégrer les bornes de capacité $Q_i^{\min} \leq d_i \leq Q_i^{\max}$ dans le modèle, avec deux modes :

1. **Vérification** : résoudre normalement, puis diagnostiquer les violations.
2. **Optimisation** : ajuster automatiquement les débits pour respecter les bornes tout en restant le plus proche possible du scénario demandé.

### Distinction fondamentale : nœuds slack vs nœuds libres

Les bornes de capacité s'appliquent différemment selon le type de nœud :

| Type de nœud | Pression | Débit | Bornes de capacité | Rôle dans l'optimisation |
|---|---|---|---|---|
| **Slack** (sources, `pressure_fixed_bar` défini) | Fixée (entrée) | Calculé par le solveur (sortie) | Vérification seulement — le débit est une conséquence | Mode check uniquement (MVP) |
| **Libre** (sinks, innodes sans pression fixée) | Calculée (sortie) | Fixé comme demande (entrée) | Le débit peut être ajusté | Mode check + optimize |

Le mode **vérification** fonctionne pour tous les nœuds. Le mode **optimisation (MVP)** ajuste uniquement les demandes des nœuds libres bornés. L'optimisation des nœuds slack (où la pression deviendrait variable) est un objectif post-MVP.

### Formulation mathématique (mode optimisation)

$$
\min_{\mathbf{d}} \quad \sum_{i \in \mathcal{B}_{\text{free}}} (d_i - d_i^{\text{cible}})^2
$$

$$
\text{s.c.} \quad F_i(\boldsymbol{\pi}(\mathbf{d})) = 0 \quad \forall \text{ nœud libre } i
$$

$$
d_i^{\min} \leq d_i \leq d_i^{\max} \quad \forall i \in \mathcal{B}_{\text{free}}
$$

où $\boldsymbol{\pi}(\mathbf{d})$ est la solution du système hydraulique pour les demandes $\mathbf{d}$.

### Méthode retenue : projection itérative avec barrière

L'approche initiale (complément de Schur sur le KKT) est mathématiquement élégante mais :
- La dérivation correcte ne donne PAS une simple modification diagonale du Jacobien (le système réduit est $J^T \Sigma J$, pas $J - \text{diag}$).
- L'implémentation est invasive et difficile à valider.

L'approche retenue est une **méthode alternée** (block-coordinate) :

```
d = clamp(d_cible, d_min + ε, d_max - ε)

pour μ décroissant (μ₀, μ₀/5, μ₀/25, …) :
  Étape 1 — Résolution physique :
    π = solve_newton(network, d)     ← solveur existant, INCHANGÉ

  Étape 2 — Mise à jour des demandes :
    pour chaque nœud borné i :
      d_i^phys = -F_i^pipe(π)        ← débit impliqué par les pressions
      d_i^new = argmin  (d - d_cible)² - μ·ln(d - d_min) - μ·ln(d_max - d)
                d∈[d_min+ε, d_max-ε]
                + ρ·(d - d_i^phys)²  ← rappel vers la solution physique

  si |d^new - d^old| < tol ET Newton convergé : STOP
```

L'étape 2 est un problème 1D convexe par nœud, résolu analytiquement (Newton 1D, ~3 itérations).

**Avantages :**
- Réutilise le solveur Newton **tel quel** (zéro modification de `newton.rs`).
- Chaque étape est correcte et vérifiable indépendamment.
- Convergence garantie pour les problèmes convexes (Bertsekas, 1999).
- ~300–400 lignes de Rust au lieu de ~600.

**Inconvénients :**
- Convergence plus lente que l'approche KKT couplée (typiquement 3–8 boucles extérieures).
- Pour GasLib-11 (~10 nœuds bornés), la surcharge est négligeable.

### Gestion de l'infaisabilité

Si les bornes de capacité sont incompatibles avec la physique (ex: demande totale > capacité totale des sources), le solveur doit :
1. Détecter que la boucle extérieure ne converge pas après `max_outer_iter`.
2. Retourner le meilleur point trouvé + un diagnostic d'infaisabilité.
3. Signaler quels nœuds sont en conflit.

### Références

- Wächter & Biegler (2006). Interior-point filter line-search for large-scale NLP. *Math. Prog.*, 106(1).
- Nocedal & Wright (2006). *Numerical Optimization*, 2e éd. Springer. Chap. 17–19.
- Koch et al. (2015). *Evaluating Gas Network Capacities*. SIAM MOS.
- Ríos-Mercado & Borraz-Sánchez (2015). Optimization in gas transport. *Applied Energy*, 147.
- Bertsekas, D.P. (1999). *Nonlinear Programming*, 2e éd. Athena Scientific. (convergence block-coordinate).

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

### Calcul du débit effectif par nœud

Le `SolverResult` contient les débits par **pipe**, pas par nœud. Pour vérifier les bornes, il faut calculer le débit net à chaque nœud :

- **Nœuds libres** (non-slack) : le débit effectif = `d_i` (la demande en entrée). Si le solveur a convergé, c'est aussi `−Σ Q_ij` (bilan des pipes). La vérification est triviale.
- **Nœuds slack** (pression fixée) : le débit effectif est calculé par le bilan des pipes : `d_i^eff = −Σ Q_ij(i)`. C'est la quantité à comparer aux bornes.

La fonction `validate_solution_physics` dans `steady_state.rs` calcule déjà ce bilan (`node_balance`). Réutiliser cette logique.

### Tâches

| # | Tâche | Fichiers | Entrée | Sortie | Tests |
|---|-------|----------|--------|--------|-------|
| **P1-1** | Créer `CapacityViolation` struct | `back/src/solver/capacity.rs` | — | `{ node_id, bound_type: Min\|Max, limit, actual, margin }` | — |
| **P1-2** | Implémenter `compute_node_effective_flows()` | `back/src/solver/capacity.rs` | `GasNetwork` + `SolverResult` + `demands` | `HashMap<String, f64>` : débit effectif par nœud | T1-1 |
| **P1-3** | Implémenter `check_capacity_violations()` | `back/src/solver/capacity.rs` | Effective flows + `CapacityBounds` | `Vec<CapacityViolation>` | T1-2, T1-3 |
| **P1-4** | Ajouter `capacity_violations: Vec<CapacityViolation>` à `SolverResult` | `back/src/solver/steady_state.rs` | — | Champ optionnel, `#[serde(default, skip_serializing_if = "Vec::is_empty")]` | T1-4 |

#### Tests Phase 1

| ID | Test | Type | Description |
|----|------|------|-------------|
| T1-1 | `test_effective_flows_match_demands_for_free_nodes` | Unitaire | Bilan des pipes = demande pour les nœuds libres après convergence |
| T1-2 | `test_no_violation_when_within_bounds` | Unitaire | Scénario dans les bornes → `violations.is_empty()` |
| T1-3 | `test_detects_overflow_violation` | Unitaire | Débit > max → violation détectée avec valeurs correctes |
| T1-4 | `test_solver_result_includes_violations` | Unitaire | `SolverResult` sérialisé en JSON contient le champ quand non vide, absent quand vide |

---

## Phase 2 — Solveur contraint (projection itérative avec barrière)

### Objectif

Implémenter le cœur du solveur contraint : boucle extérieure alternant entre résolution Newton (solveur existant, inchangé) et mise à jour des demandes par projection barrière.

### Architecture

```
solver/
├── mod.rs              ← ajouter `pub mod capacity;`
├── capacity.rs         ← NEW : CapacityBounds, vérification, solveur contraint
├── newton.rs           ← INCHANGÉ
├── steady_state.rs     ← INCHANGÉ (fonctions publiques)
├── gas_properties.rs   ← INCHANGÉ
└── iterative.rs        ← INCHANGÉ
```

**Note :** Contrairement à la première version du plan, `newton.rs` n'est PAS modifié. Le solveur contraint appelle `solve_steady_state_with_progress` comme une boîte noire.

### Tâches

| # | Tâche | Fichiers | Entrée | Sortie | Tests |
|---|-------|----------|--------|--------|-------|
| **P2-1** | Implémenter `ConstrainedSolverConfig` et `ConstrainedSolverResult` | `back/src/solver/capacity.rs` | — | Structs de config (μ₀, facteur, max_outer, ρ) et résultat enrichi (`active_bounds`, `adjusted_demands`, `objective_value`, `outer_iterations`) | T2-1 |
| **P2-2** | Implémenter `barrier_proximal_update_1d()` | `back/src/solver/capacity.rs` | `d_phys`, `d_target`, bornes, `μ`, `ρ` | `d_new` solution du sous-problème 1D convexe | T2-2 |
| **P2-3** | Implémenter `compute_physical_demands()` | `back/src/solver/capacity.rs` | `GasNetwork`, `SolverResult`, `demands` | `HashMap<String, f64>` : débit physique par nœud (bilan des pipes pour slack, demande pour free) | T2-3 |
| **P2-4** | Implémenter `clamp_initial_demands()` | `back/src/solver/capacity.rs` | `target_demands`, `capacity_bounds` | Demandes initiales clampées dans `[d_min + ε, d_max - ε]` | T2-4 |
| **P2-5** | Implémenter la boucle extérieure alternée | `back/src/solver/capacity.rs` | Config + network + bounds + demands | Boucle : Newton-solve → demand-update → convergence check | T2-5 |
| **P2-6** | Implémenter la détection d'infaisabilité | `back/src/solver/capacity.rs` | Historique d'objectif + max_outer_iter | `InfeasibilityDiagnostic` si non-convergence | T2-6 |
| **P2-7** | Exposer `solve_steady_state_constrained` + callbacks de progression | `back/src/solver/capacity.rs`, `back/src/solver/mod.rs` | Signature publique complète | Fonction appelable depuis l'API, progress report avec `outer_iter` + `inner_iter` | T2-7 |

#### Détails mathématiques — P2-2

Le sous-problème 1D pour chaque nœud borné $i$ :

$$
d_i^{\text{new}} = \arg\min_{d_i^{\min} + \varepsilon \leq d \leq d_i^{\max} - \varepsilon} \; (d - d_i^{\text{cible}})^2 + \rho (d - d_i^{\text{phys}})^2 - \mu \ln(d - d_i^{\min}) - \mu \ln(d_i^{\max} - d)
$$

La condition de stationnarité donne :

$$
2(d - d_i^{\text{cible}}) + 2\rho(d - d_i^{\text{phys}}) - \frac{\mu}{d - d_i^{\min}} + \frac{\mu}{d_i^{\max} - d} = 0
$$

Cette équation 1D est résolue par Newton scalaire (3–5 itérations suffisent, la fonction est strictement convexe sur l'intervalle ouvert).

Le paramètre $\rho$ contrôle le couplage entre la cible commerciale et la physique :
- $\rho = 0$ : on ignore la physique, on cherche le point le plus proche de la cible dans les bornes
- $\rho \gg 1$ : on colle à la solution physique, les bornes ne sont actives qu'en cas de violation
- Valeur recommandée : $\rho = 1$ (compromis, ajustable)

#### Détails techniques — P2-5

```
d = clamp_initial_demands(d_target, bounds)
μ = μ₀

pour outer_iter = 1..max_outer :
  result = solve_steady_state_with_progress(network, d, …)
  si Newton n'a pas convergé : bail avec diagnostic

  d_phys = compute_physical_demands(network, result, d)

  d_old = d.clone()
  pour chaque nœud borné i :
    d[i] = barrier_proximal_update_1d(d_phys[i], d_target[i], bounds[i], μ, ρ)

  objective = Σ (d[i] - d_target[i])²
  Δd_max = max |d[i] - d_old[i]|

  report progress(outer_iter, objective, Δd_max)

  si Δd_max < tol ET result.residual < tol : CONVERGÉ
  μ *= reduction_factor
```

#### Signature cible — P2-7

```rust
pub fn solve_steady_state_constrained<F>(
    network: &GasNetwork,
    target_demands: &HashMap<String, f64>,
    capacity_bounds: &CapacityBounds,
    initial_pressures_bar: Option<&HashMap<String, f64>>,
    config: ConstrainedSolverConfig,
    on_progress: F,
) -> Result<ConstrainedSolverResult>
where
    F: FnMut(ConstrainedProgress) -> SolverControl;
```

```rust
pub struct ConstrainedProgress {
    pub outer_iter: usize,
    pub inner_progress: SolverProgress,  // du Newton sous-jacent
    pub objective: f64,
    pub demand_delta_max: f64,
    pub mu: f64,
}
```

#### Tests Phase 2

| ID | Test | Type | Description |
|----|------|------|-------------|
| T2-1 | `test_constrained_config_defaults` | Unitaire | Config par défaut valide (μ₀, facteur, etc.) |
| T2-2 | `test_barrier_proximal_1d_stays_in_bounds` | Unitaire | Solution 1D strictement dans `]d_min, d_max[` pour divers cas |
| T2-3 | `test_barrier_proximal_1d_target_in_bounds` | Unitaire | Si `d_target` est dans les bornes et μ→0, solution → `d_target` |
| T2-4 | `test_barrier_proximal_1d_clamps_to_bound` | Unitaire | Si `d_target > d_max`, solution → `d_max - ε` quand μ→0 |
| T2-5 | `test_physical_demands_match_balance` | Unitaire | `compute_physical_demands` cohérent avec le bilan de masse |
| T2-6 | `test_constrained_solver_two_nodes_within_bounds` | Intégration | Réseau 2-nœuds, bornes larges → résultat ≈ solveur non contraint |
| T2-7 | `test_constrained_solver_two_nodes_clamps_demand` | Intégration | Réseau 2-nœuds, demande > max → débit ajusté à ~max |
| T2-8 | `test_constrained_solver_y_network` | Intégration | Y-network, une sortie bornée → débit redistribué |
| T2-9 | `test_constrained_vs_unconstrained_gaslib11` | Intégration | GasLib-11, bornes larges → résultats quasi-identiques |
| T2-10 | `test_constrained_gaslib11_tight_bounds` | Intégration | GasLib-11, bornes serrées → au moins un nœud actif |
| T2-11 | `test_constrained_infeasible_returns_diagnostic` | Intégration | Bornes impossibles → `InfeasibilityDiagnostic` retourné |

---

## Phase 3 — Intégration API et WebSocket

### Objectif

Exposer les capacités et le solveur contraint via l'API REST et le WebSocket.

### Tâches

| # | Tâche | Fichiers | Entrée | Sortie | Tests |
|---|-------|----------|--------|--------|-------|
| **P3-1** | Enrichir `NodeDto` et `GET /api/network` avec les bornes | `back/src/api/mod.rs` | `Node.flow_min_m3s/flow_max_m3s` | JSON `nodes[].flow_min_m3s`, `flow_max_m3s` (nullable) | T3-1 |
| **P3-2** | Créer struct `CapacityBoundDto { min: f64, max: f64 }` pour l'API (pas de tuple) | `back/src/api/mod.rs` | — | Struct sérialisable/désérialisable en JSON `{ "min": 0.0, "max": 5.0 }` | — |
| **P3-3** | Ajouter `capacity_bounds` optionnel à `SimulateRequest` | `back/src/api/mod.rs` | — | `Option<HashMap<String, CapacityBoundDto>>` | T3-2 |
| **P3-4** | Ajouter `capacity_bounds` et `mode` à `ClientMessage::StartSimulation` | `back/src/api/ws.rs` | — | `mode: Option<"check"\|"optimize">`, `capacity_bounds: Option<…>` | T3-3 |
| **P3-5** | Router vers solveur contraint ou classique+check selon mode | `back/src/api/mod.rs`, `back/src/api/ws.rs` | Présence de `mode`/`capacity_bounds` | Check → classique + violations, Optimize → solveur contraint | T3-4 |
| **P3-6** | Enrichir les messages WS `converged` avec `capacity_violations`, `active_bounds`, `adjusted_demands` | `back/src/api/ws.rs` | `ConstrainedSolverResult` | Nouveaux champs JSON optionnels dans le message | T3-5 |
| **P3-7** | Adapter le module export pour les résultats contraints | `back/src/api/export.rs` | `ConstrainedSolverResult` | Export JSON/CSV/ZIP inclut violations et demandes ajustées | T3-6 |

**Note API :** Ne pas utiliser `HashMap<String, (f64, f64)>` pour les bornes — `serde` ne sérialise pas les tuples de façon lisible en JSON. Utiliser une struct `CapacityBoundDto { min: f64, max: f64 }`.

#### Tests Phase 3

| ID | Test | Type | Description |
|----|------|------|-------------|
| T3-1 | `test_api_network_includes_capacity_bounds` | API | `GET /api/network` retourne `flow_min_m3s` et `flow_max_m3s` pour les nœuds |
| T3-2 | `test_api_simulate_with_capacity_bounds` | API | `POST /api/simulate` avec bornes → résultat contraint |
| T3-3 | `test_ws_start_with_capacity_bounds_and_mode` | WS | Message `start_simulation` avec `mode` et `capacity_bounds` → simulation démarre |
| T3-4 | `test_ws_check_mode_returns_violations_only` | WS | Mode `check` → résolution classique + violations sans optimisation |
| T3-5 | `test_ws_constrained_converged_includes_violations` | WS | Mode `optimize` → message `converged` contient `capacity_violations`, `active_bounds`, `adjusted_demands` |
| T3-6 | `test_export_includes_constrained_fields` | API | Export JSON d'un résultat contraint contient les demandes ajustées |
| T3-7 | `test_api_simulate_without_bounds_unchanged` | API | `POST /api/simulate` sans bornes ni mode → comportement identique à aujourd'hui |

---

## Phase 4 — Frontend

### Objectif

Afficher les capacités dans l'UI, permettre de configurer le mode (vérification / optimisation), et visualiser les résultats contraints.

### Tâches

| # | Tâche | Fichiers | Entrée | Sortie | Tests |
|---|-------|----------|--------|--------|-------|
| **P4-1** | Enrichir `NodeDto` avec `flow_min_m3s`, `flow_max_m3s` | `front/src/stores/network.ts` | API response | Champs disponibles dans le store | T4-1 |
| **P4-2** | Créer les types TS `CapacityBound`, `CapacityViolation`, `ConstrainedResult` | `front/src/services/api.ts`, `front/src/services/ws.ts` | — | Interfaces TypeScript pour les nouveaux champs API/WS | T4-2 |
| **P4-3** | Borner les sliders de `DemandControls` aux capacités | `front/src/components/DemandControls.vue` | `node.flow_min_m3s`, `node.flow_max_m3s` | `:min` et `:max` du slider reflètent les bornes (conversion m³/s → affichage) | — |
| **P4-4** | Ajouter sélecteur de mode (Vérifier / Optimiser / Libre) | `front/src/components/SimulationPanel.vue` | — | `q-btn-toggle` qui envoie `mode` avec la simulation | — |
| **P4-5** | Enrichir `WsServerMessage` avec `capacity_violations`, `active_bounds`, `adjusted_demands` | `front/src/services/ws.ts` | Message serveur | Types mis à jour, parsing dans `handleWsMessage` | T4-3 |
| **P4-6** | Enrichir `simulate` store avec state contraint | `front/src/stores/simulate.ts` | Message `converged` | `capacityViolations`, `activeBounds`, `adjustedDemands` dans le store | T4-4 |
| **P4-7** | Afficher les violations dans le panneau résultat | `front/src/components/SimulationPanel.vue` | `capacityViolations` | Bannière d'alerte avec liste des nœuds en violation et marges | — |
| **P4-8** | Colorer les nœuds en violation sur la carte 3D | `front/src/components/CesiumViewer.vue` | `capacityViolations` | Nœuds en rouge/orange quand violation, cyan par défaut | — |
| **P4-9** | Afficher les bornes actives et demandes ajustées dans le panneau | `front/src/components/SimulationPanel.vue` | `activeBounds`, `adjustedDemands` | Icône verrou à côté des nœuds contraints, affichage `d_cible → d_ajusté` | — |

#### Tests Phase 4

| ID | Test | Type | Description |
|----|------|------|-------------|
| T4-1 | `test_network_store_loads_capacity_bounds` | Vitest | Store charge `flow_min_m3s` / `flow_max_m3s` depuis l'API |
| T4-2 | `test_capacity_types_compile` | Vitest | Types TS `CapacityBound`, `CapacityViolation` utilisables dans un test |
| T4-3 | `test_ws_converged_message_with_violations` | Vitest | Message `converged` avec `capacity_violations` parsé correctement |
| T4-4 | `test_simulate_store_handles_constrained_result` | Vitest | Store `simulate` expose `capacityViolations` et `adjustedDemands` après convergence |

---

## Phase 5 — Tests d'intégration et validation scientifique

### Objectif

Valider le solveur contraint de bout en bout, y compris la cohérence physique et la comparaison avec le solveur non contraint.

### Tâches

| # | Tâche | Fichiers | Entrée | Sortie | Tests |
|---|-------|----------|--------|--------|-------|
| **P5-1** | Test E2E : simulation contrainte via WebSocket | `back/tests/api_test.rs` | GasLib-11 + bornes | Résultat convergé avec violations/bornes actives/demandes ajustées | T5-1 |
| **P5-2** | Test de régression : bornes larges ≈ non contraint | `back/src/solver/capacity.rs` | GasLib-11, bornes larges (innodes à ±1100) | `\|P_contraint - P_libre\| < ε` | T5-2 |
| **P5-3** | Test de conservation de masse avec contraintes | `back/src/solver/capacity.rs` | Y-network borné | `\|Σ Q_in - Σ Q_out\| < ε` sur chaque nœud après optimisation | T5-3 |
| **P5-4** | Test de monotonie de l'objectif | `back/src/solver/capacity.rs` | GasLib-11, bornes serrées | Objectif décroît (ou stagne) à chaque boucle extérieure | T5-4 |
| **P5-5** | Test de performance : solveur classique inchangé | `back/src/solver/capacity.rs` | GasLib-11 sans bornes (mode classique) | Temps identique au solveur non contraint (pas de surcharge si pas de bornes) | T5-5 |
| **P5-6** | Mettre à jour `docs/science/equations.md` section 7 | `docs/science/equations.md` | — | Formulation complète : distinction slack/free, méthode alternée, sous-problème 1D, convergence | — |
| **P5-7** | Mettre à jour `docs/testing/README.md` | `docs/testing/README.md` | — | Commandes pour les tests contraints, variables d'environnement de tuning | — |

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
| P1-2 | P1 | Backend/Solver | S | P0-3 | `solver/capacity.rs` | T1-1 passe |
| P1-3 | P1 | Backend/Solver | M | P1-1, P1-2 | `solver/capacity.rs` | T1-2, T1-3 passent |
| P1-4 | P1 | Backend/Solver | S | P1-3 | `solver/steady_state.rs` | T1-4 passe |
| P2-1 | P2 | Backend/Solver | S | P0-3 | `solver/capacity.rs` | T2-1 passe |
| P2-2 | P2 | Backend/Solver | M | P2-1 | `solver/capacity.rs` | T2-2, T2-3, T2-4 passent |
| P2-3 | P2 | Backend/Solver | S | P1-2 | `solver/capacity.rs` | T2-5 passe |
| P2-4 | P2 | Backend/Solver | S | P2-1 | `solver/capacity.rs` | Config defaults valides |
| P2-5 | P2 | Backend/Solver | L | P2-2, P2-3, P2-4 | `solver/capacity.rs` | T2-6, T2-7, T2-8 passent |
| P2-6 | P2 | Backend/Solver | S | P2-5 | `solver/capacity.rs` | T2-11 passe |
| P2-7 | P2 | Backend/Solver | M | P2-5, P2-6, P1-3 | `solver/capacity.rs`, `solver/mod.rs` | T2-9, T2-10 passent, fonction publique exportée |
| P3-1 | P3 | Backend/API | S | P0-1 | `api/mod.rs` | T3-1 passe |
| P3-2 | P3 | Backend/API | S | — | `api/mod.rs` | Struct `CapacityBoundDto` compile |
| P3-3 | P3 | Backend/API | S | P3-2, P2-7 | `api/mod.rs` | T3-2 passe |
| P3-4 | P3 | Backend/WS | S | P3-2, P2-7 | `api/ws.rs` | T3-3 passe |
| P3-5 | P3 | Backend/API | M | P3-3, P3-4, P1-3 | `api/mod.rs`, `api/ws.rs` | T3-4, T3-5, T3-7 passent |
| P3-6 | P3 | Backend/WS | S | P3-5 | `api/ws.rs` | T3-5 passe |
| P3-7 | P3 | Backend/Export | S | P3-5 | `api/export.rs` | T3-6 passe |
| P4-1 | P4 | Frontend/Store | S | P3-1 | `stores/network.ts` | T4-1 passe |
| P4-2 | P4 | Frontend/Types | S | P3-6 | `services/api.ts`, `services/ws.ts` | T4-2 passe |
| P4-3 | P4 | Frontend/UI | S | P4-1 | `components/DemandControls.vue` | Sliders bornés visuellement |
| P4-4 | P4 | Frontend/UI | S | — | `components/SimulationPanel.vue` | Toggle mode visible et fonctionnel |
| P4-5 | P4 | Frontend/WS | S | P4-2 | `services/ws.ts` | T4-3 passe |
| P4-6 | P4 | Frontend/Store | S | P4-5 | `stores/simulate.ts` | T4-4 passe |
| P4-7 | P4 | Frontend/UI | M | P4-6 | `components/SimulationPanel.vue` | Violations affichées avec marges |
| P4-8 | P4 | Frontend/UI | M | P4-6 | `components/CesiumViewer.vue` | Nœuds en violation colorés rouge/orange |
| P4-9 | P4 | Frontend/UI | S | P4-6 | `components/SimulationPanel.vue` | Bornes actives + demandes ajustées visibles |
| P5-1 | P5 | Test E2E | M | P3-6 | `tests/api_test.rs` | T5-1 passe |
| P5-2 | P5 | Test validation | S | P2-7 | `solver/capacity.rs` | T5-2 passe |
| P5-3 | P5 | Test validation | S | P2-7 | `solver/capacity.rs` | T5-3 passe |
| P5-4 | P5 | Test validation | S | P2-7 | `solver/capacity.rs` | T5-4 passe |
| P5-5 | P5 | Test perf | S | P2-7 | `solver/capacity.rs` | T5-5 passe |
| P5-6 | P5 | Documentation | M | P2-7 | `docs/science/equations.md` | Section 7 complète |
| P5-7 | P5 | Documentation | S | P5-1 | `docs/testing/README.md` | Commandes documentées |

### Graphe de parallélisme

```
P0-1 ──┬── P0-2 ── P0-4
       ├── P0-3 ──┬── P1-1 ── P1-3 ── P1-4
       │          ├── P1-2 ───────┘
       │          ├── P2-1 ──┬── P2-2 ──┐
       │          │          └── P2-4   │
       │          └── P2-3 ─────────────┤
       │                                ├── P2-5 ── P2-6 ── P2-7 ──┬── P3-3 ──┐
       └── P3-1 ── P4-1 ── P4-3        │                          ├── P3-4 ──┤
                                        │                          │          ├── P3-5 ── P3-6 ── P3-7
   P3-2 (indépendant) ─────────────────┘                          │          │
   P4-4 (indépendant)                                             │          │
                                                                   │          │
                                          P4-2 ── P4-5 ── P4-6 ──┤          │
                                                                   ├── P4-7  │
                                                                   ├── P4-8  │
                                                                   └── P4-9  │
                                                                              │
                                          P5-2, P5-3, P5-4, P5-5 (après P2-7)│
                                          P5-1 (après P3-6) ── P5-6 ── P5-7
```

### Chemins critiques

1. **Chemin le plus long** : P0-1 → P0-3 → P2-1 → P2-2 → P2-5 → P2-6 → P2-7 → P3-5 → P3-6 → P5-1 → P5-7
2. **Goulot** : P2-5 (boucle extérieure alternée) — c'est la tâche la plus lourde (~200 lignes), mais plus simple que l'ancien P2-8 car elle n'a pas besoin de modifier le Jacobien.

### Parallélisme maximal par vague

| Vague | Tâches parallèles | Domaines |
|-------|-------------------|----------|
| **V1** | P0-1, P3-2, P4-4 | Modèle, API types, UI toggle |
| **V2** | P0-2, P0-3, P3-1 | Parser, bornes, API network |
| **V3** | P0-4, P1-1, P1-2, P2-1, P2-4, P4-1 | Snapshots, violation, solver config, store |
| **V4** | P1-3, P2-2, P2-3, P4-3 | Vérification, proximal 1D, phys demands, sliders |
| **V5** | P1-4, P2-5 | SolverResult, boucle alternée |
| **V6** | P2-6, P2-7 | Infaisabilité, exposition publique |
| **V7** | P3-3, P3-4, P5-2, P5-3, P5-4, P5-5 | API, WS, tests validation |
| **V8** | P3-5, P4-2 | Routage, types TS |
| **V9** | P3-6, P3-7, P4-5 | Messages, export, WS types |
| **V10** | P4-6 | Store simulate enrichi |
| **V11** | P4-7, P4-8, P4-9, P5-1 | UI résultats, carte 3D, test E2E |
| **V12** | P5-6, P5-7 | Documentation |

---

## Conventions

### Nommage

- Module : `solver/capacity.rs`
- Structs publiques : `CapacityBounds`, `CapacityViolation`, `ConstrainedSolverConfig`, `ConstrainedSolverResult`, `ConstrainedProgress`
- Fonction publique : `solve_steady_state_constrained`
- Fonctions internes : `barrier_proximal_update_1d`, `compute_physical_demands`, `compute_node_effective_flows`, `clamp_initial_demands`
- Types API : `CapacityBoundDto` (pas de tuple pour JSON)

### Convention de signes (inchangée)

- `d > 0` : injection (source)
- `d < 0` : soutirage (sink)
- Bornes : `flow_min_m3s ≤ d ≤ flow_max_m3s` (respectent la convention de signe du scénario)
- Pour les sinks, `flow_min` et `flow_max` sont typiquement ≤ 0 dans GasLib (convention cohérente)

### Paramètres par défaut du solveur contraint

| Paramètre | Valeur | Justification |
|-----------|--------|---------------|
| `μ₀` | 0.1 | Standard pour NLP de petite taille |
| Facteur réduction μ | 0.2 (÷5) | Convergence en ~5–8 passes barrière |
| `ρ` (rappel physique) | 1.0 | Compromis cible commerciale / faisabilité physique |
| `ε` (marge bornes) | `1e-6` | Garde les variables strictement dans l'intérieur |
| Tolérance Δd | `tolerance × 10` | Cohérence avec le Newton sous-jacent |
| Max outer iterations | 20 | Sécurité |
| Max inner Newton iter | identique au `max_iter` courant | Pas de changement |

### Initialisation des demandes

Au démarrage du solveur contraint :
1. `d_i = d_i^cible` (demande du scénario) pour les nœuds non bornés.
2. `d_i = clamp(d_i^cible, d_min + ε, d_max - ε)` pour les nœuds bornés.
3. Les pressions initiales utilisent `initial_pressures_bar` si fourni (warm-start), sinon 70 bar uniforme.

### Rétro-compatibilité

- Sans `capacity_bounds` ni `mode` → comportement identique à aujourd'hui.
- `SolverResult.capacity_violations` est `Vec::new()` par défaut → JSON inchangé (`skip_serializing_if = "Vec::is_empty"`).
- `ClientMessage::StartSimulation.capacity_bounds` et `mode` sont `Option<…>` → messages WS existants fonctionnent.
- `NodeDto` gagne `flow_min_m3s: Option<f64>` et `flow_max_m3s: Option<f64>` → nullable, clients existants les ignorent.

### Limites du MVP et évolutions futures

| Limite MVP | Évolution future |
|------------|-----------------|
| Optimisation uniquement sur les nœuds libres (non-slack) | Optimisation avec pression variable aux slacks (NLP complet) |
| Pas d'optimisation de la compression | Coût de compression dans l'objectif |
| Bornes de débit seulement | Bornes de pression dans l'optimisation (déjà parsées : `pressure_lower_bar`/`pressure_upper_bar`) |
| Méthode alternée (convergence linéaire) | Méthode KKT couplée (convergence superlinéaire) pour réseaux > 1000 nœuds |
