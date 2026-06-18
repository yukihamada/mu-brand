#!/usr/bin/env python3
"""TAXIGEN generator — pulls data via fetchers, asks Gemini for a tee design,
saves PNG + DB row. Idempotent per (brand, drop_num).

Usage:
    python3 scripts/taxigen/generator.py metro     # generate one METRO tee
    python3 scripts/taxigen/generator.py weather   # one WEATHER tee
    python3 scripts/taxigen/generator.py haneda    # one HANEDA tee
    python3 scripts/taxigen/generator.py all       # all 3 (only enabled ones)

Each pattern has its own brand:
  - taxigen_metro
  - taxigen_weather
  - taxigen_haneda
"""
import os, sys, hashlib, sqlite3
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

sys.path.insert(0, str(Path(__file__).resolve().parent))
from fetchers import PATTERNS  # noqa: E402

from google import genai
from google.genai import types

ROOT = Path(__file__).resolve().parent.parent.parent
DB = ROOT / "store" / "products.db"
OUT_DIR = ROOT / "store" / "static" / "ads"
OUT_DIR.mkdir(parents=True, exist_ok=True)
GEMINI_MODEL = "gemini-3-pro-image-preview"

PATTERN_TO_BRAND = {
    "metro":   "taxigen_metro",
    "weather": "taxigen_weather",
    "haneda":  "taxigen_haneda",
}

PATTERN_TO_LABEL = {
    "metro":   "TAXIGEN · METRO",
    "weather": "TAXIGEN · WEATHER",
    "haneda":  "TAXIGEN · HANEDA",
}


def build_prompt(data: dict) -> str:
    """Same visual language across all 3 patterns so they read as one TAXIGEN
    series. Accent color varies per snapshot's data (delay = orange, rain =
    blue, etc.)."""
    return f"""You are an apparel graphic designer for MU × 日本交通 collab line "TAXIGEN".
Series direction: data-vis poster aesthetic. Black t-shirt print.

CRITICAL OUTPUT (mandatory):
- PNG with REAL alpha channel (RGBA mode), NOT a faked checker pattern.
- Background pixels MUST be alpha = 0 (true transparent).
- 2940×2940 square, DTG print-ready.
- Only design elements visible; everything else fully transparent.

Visual rules:
- Bold mono-spaced or grotesque typography
- Color palette: white (#FFFFFF) + accent {data['design_color']}
- Layout: kicker top → large title middle → metric value (huge) below → label small
- Add: a small ━◯━ MU monogram + small text "{PATTERN_TO_LABEL[data['pattern']]} · 公開データ"
- A single thin accent-color rule line as a 'meter'

This snapshot:
- KICKER (small top):     {data['kicker']}
- TITLE (kanji big):      {data['title_jp']}
- TITLE (latin small):    {data['title_en']}
- METRIC LABEL (small):   {data['metric_label']}
- METRIC VALUE (HUGE):    {data['metric_value']}
{f"- SUBLINE (footer):       {data.get('subline','')}" if data.get('subline') else ''}

All five data points must be visible, arranged in clean broadcast-board hierarchy.
Output: ONE 2940×2940 RGBA transparent print-ready PNG. No checker pattern."""


def gen_one_for_pattern(pattern: str, force_new_drop: bool = True) -> tuple:
    """Returns (drop_num, png_path) or (None, None) on failure."""
    if pattern not in PATTERNS:
        print(f"  ✗ unknown pattern: {pattern}")
        return None, None

    fetcher = PATTERNS[pattern]
    data = fetcher()
    brand = PATTERN_TO_BRAND[pattern]

    db = sqlite3.connect(DB)
    max_drop = db.execute(
        "SELECT COALESCE(MAX(drop_num), 0) FROM products WHERE brand=?", (brand,)
    ).fetchone()[0]
    drop_num = max_drop + 1 if force_new_drop else max(1, max_drop)

    prompt = build_prompt(data)
    print(f"  ↻ generating {brand} #{drop_num}: {data['title_jp']} {data['metric_value']}")

    client = genai.Client(api_key=os.environ["GEMINI_API_KEY"])
    try:
        resp = client.models.generate_content(
            model=GEMINI_MODEL,
            contents=prompt,
            config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"]),
        )
        img_bytes = None
        for cand in resp.candidates or []:
            for part in (cand.content.parts if cand.content else []):
                if getattr(part, "inline_data", None) and part.inline_data.data:
                    img_bytes = part.inline_data.data
                    break
            if img_bytes:
                break
        if not img_bytes:
            print(f"  ✗ no image returned")
            return None, None
    except Exception as e:
        print(f"  ✗ Gemini err: {e}")
        return None, None

    h = hashlib.sha256(img_bytes).hexdigest()[:8]
    fname = f"{brand}_{drop_num:03d}_{h}.png"
    fpath = OUT_DIR / fname
    fpath.write_bytes(img_bytes)
    rel = f"/static/ads/{fname}"
    name = f"{PATTERN_TO_LABEL[pattern]} · {data['title_jp']} · {data['metric_value']}"
    now = datetime.now().isoformat()

    db.execute(
        """INSERT INTO products
        (brand, drop_num, name, price_jpy, inventory, sold, created_at, active,
         city_slug, prompt_text, serial_code, design_url, mockup_url)
        VALUES (?, ?, ?, 5000, 1, 0, ?, 0, 'tokyo', ?, ?, ?, ?)""",
        (brand, drop_num, name, now, prompt[:500],
         f"{brand.upper()}-{drop_num:03d}", rel, rel),
    )
    db.commit()
    print(f"  ✓ saved id=(new) {brand} #{drop_num} → {rel} ({len(img_bytes):,} bytes)")
    return drop_num, rel


def main():
    target = sys.argv[1] if len(sys.argv) > 1 else "all"
    if target == "all":
        for p in PATTERNS:
            gen_one_for_pattern(p)
    elif target in PATTERNS:
        gen_one_for_pattern(target)
    else:
        sys.exit(f"unknown pattern '{target}', use: {list(PATTERNS) + ['all']}")


if __name__ == "__main__":
    main()
