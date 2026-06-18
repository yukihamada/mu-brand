#!/usr/bin/env python3
"""Regenerate designs for SKUs marked "shoddy" in the dashboard.

Input: SKU list (one per line), either via stdin, file, or arg.
For each SKU:
  - Look up brand + label + description from catalog_products
  - Compose a Gemini prompt that respects the brand's tone
  - Generate a fresh transparent PNG at higher resolution
  - Save to designs/<brand>_<sku>_<hash>.png
  - Also save to store/static/<brand>/d/design_<SKU>.png so the live
    site picks it up after deploy

Usage:
    pbpaste | python3 scripts/regen_shoddy_designs.py
    python3 scripts/regen_shoddy_designs.py MU-BJJ-01-TEE-BLACK JF-BAG-01
    python3 scripts/regen_shoddy_designs.py --file /tmp/shoddy.txt --dry-run
"""
from __future__ import annotations
import argparse
import base64
import hashlib
import json
import os
import sqlite3
import sys
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DB = ROOT / "store" / "products.db"
DESIGNS = ROOT / "designs"
STATIC = ROOT / "store" / "static"

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY:
    env = Path("/Users/yuki/.env")
    if env.exists():
        for line in env.read_text().splitlines():
            if line.startswith("GEMINI_API_KEY=") or line.startswith("GOOGLE_API_KEY="):
                KEY = line.split("=", 1)[1].strip().strip("'\"")
                break
if not KEY:
    sys.exit("GEMINI_API_KEY not set")

MODEL = "gemini-3-pro-image-preview"

# Per-brand design directive. Keep concise; the catalog description carries the concept.
BRAND_STYLE = {
    "bjj": ("BJJ / Jiu-Jitsu apparel print art. Clean editorial typography, sumi-e "
            "minimal ink-on-tee feel. Black ink or white ink on transparent."),
    "code": ("Developer aesthetic. Monospace typography, terminal motifs, subtle "
             "tech humor. Black ink on transparent."),
    "coffee": ("Coffee culture print. Warm earthy palette, hand-drawn line work, "
               "elegant kerning. Espresso-brown or off-white ink on transparent."),
    "zen": ("Zen meditation print. Sumi-e brush calligraphy, single-stroke energy, "
            "minimal Japanese typography. Black ink on transparent."),
    "moon": ("Lunar / celestial print. Crescent moon glyph, dotted lines, subtle "
             "constellation. White or pale-yellow on transparent."),
    "mu": ("MU brand · negation/void aesthetic. The ◯ / ⊘ glyph, ultra-minimal, "
           "single calligraphic stroke. Gold (#e6c449) or white on transparent."),
    "tokyo": ("Tokyo street print. Mix of katakana + roman type, mid-century travel-poster vibe. "
              "Limited 2-color palette on transparent."),
    "jiuflow": ("BJJ athlete platform branding. Bold competition typography, "
                "stopwatch / mat motifs, JF mark."),
    "kokon": ("Premium yakiniku restaurant print. Refined brass/gold serif, subtle "
              "charcoal/binchotan motif. Brass color on transparent."),
    "roll": ("BJJ rolling action print. Dynamic kanji + energetic line work. "
             "Single-color on transparent."),
}

PROMPT_TEMPLATE = """High-resolution print-ready apparel artwork.

Brand: MU × {brand}
Concept: {label}
Description: {description}
Style: {style}

Critical print requirements:
- Transparent background (alpha channel, NOT white).
- Square aspect ratio, design fully inside, max 80% width.
- No bounding rectangle, no white halo, NO photographic backdrop.
- Sharp edges, screen-print-friendly. Bold enough to read at 100mm.
- Output 1024x1024 PNG with alpha.

Avoid: photographic models, mockup t-shirts, busy gradients, JPEG-like artifacts.
"""


def fetch_sku(conn: sqlite3.Connection, sku: str) -> dict | None:
    r = conn.execute(
        "SELECT brand, label, description_ja FROM catalog_products WHERE sku=?",
        (sku,)).fetchone()
    if not r:
        return None
    return {"sku": sku, "brand": r[0], "label": r[1] or "", "description_ja": r[2] or ""}


def regen_one(sku_row: dict, dry: bool) -> bool:
    brand = sku_row["brand"]
    style = BRAND_STYLE.get(brand, "Clean editorial apparel print, minimal type, "
                                   "single-color screen-print friendly. On transparent BG.")
    prompt = PROMPT_TEMPLATE.format(
        brand=brand, label=sku_row["label"],
        description=sku_row["description_ja"], style=style)
    sku = sku_row["sku"]

    if dry:
        print(f"  [dry] {sku} ({brand}) → would gen with style={style[:60]}…")
        return True

    url = f"https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:generateContent?key={KEY}"
    body = json.dumps({
        "contents": [{"parts": [{"text": prompt}]}],
        "generationConfig": {"responseModalities": ["IMAGE", "TEXT"], "temperature": 0.85},
    }).encode()
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            j = json.load(r)
    except Exception as e:
        print(f"  [err] {sku}: {e}")
        return False

    for cand in j.get("candidates", []):
        for part in cand.get("content", {}).get("parts", []):
            d = part.get("inlineData") or part.get("inline_data")
            if d and d.get("data"):
                png = base64.b64decode(d["data"])
                h = hashlib.sha1(png).hexdigest()[:8]
                # 1. canonical store: designs/<brand>_<SKU>_<hash>.png
                p1 = DESIGNS / f"{brand}_{sku}_{h}.png"
                p1.write_bytes(png)
                # 2. live-served path: store/static/<brand>/d/design_<SKU>.png
                p2 = STATIC / brand / "d" / f"design_{sku}.png"
                p2.parent.mkdir(parents=True, exist_ok=True)
                p2.write_bytes(png)
                print(f"  ✓ {sku} → {p1.name} + {p2.relative_to(ROOT)} ({len(png):,}B)")
                return True
    print(f"  [empty] {sku}: no inline image in response")
    return False


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("skus", nargs="*", help="explicit SKUs")
    ap.add_argument("--file", help="SKU list file (one per line)")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--limit", type=int, default=None)
    args = ap.parse_args()

    skus = list(args.skus)
    if args.file:
        skus += [s.strip() for s in Path(args.file).read_text().splitlines() if s.strip()]
    if not skus and not sys.stdin.isatty():
        skus += [s.strip() for s in sys.stdin.readlines() if s.strip()]
    skus = [s for s in skus if s and not s.startswith("#")]
    if not skus:
        sys.exit("no SKUs provided (paste list on stdin or use --file or args)")
    if args.limit:
        skus = skus[: args.limit]

    print(f"regenerating {len(skus)} SKU designs (dry={args.dry_run})…")
    conn = sqlite3.connect(str(DB))
    started = time.time()
    ok = fail = miss = 0
    for sku in skus:
        row = fetch_sku(conn, sku)
        if not row:
            miss += 1
            print(f"  [no-db] {sku}: not in catalog_products — skipping")
            continue
        if regen_one(row, args.dry_run):
            ok += 1
        else:
            fail += 1
        time.sleep(1.5)  # gemini rate limit polite
    conn.close()
    cost_jpy = ok * 6
    print(f"\ndone. ok={ok} fail={fail} not_found={miss}  elapsed={time.time()-started:.0f}s  cost≈¥{cost_jpy}")


if __name__ == "__main__":
    main()
