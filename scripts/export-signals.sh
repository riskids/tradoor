#!/usr/bin/env bash
# Export the Signal panel's record — every pattern, every setup and its touch,
# the scans, and the latest replay — as CSV files in one tar.gz on stdout, for
# weighing the rules away from the server.
#
# On the server, from the directory compose.yml is in:
#   bash export-signals.sh > signals.tar.gz
# Or from a machine that reaches the server over ssh, straight to a local file:
#   ssh user@server 'bash -s' < scripts/export-signals.sh > signals.tar.gz
#
# It reads through the postgres container with the container's own user and
# database names, so no password is asked for or printed. Nothing is changed.
set -euo pipefail

cd "${TRADOOR_DIR:-/var/www/tradoor}"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

# One query's rows as CSV with a header.
dump() {
  docker compose exec -T postgres sh -c 'psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -X -q -v ON_ERROR_STOP=1 -c "$1"' sh "COPY ($2) TO STDOUT WITH (FORMAT csv, HEADER)" > "$out/$1.csv"
  echo "$1: $(($(wc -l < "$out/$1.csv") - 1)) rows" >&2
}

dump signal 'SELECT * FROM signal ORDER BY id'
dump signal_setup 'SELECT * FROM signal_setup ORDER BY seen_bar, market'
dump signal_scan 'SELECT * FROM signal_scan ORDER BY bar'
dump signal_backtest_run 'SELECT * FROM signal_backtest_run ORDER BY at'
dump signal_backtest 'SELECT * FROM signal_backtest ORDER BY run, kind, entry_at, market'
dump signal_touch_backtest 'SELECT * FROM signal_touch_backtest ORDER BY run, touched_at, market'

tar -C "$out" -czf - .
