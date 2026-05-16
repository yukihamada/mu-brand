#!/usr/bin/env python3
"""Compose the front-of-tee print file for JIUFIGHT Event Tee TOKYO 2026.

Layout (1800×2400 px = Printful Bella+Canvas 3001 print area at 150 DPI):

    ┌────────────────────────┐
    │                        │
    │                        │
    │      [JIUFIGHT]        │   ← existing brand logo, white-on-black
    │                        │
    │       TOKYO 2026       │   ← tagline, white
    │  ─────  ●  ─────       │   ← gold accent rule
    │      OFFICIAL EVENT    │
    │                        │
    └────────────────────────┘

Run:
    python3 scripts/jiufight_event_tee_front.py \
        --logo store/static/jiufight/sponsors/jiufight_logo.png \
        --out  store/static/jiufight/tee_front_v1.png
"""
import argparse, os
from PIL import Image, ImageDraw, ImageFont

CANVAS_W = 1800
CANVAS_H = 2400

def find_font(size: int) -> ImageFont.FreeTypeFont:
    candidates = [
        "/System/Library/Fonts/HelveticaNeue.ttc",
        "/System/Library/Fonts/Helvetica.ttc",
        "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
    ]
    for path in candidates:
        if os.path.exists(path):
            try:
                return ImageFont.truetype(path, size)
            except OSError:
                continue
    return ImageFont.load_default()

def invert_to_white(logo: Image.Image) -> Image.Image:
    """Convert a black-on-transparent logo to white-on-transparent.
    Preserves alpha channel; flips only the luminance of the RGB."""
    logo = logo.convert("RGBA")
    pixels = logo.load()
    for y in range(logo.height):
        for x in range(logo.width):
            r, g, b, a = pixels[x, y]
            if a > 0:
                # Anything visible becomes pure white at the original alpha
                pixels[x, y] = (255, 255, 255, a)
    return logo

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--logo", default="store/static/jiufight/sponsors/jiufight_logo.png")
    ap.add_argument("--out",  default="store/static/jiufight/tee_front_v1.png")
    ap.add_argument("--keep-color", action="store_true",
                    help="don't invert logo to white (use original colors)")
    args = ap.parse_args()

    canvas = Image.new("RGBA", (CANVAS_W, CANVAS_H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(canvas)

    # ── Main logo (top-center, large) ─────────────────────────────
    logo = Image.open(args.logo).convert("RGBA")
    if not args.keep_color:
        logo = invert_to_white(logo)
    # scale to 80% canvas width
    target_w = int(CANVAS_W * 0.78)
    scale = target_w / logo.width
    new_w = int(logo.width * scale)
    new_h = int(logo.height * scale)
    logo = logo.resize((new_w, new_h), Image.LANCZOS)
    logo_x = (CANVAS_W - new_w) // 2
    logo_y = (CANVAS_H - new_h) // 2 - 200
    canvas.paste(logo, (logo_x, logo_y), logo)

    # ── Tagline ───────────────────────────────────────────────────
    tagline = "TOKYO 2026"
    tag_font = find_font(140)
    bbox = draw.textbbox((0, 0), tagline, font=tag_font)
    tw = bbox[2] - bbox[0]
    tag_y = logo_y + new_h + 120
    # Letter-spaced manually for the wider, sober look.
    spaced = " ".join(list(tagline.replace(" ", "·")))
    spaced = spaced.replace(" · ", "  ·  ")
    bbox2 = draw.textbbox((0, 0), spaced, font=tag_font)
    tw2 = bbox2[2] - bbox2[0]
    draw.text(((CANVAS_W - tw2) // 2, tag_y), spaced,
              font=tag_font, fill=(255, 255, 255, 255))

    # ── Gold rule under the tagline ───────────────────────────────
    rule_y = tag_y + 200
    cx = CANVAS_W // 2
    draw.line([(cx - 280, rule_y), (cx - 30, rule_y)], fill=(230, 196, 73, 255), width=4)
    draw.line([(cx + 30, rule_y), (cx + 280, rule_y)], fill=(230, 196, 73, 255), width=4)
    # gold dot center
    draw.ellipse([(cx - 8, rule_y - 8), (cx + 8, rule_y + 8)], fill=(230, 196, 73, 255))

    # ── Sub-tagline ───────────────────────────────────────────────
    sub = "OFFICIAL EVENT TEE"
    sub_font = find_font(50)
    spaced_sub = " ".join(list(sub))  # very wide tracking
    bbox = draw.textbbox((0, 0), spaced_sub, font=sub_font)
    sw = bbox[2] - bbox[0]
    draw.text(
        ((CANVAS_W - sw) // 2, rule_y + 60),
        spaced_sub, font=sub_font,
        fill=(255, 255, 255, 220),
    )

    os.makedirs(os.path.dirname(args.out), exist_ok=True)
    canvas.save(args.out, "PNG")
    print(f"✓ wrote {args.out}")
    print(f"  canvas: {CANVAS_W}×{CANVAS_H} transparent PNG")
    print(f"  logo: {new_w}×{new_h} {'(white-inverted)' if not args.keep_color else '(original colors)'}")

if __name__ == "__main__":
    main()
