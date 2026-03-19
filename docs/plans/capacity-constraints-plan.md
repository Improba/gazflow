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
| **Slack** (sources, `pressure_fixed_bar` défini) | Fixée (entrée) | Calculé par le solveur (sortie) = bilan des pipes | Le débit est une conséquence des pressions et des demandes du réseau | Vérification + **contrainte implicite** sur l'optimize |
| **Libre** (sinks, innodes sans pression fixée) | Calculée (sortie) | Fixé comme demande (entrée) | Le débit est directement contrôlable | Vérification + variable d'optimisation |

### Pourquoi l'optimisation est non-triviale (couplage slack)

À première vue, optimiser les demandes des nœuds libres semble trivial : il suffirait de clamper chaque $d_i$ dans $[d_i^{\min}, d_i^{\max}]$. Mais le **couplage via les nœuds slack** rend le problème non-trivial.

**Exemple concret :** réseau Y avec 1 source (slack, capacité max 120 m³/s) et 2 sinks :
- Sink A : cible = −80, bornes = [−100, 0]
- Sink B : cible = −70, bornes = [−100, 0]

Le clampage naïf donne $d_A = -80$, $d_B = -70$, total = 150 m³/s. La source doit fournir 150, mais sa capacité max est 120. **Violation slack.**

Pour respecter toutes les bornes (y compris slack), il faut **réduire les demandes libres** de façon coordonnée. C'est un problème d'optimisation sous contraintes couplées.

### Formulation mathématique

$$
\min_{\mathbf{d}_{\text{free}}} \quad \sum_{i \in \mathcal{B}_{\text{free}}} w_i \cdot (d_i - d_i^{\text{cible}})^2
$$

$$
\text{s.c.} \quad d_i^{\min} \leq d_i \leq d_i^{\max} \quad \forall i \in \mathcal{B}_{\text{free}} \quad \text{(bornes libres)}
$$

$$
d_j^{\min} \leq d_j^{\text{eff}}(\mathbf{d}_{\text{free}}) \leq d_j^{\max} \quad \forall j \in \mathcal{B}_{\text{slack}} \quad \text{(bornes slack implicites)}
$$

où $d_j^{\text{eff}}(\mathbf{d}_{\text{free}}) = -\sum_k Q_{jk}(\boldsymbol{\pi}(\mathbf{d}_{\text{free}}))$ est le débit effectif au nœud slack $j$, fonction non-linéaire des demandes libres via la solution hydraulique $\boldsymbol{\pi}$.

**Note sur la fonction objectif :** les poids $w_i$ sont par défaut uniformes ($w_i = 1$). L'architecture du solveur permet d'autres choix (poids prioritaires, coût économique) sans changer l'algorithme.

### Méthode retenue : projection itérative

L'approche initiale (complément de Schur sur le KKT) est mathématiquement élégante mais :
- La dérivation correcte ne donne PAS une simple modification diagonale du Jacobien (le système réduit est $J^T \Sigma J$, pas $J - \text{diag}$).
- L'implémentation est invasive et difficile à valider.

L'approche retenue est une **méthode de projection itérative** :

```
d = clamp(d_cible, d_min + ε, d_max - ε)          [bornes libres]

pour outer_iter = 1..max_outer :
  Étape 1 — Résolution physique :
    π = solve_newton(network, d)                    [solveur existant, INCHANGÉ]

  Étape 2 — Calcul des débits slack effectifs :
    d_slack_eff[j] = -Σ Q_jk(π)  pour chaque slack j

  Étape 3 — Vérification slack :
    slack_ok = toutes les bornes slack respectées ?
    si slack_ok ET |d - d_old| < tol : CONVERGÉ

  Étape 4 — Mise à jour des demandes libres :
    pour chaque nœud libre borné i :
      d[i] = projet_proximal(d[i], d_cible[i], d_slack_eff, bounds, outer_iter)

  Warm-start: réutiliser π comme pressions initiales pour la prochaine itération
```

L'étape 4 (projection proximale) est le cœur de l'algorithme. Deux stratégies :

**Stratégie A — Réduction proportionnelle (simple, robuste) :**
Si la demande totale excède la capacité slack totale, réduire les demandes proportionnellement à leur écart avec la cible :
$$
d_i^{\text{new}} = d_i^{\text{cible}} \cdot \frac{C_{\text{slack}}}{\sum |d_j^{\text{cible}}|}
$$
puis re-clamper dans les bornes libres. Converge en 2–3 itérations.

**Stratégie B — Barrière proximale (optimale, plus lente) :**
Résoudre un sous-problème 1D par nœud avec barrière logarithmique, en introduisant un terme de pénalité pour le dépassement slack. Converge vers l'optimum en ~5–8 itérations.

**L'implémentation commence par la stratégie A** (plus simple, suffisante pour le MVP), avec la structure du code prévue pour intégrer la stratégie B ensuite.

### Convergence

La convergence de la méthode alternée n'est **pas formellement garantie** par les théorèmes standard (Bertsekas, 1999) car :
- Le problème n'est pas block-séparable (le débit slack dépend des demandes libres via $\pi$).
- L'application $\mathbf{d} \to \mathbf{d}^{\text{new}}$ n'est pas prouvée contractante en général.

**En pratique**, la convergence est observée car :
1. Le réseau gazier a un comportement régulier ($\pi(d)$ est localement Lipschitz, monotone en $d$).
2. La réduction proportionnelle (stratégie A) est une contraction avec facteur $< 1$ quand la sur-demande est uniforme.
3. Le warm-start des pressions accélère la convergence interne.

Un **garde-fou** est implémenté : si le résidu d ne diminue pas pendant 3 itérations consécutives, le solveur s'arrête et retourne un diagnostic d'infaisabilité.

### Gestion de l'infaisabilité

Si les bornes de capacité sont incompatibles avec la physique (ex: demande totale > capacité totale des sources même au minimum) :
1. Détecter la stagnation de l'objectif après 3 itérations sans progrès.
2. Retourner le meilleur point trouvé + un `InfeasibilityDiagnostic` :
   - Quelle contrainte (slack ou libre) est violée.
   - De combien (marge négative).
   - Suggestion : quels nœuds ajuster.

### Références

- Wächter & Biegler (2006). Interior-point filter line-search for large-scale NLP. *Math. Prog.*, 106(1).
- Nocedal & Wright (2006). *Numerical Optimization*, 2e éd. Springer. Chap. 17–19.
- Koch et al. (2015). *Evaluating Gas Network Capacities*. SIAM MOS.
- Pfetsch et al. (2015). Validation of Nominations in Gas Network Optimization. *ZIB-Report 12-41*.
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

Le XML GasLib contient `<flowMin>` et `<flowMax>` sur **tous les types de nœuds** (source, sink, innode) :

```xml
<source id="entry01">  <!-- flowMin=50, flowMax=750 (positifs, injection) -->
  <flowMin value="50.0" unit="1000m_cube_per_hour"/>
  <flowMax value="750.0" unit="1000m_cube_per_hour"/>
</source>

<sink id="exit01">    <!-- flowMin=50, flowMax=1250 (positifs = MAGNITUDES de soutirage) -->
  <flowMin value="50.0" unit="1000m_cube_per_hour"/>
  <flowMax value="1250.0" unit="1000m_cube_per_hour"/>
</sink>
```

**⚠️ Convention de signe GasLib vs code :**

Dans GasLib, les bornes des sinks sont des **magnitudes positives** (quantité soutirée). Dans le code, les soutirages sont **négatifs** ($d < 0$). Le parser doit effectuer la conversion :

| Type nœud GasLib | `flowMin` GasLib | `flowMax` GasLib | `flow_min_m3s` code | `flow_max_m3s` code |
|---|---|---|---|---|
| `source` (entry) | 50 | 750 | +50 × conv | +750 × conv |
| `sink` (exit) | 50 | 1250 | **−1250** × conv | **−50** × conv |
| `innode` | −1100 | 1100 | −1100 × conv | +1100 × conv |

où $\text{conv} = 1000/3600$ pour l'unité `1000m_cube_per_hour` → m³/s.

**Règle :** pour les `sink`, inverser et permuter : `flow_min_code = -flowMax_gaslib`, `flow_max_code = -flowMin_gaslib`. Pour les `source` et `innode`, mapper directement.

Le parser doit :
1. Ajouter `flow_min` et `flow_max` à `XmlNode` (même pattern que `pressure_min` / `pressure_max`).
2. Détecter le type de nœud (`source` / `sink` / `innode`) — déjà parsé par `XmlConnection` variant, mais le type du nœud lui-même doit être propagé (ajouter un champ `node_type: NodeType` ou utiliser une heuristique basée sur le nom/contexte).
3. Convertir les unités en m³/s (réutiliser la logique de `scenario.rs`).
4. Appliquer la règle de signe pour les sinks.
5. Mapper vers `Node.flow_min_m3s` / `Node.flow_max_m3s` dans `load_network`.

Vérification sur GasLib-11 : `entry01` (source) → `[+13.89, +208.33]` m³/s ; `exit01` (sink) → `[-347.22, -13.89]` m³/s ; innodes → pas de bornes utiles (±305 m³/s, quasi-libres).

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
| **P1-4** | Implémenter `check_pipe_flow_violations()` | `back/src/solver/capacity.rs` | `GasNetwork` + `SolverResult` | `Vec<CapacityViolation>` pour les pipes dont le débit excède `flowMin`/`flowMax` | T1-5 |
| **P1-5** | Ajouter `capacity_violations: Vec<CapacityViolation>` à `SolverResult` | `back/src/solver/steady_state.rs` | — | Champ optionnel, `#[serde(default, skip_serializing_if = "Vec::is_empty")]` | T1-6 |

**Note :** GasLib fournit aussi des bornes `<flowMin>`/`<flowMax>` sur les **pipes** (connexions). Elles sont déjà parsées dans `XmlConnectionRaw` (utilisées pour la détection valve ouverte/fermée). Les réutiliser pour la vérification est un gain rapide. Les pipes ont des bornes symétriques (ex: ±1100 1000m³/h) et un signe signé (négatif = flux inverse).

#### Tests Phase 1

| ID | Test | Type | Description |
|----|------|------|-------------|
| T1-1 | `test_effective_flows_match_demands_for_free_nodes` | Unitaire | Bilan des pipes = demande pour les nœuds libres après convergence |
| T1-2 | `test_no_violation_when_within_bounds` | Unitaire | Scénario dans les bornes → `violations.is_empty()` |
| T1-3 | `test_detects_overflow_violation` | Unitaire | Débit nœud > max → violation détectée avec valeurs correctes |
| T1-4 | `test_detects_underflow_violation` | Unitaire | Débit nœud < min → violation détectée (important pour les sinks avec min de soutirage) |
| T1-5 | `test_pipe_flow_violation_detected` | Unitaire | Débit pipe > max → violation pipe détectée |
| T1-6 | `test_solver_result_includes_violations` | Unitaire | `SolverResult` sérialisé en JSON contient le champ quand non vide, absent quand vide |

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
| **P2-1** | Implémenter `ConstrainedSolverConfig` et `ConstrainedSolverResult` | `back/src/solver/capacity.rs` | — | Structs de config (max_outer, relax_factor, stratégie) et résultat enrichi (`active_bounds`, `adjusted_demands`, `objective_value`, `outer_iterations`, `slack_violations`) | T2-1 |
| **P2-2** | Implémenter `clamp_initial_demands()` | `back/src/solver/capacity.rs` | `target_demands`, `capacity_bounds` | Demandes initiales clampées dans `[d_min + ε, d_max - ε]` | T2-2 |
| **P2-3** | Implémenter `compute_slack_effective_flows()` | `back/src/solver/capacity.rs` | `GasNetwork`, `SolverResult` | `HashMap<String, f64>` : débit effectif aux nœuds slack uniquement | T2-3 |
| **P2-4** | Implémenter `proportional_demand_reduction()` (stratégie A) | `back/src/solver/capacity.rs` | Demandes courantes, bornes libres, slack effective flows, slack bounds | Nouvelles demandes réduites proportionnellement pour satisfaire les bornes slack | T2-4 |
| **P2-5** | Implémenter la boucle extérieure | `back/src/solver/capacity.rs` | Config + network + all bounds + demands | Boucle : clamp → Newton-solve → check slack → adjust demands → repeat | T2-5 |
| **P2-6** | Implémenter la détection d'infaisabilité et le garde-fou de stagnation | `back/src/solver/capacity.rs` | Historique d'objectif + max_outer_iter | `InfeasibilityDiagnostic` si non-convergence ou stagnation (3 iter sans progrès) | T2-6 |
| **P2-7** | Exposer `solve_steady_state_constrained` + callbacks de progression | `back/src/solver/capacity.rs`, `back/src/solver/mod.rs` | Signature publique complète | Fonction appelable depuis l'API, progress report avec `outer_iter` + `inner_iter` | T2-7 |

#### Détails mathématiques — Stratégie A (réduction proportionnelle)

Après convergence du Newton interne, on calcule les débits slack effectifs. Si un slack $j$ viole sa borne max :

$$
\text{excès}_j = d_j^{\text{eff}} - d_j^{\max}
$$

L'excès total est réparti sur les demandes libres proportionnellement à leur amplitude :

$$
d_i^{\text{new}} = d_i - \alpha \cdot \frac{|d_i|}{\sum_k |d_k|} \cdot \text{excès total}
$$

puis re-clampé dans les bornes libres. Le facteur $\alpha \in (0, 1]$ (sous-relaxation, défaut 0.95) évite les oscillations. Même logique pour les violations min.

Cette stratégie est :
- **Correcte** : réduit monotonement l'excès de débit (par construction).
- **Non-optimale** : ne minimise pas formellement $(d - d_{\text{cible}})^2$. Mais la réduction proportionnelle est une heuristique raisonnable qui traite les nœuds de façon équitable.
- **Rapide** : converge en 2–4 itérations dans les cas courants.

#### Cas limite : $d_{\min} = d_{\max}$ (débit fixé)

Si un nœud a $d_{\min} = d_{\max}$ (capacité fixe), le clamp impose $d = d_{\min}$ sans degré de liberté. Ce nœud est exclu de la réduction proportionnelle. Si les nœuds à débit fixé ne laissent pas assez de marge → infaisabilité.

#### Détails techniques — P2-5

```
d_free = clamp_initial_demands(d_target, free_bounds)

pour outer_iter = 1..max_outer :
  result = solve_newton(network, d_free, warm_start_π)
  si Newton non convergé : bail avec diagnostic

  d_slack_eff = compute_slack_effective_flows(network, result)
  slack_violations = check_slack_bounds(d_slack_eff, slack_bounds)

  si slack_violations.is_empty() :
    CONVERGÉ — retourner result + adjusted_demands + check(free bounds)

  d_old = d_free.clone()
  d_free = proportional_demand_reduction(d_free, free_bounds, slack_violations)

  Δd_max = max |d_free[i] - d_old[i]|
  si Δd_max < tol : STAGNATION — retourner meilleur résultat + infeasibility diagnostic

  warm_start_π = result.pressures  [warm-start pour accélérer le Newton suivant]
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
| T2-1 | `test_constrained_config_defaults` | Unitaire | Config par défaut valide |
| T2-2 | `test_clamp_initial_demands_respects_bounds` | Unitaire | Demandes clampées dans `]d_min, d_max[` |
| T2-3 | `test_slack_effective_flows_match_balance` | Unitaire | `compute_slack_effective_flows` cohérent avec bilan de masse |
| T2-4 | `test_proportional_reduction_reduces_total` | Unitaire | Réduction proportionnelle diminue la demande totale |
| T2-5 | `test_proportional_reduction_respects_free_bounds` | Unitaire | Après réduction, toutes les demandes libres restent dans leurs bornes |
| T2-6 | `test_constrained_no_iteration_when_all_bounds_ok` | Intégration | Bornes larges, aucune violation slack → converge en 1 outer iteration |
| T2-7 | `test_constrained_reduces_demand_on_slack_violation` | Intégration | Y-network, source bornée, demande totale trop forte → demandes réduites |
| T2-8 | `test_constrained_fixed_demand_node_excluded` | Intégration | Nœud avec d_min = d_max exclu de la réduction |
| T2-9 | `test_constrained_vs_unconstrained_gaslib11` | Intégration | GasLib-11, bornes GasLib natives (larges) → résultats quasi-identiques |
| T2-10 | `test_constrained_gaslib11_tight_slack_bound` | Intégration | GasLib-11, source bornée serré → demandes ajustées, slack respecté |
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

### Convention de signes

- `d > 0` : injection (source)
- `d < 0` : soutirage (sink)
- Bornes dans le code : `flow_min_m3s ≤ d ≤ flow_max_m3s` (signées, convention du code)
- **⚠️ GasLib utilise des magnitudes positives pour les sinks.** Le parser doit inverser et permuter pour les sinks : `flow_min_code = -flowMax_gaslib × conv`, `flow_max_code = -flowMin_gaslib × conv`. Voir détails en P0-2.
- Pour les sources et innodes, mapping direct.

### Paramètres par défaut du solveur contraint

| Paramètre | Valeur | Justification |
|-----------|--------|---------------|
| `ε` (marge bornes) | `1e-6` m³/s | Garde les variables strictement dans l'intérieur des bornes |
| Tolérance Δd | `tolerance × 10` | Cohérence avec le Newton sous-jacent |
| Max outer iterations | 15 | Suffisant pour les stratégies A et B ; garde-fou de stagnation à 3 iter |
| Max inner Newton iter | identique au `max_iter` courant | Pas de changement |
| Facteur de réduction proportionnelle (stratégie A) | 0.95 | Sous-relaxation pour éviter l'oscillation |

**Note sur μ (stratégie B uniquement) :** le paramètre de barrière $\mu_0$ doit être proportionnel à l'échelle des demandes. Valeur recommandée : $\mu_0 = 0.01 \cdot \max_i |d_i^{\text{cible}}|^2$. Cette mise à l'échelle évite que les termes de barrière soient négligeables (μ trop petit) ou dominants (μ trop grand) par rapport à l'objectif quadratique.

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

| Limite MVP | Impact | Évolution future |
|------------|--------|-----------------|
| Stratégie A (réduction proportionnelle) — non-optimale au sens mathématique | La solution respecte les bornes mais ne minimise pas formellement l'écart à la cible | Stratégie B (barrière proximale) pour l'optimalité |
| Pas de bornes de pression dans l'optimisation | Les violations de pression sont détectées mais pas corrigées automatiquement | Bornes de pression comme contraintes dans l'optimisation (déjà parsées : `pressure_lower_bar`/`pressure_upper_bar`) |
| Convergence non formellement garantie | Le garde-fou de stagnation protège, mais certains réseaux pathologiques pourraient osciller | Méthode KKT couplée (convergence superlinéaire prouvée) pour réseaux > 1000 nœuds |
| Pas d'optimisation de la compression | Le coût énergétique des compresseurs n'est pas pris en compte | Coût de compression dans la fonction objectif |
| Fonction objectif fixe (moindres carrés) | L'utilisateur ne peut pas choisir entre min-deviation, min-cost, max-throughput | Architecture extensible (`ObjectiveFunction` trait) |
