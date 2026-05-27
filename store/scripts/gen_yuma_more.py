#!/usr/bin/env python3
"""MU × YUMA 碧 — 6 additional 税理士 phrase tees.

Per SKU: (a) a flat-lay light-blue tee mockup for the LP preview, and
(b) a design-only transparent print file (white/light keyed out by luminance
+ border-strip + autocrop), matching the first 4.

Out: static/yuma/preview/<key>.png  +  static/yuma/d/design_<SKU>.png
Model: gemini-3-pro-image-preview only.
"""
import os, sys, pathlib, io
from google import genai
from google.genai import types
from PIL import Image

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY:
    sys.exit("GEMINI_API_KEY not set (source /Users/yuki/.env)")

ROOT = pathlib.Path(__file__).resolve().parent.parent
PREV = ROOT / "static" / "yuma" / "preview"; PREV.mkdir(parents=True, exist_ok=True)
DSGN = ROOT / "static" / "yuma" / "d"; DSGN.mkdir(parents=True, exist_ok=True)

MOCK = (
    "Flat-lay product photograph, top-down, of a SINGLE light-blue (水色 / soft Baby "
    "Blue, ~#AEDCEC) heather t-shirt laid flat on a clean off-white background with "
    "soft natural shadow. Fresh, calm, premium minimal (爽やか). The ONLY printed "
    "graphic is centered on the chest, below. Modern refined Japanese typography, "
    "single ink color, lots of negative space. No tags, no extra logos, no props. "
    "Render every Japanese character EXACTLY, crisp and legible.\n\nChest print: "
)
PRINT = (
    "A clean screen-print-ready GRAPHIC ONLY (no t-shirt, no garment), centered with "
    "generous margin on a PURE SOLID WHITE (#FFFFFF) background, no shadow, no frame. "
    "Modern refined Japanese typography, single ink color, crisp vector-like edges for "
    "DTG printing. Render every Japanese character EXACTLY, correct strokes, legible."
    "\n\nDesign: "
)

# key: (SKU suffix, chest-design description)
DESIGNS = {
    "datsuzei": ("YUMA-TEE-DATSUZEI",
        "the phrase 「節税と脱税は、ちがう。」 in a confident modern Japanese gothic sans-serif, "
        "deep navy ink, two balanced lines, with a tiny square 碧 seal-stamp accent."),
    "donburi": ("YUMA-TEE-DONBURI",
        "the phrase 「どんぶり勘定、卒業。」 in a clean modern Japanese gothic, deep teal ink, "
        "with a tiny square 碧 seal-stamp accent."),
    "invoice": ("YUMA-TEE-INVOICE",
        "the phrase 「インボイス、登録した?」 in a clean modern Japanese gothic, deep navy ink, "
        "with a tiny square 碧 seal-stamp accent."),
    "genka": ("YUMA-TEE-GENKA",
        "the phrase 「減価償却は、人生。」 in a refined modern Japanese type, deep teal ink, "
        "with a small square 碧 seal-stamp accent."),
    "cash": ("YUMA-TEE-CASH",
        "the phrase 「黒字より、現金。」 in a bold confident modern Japanese gothic, deep navy "
        "ink, with a small square 碧 seal-stamp accent."),
    "kigen": ("YUMA-TEE-KIGEN",
        "the phrase 「期限は、待ってくれない。」 in a clean modern Japanese gothic, deep teal ink, "
        "two balanced lines, with a tiny square 碧 seal-stamp accent."),
}

def gen(client, prompt):
    resp = client.models.generate_content(
        model="gemini-3-pro-image-preview", contents=prompt,
        config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"]))
    for c in resp.candidates or []:
        for part in (c.content.parts or []):
            if getattr(part, "inline_data", None) and part.inline_data.data:
                return part.inline_data.data
    return None

def clean_print(data, out):
    im = Image.open(io.BytesIO(data)).convert("RGBA")
    px = im.load(); w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if (r*299 + g*587 + b*114)//1000 >= 205:
                px[x, y] = (r, g, b, 0)
    B = 18
    for y in range(h):
        for x in range(w):
            if x < B or x >= w-B or y < B or y >= h-B:
                px[x, y] = (px[x, y][0], px[x, y][1], px[x, y][2], 0)
    bbox = im.getchannel("A").getbbox()
    art = im.crop(bbox); aw, ah = art.size
    pad = int(max(aw, ah)*0.09)
    canvas = Image.new("RGBA", (aw+2*pad, ah+2*pad), (0, 0, 0, 0))
    canvas.alpha_composite(art, (pad, pad))
    canvas.save(out)
    return canvas.size

def main():
    client = genai.Client(api_key=KEY)
    for key, (sku, spec) in DESIGNS.items():
        print(f"[more] {sku} mockup …")
        m = gen(client, MOCK + spec)
        if m: (PREV / f"{key}.png").write_bytes(m); print(f"  preview/{key}.png")
        else: print("  MOCKUP FAILED")
        print(f"[more] {sku} print …")
        p = gen(client, PRINT + spec)
        if p:
            sz = clean_print(p, DSGN / f"design_{sku}.png")
            print(f"  design_{sku}.png {sz}")
        else: print("  PRINT FAILED")

if __name__ == "__main__":
    main()
