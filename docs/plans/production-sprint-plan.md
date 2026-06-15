# Plan sprint production — GazFlow (juin 2026)

> Objectif : fermer les écarts MVP → production par vagues parallèles, sans bloquer sur P11 PDE (XL, 6–10 semaines).

## Priorisation

| Vague | Phases | Livrables | Effort | Dépendances |
|-------|--------|-----------|--------|-------------|
| **A** | P9 fin + transversal | Météo CSV, profils week-end, persistance profils ; CI tests stables | S | — |
| **B** | P10 fin | WS streaming N-1, overlay carte violations, export Excel multi-feuilles | M | P9 optionnel |
| **C** | P13 fin | LM global 1-param, carte résidus SCADA, rapport enrichi | M | — |
| **D** | P12 | Scénarios diffs topologiques, compare ΔP/ΔQ, export GeoJSON édité | M | P6 |
| **E** | P8 | Cv ISA complet, Jacobien analytique régulateur | M | P7 partiel |
| **F** | P7 | Validation ΔP H₂, profil thermique conduites, GERG >20 % H₂ | M | — |
| **G** | P11 | PDE 1D implicite, maillage, WS, TransientPlayer + jauge linepack | XL | P7, P8 |

**Goulot** : vague G (P11 PDE). Toutes les autres vagues sont indépendantes et livrables incrémentalement.

---

## Vague A — P9 + transversal (cette session)

### A1. Import météo CSV
- Backend : `import/weather.rs` — `hour,t_ext_c` (alias `temperature`, `t`)
- API : réutilise `WeatherStep[]` existant (pas de nouvel endpoint)
- Front : `parseWeatherCsv()` + upload fichier dans `ScenarioPanel.vue`
- Corpus : `docs/testing/corpus/synthetic/demand/weather-winter-day.csv`

### A2. Profils week-end
- Backend : `weekend_winter_weights()`, `weekend_winter_tertiary_weights()` dans `demand.rs`
- Presets : option catégorie ou flag `day_type: weekday|weekend` sur profil
- Front : sélecteur jour semaine / week-end dans ScenarioPanel

### A3. Persistance profils par nœud
- Front : `localStorage` clé `gazflow:demand-profiles:{datasetId}`
- Restauration au chargement réseau ; reset bouton

### A4. CI tests parallèles
- Crate `serial_test` ou module `#[serial]` sur tests GasLib-11 / capacity / sparse
- Documenter `--test-threads=2` dans README dev

---

## Vague B — P10 N-1 production

### B1. WebSocket `start_contingency_simulation`
- Messages : `contingency_started`, `contingency_case`, `contingency_finished`
- Streaming cas par cas (progress bar front)

### B2. Overlay carte
- `CesiumViewer` : surlignage rouge/vert des nœuds violés (dernier cas sélectionné)
- `ContingencyPage` : clic ligne → highlight carte

### B3. Export Excel N-1
- Feuilles : `summary`, `cases`, `violations`
- Endpoint ou client-side via lib existante export

---

## Vague C — P13 calage production

### C1. Levenberg-Marquardt (global roughness)
- `calibration/lm.rs` : 1 paramètre, Jacobien FD, λ adaptatif
- Fallback sur grid search actuel si non-convergence

### C2. Carte résidus SCADA
- `CalibrationPage` : barres par nœud (pression) + couleur carte si nœud géolocalisé

### C3. Multi-paramètres (phase 2)
- Demande + rugosité : hors scope sprint 1

---

## Vague D–G — backlog structuré

Voir `operational-roadmap.md` sections P12, P8, P7, P11 pour détail tâches.

### P11 PDE (vague G, prochaine itération dédiée)
1. Maillage 1D par conduite (segments = arcs graphe)
2. Équation $\partial P / \partial t + \ldots$ simplifiée isotherme + linepack local
3. Euler implicite tridiagonal par pipe
4. Couplage jonctions (continuité masse)
5. WS + TransientPlayer + jauge linepack Cesium

---

## Critères « production » par phase

| Phase | Done quand |
|-------|------------|
| P9 | Météo fichier, week-end, profils persistés, 24h+ séries custom |
| P10 | N-1 WS + Excel + carte violations |
| P13 | LM converge ≥ grid search, résidus visibles par nœud |
| Transversal | `cargo test --lib` 100 % en parallèle default |

---

## Suivi session en cours (2026-06-14)

| Tâche | Statut |
|-------|--------|
| A1 Import météo CSV (`import/weather.rs`, `weatherCsv.ts`, ScenarioPanel) | ✅ |
| A2 Profils week-end (`DayType`, presets weekend) | ✅ |
| A3 Persistance profils (`stores/demandProfiles.ts`, localStorage) | ✅ |
| A4 CI tests parallèles (`serial_test`, 219/219) | ✅ |
| B1 WS streaming N-1 (`start_contingency_simulation`) | ✅ |
| B2 Overlay carte violations (CesiumViewer + ContingencyPage) | ✅ |
| B3 Export N-1 XLSX/CSV (`POST /api/contingency/export`) | ✅ |
| C1 Levenberg-Marquardt global (`calibration/lm.rs`) | ✅ |
| C2 Carte résidus SCADA (CalibrationPage + CesiumViewer) | ✅ |

**Vérification (2026-06-15)** : `cargo test --lib` → 240/240 ; `npm test` → 64/64 ; `npm run build` OK.

### Vagues D–I (2026-06-15)

| Vague | Cible | Statut |
|-------|-------|--------|
| D | P12 scénarios + compare | ✅ |
| E | P8 Cv ISA + Jacobien analytique | 🟡 MVP (Cv diamètre effectif, FD régulateur) |
| F | P7 PR-78 auto H₂ | ✅ |
| G | P11 PDE scaffold + TransientPlayer | 🟡 MVP |
| H | P13 LM multi-param | ✅ MVP |
| I | Export history + OpenAPI stub | 🟡 |
