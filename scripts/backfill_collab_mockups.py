#!/usr/bin/env python3
"""Backfill /static/proposals/<slug>-mockup-<letter>.png for all collabs.

Context (commit e31b133): SSR LP image fallback chain looks at
  1. proposal_skus.img_url
  2. /static/proposals/<slug>-mockup-<letter>.png  <-- generated here
  3. /static/proposals/<slug>-pf-<letter>.jpg
  4. /static/proposals/<slug>-design-<slug>.png
  5. inline SVG placeholder

This script audits every collab in store/partner_specs/*.json, computes the
SKU letter set, and generates a Gemini 3 Pro Image mockup for any letter that
does not yet have a -mockup- PNG. Falls back to a pillow silhouette if the
Gemini call fails twice in a row, so the script always exits cleanly.

Priority ordering (per task brief, 2026-05-21):
  amami (3)  -> blank (1) -> ele (10) -> elsoul (10) -> asoview (10)
  -> kichinan (10) -> atsume (10) -> jiufight (10) -> nojimahal (10)
  ryozo and reversal are intentionally skipped (handled by separate scripts).
  rashguard is num-based, not letter-based, so excluded.

Per-collab cap = 10 letters (alphabetical), keeping total ~74 images and the
runtime ~5-6h at 5 min/image.

Usage:
    cd /Users/yuki/workspace/mu-brand
    source /Users/yuki/.env
    python3 scripts/backfill_collab_mockups.py             # all queued
    python3 scripts/backfill_collab_mockups.py amami       # one collab
    FORCE=1 python3 scripts/backfill_collab_mockups.py     # overwrite existing
"""
from __future__ import annotations

import base64
import io
import json
import os
import sys
import time
from pathlib import Path

# Force GEMINI_API_KEY from /Users/yuki/.env (zshrc copy is revoked per
# feedback_gemini_key_env memory). Drop any pre-existing GOOGLE_API_KEY so the
# SDK does not pick a stale value.
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

from google import genai  # noqa: E402
from google.genai import types  # noqa: E402
from PIL import Image, ImageDraw, ImageFont  # noqa: E402

ROOT = Path("/Users/yuki/workspace/mu-brand")
SPECS_DIR = ROOT / "store/partner_specs"
PROPOSALS_DIR = ROOT / "store/static/proposals"
GEMINI_MODEL = "gemini-3-pro-image-preview"
OUT_SIZE = 800
MIN_BYTES = 100_000        # reject suspiciously small Gemini outputs
MAX_BYTES = 500_000        # re-compress if larger than this
PNG_OPTIMIZE = True
RETRY_DELAY_S = 4

# Priority order + max letters per collab.
COLLAB_QUEUE: list[tuple[str, int]] = [
    ("amami", 10),
    ("blank", 10),
    ("ele", 10),
    ("elsoul", 10),
    ("asoview", 10),
    ("kichinan", 10),
    ("atsume", 10),
    ("jiufight", 10),
    ("nojimahal", 10),
]

# Common preamble for every Tee mockup.
PREAMBLE = (
    "Premium Tee mockup, 800x800 e-commerce flat lay, off-white backdrop, "
    "single garment, photorealistic. Design center chest: "
)

# Per-collab style guidance (appended after the SKU label).
COLLAB_STYLE: dict[str, str] = {
    "amami": (
        "Amami Oshima nature monoline silhouette (waves / forest / whale / "
        "dolphin) translated to one-line art, black on white tee or white on "
        "black tee, deep ocean teal accent (#3aa6c4), minimal."
    ),
    "blank": (
        "Pure minimal BLANK_ MA Tee, single white moon-circle mark on the chest, "
        "off-white tee, ultra-clean editorial silhouette, no extra text."
    ),
    "ele": (
        "ELE founder uniform, white text on charcoal tee, calm sans-serif "
        "wordmark, very small ELE mark on hem, soft warm key light."
    ),
    "elsoul": (
        "ELSOUL Vietnam tech motif, abstract Vietnamese pattern in white on "
        "deep indigo blue tee (#1f3460), single garment, minimal."
    ),
    "asoview": (
        "Asoview outdoor experience Japan, scenic monoline of mountain / "
        "river / hot spring printed in single ink color on natural cotton "
        "tee, calm earth tone, minimal."
    ),
    "kichinan": (
        "Kichinan Fujimimachi rear-support / mountain village motif, simple "
        "Japanese typography reading the SKU label on a heather grey tee, "
        "one-color print, minimal."
    ),
    "atsume": (
        "ATSUME HR x Tech x BJJ founder uniform, orange (#ff5a36) accent on "
        "off-white tee, sharp sans-serif typography, single chest print, "
        "premium founder aesthetic."
    ),
    "jiufight": (
        "JIU FIGHT BJJ tournament tee, athletic black cotton tee, bold "
        "Japanese kanji typography for the SKU label centered on the chest, "
        "white ink, photographic studio backdrop."
    ),
    "nojimahal": (
        "Nojima Hal signature rashguard / apparel, deep navy garment, single "
        "white logo center chest, athletic compression silhouette, studio "
        "lighting."
    ),
}


def gemini_generate(client: genai.Client, prompt: str) -> bytes | None:
    """Call Gemini 3 Pro Image. Returns PNG bytes or None on failure."""
    try:
        resp = client.models.generate_content(
            model=GEMINI_MODEL,
            contents=[prompt],
            config=types.GenerateContentConfig(
                response_modalities=["IMAGE", "TEXT"]
            ),
        )
    except Exception as exc:  # noqa: BLE001
        # Never log the key. type(exc).__name__ + message is safe.
        print(f"    gemini error: {type(exc).__name__}: {exc}")
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
        except Exception as exc:  # noqa: BLE001
            print(f"    pillow decode error: {exc}")
            return None
        if im.size != (OUT_SIZE, OUT_SIZE):
            im = im.resize((OUT_SIZE, OUT_SIZE), Image.LANCZOS)
        buf = io.BytesIO()
        im.save(buf, format="PNG", optimize=PNG_OPTIMIZE)
        png = buf.getvalue()
        # If too large, drop to adaptive palette (256 colors). For ecommerce
        # mockups this is visually identical and 4-8x smaller. Iterate until
        # we are under MAX_BYTES.
        if len(png) > MAX_BYTES:
            for ncolors in (256, 192, 128, 96, 64):
                buf2 = io.BytesIO()
                pal = im.convert(
                    "P", palette=Image.Palette.ADAPTIVE, colors=ncolors
                )
                pal.save(buf2, format="PNG", optimize=True, compress_level=9)
                cand = buf2.getvalue()
                if len(cand) < MAX_BYTES:
                    return cand
            # Final fallback: smaller resolution PNG-256.
            im_small = im.resize((640, 640), Image.LANCZOS)
            buf3 = io.BytesIO()
            pal = im_small.convert(
                "P", palette=Image.Palette.ADAPTIVE, colors=128
            )
            pal.save(buf3, format="PNG", optimize=True, compress_level=9)
            return buf3.getvalue()
        return png
    return None


def _font(size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    candidates = [
        "/System/Library/Fonts/Helvetica.ttc",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/Library/Fonts/Arial.ttf",
    ]
    for p in candidates:
        if Path(p).exists():
            try:
                return ImageFont.truetype(p, size)
            except Exception:  # noqa: BLE001
                continue
    return ImageFont.load_default()


def pillow_fallback(slug: str, letter: str, label: str) -> bytes:
    """Last-resort silhouette: a generic Tee silhouette + slug-letter text.

    Always better than an inline SVG placeholder.
    """
    card = Image.new("RGB", (OUT_SIZE, OUT_SIZE), (245, 244, 240))
    d = ImageDraw.Draw(card)
    # Simple tee silhouette (centered).
    cx = OUT_SIZE // 2
    # Body rectangle
    body = (cx - 200, 220, cx + 200, 660)
    d.rounded_rectangle(body, radius=18, fill=(40, 40, 40))
    # Sleeves
    d.polygon([(cx - 200, 220), (cx - 290, 320), (cx - 230, 380), (cx - 200, 310)],
              fill=(40, 40, 40))
    d.polygon([(cx + 200, 220), (cx + 290, 320), (cx + 230, 380), (cx + 200, 310)],
              fill=(40, 40, 40))
    # Neck
    d.ellipse((cx - 60, 200, cx + 60, 260), fill=(245, 244, 240))
    # Slug label centered on chest.
    f1 = _font(34)
    f2 = _font(20)
    text = f"{slug.upper()}-{letter.upper()}"
    tb = d.textbbox((0, 0), text, font=f1)
    tw = tb[2] - tb[0]
    d.text((cx - tw // 2, 420), text, font=f1, fill=(245, 244, 240))
    # Label clipped
    clip = (label or "")[:36]
    tb2 = d.textbbox((0, 0), clip, font=f2)
    tw2 = tb2[2] - tb2[0]
    d.text((cx - tw2 // 2, 470), clip, font=f2, fill=(200, 200, 200))
    buf = io.BytesIO()
    card.save(buf, format="PNG", optimize=True)
    return buf.getvalue()


def build_prompt(slug: str, label: str) -> str:
    style = COLLAB_STYLE.get(slug, "minimal monoline mark, single ink color")
    label_clean = (label or "").replace('"', "")
    return (
        PREAMBLE
        + f'"{label_clean}". '
        + style
        + " Single garment, no people, no extra brand logos, no watermarks, "
        "no text other than the SKU label itself rendered tastefully."
    )


def collab_letters(spec_path: Path) -> list[tuple[str, str]]:
    """Returns ordered [(letter, label), ...] for a partner spec."""
    data = json.loads(spec_path.read_text())
    out: list[tuple[str, str]] = []
    for sku in data.get("skus", []) or []:
        if not isinstance(sku, dict):
            continue
        letter = sku.get("letter") or sku.get("id") or sku.get("key")
        if not letter:
            continue
        label = (
            sku.get("label")
            or sku.get("subtitle")
            or sku.get("name")
            or sku.get("en_name")
            or ""
        )
        out.append((str(letter).lower(), str(label)))
    return out


def audit(force: bool = False) -> dict[str, dict]:
    """Returns {slug: {total, missing_letters[], existing_letters[]}}.

    When force=True, files that exist are still queued for regeneration
    (returned under both `existing` and `missing`).
    """
    result: dict[str, dict] = {}
    for spec_path in sorted(SPECS_DIR.glob("*.json")):
        slug = spec_path.stem
        letters = collab_letters(spec_path)
        if not letters:
            continue
        missing, existing = [], []
        for letter, label in letters:
            target = PROPOSALS_DIR / f"{slug}-mockup-{letter}.png"
            if target.exists():
                existing.append((letter, label))
                if force:
                    missing.append((letter, label))
            else:
                missing.append((letter, label))
        result[slug] = {
            "total": len(letters),
            "existing": existing,
            "missing": missing,
        }
    return result


def generate_one(client: genai.Client, slug: str, letter: str, label: str,
                 force: bool) -> tuple[str, int]:
    """Generate one mockup. Returns (method, bytes_written)."""
    out_path = PROPOSALS_DIR / f"{slug}-mockup-{letter}.png"
    if out_path.exists() and not force:
        return ("skip-exists", out_path.stat().st_size)

    prompt = build_prompt(slug, label)
    png: bytes | None = None
    for attempt in range(2):
        png = gemini_generate(client, prompt)
        if png and len(png) >= MIN_BYTES:
            method = "gemini"
            break
        size = len(png) if png else 0
        print(f"    retry {slug}-{letter} attempt={attempt} got={size}b")
        time.sleep(RETRY_DELAY_S)
    else:
        png = None
        method = "pillow-fallback"

    if png is None:
        png = pillow_fallback(slug, letter, label)
        method = "pillow-fallback"

    out_path.write_bytes(png)
    return (method, len(png))


def main(argv: list[str]) -> int:
    force = bool(int(os.environ.get("FORCE", "0") or "0"))
    only = {a.lower() for a in argv[1:]} if len(argv) > 1 else None

    api_key = os.environ.get("GEMINI_API_KEY")
    if not api_key:
        print("ERROR: GEMINI_API_KEY not set (source /Users/yuki/.env)",
              file=sys.stderr)
        return 2

    # AUDIT
    report = audit(force=force)
    print("=== AUDIT ===")
    for slug, info in sorted(report.items()):
        miss = [m[0] for m in info["missing"]]
        print(f"  [{slug:<12}] total={info['total']:>3} missing={len(miss):>3} "
              f"-> {','.join(miss[:25])}{'...' if len(miss)>25 else ''}")

    client = genai.Client(api_key=api_key)

    # GENERATE
    print("\n=== GENERATE ===")
    summary: list[tuple[str, str, str, int]] = []
    total_collabs = 0
    for slug, cap in COLLAB_QUEUE:
        if only and slug not in only:
            continue
        info = report.get(slug)
        if not info:
            print(f"  [{slug}] spec missing or no letters, skip")
            continue
        missing = info["missing"][:cap]
        if not missing:
            print(f"  [{slug}] all present, skip")
            continue
        total_collabs += 1
        print(f"\n--- {slug} ({len(missing)} of {info['total']}) ---")
        for letter, label in missing:
            t0 = time.time()
            method, nbytes = generate_one(client, slug, letter, label, force)
            dt = time.time() - t0
            label_short = (label or "")[:48]
            print(f"  {slug}-{letter:<3} {nbytes:>7}b {method:<18} "
                  f"{dt:>5.1f}s  {label_short}")
            summary.append((slug, letter, method, nbytes))

    # SUMMARY
    print("\n=== SUMMARY ===")
    by_collab: dict[str, list] = {}
    for slug, letter, method, nbytes in summary:
        by_collab.setdefault(slug, []).append((letter, method, nbytes))
    for slug, rows in by_collab.items():
        gem = sum(1 for _, m, _ in rows if m == "gemini")
        pil = sum(1 for _, m, _ in rows if m.startswith("pillow"))
        skip = sum(1 for _, m, _ in rows if m.startswith("skip"))
        avg = sum(n for _, _, n in rows) / max(1, len(rows))
        print(f"  {slug:<12} count={len(rows):>3} gemini={gem:>3} "
              f"pillow={pil:>3} skip={skip:>3} avg={int(avg):>7}b")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
