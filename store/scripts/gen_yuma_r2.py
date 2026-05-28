#!/usr/bin/env python3
"""MU × YUMA 碧 — round 2 additions: long-sleeve tee + 4×4 sticker.

- Long-sleeve preview reuses the hero design_YUMA-TEE-AO.png as the print.
- Sticker gets its own 碧-ONLY design file (no phrase) — cleaner at 4×4.
"""
import os, sys, pathlib, io
from google import genai
from google.genai import types
from PIL import Image

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY: sys.exit("GEMINI_API_KEY not set")
ROOT = pathlib.Path(__file__).resolve().parent.parent
PREV = ROOT / "static" / "yuma" / "preview"; PREV.mkdir(parents=True, exist_ok=True)
DSGN = ROOT / "static" / "yuma" / "d"; DSGN.mkdir(parents=True, exist_ok=True)

PROMPTS = {
    "longsleeve_ao": ("preview",
        "Flat-lay product photograph, top-down, of a SINGLE soft LIGHT BLUE long-sleeve "
        "unisex t-shirt (Gildan 2400-style) laid flat with sleeves arranged neatly, on a "
        "clean off-white background with soft natural shadow. Fresh, premium, minimal "
        "(爽やか). Centered on the chest: a large brush-calligraphy kanji 「碧」 in deep "
        "teal ink with the phrase 「青色申告は、正義。」 in clean modern Japanese gothic "
        "below it, same teal ink. Render Japanese EXACTLY. No props, no labels."),
    "sticker_ao": ("preview",
        "Product photograph of a single 4×4 inch KISS-CUT VINYL STICKER sitting on a "
        "clean light wood desk next to a fountain pen and a folded calculator slip "
        "(subtle props, slightly out of focus). The sticker has a thin white kiss-cut "
        "border around a single bold brush-calligraphy kanji 「碧」 in deep teal-blue ink "
        "on a pure white background. Render the 碧 kanji EXACTLY, correct strokes, "
        "perfectly legible. Top-down, soft natural light, premium minimal."),
    "STICKER_AO_PRINT": ("print",
        "A clean print-ready GRAPHIC ONLY, centered with generous margin on a PURE SOLID "
        "WHITE (#FFFFFF) background, no shadow, no frame, no t-shirt. The graphic is a "
        "single large bold brush-calligraphy kanji 「碧」 in deep teal-blue ink, "
        "rendered EXACTLY with correct strokes. High resolution, crisp vector-like edges "
        "suitable for kiss-cut vinyl sticker printing."),
}

def gen(client, prompt):
    resp = client.models.generate_content(
        model="gemini-3-pro-image-preview", contents=prompt,
        config=types.GenerateContentConfig(response_modalities=["IMAGE","TEXT"]))
    for c in resp.candidates or []:
        for part in (c.content.parts or []):
            if getattr(part,"inline_data",None) and part.inline_data.data:
                return part.inline_data.data
    return None

def clean_print(data, out):
    im = Image.open(io.BytesIO(data)).convert("RGBA")
    px = im.load(); w,h = im.size
    for y in range(h):
        for x in range(w):
            r,g,b,a = px[x,y]
            if (r*299+g*587+b*114)//1000 >= 205: px[x,y] = (r,g,b,0)
    B=18
    for y in range(h):
        for x in range(w):
            if x<B or x>=w-B or y<B or y>=h-B:
                px[x,y]=(px[x,y][0],px[x,y][1],px[x,y][2],0)
    bbox = im.getchannel("A").getbbox()
    art = im.crop(bbox); aw,ah = art.size; pad=int(max(aw,ah)*0.09)
    canvas = Image.new("RGBA",(aw+2*pad, ah+2*pad),(0,0,0,0))
    canvas.alpha_composite(art,(pad,pad)); canvas.save(out)
    return canvas.size

def main():
    client = genai.Client(api_key=KEY)
    for name, (kind, prompt) in PROMPTS.items():
        print(f"[r2] {name} ({kind}) …")
        data = gen(client, prompt)
        if not data: print("  NO IMAGE"); continue
        if kind == "preview":
            (PREV / f"{name}.png").write_bytes(data)
            print(f"  saved preview/{name}.png")
        else:
            sz = clean_print(data, DSGN / "design_YUMA-STICKER-AO.png")
            print(f"  saved design_YUMA-STICKER-AO.png {sz}")

if __name__ == "__main__":
    main()
