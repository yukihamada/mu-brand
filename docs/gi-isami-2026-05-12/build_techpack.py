#!/usr/bin/env python3
"""
Tech-pack: 実寸スケールで道着シルエット + 各スポンサー位置/サイズを正確に描画。
ISAMI などの刺繍工房に「ここに、この大きさで、この糸色で刺繍してください」と
渡せる業界標準フォーマット。

Scale: 1cm = 32px → A3 landscape (4960 × 3508 @ 300dpi) でフィット
"""
import io, os, urllib.parse
from PIL import Image, ImageDraw, ImageFont, ImageOps

ROOT = os.path.dirname(os.path.abspath(__file__))
OUT  = ROOT + "/techpack"
os.makedirs(OUT, exist_ok=True)

# ── scale ─────────────────────────────────────────────────
PX_PER_CM = 32

def cm(x): return int(x * PX_PER_CM)

# A3 landscape @ 300dpi
W, H = cm(40), cm(28)  # 1280 × 896 (margins included)

# Colors
BG = (250, 250, 248)           # off-white tech-pack background
GI = (18, 18, 20)               # gi black
GI_EDGE = (80, 80, 84)          # stitch line
PATCH_BORDER = (230, 196, 73)   # Old Gold (highlight)
PATCH_BG = (230, 196, 73, 28)   # transparent gold
TEXT_FG = (250, 250, 250)        # white thread embroidery
GOLD = (230, 196, 73)
ANNOT = (60, 60, 64)
GRID = (200, 200, 200)
ARROW = (40, 40, 44)

# ── font loader ───────────────────────────────────────────
def load_font(size, bold=False):
    paths = [
        # macOS system fonts
        "/System/Library/Fonts/Supplemental/Futura.ttc",
        "/Library/Fonts/Helvetica Neue.ttc",
        "/System/Library/Fonts/HelveticaNeue.ttc",
        "/System/Library/Fonts/Supplemental/Arial Bold.ttf" if bold else "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
    ]
    for p in paths:
        if os.path.exists(p):
            try:
                return ImageFont.truetype(p, size)
            except Exception:
                continue
    return ImageFont.load_default()

# ── data ──────────────────────────────────────────────────
# Format: (id, x_cm_from_center, y_cm_from_top, w_cm, h_cm, label, color, note)
# Coords are CENTER of the patch.

# JACKET FRONT — anchor: chest center, y starts at collar
FRONT = [
    # (x, y, w, h, label, color, note)
    (-13.0, 18.0, 8, 8, "MU",         "white", "Host"),
    ( 13.0, 18.0, 8, 8, "JiuFlow",    "white", "Host"),
    # Left sleeve outside (4 stacked, y 28→58, x -28)
    (-28.0, 28.0, 6, 6, "SOLUNA",     "white", ""),
    (-28.0, 36.0, 6, 6, "Koe",        "white", ""),
    (-28.0, 44.0, 6, 6, "KAGI",       "white", ""),
    (-28.0, 52.0, 6, 6, "PASHA",      "white", ""),
    # Right sleeve outside (4 stacked)
    ( 28.0, 28.0, 6, 6, "NOT A HOTEL","white", ""),
    ( 28.0, 36.0, 6, 6, "FiNANCiE",   "white", ""),
    ( 28.0, 44.0, 6, 6, "NEWT",       "white", ""),
    ( 28.0, 52.0, 6, 6, "焼肉古今",   "gold",  "Old Gold"),
    # Lower hem - left
    (-12.0, 64.0, 6, 6, "ATSUME",     "white", ""),
    (-12.0, 72.0, 6, 6, "GIFTMALL",   "white", ""),
    # Lower hem - right
    ( 12.0, 64.0, 6, 6, "VUILD",      "white", ""),
    ( 12.0, 72.0, 6, 6, "NESTING",    "white", ""),
]

# JACKET BACK — anchor: spine center
BACK = [
    # Top main: CYBRIDGE × ENABLER crest (heraldic shield)
    (0.0, 18.0, 24, 12, "CYBRIDGE × ENABLER", "white", "Heraldic crest, laurel wreath border"),
    # QR code center-back
    (0.0, 38.0, 10, 10, "[QR]\nwearmu.com/gi/01", "gold", "Embroidered QR, Old Gold thread, corner markers M/J/E/C"),
    # Sleeves visible from back (mirror)
    (-28.0, 28.0, 6, 6, "SOLUNA",     "white", ""),
    (-28.0, 36.0, 6, 6, "Koe",        "white", ""),
    (-28.0, 44.0, 6, 6, "KAGI",       "white", ""),
    (-28.0, 52.0, 6, 6, "PASHA",      "white", ""),
    ( 28.0, 28.0, 6, 6, "NOT A HOTEL","white", ""),
    ( 28.0, 36.0, 6, 6, "FiNANCiE",   "white", ""),
    ( 28.0, 44.0, 6, 6, "NEWT",       "white", ""),
    ( 28.0, 52.0, 6, 6, "焼肉古今",   "gold",  ""),
    # Collar inside (peeking out)
    (0.0, 9.0,  8, 3, "CASTER",       "white", "Inside collar, hidden"),
]

# PANTS — separate panel
PANTS = [
    # left thigh outside
    (-12.0, 22.0, 10, 10, "FiNANCiE",  "white", "Pants L thigh"),
    # right thigh outside (reserved)
    ( 12.0, 22.0, 10, 10, "(reserve)", "white", "Add later"),
    # belt tail tip
    ( 0.0,  -2.0, 4, 4, "MU",          "gold",  "Belt tail, Old Gold"),
]

# ── gi silhouette ──────────────────────────────────────────
def draw_jacket(d, cx, cy, label):
    """Draw a stylized BJJ jacket silhouette centered at (cx, cy)."""
    # Body torso
    half_w = cm(20)   # 40cm wide jacket body
    top    = cy - cm(0)  # collar
    bottom = cy + cm(80) # hem
    # Sleeves (extending sideways)
    sleeve_top    = cy + cm(2)
    sleeve_bottom = cy + cm(60)
    sleeve_outer  = cx + cm(38)  # outer edge of right sleeve

    # Body rectangle (with slight collar V)
    body = [
        (cx - half_w, top),
        (cx + half_w, top),
        (cx + half_w, bottom),
        (cx - half_w, bottom),
    ]
    d.polygon(body, fill=GI, outline=GI_EDGE)

    # Left sleeve
    d.polygon([
        (cx - half_w, sleeve_top),
        (cx - half_w - cm(18), sleeve_top + cm(2)),
        (cx - half_w - cm(18), sleeve_bottom),
        (cx - half_w, sleeve_bottom - cm(4)),
    ], fill=GI, outline=GI_EDGE)

    # Right sleeve
    d.polygon([
        (cx + half_w, sleeve_top),
        (cx + half_w + cm(18), sleeve_top + cm(2)),
        (cx + half_w + cm(18), sleeve_bottom),
        (cx + half_w, sleeve_bottom - cm(4)),
    ], fill=GI, outline=GI_EDGE)

    # Collar V
    d.polygon([
        (cx - cm(4), top),
        (cx + cm(4), top),
        (cx, top + cm(8)),
    ], fill=BG, outline=GI_EDGE)

    # Lapel stripes
    d.line([(cx - cm(4), top), (cx - cm(6), bottom)], fill=GI_EDGE, width=2)
    d.line([(cx + cm(4), top), (cx + cm(6), bottom)], fill=GI_EDGE, width=2)

    # Belt
    belt_y = cy + cm(50)
    d.rectangle([(cx - half_w - cm(3), belt_y - cm(2)), (cx + half_w + cm(3), belt_y + cm(2))],
                fill=(20,20,22), outline=GOLD)
    d.line([(cx + half_w - cm(8), belt_y), (cx + half_w + cm(2), belt_y)],
           fill=GOLD, width=2)

    # Label
    f = load_font(20, bold=True)
    d.text((cx, cy + cm(85)), label, font=f, fill=ANNOT, anchor="mm")


def draw_pants(d, cx, cy):
    half_w = cm(20)
    top    = cy
    bottom = cy + cm(50)
    # waist + two leg silhouette
    d.polygon([
        (cx - half_w, top),
        (cx + half_w, top),
        (cx + cm(12), bottom),
        (cx + cm(2),  bottom),
        (cx, top + cm(20)),  # crotch
        (cx - cm(2), bottom),
        (cx - cm(12), bottom),
    ], fill=GI, outline=GI_EDGE)
    # Belt sash (around waist)
    d.rectangle([(cx - half_w - cm(2), top - cm(4)), (cx + half_w + cm(2), top + cm(0))],
                fill=(20,20,22), outline=GOLD)
    f = load_font(20, bold=True)
    d.text((cx, bottom + cm(5)), "PANTS / BELT", font=f, fill=ANNOT, anchor="mm")


# ── patch drawer ───────────────────────────────────────────
def draw_patch(d, cx, cy, w_cm, h_cm, label, color, note=""):
    """Draw a patch box at center (cx, cy) with size w_cm x h_cm."""
    w = cm(w_cm); h = cm(h_cm)
    x1, y1 = cx - w//2, cy - h//2
    x2, y2 = cx + w//2, cy + h//2

    # Patch frame (gold outline = highlight)
    d.rectangle([x1, y1, x2, y2], outline=PATCH_BORDER, width=3)

    # Brand text (white = embroidery color, gold = Old Gold)
    color_rgb = TEXT_FG if color == "white" else GOLD
    # Pick font size proportional to box
    font_px = min(w // max(len(label.split('\n')[0]), 4) * 2, h // 3)
    font_px = max(14, min(font_px, 44))
    is_jp = any(0x3000 <= ord(c) <= 0x9fff for c in label)
    if is_jp:
        f = load_font(font_px - 2, bold=True)
    else:
        f = load_font(font_px, bold=True)
    # Multi-line support
    lines = label.split("\n")
    line_h = font_px + 4
    total_h = line_h * len(lines)
    y = cy - total_h // 2 + line_h // 2
    for line in lines:
        d.text((cx, y), line, font=f, fill=color_rgb, anchor="mm")
        y += line_h

    # Size annotation in small gold below the patch
    if note:
        anno = f"{int(w_cm)}×{int(h_cm)}cm · {note}"
    else:
        anno = f"{int(w_cm)}×{int(h_cm)}cm"
    fs = load_font(12)
    d.text((cx, y2 + 10), anno, font=fs, fill=GOLD, anchor="mt")


# ── build a panel ─────────────────────────────────────────
def build_panel(items, label, with_jacket=True):
    img = Image.new("RGB", (W, H), BG)
    d   = ImageDraw.Draw(img, "RGBA")

    # Grid lines (every 10cm)
    for x in range(0, W, cm(10)):
        d.line([(x, 0), (x, H)], fill=GRID, width=1)
    for y in range(0, H, cm(10)):
        d.line([(0, y), (W, y)], fill=GRID, width=1)
    # Scale ruler
    ruler_y = H - 30
    d.rectangle([20, ruler_y, 20 + cm(10), ruler_y + 8], fill=ANNOT)
    f10 = load_font(13, bold=True)
    d.text((20 + cm(10) + 8, ruler_y - 2), "10cm", font=f10, fill=ANNOT)

    cx = W // 2
    cy = cm(2)
    if with_jacket:
        draw_jacket(d, cx, cy, label)
    else:
        draw_pants(d, cx, cy)

    for (x_off, y_off, wcm, hcm, lbl, color, note) in items:
        pcx = cx + cm(x_off)
        pcy = cy + cm(y_off)
        draw_patch(d, pcx, pcy, wcm, hcm, lbl, color, note)

    # Title block (top-right)
    tf = load_font(28, bold=True)
    sf = load_font(14)
    d.text((W - 30, 30), label, font=tf, fill=ANNOT, anchor="ra")
    d.text((W - 30, 65), "MU × JiuFlow Sponsored Gi  /  TECH PACK v1  /  2026-05-12", font=sf, fill=ANNOT, anchor="ra")
    d.text((W - 30, 85), "Scale 1cm = 32px  ·  Wearer: 濱田優貴 A2  ·  Base: 黒パールウィーブ 350-450GSM", font=sf, fill=ANNOT, anchor="ra")
    return img


def main():
    front = build_panel(FRONT, "JACKET — FRONT 前面", with_jacket=True)
    back  = build_panel(BACK,  "JACKET — BACK 背面",  with_jacket=True)
    pants = build_panel(PANTS, "PANTS / BELT", with_jacket=False)

    front.save(OUT + "/01_front.jpg", quality=92)
    back.save( OUT + "/02_back.jpg",  quality=92)
    pants.save(OUT + "/03_pants.jpg", quality=92)
    print(f"✅ {OUT}/01_front.jpg")
    print(f"✅ {OUT}/02_back.jpg")
    print(f"✅ {OUT}/03_pants.jpg")

    # Also combine into a single PDF
    p1 = Image.open(OUT + "/01_front.jpg").convert("RGB")
    p2 = Image.open(OUT + "/02_back.jpg").convert("RGB")
    p3 = Image.open(OUT + "/03_pants.jpg").convert("RGB")
    p1.save(OUT + "/techpack.pdf", save_all=True, append_images=[p2, p3], resolution=200)
    print(f"✅ {OUT}/techpack.pdf")


if __name__ == "__main__":
    main()
