#!/usr/bin/env python3
"""Re-generate the 8 problematic mockups with stricter prompts.

Issues being fixed:
  1. dark-on-dark invisibility (HOODIE, ZEN) — explicit ink inversion
  2. background watermark / scattered design (BJJ trio) — clean white BG
  3. design overflow / cut off (CODE, JF-HOOD, KK-APRON) — strict print area
  4. typos (ZEN: BLRCK / MIU) — exact text preservation
"""
from __future__ import annotations
import base64, json, os, re, sqlite3, sys, time, urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DB = ROOT / "store" / "products.db"
MAP = Path("/tmp/wearmu_perfect10.json")

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY:
    for line in Path("/Users/yuki/.env").read_text().splitlines():
        if line.startswith(("GEMINI_API_KEY=", "GOOGLE_API_KEY=")):
            KEY = line.split("=", 1)[1].strip().strip("'\"")
            break

PRINTFUL = json.loads(Path("/tmp/wearmu_printful_variants.json").read_text())
MODEL = "gemini-3-pro-image-preview"

TO_REDO = [
    "MU-BJJ-01-LONG-SLEEVE-BLACK-L",
    "MU-BJJ-01-RASH",
    "MU-CODE-01-TEE-BLACK",
]


def extract_concept(sku: str) -> str:
    m = re.match(r"^MU-([A-Z0-9]+)-(\d+)-", sku)
    if m:
        return f"MU-{m.group(1)}-{m.group(2)}"
    return re.sub(r"-(?:XS|S|M|L|XL|2XL|3XL|4XL|one|os)$", "", sku, flags=re.IGNORECASE)


def design_bytes(brand: str, concept: str) -> bytes | None:
    p = ROOT / "store" / "static" / brand / "d" / f"design_{concept}.png"
    return p.read_bytes() if p.exists() else None


def product_bytes(url: str) -> bytes | None:
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0 wearmu/1"})
        with urllib.request.urlopen(req, timeout=20) as r:
            return r.read()
    except Exception as e:
        print(f"  fetch err: {e}")
        return None


PRODUCT_KIND = {
    "MU-BJJ-01-TEE-BLACK": ("black short-sleeve T-shirt", "white"),
    "MU-BJJ-01-HOODIE-BLACK-M": ("black heavyweight pullover hoodie", "white"),
    "MU-BJJ-01-LONG-SLEEVE-BLACK-L": ("black long-sleeve T-shirt", "white"),
    "MU-BJJ-01-RASH": ("white long-sleeve rashguard (BJJ athletic shirt)", "black"),
    "MU-CODE-01-TEE-BLACK": ("black short-sleeve T-shirt", "white"),
    "MU-ZEN-01-TEE-BLACK": ("black short-sleeve T-shirt", "white"),
    "JF-HOOD-01": ("black heavyweight pullover hoodie", "white"),
    "KK-APRON-01": ("natural-cotton chef apron with neck strap and waist tie", "gold-thread embroidered"),
}


def build_prompt(sku: str, label: str) -> str:
    kind, ink = PRODUCT_KIND[sku]
    return f"""E-commerce product mockup. CRITICAL: clean SOLID WHITE background.

REFERENCES:
  Image 1: a {kind} photographed against a plain neutral background. Use its
           garment shape, color, drape exactly.
  Image 2: a transparent PNG of the print artwork. IGNORE the checkered
           transparency pattern visible in Image 2 — that is the file's
           transparent background and must NOT appear in the output.

TASK: produce a single product photograph: the {kind} centered on a SOLID
WHITE #FFFFFF studio background, with the artwork from Image 2 printed on
the FRONT CENTER CHEST.

ABSOLUTE RULES:
  1. BACKGROUND = pure flat WHITE only. NO checkered pattern, NO duplicate
     artwork, NO faded watermark, NO scattered ink, NO decorative shapes.
     Nothing exists behind the garment except plain white.
  2. INK = {ink} on the {kind} so it is CLEARLY READABLE. If dark on dark
     would be invisible, INVERT to white (or to {ink}).
  3. PRINT AREA = the FRONT CHEST PANEL only, well within the garment width.
     The artwork's overall width must be AT MOST 60% of the garment width
     at its widest visible point. NO part of the artwork may touch or pass
     the side seams, sleeves, or be cut off at any edge. Leave clean fabric
     margin on left and right of the print.
  4. TYPOGRAPHY = copy every letter EXACTLY as in the source artwork. Do
     not introduce typos. All letters must be visible (no truncation).
  5. FRAMING = the entire garment is visible, centered, no body parts cut
     off. If a model is shown, show full upper body including head and
     shoulders. Otherwise use a clean ghost-mannequin / flat-lay shot.

Output: 1024x1024 PNG, photoreal, magazine product-shot quality.
"""


def gemini_compose(prompt: str, product_b: bytes, design_b: bytes) -> bytes | None:
    parts = [
        {"text": prompt},
        {"inlineData": {"mimeType": "image/jpeg", "data": base64.b64encode(product_b).decode()}},
        {"inlineData": {"mimeType": "image/png", "data": base64.b64encode(design_b).decode()}},
    ]
    url = f"https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:generateContent?key={KEY}"
    body = json.dumps({
        "contents": [{"parts": parts}],
        "generationConfig": {"responseModalities": ["IMAGE", "TEXT"], "temperature": 0.55},
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


def main():
    conn = sqlite3.connect(str(DB))
    out = json.loads(MAP.read_text())
    for i, sku in enumerate(TO_REDO, start=1):
        print(f"\n[{i}/{len(TO_REDO)}] {sku}")
        r = conn.execute(
            "SELECT brand, label, printful_variant_id FROM catalog_products WHERE sku=?",
            (sku,)).fetchone()
        brand, label, vid = r
        cid = extract_concept(sku)
        d_bytes = design_bytes(brand, cid)
        if not d_bytes:
            print(f"  ✗ no design"); continue
        purl = PRINTFUL.get(str(vid))
        if not purl:
            print(f"  ✗ no product url"); continue
        p_bytes = product_bytes(purl)
        if not p_bytes:
            continue
        prompt = build_prompt(sku, label or "")
        print(f"  generating with stricter prompt…")
        mockup = gemini_compose(prompt, p_bytes, d_bytes)
        if not mockup:
            print(f"  ✗ Gemini failed"); continue
        out_path = ROOT / "store" / "static" / brand / "m" / f"perfect_{sku}.jpg"
        out_path.write_bytes(mockup)
        out[sku]["mockup"] = out_path.resolve().as_uri()
        print(f"  ✓ → {out_path.relative_to(ROOT)} ({len(mockup):,}B)")
        MAP.write_text(json.dumps(out, indent=2))
        time.sleep(1.5)
    conn.close()
    print(f"\ndone. updated map → {MAP}")


if __name__ == "__main__":
    main()
