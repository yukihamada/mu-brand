#!/usr/bin/env python3
"""Generate T-shirt designs for ads_* SKUs using Gemini 3.

Reads prompt_text from store/products.db, generates a single 2940×2940
transparent-background design PNG per row, saves to store/static/ads/,
updates design_url + mockup_url. Idempotent — rows that already have
design_url are skipped unless --force.

Usage:
    python3 scripts/gen_ads_designs.py                   # all ads_* without design
    python3 scripts/gen_ads_designs.py --ids 195,200,212 # specific
    python3 scripts/gen_ads_designs.py --top 5           # 5 priority SKUs
    python3 scripts/gen_ads_designs.py --force --ids 195 # overwrite
"""
import argparse, hashlib, os, sqlite3, sys
from datetime import datetime
from pathlib import Path

# Force-load /Users/yuki/.env (zshrc has revoked GEMINI key — feedback_gemini_key_env)
os.environ.pop("GOOGLE_API_KEY", None)
_env = Path("/Users/yuki/.env")
if _env.exists():
    for ln in _env.read_text().splitlines():
        ln = ln.strip()
        if "=" in ln and not ln.startswith("#"):
            k, v = ln.split("=", 1)
            if k.strip() == "GEMINI_API_KEY":
                os.environ["GEMINI_API_KEY"] = v.strip().strip('"').strip("'")

from google import genai
from google.genai import types

ROOT = Path(__file__).resolve().parent.parent
DB = ROOT / "store" / "products.db"
OUT_DIR = ROOT / "store" / "static" / "ads"
OUT_DIR.mkdir(parents=True, exist_ok=True)

GEMINI_MODEL = "gemini-3-pro-image-preview"
PRIORITY_IDS = [195, 200, 203, 207, 212]  # NOGI / 黒帯 / 三田 / 焼肉古今 / SOLUNA FEST

# Wrap the SKU's stored prompt with a print-ready system instruction.
SYSTEM_PROMPT = """You are a professional apparel graphic designer for the MU brand (wearmu.com).
Produce a single T-shirt print design as a square 2940×2940 PNG with a
transparent background. Optimized for direct-to-garment printing.

Strict rules:
- Solid flat shapes, max 3 colors total, high contrast
- NO photographic backgrounds, NO gradients except subtle, NO mesh effects
- NO realistic faces, NO trademarked logos, NO brand names of others
- Center the design — leave breathing room near edges (10% padding minimum)
- Text must be legible at 4cm tall

Design brief: {brief}

Output: ONE square print-ready graphic, transparent background."""


def _strip_ad_keyword(p: str) -> str:
    """Remove '[Ad keyword: ...] ' prefix — Gemini sometimes renders it as
    visible design text (e.g. Engineer Tee). The keyword is metadata for
    Google Ads, not part of the visual brief."""
    import re
    return re.sub(r"^\s*\[Ad keyword:[^\]]*\]\s*", "", p)


def gen_image(client, brief: str) -> bytes:
    """Call Gemini image preview model. Returns PNG bytes."""
    brief = _strip_ad_keyword(brief)
    resp = client.models.generate_content(
        model=GEMINI_MODEL,
        contents=SYSTEM_PROMPT.format(brief=brief),
        config=types.GenerateContentConfig(
            response_modalities=["IMAGE", "TEXT"],
        ),
    )
    for cand in resp.candidates or []:
        for part in (cand.content.parts if cand.content else []):
            if getattr(part, "inline_data", None) and part.inline_data.data:
                return part.inline_data.data
    raise RuntimeError("No image returned by Gemini")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ids", help="comma-separated product ids")
    ap.add_argument("--top", type=int, help="generate top N priority SKUs")
    ap.add_argument("--force", action="store_true", help="overwrite existing")
    args = ap.parse_args()

    if not os.environ.get("GEMINI_API_KEY"):
        sys.exit("GEMINI_API_KEY not set")

    db = sqlite3.connect(DB)
    db.row_factory = sqlite3.Row

    if args.ids:
        ids = [int(s) for s in args.ids.split(",")]
        where = f"id IN ({','.join('?' * len(ids))})"
        params = ids
    elif args.top:
        ids = PRIORITY_IDS[: args.top]
        where = f"id IN ({','.join('?' * len(ids))})"
        params = ids
    else:
        where = "brand LIKE 'ads_%'"
        params = []

    rows = db.execute(
        f"SELECT id, brand, drop_num, name, design_url, prompt_text "
        f"FROM products WHERE {where} ORDER BY id",
        params,
    ).fetchall()

    if not rows:
        sys.exit("No matching products.")

    client = genai.Client(api_key=os.environ["GEMINI_API_KEY"])
    done, skipped, failed = 0, 0, 0

    for r in rows:
        pid, brand, drop, name, design_url, prompt = (
            r["id"], r["brand"], r["drop_num"], r["name"],
            r["design_url"], r["prompt_text"] or "",
        )
        if design_url and not args.force:
            print(f"  ◯ skip id={pid} (already has design_url={design_url})")
            skipped += 1
            continue
        try:
            print(f"  ↻ generating id={pid} {brand}#{drop} — {name[:50]}...")
            img_bytes = gen_image(client, prompt)
            h = hashlib.sha256(img_bytes).hexdigest()[:8]
            fname = f"{brand}_{drop:03d}_{h}.png"
            fpath = OUT_DIR / fname
            fpath.write_bytes(img_bytes)
            rel_url = f"/static/ads/{fname}"
            db.execute(
                "UPDATE products SET design_url=?, mockup_url=? WHERE id=?",
                (rel_url, rel_url, pid),
            )
            db.commit()
            print(f"  ✓ id={pid} → {rel_url} ({len(img_bytes):,} bytes)")
            done += 1
        except Exception as e:
            print(f"  ✗ id={pid} FAILED: {e}")
            failed += 1

    print()
    print(f"📊 Generated: {done}, Skipped: {skipped}, Failed: {failed}")


if __name__ == "__main__":
    main()
