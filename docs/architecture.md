# Architecture

## Threads

```text
page (SolidJS)                         engine worker (WASM)                    network
─────────────────────────────────────  ──────────────────────────────────────  ─────────────────────────
layout, toolbars, legend, dialogs  ──► ChartEngine per chart                   /data partitions (CDN)
pointer and wheel input            ──►   tradoor-render model → Frame        ◄─ exchange REST (klines)
                                   ◄──   WebGL2 triangles + 2D text           ◄─ exchange WS (kline, trades)
legend, cursor, events, status     ◄──  history series, folding, live bars
watchlist days     ◄─ exchange WS                                             /api (tradoor-server)
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
   stored one. An exchange series pages back through the REST API instead,
   asking for no more than a screenful first (600 bars) and the rest behind it,
   a chunk at a time with a pause between, each queued behind whatever was
   asked meanwhile — another chart's first screen waits on one chunk, not on
   the whole history. Requests to Bitget are spaced 70 ms apart, under its
   twenty a second. A series asked for again takes in the bars that closed
   since it was loaded before it is drawn. A market is fetched before it is
   chosen: a watchlist row under the pointer, or the result highlighted in the
   symbol search.

   Bitget's stock tokens ("rMU", "rSPY" and the like, some with the stock's
   history back to 2011) refuse its UTC day, week and month, code 48001, and
   their days at UTC+8 start mid-session. The first time a Bitget market is
   opened, one bar of its UTC day is asked for (`probeUtc`); refused, its 6h,
   12h, D, W and M are read and streamed as 1h or 4h bars and folded on UTC
   boundaries (`marketInterval`), at most 12,000 of them behind a chart. Their
   candles stop between trading hours, so their stream is quiet: Bitget taking
   the subscription is enough for it to be live.
3. `fold_candles` builds the chart timeframe from the base bars — 4h from 1h, a
   Monday-aligned week from days — and drops a bucket that has not closed.
4. The kline stream (plus aggregate trades on Binance) feeds `LiveFeedEngine`,
   which never lets a stale kline rewind trades already folded. When the bar
   rolls over, the finished bar is kept as closed until REST confirms it. The
   stream is asked for beside the history rather than after it, and the forming
   bar is first read from REST (`formingRows`) — a quiet market's stream can say
   nothing for seconds, and the chart showed the last closed bar and its close
   till then. That bar has no trade id, so the stream's first kline replaces it.
   Both wait for the closed bars to be drawn, the chart on screen till then
   being the last market's.
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
| `GET /api/flow?coin=` · `GET /api/flow/minutes?market=` · `PUT /api/flow/{market}` | one coin's flow on every market it is followed on, the coins followed, and this reader's crossings; the minutes a chart marks; the threshold and whether to be told |
| `PUT /api/flow/coins/{coin}` · `DELETE /api/flow/coins/{coin}` | an admin's (`TRADOOR_ADMINS`): follow a coin's flow for the next day, week (`?for=week`) or until stopped (`?for=always`), or stop |
| `GET /api/signals` · `PUT /api/signals/notify` · `GET /api/signals/stats` | what the latest screener scan found, with when it ran and runs next; whether this reader is told on Telegram when a pattern turns; the record — the history replayed, the live patterns since, and the latest of them |
| `GET /api/open-interest?coin=&step=&from=&venues=` | aggregated open interest as candles of `step`, in coins; asking for a coin any exchange lists a perpetual on starts recording it |
| `GET POST DELETE /api/alerts` | this browser's price alerts and their log; create; delete the stopped, active or all |
| `GET PATCH /api/telegram` · `POST /api/telegram/link` · `POST /api/telegram/test` · `DELETE /api/telegram/{chat}` | the bot's state and this browser's chats; the reader's zone and how long messages stay; a link code; a test message; unlink |
| `PATCH DELETE /api/alerts/{id}` | edit, restart or record a trigger (which stops it and logs it); delete |
| `POST /api/alerts/restart` · `DELETE /api/alerts/log` | restart every stopped alert; clear the log |
| `GET PUT /api/workspaces/{id}` | the layout document; a PUT carries the version it was based on and gets `409` with the current document when another tab saved first |
| `/data/…` | partitions (`immutable`) and manifests (`no-cache`) |

Drawings and workspaces are scoped by the `x-tradoor-owner` header, the browser's own random id: one browser cannot read or change another's.

## Database

| Table | |
|---|---|
| `drawing` | hand-drawn markup per browser id (`x-tradoor-owner`) and market, shown on every timeframe; points as `[{time, price}]`, `style` a flat object of settings |
| `open_interest` | each venue's open interest in coins, a row a minute (backfilled at five minutes where the exchange keeps history), kept 35 days |
| `alert`, `alert_log` | price alerts per browser id, and each trigger (the last 500); the server follows the active ones' markets and fires them |
| `signal_scan`, `signal`, `signal_setup`, `signal_watch`, `signal_backtest_run`, `signal_backtest`, `signal_touch_backtest` | each screener scan, every pattern found (kept with the last scan that saw it, for ninety days, with how it played out), who is told when one turns, and the latest replay of the rules over the history |
| `flow_minute`, `flow_watch`, `flow_event`, `flow_coin` | net taker flow by the minute for two days, what each reader asks of each market, the crossings they were told about, and the coins followed besides BTC with when each lapses, none for one followed until stopped |
| `telegram_chat`, `telegram_link` | chats an alert is sent to, per browser id, and the one-use codes that link them |
| `account`, `account_signin` | a Telegram user and the browser id their things are kept under, and the sign-ins waiting on their yes |
| `telegram_message` | what the bot sent and when to take it back |
| `workspace` | the terminal layout per browser id — panels, watchlists, indicators, theme, favourite timeframes — whole, with an optimistic version |
| `tracked_market` | what the recorder keeps current, and from when |
| `partition` | every file on disk: rows, bytes, sha256, first and last open |
| `data_gap` | ranges the exchange could not supply |
| `ingest_run` | recorder and backfill history |

## Workspace

A tree of splits and tab groups, the panels in it, four link groups and theme
overrides, stored as one JSON document: in localStorage so the page opens
instantly and offline, and on the server so it follows the reader. Whichever
copy changed last wins. The local copy is stamped with the browser id that
saved it: taking another browser's id (Settings) asks for their layout, so a
copy stamped for someone else is not opened with — it would show the wrong
workspace and, being the newer of the two, save it over the one asked for. Panels in a link group share one market and
timeframe; a watchlist that is not linked drives the last chart used. The
timeframes starred in a chart's timeframe menu are the buttons on every chart's
bar (`WorkspaceDoc.timeframes`, kept with the layout).

Panels: chart, watchlist, values, liquidations, flow, signal and alerts.

Flow is the net taker volume of a market — what the buyers took across the
spread less what the sellers did — kept minute by minute for two days
(`flow`, `flow_minute`). Spot and perpetual are counted apart, since the same
number means something different on each, and each carries its own threshold:
a minute that runs harder than it is a crossing, told to the reader here and on
Telegram, and marked on the candles where it traded. Eleven markets are read —
Binance, Bybit, OKX, Bitget, Hyperliquid and Kraken, spot and perpetual — each
in the exchange's own name for it, and each size in whatever that exchange
counts in: an OKX swap trades in contracts of a hundredth of a coin, so a print
of fifty is half a coin and not fifty of them. The chart's marks are the
Flow study, added from the indicator list like any other and hidden with its
eye: the minutes themselves, so they show at any timeframe, and hovering one
names the money, the minute and the price. After a crossing a market stays quiet until
its flow falls back to half the threshold, so one push does not ring over.

BTC is always followed. An admin — a browser id listed in `TRADOOR_ADMINS` —
adds another coin from the panel's `+`, and the server follows it on each of
those exchanges that lists it, quoted as BTC is there, for a day or a week from
when it was last asked for, or until it is stopped (`flow_coin.ends_at` empty);
Keep 24h, 7d or Always asks again, Stop or the chip's × ends it, and at the end
of its time it lapses and its streams close. Its threshold until the reader sets one is
three times its usual minute over the last day. Anyone can read the coins being
followed, and drag the chips into the order they like (kept with the panel);
only an admin chooses them, at most ten at once.

Signal is the server's screener (`signals`, `drives`). A minute after each
four-hour close it reads the last five hundred closed bars of the two hundred
coins with the most turnover on Binance's and Bybit's USDT perpetuals and
looks for three drives either way: three swing highs (a pivot of five bars
before and two after), each clearing the last by half an ATR, the first a fresh
high over as long a stretch as the drives are apart; the third within an ATR of
the line through the first two, nothing between them poking more than half an
ATR through it, and a real pullback between each — two ATRs, and at least 38%
of the push before. It turns when Stochastic RSI (14 14 3 3, the chart's own)
crosses down having been over 80 in the three bars before; a pattern waits two
days for that and stays news two days after, and a close past the third drive
ends it — a wick past it is a retest. So does price reaching the first
take-profit (38.2% back down the third drive's leg) by the bar a scan first
sees the turn on: the move is made, with nothing left to enter it on. Each scan
says why the patterns it no longer sees went (`signal.ended_why`), for the
list's Ended rows. A scan takes the oldest pattern still
standing among the recent drives, so a later high does not take its place. The bullish search is the same run on the bars mirrored. The
score adds how well the third drive sits on the line, RSI weaker at it than at
the second, even spacing, volume fading and the turn. A row opens its market at
four hours with the line and the drives' numbered circles drawn — locked, and
gone when closed or the market changes, unless Keep drawings saves them as the
chart's own (`circle` is a drawing kind for that). Telegram, for readers who
turn it on, hears once per scan of the patterns scoring 70 or more that
turned in it; the panel lists them all.

The same search runs on the day's and the week's bars (`signals::Frame`, kept
in `signal.timeframe`), the strong kind: the list puts them first, starred, and
a four-hour row says +1D or +1W where the same coin has one the same way. Their
rules are a little looser (`drives::Rules::LONGER`) — a pullback of 30% of the
push before is enough, and the bar either side of a drive may poke through the
line, a drive on long bars often being two of them — and untested by the
replay, which runs the four hours' alone. Their trades and limits are weighed on
their own bars and kept apart in the Record (`higher` in the stats); setups and
AT LINE are the four hours' only. Telegram hears of each that turns whatever its
score, while the turn is new — within two of its bars.

Every message of a pattern or a touch links to it (`TRADOOR_SITE_URL`):
`?signal=<id>&market=…&tf=…` opens the pattern on its chart, at its length and
drawn, `?market=…&tf=4h&touched=<ms>` a touch (`web/src/lib/deeplink.ts`). A
page left open reads the list every minute and tells of what is new there — a
four-hour pattern that turned, a day's or week's from when it appears, price at
a line — as a notice that opens it, and as the system's own notification while
the tab is in the background (`web/src/lib/signal-alerts.ts`). What a browser
was told is kept in it, so a later visit tells what came meanwhile, once; its
first visit only learns what is there.

Where the exchange says who took each trade — Binance's klines carry the
taker buy volume, Bybit's do not — the delta at the drives is read too: the
share of the volume buyers took on the push into the third drive (from the low
of the pullback before it) against the push into the second, a smaller share
being delta divergence (ΔDIV in the list), and whether the third drive's own
bar was sold more than bought. Neither changes the score; the record counts
them as cuts of its own, beside RSI divergence and the score bands. The list
shows, in place of the drives' prices, each push's delta in dollars — bought
less sold, each bar's coins at its average price (quote volume over volume) —
the first push from the low of as many bars before it as the first two drives
are apart (`signal.deltas`, `signal_setup.deltas`). A setup keeps the pushes
into its two drives, and on reaching the line one more read off the bars so
far, the forming one with them. ΔDIV compares shares, not dollars: a push with
more volume can carry more dollars and still a smaller share.

Between scans the third drive is caught as it happens. Each scan also keeps,
for every coin and side without a pattern, the newest two drives that wait for
a third — the pullback after the second made, the line not yet reached
(`drives::setup`, table `signal_setup`). Every fifteen seconds live prices —
Binance's perpetuals all in one ask, Bybit's one by one — are weighed against
those lines: price past the second drive by half an ATR and within an ATR of
the line marks the setup AT LINE, shown at the top of the list and told at
once to readers who turned that on (a second Telegram switch). It stays there
while its trade is open; once a scan has weighed the trade out, it moves under
Ended with what it reached. The bars still
have to close and Stochastic RSI to turn for it to be a pattern.

A touch has a record too: the same trade entered at the price that reached
the line, which is the third drive's extreme so far — the take-profits and the
stop both measured from it — and marked with what the scans of the next four
days made of its two drives: three drives that turned, three drives, or none. The
replay catches touches the way the live check does, the setup the scan before
had kept and the bar after reaching the line's near edge, and the scan and
the replay share the setup code (`Prepared::setup_at`).

The record says how they went, each signal as a trade under the live plan
(`drives::LIVE`, plan C below): take-profits 38.2, 61.8 and 100% back down the
third drive's leg, a stop an ATR past the third drive's extreme, half the
position closed at the first and the stop then moved to the entry, a quarter
at each of the others, and what is still open after four days closed at that
close. A trade's prices are kept with it when it is entered. A bar reaching the stop and a
take-profit both counts as the stop. A pattern that turned is entered at the
close the scan read; each scan after weighs the bars since. The list and the
Telegram messages carry each trade's prices, and opening one draws them. A
take-profit counts only when price reaches it after the entry — one already
past at the entry is closed there for nothing — and Win is a trade closed in
profit.

Each pattern also has a limit entry (`drives::limit_entry`): set by the scan
that first sees it, half way back from the furthest price since the third
drive toward it, waiting two days, and off if price reaches the first
take-profit first. It fills at that scan's close if price is already past it;
filled within a bar, that bar takes no take-profit, not knowing the order.
Each scan works it out again from the bars until it is decided (`signal.limit_*`);
the list shows it under the row and opening a pattern draws it, and Telegram,
for readers who turn it on, hears of those set for bearish patterns scoring 55
or more before the turn with delta divergence. The live record of limits
counts the patterns first seen from the bar they began (`LIMIT_SINCE`).

The replay also runs every signal under four plans side by side
(`drives::Plan`) — A 3/6/9% with the stop 3.5% past the third drive; B take-profits at 38.2, 61.8 and 100% back
down the third drive's leg and the stop at its 1.272 extension; C the same
take-profits and the stop an ATR past the third drive, the live one; D 3/6/9% with the stop
3.5% from the entry — and keeps each plan's tallies with the run, by kind,
side and group and by half of the period, for the Record's comparison. The same rules are replayed weekly, and whenever `VERSION` in `signals`
changes, over the last thousand bars (some 166 days) of every coin: as of each
bar, seeing nothing after it: the replay runs the live scan at every bar. The panel's Record
shows both by side, score and divergence, with the latest live patterns, and
the limit entries replayed and live — their score bands fifteen lower, as they
are set before the turn adds its fifteen. A restart under a new `VERSION` scans
the bar again.

Footprint is the same prints kept by price rather than summed: a trade lands
in the level its price falls in, and a minute is a column of levels
(`footprint_minute`, `/api/footprint?market&from&to`). Eight markets are
recorded — BTC and ETH, spot and perpetual, on Binance and Bybit — at the
finest level a reader would want, a dollar on BTC and a tenth on ETH, since
coarser groupings are made by adding levels together and cannot be undone. A
month is kept. The levels of a minute are packed into one blob: the lowest
level, then each level as the gap from the one before it and the two volumes,
as varints of thousandths of a coin. A busy minute of BTC packs into some 130
bytes where a row per level would be forty rows and ten times the bytes.

The chart folds those minutes into its own bars and groups them onto ONE price
grid for the screen, since a row that does not line up with its neighbour has
nothing to compare against. A level reads as Split (the two sides apart),
Delta, Total, Cluster (a bar as long as the level was busy) or Ladder (the
delta added up from the foot of the bar); the candle stays underneath. A side
that took two and a half times what the other side took one level away — the
diagonal, since a buyer lifts the offer where a seller hits the bid beneath it
— is ringed, and three in a row are marked as one run. Each bar outlines the
rows holding seventy per cent of its volume and rings the one that traded most;
its volume and delta are written under it, and every visible bar summed by
price is drawn against the right edge.

Price alerts are rows per browser id, like drawings (`/api/alerts`); the
browser watches the prices, each alert market's daily candle stream, and fires
them — Crossing, Crossing Up or Down, Greater or Less Than — with a sound, a
notice in the page and, when the tab is out of sight, a system notification.
One tab watches, the one holding the `tradoor-alerts` Web Lock; the rest follow
over a BroadcastChannel and read the list again every half minute. The server
watches as well (`alert_watch`): every market with an active alert is followed
on its exchange's public trade stream, so an alert fires with no browser open
at all — on Binance, Bybit, OKX and Hyperliquid, which between them carry the
markets alerts are set on; elsewhere the page is still the only watcher. OKX
and Hyperliquid name their markets their own way, so the catalogue's `native`
translates both the asking and the answering (`trades`). Both sides record a trigger through the same two statements and the
alert must still be active for them to take, so it fires once however many
watchers see it. The chart draws each active one as a level to drag or delete:
with a mouse a click picks it — its ring fills — and only then does a drag
move it, so a press meant for the chart cannot shift it; a finger holds it
instead. Its label is written in the line's own colour.

Signing in with Telegram (`account`) is how a browser gets its id back —
history cleared, another device — with every row still keyed by that id.
Settings mints a one-use code and opens `t.me/<bot>?start=in_<code>`; the bot
asks the Telegram user who opened it, in their private chat, whether it was
them, with a Yes and a No button (so a link someone else sent cannot sign
their browser in as you), and on yes the code holds the account's id until the
browser that minted it collects it, once (`/api/account/signin/{code}`). A
user's first sign-in makes the asking browser's id their account's, and their
alerts then go to that chat. Chats linked before accounts existed became
accounts, the linked id with the most drawings standing for each person.
Signing out gives the browser a fresh id; the account keeps everything.

An alert that fires is also sent to Telegram, where the reader has linked a
chat (`telegram`, `TRADOOR_TELEGRAM_TOKEN`). A bot cannot open a conversation,
so Settings opens `t.me/<bot>?start=<code>` with a one-use code that says which
browser id the chat belongs to; `/stop` in the chat ends it. Updates are long
polled, so the server needs nothing reachable from the internet, and messages
go out one at a time to stay inside Telegram's per-chat rate. Each carries the
level, the price that met it and the time in the reader's own zone, with a
green or red circle for the way it came — a chat message carries no colour of
its own, where the page draws the same thing as a green or red arrow. Messages are taken back
after two days by default (Settings can change it, or keep them): Telegram
allows that for 48 hours after sending and not a minute longer. The watchlist streams each
market's daily candle, so its change and volume run from 00:00 UTC as
TradingView's do, not over the rolling 24 hours an exchange ticker reports. It
is one ordered list in which `#Name` entries head sections; a section folds
shut and drags with its markets, and markets carry colour flags. Values reads
one market at a glance: its day against the 14-day ATR, returns from 7 days to
a year, RSI, StochRSI and EMA trend on 15m to 1W, the prior day's, week's and
month's highs and lows with the prior session's value area, sorted around
price, and for a perpetual the server's open interest and liquidation totals. TPO is a way of
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
sockets fail before a word. A carrier may swallow a call rather than refuse it,
so a socket to an exchange not open in five seconds, or a request not answering in
seven, counts as failed; the relay, with nothing after it, is given twenty. What failed is remembered for five minutes by `engine/reach.ts`,
shared between the page and the worker over a BroadcastChannel and kept by the
page past a reload, so on such a network only the first call waits. When the page
comes back into view or online, each socket is asked to prove itself and one that
stays silent six seconds is replaced. The server looks exchange names up over HTTPS at
1.1.1.1 and 8.8.8.8, by address, so its own resolver cannot be lied to either;
the liquidation collector and the market catalogue use the same lookups.

Shapes are drawn with WebGL2 in the worker where the browser allows it there
(iOS 17 and up, every current desktop browser) and with the worker's 2D canvas
where it does not: a frame is single-coloured triangles, so runs of one colour
fill as one path. `?renderer=2d` forces that path in development.

iOS takes a page's WebGL contexts back when it is put away or short of memory,
which leaves the chart's shapes blank, its labels still there. The worker asks
for the context back (`preventDefault` on `webglcontextlost`) and, once it is
restored, the engine makes its program and buffers again and draws
(`Gl::restore`); a context not back within three seconds says so in the status
line. `__tradoorSend({ t: 'loseGpu', id: 'chart-1', ms: 1500 })` in a
development console loses one and gives it back.

The worker is built as one file, its lazy imports inlined (`vite.config.ts`).
A chunk split off it imports back from the worker's own file, and Safari runs
that file a second time in the same worker on the first such import — the
loaders of the exchanges past Binance and Bybit, or a liquidation feed reading
one: the copy took the page's messages with no charts, while the first kept
drawing what it had, a chart that no longer answered a touch or a market.
`?debug` shows, under every chart, what its engine holds, and which worker
answers a ping, since when, and with how many charts.

A chart is told its market by an effect, and an error anywhere in the same
update ends the rest of that update — on an iPhone a chart kept the last
market's bars under the new one's name. The engine's status names the market
it holds, so every two seconds a chart that does not follow is told again; and
an error nobody caught shows as a notice, a phone having no console to read it in.

A chart's market may be a ratio, `venue:A/venue:B` — XPL on Bybit priced in BTC
on Binance — offered by the symbol search when a query has a stroke between two
markets (TradingView's `BYBIT:XPLUSDT.P/BINANCE:BTCUSDT` works too). The worker
loads `engine/ratio.ts` only then: each side is its own market's series, folded
to the chart's bars and divided bar by bar, and the forming bar is the two
streams divided once both stand on the same bar. No exchange ticks a ratio, so
its decimals come from its own moves, up to ten. Liquidations, flow marks and
alerts stay off it; its drawings are kept under its own name.

On a phone (under 760px wide) the workspace is one chart and nothing else,
the shape every phone charting app has: the chart in use fills the screen, its
own toolbar is the top of the app and carries the menu, and the panels a wide
window keeps beside it open over the chart from that menu, one at a time. A
panel the layout never had is added as a tab beside the chart the first time it
is asked for. Choosing a market in the watchlist closes the sheet, since that is
what it was opened for. Two fingers on the chart zoom time about the point
between them and carry the chart with it; the engine hears a pinch in the
wheel's own language, since it knows nothing of fingers. The saved layout is not changed, so a wider window shows
the splits again.

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
