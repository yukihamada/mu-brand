#!/usr/bin/env python3
"""Fetch public Printful CDN image URLs for every variant_id in catalog.

Used as a final fallback for the dashboard when AI mockups and POD
returns are unavailable. The Printful API endpoint
`/products/variant/{id}` returns an `image` field that is publicly
hostable on `files.cdn.printful.com`.

Output: /tmp/wearmu_printful_variants.json
        { "variant_id": "https://files.cdn.printful.com/...", ... }
"""
from __future__ import annotations
import concurrent.futures as cf
import json
import os
import sqlite3
import sys
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DB = ROOT / "store" / "products.db"
OUT = Path("/tmp/wearmu_printful_variants.json")

KEY = os.environ.get("PRINTFUL_API_KEY") or os.environ.get("PRINTFUL_API_TOKEN")
if not KEY:
    # try to read from /Users/yuki/.env
    env = Path("/Users/yuki/.env")
    if env.exists():
        for line in env.read_text().splitlines():
            if line.startswith("PRINTFUL_API_KEY="):
                KEY = line.split("=", 1)[1].strip().strip("'\"")
                break
            if line.startswith("PRINTFUL_API_TOKEN="):
                KEY = line.split("=", 1)[1].strip().strip("'\"")
                break
if not KEY:
    sys.exit("PRINTFUL_API_KEY missing")


def variant_image(variant_id: int) -> tuple[int, str | None]:
    url = f"https://api.printful.com/products/variant/{variant_id}"
    req = urllib.request.Request(url, headers={
        "Authorization": f"Bearer {KEY}",
        "User-Agent": "wearmu-dashboard/1",
    })
    try:
        with urllib.request.urlopen(req, timeout=15) as r:
            j = json.load(r)
        return variant_id, j.get("result", {}).get("variant", {}).get("image")
    except Exception as e:
        return variant_id, None


def main():
    conn = sqlite3.connect(str(DB))
    rows = conn.execute(
        "SELECT DISTINCT printful_variant_id FROM catalog_products WHERE status='live' AND printful_variant_id IS NOT NULL"
    ).fetchall()
    conn.close()
    variant_ids = sorted({r[0] for r in rows if r[0]})
    print(f"fetching {len(variant_ids):,} unique Printful variants…")

    # warm cache from existing file (if any) so reruns are cheap
    cache: dict[str, str] = {}
    if OUT.exists():
        try:
            cache = json.loads(OUT.read_text())
        except Exception:
            pass
    todo = [v for v in variant_ids if str(v) not in cache]
    print(f"cached={len(cache):,}  todo={len(todo):,}")

    started = time.time()
    with cf.ThreadPoolExecutor(max_workers=4) as ex:
        for vid, img in ex.map(variant_image, todo):
            if img:
                cache[str(vid)] = img
            time.sleep(0.05)  # very light rate-limit

    OUT.write_text(json.dumps(cache, indent=2))
    elapsed = time.time() - started
    print(f"\ndone. cache size={len(cache):,}  elapsed={elapsed:.0f}s → {OUT}")


if __name__ == "__main__":
    main()
