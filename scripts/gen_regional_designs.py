#!/usr/bin/env python3
"""
Generate region-specific T-shirt mockups via Gemini 3 Pro Image.

Strategy: 6 city editions × 2 price points × MU brand grammar.
Each design is a minimalist mark unique to the city's identity, rendered
ON a cream cotton T-shirt at 1024×1024 so the user can immediately see
the final wearable. Saved to /tmp/mu_regional/<city>.jpg + an index.html
viewer.

Run:
  python scripts/gen_regional_designs.py
  open /tmp/mu_regional/index.html
"""
from __future__ import annotations
import base64
import json
import os
import sys
import urllib.request
import urllib.error
from pathlib import Path

ENV_FILE = Path("/Users/yuki/.env")
OUT_DIR = Path("/tmp/mu_regional")
OUT_DIR.mkdir(parents=True, exist_ok=True)

# Load env
env = {}
if ENV_FILE.exists():
    for line in ENV_FILE.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line: continue
        k, v = line.split("=", 1)
        env[k.strip()] = v.strip().strip('"').strip("'")

API_KEY = env.get("GEMINI_API_KEY") or os.environ.get("GEMINI_API_KEY")
if not API_KEY:
    sys.exit("missing GEMINI_API_KEY in /Users/yuki/.env or env")

# Strict per CLAUDE.md: gemini-3-pro-image-preview ONLY.
MODEL = "gemini-3-pro-image-preview"

# 6 city editions. Each design intentionally restrained — MU "numbers over
# adjectives" + "quiet confidence". The lat/lon and city name in white on
# cream are MU brand grammar; the abstract mark is the city's identity.
CITIES = [
    {
        "slug": "tokyo",
        "name_jp": "東京",
        "name_en": "TOKYO",
        "coords": "35.6762°N · 139.6503°E",
        "mark": "5 horizontal lines of varying length stacked vertically — representing the staggered skyline density of central Tokyo skyscrapers at twilight",
        "tagline": "都市の密度 / urban density",
    },
    {
        "slug": "kyoto",
        "name_jp": "京都",
        "name_en": "KYOTO",
        "coords": "35.0116°N · 135.7681°E",
        "mark": "a single slightly imperfect hand-drawn circle (moss-like, asymmetric) representing the rock garden silence of 龍安寺 (Ryoan-ji). White ink only, no fill.",
        "tagline": "石庭の沈黙 / stone garden silence",
    },
    {
        "slug": "osaka",
        "name_jp": "大阪",
        "name_en": "OSAKA",
        "coords": "34.6937°N · 135.5023°E",
        "mark": "a horizontal wavy line crossed by 8 small vertical strokes — the 8 bridges of 中之島 over the 大川 river, abstract minimalist topography",
        "tagline": "八百八橋 / eight hundred bridges",
    },
    {
        "slug": "sapporo",
        "name_jp": "札幌",
        "name_en": "SAPPORO",
        "coords": "43.0618°N · 141.3545°E",
        "mark": "a single 6-pointed snowflake mark, very small (~1.5 inch print size), with thin clean radial lines, slightly off-center to leave breathing room",
        "tagline": "雪の結晶 / snow crystal",
    },
    {
        "slug": "fukuoka",
        "name_jp": "福岡",
        "name_en": "FUKUOKA",
        "coords": "33.5904°N · 130.4017°E",
        "mark": "two parallel horizontal lines representing the 玄界灘 (sea) and 博多湾 (bay) coastlines, with a single small triangle marking a 屋台 (yatai food stall) between them",
        "tagline": "海と屋台 / sea and yatai",
    },
    {
        "slug": "okinawa",
        "name_jp": "沖縄",
        "name_en": "OKINAWA",
        "coords": "26.2124°N · 127.6809°E",
        "mark": "a single thin curved line — a southern breeze (南風 paikaji) flowing diagonally, with one small open circle at the line's start representing 月桃 (alpinia flower)",
        "tagline": "南風 / paikaji",
    },
]


PROMPT_TEMPLATE = """A high-quality lifestyle product photograph of a heavyweight cream-colored / off-white cotton T-shirt (Bella+Canvas 3001 Soft Cream tone), laid flat on a soft warm neutral background (light beige paper or natural wood). The T-shirt is the ONLY product in frame, centered, slightly above the horizontal midline so the print area is most visible.

Centered on the chest area of the shirt, printed in white ink (DTG print), is the following minimalist design — and ONLY this design, nothing else:

TOP (small white sans-serif text, ~8mm tall on the shirt):
  {name_en}
  {name_jp}

CENTER (the visual mark — clean white line art, ~12cm tall, restrained, museum-quality minimalism):
  {mark}

BOTTOM (very small white monospaced numeric coordinates, ~4mm tall):
  {coords}

CRITICAL CONSTRAINTS — follow exactly:
- Print is white ink only on cream fabric (legible but soft contrast — NOT pure white, slight ivory tone).
- No other text, no logos, no MU branding elsewhere on the shirt.
- The shirt is a relaxed-fit unisex tee; sleeves visible; no model wearing it.
- Lighting is soft, natural, slightly directional from upper left.
- Background is matte and uncluttered — only one shadow under the shirt.
- 1:1 aspect ratio, hi-resolution photographic realism.
- Style references: Aesop product photography + COS minimalism + traditional 京焼 catalog quality.
- Do NOT add any patterns, watermarks, color tags, hangtags, or accessories.

This is the "{slug}" edition of MU regional T-shirts. Concept: {tagline}.
"""


def gen_one(city: dict) -> Path:
    out_path = OUT_DIR / f"{city['slug']}.png"
    if out_path.exists() and out_path.stat().st_size > 100_000:
        print(f"  ✓ cached: {out_path.name} ({out_path.stat().st_size} bytes)")
        return out_path

    prompt = PROMPT_TEMPLATE.format(**city)
    payload = {
        "contents": [{"parts": [{"text": prompt}]}],
        "generationConfig": {"responseModalities": ["IMAGE", "TEXT"]},
    }
    url = (f"https://generativelanguage.googleapis.com/v1beta/models/"
           f"{MODEL}:generateContent?key={API_KEY}")
    req = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=180) as resp:
            data = json.loads(resp.read())
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", errors="replace")[:400]
        print(f"  ✗ {city['slug']}: HTTP {e.code}: {body}")
        return out_path
    except Exception as e:
        print(f"  ✗ {city['slug']}: {type(e).__name__}: {e}")
        return out_path

    # Extract inline_data
    cand = (data.get("candidates") or [{}])[0]
    parts = (cand.get("content") or {}).get("parts") or []
    inline = None
    for p in parts:
        ind = p.get("inline_data") or p.get("inlineData")
        if ind and ind.get("data"):
            inline = ind
            break
    if not inline:
        print(f"  ✗ {city['slug']}: no image in response. keys={list(cand.keys())}")
        # Save debug
        (OUT_DIR / f"{city['slug']}_debug.json").write_text(json.dumps(data, ensure_ascii=False, indent=2)[:4000])
        return out_path

    img_bytes = base64.b64decode(inline["data"])
    out_path.write_bytes(img_bytes)
    print(f"  ✓ {city['slug']}: {len(img_bytes)} bytes → {out_path.name}")
    return out_path


def build_gallery():
    rows = []
    for city in CITIES:
        path = OUT_DIR / f"{city['slug']}.png"
        if not path.exists() or path.stat().st_size < 50_000:
            ok = False
            img_html = '<div style="height:340px;background:#1a1a1a;display:flex;align-items:center;justify-content:center;color:#666;font-size:12px">(no image)</div>'
        else:
            ok = True
            img_html = f'<img src="{path.name}" style="width:100%;display:block">'
        rows.append(f"""
        <div class="card">
          {img_html}
          <div class="meta">
            <div class="cap">{city['name_en']} EDITION</div>
            <div class="ttl">{city['name_jp']} — {city['tagline']}</div>
            <div class="coords">{city['coords']}</div>
            <div class="prices">
              <span>¥4,900 entry</span> · <span>¥6,800 standard</span>
            </div>
          </div>
        </div>
        """)

    html = f"""<!doctype html><html lang="ja"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>MU Regional Editions — review</title>
<style>
body{{background:#0A0A0A;color:#F5F5F0;font-family:-apple-system,'Helvetica Neue','Hiragino Sans',Arial,sans-serif;margin:0;padding:32px 24px;line-height:1.7}}
h1{{font-size:24px;font-weight:300;letter-spacing:0.04em;margin:0 0 8px}}
p.lede{{color:rgba(245,245,240,0.62);font-size:14px;margin:0 0 32px;max-width:680px}}
.grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(320px,1fr));gap:18px;max-width:1320px;margin:0 auto}}
.card{{background:#141414;border:1px solid rgba(255,255,255,0.08);border-radius:4px;overflow:hidden}}
.card img{{aspect-ratio:1/1;object-fit:cover;background:#1a1a1a}}
.meta{{padding:20px 22px}}
.cap{{font-size:10px;letter-spacing:0.4em;color:#e6c449;text-transform:uppercase;margin-bottom:8px}}
.ttl{{font-size:17px;font-weight:400;margin-bottom:8px}}
.coords{{font-family:'SF Mono','Menlo',monospace;font-size:11px;color:rgba(245,245,240,0.55);margin-bottom:12px}}
.prices{{font-size:12px;color:#e6c449;letter-spacing:0.06em}}
</style></head><body>
<h1>━◯━ MU Regional Editions — 内部レビュー</h1>
<p class="lede">6 都市 × 2 価格 (¥4,900 / ¥6,800) = 12 SKU 候補。 採用したもののみ products に投入 + 地域 geo-targeted Google Ads を spawn します。</p>
<div class="grid">{''.join(rows)}</div>
</body></html>"""
    (OUT_DIR / "index.html").write_text(html)
    print(f"\n✓ gallery: file://{OUT_DIR}/index.html")


if __name__ == "__main__":
    print(f"generating {len(CITIES)} regional designs...")
    for city in CITIES:
        gen_one(city)
    build_gallery()
