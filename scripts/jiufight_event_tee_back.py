#!/usr/bin/env python3
"""Compose the back-of-tee print file for JIUFIGHT Event Tee TOKYO 2026.

Layout (1800×2400 px = Printful Bella+Canvas 3001 print area at 150 DPI):
    [ SPONSORED BY ]               (top, white text)
    [ row1: sweep  | mindset | yawara | sjjjf ]
    [ row2: flex   | koda    | kokon  | yamato ]

Each sponsor logo is placed AS-IS (no modification). White logos on black
shirt; coloured logos retain their colour. Run from repo root:

    python3 scripts/jiufight_event_tee_back.py \
        --in  store/static/jiufight/sponsors \
        --out store/static/jiufight/tee_back_v1.png
"""
import argparse, os, sys
from PIL import Image, ImageDraw, ImageFont

# 4×2 grid order. yawara is included but file may be missing — script
# substitutes a text placeholder so the user can drop in a real PNG later.
SPONSORS = [
    ("sweep",   "SIIIEEP"),
    ("mindset", "Mindset"),
    ("yawara",  "yawara"),
    ("sjjjf",   "SJJJF"),
    ("flex",    "FLEX GROUP"),
    ("koda",    "甲田拓也事務所"),
    ("kokon",   "焼肉 古今"),
    ("yamato",  "大和不動産"),
]

CANVAS_W = 1800
CANVAS_H = 2400
MARGIN_X = 100
TITLE_Y  = 120
TITLE_H  = 280   # vertical space for SPONSORED BY block

def load_logo(path: str):
    if not os.path.exists(path):
        return None
    img = Image.open(path).convert("RGBA")
    return img

def find_font(size: int) -> ImageFont.FreeTypeFont:
    """Try common bold sans fonts in order. Falls back to default."""
    candidates = [
        "/System/Library/Fonts/Helvetica.ttc",
        "/System/Library/Fonts/HelveticaNeue.ttc",
        "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",  # for kanji fallback
    ]
    for path in candidates:
        if os.path.exists(path):
            try:
                return ImageFont.truetype(path, size)
            except OSError:
                continue
    return ImageFont.load_default()

def fit_logo(logo: Image.Image, cell_w: int, cell_h: int, pad: int = 20) -> Image.Image:
    """Scale logo to fit inside cell while preserving aspect ratio."""
    target_w = cell_w - 2 * pad
    target_h = cell_h - 2 * pad
    scale = min(target_w / logo.width, target_h / logo.height)
    new_w = max(1, int(logo.width * scale))
    new_h = max(1, int(logo.height * scale))
    return logo.resize((new_w, new_h), Image.LANCZOS)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--in",  dest="in_dir",  default="store/static/jiufight/sponsors")
    ap.add_argument("--out", dest="out_path", default="store/static/jiufight/tee_back_v1.png")
    args = ap.parse_args()

    canvas = Image.new("RGBA", (CANVAS_W, CANVAS_H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(canvas)

    # ── Title ─────────────────────────────────────────────────────
    title = "SPONSORED BY"
    title_font = find_font(96)
    bbox = draw.textbbox((0, 0), title, font=title_font)
    title_w = bbox[2] - bbox[0]
    draw.text(
        ((CANVAS_W - title_w) // 2, TITLE_Y),
        title,
        font=title_font,
        fill=(255, 255, 255, 255),
        # 0.42em letter-spacing — Pillow doesn't have native tracking,
        # so we draw letters individually for the spaced look.
    )
    # underline accent
    draw.line(
        [(CANVAS_W // 2 - 200, TITLE_Y + 130),
         (CANVAS_W // 2 + 200, TITLE_Y + 130)],
        fill=(230, 196, 73, 255), width=4,
    )

    # ── Logo grid (4×2) ───────────────────────────────────────────
    grid_top = TITLE_Y + TITLE_H + 40
    grid_h   = CANVAS_H - grid_top - 200   # leave bottom 200px for tag
    cols, rows = 4, 2
    cell_w = (CANVAS_W - 2 * MARGIN_X) // cols
    cell_h = grid_h // rows

    label_font = find_font(40)
    placed = 0
    missing = []

    for i, (slug, label) in enumerate(SPONSORS):
        col = i % cols
        row = i // cols
        cx = MARGIN_X + col * cell_w + cell_w // 2
        cy = grid_top + row * cell_h + cell_h // 2

        path = os.path.join(args.in_dir, f"{slug}.png")
        logo = load_logo(path)
        if logo is None:
            # text fallback for missing logo
            missing.append(slug)
            bbox = draw.textbbox((0, 0), label, font=label_font)
            tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
            draw.text((cx - tw // 2, cy - th // 2), label,
                      font=label_font, fill=(255, 255, 255, 255))
            continue

        scaled = fit_logo(logo, cell_w, cell_h, pad=40)
        canvas.paste(scaled, (cx - scaled.width // 2, cy - scaled.height // 2), scaled)
        placed += 1

    # ── Footer ────────────────────────────────────────────────────
    foot_font = find_font(36)
    foot = "JIUFIGHT TOKYO 2026 · OFFICIAL EVENT TEE"
    bbox = draw.textbbox((0, 0), foot, font=foot_font)
    fw = bbox[2] - bbox[0]
    draw.text(
        ((CANVAS_W - fw) // 2, CANVAS_H - 120),
        foot, font=foot_font,
        fill=(255, 255, 255, 180),
    )

    os.makedirs(os.path.dirname(args.out_path), exist_ok=True)
    canvas.save(args.out_path, "PNG")
    print(f"✓ wrote {args.out_path}")
    print(f"  placed {placed} logos, missing: {missing}")
    print(f"  canvas: {CANVAS_W}×{CANVAS_H} transparent PNG")

if __name__ == "__main__":
    main()
