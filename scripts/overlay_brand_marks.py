#!/usr/bin/env python3
"""Overlay '<BRAND> × MU' + QR + timestamp onto print designs.

Adds a 14% tall footer band to each design PNG with:
  · '<BRAND> × MU' wordmark (left)
  · era timestamp + drop tag (center)
  · QR code linking to a canonical URL (right)

Usage:
  python3 scripts/overlay_brand_marks.py jiufight \
      --in store/static/jiufight/products \
      --pattern '*_front.png' \
      --qr-base https://wearmu.com/jiufight \
      --brand JIUFIGHT

Idempotent: writes to <stem>_marked.png next to the source so the
originals stay intact.
"""
from __future__ import annotations
import argparse, datetime, hashlib, io, re
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont
import qrcode
from qrcode.image.pil import PilImage

FONT_BOLD = "/System/Library/Fonts/ヒラギノ角ゴシック W7.ttc"
FONT_REG  = "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc"

ERA_NUMS = {1: "I", 4: "IV", 5: "V", 9: "IX", 10: "X", 40: "XL", 50: "L",
            90: "XC", 100: "C", 400: "CD", 500: "D", 900: "CM", 1000: "M"}

def roman(n: int) -> str:
    out, keys = [], sorted(ERA_NUMS.keys(), reverse=True)
    for k in keys:
        while n >= k:
            out.append(ERA_NUMS[k]); n -= k
    return "".join(out)

def fit_text(font_path: str, text: str, max_w: int, max_h: int) -> ImageFont.FreeTypeFont:
    sz = max_h
    while sz > 8:
        f = ImageFont.truetype(font_path, sz)
        bb = f.getbbox(text)
        if (bb[2]-bb[0]) <= max_w and (bb[3]-bb[1]) <= max_h:
            return f
        sz -= 2
    return ImageFont.truetype(font_path, 12)

def overlay(src: Path, dst: Path, brand: str, qr_url: str, drop_num: int):
    img = Image.open(src).convert("RGBA")
    w, h = img.size
    band_h = int(h * 0.14)
    band_y = h - band_h
    canvas = img.copy()
    draw = ImageDraw.Draw(canvas, "RGBA")

    # Soft underlay so the band reads on busy art (semi-transparent black).
    draw.rectangle([(0, band_y), (w, h)], fill=(0, 0, 0, 200))
    # Top hairline.
    draw.line([(int(w*0.06), band_y), (int(w*0.94), band_y)], fill=(255,255,255,180), width=2)

    pad = int(w * 0.04)
    inner_h = band_h - pad * 2
    cy = band_y + band_h // 2

    # ── QR (right) ─────────────────────────────────────────────────────
    qr = qrcode.QRCode(error_correction=qrcode.constants.ERROR_CORRECT_M, border=2)
    qr.add_data(qr_url)
    qr.make(fit=True)
    qr_img = qr.make_image(fill_color="white", back_color="black").convert("RGBA")
    qr_size = inner_h
    qr_img = qr_img.resize((qr_size, qr_size), Image.NEAREST)
    qr_x = w - pad - qr_size
    canvas.paste(qr_img, (qr_x, band_y + pad), qr_img)

    # ── Wordmark (left) ────────────────────────────────────────────────
    mark = f"{brand.upper()} × MU"
    avail_w = qr_x - pad * 2
    f_mark = fit_text(FONT_BOLD, mark, avail_w, inner_h // 2)
    bb = f_mark.getbbox(mark)
    mw, mh = bb[2]-bb[0], bb[3]-bb[1]
    mx, my = pad, cy - mh
    draw.text((mx, my), mark, fill=(255,255,255,255), font=f_mark)

    # ── Timestamp / drop tag (left, below wordmark) ────────────────────
    now = datetime.datetime.now()
    era = roman(now.year)            # 2026 → MMXXVI
    drop_tag = f"DROP {drop_num:02d} · {era} · {now.strftime('%Y-%m-%d')}"
    f_ts = fit_text(FONT_REG, drop_tag, avail_w, max(18, inner_h // 4))
    draw.text((mx, cy + 8), drop_tag, fill=(220,220,220,235), font=f_ts)

    # ── Save ───────────────────────────────────────────────────────────
    out = canvas.convert("RGB") if dst.suffix.lower() in (".jpg", ".jpeg") else canvas
    out.save(dst, "PNG", optimize=True)
    return dst

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("brand", help="brand label, e.g. JIUFIGHT, ATSUME")
    ap.add_argument("--in", dest="indir", required=True, help="directory of design PNGs")
    ap.add_argument("--pattern", default="*_front.png", help="glob, default *_front.png")
    ap.add_argument("--qr-base", required=True, help="base URL; final = {base}/<drop_num>")
    ap.add_argument("--qr-url", default=None, help="full URL override (skip per-drop suffix)")
    ap.add_argument("--out-suffix", default="_marked", help="suffix for output (default _marked)")
    args = ap.parse_args()

    indir = Path(args.indir)
    if not indir.is_dir():
        raise SystemExit(f"not a directory: {indir}")
    files = sorted(indir.glob(args.pattern))
    if not files:
        raise SystemExit(f"no files matched {args.pattern} in {indir}")
    print(f"processing {len(files)} files from {indir}")
    for src in files:
        m = re.match(r"^(\d+)", src.stem)
        drop_num = int(m.group(1)) if m else 0
        qr_url = args.qr_url or f"{args.qr_base.rstrip('/')}/{drop_num:02d}"
        dst = src.with_name(f"{src.stem}{args.out_suffix}.png")
        overlay(src, dst, args.brand, qr_url, drop_num)
        print(f"  ✓ {src.name} → {dst.name}  (qr={qr_url})")

if __name__ == "__main__":
    main()
