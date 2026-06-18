#!/usr/bin/env python3
"""Generate 10 JiuFight T-shirt mockup designs (front + back PNGs).

Output: store/static/jiufight/products/{01..10}_{front,back}.png
"""
from __future__ import annotations
import os, math
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont, ImageOps

ROOT = Path(__file__).resolve().parent.parent
LOGOS_DIR = ROOT / "store" / "static" / "yawara-cup" / "logos"
OUT_DIR = ROOT / "store" / "static" / "jiufight" / "products"
OUT_DIR.mkdir(parents=True, exist_ok=True)

FONT_BOLD = "/System/Library/Fonts/ヒラギノ角ゴシック W7.ttc"
FONT_REG = "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc"
FONT_MINCHO = "/System/Library/Fonts/ヒラギノ明朝 ProN.ttc"

CANVAS = (1600, 1900)  # T-shirt mockup canvas

# Logos (in display order: SJJJF + YAWARA first, then 6 sponsors)
LOGO_ORDER = [
    ("sjjjf",   LOGOS_DIR / "sjjjf.png"),
    ("yawara",  LOGOS_DIR / "yawara.png"),
    ("siiieep", LOGOS_DIR / "siiieep.png"),
    ("mindset", LOGOS_DIR / "mindset.png"),
    ("kokon",   LOGOS_DIR / "kokon.png"),
    ("flex",    LOGOS_DIR / "flex.png"),
    ("kouda",   LOGOS_DIR / "kouda.png"),
    ("daiwa",   LOGOS_DIR / "daiwa.png"),
]


def load_logo(name: str, size: tuple[int, int], white: bool = False) -> Image.Image:
    """Load a logo PNG, fit into the given box, optionally recolor to white."""
    src = next(p for n, p in LOGO_ORDER if n == name)
    img = Image.open(src).convert("RGBA")
    img.thumbnail(size, Image.LANCZOS)
    if white:
        # Recolor non-transparent pixels to near-white
        r, g, b, a = img.split()
        white_img = Image.new("RGBA", img.size, (245, 245, 240, 0))
        white_img.putalpha(a)
        img = white_img
    return img


def tshirt_silhouette(color: tuple[int, int, int]) -> Image.Image:
    """Draw a flat T-shirt mockup silhouette."""
    w, h = CANVAS
    img = Image.new("RGBA", (w, h), (240, 240, 238, 255))
    d = ImageDraw.Draw(img)

    # Studio backdrop shadow
    d.ellipse([(w*0.15, h*0.88), (w*0.85, h*0.96)], fill=(220, 220, 218, 255))

    # T-shirt body polygon
    poly = [
        (w*0.20, h*0.12),  # left shoulder
        (w*0.36, h*0.06),  # left neck dip start
        (w*0.42, h*0.08), (w*0.50, h*0.10), (w*0.58, h*0.08),  # neck curve
        (w*0.64, h*0.06),  # right neck dip end
        (w*0.80, h*0.12),  # right shoulder
        (w*0.95, h*0.22),  # right sleeve
        (w*0.83, h*0.30),  # right armpit
        (w*0.83, h*0.86),  # bottom right
        (w*0.17, h*0.86),  # bottom left
        (w*0.17, h*0.30),  # left armpit
        (w*0.05, h*0.22),  # left sleeve
    ]
    d.polygon(poly, fill=color + (255,))

    # Collar ring
    d.arc(
        [(w*0.40, h*0.04), (w*0.60, h*0.13)],
        start=0, end=180,
        fill=tuple(max(0, c-25) for c in color) + (255,),
        width=4,
    )
    return img


def design_area_box(view: str = "front") -> tuple[int, int, int, int]:
    """Return (left, top, right, bottom) for printable design area."""
    w, h = CANVAS
    return (int(w*0.27), int(h*0.20), int(w*0.73), int(h*0.66))


def render_text(text: str, size: int, color: tuple, font_path: str = FONT_BOLD,
                spacing: float = 0.05) -> Image.Image:
    """Render text with custom letter spacing into a tight image."""
    font = ImageFont.truetype(font_path, size)
    spacing_px = int(size * spacing)
    chars = list(text)
    widths = []
    for ch in chars:
        bbox = font.getbbox(ch)
        widths.append(bbox[2] - bbox[0])
    total_w = sum(widths) + spacing_px * (len(chars) - 1)
    asc = max(font.getbbox(c)[3] for c in chars if c.strip())
    img = Image.new("RGBA", (total_w + 20, asc + 20), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    x = 10
    for ch, w in zip(chars, widths):
        d.text((x, 5), ch, font=font, fill=color)
        x += w + spacing_px
    return img


def paste_logos_grid(canvas: Image.Image, names: list[str], box: tuple, cols: int = 4,
                     white: bool = False, gap: int = 16, cell_pad: int = 12,
                     cell_bg: tuple | None = (255, 255, 255, 255)):
    """Paste logos in a grid inside the given bbox."""
    l, t, r, b = box
    n = len(names)
    rows = math.ceil(n / cols)
    cell_w = (r - l - gap * (cols - 1)) // cols
    cell_h = (b - t - gap * (rows - 1)) // rows
    for i, name in enumerate(names):
        cr, cc = divmod(i, cols)
        cx = l + cc * (cell_w + gap)
        cy = t + cr * (cell_h + gap)
        # Cell background
        if cell_bg is not None:
            cell_img = Image.new("RGBA", (cell_w, cell_h), cell_bg)
            canvas.alpha_composite(cell_img, (cx, cy))
        # Logo
        logo = load_logo(name, (cell_w - cell_pad*2, cell_h - cell_pad*2), white=white)
        lx = cx + (cell_w - logo.width) // 2
        ly = cy + (cell_h - logo.height) // 2
        canvas.alpha_composite(logo, (lx, ly))


def paste_logos_row(canvas: Image.Image, names: list[str], y_center: int,
                    x_start: int, x_end: int, max_h: int, white: bool = False, gap: int = 10):
    """Paste logos in a single row, centered around y_center."""
    n = len(names)
    cell_w = (x_end - x_start - gap * (n - 1)) // n
    for i, name in enumerate(names):
        logo = load_logo(name, (cell_w, max_h), white=white)
        lx = x_start + i * (cell_w + gap) + (cell_w - logo.width) // 2
        ly = y_center - logo.height // 2
        canvas.alpha_composite(logo, (lx, ly))


# ═══════════════════════════════════════════════════════════════════════
# 10 DESIGN VARIANTS — Each function returns (front_img, back_img, meta)
# ═══════════════════════════════════════════════════════════════════════

W, H = CANVAS

def design_01_classic_black():
    """01 — Classic Black Tournament: JIUFIGHT front, 8 sponsors back grid."""
    color = (26, 26, 26)
    front = tshirt_silhouette(color)
    # Big JIUFIGHT
    txt = render_text("JIUFIGHT", 180, (245, 245, 240, 255))
    front.alpha_composite(txt, ((W - txt.width)//2, int(H*0.30)))
    sub = render_text("2026 · TOKYO", 42, (200, 200, 200, 220), font_path=FONT_REG, spacing=0.25)
    front.alpha_composite(sub, ((W - sub.width)//2, int(H*0.42)))

    back = tshirt_silhouette(color)
    # SJJJF + YAWARA top row (bigger)
    paste_logos_row(back, ["sjjjf", "yawara"], int(H*0.27), int(W*0.30), int(W*0.70), 180)
    # 6 sponsors grid 3x2
    paste_logos_grid(back, ["siiieep","mindset","kokon","flex","kouda","daiwa"],
                     (int(W*0.27), int(H*0.40), int(W*0.73), int(H*0.66)), cols=3)
    return front, back, {"name": "Classic Black", "tagline": "米国BJJ大会の定番。前は大見出し、背中にスポンサー格付け。", "color": "Black", "price": 4900}


def design_02_premium_white():
    """02 — Premium White Belt Band: minimal front, belt band back."""
    color = (245, 245, 240)
    front = tshirt_silhouette(color)
    # JIUFIGHT framed
    txt = render_text("JIUFIGHT", 110, (20, 20, 20, 255), spacing=0.18)
    cx = (W - txt.width)//2; cy = int(H*0.35)
    front.alpha_composite(txt, (cx, cy))
    d = ImageDraw.Draw(front)
    d.line([(cx-30, cy-12), (cx+txt.width+30, cy-12)], fill=(20,20,20,255), width=3)
    d.line([(cx-30, cy+txt.height+8), (cx+txt.width+30, cy+txt.height+8)], fill=(20,20,20,255), width=3)
    sub = render_text("二〇二六", 36, (60, 60, 60, 240), font_path=FONT_MINCHO, spacing=0.3)
    front.alpha_composite(sub, ((W - sub.width)//2, int(H*0.46)))

    back = tshirt_silhouette(color)
    d = ImageDraw.Draw(back)
    label = render_text("PARTNERS", 34, (120, 120, 120, 230), font_path=FONT_REG, spacing=0.4)
    back.alpha_composite(label, ((W - label.width)//2, int(H*0.24)))
    # Black belt band
    band_top = int(H*0.30); band_bot = int(H*0.40)
    d.rectangle([(int(W*0.18), band_top), (int(W*0.82), band_bot)], fill=(20, 20, 20, 255))
    paste_logos_row(back, [n for n,_ in LOGO_ORDER], (band_top+band_bot)//2,
                    int(W*0.20), int(W*0.80), int((band_bot-band_top)*0.75), white=True, gap=4)
    return front, back, {"name": "Premium White", "tagline": "F1の黒帯バンド。前面ミニマル、背中の帯にスポンサーが整列。", "color": "White", "price": 7800}


def design_03_battle_red():
    """03 — Battle Red Kanji: red tee, kanji front, sponsors small."""
    color = (122, 31, 31)
    front = tshirt_silhouette(color)
    # 柔 kanji huge
    try:
        font = ImageFont.truetype(FONT_MINCHO, 480)
    except Exception:
        font = ImageFont.truetype(FONT_BOLD, 480)
    d = ImageDraw.Draw(front)
    bbox = font.getbbox("柔")
    kw = bbox[2] - bbox[0]; kh = bbox[3] - bbox[1]
    d.text(((W - kw)//2 - bbox[0], int(H*0.20)), "柔", font=font, fill=(245, 245, 240, 255))
    # JIUFIGHT small below
    txt = render_text("JIUFIGHT", 70, (245, 245, 240, 240), spacing=0.25)
    front.alpha_composite(txt, ((W - txt.width)//2, int(H*0.58)))

    back = tshirt_silhouette(color)
    paste_logos_grid(back, [n for n,_ in LOGO_ORDER],
                     (int(W*0.27), int(H*0.22), int(W*0.73), int(H*0.60)), cols=4)
    return front, back, {"name": "Battle Red", "tagline": "巨大な「柔」+ 紅色。武道の象徴。日本らしさ最大。", "color": "Red", "price": 5900}


def design_04_stealth_black():
    """04 — Stealth Black: black on black, subtle JF."""
    color = (18, 18, 18)
    front = tshirt_silhouette(color)
    # JF subtle tonal
    txt = render_text("JF", 380, (45, 45, 45, 255), spacing=0)
    front.alpha_composite(txt, ((W - txt.width)//2, int(H*0.26)))
    sub = render_text("JIUFIGHT · STEALTH", 28, (90, 90, 90, 220), font_path=FONT_REG, spacing=0.3)
    front.alpha_composite(sub, ((W - sub.width)//2, int(H*0.56)))

    back = tshirt_silhouette(color)
    paste_logos_grid(back, [n for n,_ in LOGO_ORDER],
                     (int(W*0.30), int(H*0.32), int(W*0.70), int(H*0.58)), cols=4,
                     cell_bg=(35, 35, 35, 255))
    return front, back, {"name": "Stealth Black", "tagline": "黒に黒のトーン・オン・トーン。最も控えめで上品。", "color": "Black", "price": 6800}


def design_05_gold_edition():
    """05 — Gold Edition: black + gold accent."""
    color = (12, 12, 12)
    front = tshirt_silhouette(color)
    txt = render_text("JIUFIGHT", 160, (230, 196, 73, 255), spacing=0.08)
    front.alpha_composite(txt, ((W - txt.width)//2, int(H*0.30)))
    # Decorative line
    d = ImageDraw.Draw(front)
    d.line([(int(W*0.30), int(H*0.43)), (int(W*0.70), int(H*0.43))], fill=(230, 196, 73, 255), width=2)
    sub = render_text("GOLD EDITION · 2026", 36, (230, 196, 73, 220), font_path=FONT_REG, spacing=0.35)
    front.alpha_composite(sub, ((W - sub.width)//2, int(H*0.46)))

    back = tshirt_silhouette(color)
    paste_logos_grid(back, [n for n,_ in LOGO_ORDER],
                     (int(W*0.27), int(H*0.24), int(W*0.73), int(H*0.62)), cols=4,
                     cell_bg=(230, 196, 73, 255))
    return front, back, {"name": "Gold Edition", "tagline": "黒×ゴールド。プレミアム限定モデル。", "color": "Black/Gold", "price": 8800}


def design_06_pocket_chest():
    """06 — Pocket Chest: small chest logo, big back grid."""
    color = (28, 28, 28)
    front = tshirt_silhouette(color)
    # Small chest emblem (left)
    txt = render_text("JF", 80, (245, 245, 240, 255))
    front.alpha_composite(txt, (int(W*0.62), int(H*0.20)))
    sub = render_text("JIUFIGHT", 20, (180, 180, 180, 200), font_path=FONT_REG, spacing=0.35)
    front.alpha_composite(sub, (int(W*0.62), int(H*0.25)))

    back = tshirt_silhouette(color)
    # Title at top
    title = render_text("JIUFIGHT 2026", 120, (245, 245, 240, 255), spacing=0.1)
    back.alpha_composite(title, ((W - title.width)//2, int(H*0.16)))
    paste_logos_grid(back, [n for n,_ in LOGO_ORDER],
                     (int(W*0.25), int(H*0.30), int(W*0.75), int(H*0.66)), cols=4)
    return front, back, {"name": "Pocket Chest", "tagline": "胸元に小さなJFマーク、背中に大きな大会タイトル＋全ロゴ。", "color": "Charcoal", "price": 5400}


def design_07_sleeve_wrap():
    """07 — Sleeve wrap: logos on sleeve area, big number on back."""
    color = (245, 245, 240)
    front = tshirt_silhouette(color)
    # Center wordmark
    txt = render_text("JIUFIGHT", 140, (20, 20, 20, 255), spacing=0.15)
    front.alpha_composite(txt, ((W - txt.width)//2, int(H*0.33)))
    # Sleeve sponsor strip (left sleeve area)
    paste_logos_grid(front, ["yawara", "sjjjf"],
                     (int(W*0.06), int(H*0.22), int(W*0.16), int(H*0.32)), cols=1, cell_pad=4,
                     cell_bg=None)
    paste_logos_grid(front, ["siiieep", "mindset"],
                     (int(W*0.84), int(H*0.22), int(W*0.94), int(H*0.32)), cols=1, cell_pad=4,
                     cell_bg=None)

    back = tshirt_silhouette(color)
    # Big number 01
    try:
        font = ImageFont.truetype(FONT_BOLD, 600)
    except Exception:
        font = ImageFont.load_default()
    d = ImageDraw.Draw(back)
    bbox = font.getbbox("01")
    nw = bbox[2] - bbox[0]
    d.text(((W - nw)//2 - bbox[0], int(H*0.18)), "01", font=font, fill=(20, 20, 20, 255))
    # Sponsors small below
    paste_logos_row(back, ["kokon", "flex", "kouda", "daiwa"], int(H*0.60),
                    int(W*0.25), int(W*0.75), 90)
    return front, back, {"name": "Sleeve Wrap", "tagline": "袖にスポンサー、背中に巨大な「01」=第1回大会。", "color": "White", "price": 6400}


def design_08_champion():
    """08 — Champion: bold winner-style design."""
    color = (12, 12, 12)
    front = tshirt_silhouette(color)
    # CHAMPION arched
    txt = render_text("CHAMPION", 90, (230, 196, 73, 255), spacing=0.4)
    front.alpha_composite(txt, ((W - txt.width)//2, int(H*0.20)))
    big = render_text("JIUFIGHT", 200, (245, 245, 240, 255), spacing=0.06)
    front.alpha_composite(big, ((W - big.width)//2, int(H*0.30)))
    # YAWARA logo big underneath
    yawara = load_logo("yawara", (450, 120), white=True)
    front.alpha_composite(yawara, ((W - yawara.width)//2, int(H*0.50)))

    back = tshirt_silhouette(color)
    title = render_text("SUPPORTERS", 42, (180, 180, 180, 230), font_path=FONT_REG, spacing=0.4)
    back.alpha_composite(title, ((W - title.width)//2, int(H*0.20)))
    paste_logos_grid(back, [n for n,_ in LOGO_ORDER if n not in ("yawara",)],
                     (int(W*0.27), int(H*0.28), int(W*0.73), int(H*0.62)), cols=4,
                     cell_bg=(255, 255, 255, 255))
    return front, back, {"name": "Champion", "tagline": "YAWARA優勝者モデル。CHAMPION + 大きなJIUFIGHT。", "color": "Black/Gold", "price": 7400}


def design_09_mu_native():
    """09 — MU Native: constellation, 1-of-1 serial."""
    color = (10, 10, 10)
    front = tshirt_silhouette(color)
    # Big JF in MU yellow
    try:
        font = ImageFont.truetype(FONT_REG, 540)
    except Exception:
        font = ImageFont.load_default()
    d = ImageDraw.Draw(front)
    bbox = font.getbbox("JF")
    nw = bbox[2] - bbox[0]
    d.text(((W - nw)//2 - bbox[0], int(H*0.22)), "JF", font=font, fill=(230, 196, 73, 255))
    sub = render_text("JIUFIGHT", 28, (160, 160, 160, 220), font_path=FONT_REG, spacing=0.5)
    front.alpha_composite(sub, ((W - sub.width)//2, int(H*0.55)))
    # 8 dot constellation
    cx = W // 2; cy = int(H * 0.66); r = 8
    for i in range(8):
        x = int(W*0.30) + i * int(W*0.40/7)
        d.ellipse([(x-r, cy-r), (x+r, cy+r)], fill=(230, 196, 73, 230))

    back = tshirt_silhouette(color)
    paste_logos_grid(back, [n for n,_ in LOGO_ORDER],
                     (int(W*0.27), int(H*0.26), int(W*0.73), int(H*0.58)), cols=4,
                     cell_bg=(28, 28, 28, 255))
    ts = render_text("JF · 2026-05-16T22:41 · 1-OF-1", 22, (110, 110, 110, 200),
                     font_path=FONT_REG, spacing=0.18)
    back.alpha_composite(ts, ((W - ts.width)//2, int(H*0.78)))
    return front, back, {"name": "MU Native (1-of-1)", "tagline": "MUブランド一貫性。constellation+1-of-1 serial。§27適用。", "color": "Off-black", "price": 6400}


def design_10_kanji_bushido():
    """10 — Kanji Bushido: vertical kanji + serial."""
    color = (24, 24, 24)
    front = tshirt_silhouette(color)
    d = ImageDraw.Draw(front)
    try:
        font = ImageFont.truetype(FONT_MINCHO, 220)
    except Exception:
        font = ImageFont.load_default()
    chars = list("武術礼節")
    cx = W // 2
    start_y = int(H * 0.18)
    for i, ch in enumerate(chars):
        bbox = font.getbbox(ch)
        cw = bbox[2] - bbox[0]
        d.text((cx - cw//2 - bbox[0], start_y + i * 240), ch, font=font, fill=(245, 245, 240, 255))
    # Small JIUFIGHT below
    sub = render_text("JIUFIGHT", 50, (200, 200, 200, 230), font_path=FONT_REG, spacing=0.32)
    front.alpha_composite(sub, ((W - sub.width)//2, int(H*0.74)))

    back = tshirt_silhouette(color)
    paste_logos_grid(back, [n for n,_ in LOGO_ORDER],
                     (int(W*0.27), int(H*0.24), int(W*0.73), int(H*0.60)), cols=4,
                     cell_bg=(255, 255, 255, 255))
    return front, back, {"name": "Bushido", "tagline": "縦書きの「武術礼節」+ 全ロゴ。書道風で日本らしい。", "color": "Charcoal", "price": 6900}


# ═══════════════════════════════════════════════════════════════════════
# Main runner
# ═══════════════════════════════════════════════════════════════════════
DESIGNS = [
    design_01_classic_black,
    design_02_premium_white,
    design_03_battle_red,
    design_04_stealth_black,
    design_05_gold_edition,
    design_06_pocket_chest,
    design_07_sleeve_wrap,
    design_08_champion,
    design_09_mu_native,
    design_10_kanji_bushido,
]


def main():
    metas = []
    for i, fn in enumerate(DESIGNS, 1):
        print(f"▶ Generating design {i:02d}: {fn.__name__}")
        front, back, meta = fn()
        meta["id"] = i
        meta["slug"] = fn.__name__.replace("design_", "")
        front_path = OUT_DIR / f"{i:02d}_front.png"
        back_path = OUT_DIR / f"{i:02d}_back.png"
        front.convert("RGB").save(front_path, "PNG", optimize=True)
        back.convert("RGB").save(back_path, "PNG", optimize=True)
        meta["front"] = f"jiufight/products/{front_path.name}"
        meta["back"] = f"jiufight/products/{back_path.name}"
        metas.append(meta)
        print(f"   → {front_path.name}, {back_path.name}")

    # Write meta JSON
    import json
    meta_path = OUT_DIR / "designs.json"
    with open(meta_path, "w") as f:
        json.dump(metas, f, ensure_ascii=False, indent=2)
    print(f"✅ {len(DESIGNS)} designs generated in {OUT_DIR}")
    print(f"   Meta: {meta_path}")


if __name__ == "__main__":
    main()
