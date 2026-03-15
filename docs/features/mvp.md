# MVP features — GazFlow

## Included in the MVP

1. **GasLib network loading**
   - XML parsing of .net files (topology, dimensions, coordinates)
   - Support for GasLib-11 (11 nodes)
   - Progressive extension to GasLib-24 and GasLib-40

2. **Steady-state simulation**
   - Darcy-Weisbach equations
   - Newton-Raphson (or Picard) solver
   - Results: pressure at each node, flow in each pipe

3. **Geospatial visualisation**
   - CesiumJS globe with base map
   - Nodes positioned by GPS (WGS84)
   - Pipes drawn between nodes
   - Dynamic colouring by flow
   - Side panel with numerical results

4. **REST API**
   - `GET /api/network` — network topology
   - `GET /api/simulate` — simulation results

## Beyond MVP (Phase 4+)

- Transient regime (time-domain simulation)
- Compressor stations and control valves
- Graphical network editing
- GPU parallelisation (wgpu)
- Scenario import/export
- Multi-user / sessions
