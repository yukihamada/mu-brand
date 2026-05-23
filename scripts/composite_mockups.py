#!/usr/bin/env python3
"""Composite design PNG onto Printful blank product photo using PIL.

For SKUs whose live mockup URL 404s, generate a locally-composited
"design on product" image by:
  1. Download Printful catalog blank product photo (cached in
     /tmp/wearmu_printful_variants.json)
  2. Open the concept's design PNG (transparent)
  3. Paste design at product-type-appropriate position + scale
  4. Save to /tmp/composites/<sku>.jpg

The dashboard then uses these as the AI mockup column for SKUs
without a real mockup.

Output: /tmp/composites/*.jpg + /tmp/wearmu_composites.json (sku → file://uri)

Usage:
    python3 scripts/composite_mockups.py
    python3 scripts/composite_mockups.py --brand bjj --limit 50
"""
from __future__ import annotations
import argparse
import json
import re
import sqlite3
import sys
import urllib.request
from io import BytesIO
from pathlib import Path

try:
    from PIL import Image
except ImportError:
    sys.exit("Pillow missing: pip install pillow")

ROOT = Path(__file__).resolve().parent.parent
DB = ROOT / "store" / "products.db"
OUT_DIR = Path("/tmp/composites")
OUT_DIR.mkdir(exist_ok=True)
MAP_FILE = Path("/tmp/wearmu_composites.json")

PRINTFUL_VARIANTS = json.loads(Path("/tmp/wearmu_printful_variants.json").read_text())
URL_STATUS = json.loads(Path("/tmp/wearmu_url_status.json").read_text()) if Path("/tmp/wearmu_url_status.json").exists() else {}

# Cache for downloaded blank product photos
PRODUCT_CACHE: dict[str, Image.Image] = {}


def extract_concept(sku: str) -> str:
    m = re.match(r"^MU-([A-Z0-9]+)-(\d+)-", sku)
    if m:
        return f"MU-{m.group(1)}-{m.group(2)}"
    return re.sub(r"-(?:XS|S|M|L|XL|2XL|3XL|4XL|one|os)$", "", sku, flags=re.IGNORECASE)


def design_path_for(brand: str, concept: str) -> Path:
    return ROOT / "store" / "static" / brand / "d" / f"design_{concept}.png"


def fetch_product(url: str) -> Image.Image | None:
    if url in PRODUCT_CACHE:
        return PRODUCT_CACHE[url]
    try:
        req = urllib.request.Request(url, headers={
            "User-Agent": "Mozilla/5.0 wearmu-composite/1.0",
            "Accept": "image/jpeg,image/png,image/*"
        })
        with urllib.request.urlopen(req, timeout=20) as r:
            data = r.read()
        img = Image.open(BytesIO(data)).convert("RGBA")
        PRODUCT_CACHE[url] = img
        return img
    except Exception as e:
        print(f"  fetch err {url[:80]}…: {e}")
        return None


def position_for(product_label: str, product_w: int, product_h: int, design_w: int, design_h: int) -> tuple[int, int, int, int]:
    """Return (x, y, scaled_w, scaled_h) where to paste design on the product image."""
    label = (product_label or "").lower()
    # default: chest area for apparel
    if "tee" in label or "t-shirt" in label or "shirt" in label or "polo" in label or "tank" in label:
        target_w = int(product_w * 0.34)
        center_y = int(product_h * 0.40)
    elif "hoodie" in label or "sweat" in label:
        target_w = int(product_w * 0.32)
        center_y = int(product_h * 0.46)
    elif "long" in label or "ls " in label:
        target_w = int(product_w * 0.32)
        center_y = int(product_h * 0.40)
    elif "rash" in label:
        target_w = int(product_w * 0.30)
        center_y = int(product_h * 0.42)
    elif "canvas" in label or "poster" in label:
        target_w = int(product_w * 0.75)
        center_y = int(product_h * 0.50)
    elif "mug" in label:
        target_w = int(product_w * 0.30)
        center_y = int(product_h * 0.55)
    elif "tote" in label or "bag" in label:
        target_w = int(product_w * 0.40)
        center_y = int(product_h * 0.50)
    elif "sticker" in label or "pin" in label or "card" in label or "coas" in label:
        target_w = int(product_w * 0.80)
        center_y = int(product_h * 0.50)
    elif "cap" in label or "snapback" in label or "beanie" in label or "hat" in label:
        target_w = int(product_w * 0.18)
        center_y = int(product_h * 0.50)
    elif "leg" in label or "spat" in label or "jog" in label or "short" in label:
        target_w = int(product_w * 0.18)
        center_y = int(product_h * 0.55)
    elif "apron" in label:
        target_w = int(product_w * 0.30)
        center_y = int(product_h * 0.55)
    else:
        # fallback center chest
        target_w = int(product_w * 0.30)
        center_y = int(product_h * 0.45)
    scale = target_w / max(design_w, 1)
    scaled_w = target_w
    scaled_h = int(design_h * scale)
    x = (product_w - scaled_w) // 2
    y = center_y - scaled_h // 2
    return x, y, scaled_w, scaled_h


def composite_one(sku: str, brand: str, label: str, design: Image.Image, product: Image.Image) -> Path | None:
    """Composite design onto product, save to OUT_DIR/<sku>.jpg."""
    out = OUT_DIR / f"{sku}.jpg"
    pw, ph = product.size
    dw, dh = design.size
    x, y, sw, sh = position_for(label, pw, ph, dw, dh)
    # ensure transparent background for design (assume RGBA)
    if design.mode != "RGBA":
        design = design.convert("RGBA")
    resized = design.resize((sw, sh), Image.LANCZOS)
    canvas = product.copy()
    canvas.alpha_composite(resized, (x, y))
    canvas.convert("RGB").save(out, "JPEG", quality=88, optimize=True)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--brand", help="restrict")
    ap.add_argument("--limit", type=int)
    ap.add_argument("--force", action="store_true", help="rebuild even if file exists")
    args = ap.parse_args()

    conn = sqlite3.connect(str(DB))
    where = "WHERE status='live'"
    params = []
    if args.brand:
        where += " AND brand=?"
        params.append(args.brand)
    rows = conn.execute(
        f"SELECT sku, brand, label, printful_variant_id, mockup_main_file FROM catalog_products {where} ORDER BY brand, sku",
        params,
    ).fetchall()
    conn.close()
    if args.limit:
        rows = rows[: args.limit]

    print(f"compositing for {len(rows):,} SKUs…")
    composites: dict[str, str] = {}
    if MAP_FILE.exists():
        try:
            composites = json.loads(MAP_FILE.read_text())
        except Exception:
            pass

    designs_cache: dict[tuple[str, str], Image.Image] = {}
    ok = skip = fail = nodesign = noproduct = 0
    for i, (sku, brand, label, variant_id, mockup) in enumerate(rows, start=1):
        out_path = OUT_DIR / f"{sku}.jpg"
        if not args.force and out_path.exists() and out_path.stat().st_size > 20_000:
            skip += 1; continue
        # Need a concept design
        cid = extract_concept(sku)
        key = (brand, cid)
        design_img = designs_cache.get(key)
        if design_img is None:
            dpath = design_path_for(brand, cid)
            if not dpath.exists():
                nodesign += 1; continue
            try:
                design_img = Image.open(dpath).convert("RGBA")
                designs_cache[key] = design_img
            except Exception:
                nodesign += 1; continue
        # Need a product blank
        prod_url = PRINTFUL_VARIANTS.get(str(variant_id))
        if not prod_url:
            noproduct += 1; continue
        product = fetch_product(prod_url)
        if not product:
            noproduct += 1; continue
        try:
            out = composite_one(sku, brand, label, design_img, product)
            composites[sku] = out.as_uri()
            ok += 1
        except Exception as e:
            fail += 1
        if i % 100 == 0:
            print(f"  {i}/{len(rows)}  ok={ok} skip={skip} nodesign={nodesign} noproduct={noproduct} fail={fail}")
            MAP_FILE.write_text(json.dumps(composites, indent=2))

    MAP_FILE.write_text(json.dumps(composites, indent=2))
    print(f"\ndone. ok={ok} skip={skip} nodesign={nodesign} noproduct={noproduct} fail={fail}")
    print(f"map → {MAP_FILE} ({len(composites):,} entries)")


if __name__ == "__main__":
    main()
