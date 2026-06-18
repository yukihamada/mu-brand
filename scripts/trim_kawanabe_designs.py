#!/usr/bin/env python3
"""Convert Gemini's faux-transparency checkerboard pattern into real alpha=0.

Gemini sometimes returns PNGs in RGB mode with the standard 'transparency
indicator' (white + light gray checker) literally drawn into the image.
Printful then renders that pattern as part of the printed design.

Fix: detect those exact checker colors (255,255,255 and 203,203,203, ±2)
and any color close enough to white/gray that's clearly background → set
alpha = 0. Output RGBA. Then re-crop to the actual design bbox.
"""
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
DIR = ROOT / "store" / "static" / "ads"

# Checkerboard indicator colors used by image viewers / Photoshop
CHECKER_COLORS = [(255, 255, 255), (203, 203, 203), (204, 204, 204), (202, 202, 202)]
TOL = 4   # ± tolerance on each RGB channel
PAD = 20  # safety margin after crop

def is_checker(rgb):
    r, g, b = rgb
    # Grayscale check: R≈G≈B (within tolerance)
    if abs(r - g) > TOL or abs(g - b) > TOL:
        return False
    for cr, cg, cb in CHECKER_COLORS:
        if abs(r - cr) <= TOL and abs(g - cg) <= TOL and abs(b - cb) <= TOL:
            return True
    return False


def main():
    count = 0
    for p in sorted(DIR.glob("kawanabe_*.png")):
        img = Image.open(p)
        orig_mode = img.mode
        if img.mode != "RGBA":
            img = img.convert("RGBA")
        w, h = img.size
        px = img.load()
        changed = 0
        for y in range(h):
            for x in range(w):
                r, g, b, a = px[x, y]
                if is_checker((r, g, b)):
                    px[x, y] = (0, 0, 0, 0)
                    changed += 1
        # Now crop to non-transparent bbox + pad
        alpha = img.split()[-1]
        thr = alpha.point(lambda v: 255 if v >= 16 else 0)
        bbox = thr.getbbox()
        if bbox:
            l, t, r2, b2 = bbox
            l = max(0, l - PAD); t = max(0, t - PAD)
            r2 = min(w, r2 + PAD); b2 = min(h, b2 + PAD)
            img = img.crop((l, t, r2, b2))
        img.save(p, optimize=True)
        new_w, new_h = img.size
        print(f"  ✓ {p.name}: {orig_mode} {w}×{h} → RGBA {new_w}×{new_h} (transparent: {changed} px)")
        count += 1
    print(f"\nDone. Processed {count} files.")


if __name__ == "__main__":
    main()
