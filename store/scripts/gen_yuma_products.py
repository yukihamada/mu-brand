#!/usr/bin/env python3
"""MU × YUMA 碧 — preview mockups for the other product types (hoodie /
crewneck / mug / tote). Reuses the existing hero 碧 + 「青色申告は、正義。」
design as the print. Generates only the preview images for the LP / shop
card; print files reuse design_YUMA-TEE-AO.png so no extra print gens.

Out: static/yuma/preview/{hoodie,crewneck,mug,tote}_ao.png
"""
import os, sys, pathlib
from google import genai
from google.genai import types

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY:
    sys.exit("GEMINI_API_KEY not set")

OUT = pathlib.Path(__file__).resolve().parent.parent / "static" / "yuma" / "preview"
OUT.mkdir(parents=True, exist_ok=True)

DESIGN = (
    "the print is a large single brush-calligraphy kanji 「碧」 in deep teal ink, "
    "with the phrase 「青色申告は、正義。」 set neatly beneath it in a clean modern "
    "Japanese gothic, same teal ink. Render every Japanese character EXACTLY."
)

PROMPTS = {
    "hoodie_ao": (
        "Flat-lay product photograph, top-down, of a SINGLE soft LIGHT BLUE / heather "
        "blue pullover hoodie (Gildan-style heavy blend) laid flat with the hood "
        "neatly arranged, on a clean off-white background, soft natural shadow. "
        "Fresh, premium, minimal (爽やか). Centered on the chest, " + DESIGN +
        " No other text, no labels, no props."),
    "crewneck_ao": (
        "Flat-lay product photograph, top-down, of a SINGLE soft LIGHT BLUE crew-neck "
        "sweatshirt (Gildan-style) laid flat, on a clean off-white background, soft "
        "natural shadow. Fresh, premium, minimal (爽やか). Centered on the chest, "
        + DESIGN + " No other text, no labels, no props."),
    "mug_ao": (
        "Studio product photograph of a SINGLE white ceramic coffee mug with a clearly "
        "visible BLUE interior (the inside of the mug is solid blue), 11oz, sitting "
        "upright on a clean off-white surface with soft natural light and shadow. The "
        "visible side of the mug is printed with " + DESIGN +
        " Slight 3/4 angle so the blue interior reads clearly. No props, no liquid."),
    "tote_ao": (
        "Flat-lay product photograph, top-down, of a SINGLE natural-white cotton tote "
        "bag laid flat with straps neatly arranged, on a clean off-white background, "
        "soft natural shadow. Centered on the front of the tote, " + DESIGN +
        " Fresh, minimal aesthetic. No other text, no labels, no props."),
}

def main():
    client = genai.Client(api_key=KEY)
    for k, prompt in PROMPTS.items():
        print(f"[products] {k} …")
        try:
            resp = client.models.generate_content(
                model="gemini-3-pro-image-preview", contents=prompt,
                config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"]))
            data = None
            for c in resp.candidates or []:
                for part in (c.content.parts or []):
                    if getattr(part, "inline_data", None) and part.inline_data.data:
                        data = part.inline_data.data; break
                if data: break
            if not data: print(f"  NO IMAGE"); continue
            (OUT / f"{k}.png").write_bytes(data)
            print(f"  saved preview/{k}.png")
        except Exception as e:
            print(f"  ERROR {e}")

if __name__ == "__main__":
    main()
