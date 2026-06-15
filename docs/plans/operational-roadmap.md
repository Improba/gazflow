# Plan d'implémentation — GazFlow Post-MVP opérationnel

> Feuille de route pour transformer GazFlow d'un démonstrateur académique (GasLib) en outil utilisable par un exploitant de réseau gaz. Chaque phase est conçue pour apporter de la valeur métier indépendamment, tout en construisant les fondations des phases suivantes.

## État d'avancement (2026-06-14, plan complétion)

| Phase | Statut | Commentaire |
|-------|--------|-------------|
| MVP (P0–P5) | ✅ | Steady-state, Cesium, WS, export, capacités |
| **P6** Import | ✅ **~100 %** | GeoJSON/CSV/Shapefile, mapping, validation, UI + aperçu carte |
| **P7** Physique | 🟨 **~88 %** | + PR-78 auto H₂>20 % ; thermique conduites restant |
| **P8** Régulation | 🟨 **~92 %** | Cv MVP ; Jacobien analytique restant |
| **P9** Profils demande | ✅ **~98 %** | Météo CSV, week-end, persistance, WS timeseries |
| **P10** N-1 | ✅ **~100 %** | WS, export, overlay carte — code complet |
| **P11** Transitoire | 🟨 **~55 %** | PDE 1D scaffold + mode API ; réseaux branchés → fallback |
| **P12** Édition | 🟨 **~85 %** | Scénarios diffs, compare ΔP/ΔQ, export GeoJSON restant |
| **P13** Calage SCADA | 🟨 **~90 %** | LM multi-param (≤5), demand scale ; GERG calage restant |
| Corpus tests | ✅ | `docs/testing/corpus/` + CI ; **237** tests back, **62** front |

Plan complétion : `docs/plans/completion-plan.md`.

## Contexte

### Ce qui existe (MVP, phases 0–5)

- Solveur **steady-state** (Newton hybride, sparse LU / GMRES+ILU, jusqu'à ~4000 nœuds).
- Parser **GasLib XML** (+ chemin `load_network_raw` → `RawNetwork` → `GasNetwork::from_raw`).
- **Import réseau (P6, backend)** : GeoJSON, CSV + mapping YAML, validation topologique ; trait `NetworkImporter`.
- Modèle physique : Darcy-Weisbach, Papay Z(P,T), densité dynamique, compresseurs MVP (ratio de pression), vannes ouvertes/fermées.
- Visualisation CesiumJS temps réel + WebSocket + export JSON/CSV/XLSX/ZIP.
- Capacités d'entrée/sortie (vérification + optimisation par projection itérative).
- **Corpus de validation** post-MVP : fixtures synthétiques versionnées + jeux externes (GasLib-39, TRR154, SciGRID FR).

### Ce qui manque pour un usage métier

Un exploitant gaz (type Natran) a besoin de simuler **son propre réseau** (pas GasLib), avec **ses organes de régulation** (détendeurs, régulateurs), sur des **scénarios temporels** (transitoire, profils de demande), et de réaliser des **analyses de sécurité** (N-1) et de **dimensionnement** (édition topologique). Le tout calé sur ses **mesures SCADA**.

---

## Vue d'ensemble des phases

| Phase | Nom | Objectif métier | Prérequis | Statut |
|-------|-----|----------------|-----------|--------|
| **P6** | Import réseau réel | Charger un réseau depuis GeoJSON/Shapefile/CSV | — | ✅ ~100 % |
| **P7** | Modèle physique étendu | Gravité, gaz multi-composant, H₂, viscosité dynamique | — | 🟨 ~88 % |
| **P8** | Organes de régulation | Détendeurs, régulateurs PID, postes de livraison | P6 | 🟨 ~92 % |
| **P9** | Profils de demande | Courbes horaires, thermosensibilité, catégories clients | P6 | ✅ ~98 % |
| **P10** | Analyse N-1 | Simulation automatique de contingences | P8, P9 | ✅ ~100 % |
| **P11** | Simulation transitoire | Résolution PDE instationnaire (linepack, propagation) | P7, P8 | 🟨 ~55 % |
| **P12** | Édition topologique | Création/modification du réseau dans l'UI | P6 | 🟨 ~85 % |
| **P13** | Calage SCADA | Comparaison mesures/simulation, calibration inverse | P6, P9 | 🟨 ~90 % |

```
P6 (Import) ──┬── P8 (Régulation) ──┬── P10 (N-1)
               ├── P9 (Demandes) ────┤
               ├── P12 (Édition)     │
               └── P13 (Calage) ─────┘
                                     │
P7 (Physique) ──── P8 (Régulation) ──┴── P11 (Transitoire)
```

### Estimation de charge globale

| Phase | Complexité | Estimation |
|-------|-----------|------------|
| P6 | L | 3–4 semaines |
| P7 | M-L | 2–3 semaines |
| P8 | L | 3–4 semaines |
| P9 | M | 2 semaines |
| P10 | M | 2 semaines |
| P11 | XL | 6–10 semaines |
| P12 | L | 3–4 semaines |
| P13 | L | 3–4 semaines |

---

## Phase 6 — Import réseau réel

### Problème

GazFlow ne lit que le GasLib XML, un format académique qu'aucun exploitant n'utilise. Les données réseau d'un opérateur sont dans un SIG (GeoJSON, Shapefile, base PostGIS) ou des fichiers CSV/Excel métier.

### Objectif

Permettre l'import d'un réseau depuis des formats standards (GeoJSON, CSV) et la définition d'un schéma de mapping configurable pour s'adapter aux conventions de nommage de chaque opérateur.

### Architecture

```
back/src/
├── import/
│   ├── mod.rs            ← trait NetworkImporter + dispatch ✅
│   ├── geojson.rs        ← import GeoJSON ✅
│   ├── csv.rs            ← import CSV tabulaire ✅
│   ├── mapping.rs        ← schéma de mapping configurable (YAML) ✅
│   ├── validation.rs     ← validation topologique post-import ✅
│   └── shapefile.rs      ← import Shapefile (.shp/.dbf, base64 API) ✅
├── graph/
│   ├── mod.rs            ← GasNetwork + from_raw() ✅
│   └── raw.rs            ← RawNetwork / RawNode / RawPipe ✅
├── gaslib/               ← load_network_raw + GasLibImporter ✅
```

### Format de mapping

```yaml
# mapping.yaml — correspondance colonnes SIG → modèle GazFlow
format: geojson
nodes:
  id_field: "ID_NOEUD"
  type_field: "TYPE"
  type_mapping:
    "ALIM": source
    "LIVR": sink
    "JONC": innode
  pressure_fixed_field: "P_CONSIGNE_BAR"
  lat_field: "geometry.coordinates[1]"
  lon_field: "geometry.coordinates[0]"
  elevation_field: "ALTITUDE_M"
pipes:
  id_field: "ID_CANA"
  from_field: "NOEUD_AMONT"
  to_field: "NOEUD_AVAL"
  length_field: "LONGUEUR_M"
  diameter_field: "DIAMETRE_MM"
  diameter_unit: mm
  roughness_field: "RUGOSITE_MM"
  roughness_unit: mm
  material_field: "MATERIAU"
```

### Tâches

| # | Tâche | Fichiers | Status |
|---|-------|----------|--------|
| 6.1 | Définir le trait `NetworkImporter` et le modèle intermédiaire `RawNetwork` | `import/mod.rs`, `graph/raw.rs` | ✅ |
| 6.2 | Implémenter l'import GeoJSON (nœuds + arcs comme FeatureCollection) | `import/geojson.rs` | ✅ |
| 6.3 | Implémenter le schéma de mapping configurable (YAML) | `import/mapping.rs` | ✅ |
| 6.4 | Implémenter l'import CSV (fichiers `nodes.csv` + `pipes.csv`) | `import/csv.rs` | ✅ |
| 6.5 | Implémenter l'import Shapefile via `shapefile` crate | `import/shapefile.rs`, `api/import.rs` | ✅ (pas de fixture corpus dédiée) |
| 6.6 | Validation topologique post-import : graphe connexe, nœuds orphelins, boucles, au moins un slack | `import/validation.rs` | ✅ |
| 6.7 | Adapter `GasNetwork::from_raw()` pour construire le graphe depuis `RawNetwork` | `graph/mod.rs` | ✅ |
| 6.8 | Endpoint REST `POST /api/import` (upload fichier + mapping) | `api/import.rs` | ✅ |
| 6.9 | Frontend : page/dialog d'import avec sélection format, upload, aperçu colonnes, mapping assisté | `pages/ImportPage.vue` | ✅ |
| 6.10 | Frontend : validation visuelle pré-import (aperçu carte SVG, erreurs topologiques) | `components/ImportMapPreview.vue` | ✅ |
| 6.11 | Refactorer le GasLib parser comme implémentation de `NetworkImporter` | `gaslib/parser.rs`, `import/mod.rs` | ✅ |
| 6.12 | Sélection du réseau actif (multi-réseau) dans la barre d'outils | `stores/network.ts`, `api/mod.rs` | ✅ |

### Tests

| ID | Test | Description | Statut |
|----|------|-------------|--------|
| T6-1 | `test_geojson_import_minimal` | Import GeoJSON 3 nœuds / 2 pipes → `RawNetwork` valide | ✅ |
| T6-2 | `test_csv_import_with_mapping` | Import CSV + mapping YAML → nœuds/pipes avec bons champs | ✅ |
| T6-3 | `test_validation_detects_orphan_node` | Nœud sans pipe → erreur topologique | ✅ |
| T6-4 | `test_validation_detects_no_slack` | Réseau sans nœud à pression fixée → erreur | ✅ |
| T6-5 | `test_validation_detects_disconnected_graph` | Graphe non connexe → erreur | ✅ |
| T6-6 | `test_gaslib_as_network_importer` | GasLib parser wrappé dans le trait → même résultat qu'avant | ✅ |
| T6-7 | `test_import_then_solve` | Import GeoJSON → build graph → steady-state solve → convergence | ✅ |
| T6-8 | `test_api_import_upload` | `POST /api/import` avec GeoJSON → réseau chargé et simulable | ✅ |
| T6-9 | `mapping::*` | Résolution SIG (coords imbriquées, type ALIM/LIVR, unités m→km) | ✅ |
| T6-10 | `validation::*` + API 422 | Nœud inconnu, réseau vide, graphe déconnecté, orphelin via HTTP | ✅ |
| T6-11 | `test_api_import_*` | validate_only, CSV gravité, import inactif + sélection, mapping invalide | ✅ |
| T6-12 | `test_csv_gravity_downhill_altitudes` | Variante métier descente (Δz = 150 m) | ✅ |
| T6-13 | `test_geojson_minimal_line_operateur_fields` | Pression slack, rôles, GPS, altitude depuis corpus SIG | ✅ |
| T6-14 | front `importError` + store | Erreurs API affichables, import sans activation | ✅ |

Corpus : `docs/testing/corpus/synthetic/minimal-line/` (T6-1, T6-7, T6-13), `gravity-pipe/` (T6-2, T6-12), `topo-errors/` (T6-3–T6-5, T6-10). Vérification : `./scripts/verify_test_corpus.sh`.

---

## Phase 7 — Modèle physique étendu

### Problème

Le MVP suppose du CH₄ pur, ignore la gravité, utilise un Reynolds fixe et une viscosité constante. Ces simplifications sont acceptables pour GasLib-11 (~70 bar, terrain plat) mais pas pour un réseau réel avec du relief, du biométhane, ou de l'injection H₂.

### Objectif

Enrichir le modèle physique pour couvrir les cas métier courants : terrain en pente, gaz multi-composant (CH₄/C₂H₆/CO₂/N₂/H₂), viscosité dynamique, Reynolds variable.

### Tâches

| # | Tâche | Fichiers | Status |
|---|-------|----------|--------|
| 7.1 | **Gravité** : ajouter le terme $\rho g \Delta h$ dans l'équation de perte de charge | `solver/steady_state.rs`, `solver/newton.rs` | ✅ |
| 7.2 | Propager l'altitude des nœuds depuis le parser/import dans `GasNetwork` | `graph/mod.rs`, `import/mapping.rs`, `gaslib/parser.rs` | ✅ |
| 7.3 | **Gaz multi-composant** : struct `GasComposition { ch4, c2h6, co2, n2, h2, ... }` avec pseudo-critiques Kay | `solver/gas_properties.rs` | ✅ |
| 7.4 | Adapter Papay Z pour les mélanges (pseudo-Pr, pseudo-Tr) | `solver/gas_properties.rs` | ✅ |
| 7.5 | **PCS/PCI et Wobbe** : calcul du pouvoir calorifique et de l'indice de Wobbe en fonction de la composition | `solver/gas_properties.rs` | ✅ |
| 7.6 | **Viscosité dynamique** : Lee-Gonzalez-Eakin (T en °R, facteur 10⁻⁴) | `solver/gas_properties.rs` | ✅ |
| 7.7 | **Reynolds dynamique** : `pipe_resistance_hydraulic` avec débit connu ; Newton garde Re=10⁷ (stabilité Jacobian) | `solver/steady_state.rs`, `solver/newton.rs` | ✅ |
| 7.8 | Configuration de la composition gaz par réseau (`PATCH /api/network/gas-composition`, G20 par défaut import) | `api/mod.rs`, `stores/network.ts` | ✅ |
| 7.9 | Affichage/édition composition, PCS et Wobbe dans le panneau simulation | `components/SimulationPanel.vue` | ✅ |
| 7.10 | Validation physique : tendances monotones (plus de H₂ → moins de PCS, gravité atténuée, ΔP friction ↓ sur conduite plate) | `solver/gas_properties.rs`, `steady_state.rs` | ✅ tests friction/gravité/Re H₂ |

### Formulation gravité

L'équation pipe devient :

$$
P_1^2 - P_2^2 = K \cdot Q |Q| + \rho_{\text{moy}} \cdot g \cdot (z_2 - z_1) \cdot (P_1 + P_2)
$$

Le terme gravitaire est linéarisé lors de l'assemblage du Jacobien (utilise $P_1 + P_2$ de l'itération précédente).

### Formulation gaz multi-composant

Propriétés pseudo-critiques (Kay's mixing rule, couplage Papay) :

$$
P_{pc} = \sum_i y_i \cdot P_{c,i}, \quad T_{pc} = \sum_i y_i \cdot T_{c,i}
$$

$$
M_{\text{mix}} = \sum_i y_i \cdot M_i
$$

Puis $Z(P, T) = \text{Papay}(P/P_{pc}, T/T_{pc})$, $\rho(P,T) = P \cdot M_{\text{mix}} / (Z \cdot R \cdot T)$.

### Tests

| ID | Test | Description | Statut |
|----|------|-------------|--------|
| T7-1 | `test_gravity_uphill_increases_pressure_drop` | Pipe en montée → perte de charge plus grande qu'à plat | ✅ |
| T7-2 | `test_gravity_downhill_decreases_pressure_drop` | Pipe en descente → perte de charge réduite | ✅ |
| T7-3 | `test_gravity_flat_matches_same_elevation_offset` | Δz = 0 → résultat identique (offset altitude global) | ✅ |
| T7-4 | `test_gas_composition_pure_ch4_matches_legacy` | Composition 100% CH₄ → mêmes propriétés que le modèle actuel | ✅ |
| T7-5 | `test_h2_blend_reduces_density` | 20% H₂ → densité inférieure au CH₄ pur | ✅ |
| T7-6 | `test_h2_blend_reduces_pcs` | 20% H₂ → PCS inférieur (H₂ : 12.7 MJ/m³ vs CH₄ : 39.8 MJ/m³) | ✅ |
| T7-7 | `test_g20_wobbe_matches_literature_order_of_magnitude` | Wobbe G20 ~46 MJ/Nm³ (EN 437) | ✅ |
| T7-8 | `test_dynamic_viscosity_lee_gonzalez` | Viscosité CH₄ ~10⁻⁵ Pa·s à 70 bar | ✅ |
| T7-9 | `test_dynamic_reynolds_varies_with_flow` | Re augmente avec le débit | ✅ |
| T7-11 | `test_pipe_resistance_hydraulic_varies_with_standard_flow` | K varie avec Q via Re dynamique | ✅ |
| T7-12 | `test_newton_resistance_path_uses_turbulent_reynolds_plateau` | Jacobian Newton : Re=10⁷ (Q=0) | ✅ |
| T7-10 | `test_full_model_gaslib11_regression` | GasLib-11 avec gravité (Δz=0) ≈ résultat antérieur | ✅ |

---

## Phase 8 — Organes de régulation

### Problème

Un réseau gaz réel est piloté par des **postes de détente** (réduisent la pression d'un niveau à l'autre) et des **régulateurs** (maintiennent une consigne de pression aval). Le MVP ne modélise que des vannes (ouvert/fermé) et des compresseurs (ratio fixe). Sans régulation, le modèle ne reproduit pas le comportement opérationnel.

### Objectif

Modéliser les organes de régulation couramment rencontrés : détendeurs à consigne aval, régulateurs avec caractéristique, vannes de régulation (Cv), postes de livraison.

### Modèles physiques

**Détendeur / régulateur à pression aval fixe :**
- Comportement : impose $P_{\text{aval}} = P_{\text{consigne}}$ tant que $P_{\text{amont}} > P_{\text{consigne}} + \Delta P_{\text{min}}$.
- Si $P_{\text{amont}}$ tombe sous la consigne, le détendeur est « en bypass » (transparent).
- Modélisation : nœud intermédiaire à pression conditionnellement fixée.

**Vanne de régulation (Cv) :**

$$
Q = C_v \cdot N \cdot Y \cdot \sqrt{\frac{x \cdot P_1}{\rho_1}}
$$

avec $x = (P_1 - P_2) / P_1$ (ratio de chute), $Y$ facteur d'expansion, $N$ constante dimensionnelle. Linéarisé dans le Jacobien.

**Poste de livraison :**
- Combinaison détendeur + compteur + vanne de coupure.
- Pression aval contractuelle minimale $P_{\text{min,livr}}$.
- Le compteur n'a pas d'effet hydraulique mais fournit le débit mesuré (utile pour le calage).

### Tâches

| # | Tâche | Fichiers | Status |
|---|-------|----------|--------|
| 8.1 | `ConnectionKind::PressureRegulator` + `EquipmentSpec::regulator_setpoint_bar` | `graph/mod.rs`, `graph/equipment.rs` | ✅ |
| 8.2 | `ConnectionKind::ControlValve` + `EquipmentSpec` (Cv, ouverture) | `graph/mod.rs`, `graph/equipment.rs` | ✅ |
| 8.3 | `ConnectionKind::DeliveryStation` + P_min contractuelle | `graph/mod.rs`, `graph/equipment.rs` | ✅ |
| 8.4 | Parser organes depuis GeoJSON/CSV (mapping) + GasLib `controlValve` | `import/mapping.rs`, `gaslib/parser.rs` | ✅ |
| 8.5 | Boucle externe régulateur : slack aval conditionnel + bypass | `solver/regulator.rs`, `solver/steady_state.rs` | ✅ |
| 8.6 | Modèle hydraulique vanne Cv (approx. ouverture → diamètre effectif) | `solver/steady_state.rs` | 🟡 MVP |
| 8.7 | Jacobien régulateur (FD sur ligne nœud active ; analytique Cv restant) | `solver/newton.rs` | 🟡 MVP |
| 8.8 | Logique de commutation régulateur (actif / bypass) avec hystérésis | `solver/regulator.rs` | ✅ |
| 8.9 | API : exposer les organes et leur état dans les résultats | `api/mod.rs`, `SolverResult` | ✅ |
| 8.10 | Frontend : marqueurs carte + popup organes | `components/CesiumViewer.vue` | ✅ |
| 8.11 | Frontend : panneau organes (édition + résultats) + avertissements | `EquipmentControls.vue`, `SimulationPanel.vue` | ✅ |

### Tests

| ID | Test | Description |
|----|------|-------------|
| T8-1 | `test_regulator_imposes_downstream_pressure` | P_aval = consigne quand P_amont suffisante | ✅ |
| T8-2 | `test_regulator_bypass_when_upstream_low` | P_amont < consigne → régulateur transparent | ✅ |
| T8-3 | `test_control_valve_cv_flow` | Résistance / diamètre effectif cohérents avec Cv et ouverture | ✅ |
| T8-4 | `test_control_valve_closed_blocks_flow` | Ouverture 0 % → débit nul | ✅ |
| T8-5 | `test_delivery_station_min_pressure` | Vérification que P_livraison ≥ P_min contractuelle | ✅ |
| T8-6 | `test_regulator_hysteresis` | Pas d'oscillation actif/bypass entre deux itérations | ✅ |
| T8-7 | `test_mixed_network_two_regulators_converges` | Réseau avec 2 régulateurs en cascade → convergence | ✅ |

---

## Phase 9 — Profils de demande

### Problème

Les demandes sont des scalaires fixes. Un exploitant raisonne en **profils temporels** : courbes horaires, journalières, saisonnières, indexées sur la température extérieure.

### Objectif

Permettre la définition de profils de demande paramétriques et la simulation de scénarios temporels (séquence de pas steady-state ou alimentation du transitoire).

### Modèle de thermosensibilité

Modèle linéaire standard des distributeurs français (débits en Nm³/h aux conditions normales) :

$$
Q_{\mathrm{chauff}}(T_{\mathrm{ext}}) = \min\!\bigl(\alpha \max(0,\; T_{\mathrm{seuil}} - T_{\mathrm{ext}}),\; Q_{\mathrm{chauff,max}}\bigr)
$$

$$
Q_{\mathrm{ref}}(T_{\mathrm{ext}}) = Q_0 + Q_{\mathrm{chauff}}(T_{\mathrm{ext}}), \qquad
Q_h = Q_{\mathrm{ref}}(T_{\mathrm{ext}})\, m_h
$$

- $Q_0$ : consommation de base (ECS, cuisson, procédé continu) — indépendante de la température.
- $\alpha$ : gradient de thermosensibilité (Nm³/h/°C) — dépend de la catégorie client.
- $T_{\mathrm{seuil}}$ : température de non-chauffage (~16–18 °C selon zone climatique ; preset 17 °C zones H1–H2).
- $Q_{\mathrm{chauff,max}}$ : plafond optionnel (saturation froid extrême) sur presets résidentiel / tertiaire.
- $m_h$ : multiplicateur journalier ($\bar m = 1$) — profils distincts résidentiel / tertiaire / industriel plat.

Les presets ciblent des **points de livraison ou postes de soutirage agrégés**, pas un logement individuel.

### Tâches

| # | Tâche | Fichiers | Status |
|---|-------|----------|--------|
| 9.1 | Struct `DemandProfile` : base + gradient + seuil + catégorie client + courbe journalière normalisée | `solver/demand.rs` | ✅ |
| 9.2 | Catégories prédéfinies : résidentiel, tertiaire, industriel (profils journaliers types) | `solver/demand.rs` | ✅ |
| 9.3 | Fonction `resolve_demands(profiles, T_ext, datetime) → HashMap<String, f64>` | `solver/demand.rs` | ✅ |
| 9.4 | Import de profils depuis CSV (nœud, catégorie, Q0, alpha, T_seuil) | `import/demand_profiles.rs` | ✅ |
| 9.5 | **Simulation multi-pas** : boucle sur une série temporelle (T_ext horaire), résolution steady-state à chaque pas, agrégation des résultats | `solver/timeseries.rs` | ✅ |
| 9.6 | API : endpoint `POST /api/simulate/timeseries` avec série météo + profils | `api/mod.rs` | ✅ |
| 9.7 | API : WebSocket `start_timeseries_simulation` avec streaming pas-par-pas | `api/ws.rs` | ✅ |
| 9.8 | Frontend : scénario temporel (T_ext, profils par nœud, météo CSV, week-end) | `components/ScenarioPanel.vue` | ✅ |
| 9.9 | Frontend : graphiques temporels (pression min, soutirage total) | `components/TimeseriesChart.vue` | ✅ MVP |
| 9.10 | Frontend : animation temporelle sur la carte (slider de temps, couleurs évoluent) | `components/CesiumViewer.vue` | ✅ MVP |

### Tests

| ID | Test | Description |
|----|------|-------------|
| T9-1 | `test_demand_profile_zero_below_threshold` | T_ext > T_seuil → Q = Q_0 (pas de chauffage) | ✅ |
| T9-2 | `test_demand_profile_linear_above_threshold` | T_ext = T_seuil - 10 → Q = Q_0 + 10α | ✅ |
| T9-3 | `test_daily_profile_normalization` | Parts journalières $s_h$ somment à 1 ; multiplicateurs $m_h$ de moyenne 1 | ✅ |
| T9-4 | `test_resolve_demands_residential_winter` | Résidentiel, T_ext = -5°C, 7h → demande élevée | ✅ |
| T9-5 | `test_timeseries_24h_converges_all_steps` | 24 pas horaires → tous convergent | ✅ |
| T9-6 | `test_timeseries_warm_start_speeds_up` | Warm-start du pas N depuis le pas N-1 → moins d'itérations | ✅ |

---

## Phase 10 — Analyse de sécurité N-1

### Problème

L'exploitant doit vérifier que son réseau tient en cas de perte d'un élément critique (obligation réglementaire de sécurité d'approvisionnement). Aujourd'hui, il faudrait relancer manuellement une simulation par contingence, ce qui est impraticable sur un réseau de 500+ éléments.

### Objectif

Automatiser le lancement d'une batterie de simulations de contingence (N-1), agréger les résultats, et produire un rapport exploitable.

### Définition N-1

Pour chaque élément $e$ d'un ensemble d'éléments critiques $\mathcal{C}$ :
1. Retirer $e$ du réseau (fermer la vanne, couper l'alimentation, mettre le pipe hors service).
2. Résoudre le steady-state sur le réseau dégradé.
3. Vérifier les contraintes : $P_i \geq P_{\min}$ aux points de livraison, pas de sur-débit aux sources.

**Résultat** : matrice de contingence — pour chaque élément retiré, quels nœuds sont en violation et de combien.

### Tâches

| # | Tâche | Fichiers | Status |
|---|-------|----------|--------|
| 10.1 | Struct `ContingencyCase { element_id, element_type, action }` | `solver/contingency.rs` | ✅ |
| 10.2 | `generate_n_minus_1_cases` : sources, vannes, compresseurs | `solver/contingency.rs` | ✅ |
| 10.3 | `apply_contingency` : réseau dégradé | `solver/contingency.rs` | ✅ |
| 10.4 | Exécution parallèle Rayon | `solver/contingency.rs` | ✅ |
| 10.5 | `ContingencyReport` + violations pression | `solver/contingency.rs` | ✅ |
| 10.6 | Cas rouges / verts | `solver/contingency.rs` | ✅ |
| 10.7 | API `POST /api/contingency` (scope all / sources / custom) | `api/mod.rs` | ✅ |
| 10.8 | WebSocket `start_contingency_simulation` | `api/ws.rs` | ✅ |
| 10.9 | Frontend `ContingencyPage.vue` | `pages/ContingencyPage.vue` | ✅ |
| 10.10 | Overlay carte violations | `CesiumViewer.vue`, `ContingencyPage.vue` | ✅ |
| 10.11 | Export N-1 XLSX/CSV | `api/export.rs`, `POST /api/contingency/export` | ✅ |

### Tests

| ID | Test | Description |
|----|------|-------------|
| T10-1 | `test_generate_cases_covers_all_sources` | Toutes les sources apparaissent dans les cas générés |
| T10-2 | `test_apply_contingency_removes_pipe` | Pipe retiré → réseau a un arc de moins, toujours connexe (ou diagnostic) |
| T10-3 | `test_apply_contingency_disconnects_detects` | Retrait d'un pipe qui déconnecte le réseau → diagnostic explicite |
| T10-4 | `test_n_minus_1_parallel_deterministic` | Résultats identiques en 1 thread et N threads |
| T10-5 | `test_n_minus_1_report_identifies_red_cases` | Au moins un cas rouge quand le réseau est fragile |
| T10-6 | `test_n_minus_1_gaslib11_all_green` | GasLib-11 (surdimensionné) → tous les cas verts |

---

## Phase 11 — Simulation transitoire

### Problème

Le steady-state donne une photo figée. L'exploitant a besoin de la dynamique : propagation des ondes de pression, vidange/remplissage du linepack, réponse temporelle aux variations de demande et aux événements (fermeture de vanne, démarrage compresseur).

### Objectif

Implémenter un solveur transitoire isotherme 1D basé sur les équations de Saint-Venant pour les gaz compressibles, avec un schéma implicite stable.

### Équations

Système hyperbolique 1D (forme conservative, isotherme) :

$$
\frac{\partial \rho}{\partial t} + \frac{\partial (\rho v)}{\partial x} = 0 \quad \text{(continuité)}
$$

$$
\frac{\partial (\rho v)}{\partial t} + \frac{\partial (\rho v^2 + P)}{\partial x} = -\frac{f}{2D} \rho v |v| - \rho g \sin\theta \quad \text{(quantité de mouvement)}
$$

Avec $P = \rho Z R T / M$ (équation d'état).

### Schéma numérique retenu

**Méthode implicite aux différences finies** (Euler implicite en temps, centré en espace) :
- Chaque pipe est discrétisé en $N_x$ segments (adaptatif selon $L/D$ et CFL).
- Le système non-linéaire résultant est résolu par Newton (réutilisation de l'infrastructure sparse existante).
- Pas de temps adaptatif basé sur le CFL et la vitesse de convergence.

### Tâches

| # | Tâche | Fichiers | Status |
|---|-------|----------|--------|
| 11.1 | Maillage 1D par conduite (`n_cells_per_pipe`) | `solver/transient/mesh.rs` | ✅ MVP |
| 11.2 | Système transitoire (Euler implicite tridiagonal par pipe) | `solver/transient/system.rs` | 🟡 MVP |
| 11.3 | Conditions aux limites source/sink | `solver/transient/boundary.rs` | 🟡 MVP |
| 11.4 | Intégration temporelle | `solver/transient/time_integration.rs` | 🟡 MVP |
| 11.5 | Initialisation steady-state + modes `quasi_steady` / `pde` | `solver/transient/mod.rs` | ✅ |
| 11.6 | Événements temporels (vanne, demande, consigne) | `solver/transient/events.rs` | ✅ |
| 11.7 | Linepack agrégé $M = \sum \rho A L$ | `solver/transient/linepack.rs` | ✅ |
| 11.8 | API `POST /api/simulate/transient` (+ `mode`, `n_cells_per_pipe`) | `api/mod.rs` | ✅ |
| 11.9 | WebSocket streaming transitoire $P(x,t)$ | `api/ws.rs` | ⬜ |
| 11.10 | Frontend timeline + player | `components/TransientPlayer.vue` | ✅ MVP |
| 11.11 | Graphiques P(t), Q(t) dédiés | `components/TransientChart.vue` | ⬜ |
| 11.12 | Jauge linepack par zone | `components/LinepackGauge.vue` | ⬜ (linepack dans steps API) |

### Tests

| ID | Test | Description |
|----|------|-------------|
| T11-1 | `test_transient_steady_initial_stays_steady` | Conditions initiales = steady-state, pas d'événement → solution reste constante |
| T11-2 | `test_transient_step_demand_propagates` | Augmentation brusque de demande → onde de dépression se propage |
| T11-3 | `test_transient_mass_conservation` | Masse totale dans le réseau = intégrale(entrées) - intégrale(sorties) à ε près |
| T11-4 | `test_transient_valve_closure_pressure_wave` | Fermeture de vanne → coup de bélier (pic de pression amont) |
| T11-5 | `test_linepack_decreases_on_excess_demand` | Demande > approvisionnement → linepack diminue |
| T11-6 | `test_transient_converges_to_steady_state` | Après un transitoire, si conditions constantes → converge vers le nouveau steady-state |
| T11-7 | `test_transient_2_node_analytical` | Pipe unique, solution analytique connue → erreur < 1% |
| T11-8 | `test_cfl_adaptive_timestep` | Pas de temps s'adapte : plus petit quand les gradients sont raides |

---

## Phase 12 — Édition topologique

### Problème

L'interface est en lecture seule sur la topologie. Pour les études de dimensionnement (extension réseau, renforcement, dévoiement), l'ingénieur doit pouvoir modifier le réseau graphiquement.

### Objectif

Permettre la création, modification et suppression d'éléments du réseau directement dans l'UI, avec sauvegarde et versioning des scénarios topologiques.

### Tâches

| # | Tâche | Fichiers | Status |
|---|-------|----------|--------|
| 12.1 | API CRUD nœuds / pipes | `api/network_edit.rs` | ✅ |
| 12.2 | Validation incrémentale post-modification | `import/validation.rs` | ✅ |
| 12.3 | Scénarios topologiques (diffs vs baseline) | `graph/scenarios.rs`, `api/scenarios.rs` | ✅ |
| 12.4 | Édition carte avancée (snap, dessin pipe) | `CesiumViewer.vue` | 🟡 MVP |
| 12.5 | Palette outils éditeur | `components/EditorToolbar.vue` | ✅ |
| 12.6 | Panneau propriétés | `components/PropertyPanel.vue` | ✅ |
| 12.7 | Undo/redo | `stores/editor.ts` | ✅ |
| 12.8 | Export réseau modifié (GeoJSON, CSV) | `api/export.rs` | ⬜ |
| 12.9 | Compare ΔP/ΔQ entre scénarios | `ComparePanel.vue`, `POST /api/simulate/compare` | ✅ |

### Tests

| ID | Test | Description |
|----|------|-------------|
| T12-1 | `test_add_node_and_pipe_api` | Ajout d'un nœud + pipe → réseau étendu, simulable |
| T12-2 | `test_delete_pipe_api` | Suppression d'un pipe → réseau toujours connexe (ou erreur) |
| T12-3 | `test_modify_diameter_changes_result` | Changement de diamètre → pression différente à la simulation |
| T12-4 | `test_scenario_diff_roundtrip` | Créer variante → sauvegarder → recharger → réseau identique |
| T12-5 | `test_undo_redo_consistency` | Ajout + undo → réseau identique à l'original |

---

## Phase 13 — Calage sur mesures SCADA

### Problème

Un modèle de réseau « brut » issu du SIG donne des résultats éloignés de la réalité (rugosités inconnues, demandes estimées, géométrie simplifiée). Le calage consiste à ajuster les paramètres du modèle pour reproduire les mesures de terrain.

### Objectif

Fournir un workflow de calibration : import de mesures SCADA (pressions, débits aux postes), comparaison simulation/mesures, et optimisation inverse des paramètres (rugosités, demandes non mesurées).

### Formulation

Problème inverse (moindres carrés non-linéaire) :

$$
\min_{\boldsymbol{\theta}} \sum_{k \in \mathcal{M}} w_k \left( y_k^{\text{sim}}(\boldsymbol{\theta}) - y_k^{\text{mes}} \right)^2
$$

où $\boldsymbol{\theta}$ = vecteur de paramètres à caler (rugosités $\varepsilon_j$, demandes non mesurées $d_i$), $y_k$ = mesures (pressions, débits), $w_k$ = poids (inverse de l'incertitude de mesure).

Résolution par **Levenberg-Marquardt** (réutilise le solveur direct existant comme « boîte noire »).

### Tâches

| # | Tâche | Fichiers | Status |
|---|-------|----------|--------|
| 13.1 | Struct `ScadaMeasurement` | `calibration/mod.rs` | ✅ |
| 13.2 | Import SCADA CSV | `calibration/import.rs` | ✅ |
| 13.3 | `CalibrationParameter` (rugosité globale, par pipe, `DemandScale`) | `calibration/mod.rs` | ✅ |
| 13.4 | Fonction objectif pondérée | `calibration/objective.rs` | ✅ |
| 13.5 | Levenberg-Marquardt (global + multi-param ≤5, FD) | `calibration/lm.rs`, `optimizer.rs` | ✅ MVP |
| 13.6 | Rapport RMSE, R², résidus | `calibration/report.rs` | ✅ |
| 13.7 | API `POST /api/calibrate` | `api/mod.rs` | ✅ |
| 13.8 | Page calage | `pages/CalibrationPage.vue` | ✅ |
| 13.9 | Scatter plot sim vs mesures | `components/CalibrationChart.vue` | ⬜ |
| 13.10 | Carte résidus pression | `CalibrationPage.vue`, `CesiumViewer.vue` | ✅ |

### Tests

| ID | Test | Description |
|----|------|-------------|
| T13-1 | `test_calibration_perfect_data_recovers_params` | Données synthétiques parfaites → le calage retrouve les paramètres exacts |
| T13-2 | `test_calibration_noisy_data_improves_fit` | Données bruitées → RMSE post-calage < RMSE avant calage |
| T13-3 | `test_calibration_respects_bounds` | Paramètres calés restent dans les bornes physiques |
| T13-4 | `test_calibration_report_metrics` | Rapport contient RMSE, R², Nash-Sutcliffe, nombre d'itérations |

---

## Ordre de réalisation recommandé

### Vague 1 — Fondations (P6 + P7 en parallèle)

L'import réseau réel (P6) et le modèle physique étendu (P7) sont indépendants et constituent les deux piliers nécessaires pour tout le reste. Les développer en parallèle avec deux équipes/agents.

**Livrable V1** : on peut charger un réseau GeoJSON avec du relief et du gaz multi-composant, et le simuler en steady-state.

**Avancement V1 (2026-06-12)** :
- ✅ Import GeoJSON/CSV/Shapefile + mapping YAML + validation topo (backend)
- ✅ API `POST /api/import`, UI import, sélection multi-réseau
- ✅ GasLib refactoré via `RawNetwork`
- ✅ P7 : gravité, multi-composant, viscosité Lee-Gonzalez, Reynolds dynamique, composition G20 par réseau
- ✅ Carte pré-import (6.10)

### Vague 2 — Exploitation quotidienne (P8 + P9)

Les organes de régulation (P8) et les profils de demande (P9) rendent l'outil utilisable au quotidien par un exploitant. P8 dépend du modèle étendu (P7) pour la gravité. P9 dépend de l'import (P6) pour les profils par nœud.

**Livrable V2** : simulation steady-state d'un réseau réel avec régulateurs, sur des scénarios météo horaires.

### Vague 3 — Analyses et planification (P10 + P12)

L'analyse N-1 (P10) et l'édition topologique (P12) permettent les études de planification et de sécurité. Développables en parallèle.

**Livrable V3** : l'ingénieur peut tester des variantes de réseau et vérifier la tenue N-1.

### Vague 4 — Maturité (P11 + P13)

Le transitoire (P11) et le calage SCADA (P13) sont les plus complexes et bénéficient de la stabilité des phases précédentes. Le transitoire est le plus gros morceau du plan.

**Livrable V4** : simulation transitoire calée sur les données réelles — l'outil est à maturité opérationnelle.

---

## Risques et mitigations

| Risque | Impact | Phase | Mitigation |
|--------|--------|-------|------------|
| Formats SIG trop variés entre opérateurs | P6 bloqué par la diversité des données | P6 | Mapping configurable (YAML) + validation stricte post-import |
| Régulateurs créent des non-linéarités fortes (commutation actif/bypass) | Newton diverge | P8 | Hystérésis + sous-relaxation + continuation |
| Solveur transitoire instable (CFL, chocs) | P11 inutilisable | P11 | Schéma implicite (inconditionnellement stable), pas adaptatif, TVD limiter si explicite |
| Problème inverse mal conditionné (calage) | Calage ne converge pas | P13 | Régularisation Tikhonov, bornes physiques, données redondantes |
| Composition H₂ élevée (>20%) invalide Papay | Résultats physiquement faux | P7 | Basculer vers GERG-2008 ou Peng-Robinson au-delà de 20% H₂ |
| Maillage transitoire trop fin → temps de calcul prohibitif | P11 inutilisable en interactif | P11 | Maillage adaptatif + parallélisme Rayon + option « résolution rapide » (maillage grossier) |
| Absence de données SCADA pour le calage (confidentialité opérateur) | P13 non testable | P13 | Données synthétiques pour les tests, architecture « apportez vos données » |

---

## Références

- Osiadacz, A.J. (1987). *Simulation and Analysis of Gas Networks*. Gulf Publishing.
- Chaczykowski, M. (2010). Transient flow in natural gas pipeline — The effect of pipeline thermal model. *Applied Mathematical Modelling*, 34(4), 1051-1067.
- Ke, S.L., Ti, H.C. (2000). Transient analysis of isothermal gas flow in pipeline network. *Chemical Engineering Journal*, 76(2), 169-177.
- ISO 12213 (2006). Natural gas — Calculation of compression factor.
- GERG-2008: Kunz, O., Wagner, W. (2012). The GERG-2008 wide-range equation of state for natural gases and other mixtures. *J. Chem. Eng. Data*, 57(11), 3032-3091.
- Ríos-Mercado, R.Z., Borraz-Sánchez, C. (2015). Optimization problems in natural gas transportation systems. *Applied Energy*, 147, 536-555.
- Koch, T. et al. (2015). *Evaluating Gas Network Capacities*. SIAM MOS.
- Arrêté du 13 juillet 2000 portant règlement de sécurité de la distribution de gaz combustible par canalisations (France).
