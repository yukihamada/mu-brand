#!/usr/bin/env python3
"""
selfimprove_10min — 10-minute self-improvement loop for mu-brand factory.

Read-only against products.db. Picks the brand with the strongest 24h sales
momentum (defaulting to "mugen" on cold start), pulls its top 3 winners by
the same score generate.py uses (sold + bid_count*3), and appends a JSONL
summary so downstream tuners (ads, prompts, pricing) have a single source
of truth for "what is working right now".

Future ads/CVR-driven param tuning hooks into this same cadence — keep the
file small (<5 logical blocks) so the loop stays cheap and obvious.

CLI: `python selfimprove_10min.py` (no args). Exit 0 on every error path so
a flaky run never silences the surrounding cron.
"""
from __future__ import annotations

import json
import os
import sqlite3
import sys
import traceback
from datetime import datetime, timedelta, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DB_PATH = os.environ.get("MU_DB", str(ROOT / "products.db"))
LOG_DIR = ROOT / "logs"
JST = timezone(timedelta(hours=9))


def pick_top_brand(conn: sqlite3.Connection) -> str:
    """Brand with the most units sold in the last 24h; cold start -> mugen."""
    cutoff = (datetime.now(timezone.utc) - timedelta(hours=24)).isoformat()
    row = conn.execute(
        """
        SELECT brand, SUM(sold) AS s
        FROM products
        WHERE sold > 0 AND (sold_out_at >= ? OR created_at >= ?)
        GROUP BY brand
        ORDER BY s DESC
        LIMIT 1
        """,
        (cutoff, cutoff),
    ).fetchone()
    return row[0] if row and row[0] else "mugen"


def top_winners(conn: sqlite3.Connection, brand: str, limit: int = 3) -> list[dict]:
    rows = conn.execute(
        """
        SELECT id, name, sold, bid_count,
               (sold + COALESCE(bid_count,0) * 3) AS score
        FROM products
        WHERE brand = ? AND COALESCE(active,1) = 1
        ORDER BY score DESC, id DESC
        LIMIT ?
        """,
        (brand, limit),
    ).fetchall()
    return [
        {"id": r[0], "name": r[1], "sold": r[2] or 0, "bid_count": r[3] or 0, "score": r[4] or 0}
        for r in rows
    ]


def write_summary(summary: dict) -> Path:
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    fname = LOG_DIR / f"selfimprove_{datetime.now(JST).strftime('%Y%m%d')}.jsonl"
    with fname.open("a", encoding="utf-8") as f:
        f.write(json.dumps(summary, ensure_ascii=False) + "\n")
    return fname


def main() -> int:
    ts = datetime.now(JST).strftime("%Y-%m-%dT%H:%M%z")
    ts = ts[:-2] + ":" + ts[-2:]  # +0900 -> +09:00
    try:
        conn = sqlite3.connect(f"file:{DB_PATH}?mode=ro", uri=True)
        try:
            brand = pick_top_brand(conn)
            winners = top_winners(conn, brand)
        finally:
            conn.close()

        # optional hook: winner_picker may have richer scoring in future
        try:
            import importlib
            importlib.import_module("winner_picker")  # noqa: F401 (best-effort)
        except Exception:
            pass

        summary = {
            "ts": ts,
            "top_brand": brand,
            "winners": winners,
            "action": f"prioritize_{brand}",
        }
    except Exception as exc:  # never crash cron
        summary = {
            "ts": ts,
            "top_brand": None,
            "winners": [],
            "action": "noop",
            "error": f"{type(exc).__name__}: {exc}",
            "trace": traceback.format_exc(limit=3),
        }

    try:
        write_summary(summary)
    except Exception as exc:
        sys.stderr.write(f"[selfimprove_10min] log write failed: {exc}\n")

    print(json.dumps({k: v for k, v in summary.items() if k != "trace"}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
