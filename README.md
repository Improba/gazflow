# GazFlow

Natural gas network flow simulator, inspired by SIMONE.

## Visual overview

<table>
  <tr>
    <td align="center" width="33%">
      <img src="docs/assets/overview-3d-network.png" alt="3D view of the transport network in GazFlow" />
    </td>
    <td align="center" width="33%">
      <img src="docs/assets/scenario-control-panel.png" alt="GazFlow simulation control panel" />
    </td>
    <td align="center" width="33%">
      <img src="docs/assets/simulation-results-convergence.png" alt="Hydraulic results and convergence in GazFlow" />
    </td>
  </tr>
  <tr>
    <td align="center"><em>3D network map</em></td>
    <td align="center"><em>Scenario control</em></td>
    <td align="center"><em>Reading results</em></td>
  </tr>
</table>

## What GazFlow does (business vision)

GazFlow simulates gas flow in a transport network from a GasLib topology and a demand scenario. The tool computes a steady-state hydraulic operating point (nodal pressures and pipe flows), then presents it for operational reading: 3D map, convergence monitoring, and usable exports. You can optionally attach **min/max flow bounds** per node (and use pipe bounds from the file) to **check** a scenario against capacities or **optimize** demands toward a feasible operating point.

### Use cases

- Study the hydraulic behaviour of a network under different withdrawal/injection levels
- Quickly visualise high/low pressure zones and the most loaded pipes
- **Capacity-aware workflows**: after loading a GasLib network, optional per-node min/max flows (m³/s) can be sent with a simulation; use **check** to flag violations or **optimize** to project demands toward feasible slack (source) throughput while staying close to the target scenario
- Compare scenarios and document results (JSON/CSV/XLSX/ZIP), including capacity diagnostics when applicable

### What the tool is not

GazFlow is a simulation and visualisation prototype inspired by industrial tools. It does not replace a certified network operation simulator.

### Capacity constraints (min / max flows)

The steady-state hydraulic core still solves for pressures and pipe flows from **nodal demands** (injections positive, withdrawals negative). On top of that, you can work with **flow bounds**:

- **From GasLib (`.net`)**: optional `flow_min` / `flow_max` on nodes and pipes are parsed into the graph. Node bounds appear on `GET /api/network` as `flow_min_m3s` / `flow_max_m3s`. Pipe bounds are kept on the backend and used whenever you run a capacity-aware solve.
- **From the client**: `POST /api/simulate` and the WebSocket `start_simulation` message accept optional `capacity_bounds` (`{ "nodeId": { "min", "max" } }`, m³/s) and optional `mode`:
  - **`check`** — Run the usual solve with your demands, then return **`capacity_violations`** where effective node net flows or pipe flows fall outside bounds.
  - **`optimize`** — Iterative **projection**: bounded free-node demands are clamped and the hydraulic solve is repeated; if a **slack** node (fixed pressure) would exceed its bounds, bounded free-node demands are adjusted proportionally until slack is feasible or an infeasibility / stagnation diagnostic is returned. The response includes **adjusted demands**, **active bounds**, and a simple squared-distance **objective** vs the target scenario.

This supports operational questions such as “does this nomination respect entry/exit-style envelopes?” and “what feasible demands are closest if the source is capped?”. It is **not** full market or contract optimisation (products, time slices, tariffs) unless you encode them yourself as static min/max.

For the algorithm and limitations in depth, see [Capacity constraints plan](docs/plans/capacity-constraints-plan.md).

## Architecture

- **back/** — Rust backend: computation engine (Darcy-Weisbach, Newton-Raphson) + REST API (Axum)
- **front/** — Vue 3 / QuasarJS / CesiumJS frontend: 3D geospatial visualisation
- **docker/** — Dockerfiles for back and front services
- **docs/** — Documentation (architecture, science, plans)

## Prerequisites

- Docker & Docker Compose

That’s it. Rust and Node toolchains live inside the containers.

## Quickstart

```bash
# 1. Download GasLib data
./scripts/fetch_gaslib.sh GasLib-11

# 2. Start the development environment
./scripts/dev.sh
```

- Backend (Rust API): `http://localhost:3001`
- Frontend (Quasar/CesiumJS): `http://localhost:9000`

## Scripts

| Script | Description |
|--------|-------------|
| `./scripts/dev.sh` | Starts back + front via Docker Compose |
| `./scripts/stop.sh` | Stops all containers |
| `./scripts/back-shell.sh` | Shell in the back container (`cargo add`, etc.) |
| `./scripts/front-shell.sh` | Shell in the front container (`npm install`, etc.) |
| `./scripts/back-test.sh` | Runs `cargo test` in the container |
| `./scripts/front-test.sh` | Runs `npm test` in the container |
| `./scripts/ci.sh` | Full CI (build + back & front tests) |
| `./scripts/fetch_gaslib.sh` | Downloads GasLib data |

## Adding a dependency

Always use the container:

```bash
# Rust
./scripts/back-shell.sh
cargo add my-crate

# Node
./scripts/front-shell.sh
npm install my-package
```

The `Cargo.toml` and `package.json` files are on the shared volume: changes are visible on the host and versioned by git.

## Tests

```bash
./scripts/back-test.sh     # Rust tests
./scripts/front-test.sh    # Frontend tests
./scripts/ci.sh            # Full CI
```

## Documentation

- [Quickstart](docs/quickstart.md)
- [Architecture](docs/architecture/overview.md)
- [Results export contract](docs/architecture/export-contract.md)
- [Physical equations](docs/science/equations.md)
- [Capacity constraints plan](docs/plans/capacity-constraints-plan.md)
- [Implementation plan (shared)](docs/plans/implementation-plan.md)
- [MVP features](docs/features/mvp.md)
