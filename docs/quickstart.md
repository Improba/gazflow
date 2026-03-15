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

## Stop

```bash
./scripts/stop.sh
```
