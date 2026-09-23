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
5. Liquidations follow the ticker, not the venue: the page matches the chart's
   coin and quote against the catalogue on every perpetual exchange — Binance's
   `1000SHIBUSDT` is Bybit's `SHIB1000USDT` — and the worker subscribes to each.
   Every hit joins a feed at the foot of the price pane: the newest lands at the
   bottom, pushes the stack up, and fades out five seconds later, marked with its
   exchange's logo, side and size. The Liquidations panel keeps a tape of the
   same streams for the page — switching tabs keeps it, a reload does not — with
   each exchange's connection beside it.
6. For a few tickers (`TRADOOR_LIQUIDATIONS`, BTCUSDT, ETHUSDT and SOLUSDT by
   default) the server holds its own socket to every perpetual exchange, writes
   each hit to the `liquidation` table in one-second batches and deletes rows
   older than 24 hours every minute. `GET /api/liquidations/{ticker}` sums the
   last 1H, 4H, 12H and 24H across exchanges and returns the latest hits, which
   seed the panel's tape. A day of three tickers is at most a few hundred
   thousand rows, on a crash day.
7. `ChartEngine` (tradoor-render) turns candles, studies, drawings and the view
   into a `Frame`: one vertex buffer of triangles in draw order with clip
   batches, and a list of text commands. The worker uploads it in two buffer
   writes and replays the text on the 2D canvas. Nothing is drawn unless
   something changed or the countdown needs its next second.
8. Panning near the oldest bar held emits `needHistory`; the series loads the
   next older partition, or pages REST back, and every chart on it refolds.

Series nobody reads are dropped after two minutes, so a deep 1-minute history
does not outlive its chart.

Every chart and watchlist row shares one socket per venue, each topic subscribed
once and reference counted. A socket that carries only liquidations is live as
soon as the exchange answers it: those topics are silent until someone is closed
out, and treating that silence as a stall would reconnect it forever. Bybit refuses a whole subscribe frame when one topic
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
| `GET /api/markets` | every market on twelve venues (Binance, Bybit, OKX, Bitget, Hyperliquid, Kraken, Lighter) with 24h stats and each exchange's own name for it, cached 30 minutes |
| `GET /api/history` | archived markets and what each base covers |
| `GET POST DELETE /api/drawings?market=&timeframe=` | list, create, clear — every timeframe of the market; `timeframe` records where it was drawn |
| `PATCH DELETE /api/drawings/{id}` | move, relabel or restyle one drawing; remove it |
| `GET /api/liquidations/{ticker}/live` | every exchange's liquidations of a followed ticker as they happen, as server-sent events |
| `GET /api/open-interest?coin=&step=&from=&venues=` | aggregated open interest as candles of `step`, in coins; asking for a coin starts recording it |
| `GET PUT /api/workspaces/{id}` | the layout document; a PUT carries the version it was based on and gets `409` with the current document when another tab saved first |
| `/data/…` | partitions (`immutable`) and manifests (`no-cache`) |

Drawings and workspaces are scoped by the `x-tradoor-owner` header, the browser's own random id: one browser cannot read or change another's.

## Database

| Table | |
|---|---|
| `drawing` | hand-drawn markup per browser id (`x-tradoor-owner`) and market, shown on every timeframe; points as `[{time, price}]`, `style` a flat object of settings |
| `open_interest` | each venue's open interest in coins, a row a minute (backfilled at five minutes where the exchange keeps history), kept 35 days |
| `workspace` | the terminal layout per browser id — panels, watchlists, indicators, theme — whole, with an optimistic version |
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

Panels: chart, watchlist, indicator values and liquidations. TPO is a way of
drawing a chart rather than a panel of its own: each candle of the chosen
timeframe becomes a profile. The chart loads a smaller timeframe for its
brackets — 1m under 15m, 5m under 4h, 30m under a day or a week — and the engine
stacks them into their candle's profile, pushed left against the candle's open
on the same time and price axes, so drawings, zoom and the crosshair work on it
as on candles. Rows are one grid across profiles; blocks share one width, the
widest that keeps the fullest profile inside its own candle, so profiles
compare by size.

TPO options follow Exocharts' settings — Main, Colors, POC & VA, IB & Stats,
Single prints & Tails — under its names and defaults. A chart stores only what
differs from the defaults, and the engine is sent them as `key=value;…`.
Everything a profile is read by is built once per bar change in
`tradoor-render::tpo`: rows, volume per row (a bracket's volume spread over the
rows it spans), value area by time or volume, initial balance over the first N
brackets, single prints and tails as runs of one-bracket rows, and for each level
the first later bar that traded back to it, which is where a naked level ends.
Delta colouring and overlays are not offered: candles carry no taker side.

Times are written in the reader's zone — their device's by default, or UTC or any
named zone from Settings, kept with the workspace. The page resolves the zone to
an offset (again every minute, for daylight saving) and the engine shifts
intraday axis and crosshair labels by it; daily and longer bars keep their
exchange's UTC dates, as a daily candle is a UTC day.

Some networks block the exchanges' names — Indonesian ISPs and carriers answer
Binance and Bybit with a block page — and a browser there reaches neither their
REST calls nor their streams. It falls back to the tradoor server: a REST call
that fails at the network level is retried through `/api/relay/rest` (klines and
pings on allow-listed hosts and paths only), and each stream's address list ends
with `/api/relay/ws?venue=…`, which the hub moves to when the exchange's own
sockets fail before a word. The server looks exchange names up over HTTPS at
1.1.1.1 and 8.8.8.8, by address, so its own resolver cannot be lied to either;
the liquidation collector and the market catalogue use the same lookups.

Shapes are drawn with WebGL2 in the worker where the browser allows it there
(iOS 17 and up, every current desktop browser) and with the worker's 2D canvas
where it does not: a frame is single-coloured triangles, so runs of one colour
fill as one path. `?renderer=2d` forces that path in development.

On a phone (under 760px wide) the workspace is shown as one scrolling column
rather than splits: the chart in use first, as tall as the screen less a peek of
the next panel, then every other panel in layout order. The saved layout is not
changed, so a wider window shows the splits again.

A chart panel's canvases and engine slot live as long as the panel, not as long
as its tab is shown: a browser allows a page about sixteen WebGL contexts and
frees a dropped one lazily, so a context per mount blanks charts after a few tab
switches. A hidden chart is not drawn but keeps its bars and streams, and shows
again at once.

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
