# AGENTS.md — GazFlow

Ce fichier définit uniquement les règles de contribution pour agents/assistants.
Les procédures d'exécution détaillées (setup, scripts, tests) sont dans les READMEs.

## Sources de vérité

- Setup et scripts: `README.md`
- Exécution des tests: `docs/testing/README.md`
- Priorités projet et protocole scientifique détaillé (partagé): `docs/plans/implementation-plan.md`
- Plans/brouillons locaux non versionnés: `docs/temps/`
- Modèle physique / équations: `docs/science/equations.md`

## Règles de contribution

1. **Docker obligatoire**: ne pas lancer `cargo`/`npm`/`npx` sur l'hôte.
2. **Avant modification**: lire les fichiers impactés et la phase correspondante du plan.
3. **Après modification**: exécuter au minimum les tests ciblés du scope modifié.
4. **Si logique physique modifiée**: mettre à jour la doc scientifique et les tests associés.
5. **Si tâches plan impactées**: mettre à jour le statut dans `docs/plans/implementation-plan.md`.
6. **Ne jamais versionner les données GasLib** dans `back/dat/`.
7. **Fichiers temporaires de plan**: utiliser `docs/temps/` (contenu local ignoré par git).

## Conventions techniques minimales

### Backend Rust

- `anyhow` pour erreurs applicatives; `thiserror` pour erreurs de bibliothèque.
- Doc-comments pour modules/fonctions publiques.
- Tests unitaires dans les modules (`#[cfg(test)]`) quand applicable.
- Ne pas bloquer Tokio pour du calcul CPU (utiliser `spawn_blocking`).

### Frontend Vue/TypeScript

- TypeScript strict.
- Composition API uniquement.
- Tests unitaires via `vitest`.

## Principe anti-redondance

Ne pas dupliquer ici des sections déjà maintenues ailleurs (scripts, quickstart, catalogues).
Ajouter un lien vers la source de vérité plutôt qu'une copie.
