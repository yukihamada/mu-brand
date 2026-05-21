#!/usr/bin/env python3
"""Regenerate RYOZO TOP TEAM mockups for letters T-AD on /ryozo.

Context: letters A-S shipped with proper RYOZO design overlays (76-108KB
photorealistic Printful-style mockups), but T-AD shipped with raw blank
Printful templates (21-77KB, no design). This script uses Gemini 3 Pro
Image to composite the appropriate RYOZO design PNG onto each product
template and save back to ryozo-pf-{letter}.jpg at 800x800.

Strategy: Gemini 3 Pro Image (gemini-3-pro-image-preview) takes
  - the existing design PNG (ryozo-design-<slug>.png) as a reference for
    look-and-feel (gold wordmark + green-T accent),
  - a precise product/placement prompt,
and emits an on-product photorealistic mockup matching the A-S style.

Fallback: if Gemini fails for a letter, falls back to a pillow composite
of the design PNG over a neutral product card (so the row never ships
fully blank again).

Idempotent: backs up the existing jpg to ryozo-pf-<letter>_old.jpg before
overwriting. Existing _old.jpg is preserved (won't double-overwrite the
original backup on re-run).

Usage:
    cd /Users/yuki/workspace/mu-brand
    source /Users/yuki/.env  # GEMINI_API_KEY
    python3 scripts/regen_ryozo_mockups.py             # all 11 letters
    python3 scripts/regen_ryozo_mockups.py t u v       # subset
"""
from __future__ import annotations
import base64
import io
import os
import sys
import time
from pathlib import Path

# Force-OVERRIDE GEMINI_API_KEY from /Users/yuki/.env (per
# feedback_gemini_key_env memory: ~/.zshrc copy is revoked). Also drop any
# pre-existing GOOGLE_API_KEY so the SDK doesn't accidentally pick that
# (commonly a different/expired key).
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

from google import genai
from google.genai import types
from PIL import Image

PROPOSALS_DIR = Path("/Users/yuki/workspace/mu-brand/store/static/proposals")
GEMINI_MODEL = "gemini-3-pro-image-preview"
OUT_SIZE = 800
JPG_QUALITY = 88

# letter → (product label, product prompt, design_slug)
# design_slug matches main.rs RYOZO SKU table (lines ~58104-58114)
JOBS: list[tuple[str, str, str, str]] = [
    ("t",  "Team Sticker Pack",          "a tight flat-lay of five die-cut vinyl team stickers arranged in a fan on a clean light grey concrete surface, soft natural daylight, each sticker shows the design printed full-bleed with crisp white kiss-cut border, slight depth/shadow", "patch"),
    ("u",  "Team Joggers",               "a single pair of premium athletic joggers (Jerzees 975MPR style, heather grey) hanging cleanly against a soft off-white studio backdrop, design printed on upper left thigh at hip level, gold tones reading clearly, ecommerce product photo", "athletic"),
    ("v",  "Team Zip Hoodie",            "a single black full-zip hoodie (Gildan 18600 style) on a clean invisible mannequin against pure white background, zipper fully closed, design centered on the chest panel filling roughly 30 percent width, crisp Printful-grade studio lighting", "varsity"),
    ("w",  "Coach Quarter-Zip Pullover", "a single dark heather quarter-zip pullover (Lane Seven LS21B style) on an invisible mannequin against pure white background, quarter zipper closed, design embroidered on left chest pec area roughly 8cm wide, premium athletic product photography", "stacked"),
    ("x",  "Stainless Water Bottle",     "a 17oz stainless steel sport water bottle, brushed silver finish with white screw cap, standing upright against a soft off-white seamless backdrop, design wrapped around the front center of the bottle filling roughly 60 percent height, slight reflection at base, ecommerce photo", "monogram"),
    ("y",  "Recovery Beach Towel",       "a large white beach towel folded into a soft square stack on a clean wooden bench, design printed full-color across the visible top fold, very slight texture of cotton terry, warm natural light, lifestyle product photo", "script"),
    ("z",  "Embroidered Gi Patch",       "a single rectangular embroidered patch (5cm by 5cm) with merrow border, photographed flat at slight 15 degree angle on a soft black cotton gi sleeve, very crisp embroidery thread texture clearly visible, gold metallic thread for the wordmark and bright green thread for the T accent, studio macro photo", "patch"),
    ("aa", "Post-training Apron",        "a single natural canvas apron with adjustable neck strap, hanging on a simple hook against a soft off-white wall, design screen-printed on the chest pocket area, slight fabric wrinkle realism, lifestyle product photo", "monogram"),
    ("ab", "Summer Training Tank Top",   "a single black athletic tank top (Bella+Canvas 3480 style) on an invisible mannequin against pure white background, design centered on the chest filling roughly 40 percent width, crisp Printful-grade studio lighting", "jersey"),
    ("ac", "Crossbody Sling Bag",        "a single black nylon crossbody sling bag with adjustable strap, photographed flat against a clean off-white surface, design printed on the front main panel filling roughly 50 percent of that panel, premium product photo", "monogram"),
    ("ad", "Team Mug",                   "a single white 11oz ceramic coffee mug with C-shape handle on the right, standing on a clean light grey surface, design wrapped around the front of the mug filling roughly 60 percent height, soft daylight, ecommerce product photo", "varsity"),
]

PROMPT_TEMPLATE = (
    "Produce a single photorealistic ecommerce product mockup of {label}. "
    "{product_prompt}. "
    "The print/embroidery design itself must be EXACTLY the artwork shown in "
    "the reference image (gold metallic wordmark with one bright green accent "
    "letter / element, on a transparent background). Reproduce that design "
    "faithfully — same gold-to-darker-gold gradient, same green accent, same "
    "lockup. Render at 1:1 square aspect, 800x800, clean white or neutral "
    "background, no people unless the product description says so, no extra "
    "text, no watermarks, no logos other than the RYOZO TOP TEAM design "
    "itself. Style-match a clean Printful/Etsy product listing photo."
)


def gemini_compose(client: genai.Client, design_png: Path, prompt: str) -> bytes | None:
    """Send (prompt + design PNG) to Gemini 3 Pro Image. Return JPG bytes or None."""
    design_bytes = design_png.read_bytes()
    try:
        resp = client.models.generate_content(
            model=GEMINI_MODEL,
            contents=[
                prompt,
                types.Part.from_bytes(data=design_bytes, mime_type="image/png"),
            ],
            config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"]),
        )
    except Exception as exc:  # noqa: BLE001
        print(f"    gemini error: {type(exc).__name__}: {exc}")
        return None
    for part in resp.candidates[0].content.parts:
        inline = getattr(part, "inline_data", None)
        if not inline:
            continue
        data = inline.data
        if isinstance(data, str):
            data = base64.b64decode(data)
        # Normalize to 800x800 JPG quality=88.
        try:
            im = Image.open(io.BytesIO(data)).convert("RGB")
        except Exception as exc:  # noqa: BLE001
            print(f"    pillow decode error: {exc}")
            return None
        if im.size != (OUT_SIZE, OUT_SIZE):
            im = im.resize((OUT_SIZE, OUT_SIZE), Image.LANCZOS)
        buf = io.BytesIO()
        im.save(buf, format="JPEG", quality=JPG_QUALITY, optimize=True)
        return buf.getvalue()
    return None


def pillow_fallback(design_png: Path, label: str) -> bytes:
    """Last-resort composite: design centered on neutral card.

    Better than shipping a blank Printful template — at least the RYOZO
    design is visible.
    """
    card = Image.new("RGB", (OUT_SIZE, OUT_SIZE), (245, 244, 240))
    design = Image.open(design_png).convert("RGBA")
    # Fit design to ~70 percent of card with margin.
    target_w = int(OUT_SIZE * 0.70)
    ratio = target_w / design.width
    new_size = (target_w, int(design.height * ratio))
    design = design.resize(new_size, Image.LANCZOS)
    px = (OUT_SIZE - design.width) // 2
    py = (OUT_SIZE - design.height) // 2
    card.paste(design, (px, py), design)
    buf = io.BytesIO()
    card.save(buf, format="JPEG", quality=JPG_QUALITY, optimize=True)
    return buf.getvalue()


def regen_letter(client: genai.Client, letter: str, label: str,
                 product_prompt: str, design_slug: str) -> tuple[int, int, str]:
    """Returns (old_size, new_size, method)."""
    out_path = PROPOSALS_DIR / f"ryozo-pf-{letter}.jpg"
    backup_path = PROPOSALS_DIR / f"ryozo-pf-{letter}_old.jpg"
    design_png = PROPOSALS_DIR / f"ryozo-design-{design_slug}.png"
    if not design_png.exists():
        raise FileNotFoundError(f"design PNG missing: {design_png}")

    old_size = out_path.stat().st_size if out_path.exists() else 0
    if out_path.exists() and not backup_path.exists():
        backup_path.write_bytes(out_path.read_bytes())

    prompt = PROMPT_TEMPLATE.format(label=label, product_prompt=product_prompt)
    method = "gemini"
    jpg_bytes: bytes | None = None
    for attempt in range(2):
        jpg_bytes = gemini_compose(client, design_png, prompt)
        if jpg_bytes and len(jpg_bytes) >= 30_000:  # avoid suspiciously small
            break
        if attempt == 0:
            print(f"    retry {letter} (got {len(jpg_bytes) if jpg_bytes else 0} bytes)")
            time.sleep(4)
    if not jpg_bytes:
        method = "pillow-fallback"
        jpg_bytes = pillow_fallback(design_png, label)

    out_path.write_bytes(jpg_bytes)
    return old_size, len(jpg_bytes), method


def main(argv: list[str]) -> int:
    selected = {a.lower() for a in argv[1:]} if len(argv) > 1 else None
    api_key = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
    if not api_key:
        print("ERROR: GEMINI_API_KEY not set (source /Users/yuki/.env)", file=sys.stderr)
        return 2
    client = genai.Client(api_key=api_key)

    results: list[tuple[str, str, int, int, str]] = []
    for letter, label, prompt, slug in JOBS:
        if selected and letter not in selected:
            continue
        print(f"[{letter}] {label}  (slug={slug})")
        try:
            old, new, method = regen_letter(client, letter, label, prompt, slug)
        except Exception as exc:  # noqa: BLE001
            print(f"  FAIL: {type(exc).__name__}: {exc}")
            results.append((letter, label, 0, 0, f"fail:{type(exc).__name__}"))
            continue
        print(f"  {old:>7} → {new:>7} bytes  [{method}]")
        results.append((letter, label, old, new, method))

    print("\n=== summary ===")
    for letter, label, old, new, method in results:
        delta = f"{old} → {new}"
        print(f"  {letter:<3}  {delta:<22}  {method:<18}  {label}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
