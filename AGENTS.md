# tradoor (Rust workspace, WASM chart engine, SolidJS web app run with Deno)

## Setup and verify
Rust 1.95 with the wasm32 target, wasm-bindgen 0.2.128, wasm-opt and Deno are installed. From the repo root:
- `cargo test --workspace --exclude tradoor-wasm --locked` — must pass. The first run compiles for about 6 minutes.
- `./scripts/build-wasm.sh` — must pass. Writes `web/src/wasm/pkg` (git-ignored); every web step below needs it.
- `cd web && deno install && deno task check && deno task test` — must pass (tsc, then the web tests).
- `cd web && deno task build && deno task size` — must pass. `size` enforces the gzip budgets in `web/package.json`.
- A change with nothing under `crates/`, `bins/` or `migrations/` may skip `cargo test`; it still needs `build-wasm.sh`.
- Do not run `cargo test -- --ignored`, `docker compose`, or the server, recorder and backfill binaries: the sandbox has no Postgres.
- UI check: after `deno task build`, run `cd web && nohup deno task preview --host 127.0.0.1 --port 4173 >/tmp/preview.log 2>&1 &`, then `node /lib-agent/shot.cjs http://127.0.0.1:4173/ <name> 1440x900 15000`. Charts draw from live exchange data. `/api` and `/data` answer 502 because no server runs, so anything that needs it (journal, alerts, accounts) cannot be checked.

## Rules
- A merge to `main` deploys to production. Change only what the issue asks.
- `crates/core`, `crates/codec` and `crates/render` also build to WASM for the browser: no I/O and no new dependencies in them.
- The web app uses Deno: `deno install` and `web/deno.lock`, never `npm install` or a `package-lock.json`.
- A schema change is one new file `migrations/V<highest + 1>__<name>.sql`. Never edit or renumber an existing migration.
- If `deno task size` fails, shrink the change first. Raise a limit in `web/package.json` only by what the change needs, and say so in the summary.
- Rust tests are inline `#[cfg(test)]` modules next to the code; web tests are `*.test.ts` next to the module.
- Do not edit `.github/` or this file. Do not commit or push; the runner does that.
