# Plan d'implémentation — Interface NoVa pour ingénieur Natran

> **Note (juillet 2026)** : l'audit §1 est **historique** (état avant implémentation). L'état réel du livré est en **§16** (Second Increment) et **§17** (post-Second Increment).

**Date** : juillet 2026
**Cible** : rendre GazFlow opérationnel pour Camille, ingénieure d'études réseau (GRT gaz) dont la mission quotidienne est la **validation de nominations** (NoVa) et l'analyse N-1.
**Sortie attendue** : un workflow NoVa complet en 2 clics — charger une nomination → verdict de faisabilité → causes → réduire & re-valider → export certification.

---

## 1. Contexte et diagnostic

### Persona (rappel)
Camille valide des nominations entry/exit contre la capacité réseau. Elle veut, dans l'ordre :
1. Un **verdict** (faisable / non, quels points en déficit).
2. La **cause** par point en déficit (reachabilité pression, compresseur, ancrage).
3. Le **débit max faisable** par sink pour négocier une réduction.
4. Un **rapport de certification** exportable.

### État actuel (audit)
- **Backend riche mais non exposé** : `scenario_pressure_slips`, `boundary_pressure_supply_reports` (max_up vs need), `upstream_pressure_trace`, `nova_capacity::study_default_marginal_sinks` existent dans `back/src/solver/` et sont utilisés par le binaire bench `compressor_diag`. **Aucun n'est renvoyé par l'API `/api/ws/sim`**.
- **Résultat simulation WS** (`back/src/api/ws.rs` + `front/src/services/api.ts` `SimulationResult`) : `pressures`, `flows`, `iterations`, `residual`, `capacity_violations`, `adjusted_demands`, `active_bounds`, `infeasibility_diagnostic`, `equipment_states`, `warnings`, `demand_scale_achieved`. Pas de slips pression, pas de trace amont, pas de capacité par sink.
- **Frontend** : `SimulationPanel.vue` empile config + résultats sur une colonne infinie ; pas de verdict explicite ; pas d'objet « Nomination » ; nav plate mêlant écrans techniques et analyses métier ; vocabulaire solveur (« Demandes », « Organes », « Mode robuste (continuation) »).
- **Aucun endpoint `/api/nova/*`**.

### Cible (information architecture)
```
┌─ Barre de workflows (gauche) ──────────────────┐
│  ◆ Valider une nomination (NoVa)   ← focus      │
│  ◇ Analyser N-1                                 │
│  ◇ Caler sur SCADA                              │
│  ◇ Transitoire                                  │
└──────────────────────────────────────────────────┘
Espace central : Carte (coloration écart à borne) + carte de verdict en haut.
Panneau droit contextuel : Nomination / Capacité par sink / Détails solveur (replié).
```

---

## 2. Workstreams et séquencement

Priorisation par ratio valeur/effort. Les **3 premiers workstreams** s'appuient sur du backend déjà existant : c'est du *surfacing*, pas du développement moteur.

| WS | Titre | Effort | Valeur | Dépendance |
|----|-------|--------|--------|------------|
| WS0 | Backend : exposer les diagnostics NoVa dans l'API | Moyen | Sine qua non | — |
| WS1 | Frontend : carte de verdict + déficits triés | Faible | Énorme | WS0 |
| WS2 | Frontend : capacité par sink + action réduire | Moyen | Énorme | WS0 |
| WS3 | Frontend : diagnostic geospatial (trace au clic) | Moyen | Énorme | WS0 |
| WS4 | Frontend : objet Nomination first-class | Moyen | Grand | WS0 |
| WS5 | Frontend : IA workflows + nav gauche | Grand | Grand | WS1-4 |
| WS6 | Frontend : vocabulaire métier | Faible | Moyen | WS5 |
| WS7 | Frontend + backend : export rapport certification | Moyen | Moyen | WS1, WS2 |

**Recommandation** : livrer WS0 + WS1 + WS2 + WS3 comme **Premier Increment NoVa** (Camille a un workflow complet). WS4-WS7 en Second Increment.

---

## 3. WS0 — Backend : exposer les diagnostics NoVa dans l'API

### Objectif
Le endpoint `/api/ws/sim` renvoie déjà un résultat. On **enrichit le DTO** avec les diagnostics existants, et on ajoute un endpoint dédié `/api/nova/capacity` pour l'étude capacité (coûteuse, opt-in).

### 3.1 Enrichir `SimulationResult` (slips + boundary supply)

**Fichiers** :
- `back/src/api/ws.rs` — dans la branche qui construit le résultat final (autour de L800-880, après `check_capacity_violations`), ajouter :
  ```rust
  let pressure_slips = solver::scenario_pressure_slips(&network, &result);
  let boundary_supply = solver::boundary_pressure_supply_reports(&network, &result, &pressure_slips, 12);
  ```
  et les sérialiser dans le payload WS envoyé au client.
- `back/src/solver/steady_state.rs` — `ScenarioPressureSlip` et `BoundaryPressureSupplyReport` sont déjà `serde::Serialize`. Vérifier qu'ils sont ré-exportés depuis `back/src/solver/mod.rs`.
- `front/src/services/api.ts` — ajouter les DTOs :
  ```ts
  export interface PressureSlipDto {
    node_id: string;
    solved_pressure_bar: number;
    lower_bar: number | null;
    upper_bar: number | null;
    shortfall_bar: number;
    excess_bar: number;
    from_scenario_envelope: boolean;
    shortpipe_partner_id: string | null;
  }
  export interface BoundarySupplyDto {
    node_id: string;
    required_lower_bar: number | null;
    solved_pressure_bar: number;
    max_upstream_pressure_bar: number;
    upstream_hops: number;
    supply_gap_bar: number;
  }
  ```
  et étendre `SimulationResult` avec `pressure_slips?: PressureSlipDto[]` et `boundary_supply?: BoundarySupplyDto[]`.

**Gate de coût** : `scenario_pressure_slips` + `boundary_pressure_supply_reports` sont O(nœuds × hops) — négligeable vs un solve. Pas d'opt-in nécessaire.

### 3.2 Endpoint `/api/nova/capacity` (étude capacité opt-in)

**Raison** : `study_default_marginal_sinks` lance ~5-15 solves séquentiels (~15-45 min sur 582). Coûteux → endpoint séparé, asynchrone, opt-in, jamais dans le flow `/api/ws/sim`.

**Fichiers** :
- `back/src/api/mod.rs` — nouvelle route `.route("/api/nova/capacity", post(nova::post_nova_capacity))`.
- `back/src/api/nova.rs` (nouveau) — handler qui :
  1. Reçoit `{ network_id?, nomination?: Record<string,number>, bisection_steps?: usize, sink_ids?: string[] }`.
  2. Charge le réseau + scénario, applique `network_with_scenario_boundaries`.
  3. Appelle `nova_capacity::study_sink_max_feasible_delivery` par sink (séquentiel — voir note deadlock rayon dans `gaslib-582-compressor-diagnosis.md`).
  4. Renvoie `{ reports: SinkCapacityReport[], residual_nominal_m3s: number, status: 'ok'|'error' }`.
- `back/src/solver/nova_capacity.rs` — `SinkCapacityReport` déjà `Serialize`. Généraliser `study_default_marginal_sinks` pour accepter une liste de sink_ids explicite (pas seulement les 5 par défaut).
- `front/src/services/api.ts` — `SinkCapacityReportDto` + `runNovaCapacity(req): Promise<NovaCapacityResponse>`.

**Note perf** : envisager un mode synchrone avec timeout (small networks) + un mode async (job UUID, polling) pour 582. Pour le Premier Increment, **synchrone avec progress WS** réutilise le canal existant.

### 3.3 Acceptance WS0
- [ ] `/api/ws/sim` renvoie `pressure_slips` et `boundary_supply` peuplés pour mild_618.
- [ ] `/api/nova/capacity` renvoie les 5 `SinkCapacityReport` (frac=0 pour 88/83/108/122, frac=1 pour 125).
- [ ] Tests backend : snapshot du payload WS incluant les slips ; test endpoint capacity sur un réseau trivial.
- [ ] Pas de régression sur `cargo test` existant.

---

## 4. WS1 — Frontend : carte de verdict + déficits triés

### Objectif
Remplacer la ligne « Convergence en N itérations (résidu) » par une **carte de verdict NoVa** en haut de l'espace de travail.

### 4.1 Composant `VerdictCard.vue` (nouveau, `front/src/components/`)

Affiche :
- **Statut binaire** : ✅ Faisable / ⛔ Non faisable (dérivé de `pressure_slips.length === 0 && demand_scale_achieved >= 1`).
- **Résumé** : « N points de livraison en déficit pression » + liste triée par `shortfall_bar` décroissant (top 5, repliable pour le reste).
- **Cause racine** quand disponible : si `boundary_supply[i].supply_gap_bar > 0` pour tous les sinks déficitaires → badge « Reachabilité pression (pas un déficit compresseur) ».
- **CTA** : `[Voir le diagnostic]` (focus carte sur le pire sink) + `[Tester une nomination réduite]` (scroll vers le tableau capacité WS2).

### 4.2 Store

- `front/src/stores/simulate.ts` — exposer `pressureSlips` et `boundarySupply` depuis le résultat WS (mapper les nouveaux champs DTO). Ajouter un getter `verdict` : `{ feasible: boolean, deficitSinks: PressureSlipDto[], cause: 'pressure_reachability' | 'capacity' | 'unknown' }`.

### 4.3 Intégration

- `front/src/pages/MapPage.vue` — insérer `<VerdictCard>` en overlay au-dessus de la carte (z-index overlay), visible seulement si `simulateStore.result` est présent.
- `SimulationPanel.vue` — retirer la ligne « Convergence en N itérations (résidu) » et le bloc « Pressions (N) » / « Débits (N) » par défaut ; les basculer dans un onglet « Détails solveur » (WS5).

### 4.4 Acceptance WS1
- [ ] Au chargement du résultat mild_618, la carte affiche ⛔ Non faisable, 4 sinks en déficit (88/83/108/122), sink_125 absent (OK).
- [ ] Tri par gravité ; le pire sink (sink_88, 23,4 bar) en tête.
- [ ] Badge cause « Reachabilité pression » visible.
- [ ] CTA `[Voir le diagnostic]` centre la carte sur sink_88.

---

## 5. WS2 — Frontend : capacité par sink + action réduire

### Objectif
Surfacer le `sink_capacity_report` et offrir le levier NoVa : appliquer le Q max faisable par sink puis re-valider.

### 5.1 Composant `SinkCapacityTable.vue` (nouveau)

Tableau :
| Point de livraison | Q nominé (Nm³/s) | Q max faisable | % | Verdict |
- Données depuis `runNovaCapacity()`.
- Badge verdict : ✅ / ⛔ unreachable / ⛔ pression amont < borne.
- **Bouton par ligne** `[Appliquer]` → écrit `max_feasible_q` dans `demandOverrides[sinkId]`.
- **Bouton global** `[Appliquer la capacité max partout]` → remplit tous les sinks à leur Q max, puis déclenche `startSimulation()`.

### 5.2 Store

- `front/src/stores/simulate.ts` (ou nouveau `novaStore`) — `novaCapacityReports`, `novaLoading`, `runNovaCapacity()` qui appelle l'API WS0 §3.2.

### 5.3 Intégration

- `SimulationPanel.vue` — section « Capacité par sink » entre la carte de verdict et les détails, avec bouton `[Lancer l'étude capacité]` (coûteux → explicite, avec warning temps estimé).

### 5.4 Acceptance WS2
- [ ] Bouton lance l'étude ; progress visible ; résultats affichés pour 5 sinks.
- [ ] `[Appliquer]` sur sink_108 met `demandOverrides['sink_108'] = 0` et marque la nomination « modifiée ».
- [ ] `[Appliquer la capacité max partout]` + re-sim → verdict passe à ✅ (la nomination réduite est faisable, attendu vu capacity=0 partout sauf 125).

---

## 6. WS3 — Frontend : diagnostic geospatial (trace au clic)

### Objectif
Au clic sur un sink déficitaire, popover avec la trace amont et `max_up vs need`.

### 6.1 Backend (complément WS0)
- Ajouter au payload WS un `upstream_trace` par sink déficitaire (top 6) : réutiliser `solver::upstream_pressure_trace(&network, &result, &node_id, 6)`. Pour limiter la taille, ne l'envoyer que pour les sinks en `pressure_slips` (≤ 25). DTO :
  ```ts
  export interface UpstreamHopDto { node_id: string; pressure_bar: number; }
  export interface SinkDiagnosticDto {
    node_id: string;
    trace: UpstreamHopDto[];
    max_upstream_pressure_bar: number;
    required_lower_bar: number | null;
    supply_gap_bar: number;
  }
  ```
  Ajouter `sink_diagnostics?: SinkDiagnosticDto[]` à `SimulationResult`.

### 6.2 Composant `SinkDiagnosticPopover.vue` (nouveau)

Au clic d'un nœud en déficit sur la carte :
- Trace amont : liste `nœud → pression`, mise en évidence du point où la pression s'effondre.
- Ligne « Pression amont max reachable : X bar — besoin contractuel : Y bar — gap : Z bar ».
- Mention « Aucun compresseur / organe sur le chemin » si la trace ne croise pas de `CompressorStation`/`PressureRegulator` (à dériver des pipes du réseau).
- CTA `[Réduire ce sink à son Q max]` (lien vers WS2).

### 6.3 CesiumViewer
- `front/src/components/CesiumViewer.vue` — au clic entité, si le nœud est dans `simulateStore.sinkDiagnostics`, ouvrir le popover. Passer `sinkDiagnostics` en prop.

### 6.4 Coloration carte
- `front/src/utils/pressureColor.ts` existe. Étendre / ajouter `pressureDeficitColor(slip): {color, opacity}` : rouge si `shortfall_bar > 1`, orange si `0 < shortfall ≤ 1`, vert si OK. L'appliquer aux entités sink dans CesiumViewer via le `pressure_slips` du résultat.

### 6.5 Acceptance WS3
- [ ] Clic sur sink_88 → popover avec trace [`sink_88` 2,64 → `innode_269` 2,64 → ...], « max_up 2,64 — besoin 26,01 — gap 23,4 ».
- [ ] Sinks en déficit colorés en rouge sur la carte dès le résultat chargé.
- [ ] Sinks OK (sink_125) en vert.

---

## 7. WS4 — Frontend : objet Nomination first-class

### Objectif
La nomination (entry/exit) devient un objet explicite, éditable, importable, réutilisable entre workflows.

### 7.1 Composant `NominationPanel.vue` (nouveau)
- Tableau entry/exit : `point frontière | sens (entry/exit) | Q nominé | Q servi (résultat) | statut`.
- Import `.scn` (GasLib) ou CSV entry/exit.
- Édition inline d'une ligne.
- Actions : `[Réduire à X %]` (slider global), `[Réduire le sink sélectionné]`, `[Re-valider]`.

### 7.2 Store
- `front/src/stores/nomination.ts` (nouveau) — `entries: NominationEntry[]`, `nominationOverrides`, `applyReduction(factor)`, `importScn(file)`. Pont avec `demandOverrides` existant (source de vérité unique).

### 7.3 Acceptance WS4
- [ ] Import d'un `.scn` peuplée le tableau entry/exit.
- [ ] Slider « réduire à 70 % » multiplie les exits et re-sim.
- [ ] Cohérence : `NominationPanel` et `DemandControls` (legacy) restent synchronisés ; `DemandControls` migre vers un accordéon « Avancé » puis est supprimé en WS6.

---

## 8. WS5 — Frontend : IA workflows + nav gauche

### Objectif
Remplacer la nav plate `MainLayout.vue` par une **barre de workflows** à gauche.

### 8.1 `MainLayout.vue` (refonte)
- `q-drawer` gauche : liste de workflows (Valider / N-1 / Caler / Transitoire) avec icônes.
- « Import » et « Exports » deviennent des boutons d'action dans le workflow concerné, pas des entrées de nav.
- Header : garde `GazFlow` + refresh + info.

### 8.2 Pages
- Renommer/structurer : `MapPage` devient l'espace de travail du workflow « Valider ». Les sections N-1 / Calage / Transitoire gardent leurs pages mais accédées via le drawer.
- `front/src/router/routes.ts` — ajuster les routes et le `meta.workflow`.

### 8.3 Acceptance WS5
- [ ] Le drawer gauche liste 4 workflows.
- [ ] « Import » accessible comme action dans « Valider » et via un bouton header, plus comme destination de nav.
- [ ] Routes préservées (pas de casse de liens profonds).

---

## 9. WS6 — Vocabulaire métier

### Objectif
Traduire le jargon solveur en vocabulaire réseau.

### 9.1 Glossaire appliqué (fichiers impactés)
| Terme actuel | Nouveau | Fichier |
|--------------|---------|---------|
| Demandes | Nomination (entry/exit) | `SimulationPanel.vue`, `DemandControls.vue`, stores |
| Organes | Composants réseau | `SimulationPanel.vue`, `equipmentLabels.ts` |
| Mode robuste (continuation) | Lancement par paliers (grands réseaux) | `SimulationPanel.vue` |
| Résidu / itérations | Détails solveur (replié) | `SimulationPanel.vue` |
| Demandes ajustées | Quantités relaxées par le solveur | `SimulationPanel.vue` |
| Libre / Vérifier / Optimiser | Simulation libre / Vérifier la nomination / Chercher une solution réalisable | `SimulationPanel.vue`, `simulationStatus.ts` |

### 9.2 Acceptance WS6
- [ ] Aucune occurrence utilisateur-visible de « résidu », « continuation », « demandes » dans le workflow NoVa.
- [ ] Tooltips reformulés en métier.

---

## 10. WS7 — Export rapport de certification

### Objectif
Un export pré-formaté joint à la décision de certification.

### 10.1 Backend
- `back/src/api/export.rs` — nouveau format `certification` : génération d'un PDF (ou HTML→PDF) avec : nomination en entrée, verdict, tableau des déficits + cause, capacités par sink, traces amont top 5. Réutiliser les DTOs WS0.
- Endpoint : `GET /api/exports/{id}/download?format=certification` (étendre le switch existant).

### 10.2 Frontend
- `SimulationPanel.vue` — bouton `[Rapport de certification]` à côté des exports raw.

### 10.3 Acceptance WS7
- [ ] Le rapport PDF contient verdict, tableau déficits, capacités, trace sink_88.
- [ ] Lisible hors ligne (PDF autonome).

---

## 11. Risques et mitigations

| Risque | Mitigation |
|--------|------------|
| Étude capacité (~15-45 min sur 582) bloque l'UI | Endpoint async + progress WS ; bouton explicite avec estimation ; défaut OFF dans `/api/ws/sim` |
| Deadlock rayon si on parallélise l'étude capacité | Garder séquentiel (cf. `gaslib-582-compressor-diagnosis.md` Phase VII-bis) |
| Taille du payload WS (upstream_trace × 25 sinks) | Limiter à 25 sinks × 6 hops = 150 entrées ; négligeable |
| Casse des liens profonds lors de la refonte IA (WS5) | Préserver les noms de routes ; redirect ancienne nav vers nouveaux workflows |
| `DemandControls` legacy vs `NominationPanel` (WS4) | Source de vérité unique (`nominationStore`) ; `DemandControls` en accordéon « Avancé » puis suppression |
| Performance Cesium (coloration 582 entités) | Coloration uniquement des sinks + nœuds frontière, pas des 582 nœuds internes |

---

## 12. Premier Increment NoVa — définition of done

Livrable minimal qui rend Camille opérationnelle :

1. **WS0** : `/api/ws/sim` renvoie `pressure_slips` + `boundary_supply` + `sink_diagnostics` ; `/api/nova/capacity` opérationnel.
2. **WS1** : `VerdictCard` visible sur `MapPage`.
3. **WS2** : `SinkCapacityTable` + boutons « Appliquer » + re-validation.
4. **WS3** : coloration carte par écart à borne + `SinkDiagnosticPopover` au clic.

**Critère Camille** : « Je charge mild_618 → je vois ⛔ Non faisable avec 4 sinks en déficit → je clique sink_88 → je vois max_up 2,64 vs besoin 26 → je lance l'étude capacité → je clique "Appliquer la capacité max partout" → je re-valide → ✅ Faisable → j'exporte. » sans jamais ouvrir un JSON ni scroller une liste de 582 pressions.

---

## 13. Estimation (ordres de grandeur)

| WS | Effort développeur |
|----|---------------------|
| WS0 | 2-3 jours (backend enrichissement + endpoint capacity async) |
| WS1 | 1 jour |
| WS2 | 1,5 jour |
| WS3 | 2 jours (Cesium interaction + coloration) |
| **Premier Increment** | **~6-8 jours** |
| WS4 | 2-3 jours |
| WS5 | 3-4 jours |
| WS6 | 0,5 jour |
| WS7 | 2 jours |
| **Total** | **~13-17 jours** |

---

## 14. Fichiers impactés (récap)

### Backend
- `back/src/api/ws.rs` — enrichir payload résultat.
- `back/src/api/mod.rs` — route `/api/nova/capacity`.
- `back/src/api/nova.rs` (nouveau) — handler étude capacité.
- `back/src/api/export.rs` — format certification (WS7).
- `back/src/solver/mod.rs` — ré-exports diagnostics.
- `back/src/solver/nova_capacity.rs` — généraliser sink_ids.

### Frontend
- `front/src/services/api.ts` — DTOs + `runNovaCapacity`.
- `front/src/stores/simulate.ts` — `pressureSlips`, `boundarySupply`, `sinkDiagnostics`, `verdict`.
- `front/src/stores/nomination.ts` (nouveau, WS4).
- `front/src/components/VerdictCard.vue` (nouveau).
- `front/src/components/SinkCapacityTable.vue` (nouveau).
- `front/src/components/SinkDiagnosticPopover.vue` (nouveau).
- `front/src/components/NominationPanel.vue` (nouveau, WS4).
- `front/src/components/SimulationPanel.vue` — restructuration.
- `front/src/components/CesiumViewer.vue` — coloration + clic popover.
- `front/src/layouts/MainLayout.vue` — drawer workflows (WS5).
- `front/src/utils/pressureColor.ts` — `pressureDeficitColor`.
- `front/src/utils/equipmentLabels.ts`, `simulationStatus.ts` — vocabulaire (WS6).

---

## 15. Suite (hors présent plan)

- **Scénario de nomination réduite** : bench automatisé d'une nomination tronquée (validation du verdict ✅ attendu).
- **Transposition** : rejouer le pipeline sur d'autres datasets GasLib pour confirmer la généralité du verdict NoVa.
- **N-1 comme workflow NoVa** : brancher `ContingencyPage` sur le même `VerdictCard` (une contingence = une nouvelle nomination à valider).

---

## 16. Statut d'exécution (juillet 2026)

### Audit de cohérence (avant exécution)
Le plan qualifiait WS0 de « pur surfacing ». **Gap de cohérence trouvé** : le chemin `/api/ws/sim` n'applique **pas** les enveloppes pression scénario (`network_prepared` ne charge pas le `.scn`). Appeler directement `scenario_pressure_slips(&network_prepared, …)` relirait le plancher générique `.net` (~2,01325 bar) au lieu des bornes contractuelles (~26 bar) — soit exactement le bug Phase VII-bis. **WS0 n'est donc pas du pur surfacing** : il faut que l'API simulation charge le scénario pour obtenir les bornes contractuelles.

### Réalisation — Premier Increment NoVa

**WS0a — Backend : diagnostics NoVa scenario-aware** ✅
- Nouveau module `back/src/solver/nova_diagnostics.rs` : `compute_nova_diagnostics(network, scenario, result)` clone le réseau, applique `apply_scenario_pressure_envelopes` (bornes contractuelles), puis calcule `pressure_slips` + `boundary_supply` + `sink_diagnostics` (trace amont + `supply_gap`). + `nova_verdict` (cause `Feasible` / `PressureDeficit` / `PressureReachability`).
- `back/src/gaslib/scenario.rs` : `resolve_scenario_path(dat_dir, dataset, scenario_id)` public (généralise le résolveur du binaire bench).
- `back/src/api/ws.rs` : `StartOptions.scenario_id` optionnel ; chargement + `enrich_scenario_with_balance_hub` côté handler ; les 3 branches `Converged` (Normal/Check/Constrained) attachent `pressure_slips`, `boundary_supply`, `sink_diagnostics`, `nova_verdict` au message. *(Historique WS0a)* : le solve restait sur `network_prepared` et le scénario ne servait qu'à évaluer le résultat post-hoc. **État actuel (§17)** : avec `scenario_id`, `resolve_simulation_demands` charge les Q du `.scn` et fusionne les overrides client ; les diagnostics pression restent post-hoc sur enveloppes contractuelles.
- Tests : 4 unitaires (`nova_diagnostics`) + 1 intégration GasLib-582/mild_618 (garde-contre le bug Phase VII-bis : borne contractuelle ≥ 20 bar détectée) — **6 tests backend verts**.

**WS4 (amorce) — Endpoint `/api/nova/scenarios`** ✅
- `back/src/api/nova.rs` : `list_nova_scenarios` scanne récursivement les `.scn` du dataset actif (id = stem du fichier, aligné sur `scenario_id`). + test.

**Frontend — contrat + store** ✅
- `front/src/services/api.ts` : DTOs `ScenarioPressureSlip`, `BoundaryPressureSupplyReport`, `UpstreamHop`, `SinkDiagnostic`, `NovaVerdict`, `NovaScenarioSummary` ; champs NoVa dans `SimulationResult` ; `api.listNovaScenarios()`.
- `front/src/services/ws.ts` : `WsStartOptions.scenario_id` ; champs `converged` ; `mergeConvergedMessage` propage les diagnostics. + test de contrat.
- `front/src/stores/simulate.ts` : refs `pressureSlips`, `boundarySupply`, `sinkDiagnostics`, `novaVerdict`, `activeScenarioId`, `novaActive` ; peuplés au `converged`, réinitialisés au reset. — **80 tests frontend verts**.

**WS1 — VerdictCard** ✅
- `front/src/components/VerdictCard.vue` : bannière verte/rouge (faisable / déficit), cause, action « Voir les points déficitaires » → sélection du premier sink sur la carte.

**WS3 — Diagnostics géospatiaux** ✅ (coloration + liste + popover au clic)
- `front/src/components/SinkDiagnosticsList.vue` : liste repliable des sinks déficitaires (besoin, pression résolue, manque amont, trace amont) ; clic → sélection du nœud sur la carte.
- `front/src/components/SinkDiagnosticPopover.vue` (WS3-fin) : inspector flottant (fixed bottom-right) du sink déficitaire **sélectionné** — borne contractuelle, pression résolue, manque amont, trace amont, CTA `[Réduire à Q max]` (si étude capacité disponible) ou `[Étudier la capacité]`. Fermeture → `clearSelection`.
- `front/src/components/NovaScenarioPicker.vue` : select des nominations `.scn` (charge `/api/nova/scenarios`), émet `scenario_id`.
- `front/src/components/CesiumViewer.vue` : `applyNovaDeficitHighlights` colore les sinks déficitaires en rouge (`#ff1744`) par-dessus la coloration pression ; watch sur `pressureSlips`. **Sélection de nœud autorisée en mode visualisation** (`handleViewModeNodePick`) — Camille n'a pas à entrer en mode édition pour inspecter un point déficitaire.
- `front/src/components/SimulationPanel.vue` : picker + VerdictCard + SinkDiagnosticsList + SinkDiagnosticPopover intégrés ; `scenario_id` passé au démarrage WS.

### Réalisation — Second Increment NoVa

**WS0b — Backend : endpoint `/api/nova/capacity`** ✅
- `back/src/solver/nova_capacity.rs` : nouvelle API publique `study_sinks_capacity(network, scenario, sink_ids, preset, gas, bisection_steps)` — **non gated** par `GAZFLOW_NOVA_SINK_CAPACITY_STUDY` (le bench reste opt-in via le flag, l'API non). `study_sink_max_feasible_delivery` prend désormais `bisection_steps` explicite (au lieu de lire l'env) pour rester thread-safe sous charge HTTP. `study_default_marginal_sinks` refactorisé pour déléguer.
- `back/src/gaslib/scenario.rs` : `network_with_scenario_boundaries_for_nova` — variante qui **applique toujours les enveloppes pression contractuelles** (le `network_with_scenario_boundaries` standard ne le fait que si `GAZFLOW_SCENARIO_PRESSURE_ENVELOPES=1`). Corrige le même gap de cohérence que WS0a : l'API capacité ne dépend plus d'un flag env global.
- `back/src/api/nova.rs` : `post_nova_capacity` — `POST /api/nova/capacity` (body `NovaCapacityRequest { scenario_id, sink_ids?, bisection_steps?, robust_mode?, max_iter? }`). Charge + enrichit le scénario, acquiert un slot simulation (`simulation_slots`), `spawn_blocking` + `rayon_pool.install` (solveur séquentiel), cap à 12 sinks, dichotomie défaut 6, robust défaut true, timeout 120 s. Réponses 404 / 422 / 503 / 500 typées.
- Route enregistrée dans `back/src/api/mod.rs`.
- Tests : `study_sinks_capacity_reports_zero_when_bound_unreachable` (réseau minimal, borne 80 bar inaccessible depuis source 70 bar → fraction ~0, valide la dichotomie + bornes contractuelles) + `post_nova_capacity_returns_404_for_unknown_scenario` (HTTP oneshot sur router de test). Scratch dirs par test (fix concurrence). **8 tests backend NoVa verts**.

**Frontend — contrat capacité + store** ✅
- `front/src/services/api.ts` : DTO `SinkCapacityReport` + `NovaCapacityRequest` ; `api.runNovaCapacity(payload)`.
- `front/src/stores/simulate.ts` : refs `sinkCapacity`, `capacityLoading`, `capacityError` ; action `runSinkCapacity(sinkIds?)` (passe les sinks déficitaires par défaut, garde `activeScenarioId`) ; réinitialisés au reset. **2 nouveaux tests store** (peuplement depuis API + erreur sans scénario) → **82 tests frontend verts**.

**WS2 — Frontend : SinkCapacityTable + action réduire** ✅
- `front/src/components/SinkCapacityTable.vue` : tableau des rapports capacité (Q nominal, Q max faisable, fraction % colorée, P@max, borne contractuelle). Boutons par ligne `[Réduire]` (sink déficitaire uniquement) + CTA `[Tout réduire au faisable]`. Emits `run-study`, `reduce(sinkId, maxQ)`, `reduce-all`.
- `front/src/components/SimulationPanel.vue` : table intégrée sous VerdictCard/DiagnosticsList ; `runCapacityStudy` passe les `deficitSinkIds` (depuis `sinkDiagnostics` puis `novaVerdict.deficit_sinks`) ; `onReduceSink`/`onReduceAll` injectent `demandOverrides[sink] = -|maxFeasibleQ|` puis relancent la simulation (re-validation NoVa).

**WS4 (fin) — Objet `Nomination` first-class** ✅
- `front/src/stores/nomination.ts` : store dédié portant la nomination active au-delà d'un `scenario_id` — `list`, `selected` (`NovaScenarioSummary`), `loading`, `activeId`/`activeFilename` computed, `load(force)`, `selectById`, `clear`, `reset`. Au rechargement, désélectionne une nomination qui n'existe plus pour le réseau courant.
- `front/src/components/NominationPanel.vue` : section « Nomination » (icône `assignment`) intégrant le select des `.scn` (store-backed) + carte résumé de la nomination active (filename + chemin relatif + bouton désélectionner). Remplace le `NovaScenarioPicker` standalone (supprimé).
- `front/src/components/SimulationPanel.vue` : `novaScenarioId` devient un computed miroir de `nominationStore.activeId` (source de vérité unique) ; `startSimulation`/`scenarioDirty`/`launchLabel` lus depuis le store.
- Tests : `stores/nomination.spec.ts` (4) — selectById/clear/load+droop/activeId → **86 tests frontend verts**.

### Statut WS5 / WS6 (Second Increment — suite, juillet 2026)

- **WS5 partiel** : stepper NoVa Verdict → Causes → Capacité → Export (`NovaWorkflowStepper` dans l'Espace d'analyse et la carte) ; pas de drawer workflows dédié (nav « Tâches » encore plate).
- **WS6 partiel** : vocabulaire métier NoVa dans l'Espace d'analyse et la carte (`novaLabels`, nomination, soutirages, réglages équipements) ; jargon solveur possible sur calage / transitoire.

**WS7 — Rapport de certification** ✅
- `front/src/components/CertificationReportDialog.vue` : dialog de rapport de certification NoVa (verdict + cause, table des points déficitaires avec trace amont, table capacité par sink, métadonnées réseau/nomination/date/run). Actions `[Imprimer / PDF]` (fenêtre dédiée HTML print-ready → `window.print`) et `[Exporter JSON]` (Blob `rapport-certification-<nomination>.json` assemblé côté client depuis les stores). Rapport construit côté client (`buildReport`) — pas de round-trip backend, pas d'état serveur à porter.
- `front/src/components/SimulationPanel.vue` : bouton `Rapport de certification` (visible quand `novaActive`) + dialog intégré (`showReport`).
- **Le test Camille est désormais complet bout-en-bout** : charger mild_618 → ⛔ → cliquer sink_88 → max_up vs besoin → étude capacité → Appliquer la capacité max partout → re-valider → ✅ → exporter le rapport de certification. Sans JSON, sans scroller 582 pressions, sans lire « résidu ».
- Tests : `vue-tsc` OK, **86 tests frontend verts** (dialog présentationnel lecture-stores ; pas de mount harness ajouté).

### Cycles de review UX (interface / actions / flux — centrés persona Camille)
Trois passes de review/correction centrées sur l'interface, les actions et les flux d'utilisation (juillet 2026).

**Cycle 1 — Flow NoVa (le test Camille)** : remontée du `NovaScenarioPicker` en tête de config (point d'entrée nomination, avant `DemandControls`) ; vol caméra vers le nœud sélectionné en mode visualisation (`CesiumViewer` watch `selectedId` → `viewer.flyTo`, skip en édition) — Camille voit le sink déficitaire sur le 582 sans scroller ; auto-ouverture du `SinkCapacityTable` quand le verdict est défavorable (`default-opened` suit `novaVerdict?.feasible === false`) ; libellé `[Appliquer la capacité max partout]` (vocabulaire persona).

**Cycle 2 — Actions & feedback** : bouton `Lancer` → label dynamique `Valider la nomination` quand un scénario est actif ; retrait du jargon solveur — `Mode robuste (continuation)` → `Convergence renforcée`, modes `Libre/Optimiser` → `Standard/Optimiser capacités` ; banner `Nomination modifiée — relancez pour re-valider la tenue pression.` via `scenarioDirty` (`lastRunScenarioId`) pour éviter un verdict stale après changement de nomination.

**Cycle 3 — IA & triangle carte↔liste↔popover** : lisibilité trace amont (`text-grey-4` au lieu de `grey-6`/`grey-7`) + clé `v-for` `${node_id}-${i}` (anti-doublon si la trace reboucle) dans `SinkDiagnosticsList` et `SinkDiagnosticPopover` ; état vide du `SinkCapacityTable` adapté au verdict (OK → « négocier les marges », déficitaire → « négocier une réduction »). Confirmation de la chaîne : verdict → `Voir les points déficitaires` / clic liste / clic carte → sélection + vol + popover → `Réduire` ou `Étudier la capacité` → re-validation.

### Tests de validation ajoutés (cumul)
- `back` : `solver::nova_diagnostics::tests::*` (4), `solver::nova_capacity::tests::study_sinks_capacity_reports_zero_when_bound_unreachable` (1), `api::nova::tests::collect_scenarios_walks_subdirs` (1), `api::nova::tests::post_nova_capacity_returns_404_for_unknown_scenario` (1) → 7 nouveaux backend.
- `front` : `services/ws.spec.ts` `merges NoVa diagnostic fields` (1) + `stores/simulate.spec.ts` `runSinkCapacity …` (2) → 3 nouveaux (total 82).
- Vérification non-régression : les 5 tests backend préexistants en échec (`test_gaslib_11_snapshot`, `test_parse_closed_valve_from_zero_flow_bounds`, `test_control_valve_closed_blocks_flow`, `test_newton_jacobi_hybrid_fallback`, `test_valve_closed_removes_arc_and_blocks_flow`) **sont en échec sur la base sans mes changements** (vérifié par `git stash`) — non introduits par ce travail (dérive Phase VII-bis en cours sur les vannes fermées / snapshot).

---

## 17. Post-Second Increment (juillet 2026)

Livrables additionnels sur `main` après le Second Increment NoVa :

- **Contrat certification** : `pressure_margins`, `solver_signature` (NewtonPosthoc / IpoptEscalation / Unresolved), cause `PressureExcess`, libellé utilisateur `NotSolvedLocal` → « Verdict non établi ».
- **`resolve_simulation_demands`** : avec `scenario_id`, charge les Q du `.scn` et fusionne les overrides client partiels avant le solve WS.
- **Nomination réduite** : `POST /api/nova/nominations/reduced` (unités natives m³/s, entries mass-balance à débit fixe).
- **N-1 branché** : `scenario_id` sur l'analyse de contingence, bornes scénario via `network_with_scenario_boundaries_for_nova`, CTA unique depuis ResultsRail / SimulationPanel, gates `scenarioDirty`.
- **IPOPT escalade opt-in** : `GAZFLOW_NOVA_IPOPT_ESCALATION` (`on` / `on-notsolved` / `maybe`) ; `BoundViolation` depuis pressions IPOPT.
- **Compresseur first-class** : API `/api/compressor/map-mode` et `/api/compressor/operating-points` ; UI `CompressorMapPanel` dans ResultsRail / SimulationPanel.
- **Smoke** : `scripts/nova/smoke-ipopt-escalation.sh`.

