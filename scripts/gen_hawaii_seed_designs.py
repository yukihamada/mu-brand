#!/usr/bin/env python3
"""Generate MU-original Hawaii-festival "seed shirt" designs (gift/seeding
campaign). RIGHTS-SAFE: pure MU marks (無 / 月 / ━◯━), MU's own festival —
NO third-party band name, logo, likeness, or lyrics. Print-ready transparent
2940x2940 PNGs. No hard date baked in (10/28 vs 10/29 unresolved upstream).

Usage: python3 scripts/gen_hawaii_seed_designs.py
Outputs: store/static/festseed/seed-<key>.png  +  opens them.
"""
import os, sys
from pathlib import Path

# Force-load /Users/yuki/.env (zshrc GEMINI key revoked — feedback_gemini_key_env)
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
OUT = ROOT / "store" / "static" / "festseed"
OUT.mkdir(parents=True, exist_ok=True)
MODEL = "gemini-3-pro-image-preview"

SYSTEM = """You are the lead apparel graphic designer for the MU brand (wearmu.com).
MU's marks are minimal and symbolic: the kanji 無 (mu = "nothing/zero"), the
kanji 月 (moon), and the wordmark ━◯━ (a circle flanked by two short bars).
The aesthetic is quiet, confident, monochrome-plus-one-accent, lots of negative
space. Think Japanese minimalism meets a single warm Hawaiian sunset gold.

Produce ONE square 2940x2940 PNG, TRANSPARENT background, ready for
direct-to-garment printing.

Strict rules:
- Flat solid shapes, MAX 3 colors: off-white (#f2f2ee), warm gold (#f5b142),
  and optionally a deep ink (#0a0a0a) for fine detail.
- NO photographic background, NO heavy gradients, NO mesh, NO drop shadows.
- NO real faces. NO third-party logos, band names, song titles, or any
  trademark that is not MU's own. This is an MU-original design.
- Center it, keep >=12% padding from every edge, legible at 4cm.

Design brief: {brief}

Output: ONE print-ready transparent graphic, nothing else."""

DESIGNS = {
 "monogram": "The core MU imprint: the ━◯━ wordmark large and centered, with the "
   "kanji 無 set quietly inside or just above the circle. Pure brand mark, the "
   "thing you want people to subconsciously recognize after seeing it many times. "
   "Off-white on transparent, a single thin gold ring as the ◯.",
 "moon": "A clean crescent 月 (moon) rendered as a single off-white arc sitting "
   "above one calm horizontal line that reads as a Pacific horizon / wave. Far "
   "below, tiny: the ━◯━ mark. Spare, meditative, a Hawaiian night. Gold only on "
   "the thin horizon line.",
 "extra": "A bold abstract 'EXTRA / 号外' broadsheet motif — a narrow vertical "
   "newspaper column rule with the words 'MU FESTIVAL' and 'HAWAII' set in a heavy "
   "condensed sans, stacked, like a special-edition headline announcing a "
   "gathering. No date, no names of performers. Off-white text, one gold rule.",
 "key": "The campaign idea 'this shirt is the key': a stylized square QR-code-like "
   "glyph whose center module dissolves into the MU ━◯━ circle, suggesting a key / "
   "an unlock. Geometric, flat, off-white modules with the central ◯ in gold. "
   "Abstract — not a scannable real QR.",
}

def gen(client, brief):
    resp = client.models.generate_content(
        model=MODEL, contents=SYSTEM.format(brief=brief),
        config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"]),
    )
    for cand in resp.candidates or []:
        for part in (cand.content.parts if cand.content else []):
            if getattr(part, "inline_data", None) and part.inline_data.data:
                return part.inline_data.data
    raise RuntimeError("no image returned")

def main():
    if not os.environ.get("GEMINI_API_KEY"):
        sys.exit("GEMINI_API_KEY not set (check /Users/yuki/.env)")
    client = genai.Client(api_key=os.environ["GEMINI_API_KEY"])
    made = []
    for key, brief in DESIGNS.items():
        try:
            data = gen(client, brief)
            p = OUT / f"seed-{key}.png"
            p.write_bytes(data)
            print(f"  ✓ {p.relative_to(ROOT)}  ({len(data)//1024} KB)")
            made.append(str(p))
        except Exception as e:
            print(f"  ✗ {key}: {e}")
    if made:
        os.system("open " + " ".join(f"'{m}'" for m in made))
        print(f"opened {len(made)} designs")

if __name__ == "__main__":
    main()
