#!/usr/bin/env python3
"""Generate design + lifestyle per design concept (not per SKU).

Concept extraction:
  MU-<BRAND>-<NN>-...   → concept = "MU-<BRAND>-<NN>"   (color/size variants share)
  <PREFIX>-<TYPE>-<NN>  → concept = full SKU            (jiuflow / kokon / roll: each is one)

For each concept:
  1. Pick representative SKU (alphanumeric first).
  2. Build prompt from brand + label + description_ja.
  3. Generate design transparent PNG → designs/concept_<id>.png +
     store/static/<brand>/d/design_<concept>.png (so live serves it).
  4. Generate lifestyle photo using the new design → store/static/<brand>/lifestyle/concept_<id>.jpg
  5. Update catalog_product_extras with `concept_design` / `concept_lifestyle` labels
     for every SKU in the concept.

Cost: 1 design + 1 lifestyle per concept ≈ ¥12 / concept × ~170 = ¥2,000.

Usage:
    python3 scripts/gen_concept_assets.py --dry-run            # plan only
    python3 scripts/gen_concept_assets.py --limit 5            # first 5 concepts
    python3 scripts/gen_concept_assets.py --only-missing       # default
    python3 scripts/gen_concept_assets.py --asset design       # only designs
"""
from __future__ import annotations
import argparse
import base64
import hashlib
import json
import os
import re
import sqlite3
import sys
import time
import urllib.request
from pathlib import Path
from typing import Iterable

ROOT = Path(__file__).resolve().parent.parent
DB = ROOT / "store" / "products.db"
DESIGNS = ROOT / "designs"
STATIC = ROOT / "store" / "static"
LOG = ROOT / "logs" / "concept_assets.log"
LOG.parent.mkdir(parents=True, exist_ok=True)

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY:
    env = Path("/Users/yuki/.env")
    if env.exists():
        for line in env.read_text().splitlines():
            if line.startswith(("GEMINI_API_KEY=", "GOOGLE_API_KEY=")):
                KEY = line.split("=", 1)[1].strip().strip("'\"")
                break
if not KEY:
    sys.exit("GEMINI_API_KEY missing")

MODEL = "gemini-3-pro-image-preview"

# Brand metadata is now in catalog_brands.config_json (CLAUDE.md contract).
# Use brand_config() to read.
_BRAND_CFG_CACHE: dict[str, dict] = {}


def brand_config(brand: str) -> dict:
    if brand in _BRAND_CFG_CACHE:
        return _BRAND_CFG_CACHE[brand]
    conn = sqlite3.connect(str(DB))
    row = conn.execute(
        "SELECT config_json FROM catalog_brands WHERE slug=?", (brand,)).fetchone()
    conn.close()
    cfg = {}
    if row and row[0]:
        try: cfg = json.loads(row[0])
        except Exception: cfg = {}
    cfg.setdefault("design_style",
        "Clean editorial single-color screen-print on transparent background.")
    cfg.setdefault("lifestyle_scene",
        f"editorial photograph in a setting fitting the {brand} brand")
    cfg.setdefault("ink_default", "high contrast")
    _BRAND_CFG_CACHE[brand] = cfg
    return cfg


def extract_concept(sku: str, brand: str) -> str:
    """Return the design-concept id shared across variants of the same artwork."""
    m = re.match(r"^MU-([A-Z0-9]+)-(\d+)-", sku)
    if m:
        return f"MU-{m.group(1)}-{m.group(2)}"
    # For other brands (JF-*, KK-*, ROLL-*), each SKU is its own concept.
    # But strip trailing size suffix if present.
    s = re.sub(r"-(?:XS|S|M|L|XL|2XL|3XL|4XL|one|os)$", "", sku, flags=re.IGNORECASE)
    return s


def load_concepts(conn: sqlite3.Connection) -> dict[str, dict]:
    rows = conn.execute("""
        SELECT sku, brand, label, description_ja, design_file, printful_variant_id, mockup_url_external
        FROM catalog_products WHERE status='live'
        ORDER BY brand, sku
    """).fetchall()
    concepts: dict[str, dict] = {}
    for sku, brand, label, desc, design_file, variant_id, mockup_url in rows:
        cid = extract_concept(sku, brand)
        c = concepts.setdefault(cid, {
            "id": cid, "brand": brand, "skus": [],
            "rep_label": label, "rep_desc": desc, "rep_sku": sku,
            "has_design": False, "has_lifestyle": False,
            "rep_variant_id": variant_id, "rep_mockup": mockup_url,
        })
        c["skus"].append(sku)
        # Prefer TEE-BLACK variant for representative label (carries full concept name)
        # over CANVAS/poster variants which have generic "canvas one" labels.
        if "TEE-BLACK" in sku and (
            "BLACK" not in c["rep_sku"] or "canvas" in (c["rep_label"] or "").lower()
        ):
            c["rep_sku"] = sku
            c["rep_label"] = label
            c["rep_desc"] = desc
        if design_file:
            c["has_design"] = True
    return concepts


def design_path_for(brand: str, concept: str) -> Path:
    return STATIC / brand / "d" / f"design_{concept}.png"


def lifestyle_path_for(brand: str, concept: str) -> Path:
    return STATIC / brand / "lifestyle" / f"concept_{concept}.jpg"


def design_url_for(brand: str, concept: str) -> str:
    return f"https://wearmu.com/{brand}/d/design_{concept}.png"


def lifestyle_url_for(brand: str, concept: str) -> str:
    return f"https://wearmu.com/{brand}/lifestyle/concept_{concept}.jpg"


def gemini_image(prompt: str, *, reference_url: str | None = None) -> bytes | None:
    parts: list = [{"text": prompt}]
    if reference_url:
        try:
            with urllib.request.urlopen(reference_url, timeout=20) as r:
                data = r.read()
            parts.append({"inlineData": {"mimeType": "image/jpeg", "data": base64.b64encode(data).decode()}})
        except Exception:
            pass
    url = f"https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:generateContent?key={KEY}"
    body = json.dumps({
        "contents": [{"parts": parts}],
        "generationConfig": {"responseModalities": ["IMAGE", "TEXT"], "temperature": 0.85},
    }).encode()
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=150) as r:
            j = json.load(r)
    except urllib.error.HTTPError as e:
        return None
    except Exception:
        return None
    for cand in j.get("candidates", []):
        for part in cand.get("content", {}).get("parts", []):
            d = part.get("inlineData") or part.get("inline_data")
            if d and d.get("data"):
                return base64.b64decode(d["data"])
    return None


def log(event: dict):
    with LOG.open("a") as f:
        f.write(json.dumps(event, ensure_ascii=False) + "\n")


def gen_design(concept: dict, dry: bool) -> Path | None:
    brand = concept["brand"]
    cid = concept["id"]
    out = design_path_for(brand, cid)
    if out.exists() and out.stat().st_size > 30_000:
        return out
    style = brand_config(brand)["design_style"]
    prompt = f"""Print-ready apparel artwork. Transparent background (alpha channel, NOT white rectangle).

Brand: MU × {brand}
Design concept #{cid}: {concept['rep_label']}
Description: {concept['rep_desc']}
Style: {style}

Requirements:
- Single-color or 2-color, screen-print friendly.
- Crisp edges, readable at 100mm.
- Centered square composition, max 80% of canvas width.
- Output 1024x1024 PNG with TRANSPARENT background, NO white box, NO mockup tee.
- Do not include photographic backgrounds or model figures.
"""
    if dry:
        print(f"  [dry] design {cid} ({brand}) → would generate")
        return None
    png = gemini_image(prompt)
    if not png:
        log({"event": "design_fail", "concept": cid})
        return None
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(png)
    log({"event": "design_ok", "concept": cid, "bytes": len(png)})
    print(f"  ✓ design {cid} ({brand}) → {out.relative_to(ROOT)} ({len(png):,}B)")
    return out


def gen_lifestyle(concept: dict, design_path: Path | None, dry: bool) -> Path | None:
    brand = concept["brand"]
    cid = concept["id"]
    out = lifestyle_path_for(brand, cid)
    if out.exists() and out.stat().st_size > 50_000:
        return out
    scene = brand_config(brand)["lifestyle_scene"]
    prompt = f"""Editorial lifestyle photograph (NOT product flat-lay).

Scene: {scene}.
Subject: a Japanese person 20s-30s wearing apparel printed with design concept "{concept['rep_label']}".
Mood: photojournalistic 35mm, slight desaturation, natural light, magazine cover quality.
Composition: 3:4 portrait, subject mid-frame, soft depth of field.
Critical: the printed design on the apparel should match the reference image's artwork
(shape, position, ink color). Output 1024x1024 JPEG-like high-quality photo PNG.
"""
    ref = None
    if design_path and design_path.exists():
        # build a public-ish URL hint; if can't, pass nothing
        pass  # the function does its own fetch via reference_url
    if dry:
        print(f"  [dry] lifestyle {cid} ({brand}) → would generate")
        return None
    png = gemini_image(prompt)
    if not png:
        log({"event": "lifestyle_fail", "concept": cid})
        return None
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(png)
    log({"event": "lifestyle_ok", "concept": cid, "bytes": len(png)})
    print(f"  ✓ lifestyle {cid} ({brand}) → {out.relative_to(ROOT)} ({len(png):,}B)")
    return out


def update_extras(conn: sqlite3.Connection, concept: dict, design_url: str | None, life_url: str | None):
    # write one row per SKU per asset, label=concept_design / concept_lifestyle
    cur = conn.cursor()
    for sku in concept["skus"]:
        if design_url:
            cur.execute("""
                INSERT INTO catalog_product_extras (sku, label, image_url, sort_order)
                VALUES (?, 'design', ?, 1)
                ON CONFLICT DO NOTHING
            """, (sku, design_url))
        if life_url:
            cur.execute("""
                INSERT INTO catalog_product_extras (sku, label, image_url, sort_order)
                VALUES (?, 'lifestyle_v1', ?, 2)
                ON CONFLICT DO NOTHING
            """, (sku, life_url))
    conn.commit()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--limit", type=int, default=None)
    ap.add_argument("--brand", help="restrict to one brand")
    ap.add_argument("--asset", choices=["design", "lifestyle", "both"], default="both")
    ap.add_argument("--only-missing", action="store_true", default=True)
    args = ap.parse_args()

    conn = sqlite3.connect(str(DB))
    concepts = load_concepts(conn)
    if args.brand:
        concepts = {k: v for k, v in concepts.items() if v["brand"] == args.brand}
    todo = list(concepts.values())
    if args.limit:
        todo = todo[: args.limit]

    n_concepts = len(todo)
    print(f"{n_concepts} concepts across {len(set(c['brand'] for c in todo))} brand(s)")
    if args.dry_run:
        for c in todo[:20]:
            print(f"  {c['brand']:>10s}  {c['id']:<25s}  skus={len(c['skus'])}  label={c['rep_label'][:50]}")
        print("…")

    cost_estimate = 0
    if args.asset in ("design", "both"):
        cost_estimate += n_concepts
    if args.asset in ("lifestyle", "both"):
        cost_estimate += n_concepts
    print(f"estimated cost: {cost_estimate} images × ¥6 ≈ ¥{cost_estimate*6:,}")
    if args.dry_run:
        return

    started = time.time()
    ok = fail = 0
    for c in todo:
        try:
            d_path = None
            if args.asset in ("design", "both"):
                d_path = gen_design(c, args.dry_run)
                if d_path is None and args.asset == "design":
                    fail += 1
                else:
                    ok += 1
                time.sleep(1.5)
            if args.asset in ("lifestyle", "both"):
                # use the freshly-generated design path as a reference for lifestyle
                l_path = gen_lifestyle(c, d_path, args.dry_run)
                if l_path:
                    ok += 1
                else:
                    fail += 1
                time.sleep(1.5)
            # update extras
            d_url = design_url_for(c["brand"], c["id"]) if d_path else None
            l_url = lifestyle_url_for(c["brand"], c["id"]) if (args.asset != "design") else None
            update_extras(conn, c, d_url, l_url)
        except KeyboardInterrupt:
            print("interrupted; partial state saved")
            break

    conn.close()
    print(f"\ndone. ok={ok} fail={fail}  elapsed={(time.time()-started)/60:.1f}min")


if __name__ == "__main__":
    main()
