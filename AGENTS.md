# AGENTS.md — GazFlow

This file defines contribution rules for agents/assistants only. Detailed execution procedures (setup, scripts, tests) are in the READMEs.

## Sources of truth

- Setup and scripts: `README.md`
- Test execution: `docs/testing/README.md`
- Project priorities and detailed scientific protocol (shared): `docs/plans/implementation-plan.md`
- Local unversioned plans/drafts: `docs/temps/`
- Physical model / equations: `docs/science/equations.md`

## Contribution rules

1. **Docker required**: do not run `cargo`/`npm`/`npx` on the host.
2. **Before modifying**: read the affected files and the corresponding phase of the plan.
3. **After modifying**: run at least the targeted tests for the modified scope.
4. **If physical logic is modified**: update the scientific documentation and related tests.
5. **If plan tasks are impacted**: update the status in `docs/plans/implementation-plan.md`.
6. **Never version GasLib data** in `back/dat/`.
7. **Temporary plan files**: use `docs/temps/` (content ignored by git).

## Minimal technical conventions

### Backend Rust

- `anyhow` for application errors; `thiserror` for library errors.
- Doc-comments for public modules/functions.
- Unit tests in modules (`#[cfg(test)]`) when applicable.
- Do not block Tokio for CPU-bound work (use `spawn_blocking`).

### Frontend Vue/TypeScript

- Strict TypeScript.
- Composition API only.
- Unit tests via `vitest`.

## Anti-redundancy principle

Do not duplicate here sections already maintained elsewhere (scripts, quickstart, catalogues). Add a link to the source of truth rather than a copy.
