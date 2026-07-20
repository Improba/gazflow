# Quarantaine : `test_gaslib_11_vs_reference_solution`

**Statut** : test ignoré (`#[ignore]`) depuis juillet 2026. À ne pas réactiver sans résoudre les causes ci-dessous.

## Symptôme

`test_gaslib_11_vs_reference_solution` échoue à ~8 % d'erreur relative de pression (`worst_node=exit02`) contre la référence `docs/testing/references/GasLib-11.reference.internal.csv`.

## Diagnostic (vérifié, pas hypothèse)

L'échec **n'est pas un bug du modèle**. Quatre causes convergentes, toutes confirmées par expérimentation :

### 0. Pas de fichier `.sol` dans le ZIP ZIB

L'archive officielle GasLib-11 (`GasLib-11-v1-20211130.zip`) ne contient **aucun** fichier `.sol` (solution de référence Pfetsch et al.). Le script `fetch_gaslib.sh` crée un alias `GasLib-11.sol` uniquement si un `.sol` versionné est présent après extraction. Validation pro = formule analytique P² + invariants internes (bilan masse PDE, linepack↔capacitance) ; pas d'oracle externe pour GasLib-11.

### 1. Problème sous-déterminé

`dat/GasLib-11.scn` ne fixe **aucune pression d'entry** (uniquement des débits). Sans pression imposée, le niveau de pression du réseau est indéterminé : il existe une famille de solutions valides paramétrée par le nœud « slack » qu'on ancre. Le solveur ancre un nœud au débit max à 70 bar (`back/src/solver/newton.rs`, picker d'ancrage introduit en `93fd558`).

- ancien picker : ancre `exit01` → la référence flotte (`entry01/03 = 64,81 bar`, `entry02 = 70 bar`).
- picker actuel : ancre `entry01` (amont du compresseur CS01) → pousse `N01/N03` à 75,6 bar → force `entry02` à 75,6 bar.

**Les deux sont des solutions valides** d'un problème sous-déterminé. Ni l'une ni l'autre n'est « la » réponse.

### 2. Référence auto-générée invalide

`GasLib-11.reference.internal.csv` est produite par `back/src/bin/generate_gaslib11_reference.rs` (le solveur lui-même), **pas** par un oracle externe. Elle **viole les `pressureMax` natifs du `.net`** :

| Nœud | pression résolue (référence) | `pressureMax` `.net` |
|------|---|---|
| `N05` (sortie compresseur CS02) | 75,59 | **70** |
| `exit02` | 75,59 | **60** |
| `exit03` | 75,59 | **60** |
| `entry02` (sortie solveur actuel) | 75,6 | **70** |

Elle a été générée par un solveur qui **n'appliquait pas les `pressureMax` natifs**. Ce n'est donc pas un oracle de validation crédible.

### 3. Le compresseur fonctionne

Le ratio compresseur 1,08 est bien appliqué : `N05 = N04 × 1,08` (81,64 bar en sortie du solveur actuel, 75,59 dans la référence). Le modèle compresseur MVP n'est **pas** en cause.

## Tentative de fix physique (prototypée, non conservée)

Imposer les `pressureMax` natifs `.net` + plafonner le ratio compresseur par le `pressureMax` aval (modèle « validation of nominations » : ratio = variable de décision bornée). Résultat :

- 582 nominal : **inchangé** (2,045227 m³/s). Neutre.
- 582 entry-anchor : **inchangé** (23,4957 m³/s). Neutre.
- GasLib-11 : **devient infeasible** (le solveur ne converge plus).

Cause : `exit02`/`exit03` ont `pressureMax = 60` bar mais nécessitent ~75,6 bar pour évacuer leur débit nominé. **La nomination mild_618 de GasLib-11 est infeasible sous bornes strictes.** La référence committée est une solution qui les viole.

Conclusion : imposer les bornes natives par défaut casserait les baselines sans apport (la référence est invalide de toute façon). Le machinery a été reverté.

## Conditions de réactivation

Ne réactiver ce test qu'après l'une de ces deux voies :

1. **Oracle officiel ZIB** : récupérer le vrai fichier `.sol` GasLib-11 (Pfetsch et al., ZIB-Report 12-41) — la solution reconnue — et valider contre lui, avec un ancrage physique des entries (pressions fixées au régime transport).
2. **Ancrage physique + réseau feasible** : imposer la convention d'ancrage entry-transport (`GAZFLOW_ENTRY_TRANSPORT_ANCHOR`) et choisir un réseau/scénario **feasible sous bornes strictes**. GasLib-11 mild étant infeasible, il n'est pas usable pour cette voie.

## Lancer le test malgré la quarantaine

```bash
cargo test --lib test_gaslib_11_vs_reference_solution -- --ignored
```

## Contexte projet

Le statut 582 (`nomination_mild_618`) est solide et indépendant de cette quarantaine :
- alias shortPipe corrigé (`2886fc1`) : `innode_3` résolu (9,16 → 69,97 bar) ;
- `sink_88` / `sink_83` infeasibles réellement (aucun compresseur sur 43-57 hops de distribution, control valves ne font qu'abaisser la pression) ;
- `sink_122` / `sink_125` marginaux (compresseur sur le chemin, activation pourrait combler).

Voir `docs/testing/gaslib-582-compressor-diagnosis.md` pour le détail 582.
