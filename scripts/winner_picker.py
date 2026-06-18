#!/usr/bin/env python3
"""
winner_picker — surface "proven sellers" from products.db per brand.

Used by generate.py to steer fresh designs toward the visual family that
buyers and bidders are actually rewarding. Scoring is intentionally simple
and explainable:

    score = sold * 10 + bid_count * 3 + COALESCE(current_bid, 0) / 1000

so 1 sale ≈ 3 bids ≈ ¥10,000 of standing bid. Cold start (no rows scored
> 0) returns whatever the DB has — generate.py treats an empty list as
"no direction available" and falls back to pure-random prompts.

Standalone CLI:
    python scripts/winner_picker.py mugen 5
"""
from __future__ import annotations

import json
import os
import sqlite3
import sys
from pathlib import Path
from typing import Any


DEFAULT_DB = Path(__file__).resolve().parent.parent / "products.db"


def _db_path() -> Path:
    override = os.environ.get("MU_DB")
    if override:
        p = Path(override)
        if not p.is_absolute():
            p = Path(__file__).resolve().parent.parent / p
        return p
    return DEFAULT_DB


def pick_winners(brand: str, top_n: int = 5) -> list[dict[str, Any]]:
    """Return up to `top_n` highest-scoring active products for `brand`.

    Empty list when the DB or table is absent, or when the brand has no
    active rows. Never raises on a missing-DB cold start so callers can
    safely guard with `if winners:` without try/except.
    """
    db = _db_path()
    if not db.exists():
        return []

    try:
        con = sqlite3.connect(db)
        con.row_factory = sqlite3.Row
        rows = con.execute(
            """
            SELECT id, name, prompt_text, sold, bid_count, parent_design
            FROM products
            WHERE brand = ? AND active = 1
            ORDER BY (sold * 10 + bid_count * 3 + COALESCE(current_bid, 0) / 1000) DESC
            LIMIT ?
            """,
            (brand, top_n),
        ).fetchall()
        con.close()
    except sqlite3.Error:
        return []

    return [dict(r) for r in rows]


def _cli() -> None:
    if len(sys.argv) < 2:
        print("usage: python winner_picker.py <brand> [top_n]", file=sys.stderr)
        sys.exit(1)
    brand = sys.argv[1]
    top_n = int(sys.argv[2]) if len(sys.argv) > 2 else 5
    print(json.dumps(pick_winners(brand, top_n), ensure_ascii=False, indent=2))


if __name__ == "__main__":
    _cli()
