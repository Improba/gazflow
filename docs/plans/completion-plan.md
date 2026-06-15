# Plan de complétion — GazFlow (100 % opérationnel)

> Document maître pour terminer le post-MVP P6–P13. Mode équipe, exécution autonome par vagues parallélisables.
> Complète `production-sprint-plan.md` (vagues A–C ✅) et remplace le backlog D–G par un découpage exécutable.

**Date** : 2026-06-14  
**Référence état code** : `cargo test --lib` 227/227 · `npm test` 56/56 · ~60 fichiers modifiés non commités sur `main`

---

## 1. Définition de « fini »

Un phase est **100 %** quand :

1. Toutes les lignes roadmap correspondantes sont ✅ (doc alignée sur le code).
2. Critères métier de la section « Done quand » sont vérifiables par test ou scénario corpus.
3. Front + back + export + WS (si applicable) cohérents (`apiContracts.spec.ts` vert).
4. Limitations restantes documentées dans `limitations.md` (pas de surprise exploitant).

| Phase | Avancement réel | Reste pour 100 % |
|-------|-----------------|------------------|
| P6 Import | ~100 % | Fixture Shapefile corpus, doc |
| P7 Physique | ~78 % | GERG/PR-78, thermique (option prod), Re–Q Jacobien |
| P8 Régulation | ~92 % | Cv ISA, Jacobien analytique (remplace FD) |
| P9 Profils | ~96 % | Linepack couplé (via P11), météo spatiale (option) |
| P10 N-1 | ~98 % | Roadmap doc seulement — code complet |
| P11 Transitoire | ~42 % | **PDE 1D + WS + player UI** (goulot XL) |
| P12 Édition | ~65 % | Scénarios diffs, compare ΔP/ΔQ, export GeoJSON édité |
| P13 Calage | ~82 % | LM multi-param, calage demandes, perf LM |
| Transversal | ~70 % | Commit, CI corpus, OpenAPI, export history API |

**Estimation globale** : ~3 à 4 mois calendaires en parallèle (2 agents backend + 1 front + revues), dont **6–10 semaines** sur P11 seul.

---

## 2. Architecture des vagues (équipe)

```
                    ┌── H0 Stabilisation (obligatoire, semaine 0)
                    │
    ┌───────────────┼───────────────┬───────────────┐
    │               │               │               │
    ▼               ▼               ▼               ▼
  Vague D         Vague E         Vague F         Vague H
  P12 édition     P8 régulation   P7 physique     P13 calage
  (M, 2 sem)      (M, 1–2 sem)    (M–L, 2–3 sem)  (L–XL, 3–4 sem)
    │               │               │               │
    └───────────────┴───────┬───────┴───────────────┘
                            │
                            ▼
                      Vague G — P11 PDE
                      (XL, 6–10 sem)
                            │
                            ▼
                      Vague I — Production
                      (S–M, 1–2 sem)
```

### Répartition agents (préférence mode équipe)

| Rôle | Modèle | Vagues |
|------|--------|--------|
| Architecture / risques / PDE | Opus ou Codex | G, H (LM multi), revues |
| Implémentation backend | Codex ou Composer 2.5 | D, E, F, G, H |
| Front Quasar/Cesium | Composer 2.5 | D, G (player), I |
| QA / CI / docs | Composer 2.5 | H0, I |

Max **3 sous-agents parallèles** ; intégration parent après chaque vague.

---

## 3. Vague H0 — Stabilisation (priorité immédiate, ~3 j)

**Objectif** : figer la base livrable avant d’ajouter des features.

| ID | Tâche | Livrable | Agent |
|----|-------|----------|-------|
| H0.1 | Split commits par phase (P9, P10, P13, revues phys/dev) | 4–6 PRs reviewables | shell |
| H0.2 | Intégrer corpus `docs/testing/corpus/` (+ script fetch CI) | Corpus versionné ou fetch documenté | Composer |
| H0.3 | Aligner `operational-roadmap.md` (P10 ✅, tests 227/56, P7 78 %) | Doc sans contradiction | Composer |
| H0.4 | CI : `verify_test_corpus.sh` + `validate_test_corpus.py` dans `scripts/ci.sh` | Pipeline vert | Composer |
| H0.5 | Étendre `apiContracts.spec.ts` (GasProperties.warnings, transient, contingency) | Contrats gelés | Composer |

**Done quand** : branche propre, CI verte, roadmap = réalité.

---

## 4. Vague D — P12 Édition complète (~2 sem)

**Prérequis** : H0.1  
**Parallèle avec** : E, F

### D1. Modèle scénarios backend

| Fichier | Action |
|---------|--------|
| `back/src/graph/scenarios.rs` | **Nouveau** : `NetworkSnapshot`, `NetworkDiff` (nodes/pipes add/remove/modify), `apply_diff(base, diff)` |
| `back/src/api/scenarios.rs` | **Nouveau** : CRUD `/api/scenarios` (list, create, get, delete, apply) |
| `back/src/api/mod.rs` | Brancher routes, persistance par `datasetId` (JSON côté import ou sous-dossier data) |

**Diff minimal** : `{ nodes: { added, removed, updated }, pipes: { ... } }` — pas de merge 3-way au MVP.

### D2. Simulation compare

| Fichier | Action |
|---------|--------|
| `back/src/api/mod.rs` | `POST /api/simulate/compare` → `{ base, variant }` → `{ delta_p, delta_q, summary }` |
| `front/src/pages/ComparePage.vue` | **Nouveau** ou panneau dans MapPage |
| `front/src/components/ComparePanel.vue` | Table ΔP/ΔQ, sélection scénario A/B |
| `front/src/components/CesiumViewer.vue` | Mode diff : couleur = sign(ΔP) |

### D3. Export GeoJSON édité

| Fichier | Action |
|---------|--------|
| `back/src/api/export.rs` | `export_network_geojson(network)` |
| `front/src/stores/editor.ts` | Bouton « Exporter réseau » |

### Tests

- `test_scenario_diff_roundtrip`
- `test_compare_two_topologies`
- Front : `editor.spec.ts` + compare store

**Done quand** : 2 variantes sauvegardées, simulées, comparées visuellement sur carte.

---

## 5. Vague E — P8 Régulation finition (~1–2 sem)

**Parallèle avec** : D, F

### E1. Vanne Cv ISA (remplace diamètre effectif)

| Fichier | Action |
|---------|--------|
| `back/src/solver/steady_state.rs` | Branche `ControlValve` : loi $Q = C_v N Y \sqrt{x P_1 / \rho_1}$ ou forme P² équivalente isotherme |
| `back/src/solver/newton.rs` | Dérivées $\partial Q / \partial P_i$ pour Cv |
| `docs/science/equations.md` | §4.6 : statut « implémenté ISA » |

### E2. Jacobien analytique régulateur

| Fichier | Action |
|---------|--------|
| `back/src/solver/regulator.rs` | Exporter `regulator_jacobian_contribution(...)` |
| `back/src/solver/newton.rs` | Remplacer FD par analytique quand `feature` ou flag config ; garder FD fallback |
| Test | `test_regulator_jacobian_analytic_matches_fd` (tolérance 1e-4) |

**Done quand** : T8-3 Cv avec loi ISA ; 8.7 ✅ ; pas de régression GasLib-11.

---

## 6. Vague F — P7 Physique finition (~2–3 sem)

**Parallèle avec** : D, E

### F1. GERG-2008 ou PR-78 (H₂ > 20 %)

| Fichier | Action |
|---------|--------|
| `back/src/solver/eos/gerg.rs` ou `pr78.rs` | **Nouveau** module EOS |
| `back/src/solver/gas_properties.rs` | `enum EosModel { PapayKay, Gerg2008 }` ; sélection auto si H₂ > 0.2 |
| `docs/science/limitations.md` | Domaine de validité par EOS |

**Option pragmatique** : crate `gerg2008` ou port minimal 5 composants (CH₄, C₂H₆, CO₂, N₂, H₂).

### F2. Couplage Re–Q outer-loop (option précision)

| Fichier | Action |
|---------|--------|
| `back/src/solver/newton.rs` | Flag `dynamic_re_jacobian` : Re(Q) dans ∂R/∂Q |
| Benchmark | GasLib-11 : objectif < 1 % vs référence si activé |

### F3. Profil thermique conduites (option « prod avancée »)

| Fichier | Action |
|---------|--------|
| `back/src/solver/thermal.rs` | **Nouveau** : T_line = f(T_soil, Q, U) stationnaire |
| Couplage | Boucle externe T → ρ, μ → steady solve |

**Peut être reporté post-100 %** si périmètre « exploitant HP isotherme » suffit ; documenter comme P7.11.

**Done quand** : warning H₂ déclenche GERG ; tests monotonie conservés ; doc §2.4 à jour.

---

## 7. Vague G — P11 Transitoire PDE (~6–10 sem) — GOULOT

**Prérequis** : E (régulateurs stables), F1 recommandé  
**Bloque** : couplage linepack P9, usage transitoire métier

### G1. Maillage 1D

| Fichier | Action |
|---------|--------|
| `back/src/solver/transient/mesh.rs` | Segments par pipe ; $N_x$ configurable ; géométrie depuis `Pipe` |
| `back/src/solver/transient/state.rs` | $P_i$, $Q_i$ par cellule |

### G2. Système PDE isotherme simplifié

Équations cibles (isotherme, 1D) :

- Continuité : $\partial \rho / \partial t + \partial (\rho v) / \partial x = 0$
- Momentum / P² : forme quasi-stationnaire avec terme $\partial P / \partial t$ via linepack local

| Fichier | Action |
|---------|--------|
| `back/src/solver/transient/system.rs` | Résidus + Jacobien sparse par pipe (tridiagonal) |
| `back/src/solver/transient/boundary.rs` | Source P fixe, sink Q, jonctions (continuité masse) |
| `back/src/solver/transient/time_integration.rs` | Euler implicite ; pas adaptatif optionnel |

### G3. Couplage organes P8 en transitoire

| Fichier | Action |
|---------|--------|
| `back/src/solver/transient/regulator.rs` | Régulateur : consigne aval dynamique |
| `back/src/solver/transient/events.rs` | Événements → modification frontière |

### G4. API + WebSocket

| Fichier | Action |
|---------|--------|
| `back/src/api/mod.rs` | Enrichir `POST /api/simulate/transient` (mode `pde` vs `quasi_steady`) |
| `back/src/api/ws.rs` | `start_transient_simulation` : stream `{ time_s, pressures, flows, linepack }` |
| `front/src/stores/transient.ts` | **Nouveau** store (pattern timeseries) |

### G5. UI transitoire

| Fichier | Action |
|---------|--------|
| `front/src/components/TransientPlayer.vue` | Timeline, play/pause, vitesse |
| `front/src/components/TransientChart.vue` | $P(t)$, $Q(t)$ nœud sélectionné |
| `front/src/components/LinepackGauge.vue` | Jauge masse emmagasinée |
| `front/src/pages/TransientPage.vue` | Remplacer MVP par player + choix mode |
| `CesiumViewer.vue` | Animation pression transitoire |

### Tests & validation

- Pipe single : fermeture vanne → onde pression (qualitative)
- Réseau 3 nœuds : conservation masse
- Régression : mode `quasi_steady` = comportement MVP actuel

**Done quand** : 11.1–11.12 roadmap ✅ ; limitation « pas thermique transitoire » explicite.

---

## 8. Vague H — P13 Calage avancé (~3–4 sem)

**Prérequis** : H0 ; peut démarrer en parallèle de G (backend seul)

### H1. Paramètres calables

| Fichier | Action |
|---------|--------|
| `back/src/calibration/mod.rs` | Étendre `CalibrationParameter` : `DemandScale { node_id }`, `PerPipeRoughness` vectorisé |
| `back/src/calibration/objective.rs` | Résidus pression + débit ; pondération σ |

### H2. LM multi-paramètres

| Fichier | Action |
|---------|--------|
| `back/src/calibration/lm.rs` | Vecteur θ (dim ≤ 20 au MVP) ; Jacobien FD par colonne ou adjoint |
| `back/src/calibration/sensitivity.rs` | **Nouveau** : $\partial \hat y / \partial \theta$ (1 solveur + perturbations) |
| `back/src/calibration/optimizer.rs` | Remplacer grid `PerPipe` par LM quand n_params > 1 |

### H3. Perf & UX

- Cache solution warm-start entre itérations LM
- `CalibrationPage.vue` : courbe convergence, export rapport PDF/CSV

**Done quand** : cas synthétique 5 tuyaux + 10 capteurs converge mieux que grid ; 13.3–13.7 ✅.

---

## 9. Vague I — Production & transversal (~1–2 sem)

**Prérequis** : H0 ; idéalement après D–H

| ID | Tâche | Détail |
|----|-------|--------|
| I1 | OpenAPI | `utoipa` ou spec YAML générée depuis routes Axum |
| I2 | Export history | `GET /api/exports`, `GET /api/exports/{id}` |
| I3 | Validation externe | Jeu GasLib pressions + débits `.sol` ; rapport `validation.md` |
| I4 | Timeseries export history | `store_export_record` sur timeseries / contingency / transient |
| I5 | README exploitant | Guide Natran : import → scénario → N-1 → calage |
| I6 | Durcissement WS | Rate limit, max payload, reconnexion front documentée |

---

## 10. P9 polish (optionnel pour 100 % strict)

| ID | Tâche | Priorité |
|----|-------|----------|
| P9.11 | Météo spatiale (zones / nœuds) | Basse |
| P9.12 | Graphiques timeseries enrichis (percentiles, export PNG) | Moyenne |
| P9.13 | Couplage inventaire entre pas horaires | **Haute** — dépend G |

---

## 11. Calendrier indicatif

| Semaine | Vagues actives | Jalons |
|---------|----------------|--------|
| S0 | H0 | Base commitée, CI corpus |
| S1–S2 | D + E + F (parallèle) | Compare scénarios · Cv ISA · GERG MVP |
| S3–S10 | G (PDE) | Maillage → système → WS → UI |
| S3–S6 | H (calage, parallèle G) | LM multi-param |
| S11–S12 | I + polish | OpenAPI, validation externe, doc finale |

**Chemin critique** : H0 → G (PDE) → I  
**Parallélisable** : D, E, F, H pendant G

---

## 12. Critères d’acceptation globaux (release « 1.0 exploitant »)

1. Import réseau GRDF-like → simulation 24 h → N-1 → calage SCADA sur corpus interne.
2. Édition variante → compare ΔP/ΔQ sans quitter l’UI.
3. Transitoire PDE : fermeture vanne modélisée (pas seulement quasi-steady).
4. Tous tests CI + corpus + 56 tests front verts.
5. `limitations.md` relu par métier ; pas de PCI/PCS confusion non documentée.
6. Aucun P0 ouvert (WS timeout, localStorage, contrats API).

---

## 13. Risques & arbitrages

| Risque | Impact | Mitigation |
|--------|--------|------------|
| P11 PDE dérape (XL) | Retard global | Livrer G par slices (1 pipe → réseau) ; garder quasi_steady |
| GERG licence / complexité | P7 incomplet | PR-78 en fallback ; Papay + warning au-delà 20 % H₂ |
| LM multi-param lent | P13 inutilisable | Limiter dim θ ≤ 20 ; warm-start obligatoire |
| Corpus volumineux | CI lente | Jobs séparés smoke vs full ; fetch optional |
| Non commité | Perte travail | **H0.1 immédiat** |

---

## 14. Ordre d’exécution autonome (checklist agent)

```
Phase 0  [H0] Stabiliser repo + CI + doc
Phase 1  [D]  P12 scénarios + compare          ║  [E] P8 Cv + Jacobien
Phase 1  [F]  P7 GERG + Re–Q                   ║  [H] P13 LM multi (backend)
Phase 2  [G]  P11 PDE (séquentiel interne G1→G5)
Phase 3  [I]  Production + validation externe
Phase 4  Revue Bugbot + Security + mise à jour roadmap 100 %
```

---

## 15. Suivi (à mettre à jour en fin de chaque vague)

| Vague | Statut | Date cible | Notes |
|-------|--------|------------|-------|
| A–C | ✅ | 2026-06-14 | production-sprint-plan |
| H0 | ✅ | 2026-06-14 | CI corpus, doc alignée |
| D | ✅ | 2026-06-14 | scenarios + compare |
| E | 🟡 | — | Cv MVP (ISA restant) |
| F | ✅ | 2026-06-14 | PR-78 auto |
| G | 🟡 | — | PDE scaffold + UI player ; réseaux complexes restants |
| H | ✅ | 2026-06-14 | LM ≤5 params |
| I | 🟡 | — | export list API ✅ ; OpenAPI stub restant |

---

*Prochaine action autonome recommandée : lancer **H0** (commit + CI corpus), puis **D + E + F** en parallèle.*
