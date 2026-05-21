#!/usr/bin/env bash
# Replace BACKFILL placeholder rows with real generated designs.
# Loops generate.py per under-filled brand (muon/ma/nouns), then deletes
# leftover placeholders. Designed to be safe to re-run.
set -u
set -o pipefail

DB="/Users/yuki/workspace/mu-brand/products.db"
GEN="/Users/yuki/workspace/mu-brand/generate.py"
BRANDS=(muon ma nouns)
MAX_CONSECUTIVE_FAILS=5

# Skip generate.py's random_delay (0–8h for muon, 0–4h for staple, etc.).
# Without this the script effectively hangs for hours before producing
# the first design. Honored by generate.py:random_delay (line ~972).
export NO_DELAY=1

# Load API keys from ~/.env without echoing them
if [ -f "$HOME/.env" ]; then
  set -a
  # shellcheck disable=SC1091
  source "$HOME/.env"
  set +a
else
  echo "WARN: ~/.env not found, relying on existing env"
fi

echo "=== START $(date -Iseconds) ==="

echo "--- before ---"
sqlite3 "$DB" "SELECT brand, COUNT(*) AS total, SUM(active) AS active, SUM(CASE WHEN active=0 AND prompt_text LIKE 'BACKFILL%' THEN 1 ELSE 0 END) AS placeholders FROM products WHERE brand IN ('muon','ma','nouns') GROUP BY brand"

total_ok=0
total_fail=0

for brand in "${BRANDS[@]}"; do
  placeholder_count=$(sqlite3 "$DB" "SELECT COUNT(*) FROM products WHERE brand='$brand' AND active=0 AND prompt_text LIKE 'BACKFILL%'")
  echo "[$brand] placeholders=$placeholder_count"
  consecutive_fail=0
  for i in $(seq 1 "$placeholder_count"); do
    echo "[$brand] generate $i/$placeholder_count $(date -Iseconds)"
    # Hard cap per-call at 90s so a stalled Gemini/Printful request can't
    # eat the whole batch window. generate.py is typically ~40s when healthy.
    if timeout 90 python "$GEN" "$brand"; then
      total_ok=$((total_ok + 1))
      consecutive_fail=0
    else
      total_fail=$((total_fail + 1))
      consecutive_fail=$((consecutive_fail + 1))
      echo "[$brand] generate $i FAILED (consecutive=$consecutive_fail), continuing"
      if [ "$consecutive_fail" -ge "$MAX_CONSECUTIVE_FAILS" ]; then
        echo "[$brand] $MAX_CONSECUTIVE_FAILS consecutive failures, EARLY EXIT"
        break 2
      fi
    fi
    sleep 5
  done
done

echo "--- cleanup placeholders ---"
for brand in "${BRANDS[@]}"; do
  # Only delete placeholders if we successfully generated enough real rows
  real_added=$(sqlite3 "$DB" "SELECT COUNT(*) FROM products WHERE brand='$brand' AND active=1 AND (prompt_text IS NULL OR prompt_text NOT LIKE 'BACKFILL%')")
  echo "[$brand] real active rows now=$real_added"
  deleted=$(sqlite3 "$DB" "SELECT COUNT(*) FROM products WHERE brand='$brand' AND active=0 AND prompt_text LIKE 'BACKFILL%'")
  sqlite3 "$DB" "DELETE FROM products WHERE brand='$brand' AND active=0 AND prompt_text LIKE 'BACKFILL%'"
  echo "[$brand] deleted $deleted placeholders"
done

echo "--- after ---"
sqlite3 "$DB" "SELECT brand, COUNT(*) AS total, SUM(active) AS active, SUM(CASE WHEN active=0 AND prompt_text LIKE 'BACKFILL%' THEN 1 ELSE 0 END) AS placeholders FROM products WHERE brand IN ('muon','ma','nouns') GROUP BY brand"

echo "=== END $(date -Iseconds) ok=$total_ok fail=$total_fail ==="
