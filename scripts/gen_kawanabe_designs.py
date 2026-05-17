#!/usr/bin/env python3
"""One-shot: generate 3 candidate T-shirt designs for 川鍋一朗-san of 日本交通.

Tokyo taxi motifs, trademark-safe (no Nihon Kotsu logo, no GO app logo).
Outputs:
  - store/static/ads/kawanabe_<n>_<hash>.png
  - rows in store/products.db under brand='kawanabe_personal'
"""
import os, sys, hashlib
from datetime import datetime
from pathlib import Path

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
import sqlite3

ROOT = Path(__file__).resolve().parent.parent
DB = ROOT / "store" / "products.db"
OUT_DIR = ROOT / "store" / "static" / "ads"
OUT_DIR.mkdir(parents=True, exist_ok=True)

GEMINI_MODEL = "gemini-3-pro-image-preview"

SYSTEM_TMPL = """You are a professional apparel graphic designer for MU (wearmu.com).
Produce a single T-shirt print design as a square 2940×2940 PNG.

CRITICAL OUTPUT FORMAT:
- PNG must have a REAL alpha channel (RGBA mode).
- Background pixels MUST be fully transparent (alpha = 0).
- DO NOT draw a checkerboard / grid / transparency-indicator pattern
  into the image. The background should be true empty pixels, not a
  visual representation of transparency.
- Only the design elements (text, shapes, icons) should have visible
  pixels. Everything else = invisible/transparent.

Optimized for direct-to-garment printing on a BLACK t-shirt (so the
tee color shows through wherever you leave pixels transparent).

This is a COLLABORATION with a Tokyo taxi heritage theme. The MU
co-branding MUST be visible somewhere in the design:
  - Either the literal text 'MU ×' (MU cross) preceding the main motif
  - OR a small 'by MU' line tucked under the main element
  - OR the '━◯━' MU monogram (a horizontal line, then a circle outline,
    then another horizontal line, all on a single baseline) integrated
    subtly into the composition
Choose whichever fits the design best. Make sure a viewer immediately
sees this is a MU collab piece — not a generic Tokyo taxi shirt.

Strict rules:
- Solid flat shapes, max 3 colors total, high contrast
- NO photographic backgrounds, NO gradients except subtle
- NO realistic faces, NO trademarked logos (no '日本交通', no 'GO', no specific company marks)
- Center the main motif — 10% padding minimum from edges
- Text legible at 4cm tall

Design brief: {brief}

Output: ONE square print-ready graphic, transparent background, MU co-brand visible."""

CANDIDATES = [
    {
        "n": 1,
        "name": "TOKYO TAXI 1928 — Heritage Badge",
        "brief": (
            "Vintage license-plate style badge: bold serif typography "
            "'TOKYO TAXI / 1928' arranged centered, framed in a rounded "
            "rectangle border. Deep navy (#0E2A4A) frame with cream (#F5F0E0) "
            "inner background and warm yellow (#F4B71A) accent date. "
            "Single horizontal silhouette of a classic Tokyo sedan-style "
            "taxi (generic — not any specific brand) at the bottom of the "
            "badge, in deep navy. Vintage stamp/postcard aesthetic, slightly "
            "distressed edges. NO brand names, NO logos, NO faces."
        ),
    },
    {
        "n": 2,
        "name": "行灯 — ANDON",
        "brief": (
            "Minimalist single-icon design: a stylized geometric "
            "interpretation of a taxi rooftop light (行灯/andon), shaped "
            "like a small rounded rectangular lantern with a glow halo. "
            "Pure black ink linework on transparent, single warm yellow "
            "(#F4B71A) glow fill inside the lantern shape. Below the icon, "
            "small text '行灯 · ANDON' in clean Japanese sans-serif. "
            "Extreme minimalism, single icon centered, lots of negative space. "
            "Feels like a Japanese tea-room tile."
        ),
    },
    {
        "n": 3,
        "name": "四代目 — Fourth Generation",
        "brief": (
            "Heritage typography piece: large vertical Japanese kanji "
            "'四代目' (4-dai-me / 4th generation) in heavy classical brush "
            "ink style, deep black on transparent. Below in small caps "
            "Latin: 'FOURTH GENERATION · 1928→' in clean serif. To the "
            "right of the kanji, a single subtle red wax seal (印鑑 style, "
            "small circle ~15% width) for color accent. Composition: "
            "powerful kanji as the hero element. Like a samurai family "
            "scroll motif but cleaned up modern. NO company names, NO faces."
        ),
    },
    {
        "n": 4,
        "name": "黒タク — KURO TAKU",
        "brief": (
            "Premium ink-and-gold design: bold geometric typography "
            "'黒タク' in heavy modern Japanese sans-serif, all black, "
            "centered top. Below, a single horizontal warm-gold (#C9A24A) "
            "rule line (3px) spanning 40% width. Below the line, smaller "
            "text 'PREMIUM HIRE — TOKYO' in clean serif caps. Single "
            "negative-space crown silhouette (generic, 3-point) above the "
            "kanji in subtle gold. Designed for a black t-shirt — extreme "
            "minimalism, 'expensive whiskey label' aesthetic. NO logos."
        ),
    },
    {
        "n": 5,
        "name": "流し — RUNNER",
        "brief": (
            "Single large vertical kanji '流' (nagasu — to cruise/flow) "
            "in heavy classical brush ink style, deep black, with one "
            "stroke ending in a long abstract trail/flow extending to "
            "the right edge of the canvas (representing motion of a "
            "cruising taxi). Below the kanji, small text 'RUNNER · 流し' "
            "in monospace. Minimalist, sumi-e calligraphy feel. NO faces, "
            "NO logos. Single ink color (black) + 1 thin red accent line "
            "as a horizon below the text."
        ),
    },
    {
        "n": 6,
        "name": "メーター — METER",
        "brief": (
            "Bold digital-readout aesthetic: large 7-segment LCD-style "
            "numbers showing '¥1,928' (1928 yen — homage to the founding "
            "year) in warm amber (#F4B71A) on a black rectangular "
            "backdrop with rounded corners (faux taxi-meter display). "
            "Below the display, small caps text 'TOKYO METER · EST. 1928' "
            "in clean monospace, white. The amber numbers glow slightly. "
            "Composition centered, 70% width. Retro arcade meets taxi-cab. "
            "NO brand names beyond TOKYO. NO logos."
        ),
    },
    {
        "n": 7,
        "name": "おもてなし — OMOTENASHI",
        "brief": (
            "Heritage circle seal design: a single large red circle "
            "(印鑑 wax-seal style, #B0252A) with white reverse-out "
            "vertical kanji 'おもてなし' (omotenashi — hospitality) "
            "inside, in classical script. Around the circle, a thin "
            "black ring of small uppercase Latin text reading "
            "'THE ART OF JAPANESE HOSPITALITY · TOKYO' wrapping the "
            "full circumference. Composition: single seal centered, "
            "60% width. NO brand names beyond TOKYO. NO faces."
        ),
    },
    {
        "n": 8,
        "name": "NEXT STOP — TESHIKAGA",
        "brief": (
            "Bus-stop / route-sign hybrid: bold horizontal typography "
            "'NEXT STOP →' in clean sans-serif (white) on a deep navy "
            "(#0E2A4A) horizontal bar, top half. Below, larger letters "
            "'TESHIKAGA' (the Hokkaido town MU supports), in warm yellow "
            "(#F4B71A) on the same navy backdrop. Below that, small text "
            "'2,300 KM FROM TOKYO · §27 MU' in cream (#F5F0E0). "
            "Tongue-in-cheek — a Tokyo taxi sign pointing impossibly "
            "to Hokkaido. Composition: stacked horizontal bars like a "
            "real route board. NO faces, NO logos."
        ),
    },
    {
        "n": 9,
        "name": "DRIVER 0001",
        "brief": (
            "Industrial number-plate aesthetic: huge bold typography "
            "'0001' in mono digital font, white on deep navy (#0E2A4A) "
            "rectangular plate with rounded corners. Above the number, "
            "small label 'DRIVER' in spaced caps (cream). Below the "
            "number, small label 'SINCE 1928' in spaced caps (cream). "
            "Plate has subtle 4-corner screw heads (small circles). "
            "Composition: single plate centered, 70% width. Feels like "
            "the back of a cab partition. NO brand names. NO faces."
        ),
    },
    {
        "n": 10,
        "name": "東京 / TOKYO — Vertical Wordmark",
        "brief": (
            "Pure typography composition: huge vertical Japanese kanji "
            "'東京' in heavy modern sans-serif, deep black, left side of "
            "canvas. To the right, vertical Latin 'TOKYO' in matching "
            "weight, rotated 90 degrees counter-clockwise, in warm "
            "yellow (#F4B71A). Between them, a single thin black vertical "
            "rule. Below both, small horizontal text '1603 — PRESENT · MU' "
            "in cream serif. Minimalist editorial / fashion-label feel. "
            "NO car silhouettes, NO logos, NO faces. Pure type."
        ),
    },
]


def gen_one(client, brief):
    resp = client.models.generate_content(
        model=GEMINI_MODEL,
        contents=SYSTEM_TMPL.format(brief=brief),
        config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"]),
    )
    for cand in resp.candidates or []:
        for part in (cand.content.parts if cand.content else []):
            if getattr(part, "inline_data", None) and part.inline_data.data:
                return part.inline_data.data
    raise RuntimeError("no image returned")


def main():
    client = genai.Client(api_key=os.environ["GEMINI_API_KEY"])
    db = sqlite3.connect(DB)
    now = datetime.now().isoformat()
    for c in CANDIDATES:
        n, name, brief = c["n"], c["name"], c["brief"]
        print(f"  ↻ generating #{n}: {name}")
        img = gen_one(client, brief)
        h = hashlib.sha256(img).hexdigest()[:8]
        fname = f"kawanabe_{n:03d}_{h}.png"
        (OUT_DIR / fname).write_bytes(img)
        rel = f"/static/ads/{fname}"
        # Upsert into local DB
        existing = db.execute(
            "SELECT id FROM products WHERE brand='kawanabe_personal' AND drop_num=?",
            (n,)
        ).fetchone()
        if existing:
            db.execute(
                "UPDATE products SET name=?, design_url=?, mockup_url=?, prompt_text=?, active=0 WHERE id=?",
                (name, rel, rel, brief, existing[0]),
            )
            print(f"  ↑ updated id={existing[0]} ({len(img):,} bytes) → {rel}")
        else:
            db.execute(
                """INSERT INTO products
                (brand, drop_num, name, price_jpy, inventory, sold, created_at, active,
                 city_slug, prompt_text, serial_code, design_url, mockup_url)
                VALUES (?, ?, ?, 0, 1, 0, ?, 0, 'teshikaga', ?, ?, ?, ?)""",
                ("kawanabe_personal", n, name, now, brief,
                 f"KAWANABE-{n:03d}", rel, rel),
            )
            print(f"  ✓ inserted brand=kawanabe_personal #{n} ({len(img):,} bytes) → {rel}")
    db.commit()
    print(f"\nDone. 3 candidates in store/static/ads/ + store/products.db (brand=kawanabe_personal, active=0)")


if __name__ == "__main__":
    main()
