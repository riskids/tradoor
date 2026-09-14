#!/usr/bin/env bash
# Export rsalianto's trading.drawing rows as JSON lines for
# `tradoor-server import-drawings`.
#
#   RSALIANTO_DATABASE_URL=postgres://… scripts/export-rsalianto-drawings.sh > drawings.jsonl
#   tradoor-server import-drawings drawings.jsonl
#
# Drawings on a catalogue market keep their market_ref. Drawings on one of the
# old screener's instruments were Binance spot; those on stocks (IDX, US) have
# no market here, export with a null market, and are skipped by the import.
set -euo pipefail

url="${RSALIANTO_DATABASE_URL:?set RSALIANTO_DATABASE_URL to the rsalianto database}"
# psql rejects Prisma's ?schema= parameter.
url="${url%%\?*}"

psql "$url" -X -A -t -q -v ON_ERROR_STOP=1 <<'SQL'
SELECT json_build_object(
  'market', COALESCE(d.market_ref, CASE WHEN i.provider = 'binance' THEN 'binance-spot:' || i.symbol END),
  'timeframe', d.timeframe,
  'kind', d.kind,
  'points', d.points,
  'style', d.style,
  'label', d.label
)
FROM trading.drawing d
LEFT JOIN trading.instrument i ON i.id = d.instrument_id
ORDER BY d.id;
SQL
