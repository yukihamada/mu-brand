#!/usr/bin/env python3
"""MU "House Drip" seed-shirt set for the beachside rental (Day 1 -> Day 4).
Asks escalate day by day: silent mark -> witty line -> QR reveal -> take-me-home
+ festival invite. RIGHTS-SAFE: MU-original marks only (無 / 月 / ━◯━), MU's own
festival. NO third-party band/logo/likeness. Print-ready transparent PNGs.

Usage: python3 scripts/gen_house_drip_designs.py
Outputs: store/static/festseed/drip-<day>.png  + opens them.

See docs/hawaii_house_seed_campaign.md for the concept + placement plan.
"""
import os, sys
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

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "store" / "static" / "festseed"
OUT.mkdir(parents=True, exist_ok=True)
MODEL = "gemini-3-pro-image-preview"

SYSTEM = """You are the lead apparel graphic designer for the MU brand (wearmu.com).
MU's marks are minimal and symbolic: 無 (mu = "nothing/zero"), 月 (moon), and the
wordmark ━◯━ (a circle flanked by two short bars). Aesthetic: quiet Japanese
minimalism + a single warm Hawaiian sunset gold, lots of negative space.

Produce ONE square 2940x2940 PNG, TRANSPARENT background, for direct-to-garment
printing.

Strict rules:
- Flat solid shapes. MAX 3 colors: off-white (#f2f2ee), warm gold (#f5b142),
  optional deep ink (#0a0a0a) for fine detail.
- NO photo background, NO heavy gradient, NO mesh, NO drop shadow.
- NO real faces. NO third-party logos/brand names/song titles. MU-original only.
- Center it, >=12% padding from edges, legible at 4cm.
- TEXT: render EXACTLY the words given below, spelled exactly, and NO other
  words. Heavy clean condensed sans (Helvetica Neue / Arial Black). If the brief
  says "no text", render zero letters.

Design brief: {brief}

Output: ONE print-ready transparent graphic, nothing else."""

DESIGNS = {
 "day1": "NO TEXT. Pure silent mark: the ━◯━ wordmark centered, with 無 set "
   "inside the circle. The ◯ is a single thin gold ring; bars and 無 in off-white. "
   "This is Day 1 — it says nothing on purpose. Calm, mysterious.",
 "day2": "Centered, the only words: THIS SHIRT SAYS NOTHING. Set in a heavy "
   "condensed sans, 2-3 stacked lines, off-white. Below it, very small and gold: "
   "the ━◯━ mark. Dry, deadpan humor (無 literally means nothing). Nothing else.",
 "day3": "Top line, the only headline words: DAY THREE. CURIOUS YET? in heavy "
   "condensed off-white sans. Centered below it, a bold flat square QR-code-style "
   "glyph (geometric modules, NOT a real scannable code) with its center module "
   "replaced by the gold ◯. Under the glyph, tiny gold lowercase word: scan. "
   "Only those words: 'DAY THREE. CURIOUS YET?' and 'scan'.",
 "day4": "Big top line: TAKE ME HOME. (off-white heavy sans). A thin gold "
   "horizontal rule. Under it, a second smaller line: MU FESTIVAL · HAWAII. To the "
   "side or below, a small flat square QR-style glyph with a gold ◯ center, and the "
   "tiny ━◯━ mark. Warm, inviting. Only those words: 'TAKE ME HOME.' and "
   "'MU FESTIVAL · HAWAII'. No dates.",
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
            p = OUT / f"drip-{key}.png"
            p.write_bytes(data)
            print(f"  ✓ {p.relative_to(ROOT)}  ({len(data)//1024} KB)")
            made.append(str(p))
        except Exception as e:
            print(f"  ✗ {key}: {e}")
    if made:
        os.system("open " + " ".join(f"'{m}'" for m in made))
        print(f"opened {len(made)} drip designs")

if __name__ == "__main__":
    main()
