#!/usr/bin/env python3
"""TAXIGEN prototype: 12 sample tee designs from synthesized Tokyo pickup data.

Each sample = a different hour + Tokyo area + estimated pickup count.
Real production cron would pull from Mobility Tech aggregates; for the
pitch we use synthesized but plausible values so Kawanabe-san can imagine
the cadence.
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

# 24 mock data points — one per hour of a representative Tokyo day.
# Format: (hour, area_jp, area_en, pickup_count, vibe)
SAMPLES = [
    (1,  "歌舞伎町",  "KABUKICHO",   1842, "after-club"),
    (5,  "築地",      "TSUKIJI",      287, "early-shift"),
    (8,  "丸の内",    "MARUNOUCHI",  3104, "morning-rush"),
    (10, "羽田 第二", "HND T2",       912, "tourist-arrival"),
    (12, "銀座 六丁目","GINZA 6",    1521, "lunch-meet"),
    (15, "渋谷 道玄坂","DOGENZAKA",   876, "afternoon-cruise"),
    (17, "兜町",      "KABUTO-CHO",  1208, "market-close"),
    (19, "新宿 三丁目","SHINJUKU 3",  2304, "evening-peak"),
    (21, "六本木",    "ROPPONGI",    1647, "after-dinner"),
    (22, "東京駅 八重洲", "YAESU",   1089, "last-train"),
    (23, "西麻布",    "NISHI-AZABU",  734, "late-night"),
    (0,  "中目黒",    "NAKA-MEGURO",  421, "midnight-cruise"),
]

SYSTEM_TMPL = """You are an apparel graphic designer for MU × 日本交通 collab line "TAXIGEN".
Each design represents one snapshot of Tokyo taxi pickup density at a specific hour.

CRITICAL OUTPUT:
- PNG with REAL alpha channel (RGBA), NOT a faked checker pattern.
- Background pixels MUST be alpha=0 (true transparent).
- Only design elements visible. Optimized for printing on a BLACK t-shirt.
- 2940×2940 square, DTG print-ready.

Design language (consistent across the whole TAXIGEN series):
- Information-graphic / data-vis aesthetic
- Bold mono-spaced or grotesque typography (white + 1 warm-yellow accent #F4B71A)
- Layout: AREA name dominant top, time in middle, pickup count bottom, MU/TAXIGEN wordmark small
- One subtle ━◯━ MU monogram somewhere
- One small horizontal accent line in warm yellow as a 'meter rule'
- Optional: a tiny abstract taxi-rooftop-light icon as a recurring brand mark

This snapshot:
- AREA (kanji): {area_jp}
- AREA (latin): {area_en}
- TIME: {hour:02d}:00
- PICKUP COUNT estimate: {pickup_count}
- VIBE (do not write this on the shirt): {vibe}

The four data points (area-jp, area-en, time, count) must all appear,
arranged in a clean broadcast-board / departures-board hierarchy.

Output: ONE 2940×2940 RGBA print-ready PNG."""


def gen_one(client, brief):
    resp = client.models.generate_content(
        model=GEMINI_MODEL,
        contents=brief,
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
    for i, (hour, area_jp, area_en, count, vibe) in enumerate(SAMPLES, 1):
        prompt = SYSTEM_TMPL.format(
            area_jp=area_jp, area_en=area_en, hour=hour, pickup_count=count, vibe=vibe
        )
        name = f"TAXIGEN · {area_jp} {hour:02d}:00 · {count} pickup"
        print(f"  ↻ #{i} {area_jp} {hour:02d}:00")
        try:
            img = gen_one(client, prompt)
        except Exception as e:
            print(f"  ✗ {e}")
            continue
        h = hashlib.sha256(img).hexdigest()[:8]
        fname = f"taxigen_{i:03d}_{h}.png"
        (OUT_DIR / fname).write_bytes(img)
        rel = f"/static/ads/{fname}"
        existing = db.execute(
            "SELECT id FROM products WHERE brand='taxigen' AND drop_num=?", (i,)
        ).fetchone()
        if existing:
            db.execute(
                "UPDATE products SET name=?, design_url=?, mockup_url=?, prompt_text=?, active=0 WHERE id=?",
                (name, rel, rel, prompt[:500], existing[0]),
            )
            print(f"  ↑ updated id={existing[0]} ({len(img):,} bytes)")
        else:
            db.execute(
                """INSERT INTO products
                (brand, drop_num, name, price_jpy, inventory, sold, created_at, active,
                 city_slug, prompt_text, serial_code, design_url, mockup_url)
                VALUES ('taxigen', ?, ?, 5000, 1, 0, ?, 0, 'tokyo', ?, ?, ?, ?)""",
                (i, name, now, prompt[:500], f"TAXIGEN-{i:03d}", rel, rel),
            )
            print(f"  ✓ inserted ({len(img):,} bytes) → {rel}")
        db.commit()  # commit per-row so killed scripts don't lose progress
    print("\n✅ TAXIGEN samples generated (active=0, brand=taxigen)")


if __name__ == "__main__":
    main()
