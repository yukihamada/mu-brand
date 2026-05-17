#!/usr/bin/env python3
"""tight_crop.py — Crop white margins from design PNGs for maximum SUZURI print size.

INPUT  : a directory of finished design PNGs (white bg, dark content)
OUTPUT : <dir>/print/*.png  — tightly cropped, ready for SUZURI material upload

USAGE
─────
  python3 scripts/tight_crop.py /path/to/fixed/   [--pad 16] [--threshold 248]
"""
import argparse, sys
from pathlib import Path
from PIL import Image

def trim(im: Image.Image, thresh: int, pad: int) -> Image.Image:
    if im.mode != "RGB": im = im.convert("RGB")
    gray = im.convert("L")
    # mark dark pixels as content
    mask = gray.point(lambda v: 0 if v >= thresh else 255)
    bbox = mask.getbbox()
    if not bbox:
        return im
    l, t, r, b = bbox
    l = max(0, l - pad); t = max(0, t - pad)
    r = min(im.width, r + pad); b = min(im.height, b + pad)
    return im.crop((l, t, r, b))

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("src_dir")
    ap.add_argument("--pad", type=int, default=16)
    ap.add_argument("--threshold", type=int, default=248)
    args = ap.parse_args()
    src = Path(args.src_dir).resolve()
    out = src / "print"
    out.mkdir(exist_ok=True)
    for f in sorted(src.glob("*.png")):
        if f.name.startswith("_"): continue
        if f.parent != src: continue
        im = Image.open(f)
        cropped = trim(im, args.threshold, args.pad)
        out_path = out / f.name
        cropped.save(out_path, "PNG", optimize=True)
        print(f"  {f.name}: {im.size} → {cropped.size}  (saved {out_path})")

if __name__ == "__main__":
    main()
