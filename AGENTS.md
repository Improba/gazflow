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

## Cursor Cloud specific instructions

### Docker-in-Docker

The Cloud Agent VM does not ship Docker pre-installed. The VM snapshot installs Docker CE, `fuse-overlayfs`, and configures `iptables-legacy` for DinD compatibility. After the snapshot boots, the Docker daemon must be started before any `docker compose` command:

```bash
sudo dockerd &>/tmp/dockerd.log &
sleep 3
```

### Starting development services

See `README.md` § Quickstart. Summary:

1. **GasLib data** (one-time): `./scripts/fetch_gaslib.sh GasLib-11` — downloads XML topology into `back/dat/`.
2. **Services**: `sudo docker compose up -d --build` (backend on `:3001`, frontend on `:9000`).
3. First start compiles Rust deps (~60-90s); subsequent starts use cached volumes (`back-target`, `cargo-registry`).

Use `sudo` for all `docker` / `docker compose` commands (the `ubuntu` user is in the `docker` group but the shell session may not reflect it without re-login).

### Running tests

- Backend: `sudo docker compose exec back cargo test`
- Frontend: `sudo docker compose exec front npm test`
- Full CI: `sudo docker compose run --rm back cargo test && sudo docker compose run --rm front npx quasar build`
- See `docs/testing/README.md` for targeted tests and validation packs.

### Key gotchas

- The frontend container runs `npm install` on startup (CMD in Dockerfile). If `node_modules` volume is stale after dependency changes, remove it: `sudo docker volume rm workspace_front-node-modules` then restart.
- `cargo watch` hot-reloads the backend on file changes; no manual restart needed.
- The `back/dat/` directory is `.gitignored` — always re-run `fetch_gaslib.sh` on a fresh clone.
