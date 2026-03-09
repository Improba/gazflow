# Contrat d'export des résultats — OpenGasSim (v1)

## Objectif

Définir un contrat d'export **stable, versionné et testable** pour les résultats de simulation,
afin de garantir :

- interopérabilité (outils data, scripts, Excel, BI),
- traçabilité (métadonnées et unités explicites),
- fluidité UX (export sans bloquer la carte/live updates).

## Principes

- **Versionné** : chaque export embarque `schema_version`.
- **Déterministe** : ordre trié des nœuds/tuyaux pour des diff reproductibles.
- **Auto-descriptif** : unités et contexte de simulation inclus.
- **Backward-compatible** : ajout de champs permis, suppression/renommage interdit dans une même version majeure.

---

## Endpoints d'export (cible MVP+)

### 1) JSON complet

`GET /api/export/{simulation_id}?format=json`

- Content-Type: `application/json`
- Response: payload complet versionné (voir schéma ci-dessous)
- Usage: partage, archivage, post-traitement Python/R

### 2) CSV plat

`GET /api/export/{simulation_id}?format=csv`

- Content-Type: `text/csv; charset=utf-8`
- Response: table unique avec colonne `kind` (`pressure` | `flow`)
- Usage: ouverture directe tableur / ETL simple

### 3) Bundle complet (optionnel)

`GET /api/export/{simulation_id}?format=zip&include_logs=true`

- Content-Type: `application/zip`
- Contenu recommandé :
  - `result.json` (format JSON v1),
  - `result.csv` (format CSV v1),
  - `logs.ndjson` (si `include_logs=true`),
  - `context.json` (infos run/frontend).

> Si `simulation_id` est inconnu: `404` avec payload d'erreur standard.

---

## Schéma JSON v1

### Champs obligatoires

- `schema_version`: `"opengassim-export/v1"`
- `simulation`
- `units`
- `results`
- `stats`

### Exemple de payload

```json
{
  "schema_version": "opengassim-export/v1",
  "simulation": {
    "id": "sim_2026-03-09T10-15-27Z_3f6a",
    "created_at": "2026-03-09T10:15:27Z",
    "status": "converged",
    "network_id": "GasLib-11",
    "scenario_id": "default",
    "demands": {
      "sink_1": -10.0
    },
    "solver": {
      "method": "newton_sparse_lu",
      "iterations": 42,
      "residual": 0.00023,
      "elapsed_ms": 45
    }
  },
  "units": {
    "pressure": "bar",
    "flow": "m3/s"
  },
  "results": {
    "pressures": [
      { "node_id": "A", "pressure": 65.12 },
      { "node_id": "B", "pressure": 66.30 }
    ],
    "flows": [
      { "pipe_id": "SJ", "from": "S", "to": "J", "flow": 10.0, "abs_flow": 10.0, "direction": "forward" },
      { "pipe_id": "JA", "from": "J", "to": "A", "flow": 5.2, "abs_flow": 5.2, "direction": "forward" }
    ]
  },
  "stats": {
    "node_count": 11,
    "pipe_count": 12,
    "min_pressure": 61.8,
    "max_pressure": 70.0,
    "max_abs_flow": 10.0
  }
}
```

### Règles de sérialisation

- `pressures` trié par `node_id` (ordre lexicographique).
- `flows` trié par `pipe_id` (ordre lexicographique).
- nombres exportés en `f64` (pas de format local avec virgule).
- dates au format ISO-8601 UTC.

---

## Contrat CSV v1

Colonnes obligatoires:

- `kind`
- `id`
- `from`
- `to`
- `value`
- `abs_value`
- `unit`
- `direction`

Sémantique:

- `kind=pressure`: `id=node_id`, `from/to/direction` vides, `value=pression`.
- `kind=flow`: `id=pipe_id`, `from/to` renseignés, `value=débit signé`.

Exemple:

```csv
kind,id,from,to,value,abs_value,unit,direction
pressure,A,,,65.12,65.12,bar,
pressure,B,,,66.30,66.30,bar,
flow,SJ,S,J,10.0,10.0,m3/s,forward
flow,JA,J,A,5.2,5.2,m3/s,forward
```

---

## Contrat d'erreur

Format d'erreur recommandé (JSON):

```json
{
  "error": {
    "code": "EXPORT_NOT_FOUND",
    "message": "simulation_id inconnu",
    "details": {
      "simulation_id": "sim_xxx"
    }
  }
}
```

Codes minimaux:

- `EXPORT_NOT_FOUND` (`404`)
- `EXPORT_FORMAT_UNSUPPORTED` (`400`)
- `EXPORT_NOT_READY` (`409`)
- `EXPORT_INTERNAL_ERROR` (`500`)

---

## Exigences fluidité (UI + API)

- Export depuis un résultat déjà convergé: **pas de recalcul solveur**.
- L'export ne doit pas bloquer le thread UI : déclenchement asynchrone côté front.
- En charge normale, démarrage du téléchargement cible `< 300 ms` après clic.
- Pendant un export, la navigation Cesium reste fluide (pas de freeze perceptible > 100 ms).

Recommandations implémentation:

- backend: sérialisation en streaming quand taille importante;
- frontend: état `exporting` local (spinner bouton), sans bloquer les autres interactions;
- conserver l'état live (`running/converged/error`) visible pendant l'export.

---

## Plan de tests de conformité

- `test_export_result_json_schema`: présence des champs obligatoires et unités.
- `test_export_result_csv_headers`: colonnes exactes et ordre stable.
- `test_export_order_is_deterministic`: tri lexical stable.
- `test_export_unknown_id_returns_404`: contrat d'erreur respecté.
- E2E front: clic export JSON/CSV en simulation convergée + UI toujours interactive.

---

## Versioning

- Version actuelle: `opengassim-export/v1`.
- Changement **breaking** (renommage/suppression de champ) => `v2`.
- Changement **non-breaking** (ajout de champ facultatif) => même version majeure.

En cas d'évolution, documenter la migration dans ce fichier avec exemples avant/après.
