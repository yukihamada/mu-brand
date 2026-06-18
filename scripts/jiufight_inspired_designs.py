#!/usr/bin/env python3
"""Generate 4 new T-shirt designs inspired by jiufight.jp's design language.

Source of aesthetic:
- Event: SUPER YAWARA SWEEP CUP, 2026-05-24, 芝公園 (港区)
- Format: 4セット, 色帯 2分×3R, 白帯 3分×1, 早慶戦 + 対抗戦
- Palette: near-monochrome (white/black), minimal red accent
- Tone: minimalist, sober, bilingual JP/EN, technical

Each design is original — only factual event metadata (date, name) is reused;
no specific layout, logo, or graphic is copied from the source site.
"""
import os, sys, base64, json, urllib.request, urllib.error, subprocess
from pathlib import Path

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY:
    sys.exit("GEMINI_API_KEY missing (source /Users/yuki/.env first)")

OUT = Path("/Users/yuki/workspace/mu-brand/store/static/jiufight/inspired")
OUT.mkdir(parents=True, exist_ok=True)

DESIGNS = [
    ("01_datestamp", """T-shirt graphic design, original work, large minimalist date typography.
The numerals 2026.05.24 occupy 60% of a square 1024×1024 canvas, set in a very thin
geometric sans-serif font, pure white on a fully transparent background. Below the
date, a tiny line of letterspaced text 'SUPER YAWARA SWEEP CUP · TOKYO'. A single
narrow horizontal hairline crosses the design at mid-height. Composition is sober,
quiet, restrained — no decorative flourish, no gradients, no shadows. Pure flat
white shapes only. Output: 1024×1024 PNG, transparent background, white ink only
(will print on a black T-shirt). Crisp vector-like edges."""),

    ("02_bracket", """T-shirt graphic design, original work, abstract single-elimination tournament
bracket. Eight horizontal lines on the left collapsing through three rounds into one
champion line on the right. All lines pure white, thin, geometric. The bracket
occupies 70% of a 1024×1024 square canvas. Tiny letterspaced caption above:
'TEAM MATCHES · 2026'. No names, no scores. Pure white linework on transparent
background, no shading. Sober, restrained, architectural. Output 1024×1024 PNG."""),

    ("03_kanji_seal", """T-shirt graphic design, original work, single bold sumi-ink brushstroke
of the kanji 柔 (yawara — softness, flexibility). Confident, one-stroke calligraphy
style — quiet authority, not aggressive. Pure white on transparent background.
Below it, very small letterspaced text: '2026·05·24 TOKYO'. Composition centered,
柔 occupies ~55% of a 1024×1024 canvas. The brushstroke has subtle dry-edge
texture but no color variation. Sober, traditional restraint. Output 1024×1024 PNG."""),

    ("04_versus_bars", """T-shirt graphic design, original work, vertical bar chart suggesting
opposing teams. Two stacks of horizontal bars facing each other across a central
vertical hairline — left stack 5 bars descending in length, right stack 5 bars
descending mirror-style. All bars and lines pure white, geometric, thin. A tiny
letterspaced caption beneath: 'EAST vs WEST · 2026'. Composition occupies ~65% of
a 1024×1024 canvas, transparent background. Strict minimal palette, white only.
Sober, almost technical-drawing feel. Output 1024×1024 PNG, transparent."""),
]

def generate(slug: str, prompt: str) -> bool:
    url = f"https://generativelanguage.googleapis.com/v1beta/models/gemini-3-pro-image-preview:generateContent?key={KEY}"
    body = json.dumps({
        "contents": [{"parts": [{"text": prompt}]}],
        "generationConfig": {"responseModalities": ["IMAGE", "TEXT"]}
    }).encode()
    req = urllib.request.Request(url, data=body, headers={"content-type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=180) as r:
            j = json.loads(r.read())
    except urllib.error.HTTPError as e:
        msg = e.read().decode(errors="replace")[:300]
        print(f"  [HTTP {e.code}] {slug}: {msg}")
        return False
    except Exception as e:
        print(f"  [err] {slug}: {e}")
        return False
    # extract inline_data
    parts = j.get("candidates", [{}])[0].get("content", {}).get("parts", [])
    for p in parts:
        d = p.get("inlineData") or p.get("inline_data")
        if d and d.get("data"):
            png = base64.b64decode(d["data"])
            out = OUT / f"{slug}.png"
            out.write_bytes(png)
            print(f"  ✓ {slug} → {out} ({len(png):,}B)")
            return True
    print(f"  [empty] {slug}: no inline_data in response")
    return False

print(f"generating {len(DESIGNS)} designs → {OUT}/")
ok_files = []
for slug, prompt in DESIGNS:
    if generate(slug, prompt):
        ok_files.append(OUT / f"{slug}.png")

print(f"\ndone. {len(ok_files)}/{len(DESIGNS)} succeeded.")
if ok_files:
    subprocess.run(["open"] + [str(p) for p in ok_files])
