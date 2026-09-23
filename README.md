# tradoor

A trading terminal in the browser: TradingView-style charts and ExoCharts-style
market profiles in a workspace you lay out yourself. Lightness and speed come
first.

| Part | What it is |
|---|---|
| Chart engine | Rust compiled to WebAssembly (115 KB gzip after wasm-opt), run in a Web Worker. Candles are drawn with WebGL2 on an OffscreenCanvas and text on a 2D layer, so the page thread never touches a candle. |
| History | Immutable columnar partition files, about 16 bytes per 1-minute bar: five years of BTCUSDT perpetual minutes is 42 MB. Served from disk or a CDN and cached by the browser for a year. |
| Live | Exchange WebSockets straight to the browser: trades folded into the forming bar between kline updates, and liquidations feeding a ticker above the volume and a tape of their own as they land. |
| Server | Rust — Axum and Tokio — for the market catalogue, drawings and workspaces, and a day of liquidations for a few tickers. |
| Database | PostgreSQL 18, for metadata only. Candles never enter it. |
| UI | SolidJS (35 KB gzip): splits, tabs, docking by drag, link groups, command palette, theme; one scrolling column on a phone. |

## Repository

```text
crates/core      candles, timeframes, indicators and studies with Pine Script semantics; no dependencies
crates/codec     the TDC1 partition format (docs/format.md)
crates/venues    the exchanges: market lists, klines, archives (Binance and Bybit)
crates/store     the data directory and PostgreSQL metadata
crates/ingest    archive and REST sync into partitions
crates/render    the chart as a pure model: layout, scales, interaction, frames
crates/wasm      the browser engine: WebGL2 and text, decoding, folding, live bars
bins/server      tradoor-server — API, /data, and optionally the built web app
bins/backfill    tradoor-backfill — download history
bins/recorder    tradoor-recorder — keep tracked markets current
web/             the SolidJS app, built with Vite under Deno
migrations/      PostgreSQL schema, applied by every binary on start
deploy/          systemd units and an nginx site
```

How the pieces fit is in [docs/architecture.md](docs/architecture.md).

## Requirements

- Rust 1.95 with the `wasm32-unknown-unknown` target, and `wasm-bindgen-cli` 0.2.128
- Deno 2
- Docker, for PostgreSQL
- Optional: `wasm-opt` from binaryen, which `scripts/build-wasm.sh` uses when present

## Run it locally

```sh
cp .env.example .env          # choose a database password
docker compose up -d          # PostgreSQL 18 in its own container, on 127.0.0.1:5438
./scripts/build-wasm.sh       # the chart engine, into web/src/wasm/pkg
cargo build --release

set -a; . ./.env; set +a
# Five years of BTCUSDT perpetual, tracked so the recorder keeps it current.
./target/release/tradoor-backfill binance-perp:BTCUSDT --from 2021-09-01 --track
./target/release/tradoor-recorder &
./target/release/tradoor-server

cd web && deno install && deno task dev   # http://127.0.0.1:5173
```

Markets are written `venue:SYMBOL`, with the venue one of `binance-spot`,
`binance-perp`, `bybit-spot` or `bybit-perp`. A market that has not been backfilled
still charts: its candles come from the exchange's REST API instead of partitions.

`cargo test` runs the Rust tests; `deno task check` type-checks the web app,
`deno task test` replays recorded exchange payloads through the stream hub, and
`deno task size` checks the bundle budget after `deno task build`.

## Production

The `Dockerfile` builds one image holding the three binaries and the web app.
`deploy/compose.yml` runs it beside PostgreSQL and copies the web app out to
`./web`; the host's nginx serves that folder and proxies `/api` and `/data`
(`deploy/nginx/tradoor.conf`). For a Debian server with Docker and nginx, in
`/var/www/tradoor`:

```sh
# On your machine: build the image and copy it and the deploy files over.
docker build -t tradoor:latest .
ssh user@server 'sudo mkdir -p /var/www/tradoor && sudo chown $USER /var/www/tradoor'
docker save tradoor:latest | gzip | ssh user@server 'gunzip | docker load'
scp deploy/compose.yml deploy/nginx/tradoor.conf user@server:/var/www/tradoor/
scp deploy/env.example user@server:/var/www/tradoor/.env

# On the server.
cd /var/www/tradoor
nano .env        # POSTGRES_PASSWORD: openssl rand -hex 24
docker compose up -d
docker compose run --rm server tradoor-backfill binance-perp:BTCUSDT --from 2021-09-01 --track

sudo cp tradoor.conf /etc/nginx/sites-available/tradoor   # set server_name first
sudo ln -s /etc/nginx/sites-available/tradoor /etc/nginx/sites-enabled/
sudo nginx -t && sudo systemctl reload nginx
sudo certbot --nginx -d tradoor.example.com
```

The server and recorder apply database migrations when they start; partitions
live in the `data` volume. Drawings and layouts need no setup: each browser
keeps its own under its browser id.

To update, rebuild and load the image again, then `docker compose up -d` on the
server; that also refreshes `web/`. Without Docker, `deploy/systemd` runs the
binaries directly.

## CI and deploy

`.github/workflows/ci.yml` runs on every push and pull request: Rust tests, the
WASM engine, and the web app's type check, build and size budget. A push to
`main` then builds the image with its dependency layers cached, pushes it to
GitHub Container Registry as `ghcr.io/<owner>/tradoor:<commit>`, copies
`deploy/compose.yml` to the server, points `TRADOOR_IMAGE` in `.env` at that
commit, runs `docker compose up -d` and waits for `/api/health`.

One-time setup, after the server runs the stack as described above:

```sh
# A key only GitHub Actions uses, authorised on the server.
ssh-keygen -t ed25519 -N '' -C tradoor-github-actions -f ~/.ssh/tradoor_deploy
ssh -p <port> root@<server> 'cat >> ~/.ssh/authorized_keys' < ~/.ssh/tradoor_deploy.pub

# Repository secrets.
gh secret set VPS_HOST --body <server>
gh secret set VPS_PORT --body <port>
gh secret set VPS_USER --body root
gh secret set VPS_SSH_KEY < ~/.ssh/tradoor_deploy
ssh-keyscan -p <port> <server> | gh secret set VPS_KNOWN_HOSTS
```

To roll back, set `TRADOOR_IMAGE` in `/var/www/tradoor/.env` to an earlier
commit's tag and run `docker compose up -d`.

## Drawings and layouts belong to a browser

Each browser makes a random id on first visit (Settings → Browser id) and its
drawings, layout, watchlists and indicators are kept on the server under it: a
private window or another browser starts empty. To carry them to another
device, paste the id there.

Drawings and the layout saved before browsers had ids belong to nobody. Give
them to your browser's id once:

```sh
docker compose exec server tradoor-server adopt <your-browser-id>
```

## Moving drawings from rsalianto

```sh
RSALIANTO_DATABASE_URL=postgres://… scripts/export-rsalianto-drawings.sh > drawings.jsonl
./target/release/tradoor-server import-drawings drawings.jsonl --owner <your-browser-id>
```

## Keyboard

`a–z` search a symbol · `← →` timeframe · `/` or `Ctrl K` commands · `?` help ·
`Esc` cancel a tool or deselect · `Delete` remove the selected drawing · drag a
tab onto another group, or its edge, to dock it.
