# Quickstart

Fastest way to run a GazFlow simulation locally.

## Prerequisites

- Docker
- Docker Compose

## 1) Fetch a dataset

```bash
./scripts/fetch_gaslib.sh GasLib-11
```

## 2) Start the environment

```bash
./scripts/dev.sh
```

## 3) Open the application

- Frontend: `http://localhost:9000`
- Backend API: `http://localhost:3001`

## 4) Run a simulation

1. Open the map page.
2. In the **Simulation** panel, click **Start**.
3. Follow progress (iterations, residual, logs) in real time.
4. Export to JSON/CSV/ZIP once converged.

## 5) Optional — transient simulation

1. Open **Transitoire** (`/transient`) from the task menu.
2. Choose **PDE** mode (trees/cycles supported; GasLib-11 works). Prefer `dt_s ≈ 60` with **adaptive dt** for multi-hour runs.
3. Set duration and time step, then run. Use the player to inspect pressures, flows, and `flows_in` / `flows_out` per step.

Transient results are not included in the steady-state ZIP export v1.

## Stop

```bash
./scripts/stop.sh
```
