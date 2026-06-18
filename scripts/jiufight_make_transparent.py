#!/usr/bin/env python3
"""Strip uniform-corner backgrounds → alpha=0 (chromakey) for jiufight PNGs.

Detection:
  · Sample the 4 corners + 4 edge midpoints
  · If they all agree within tol (default 6 per channel), treat that color as
    the background and make every pixel within tol_pixel of it fully transparent
  · Anti-alias edge: pixels within `feather` of the bg get partial alpha

Writes alongside the source as `<stem>_trans.png` (so the marked overlay can
consume them).

Usage:
  python3 scripts/jiufight_make_transparent.py \
      --in store/static/jiufight/products --pattern '*_front.png'
"""
from __future__ import annotations
import argparse
from pathlib import Path
from PIL import Image
import numpy as np


def detect_bg(arr: np.ndarray, tol_corner: int) -> tuple[int, int, int] | None:
    """Try (1) 4 corners agree, then (2) most-common color in corner blocks.
    Returns (r,g,b) or None.
    """
    h, w = arr.shape[:2]
    corners = [arr[2, 2, :3], arr[2, w - 3, :3], arr[h - 3, 2, :3], arr[h - 3, w - 3, :3]]
    s = np.stack(corners).astype(np.int16)
    if (s.max(axis=0) - s.min(axis=0)).max() <= tol_corner:
        return tuple(int(v) for v in s.mean(axis=0).round().astype(int))

    # Fallback: take 64x64 squares from each corner and pick the most common
    # rounded color (quantized to 8 levels per channel so anti-aliasing
    # doesn't fragment the histogram).
    box = 64
    regions = [
        arr[:box, :box, :3],
        arr[:box, -box:, :3],
        arr[-box:, :box, :3],
        arr[-box:, -box:, :3],
    ]
    pix = np.concatenate([r.reshape(-1, 3) for r in regions])
    quant = (pix.astype(np.int16) >> 4) << 4
    keys, counts = np.unique(quant.view([('', quant.dtype)] * 3), return_counts=True)
    top = keys[np.argmax(counts)]
    # top is a structured array element of shape (3,) — unpack via .item()
    bg = tuple(int(top[i]) for i in range(3))
    return bg


def chromakey(src: Path, dst: Path, tol_corner: int, tol_pixel: int, feather: int) -> bool:
    img = Image.open(src).convert("RGBA")
    arr = np.array(img)
    bg = detect_bg(arr, tol_corner)
    if bg is None:
        print(f"  - {src.name}: corners not uniform, skip")
        img.save(dst, "PNG", optimize=True)
        return False

    r, g, b = bg
    rgb = arr[..., :3].astype(np.int16)
    diff = np.max(np.abs(rgb - np.array([r, g, b], dtype=np.int16)), axis=-1)
    # Hard transparent for tight match, feathered band for soft edge.
    new_alpha = arr[..., 3].astype(np.float32)
    new_alpha = np.where(diff <= tol_pixel, 0.0, new_alpha)
    if feather > 0:
        band = (diff > tol_pixel) & (diff <= tol_pixel + feather)
        falloff = ((diff - tol_pixel) / max(feather, 1)).clip(0.0, 1.0)
        new_alpha = np.where(band, new_alpha * falloff, new_alpha)
    arr[..., 3] = new_alpha.clip(0, 255).astype(np.uint8)
    Image.fromarray(arr, "RGBA").save(dst, "PNG", optimize=True)

    transparent_pct = float((arr[..., 3] == 0).mean()) * 100
    print(f"  ✓ {src.name} → bg=rgb{bg} → transparent {transparent_pct:.1f}%")
    return True


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--in", dest="indir", required=True)
    ap.add_argument("--pattern", default="*_front.png")
    ap.add_argument("--out-suffix", default="_trans")
    ap.add_argument("--tol-corner", type=int, default=8,
                    help="max channel diff for corners to agree (default 8)")
    ap.add_argument("--tol-pixel", type=int, default=18,
                    help="max channel diff to treat as bg (default 18)")
    ap.add_argument("--feather", type=int, default=10,
                    help="soft alpha band in diff units (default 10)")
    args = ap.parse_args()

    indir = Path(args.indir)
    files = sorted(p for p in indir.glob(args.pattern) if "_marked" not in p.stem and "_trans" not in p.stem)
    if not files:
        raise SystemExit(f"no files matched {args.pattern} in {indir}")
    print(f"processing {len(files)} files")
    for src in files:
        dst = src.with_name(f"{src.stem}{args.out_suffix}.png")
        chromakey(src, dst, args.tol_corner, args.tol_pixel, args.feather)


if __name__ == "__main__":
    main()
