# Partition format (TDC1)

## Layout on disk

```text
data/{venue}/{SYMBOL}/manifest.json
data/{venue}/{SYMBOL}/{1m|1h|1d}/{YYYY | YYYY-MM | YYYY-MM-DD}.tdc.gz
```

Only closed periods are written. Minutes are kept as month files for closed
months and day files within the current month. Hours and days are kept as year
files for closed years, month files for closed months, and day files within
the current month. When a longer period closes, its file replaces the shorter
ones. A file never changes once written.

## manifest.json

```json
{
  "market": "binance-perp:BTCUSDT",
  "updatedAt": 1789368469410,
  "bases": {
    "1m": [{ "period": "2026-08", "rows": 44640, "bytes": 712345, "firstOpen": 1785542400000, "lastOpen": 1788220740000, "sha256": "…" }]
  }
}
```

Rebuilt whenever a partition is written. Clients read it to pick files; the
server's `partition` table mirrors it.

## A partition

Gzip around this, all integers little-endian:

```text
magic         "TDC1"
interval      u8
price_scale   u8     prices are integers × 10^-price_scale
volume_scale  u8     base volume and taker buy volume
quote_scale   u8     quote volume
flags         u8     1 quote volume · 2 taker buy volume · 4 trade count
count         u32
first_open    i64    epoch milliseconds

steps         uvarint × (count − 1)   intervals between consecutive opens (1 unless there is a gap)
open          ivarint × count         open[i] − close[i−1]; open[0] as is
high          ivarint × count         high − max(open, close)
low           ivarint × count         min(open, close) − low
close         ivarint × count         close − open
volume        uvarint × count
quote volume  uvarint × count         when flagged
taker buy     uvarint × count         when flagged
trades        uvarint × count         when flagged
```

`uvarint` is LEB128; `ivarint` is LEB128 over zigzag. Open times step by the
interval, or by calendar months for `1M`.

Prices are parsed from the exchange's decimal strings exactly, never through
floating point, so a partition round-trips to the same digits the exchange
sent. Columns rather than rows: a column of small, similar deltas is what gzip
compresses best — five years of 1-minute BTCUSDT is 2.65 million bars in 42 MB.

The reference implementation is `crates/codec/src/partition.rs`; the browser
decodes with the same code compiled to WASM.
