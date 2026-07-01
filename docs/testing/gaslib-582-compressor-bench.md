# GasLib-582 — bench compresseur (Phase I, juin–juillet 2026)

Protocole figé : `compressor_diag`, réseau baseline connecté, CDF off, scénario `nomination_mild_618.scn`, preset `robust` (release).

## Définitions (à ne pas confondre)

| Terme | Définition |
|-------|------------|
| **`residual`** | Max \|f_node\| sur nœuds libres Newton = **déséquilibre massique nodal** au state retourné (m³/s). |
| **`mass_balance`** (JSON) | Même quantité par nœud, avec demandes **effectives** (slack + boundaries assouplies retirées). |
| **`nomination_mass_balance`** (JSON) | Bilan avec demandes **nominales** du `.scn` — fidélité nomination GasLib. |
| **Partial accept** | Newton outer loop retourne le dernier état si `residual > tolerance` au lieu d'échouer. |
| **Référence nomination intacte** | Sans `contract_flow_relaxed` : le solveur impose encore les Q du mild_618. |

## Synthèse scientifique (juillet 2026)

| Indicateur | Nomination intacte (v17) | Avec heuristique v18 (Q retiré) |
|------------|------------------------|----------------------------------|
| Résidu effectif | **2,045 m³/s** | **~2,000 m³/s** (run unique, non revalidé après correctif gate) |
| Résidu nominal (`nomination_mass_balance`) | = effectif | **>> 2** (violation Q sur boundaries assouplies) |
| Tolérance preset robust | 3×10⁻³ m³/s | idem |
| Convergence stricte | Non (partial accept) | idem |
| Pire nœud (v17) | `sink_24` (Q nominatif −4,96 m³/s imposé) | `innode_402` après assouplissement |

**Conclusion Phase I** : le plancher **~2 m³/s** n'est pas un artefact d'arrondi ; c'est l'échelle du déséquilibre massique au state partial accept. Une douzaine de nœuds libres portent simultanément \|imbalance\| ≈ résidu (signature d'un état **non convergé**, pas d'erreurs locales indépendantes). L'assouplissement v18 **change le problème** (retire des Q contractuels) : la baisse 2,045 → 2,000 n'est **pas** une convergence vers la nomination GasLib.

Progression (résidu **effectif**, nomination intacte sauf mention) :

```
8,2 → 5,0 (v4) → 3,0 (v13) → 2,045 (v14–v19, HEAD_JAC off)
```

v19 (Jacobian carte→P² opt-in) : pas de gain ; ON légèrement pire (2,045, run unique). Ce n'est **pas** un modèle enthalpique : toujours MVP P² avec coefficient issu de `had_to_pressure_ratio`.

Référence architecture : [gaslib-582-compressor-diagnosis.md](./gaslib-582-compressor-diagnosis.md).

## Méthodologie et limites

- **Runs manuels** hors CI ; durée ~15–25 min/run (582, release, refinement).
- **Une répétition** par version documentée — pas d'intervalle de confiance.
- **Non déterminisme léger** : ordre des assouplissements contractuels dépend du bilan massique post-solve.
- **Correctif juillet 2026** : les assouplissements contractuels v18 sont **revertés** si une passe n'améliore pas le résidu (aligné sur la gate des ancrages `innode_*`). Les chiffres v18 « 2,000 » proviennent d'une version qui conservait les relaxations sans gain intermédiaire.

## Commandes

```bash
# Bench reproductible (nomination intacte par défaut)
./scripts/bench-gaslib-582.sh nominal

# Expérience v18 (assouplissement Q contractuel)
GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT=1 ./scripts/bench-gaslib-582.sh contract-relax

cd back && cargo build --release --bin compressor_diag

GAZFLOW_COMPRESSOR_MAP_MODE=measurement ./target/release/compressor_diag GasLib-582 --json /tmp/582-measurement.json

# Référence nomination intacte (défaut bench script)
./scripts/bench-gaslib-582.sh nominal

# Expérience assouplissement Q (hors nomination)
GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT=1 ./scripts/bench-gaslib-582.sh contract-relax
```

## Variables d'environnement (Phase I)

| Variable | Rôle | Défaut |
|----------|------|--------|
| `GAZFLOW_COMPRESSOR_MAP_MODE` | `legacy` \| `measurement` \| `biquadratic` | — |
| `GAZFLOW_NEWTON_COMPRESSOR_MAP` | Carte → coeff P² recouplé in-Newton (v17) | `1` en measurement |
| `GAZFLOW_NEWTON_COMPRESSOR_HEAD_JAC` | ∂(coeff carte)/∂Q, ∂/∂P_in implicite (v19) | `0` |
| `GAZFLOW_COMPRESSOR_STRICT_NEWTON` | Désactive partial accept outer loop | `0` |
| `GAZFLOW_CONTRACT_BOUNDARY_REFINEMENT` | Retrait Q itératif boundaries (v18, **opt-in**) | `0` |
| `GAZFLOW_RELAX_DUAL_PRESSURE_CONTRACTS` | Retrait Q upfront (29 nœuds mild_618) | `0` |
| `GAZFLOW_MASS_BALANCE_REFINEMENT_PASSES` | Passes post-solve ancrages / contract | `4` |
| `GAZFLOW_COMPRESSOR_R2_CAP_UNTIL_CONVERGED` | Plafond r²≤9 jusqu'à convergence | `1` |
| `GAZFLOW_COMPRESSOR_OUTER_MAX_ITERS` | Itérations outer loop ratio | 12 |
| `GAZFLOW_COMPRESSOR_RELAX` | Relaxation ω mise à jour ratio | 0.5 |

## Champs JSON `compressor_diag`

| Champ | Description |
|-------|-------------|
| `residual` | Max \|f_node\| nœuds libres (≈ pire déséquilibre massique effectif) |
| `mass_balance` | Bilan avec demandes **effectives** (`effective_solver_demands`) |
| `nomination_mass_balance` | Bilan avec demandes **nominales** `.scn` |
| `contract_flow_relaxed` | Boundaries dont le Q nominatif a été retiré (v18) |
| `mass_balance_refinement_passes` | Passes refinement post-solve |
| `mass_balance_anchors` | Innodes ancrés dynamiquement |
| `compressor_stations[]` | `flow_m3s`, `ratio_max`, `map_target_ratio`, … |

Artefact référence nomination intacte : `/tmp/582-v17.json` (résidu 2,045 m³/s).

## Progression chronologique (résidu effectif mild_618)

| v | Résidu | Levier principal |
|---|--------|------------------|
| v4 | 5,0 | garde outer loop + r² hybride |
| v13 | 3,0 | balance hubs (`sink_2`, `sink_96`) |
| v14–v17 | **2,045** | junction/spine anchors ; in-Newton map (v17 sans gain) |
| v18 | 2,045 → ~2,000* | assouplissement Q (*hors nomination) |
| v19 | 2,045 | head-Jacobian opt-in, défaut off |

## Interprétation globale

1. **Option 1 ratio** : `compressor_ratio_max` ← `.cs` ; cap `.net` séparé — validé.
2. **Ancrages pression (v13–v16)** : gain réel 5 → 2,045 m³/s sur nomination intacte.
3. **Partial accept** : masque l'échec convergence ; cluster ±2 m³/s sur ~14 nœuds = état global non convergé.
4. **v18** : heuristique numérique, pas solution contractuelle ; toujours reporter `nomination_mass_balance`.
5. **v19** : Jacobian semi-implicite sur coeff P² — pas modèle enthalpique ; pas de gain mesuré.
6. **Prochain levier** : bilan énergétique compresseur ou convergence stricte pour qualifier le plancher.

## Test intégration

`test_solve_gaslib_582` (`.scn` défaut, tol smoke 0,3) : `GAZFLOW_ENABLE_LARGE_DATASET_TESTS=1`.
