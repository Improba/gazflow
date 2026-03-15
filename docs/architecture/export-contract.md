# Results export contract — GazFlow (v1)

## Objective

Define a **stable, versioned and testable** export contract for simulation results, to ensure:

- interoperability (data tools, scripts, Excel, BI),
- traceability (explicit metadata and units),
- smooth UX (export without blocking the map/live updates).

## Principles

- **Versioned:** each export includes `schema_version`.
- **Deterministic:** sorted order of nodes/pipes for reproducible diffs.
- **Self-describing:** units and simulation context included.
- **Backward-compatible:** adding fields allowed; removal/rename forbidden within the same major version.

---

## Export endpoints (MVP+ target)

### 1) Full JSON

`GET /api/export/{simulation_id}?format=json`

- Content-Type: `application/json`
- Response: full versioned payload (see schema below)
- Use: sharing, archiving, Python/R post-processing

### 2) Flat CSV

`GET /api/export/{simulation_id}?format=csv`

- Content-Type: `text/csv; charset=utf-8`
- Response: single table with `kind` column (`pressure` | `flow`)
- Use: direct spreadsheet opening / simple ETL

### 3) Full bundle (optional)

`GET /api/export/{simulation_id}?format=zip&include_logs=true`

- Content-Type: `application/zip`
- Recommended contents:
  - `result.json` (JSON v1 format),
  - `result.csv` (CSV v1 format),
  - `logs.ndjson` (if `include_logs=true`),
  - `context.json` (run/frontend info).

> If `simulation_id` is unknown: `404` with standard error payload.

---

## JSON schema v1

### Required fields

- `schema_version`: `"gazflow-export/v1"`
- `simulation`
- `units`
- `results`
- `stats`

### Example payload

```json
{
  "schema_version": "gazflow-export/v1",
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

### Serialisation rules

- `pressures` sorted by `node_id` (lexicographic order).
- `flows` sorted by `pipe_id` (lexicographic order).
- Numbers exported as `f64` (no locale format with comma).
- Dates in ISO-8601 UTC format.

---

## CSV contract v1

Required columns:

- `kind`
- `id`
- `from`
- `to`
- `value`
- `abs_value`
- `unit`
- `direction`

Semantics:

- `kind=pressure`: `id=node_id`, `from`/`to`/`direction` empty, `value=pressure`.
- `kind=flow`: `id=pipe_id`, `from`/`to` set, `value=signed flow`.

Example:

```csv
kind,id,from,to,value,abs_value,unit,direction
pressure,A,,,65.12,65.12,bar,
pressure,B,,,66.30,66.30,bar,
flow,SJ,S,J,10.0,10.0,m3/s,forward
flow,JA,J,A,5.2,5.2,m3/s,forward
```

---

## Error contract

Recommended error format (JSON):

```json
{
  "error": {
    "code": "EXPORT_NOT_FOUND",
    "message": "unknown simulation_id",
    "details": {
      "simulation_id": "sim_xxx"
    }
  }
}
```

Minimum codes:

- `EXPORT_NOT_FOUND` (`404`)
- `EXPORT_FORMAT_UNSUPPORTED` (`400`)
- `EXPORT_NOT_READY` (`409`)
- `EXPORT_INTERNAL_ERROR` (`500`)

---

## Fluidity requirements (UI + API)

- Export from an already converged result: **no solver recomputation**.
- Export must not block the UI thread: trigger asynchronously on the front.
- Under normal load, download start target `< 300 ms` after click.
- During export, Cesium navigation stays smooth (no perceptible freeze > 100 ms).

Implementation recommendations:

- backend: streaming serialisation when size is large;
- frontend: local `exporting` state (button spinner), without blocking other interactions;
- keep live state (`running`/`converged`/`error`) visible during export.

---

## Conformance test plan

- `test_export_result_json_schema`: presence of required fields and units.
- `test_export_result_csv_headers`: exact columns and stable order.
- `test_export_order_is_deterministic`: stable lexical sort.
- `test_export_unknown_id_returns_404`: error contract respected.
- E2E front: click export JSON/CSV on converged simulation + UI remains interactive.

---

## Versioning

- Current version: `gazflow-export/v1`.
- **Breaking** change (field rename/removal) => `v2`.
- **Non-breaking** change (optional field addition) => same major version.

On evolution, document migration in this file with before/after examples.
