#!/usr/bin/env python3
"""
Generate transparent-background MUGEN design variants for drops #71-#90.

Why this script (not generate.py): generate.py emits one drop at a time,
solid bg, scheduled on cron. This is a one-shot bulk job that:
  - Targets the high-cycle / pre-CHAPTER_END range (71-90)
  - Generates N variants per drop number
  - Forces transparent background output
  - Adds "end-game" / "残り" themed directions on top of the existing
    wabi-sabi / mono no aware / data poetry directions
  - Writes to designs/mugen_NNNN_HASH.png so the existing publish path
    can pick them up

Run:
  cd /Users/yuki/workspace/mu-brand
  python scripts/gen_mugen_71_90_transparent.py            # 3 variants × 20 = 60 designs
  python scripts/gen_mugen_71_90_transparent.py --variants 1 --start 71 --end 72  # smoke test
"""

import os, sys, io, json, hashlib, random, time, argparse
from datetime import datetime
from pathlib import Path

from PIL import Image
import numpy as np

REPO = Path(__file__).resolve().parent.parent

# Reuse generate.py's env-loading + Gemini client setup
sys.path.insert(0, str(REPO))

# Load /Users/yuki/.env before importing genai (mirrors generate.py)
_env = Path("/Users/yuki/.env")
if _env.exists():
    for _ln in _env.read_text().splitlines():
        _ln = _ln.strip()
        if "=" in _ln and not _ln.startswith("#"):
            _k, _v = _ln.split("=", 1)
            if _k.strip() in ("GEMINI_API_KEY",):
                os.environ[_k.strip()] = _v.strip().strip('"').strip("'")
os.environ.pop("GOOGLE_API_KEY", None)

from google import genai
from google.genai import types

GEMINI_MODEL = "gemini-3-pro-image-preview"
DESIGNS_DIR = REPO / "designs"
DESIGNS_DIR.mkdir(exist_ok=True)

# ── Design directions tuned for the 71-90 "end-stretch" zone ─────
# Cycle 108 is CHAPTER END (always 1 piece). 71-90 is the run-up:
# scarcity rising, theme = "close to the end", "remaining count matters".
END_STRETCH_DIRECTIONS = [
    "残り {remaining} 個のチャプター終盤。残量(remaining count)を主役にした、燃え尽きていく数字のタイポグラフィ",
    "108まで残り {remaining}。砂時計の最後の砂のような、終わりが近づく緊張感を抽象記号で表現",
    "Chapter end approach: '#{cycle}/108' を巨大に配置。 数字そのものが残量メーターであるかのように",
    "Premium scarcity: '{cycle} pieces only' を漢字書道のように一筆で書いた力強い一文字",
    "Decay & continuity: 円が閉じる直前の幾何学。 中心に '残{remaining}' (残り{remaining}個)",
    "Numbered farewell: '{cycle}' という数字に '無限' (mugen) の二文字を重ね、有限の中の無限を表す",
    "Negative space study: cycle {cycle}/108 を計算式 (108 − {remaining_from_end} = {cycle}) として書く。 数学的に。",
    "End-game stamp: 印鑑のように赤の四角に '{cycle}' を白抜き。 周りに小さく日付と座標",
]

# Mix with the existing generate.py directions so we don't lose continuity
BASE_DIRECTIONS = [
    "侘び寂び wabi-sabi の感覚で。 不完全さと時間の経過",
    "物の哀れ mono no aware。 過ぎ去るものへの優しい認識",
    "一期一会 ichigo ichie。 二度とない瞬間としての #{cycle}",
    "余白 yohaku のデザイン。 大胆な空白の中に最小限の要素",
    "Bold kanji: 一文字を full-chest で。 #{cycle} の意味から選ぶ",
]

AESTHETIC_STYLES = [
    "Hand-drawn calligraphic ink — sumi-e, organic, imperfect strokes",
    "Glitch typography — datamoshed digits, RGB-shifted edges",
    "Minimal swiss grid — Helvetica-bold, ruled lines, clinical",
    "Stamp / hanko — red square seal with white-cut numbers",
    "Concert-poster brutalist — heavy black ink, slanted block letters",
    "Receipt typography — monospace, dotted lines, time-stamped feel",
]


def build_prompt(drop_num: int, cycle: int, variant_idx: int) -> str:
    now = datetime.now()
    remaining = 108 - cycle
    remaining_from_end = remaining

    end_dir = random.choice(END_STRETCH_DIRECTIONS).format(
        remaining=remaining, cycle=cycle,
        remaining_from_end=remaining_from_end,
    )
    base_dir = random.choice(BASE_DIRECTIONS).format(cycle=cycle)
    style = random.choice(AESTHETIC_STYLES)

    return f"""FLAT PRINT ARTWORK on PURE WHITE BACKGROUND (RGB 255,255,255).


OUTPUT REQUIREMENTS — read these first:
- Background: 100% pure white #FFFFFF, completely uniform, NO gradient, NO texture, NO shadows around the design
- Design elements rendered in PURE BLACK #000000 or saturated solid colors only (no greys near the bg)
- High contrast: every design pixel is at least 60 luminosity units away from white
- Crisp anti-aliased edges (a thin grey halo around marks is OK — it will be cleaned up)
- Portrait orientation, screen-print ready

This will be post-processed: white background pixels → fully transparent alpha. So:
- DO NOT put faint pencil sketches or grey fills near the background — they will disappear
- DO NOT use white as part of the design — it will become transparent

This is NOT a t-shirt photo, NOT a clothing mockup. Pure 2D graphic, like a die-cut sticker.

Brand: MUGEN (無限) — drop #{drop_num}, cycle {cycle}/108. {cycle} pieces only.
Timestamp: {now.strftime('%Y.%m.%d')} JST  ·  variant {variant_idx}

Primary direction (end-stretch zone, 71-90 range):
{end_dir}

Secondary direction (continuity with earlier drops):
{base_dir}

Aesthetic style:
{style}

Must include in the composition:
- "{now.strftime('%Y.%m.%d')}"
- "{cycle}/108"

Hard NOs:
- No t-shirt silhouette
- No clothing mockup
- No model
- No gradient backgrounds, no shadows behind the design
- No solid background of any color — TRANSPARENT only
"""


def generate_with_gemini(prompt: str) -> bytes:
    client = genai.Client(api_key=os.environ["GEMINI_API_KEY"])
    response = client.models.generate_content(
        model=GEMINI_MODEL,
        contents=[prompt],
        config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"]),
    )
    for part in response.candidates[0].content.parts:
        if hasattr(part, "inline_data") and part.inline_data:
            data = part.inline_data.data
            if isinstance(data, str):
                import base64
                return base64.b64decode(data)
            return data
    raise RuntimeError("Gemini returned no image")


def ensure_transparent(image_bytes: bytes) -> bytes:
    """Convert white-ish background (as prompted) to alpha=0.

    Strategy: sample 4 corners. If most corners are light, treat as
    white-bg → key out. If dark, key out near-black. Otherwise, fall
    back to the median corner color and key chroma-distance.

    Anti-aliased edges (where a pixel is partly white): use a smooth
    alpha ramp so the design's edge stays crisp on any shirt color.
    """
    img = Image.open(io.BytesIO(image_bytes)).convert("RGBA")
    arr = np.array(img).astype(np.int16)
    h, w = arr.shape[:2]

    # 4 corner patches, 20x20 each
    patches = np.concatenate([
        arr[0:20, 0:20, :3].reshape(-1, 3),
        arr[0:20, w-20:w, :3].reshape(-1, 3),
        arr[h-20:h, 0:20, :3].reshape(-1, 3),
        arr[h-20:h, w-20:w, :3].reshape(-1, 3),
    ])
    bg_color = np.median(patches, axis=0)
    bg_brightness = float(bg_color.mean())

    rgb = arr[..., :3]
    if bg_brightness > 180:
        # white-ish: distance-from-white
        dist = np.linalg.norm(rgb - 255, axis=-1)
    elif bg_brightness < 60:
        # black-ish: distance-from-black
        dist = np.linalg.norm(rgb, axis=-1)
    else:
        # mid-tone: distance-from-sampled-bg-median
        dist = np.linalg.norm(rgb - bg_color, axis=-1)

    # Smooth alpha ramp: dist 0..15 → fully transparent; 15..40 → linear ramp; >40 → opaque.
    # Keeps anti-aliased edges clean.
    alpha = np.clip((dist - 15) / 25.0, 0.0, 1.0) * 255
    arr[..., 3] = alpha.astype(np.int16)

    out = Image.fromarray(arr.astype(np.uint8), "RGBA")
    buf = io.BytesIO()
    out.save(buf, format="PNG")
    return buf.getvalue()


def save_design(image_bytes: bytes, drop_num: int) -> Path:
    h = hashlib.sha256(image_bytes).hexdigest()[:8]
    out_path = DESIGNS_DIR / f"mugen_{drop_num:04d}_{h}.png"
    out_path.write_bytes(image_bytes)
    return out_path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--start", type=int, default=71)
    parser.add_argument("--end", type=int, default=90, help="inclusive")
    parser.add_argument("--variants", type=int, default=3,
                        help="variants per drop number")
    parser.add_argument("--retries", type=int, default=2)
    parser.add_argument("--sleep", type=float, default=2.0,
                        help="seconds between Gemini calls")
    args = parser.parse_args()

    drops = list(range(args.start, args.end + 1))
    total = len(drops) * args.variants
    print(f"Generating {total} designs across drops {args.start}-{args.end} × {args.variants} variants")
    print(f"Output → {DESIGNS_DIR}/")
    print()

    saved = []
    failed = []

    for drop_num in drops:
        cycle = ((drop_num - 1) % 108) + 1
        for v in range(1, args.variants + 1):
            tag = f"#{drop_num} cycle={cycle} v{v}"
            for attempt in range(args.retries + 1):
                try:
                    prompt = build_prompt(drop_num, cycle, v)
                    raw = generate_with_gemini(prompt)
                    trans = ensure_transparent(raw)
                    path = save_design(trans, drop_num)
                    saved.append(path.name)
                    print(f"  ✓ {tag} → {path.name}")
                    time.sleep(args.sleep)
                    break
                except Exception as e:
                    if attempt < args.retries:
                        print(f"  · {tag} retry {attempt+1}: {e}")
                        time.sleep(5)
                    else:
                        failed.append((drop_num, v, str(e)))
                        print(f"  ✗ {tag} FAILED: {e}")

    print()
    print(f"Done. saved={len(saved)} failed={len(failed)}")
    if failed:
        print("Failures:")
        for d, v, e in failed:
            print(f"  drop {d} v{v}: {e[:120]}")


if __name__ == "__main__":
    main()
