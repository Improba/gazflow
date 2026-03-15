# Architecture — GazFlow

## Guiding principles

1. **Systematic multi-threading:** computation never blocks I/O.
2. **Streaming:** results are sent to the client during solving, not after.
3. **Data parallelism:** Rayon for pipe traversal, faer for linear algebra.

---

## Flow diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                           Web Browser                                │
│  ┌──────────────┐  ┌──────────┐  ┌──────────┐  ┌────────────────┐  │
│  │  QuasarJS UI  │  │  Pinia   │  │ WS client│  │  CesiumJS       │  │
│  │  SimPanel     │◄►│  Stores  │◄►│ (live)   │  │  Globe 3D       │  │
│  │  LogPanel     │  │          │  │          │  │  (live colors)   │  │
│  │  DemandCtrl   │  │          │  │          │  │                 │  │
│  └──────────────┘  └────┬─────┘  └────┬─────┘  └────────────────┘  │
│                         │              │                            │
│                         │ HTTP         │ WebSocket                  │
└─────────────────────────┼──────────────┼────────────────────────────┘
                          │              │
               ┌──────────▼──────────────▼──────────┐
               │         Axum Server :3001          │
               │  ┌──────────┐  ┌─────────────────┐  │
               │  │ REST API │  │ WebSocket handler│  │
               │  │ /network │  │ /ws/simulation   │  │
               │  │ /health  │  │                  │  │
               │  └──────────┘  └────────┬────────┘  │
               │                         │          │
               │         tokio::spawn_blocking       │
               │                         │          │
               │  ┌──────────────────────▼─────────┐  │
               │  │        Solver Thread Pool      │  │
               │  │  ┌─────────────────────────┐   │  │
               │  │  │   Newton-Raphson Loop    │   │  │
               │  │  │                          │   │  │
               │  │  │  ┌──────────────────┐   │   │  │
               │  │  │  │  Rayon par_iter   │   │   │  │
               │  │  │  │  (pipes → residual)│   │   │  │
               │  │  │  └──────────────────┘   │   │  │
               │  │  │                          │   │  │
               │  │  │  ┌──────────────────┐   │   │  │
               │  │  │  │  faer sparse LU   │   │   │  │
               │  │  │  │  (Jacobian solve) │   │   │  │
               │  │  │  └──────────────────┘   │   │  │
               │  │  │           │              │   │  │
               │  │  │     mpsc::Sender         │   │  │
               │  │  │     (progress)           │   │  │
               │  │  └─────────────────────────┘   │  │
               │  └────────────────────────────────┘  │
               └──────────────────────────────────────┘
                          │
               ┌──────────▼──────────┐
               │   GasNetwork        │  Arc<GasNetwork>
               │   (petgraph, immutable, thread-safe)
               └──────────┬──────────┘
                          │
               ┌──────────▼──────────┐
               │   GasLib Parser     │  (quick-xml)
               │   .net + .scn       │
               └──────────┬──────────┘
                          │
               ┌──────────▼──────────┐
               │   back/dat/         │
               │   GasLib-11, 24…   │
               └─────────────────────┘
```

---

## Multi-threading strategy

### Level 1: I/O vs computation (tokio + spawn_blocking)

The tokio runtime handles HTTP and WebSocket connections asynchronously. The solver is pure CPU-bound work and runs via `tokio::spawn_blocking` so it does not block I/O tasks.

```rust
// api/ws.rs (simplified)
let (tx, mut rx) = tokio::sync::mpsc::channel(64);

tokio::spawn_blocking(move || {
    solve_with_progress(&network, &demands, tx);
});

while let Some(msg) = rx.recv().await {
    ws_sender.send(Message::Text(serde_json::to_string(&msg)?)).await?;
}
```

### Level 2: Data parallelism (Rayon)

At each solver iteration, nodal residual and Jacobian computation traverse all pipes. This traversal is parallelised via Rayon:

```rust
use rayon::prelude::*;

// Parallel computation of pipe → (f_node, j_diag) contributions
let contributions: Vec<(usize, usize, f64, f64)> = pipes
    .par_iter()
    .map(|pipe| {
        let k = pipe_resistance(pipe);
        let dp = pressures_sq[pipe.from_idx] - pressures_sq[pipe.to_idx];
        let q = dp.signum() * (dp.abs() / k).sqrt();
        let g = 1.0 / (2.0 * (k * dp.abs().max(1e-10)).sqrt());
        (pipe.from_idx, pipe.to_idx, q, g)
    })
    .collect();

// Sequential reduction (fast, just additions)
for (a, b, q, g) in contributions {
    f_node[a] -= q;
    f_node[b] += q;
    j_diag[a] += g;
    j_diag[b] += g;
}
```

**Expected gain:** significant from ~100 pipes (GasLib-135+). For GasLib-11, Rayon overhead dominates — parallelism can be conditioned on pipe count.

### Level 3: Sparse linear algebra (faer)

For full Newton-Raphson, the system `J · Δπ = -F` is solved with sparse LU decomposition (faer). faer uses internal parallelism for matrix operations.

```rust
use faer::sparse::*;

// Sparse Jacobian assembly (CSC format)
let jacobian = assemble_sparse_jacobian(&network, &pressures_sq);
let lu = jacobian.sp_lu(); // faer parallelises internally
let delta = lu.solve(&rhs);
```

### Level 4: Concurrent simulations

Multiple WebSocket clients can run simulations at the same time with different demand parameters. Each simulation runs in its own `spawn_blocking`, sharing the network (`Arc<GasNetwork>`, immutable and thread-safe without mutex).

---

## Backend components

| Module | Responsibility | Thread model | Crate |
|--------|----------------|-------------|-------|
| `api::rest` | REST endpoints (network, health) | tokio async | `axum` |
| `api::ws` | WebSocket simulation streaming | tokio async → spawn_blocking | `axum`, `tokio` |
| `gaslib` | GasLib XML parsing (.net, .scn) | single-threaded (startup) | `quick-xml` |
| `graph` | Network model (`Arc<GasNetwork>`) | immutable, thread-safe | `petgraph` |
| `solver` | Newton-Raphson + Jacobi | CPU-bound, Rayon parallel | `faer`, `rayon` |

## Frontend components

| Component | Responsibility |
|-----------|----------------|
| `CesiumViewer` | 3D globe, live colour updates on each WS snapshot |
| `SimulationPanel` | Start/stop, final pressures and flows |
| `LogPanel` | Scrollable iteration stream (iter, residual, time) |
| `ProgressBar` | Progress bar + current residual |
| `DemandControls` | Demand sliders per sink node |
| `Legend` | Colour gradient (pressure or flow) |
| `ws` service | WebSocket connection, auto-reconnect |
| `network` store | Network topology (REST) |
| `simulate` store | Simulation state (WS: progress + result) |

---

## Communication

| Channel | Transport | Direction | Usage |
|---------|-----------|-----------|-------|
| Network topology | REST GET | Front → Back | On load |
| Start simulation | WebSocket | Front → Back | Start + parameters |
| Iteration progress | WebSocket | Back → Front | Each iteration |
| Intermediate snapshots | WebSocket | Back → Front | Every N iterations |
| Final result | WebSocket | Back → Front | On convergence |
| Result export | REST GET | Front → Back | JSON/CSV/ZIP download post-convergence |

---

## Deployment

### Development (Docker Compose)

```bash
./scripts/dev.sh   # docker compose up --build
# back:3001  (Axum + cargo-watch)
# front:9000 (Quasar dev + proxy → back:3001)
```

### Production

```bash
# Optimised build
cd back && cargo build --release
cd front && quasar build

# Rust binary serves both API and static files
./target/release/gazflow-back
# :3001/api/*      → REST API + WebSocket
# :3001/*          → Quasar static files (tower-http::fs)
```
