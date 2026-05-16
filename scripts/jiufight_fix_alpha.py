#!/usr/bin/env python3
"""Post-process Gemini-generated PNGs for Printful DTG:

  1. Convert opaque-black-BG output → transparent BG (alpha = max(R,G,B))
     so the design prints as white ink on a black T-shirt cleanly without a
     dark rectangle ghosting on the garment.
  2. Threshold near-white shapes to pure white (255,255,255) to remove any
     residual gradient halo from Gemini's anti-aliasing.
  3. Upscale 4× to 4096×4096 with LANCZOS — gives Printful enough headroom
     for 300 DPI at the standard 12"×12" chest placement.

  Overwrites the source files in place.
"""
import sys, os
from PIL import Image

INSPIRED = "/Users/yuki/workspace/mu-brand/store/static/jiufight/inspired"
UPSCALE  = 4
THRESHOLD = 32  # any RGB pixel with max(R,G,B) <= this is treated as background

def process(path: str):
    img = Image.open(path).convert("RGB")
    w, h = img.size
    out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    src = img.load()
    dst = out.load()
    n_transp = 0
    n_white  = 0
    for y in range(h):
        for x in range(w):
            r, g, b = src[x, y]
            lum = max(r, g, b)
            if lum <= THRESHOLD:
                # background black → fully transparent
                dst[x, y] = (0, 0, 0, 0)
                n_transp += 1
            else:
                # foreground → pure white at alpha proportional to luminance
                dst[x, y] = (255, 255, 255, lum)
                n_white += 1
    # Upscale for Printful DTG
    target = (w * UPSCALE, h * UPSCALE)
    out_up = out.resize(target, Image.LANCZOS)
    out_up.save(path, "PNG", optimize=True)
    print(f"  ✓ {os.path.basename(path)}: {target[0]}×{target[1]} RGBA  "
          f"(transparent: {n_transp:,}px / fg: {n_white:,}px / {n_transp*100//(w*h)}% bg)")

if __name__ == "__main__":
    files = sorted([os.path.join(INSPIRED, f) for f in os.listdir(INSPIRED)
                    if f.endswith(".png")])
    print(f"processing {len(files)} PNGs in {INSPIRED}/")
    for f in files:
        process(f)
