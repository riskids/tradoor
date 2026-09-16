# Architecture

## Threads

```text
page (SolidJS)                         engine worker (WASM)                    network
─────────────────────────────────────  ──────────────────────────────────────  ─────────────────────────
layout, toolbars, legend, dialogs  ──► ChartEngine per chart                   /data partitions (CDN)
pointer and wheel input            ──►   tradoor-render model → Frame        ◄─ exchange REST (klines)
                                   ◄──   WebGL2 triangles + 2D text           ◄─ exchange WS (kline, trades)
legend, cursor, events, status     ◄──  history series, folding, live bars
watchlist tickers  ◄─ exchange WS                                             /api (tradoor-server)
drawings, workspace ─────────────────────────────────────────────────────────►
```

One worker serves every chart, so charts share WASM memory, loaded history and
exchange sockets. The page sends settings and input; the worker sends back
only what the DOM shows. Canvases are transferred to the worker with
`transferControlToOffscreen`, so drawing never waits on the page, and a busy
page never drops a frame of the chart.

## A chart, from request to pixels

1. `openSeries(market, base, interval)` reads `/data/{venue}/{SYMBOL}/manifest.json`.
   When the market is archived at the chart's base (1m for minutes, 1h for hours,
   1d for everything longer) the series is `archive`; otherwise `exchange`.
2. An archive series loads the newest partitions until it holds enough base bars
   for 1,500 chart bars, then asks the exchange for bars newer than the last
   stored one. An exchange series pages back through the REST API instead.
3. `fold_candles` builds the chart timeframe from the base bars — 4h from 1h, a
   Monday-aligned week from days — and drops a bucket that has not closed.
4. The kline stream (plus aggregate trades on Binance) feeds `LiveFeedEngine`,
   which never lets a stale kline rewind trades already folded. When the bar
   rolls over, the finished bar is kept as closed until REST confirms it.
5. On a perpetual the venue's liquidation stream is subscribed too. Each hit is
   pushed into the engine at the price it filled at and joins a feed above the
   volume: the newest lands at the foot, pushes the stack up as it arrives, and
   fades out five seconds later. A row carries the venue's mark and what the hit
   cost. Nothing is stored — the Liquidations panel keeps its own tape of the
   same stream in the tab, and a reload starts it empty.
6. `ChartEngine` (tradoor-render) turns candles, studies, drawings and the view
   into a `Frame`: one vertex buffer of triangles in draw order with clip
   batches, and a list of text commands. The worker uploads it in two buffer
   writes and replays the text on the 2D canvas. Nothing is drawn unless
   something changed or the countdown needs its next second.
7. Panning near the oldest bar held emits `needHistory`; the series loads the
   next older partition, or pages REST back, and every chart on it refolds.

Series nobody reads are dropped after two minutes, so a deep 1-minute history
does not outlive its chart.

Every chart and watchlist row shares one socket per venue, each topic subscribed
once and reference counted. Bybit refuses a whole subscribe frame when one topic
in it is unknown — `error:handler not found` — so a refused frame drops the topic
it names and re-sends the rest one at a time, and the named topic is not tried
again on that socket.

## Stored history

`tradoor-backfill` and `tradoor-recorder` write partitions; nothing else does.
Minutes are the source of truth: Binance's monthly and daily archives
(checksum-verified) first, then REST for what the archives do not have yet.
Hours and days are rolled up from stored minutes. Only closed periods are
written, so a file never changes after it exists — which is what lets
browsers and CDNs cache them for a year. Gaps the exchange cannot fill are
recorded in `data_gap`.

The file format is described in [format.md](format.md).

## Server

| Route | |
|---|---|
| `GET /api/health` | `{ok, db}` |
| `GET /api/markets` | every market on four venues with 24h stats, cached 30 minutes |
| `GET /api/history` | archived markets and what each base covers |
| `GET POST DELETE /api/drawings?market=&timeframe=` | list, create, clear |
| `DELETE /api/drawings/{id}` | |
| `GET PUT /api/workspaces/{id}` | the layout document; a PUT carries the version it was based on and gets `409` with the current document when another tab saved first |
| `/data/…` | partitions (`immutable`) and manifests (`no-cache`) |

Writes require `Authorization: Bearer` when `TRADOOR_API_TOKEN` is set.

## Database

| Table | |
|---|---|
| `drawing` | hand-drawn markup per market and timeframe; points as `[{time, price}]` |
| `workspace` | the terminal layout, whole, with an optimistic version |
| `tracked_market` | what the recorder keeps current, and from when |
| `partition` | every file on disk: rows, bytes, sha256, first and last open |
| `data_gap` | ranges the exchange could not supply |
| `ingest_run` | recorder and backfill history |

## Workspace

A tree of splits and tab groups, the panels in it, four link groups and theme
overrides, stored as one JSON document: in localStorage so the page opens
instantly and offline, and on the server so it follows the reader. Whichever
copy changed last wins. Panels in a link group share one market and
timeframe; a watchlist that is not linked drives the last chart used.

Panels: chart, watchlist, TPO, and indicator values.

## Why these choices

- **WebGL2 written by hand, not wgpu.** Measured on this renderer: 11 KB gzip
  for the WebGL2 path against 653 KB for wgpu. WebGPU can come later behind
  the same `Frame`.
- **Partitions, not candle tables.** Integer ticks, delta and varint columns,
  gzip: about 16 bytes a bar, served without a query.
- **The browser talks to exchanges directly.** Both venues allow it (CORS), so
  the server never proxies market data and never becomes the bottleneck.
- **One engine for server and browser.** Indicators, folding and profiles are
  the same Rust in both places, so a number on the chart is the number a
  backtest or alert would use.
