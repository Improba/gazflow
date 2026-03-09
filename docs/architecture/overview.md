# Architecture — GazSim

## Principes directeurs

1. **Multi-threading systématique** : le calcul ne bloque jamais l'I/O.
2. **Streaming** : les résultats sont envoyés au client pendant la résolution, pas après.
3. **Parallélisme de données** : Rayon pour le parcours des tuyaux, faer pour l'algèbre linéaire.

---

## Diagramme de flux

```
┌─────────────────────────────────────────────────────────────────────┐
│                           Navigateur Web                            │
│  ┌──────────────┐  ┌──────────┐  ┌──────────┐  ┌────────────────┐  │
│  │  QuasarJS UI  │  │  Pinia   │  │ WS client│  │  CesiumJS      │  │
│  │  SimPanel     │◄►│  Stores  │◄►│ (live)   │  │  Globe 3D      │  │
│  │  LogPanel     │  │          │  │          │  │  (live colors) │  │
│  │  DemandCtrl   │  │          │  │          │  │                │  │
│  └──────────────┘  └────┬─────┘  └────┬─────┘  └────────────────┘  │
│                         │              │                            │
│                         │ HTTP         │ WebSocket                  │
└─────────────────────────┼──────────────┼────────────────────────────┘
                          │              │
               ┌──────────▼──────────────▼──────────┐
               │         Axum Server :3001           │
               │  ┌──────────┐  ┌─────────────────┐  │
               │  │ REST API │  │ WebSocket handler│  │
               │  │ /network │  │ /ws/simulation   │  │
               │  │ /health  │  │                  │  │
               │  └──────────┘  └────────┬────────┘  │
               │                         │            │
               │         tokio::spawn_blocking        │
               │                         │            │
               │  ┌──────────────────────▼─────────┐  │
               │  │        Solver Thread Pool       │  │
               │  │  ┌─────────────────────────┐   │  │
               │  │  │   Newton-Raphson Loop    │   │  │
               │  │  │                          │   │  │
               │  │  │  ┌──────────────────┐   │   │  │
               │  │  │  │  Rayon par_iter   │   │   │  │
               │  │  │  │  (pipes → résidu) │   │   │  │
               │  │  │  └──────────────────┘   │   │  │
               │  │  │                          │   │  │
               │  │  │  ┌──────────────────┐   │   │  │
               │  │  │  │  faer sparse LU   │   │   │  │
               │  │  │  │  (Jacobien solve) │   │   │  │
               │  │  │  └──────────────────┘   │   │  │
               │  │  │           │              │   │  │
               │  │  │     mpsc::Sender         │   │  │
               │  │  │     (progression)        │   │  │
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
               │   Parseur GasLib    │  (quick-xml)
               │   .net + .scn       │
               └──────────┬──────────┘
                          │
               ┌──────────▼──────────┐
               │   back/dat/         │
               │   GasLib-11, 24…    │
               └─────────────────────┘
```

---

## Stratégie multi-threading

### Niveau 1 : I/O vs Calcul (tokio + spawn_blocking)

Le runtime tokio gère les connexions HTTP et WebSocket de manière asynchrone.
Le solveur, étant du calcul pur CPU-bound, est exécuté via `tokio::spawn_blocking`
pour ne pas bloquer les tâches I/O.

```rust
// api/ws.rs (simplifié)
let (tx, mut rx) = tokio::sync::mpsc::channel(64);

tokio::spawn_blocking(move || {
    solve_with_progress(&network, &demands, tx);
});

while let Some(msg) = rx.recv().await {
    ws_sender.send(Message::Text(serde_json::to_string(&msg)?)).await?;
}
```

### Niveau 2 : Parallélisme de données (Rayon)

À chaque itération du solveur, le calcul des résidus nodaux et du Jacobien
parcourt tous les tuyaux du réseau. Ce parcours est parallélisable via Rayon :

```rust
use rayon::prelude::*;

// Calcul parallèle des contributions pipe → (f_node, j_diag)
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

// Réduction séquentielle (rapide car juste des additions)
for (a, b, q, g) in contributions {
    f_node[a] -= q;
    f_node[b] += q;
    j_diag[a] += g;
    j_diag[b] += g;
}
```

**Gain attendu :** significatif à partir de ~100 tuyaux (GasLib-135+).
Pour GasLib-11, l'overhead de Rayon domine — on peut conditionner le parallélisme
au nombre de tuyaux.

### Niveau 3 : Algèbre linéaire creuse (faer)

Pour le Newton-Raphson complet, le système `J · Δπ = -F` est résolu
avec une décomposition LU creuse (faer). `faer` utilise nativement
le parallélisme (threads internes) pour les opérations sur matrices.

```rust
use faer::sparse::*;

// Assemblage du Jacobien creux (CSC format)
let jacobian = assemble_sparse_jacobian(&network, &pressures_sq);
let lu = jacobian.sp_lu(); // faer parallélise en interne
let delta = lu.solve(&rhs);
```

### Niveau 4 : Simulations concurrentes

Plusieurs clients WebSocket peuvent lancer des simulations simultanément
avec des paramètres de demande différents. Chaque simulation tourne dans
son propre `spawn_blocking`, partageant le réseau (`Arc<GasNetwork>`,
immutable donc thread-safe sans mutex).

---

## Composants backend

| Module | Responsabilité | Thread model | Crate |
|--------|---------------|-------------|-------|
| `api::rest` | Endpoints REST (network, health) | tokio async | `axum` |
| `api::ws` | WebSocket simulation streaming | tokio async → spawn_blocking | `axum`, `tokio` |
| `gaslib` | Parsing XML GasLib (.net, .scn) | single-threaded (startup) | `quick-xml` |
| `graph` | Modèle réseau (`Arc<GasNetwork>`) | immutable, thread-safe | `petgraph` |
| `solver` | Newton-Raphson + Jacobi | CPU-bound, Rayon parallel | `faer`, `rayon` |

## Composants frontend

| Composant | Responsabilité |
|-----------|---------------|
| `CesiumViewer` | Globe 3D, mise à jour live des couleurs à chaque snapshot WS |
| `SimulationPanel` | Start/stop, pressions et débits finaux |
| `LogPanel` | Flux scrollable des itérations (iter, résidu, temps) |
| `ProgressBar` | Barre de progression + résidu courant |
| `DemandControls` | Sliders de demande par nœud puits |
| `Legend` | Gradient de couleurs (pression ou débit) |
| `ws` service | Connexion WebSocket, reconnexion automatique |
| `network` store | Topologie du réseau (REST) |
| `simulate` store | État simulation (WS : progression + résultat) |

---

## Communication

| Canal | Transport | Direction | Usage |
|-------|-----------|-----------|-------|
| Topologie réseau | REST GET | Front → Back | Au chargement |
| Lancement simulation | WebSocket | Front → Back | Démarrage + paramètres |
| Progression itérations | WebSocket | Back → Front | Chaque itération |
| Snapshots intermédiaires | WebSocket | Back → Front | Toutes les N itérations |
| Résultat final | WebSocket | Back → Front | À convergence |
| Export résultats | REST GET | Front → Back | Téléchargement JSON/CSV/ZIP post-convergence |

---

## Déploiement

### Développement (Docker Compose)

```bash
./scripts/dev.sh   # docker compose up --build
# back:3001  (Axum + cargo-watch)
# front:9000 (Quasar dev + proxy → back:3001)
```

### Production

```bash
# Build optimisé
cd back && cargo build --release
cd front && quasar build

# Le binaire Rust sert à la fois l'API et les fichiers statiques
./target/release/gazsim-back
# :3001/api/*      → API REST + WebSocket
# :3001/*          → fichiers statiques Quasar (tower-http::fs)
```
