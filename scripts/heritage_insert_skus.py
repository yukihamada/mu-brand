#!/usr/bin/env python3
"""Heritage Edition: generate 2 mockup PNGs + INSERT 2 SKUs into products.db.

Spec (made-to-order, 30-unit minimum lot):
  - Fabric: 和歌山 Loopwheel 14oz tubular knit (Supima 70% + 純国産綿 30%)
  - Dye: 弟子屈 mineral dye (火山灰 + 鉄 媒染) — Black + Natural
  - Print: minimal silk screen, 「━◯━」 1cm 胸下
  - Sew: flatlock, 国内 縫製 (ヒラオカ縫製 等)
  - Wash: 単洗い + enzyme
  - Accessories: NFC tag + serial code 内刺繍 + 100年修繕券 + 北海道 オンコ木箱
  - Price: ¥35,000 / 着
  - Lot: 30 着 (Black 20 + Natural 10)
  - ETA: 60-90 日 (pre-order)

Two SKUs:
  MU-HER-001-LS-BLK-L · Heritage Tee · Mineral Black · L · inventory=20
  MU-HER-002-LS-NAT-L · Heritage Tee · Mineral Natural · L · inventory=10

Idempotent: if rows with these serial_codes exist, the script reports their
ids and exits without re-inserting. Mockup files are skipped if present
unless FORCE=1.

Notes:
  - Numbers like 14oz / 70% Supima / 60-90 days are *spec assumptions* (前提)
    pending real supplier confirmation. Do not quote externally without
    sourcing.
  - Image generation uses Gemini gemini-3-pro-image-preview per
    ~/.claude/CLAUDE.md image-generation rule. Pillow fallback runs only
    if the API call fails or key is missing.

Usage:
    cd /Users/yuki/workspace/mu-brand
    python3 scripts/heritage_insert_skus.py
    FORCE=1 python3 scripts/heritage_insert_skus.py   # regen mockups
"""
from __future__ import annotations
import base64
import io
import json
import os
import sqlite3
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

# ── Env: GEMINI_API_KEY from /Users/yuki/.env (memory feedback_gemini_key_env) ──
_ENV_FILE = Path("/Users/yuki/.env")
if _ENV_FILE.exists():
    for _line in _ENV_FILE.read_text().splitlines():
        _line = _line.strip()
        if not _line or _line.startswith("#") or "=" not in _line:
            continue
        _k, _, _v = _line.partition("=")
        if _k.strip() == "GEMINI_API_KEY":
            os.environ["GEMINI_API_KEY"] = _v.strip().strip("'\"")
os.environ.pop("GOOGLE_API_KEY", None)

# Optional deps — script also runs in "DB only" mode if missing
try:
    from google import genai
    from google.genai import types
    HAVE_GENAI = True
except Exception:
    HAVE_GENAI = False

try:
    from PIL import Image, ImageDraw, ImageFont
    HAVE_PIL = True
except Exception:
    HAVE_PIL = False

# ── Paths ──
ROOT = Path("/Users/yuki/workspace/mu-brand")
DB_PATH = ROOT / "store" / "products.db"
PROPOSALS_DIR = ROOT / "store" / "static" / "proposals"

GEMINI_MODEL = "gemini-3-pro-image-preview"
OUT_SIZE = 800
PNG_MAX_BYTES = 600_000

# ── SKU rows ──
# (serial_code, name, color, inventory, mockup_filename, garment_color_hex, design_color_hex)
SKUS = [
    {
        "serial_code": "MU-HER-001-LS-BLK-L",
        "brand": "heritage",
        "drop_num": 1,
        "name": "Heritage Tee · Mineral Black · L",
        "color": "BLK",
        "size": "L",
        "inventory": 20,
        "price_jpy": 35_000,
        "mockup_file": "heritage-mockup-1-black.png",
        "_garment_rgb": (14, 14, 14),
        "_accent_rgb": (210, 178, 88),
        "_label_color": "matte black w/ subtle warm undertone",
        "_dye_phrase": "iron-mordant volcanic-ash mineral black",
    },
    {
        "serial_code": "MU-HER-002-LS-NAT-L",
        "brand": "heritage",
        "drop_num": 2,
        "name": "Heritage Tee · Mineral Natural · L",
        "color": "NAT",
        "size": "L",
        "inventory": 10,
        "price_jpy": 35_000,
        "mockup_file": "heritage-mockup-2-natural.png",
        "_garment_rgb": (234, 224, 200),
        "_accent_rgb": (60, 50, 36),
        "_label_color": "undyed cotton natural / ecru ivory",
        "_dye_phrase": "undyed natural / kibata raw cotton",
    },
]

# ── Prompts ──
STYLE_PREAMBLE = (
    "Photorealistic e-commerce product mockup, square 1:1 800x800, ultra-clean "
    "minimalist studio photography in the style of a premium small-batch "
    "Japanese tee brand (think 1LDK, FreshService, BEAMS Heritage). Single "
    "garment, flat-laid against a soft off-white seamless paper backdrop with "
    "a faint natural shadow falling to the lower right. No people. No model. "
    "No watermarks. No invented brand text other than what is explicitly "
    "specified. Crisp focus, true-to-fabric color, gentle textile texture "
    "visible (heavyweight loopwheel cotton knit, slightly slubby surface)."
)

PRODUCT_BASE = (
    "The garment is a single short-sleeve crew-neck premium heavyweight "
    "tubular-knit cotton t-shirt, 14oz weight, ribbed crew collar, no side "
    "seams (loopwheel tubular knit), flatlock topstitching visible at the "
    "shoulder seam, soft rolled bottom hem. The fabric reads as Supima cotton "
    "blended with domestic Japanese cotton — luxurious, slightly slubby, with "
    "a deep saturated hand-dyed character."
)

DESIGN_NOTE = (
    "Center chest, positioned about 12cm below the collar, a tiny printed "
    "mark in the brand color, exactly 1cm tall: a short horizontal bar, "
    "a thin perfect circle ring, another short horizontal bar — rendered "
    "exactly as '━◯━' (a horizontal bar, a ring, a horizontal bar, total "
    "width about 18mm). Crisp single-color silk-screen print, matte ink, "
    "no halo. No other graphics, no other lettering, no logo. Below the mark "
    "on the inner collar tag (slightly visible flipping up), tiny Japanese "
    "text reading '無 / MU · made in JAPAN · Loopwheel 14oz · mineral dye'."
)

IP_NOTE = (
    "Do not render any third-party brand logos. Do not write any partner "
    "names. Do not place 'rvddw', 'reversal', 'Loopwheel' (as a logo), or "
    "any registered marks anywhere in the image. The only allowed glyph on "
    "the chest is the abstract ━◯━ mark. Inner-tag micro-text is the only "
    "other lettering."
)


def build_prompt(sku: dict) -> str:
    garment_color_phrase = {
        "BLK": (
            "a single deep mineral-dyed BLACK heavyweight tubular-knit cotton "
            "tee — the black has a slightly warm, charcoal undertone from "
            "iron-mordant volcanic-ash dye, not a pure synthetic jet black; "
            "subtle dye variation across the surface gives a hand-finished, "
            "patinated feel"
        ),
        "NAT": (
            "a single UNDYED natural / ecru ivory heavyweight tubular-knit "
            "cotton tee — the unbleached natural cotton color shows a soft "
            "slubby surface, faint warm cream tone, no dye applied; reads as "
            "raw kibata cotton with light enzyme-wash softening"
        ),
    }[sku["color"]]
    return (
        f"{STYLE_PREAMBLE}\n\n"
        f"Product: {garment_color_phrase}. {PRODUCT_BASE}\n\n"
        f"Design (centered, single-color silk-screen): {DESIGN_NOTE}\n\n"
        f"{IP_NOTE}\n\n"
        f"Render this as the hero product photo for SKU '{sku['serial_code']}'. "
        f"The garment fills about 78% of the frame. Treat this as a museum-grade, "
        f"long-archive 'best T-shirt' shot."
    )


# ── Gemini renderer ──
def gemini_render(client, prompt: str) -> bytes | None:
    try:
        resp = client.models.generate_content(
            model=GEMINI_MODEL,
            contents=[prompt],
            config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"]),
        )
    except Exception as exc:
        print(f"    gemini error: {type(exc).__name__}: {exc}")
        return None
    if not resp.candidates:
        print("    gemini returned no candidates")
        return None
    for part in resp.candidates[0].content.parts:
        inline = getattr(part, "inline_data", None)
        if not inline:
            continue
        data = inline.data
        if isinstance(data, str):
            data = base64.b64decode(data)
        try:
            im = Image.open(io.BytesIO(data)).convert("RGB")
        except Exception as exc:
            print(f"    pillow decode error: {exc}")
            return None
        if im.size != (OUT_SIZE, OUT_SIZE):
            im = im.resize((OUT_SIZE, OUT_SIZE), Image.LANCZOS)
        buf = io.BytesIO()
        im.save(buf, format="PNG", optimize=True)
        out = buf.getvalue()
        if len(out) <= PNG_MAX_BYTES:
            return out
        im2 = im.resize((640, 640), Image.LANCZOS)
        buf = io.BytesIO()
        im2.save(buf, format="PNG", optimize=True)
        return buf.getvalue()
    print("    gemini returned text-only (no image part)")
    return None


# ── Pillow fallback (minimal: solid garment + ━◯━ mark) ──
def pillow_fallback(sku: dict) -> bytes:
    W = H = OUT_SIZE
    garment = sku["_garment_rgb"]
    accent = sku["_accent_rgb"]
    img = Image.new("RGB", (W, H), (245, 243, 238))
    draw = ImageDraw.Draw(img)

    # Stylized flat-laid tee silhouette (rounded rectangle body, two sleeves).
    body_left, body_right = int(W * 0.20), int(W * 0.80)
    body_top, body_bot = int(H * 0.20), int(H * 0.90)
    # Sleeves (trapezoid-ish via polygons).
    sleeve_l = [
        (body_left, body_top + 12),
        (body_left - 80, body_top + 60),
        (body_left - 60, body_top + 150),
        (body_left + 4, body_top + 100),
    ]
    sleeve_r = [
        (body_right, body_top + 12),
        (body_right + 80, body_top + 60),
        (body_right + 60, body_top + 150),
        (body_right - 4, body_top + 100),
    ]
    draw.polygon(sleeve_l, fill=garment)
    draw.polygon(sleeve_r, fill=garment)
    # Body rounded rect.
    draw.rounded_rectangle(
        [body_left, body_top + 6, body_right, body_bot],
        radius=18, fill=garment,
    )
    # Crew neck cut-out.
    neck_w = int(W * 0.18)
    neck_h = 38
    cx = W // 2
    draw.rounded_rectangle(
        [cx - neck_w // 2, body_top - 8, cx + neck_w // 2, body_top + neck_h],
        radius=24, fill=(245, 243, 238),
    )
    # ━◯━ mark, centered 12cm below collar (~ 22% down the body).
    mark_y = body_top + int((body_bot - body_top) * 0.28)
    mark_color = accent
    bar_len = 24
    ring_r = 9
    gap = 6
    # Left bar
    draw.rectangle(
        [cx - bar_len - gap - ring_r, mark_y - 2,
         cx - gap - ring_r, mark_y + 2], fill=mark_color,
    )
    # Ring
    draw.ellipse(
        [cx - ring_r, mark_y - ring_r, cx + ring_r, mark_y + ring_r],
        outline=mark_color, width=2,
    )
    # Right bar
    draw.rectangle(
        [cx + gap + ring_r, mark_y - 2,
         cx + bar_len + gap + ring_r, mark_y + 2], fill=mark_color,
    )

    # Tiny caption below
    try:
        font = ImageFont.truetype("/System/Library/Fonts/Helvetica.ttc", 14)
    except Exception:
        font = ImageFont.load_default()
    caption = sku["serial_code"]
    bbox = draw.textbbox((0, 0), caption, font=font)
    tw = bbox[2] - bbox[0]
    draw.text(
        (cx - tw // 2, body_bot + 20),
        caption, fill=(120, 110, 90), font=font,
    )

    buf = io.BytesIO()
    img.save(buf, format="PNG", optimize=True)
    return buf.getvalue()


def ensure_mockup(client, sku: dict, force: bool) -> tuple[int, str]:
    out_path = PROPOSALS_DIR / sku["mockup_file"]
    if out_path.exists() and not force:
        return out_path.stat().st_size, "skip-exists"

    prompt = build_prompt(sku)
    method = "gemini"
    png_bytes: bytes | None = None

    if client is not None and HAVE_PIL:
        for attempt in range(2):
            png_bytes = gemini_render(client, prompt)
            if png_bytes and len(png_bytes) >= 40_000:
                break
            print(f"    attempt {attempt + 1}: {len(png_bytes) if png_bytes else 0} bytes")
            if attempt == 0:
                time.sleep(4)

    if not png_bytes:
        if not HAVE_PIL:
            print("    Pillow not installed; cannot fallback. Skipping image.")
            return 0, "no-pil"
        method = "pillow-fallback"
        png_bytes = pillow_fallback(sku)

    PROPOSALS_DIR.mkdir(parents=True, exist_ok=True)
    out_path.write_bytes(png_bytes)
    return len(png_bytes), method


def upsert_sku(conn: sqlite3.Connection, sku: dict) -> tuple[int, str]:
    """Returns (id, status). status ∈ {'inserted','exists'}."""
    cur = conn.cursor()
    cur.execute(
        "SELECT id FROM products WHERE serial_code = ?",
        (sku["serial_code"],),
    )
    row = cur.fetchone()
    if row:
        return row[0], "exists"

    now = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S")
    mockup_url = f"/static/proposals/{sku['mockup_file']}"
    prompt_text = json.dumps({
        "edition": "MU Heritage Edition",
        "fabric": "和歌山 Loopwheel 14oz tubular knit (想定 Supima 70% + 純国産綿 30%)",
        "dye": "弟子屈 mineral dye " + (
            "(火山灰 + 鉄 媒染 — 想定 mineral black)"
            if sku["color"] == "BLK"
            else "(undyed 生成 / kibata)"
        ),
        "print": "minimal silk screen 「━◯━」 1cm 胸下 のみ",
        "sewing": "国内 縫製 (兵庫 ヒラオカ縫製 想定) · flatlock",
        "wash": "単洗い + enzyme",
        "accessories": [
            "NFC tag (per-piece)",
            "serial code 内刺繍",
            "100年修繕券",
            "北海道 オンコ木箱 packaging",
        ],
        "size": sku["size"],
        "color": sku["color"],
        "lot_total": 30,
        "lot_split": {"BLK": 20, "NAT": 10},
        "made_to_order_eta_days": "60-90",
        "donation_split": {
            "teshikaga_town": 0.35,
            "climate_reserve": 0.10,
            "ops": 0.05,
            "_total_donation": 0.50,
        },
        "note": "Spec figures are pre-supplier-confirmation (想定). Do not quote externally without sourcing.",
    }, ensure_ascii=False)

    cur.execute(
        """INSERT INTO products
           (brand, drop_num, name, design_url, mockup_url, price_jpy,
            inventory, sold, created_at, active, prompt_text,
            serial_code, color, size, city_slug)
           VALUES (?, ?, ?, NULL, ?, ?, ?, 0, ?, 1, ?, ?, ?, ?, 'teshikaga')""",
        (
            sku["brand"],
            sku["drop_num"],
            sku["name"],
            mockup_url,
            sku["price_jpy"],
            sku["inventory"],
            now,
            prompt_text,
            sku["serial_code"],
            sku["color"],
            sku["size"],
        ),
    )
    conn.commit()
    return cur.lastrowid, "inserted"


def main() -> int:
    if not DB_PATH.exists():
        print(f"FATAL: DB not found at {DB_PATH}", file=sys.stderr)
        return 2

    force = bool(os.environ.get("FORCE"))
    api_key = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
    client = None
    if api_key and HAVE_GENAI:
        try:
            client = genai.Client(api_key=api_key)
        except Exception as exc:
            print(f"WARN: gemini client init failed: {exc}", file=sys.stderr)
            client = None
    elif not HAVE_GENAI:
        print("WARN: google-genai not installed — will use Pillow fallback for images",
              file=sys.stderr)

    conn = sqlite3.connect(str(DB_PATH))
    results: list[dict] = []
    for sku in SKUS:
        print(f"→ {sku['serial_code']} ({sku['name']})")
        # 1. mockup
        size, method = ensure_mockup(client, sku, force)
        print(f"  mockup: {size} bytes via {method}")
        # 2. DB upsert
        pid, status = upsert_sku(conn, sku)
        print(f"  db: id={pid} status={status}")
        results.append({
            "serial_code": sku["serial_code"],
            "id": pid,
            "status": status,
            "mockup_bytes": size,
            "mockup_method": method,
        })
    conn.close()

    print()
    print("==== SUMMARY ====")
    print(json.dumps(results, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
