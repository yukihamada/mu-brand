#!/bin/zsh
# Snapshot store/products.db to data/backups/, keep last 48 (≈2 days hourly).
# Intended for cron: 0 * * * * cd /Users/yuki/workspace/mu-brand && scripts/backup_db.sh
set -e
cd "$(dirname "$0")/.."

DB="store/products.db"
DIR="data/backups"
KEEP=48

mkdir -p "$DIR"
TS=$(date +%Y%m%d_%H%M%S)
OUT="$DIR/products_${TS}.db"

# atomic copy via sqlite .backup (safer than cp while WAL is hot)
if command -v sqlite3 >/dev/null 2>&1; then
  sqlite3 "$DB" ".backup '$OUT'"
else
  cp "$DB" "$OUT"
fi

# prune to last KEEP
ls -1t "$DIR"/products_*.db 2>/dev/null | tail -n +$((KEEP+1)) | xargs -r rm -f

# log so we can see it ran
echo "[$(date '+%Y-%m-%d %H:%M:%S')] backup → $OUT ($(stat -f%z "$OUT" 2>/dev/null || stat -c%s "$OUT") bytes, $(ls "$DIR" | wc -l | tr -d ' ') kept)" >> logs/backup_db.log
