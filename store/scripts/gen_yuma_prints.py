#!/usr/bin/env python3
"""MU × YUMA print-ready design files (graphic only, no t-shirt).

For DTG on a Baby Blue Bella+Canvas 3001 — dark teal/navy ink art on a
transparent background. Generated on pure white, then white keyed to
transparent (matches the Rust generate_transparent_print >=248 threshold).

Out: store/static/yuma/d/design_<SKU>.png
"""
import os, sys, pathlib, io
from google import genai
from google.genai import types
from PIL import Image

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY:
    sys.exit("GEMINI_API_KEY not set (source /Users/yuki/.env)")

OUT = pathlib.Path(__file__).resolve().parent.parent / "static" / "yuma" / "d"
OUT.mkdir(parents=True, exist_ok=True)

BASE = (
    "A clean screen-print-ready GRAPHIC ONLY (no t-shirt, no garment, no mockup), "
    "centered with generous margin on a PURE SOLID WHITE (#FFFFFF) background, no "
    "shadow, no frame. Modern refined Japanese typography, single ink color, crisp "
    "vector-like edges suitable for DTG t-shirt printing. IMPORTANT: render every "
    "Japanese character EXACTLY as written, correct strokes, perfectly legible.\n\n"
    "Design: "
)

DESIGNS = {
    "YUMA-TEE-AO": BASE + (
        "a large single brush-calligraphy kanji 「碧」 in deep teal-blue ink, with the "
        "small phrase 「青色申告は、正義。」 centered neatly beneath it in a clean modern "
        "gothic Japanese sans-serif, same teal ink."
    ),
    "YUMA-TEE-KEIHI": BASE + (
        "the phrase 「それ、経費で落ちません。」 in a confident modern Japanese sans-serif, "
        "deep navy ink, two balanced lines, with a tiny square 碧 seal-stamp accent."
    ),
    "YUMA-TEE-RYOSHU": BASE + (
        "a minimal single-line-art icon of a receipt above the phrase 「領収書は、愛。」 in a "
        "friendly rounded modern Japanese type, deep teal ink."
    ),
    "YUMA-TEE-KAIHI": BASE + (
        "the phrase 「税金は、未来への会費。」 in two lines of refined modern Japanese type, "
        "deep navy ink, with a small square 碧 seal-stamp accent."
    ),
}

def key_white(data: bytes, path: pathlib.Path):
    img = Image.open(io.BytesIO(data)).convert("RGBA")
    px = img.load(); w, h = img.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if r >= 248 and g >= 248 and b >= 248:
                px[x, y] = (r, g, b, 0)
    img.save(path)
    return img.size

def main():
    client = genai.Client(api_key=KEY)
    for sku, prompt in DESIGNS.items():
        print(f"[yuma-print] {sku} …")
        try:
            resp = client.models.generate_content(
                model="gemini-3-pro-image-preview",
                contents=prompt,
                config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"]),
            )
            data = None
            for c in resp.candidates or []:
                for part in (c.content.parts or []):
                    if getattr(part, "inline_data", None) and part.inline_data.data:
                        data = part.inline_data.data; break
                if data: break
            if not data:
                print(f"[yuma-print] {sku}: NO IMAGE"); continue
            (OUT / f"raw_{sku}.png").write_bytes(data)
            size = key_white(data, OUT / f"design_{sku}.png")
            print(f"[yuma-print] {sku}: saved design_{sku}.png {size}")
        except Exception as e:
            print(f"[yuma-print] {sku}: ERROR {e}")

if __name__ == "__main__":
    main()
