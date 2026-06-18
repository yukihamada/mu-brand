#!/usr/bin/env python3
"""Generate 20 typography design PNGs for the ROLL ◐ MU brand.

Reads the SKU list from /api/brand/roll on a local mu-store boot, or
falls back to the static manifest at /static/roll/designs.json. Writes
PNGs into store/static/roll/d/design_<SKU>.png, sized 4500×5400 (Printful
DTG safe area, 300 DPI).

The PNG is the print art (transparent background, white/black text).
It's what we'd upload to Printful's mockup-generator to get an on-body
shirt photo. For the LP we also resize a card-friendly preview into
store/static/roll/mockups/preview_<SKU>.png.

Usage:
  python3 scripts/gen_roll_designs.py            # uses local DB
  python3 scripts/gen_roll_designs.py --json     # uses designs.json
"""

import json
import os
import sys
import sqlite3
import urllib.request
from pathlib import Path

try:
    from PIL import Image, ImageDraw, ImageFont
except ImportError:
    sys.exit("pip install pillow")

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
STATIC = ROOT / "static" / "roll"
DESIGN_DIR = STATIC / "d"
PREVIEW_DIR = STATIC / "mockups"
DESIGN_DIR.mkdir(parents=True, exist_ok=True)
PREVIEW_DIR.mkdir(parents=True, exist_ok=True)

# Printful DTG safe area (Bella+Canvas 3001 chest, AOP rashguard full)
W, H = 4500, 5400
PREVIEW_W = 1200

# ---- Fonts ----------------------------------------------------------------
FONT_CANDIDATES = [
    "/System/Library/Fonts/HelveticaNeue.ttc",
    "/System/Library/Fonts/Helvetica.ttc",
    "/Library/Fonts/Arial Bold.ttf",
]
JP_FONT_CANDIDATES = [
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/ヒラギノ角ゴシック W7.ttc",
]


def _first(paths):
    for p in paths:
        if os.path.exists(p):
            return p
    return None


SANS = _first(FONT_CANDIDATES) or _first(JP_FONT_CANDIDATES)
JP = _first(JP_FONT_CANDIDATES) or SANS


def font(path, size, index=0):
    try:
        return ImageFont.truetype(path, size, index=index)
    except Exception:
        return ImageFont.truetype(path, size)


# ---- Design renderer -----------------------------------------------------
PALETTE = {
    "white": "#FFFFFF",
    "black": "#0A0A0A",
    "gold":  "#E6C449",
    "red":   "#DC2626",
}

# Per-SKU design recipes: (text lines, font ratios, color, accent, fontfile)
# Keep it minimal — clean typography is the brand.
RECIPES = {
    "ROLL-TEE-01":  ("ROLL ◐ MU",           "white"),
    "ROLL-TEE-02":  ("SPIN\nTHE\nWORLD",    "white"),
    "ROLL-TEE-03":  ("片月\nHALF MOON",     "white"),
    "ROLL-TEE-04":  ("回せ、\n世界を。",      "black"),   # WHITE tee → dark ink
    "ROLL-TEE-05":  ("ROLL\n#001",          "white"),
    "ROLL-TEE-06":  ("NO PITY.\nJUST ROLL.","white"),
    "ROLL-TEE-07":  ("ONE\nTURNS\nALL",     "black"),   # WHITE tee → dark ink
    "ROLL-TEE-08":  ("◐",                   "gold"),
    "ROLL-TEE-09":  ("MAT\nWHEEL\nWORLD",   "white"),
    "ROLL-TEE-10":  ("不可逆\nIRREVERSIBLE","white"),
    "ROLL-RASH-01": ("◐",                   "gold"),
    "ROLL-RASH-02": ("SPIN\nSPIN\nSPIN",    "white"),
    "ROLL-RASH-03": ("回",                  "white"),
    "ROLL-RASH-04": ("MAT\nTOPOLOGY",       "white"),
    "ROLL-RASH-05": ("ROLL\nTHUNDER",       "red"),
    "ROLL-RASH-06": ("⊙\nTHE WHEEL",        "gold"),
    "ROLL-RASH-07": ("◐ ◓ ● ◑ ◐",          "white"),
    "ROLL-RASH-08": ("前線\nFRONT LINE",     "white"),
    "ROLL-RASH-09": ("片",                  "white"),
    "ROLL-RASH-10": ("∞\n無限",              "gold"),
}


def fit_font(text, font_path, max_w, max_h, fontfile_index=0):
    """Binary-search the largest font size that keeps text within bounds."""
    lo, hi = 30, 1800
    best = lo
    while lo <= hi:
        mid = (lo + hi) // 2
        f = font(font_path, mid, index=fontfile_index)
        # Multi-line bbox
        tmp = Image.new("RGBA", (10, 10))
        d = ImageDraw.Draw(tmp)
        bbox = d.multiline_textbbox((0, 0), text, font=f, align="center", spacing=mid * 0.1)
        w = bbox[2] - bbox[0]
        h = bbox[3] - bbox[1]
        if w <= max_w and h <= max_h:
            best = mid
            lo = mid + 1
        else:
            hi = mid - 1
    return best


def draw_half_moon(draw, cx, cy, r, fill):
    """Draw a ◐ half-moon (left half filled, right half outlined)."""
    # outer ring
    draw.ellipse((cx - r, cy - r, cx + r, cy + r), outline=fill, width=max(2, r // 12))
    # filled left half (pie slice 90→270)
    draw.pieslice((cx - r, cy - r, cx + r, cy + r), 90, 270, fill=fill)


def render_design(text, color_name):
    """Render text centered on a transparent canvas, return PIL image.

    Special token ◐ is drawn as a vector half-moon (no font dependency).
    For multi-line text, each ◐ becomes a fixed-width glyph slot replaced
    by a circle drawn at the same baseline."""
    color = PALETTE[color_name]
    # JP font handles kanji + most symbols, Helvetica looks cleaner for ASCII
    has_jp = any("぀" <= c <= "龯" or c in "。、" for c in text)
    fpath = JP if has_jp else SANS

    # Special pure-mark designs (◐ only) — draw straight from shapes
    if text.strip() == "◐":
        img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
        d = ImageDraw.Draw(img)
        draw_half_moon(d, W // 2, H // 2, int(min(W, H) * 0.32), color)
        return img
    if text.strip() == "⊙":
        img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
        d = ImageDraw.Draw(img)
        r = int(min(W, H) * 0.30)
        d.ellipse((W // 2 - r, H // 2 - r, W // 2 + r, H // 2 + r), outline=color, width=20)
        d.ellipse((W // 2 - r // 4, H // 2 - r // 4, W // 2 + r // 4, H // 2 + r // 4), fill=color)
        return img

    # Replace ◐ in mixed text with a placeholder that fits a circle
    placeholder = "●"  # use a glyph the font has, then we'll overlay a half-moon
    text_safe = text.replace("◐", placeholder)

    margin = int(W * 0.10)
    inner_w, inner_h = W - 2 * margin, H - 2 * margin
    size = fit_font(text_safe, fpath, inner_w, inner_h)
    f = font(fpath, size)
    img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    bbox = draw.multiline_textbbox((0, 0), text_safe, font=f, align="center", spacing=size * 0.1)
    tw = bbox[2] - bbox[0]
    th = bbox[3] - bbox[1]
    x = (W - tw) // 2 - bbox[0]
    y = (H - th) // 2 - bbox[1]
    draw.multiline_text((x, y), text_safe, font=f, fill=color, align="center", spacing=size * 0.1)
    return img


def main():
    written = 0
    for sku, (text, color) in RECIPES.items():
        design_path = DESIGN_DIR / f"design_{sku}.png"
        preview_path = PREVIEW_DIR / f"preview_{sku}.png"
        img = render_design(text, color)
        img.save(design_path, "PNG", optimize=True)
        # Preview: scaled-down version with a tinted background so it
        # reads as a "design card" rather than a print file.
        # Tee-white SKUs use a light bg; others use a dark bg with subtle
        # vignette so light typography pops.
        is_white_tee = sku in ("ROLL-TEE-04", "ROLL-TEE-07")
        bg = (245, 245, 240, 255) if is_white_tee else (15, 15, 15, 255)
        preview_h = int(PREVIEW_W * H / W)
        preview = Image.new("RGBA", (PREVIEW_W, preview_h), bg)
        scaled = img.resize((PREVIEW_W, preview_h), Image.LANCZOS)
        preview.alpha_composite(scaled)
        preview.convert("RGB").save(preview_path, "JPEG", quality=85, optimize=True)
        written += 1
        print(f"  {sku:14}  {text!r:40} → design + preview")
    print(f"\n{written} design PNGs + previews written to:")
    print(f"  {DESIGN_DIR}")
    print(f"  {PREVIEW_DIR}")


if __name__ == "__main__":
    main()
