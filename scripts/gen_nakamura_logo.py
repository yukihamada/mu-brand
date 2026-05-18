#!/usr/bin/env python3
"""Draw the MU × Nakamura "中" gold kanji on a truly transparent RGBA canvas.

Previous attempt used Gemini for the design, but Gemini's image output is
RGB (no alpha channel), so the background got baked into the Printful
mockups. This script uses PIL with Hiragino Sans GB and writes a real
RGBA PNG that Printful's mockup-generator respects as transparent.

Output: store/static/nakamura/_logo_v1.png (2940x2940 RGBA)
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "store" / "static" / "nakamura" / "_logo_v1.png"

# Printful DTG print bed = 2940x2940 (matches main.rs comment).
SIZE = 2940
KANJI = "中"
GOLD = (255, 215, 0, 255)        # #FFD700

# Use Hiragino Sans GB — renders 中 perfectly at heavyweight.
FONT_PATH = "/System/Library/Fonts/Hiragino Sans GB.ttc"

def draw():
    canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))  # fully transparent
    draw = ImageDraw.Draw(canvas)

    # Pick font size: target glyph ~80% of canvas height.
    target_h = int(SIZE * 0.80)
    # Binary-search the font size so the bounding box ≈ target.
    lo, hi = 100, 3500
    while lo + 4 < hi:
        mid = (lo + hi) // 2
        f = ImageFont.truetype(FONT_PATH, mid, index=1)  # index=1 = Bold
        bb = f.getbbox(KANJI)
        h = bb[3] - bb[1]
        if h < target_h:
            lo = mid
        else:
            hi = mid
    font = ImageFont.truetype(FONT_PATH, lo, index=1)

    # Measure and center.
    bb = font.getbbox(KANJI)
    w = bb[2] - bb[0]
    h = bb[3] - bb[1]
    x = (SIZE - w) // 2 - bb[0]
    y = (SIZE - h) // 2 - bb[1]

    # Draw the gold kanji.
    draw.text((x, y), KANJI, font=font, fill=GOLD)

    canvas.save(OUT, format="PNG", optimize=True)
    print(f"  ✓ saved {OUT}")
    print(f"    mode={canvas.mode}, size={canvas.size}, font_pt={lo}")

if __name__ == "__main__":
    draw()
