#!/usr/bin/env python3
"""Hourly cron entry point — runs each enabled TAXIGEN pattern.

Reads activation flags from cv_config (or the prod admin API). For each
pattern where `taxigen_<pattern>_active = '1'`, generates one tee.

Designed to be safe to call hourly without checking deeper schedules; the
gating is purely the activation flag.

Local usage (manual):
    python3 scripts/taxigen/cron_runner.py

Production (Fly cron / GH Actions schedule / etc):
    Hits this same script via cron. Check flags before doing anything paid.
"""
import os, sys, sqlite3
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from fetchers import PATTERNS  # noqa
from generator import gen_one_for_pattern, PATTERN_TO_BRAND  # noqa

ROOT = Path(__file__).resolve().parent.parent.parent
DB = Path(os.environ.get("DB_PATH") or (ROOT / "store" / "products.db"))


def is_pattern_active(pattern: str) -> bool:
    if not DB.exists():
        return False
    db = sqlite3.connect(DB)
    try:
        row = db.execute(
            "SELECT value FROM cv_config WHERE key=?",
            (f"taxigen_{pattern}_active",),
        ).fetchone()
        return bool(row and row[0] == "1")
    except Exception:
        return False
    finally:
        db.close()


def main():
    ran = 0
    skipped = 0
    for pattern in PATTERNS:
        if not is_pattern_active(pattern):
            print(f"  ◯ {pattern}: not activated (cv_config taxigen_{pattern}_active != '1')")
            skipped += 1
            continue
        print(f"  ↻ {pattern}: activated, generating...")
        drop, rel = gen_one_for_pattern(pattern)
        if drop:
            ran += 1
    print(f"\n📊 Generated: {ran}, Skipped: {skipped}")


if __name__ == "__main__":
    main()
