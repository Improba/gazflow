# Architecture — OpenGasSim

## Diagramme de flux

```
┌─────────────────────────────────────────────────────────────────┐
│                        Navigateur Web                           │
│  ┌──────────────┐  ┌──────────────┐  ┌───────────────────────┐  │
│  │  QuasarJS UI  │  │  Pinia Store │  │     CesiumJS Globe    │  │
│  │  (panels,     │◄►│  (network,   │◄►│  (3D, entités,        │  │
│  │   contrôles)  │  │   simulate)  │  │   polylines)          │  │
│  └──────┬───────┘  └──────┬───────┘  └───────────────────────┘  │
│         │                 │                                      │
│         └─────────────────┘                                      │
│                  │ HTTP (axios)                                   │
└──────────────────┼───────────────────────────────────────────────┘
                   │
          ┌────────▼────────┐
          │    Axum Server   │  :3001
          │    (API REST)    │
          ├─────────────────┤
          │  GET /api/health │
          │  GET /api/network│
          │  GET /api/simulate│
          │  POST /api/simulate (futur) │
          └────────┬────────┘
                   │
          ┌────────▼────────┐
          │   GasNetwork     │  (petgraph DiGraph)
          │   (en mémoire)   │
          └────────┬────────┘
                   │
          ┌────────▼────────┐
          │  Solveur         │
          │  Newton-Raphson  │  (faer pour l'algèbre linéaire)
          │  Darcy-Weisbach  │
          └────────┬────────┘
                   │
          ┌────────▼────────┐
          │  Parseur GasLib  │  (quick-xml)
          │  XML → Network   │
          └────────┬────────┘
                   │
          ┌────────▼────────┐
          │  Fichiers .net   │
          │  GasLib-11, 24…  │
          │  (back/dat/)     │
          └─────────────────┘
```

## Composants principaux

### Backend (Rust)

| Module | Responsabilité | Crate principal |
|--------|---------------|-----------------|
| `gaslib` | Parsing des fichiers XML GasLib (.net, .scn) | `quick-xml` |
| `graph` | Modèle de données réseau (nœuds, tuyaux, compresseurs) | `petgraph` |
| `solver` | Résolution des équations d'écoulement | `faer` |
| `api` | Exposition REST des données et résultats | `axum` |

### Frontend (TypeScript / Vue 3)

| Composant | Responsabilité |
|-----------|---------------|
| `CesiumViewer` | Globe 3D, rendu des nœuds et tuyaux géolocalisés |
| `SimulationPanel` | Interface de contrôle et affichage des résultats |
| `network` store | État du réseau (fetch depuis l'API) |
| `simulate` store | État de la simulation (résultats, loading) |
| `api` service | Client HTTP (axios) vers le backend |

## Communication

- **Frontend → Backend** : HTTP REST via proxy Vite (`/api` → `localhost:3001`).
- **Données** : JSON. Les coordonnées GPS (WGS84) sont transmises pour le placement CesiumJS.
- **Futur** : WebSockets pour le streaming de résultats transitoires.

## Déploiement (MVP)

1. `cd back && cargo run` → serveur Axum sur :3001
2. `cd front && quasar dev` → dev server Vite sur :9000 avec proxy vers :3001
3. Production : `quasar build` → fichiers statiques servis par Axum (`tower-http::fs`)
