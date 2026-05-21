#!/usr/bin/env python3
"""
backfill_brand_minimum.py
==========================
Ensure each core MU brand (mugen / muon / ma / nouns) has at least N SKUs in
products.db. Missing rows are inserted as **inactive placeholders** (active=0)
so they remain invisible on wearmu store until generate.py later fills in real
designs / mockups.

Usage:
    python scripts/backfill_brand_minimum.py                # dry-run
    python scripts/backfill_brand_minimum.py --target 36    # custom floor
    python scripts/backfill_brand_minimum.py --commit       # actually INSERT

Design notes
------------
- INSERT only. We never UPDATE existing rows.
- design_url / mockup_url stay NULL — generate.py is expected to fill them.
- drop_num starts at MAX(drop_num WHERE brand=?) + 1 and counts up.
- prompt_text marks the row as a backfill placeholder for easy grep.
- price_jpy = 4900, inventory = 0, active = 0.
- We do NOT touch store/products.db (prod mirror, synced separately).
"""
from __future__ import annotations

import argparse
import datetime as _dt
import os
import sqlite3
import sys
import uuid
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DB_PATH = REPO_ROOT / "products.db"

CORE_BRANDS = ("mugen", "muon", "ma", "nouns")
DEFAULT_TARGET = 36

PLACEHOLDER_PROMPT = "BACKFILL placeholder — to be replaced by generate.py"


def compute_gaps(target: int = DEFAULT_TARGET, db_path: Path = DB_PATH) -> dict[str, int]:
    """Return {brand: missing_count} for CORE_BRANDS where count < target."""
    gaps: dict[str, int] = {}
    with sqlite3.connect(db_path) as con:
        cur = con.cursor()
        for brand in CORE_BRANDS:
            cur.execute("SELECT COUNT(*) FROM products WHERE brand = ?", (brand,))
            (count,) = cur.fetchone()
            missing = max(0, target - count)
            gaps[brand] = missing
    return gaps


def _next_drop_start(cur: sqlite3.Cursor, brand: str) -> int:
    cur.execute("SELECT COALESCE(MAX(drop_num), 0) FROM products WHERE brand = ?", (brand,))
    (max_drop,) = cur.fetchone()
    return int(max_drop) + 1


def seed_placeholder(brand: str, count: int, db_path: Path = DB_PATH) -> int:
    """Insert `count` inactive placeholder rows for `brand`. Returns rows inserted."""
    if count <= 0:
        return 0

    now_iso = _dt.datetime.utcnow().replace(microsecond=0).isoformat() + "Z"
    inserted = 0
    con = sqlite3.connect(db_path)
    try:
        cur = con.cursor()
        start_drop = _next_drop_start(cur, brand)
        rows = []
        for i in range(count):
            drop_num = start_drop + i
            name = f"{brand.upper()} #{drop_num:03d} (placeholder)"
            prompt_hash = uuid.uuid4().hex
            rows.append(
                (
                    brand,
                    drop_num,
                    name,
                    4900,                  # price_jpy
                    0,                     # inventory
                    0,                     # sold
                    now_iso,               # created_at
                    0,                     # active
                    PLACEHOLDER_PROMPT,    # prompt_text
                    prompt_hash,           # prompt_hash
                    "teshikaga",           # city_slug
                    "BLK",                 # color
                    "M",                   # size
                )
            )
        cur.executemany(
            """
            INSERT INTO products (
                brand, drop_num, name,
                price_jpy, inventory, sold,
                created_at, active,
                prompt_text, prompt_hash,
                city_slug, color, size
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            rows,
        )
        inserted = cur.rowcount if cur.rowcount and cur.rowcount > 0 else len(rows)
        con.commit()
    except Exception:
        con.rollback()
        raise
    finally:
        con.close()
    return inserted


def _report_brand_counts(db_path: Path = DB_PATH) -> list[tuple[str, int, int]]:
    """Return [(brand, total, active_count)] for CORE_BRANDS."""
    out: list[tuple[str, int, int]] = []
    with sqlite3.connect(db_path) as con:
        cur = con.cursor()
        for brand in CORE_BRANDS:
            cur.execute(
                "SELECT COUNT(*), COALESCE(SUM(CASE WHEN active=1 THEN 1 ELSE 0 END), 0) "
                "FROM products WHERE brand = ?",
                (brand,),
            )
            total, active = cur.fetchone()
            out.append((brand, int(total), int(active)))
    return out


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    parser.add_argument("--target", type=int, default=DEFAULT_TARGET,
                        help=f"SKU floor per brand (default: {DEFAULT_TARGET})")
    parser.add_argument("--commit", action="store_true",
                        help="Actually INSERT rows. Without this, dry-run only.")
    parser.add_argument("--db", type=Path, default=DB_PATH,
                        help=f"Path to products.db (default: {DB_PATH})")
    args = parser.parse_args(argv)

    if not args.db.exists():
        print(f"ERROR: db not found at {args.db}", file=sys.stderr)
        return 2

    gaps = compute_gaps(args.target, args.db)
    print(f"== backfill_brand_minimum (target={args.target}, db={args.db}) ==")
    print("compute_gaps:")
    for brand, missing in gaps.items():
        print(f"  {brand}: missing {missing}")

    total_missing = sum(gaps.values())
    if total_missing == 0:
        print("All brands meet the floor. Nothing to do.")
        return 0

    if not args.commit:
        print("\n[DRY-RUN] No rows inserted. Re-run with --commit to apply.")
        for brand, missing in gaps.items():
            if missing > 0:
                print(f"  would insert {missing} rows for brand={brand}")
        return 0

    print("\n[COMMIT] Inserting placeholder rows...")
    inserted_total = 0
    for brand, missing in gaps.items():
        if missing <= 0:
            continue
        n = seed_placeholder(brand, missing, args.db)
        inserted_total += n
        print(f"  inserted {n} rows for brand={brand}")

    print(f"\nTotal inserted: {inserted_total}")
    print("\nPost-backfill brand counts (total / active):")
    for brand, total, active in _report_brand_counts(args.db):
        marker = "OK" if total >= args.target else "LOW"
        print(f"  [{marker}] {brand}: total={total} active={active}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
