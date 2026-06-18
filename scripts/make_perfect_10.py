#!/usr/bin/env python3
"""Generate truly-perfect 4-asset set for 10 representative SKUs.

For each SKU:
  1. design  — load existing concept transparent PNG (high quality)
  2. mockup  — Gemini 3 Pro Image: paste design on Printful product photo
              with proper photoreal shading. Saves to
              store/static/<brand>/m/perfect_<sku>.jpg
  3. AI col  — same as mockup (or omit)
  4. lifestyle — Gemini: person wearing THIS product type with THIS design
                 in scene appropriate for the brand. Saves to
                 store/static/<brand>/lifestyle/perfect_<sku>.jpg

Output map: /tmp/wearmu_perfect10.json
"""
from __future__ import annotations
import base64
import json
import os
import re
import sqlite3
import sys
import time
import urllib.request
from io import BytesIO
from pathlib import Path

try:
    from PIL import Image
except ImportError:
    sys.exit("Pillow missing")

ROOT = Path(__file__).resolve().parent.parent
DB = ROOT / "store" / "products.db"
OUT_MAP = Path("/tmp/wearmu_perfect10.json")

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY:
    env = Path("/Users/yuki/.env")
    for line in env.read_text().splitlines() if env.exists() else []:
        if line.startswith(("GEMINI_API_KEY=", "GOOGLE_API_KEY=")):
            KEY = line.split("=", 1)[1].strip().strip("'\"")
            break
if not KEY:
    sys.exit("GEMINI_API_KEY missing")

PRINTFUL_VARIANTS = json.loads(Path("/tmp/wearmu_printful_variants.json").read_text())

MODEL = "gemini-3-pro-image-preview"

THE_TEN = [
    "MU-BJJ-01-TEE-BLACK",
    "MU-BJJ-01-HOODIE-BLACK-M",
    "MU-BJJ-01-LONG-SLEEVE-BLACK-L",
    "MU-BJJ-01-RASH",
    "MU-CODE-01-TEE-BLACK",
    "MU-COFFEE-01-TEE-BLACK",
    "MU-ZEN-01-TEE-BLACK",
    "JF-HOOD-01",
    "KK-APRON-01",
    "ROLL-TEE-01",
]


def extract_concept(sku: str) -> str:
    m = re.match(r"^MU-([A-Z0-9]+)-(\d+)-", sku)
    if m:
        return f"MU-{m.group(1)}-{m.group(2)}"
    return re.sub(r"-(?:XS|S|M|L|XL|2XL|3XL|4XL|one|os)$", "", sku, flags=re.IGNORECASE)


def design_path(brand: str, concept: str) -> Path:
    return ROOT / "store" / "static" / brand / "d" / f"design_{concept}.png"


def fetch_bytes(url: str, timeout=20) -> bytes | None:
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0 wearmu/1"})
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.read()
    except Exception as e:
        print(f"  fetch err: {e}")
        return None


def gemini(prompt: str, refs: list[bytes]) -> bytes | None:
    parts = [{"text": prompt}]
    for b in refs:
        parts.append({"inlineData": {"mimeType": "image/png", "data": base64.b64encode(b).decode()}})
    url = f"https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:generateContent?key={KEY}"
    body = json.dumps({
        "contents": [{"parts": parts}],
        "generationConfig": {"responseModalities": ["IMAGE", "TEXT"], "temperature": 0.7},
    }).encode()
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=180) as r:
            j = json.load(r)
    except urllib.error.HTTPError as e:
        print(f"  HTTP {e.code}: {e.read()[:200].decode(errors='replace')}")
        return None
    except Exception as e:
        print(f"  err: {e}")
        return None
    for cand in j.get("candidates", []):
        for part in cand.get("content", {}).get("parts", []):
            d = part.get("inlineData") or part.get("inline_data")
            if d and d.get("data"):
                return base64.b64decode(d["data"])
    return None


PRODUCT_DESC = {
    # rough product description for prompt clarity
    "TEE-BLACK": "black short-sleeve unisex T-shirt",
    "TEE-WHITE": "white short-sleeve unisex T-shirt",
    "HOODIE-BLACK": "black heavyweight pullover hoodie",
    "LONG-SLEEVE-BLACK": "black long-sleeve T-shirt",
    "RASH": "white long-sleeve rashguard (BJJ athletic shirt)",
    "HOOD-01": "heavyweight pullover hoodie",
    "APRON-01": "natural-cotton chef apron with neck strap and waist tie",
    "TEE-01": "short-sleeve unisex T-shirt",
}

LIFESTYLE_SCENE = {
    "bjj": "BJJ academy lobby late afternoon, athlete with folded gi over arm",
    "code": "Tokyo developer cafe, person at MacBook, soft window light",
    "coffee": "specialty espresso bar, person ordering at counter",
    "zen": "minimalist tatami room at dawn, quiet posture",
    "jiuflow": "BJJ tournament side area, person on bench preparing",
    "kokon": "yakiniku restaurant interior, person standing behind the counter, charcoal grill behind",
    "roll": "BJJ academy after roll, towel over shoulder",
}


def product_kind(sku: str) -> str:
    for k in ("HOODIE-BLACK", "LONG-SLEEVE-BLACK", "TEE-BLACK", "TEE-WHITE", "RASH",
             "HOOD-01", "APRON-01", "TEE-01"):
        if k in sku:
            return PRODUCT_DESC[k]
    return "apparel item"


def make_mockup(sku: str, brand: str, design_bytes: bytes, product_url: str, label: str) -> bytes | None:
    product_bytes = fetch_bytes(product_url)
    if not product_bytes:
        return None
    kind = product_kind(sku)
    prompt = f"""High-quality e-commerce product mockup.

Take the FIRST reference image (a blank {kind}) and place the design from the
SECOND reference image (transparent PNG artwork) onto the front of the garment
in a photorealistic way: proper drape, fabric shading, slight folds, matte
print finish, NO hard pasted edges. The garment, background, lighting and
framing should match the FIRST image exactly. The design from the SECOND image
must be sized to fit the standard chest print area (about 25cm wide for tees,
30cm wide for hoodies, full panel for aprons). Output 1024x1024 square JPEG-
quality PNG.

Concept name printed: {label}
Do NOT change the garment color, do NOT add models, do NOT crop.
"""
    return gemini(prompt, [product_bytes, design_bytes])


def make_lifestyle(sku: str, brand: str, design_bytes: bytes, label: str, mockup_bytes: bytes | None) -> bytes | None:
    kind = product_kind(sku)
    scene = LIFESTYLE_SCENE.get(brand, f"editorial {brand} setting")
    prompt = f"""Editorial lifestyle photograph, NOT a product flat-lay.

Subject: Japanese person 20s-30s wearing the {kind} that has the printed design from the reference image
(matching concept "{label}") on the front. The design must be visible and recognizable.

Scene: {scene}.

Style: photojournalistic 35mm, magazine cover quality, natural light, soft
depth-of-field, slightly desaturated, 3:4 portrait composition with subject
mid-frame. NOT a studio flat lay.

Output 1024x1024 photographic PNG.
"""
    refs = [design_bytes]
    if mockup_bytes:
        refs.append(mockup_bytes)
    return gemini(prompt, refs)


def main():
    conn = sqlite3.connect(str(DB))
    out: dict[str, dict] = {}
    if OUT_MAP.exists():
        try:
            out = json.loads(OUT_MAP.read_text())
        except Exception:
            pass

    for i, sku in enumerate(THE_TEN, start=1):
        print(f"\n[{i}/{len(THE_TEN)}] {sku}")
        r = conn.execute(
            "SELECT brand, label, description_ja, printful_variant_id FROM catalog_products WHERE sku=?",
            (sku,)).fetchone()
        if not r:
            print(f"  NOT FOUND in db")
            continue
        brand, label, desc, vid = r
        cid = extract_concept(sku)
        # design bytes
        dpath = design_path(brand, cid)
        if not dpath.exists():
            # try generic
            for cand in [
                ROOT / "store" / "static" / brand / "d" / f"design_{sku}.png",
                ROOT / "designs" / f"{brand}_{cid}.png",
            ]:
                if cand.exists():
                    dpath = cand; break
        if not dpath.exists():
            print(f"  ✗ no design file for {brand}/{cid}")
            continue
        design_bytes = dpath.read_bytes()
        print(f"  design: {dpath.relative_to(ROOT)} ({len(design_bytes):,}B)")

        # mockup
        mockup_path = ROOT / "store" / "static" / brand / "m" / f"perfect_{sku}.jpg"
        mockup_path.parent.mkdir(parents=True, exist_ok=True)
        if mockup_path.exists() and mockup_path.stat().st_size > 50_000 and not os.environ.get("FORCE"):
            print(f"  mockup: skip (exists)")
            mockup_bytes = mockup_path.read_bytes()
        else:
            product_url = PRINTFUL_VARIANTS.get(str(vid))
            if not product_url:
                print(f"  ✗ no printful product url for variant {vid}")
                mockup_bytes = None
            else:
                print(f"  generating mockup via Gemini…")
                mockup_bytes = make_mockup(sku, brand, design_bytes, product_url, label or "")
                if mockup_bytes:
                    mockup_path.write_bytes(mockup_bytes)
                    print(f"    ✓ → {mockup_path.relative_to(ROOT)} ({len(mockup_bytes):,}B)")
                else:
                    print(f"    ✗ Gemini failed")
        time.sleep(1.5)

        # lifestyle
        life_path = ROOT / "store" / "static" / brand / "lifestyle" / f"perfect_{sku}.jpg"
        life_path.parent.mkdir(parents=True, exist_ok=True)
        if life_path.exists() and life_path.stat().st_size > 80_000 and not os.environ.get("FORCE"):
            print(f"  lifestyle: skip (exists)")
        else:
            print(f"  generating lifestyle via Gemini…")
            life_bytes = make_lifestyle(sku, brand, design_bytes, label or "", mockup_bytes)
            if life_bytes:
                life_path.write_bytes(life_bytes)
                print(f"    ✓ → {life_path.relative_to(ROOT)} ({len(life_bytes):,}B)")
            else:
                print(f"    ✗ Gemini failed")
        time.sleep(1.5)

        out[sku] = {
            "design": dpath.as_uri(),
            "mockup": mockup_path.as_uri() if mockup_path.exists() else None,
            "lifestyle": life_path.as_uri() if life_path.exists() else None,
        }
        OUT_MAP.write_text(json.dumps(out, indent=2))

    conn.close()
    print(f"\nwrote map → {OUT_MAP}  ({len(out)} entries)")


if __name__ == "__main__":
    main()
