# Fonctionnalités MVP — OpenGasSim

## Inclus dans le MVP

1. **Chargement de réseau GasLib**
   - Parsing XML des fichiers .net (topologie, dimensions, coordonnées)
   - Support de GasLib-11 (11 nœuds)
   - Extension progressive vers GasLib-24 et GasLib-40

2. **Simulation en régime permanent**
   - Équations de Darcy-Weisbach
   - Solveur Newton-Raphson (ou Picard)
   - Résultats : pression à chaque nœud, débit dans chaque tuyau

3. **Visualisation géospatiale**
   - Globe CesiumJS avec fond de carte
   - Nœuds positionnés par GPS (WGS84)
   - Tuyaux tracés entre nœuds
   - Coloration dynamique selon le débit
   - Panel latéral avec les résultats numériques

4. **API REST**
   - `GET /api/network` — topologie du réseau
   - `GET /api/simulate` — résultats de la simulation

## Hors MVP (Phase 4+)

- Régime transitoire (simulation dans le temps)
- Stations de compression et vannes de régulation
- Édition graphique du réseau
- Parallélisation GPU (wgpu)
- Import/export de scénarios
- Multi-utilisateurs / sessions
