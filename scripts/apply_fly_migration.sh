#!/bin/bash
# Run inside Fly machine via:
#   fly ssh sftp shell put scripts/apply_fly_migration.sh /tmp/m.sh
#   fly ssh console -C "bash /tmp/m.sh"
set -e

DB=/data/products.db
SQL=/tmp/migration.sql

echo "[migration] DB size before: $(stat -c%s $DB) bytes"
echo "[migration] backup live DB"
cp $DB "$DB.pre-20260523"

# Install sqlite3 binary if missing
if ! command -v sqlite3 >/dev/null 2>&1; then
  echo "[migration] installing sqlite3…"
  apt-get update -qq >/dev/null
  apt-get install -y -qq sqlite3 >/dev/null
fi

echo "[migration] applying $SQL"
sqlite3 $DB < $SQL

echo "[migration] DB size after: $(stat -c%s $DB) bytes"

echo "[migration] verify brands"
sqlite3 $DB "SELECT slug, name FROM catalog_brands WHERE slug IN ('voice','ocean','lodge','octagon','founder');"

echo "[migration] verify SKUs"
sqlite3 $DB "SELECT brand, COUNT(*) FROM catalog_products WHERE brand IN ('voice','ocean','lodge','octagon','founder') GROUP BY brand;"

echo "[migration] done"
