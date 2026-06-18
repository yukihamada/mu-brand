#!/usr/bin/env python3
"""MU × YUMA — 碧 (AO) line. Tax-accountant (税理士) collab, 水色/爽やか.

Concept by Opus; images by gemini-3-pro-image-preview (only sanctioned model).
Generates flat-lay light-blue t-shirt mockups, one per design. Japanese text
must render exactly — review output and regenerate/PIL-overlay if garbled.

Out: /tmp/yuma/<key>.png
"""
import os, sys, pathlib
from google import genai
from google.genai import types

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY:
    sys.exit("GEMINI_API_KEY not set (source /Users/yuki/.env)")

OUT = pathlib.Path("/tmp/yuma"); OUT.mkdir(parents=True, exist_ok=True)

BASE = (
    "Flat-lay product photograph, top-down, of a SINGLE light-blue (水色 / soft aqua, "
    "around #AEDCEC) heather t-shirt laid flat and neatly on a clean off-white "
    "background with soft natural shadow. Fresh, calm, premium minimal aesthetic — "
    "the feeling of clear water and a clear blue sky (爽やか). The ONLY printed graphic "
    "is centered on the chest, described below. Modern refined Japanese typography, "
    "single ink color. Lots of negative space. No tags, no extra logos, no props. "
    "IMPORTANT: render every Japanese character EXACTLY as written, crisp, correct "
    "stroke order, perfectly legible — do not invent or distort kanji.\n\nChest print: "
)

DESIGNS = {
    # Hero — the 碧 mark + the blue-tax-return pun (ties color to meaning)
    "hero_ao": BASE + (
        "a large single brush-calligraphy kanji 「碧」 in deep teal-blue ink as the hero "
        "mark, and centered neatly beneath it the small phrase 「青色申告は、正義。」 in a "
        "clean modern gothic Japanese sans-serif, same teal ink."
    ),
    # Witty classic
    "keihi": BASE + (
        "only the phrase 「それ、経費で落ちません。」 set in a clean confident modern Japanese "
        "sans-serif, deep navy ink, two balanced lines, with a tiny 碧 seal-stamp mark in "
        "the corner."
    ),
    # Warm one-liner + minimal icon
    "ryoshusho": BASE + (
        "a minimal single-line-art icon of a receipt, and below it the phrase 「領収書は、愛。」 "
        "in a friendly rounded modern Japanese type, deep teal ink."
    ),
    # Philosophical / positive
    "kaihi": BASE + (
        "the phrase 「税金は、未来への会費。」 set elegantly in two lines of refined modern "
        "Japanese type, deep navy ink, with a small 碧 mark as accent."
    ),
}

def main():
    client = genai.Client(api_key=KEY)
    for key, prompt in DESIGNS.items():
        print(f"[yuma] generating {key} …")
        try:
            resp = client.models.generate_content(
                model="gemini-3-pro-image-preview",
                contents=prompt,
                config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"]),
            )
            data = None
            for cand in resp.candidates or []:
                for part in (cand.content.parts or []):
                    if getattr(part, "inline_data", None) and part.inline_data.data:
                        data = part.inline_data.data; break
                if data: break
            if not data:
                print(f"[yuma] {key}: NO IMAGE"); continue
            (OUT / f"{key}.png").write_bytes(data)
            print(f"[yuma] {key}: saved {OUT/key}.png")
        except Exception as e:
            print(f"[yuma] {key}: ERROR {e}")

if __name__ == "__main__":
    main()
