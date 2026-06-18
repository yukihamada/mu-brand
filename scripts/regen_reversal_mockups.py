#!/usr/bin/env python3
"""Generate Reversal collab mockups for letters a-j on /reversal.

Context: /reversal SKU grid has 5-stage image fallback (proposal_skus.img_url
-> /static/proposals/reversal-mockup-<letter>.png -> reversal-pf-<letter>.jpg
-> reversal-design-<slug>.png -> inline SVG placeholder), but currently 0
files exist so every cell falls through to the SVG. This script fills the
/static/proposals/reversal-mockup-{a..j}.png slot.

Reversal spec (commit 9714feb):
  a-d : 月相 baseline (new / waxing / full / waning)
  e-h : event-drop samples (Tap / KO / Decision / Submission)
  i   : Last Fight 100 Limited
  j   : Heart Rate One-Off

IMPORTANT (IP boundary): "rvddw" / "Reversal" are Reversal Co. trademarks
and we are pre-agreement. Do NOT render those logotypes as text. Use an
abstract "R-in-circle" symbol (R glyph inscribed in a thin ring) only.
MU's own ━◯━ MU mark is fine to render. Event-drop timestamps are sample
placeholders ("2026.05.24 · HH:MM · finish") for illustrative purposes.

Model: gemini-3-pro-image-preview (per ~/.claude memory: keys/image).
Key: GEMINI_API_KEY from /Users/yuki/.env.

Fallback: if Gemini fails, Pillow draws a minimalist design card (tee
silhouette + R-in-circle + moon phase) so the grid still gets a real
asset instead of the SVG placeholder.

Idempotent: existing letter files are skipped.

Usage:
    cd /Users/yuki/workspace/mu-brand
    python3 scripts/regen_reversal_mockups.py            # all 10 letters
    python3 scripts/regen_reversal_mockups.py a b c      # subset
    FORCE=1 python3 scripts/regen_reversal_mockups.py    # overwrite
"""
from __future__ import annotations
import base64
import io
import math
import os
import sys
import time
from pathlib import Path

# Force-OVERRIDE GEMINI_API_KEY from /Users/yuki/.env (per
# feedback_gemini_key_env memory: ~/.zshrc copy is revoked). Drop any
# pre-existing GOOGLE_API_KEY so the SDK doesn't grab a stale one.
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
from PIL import Image, ImageDraw, ImageFont

PROPOSALS_DIR = Path("/Users/yuki/workspace/mu-brand/store/static/proposals")
GEMINI_MODEL = "gemini-3-pro-image-preview"
OUT_SIZE = 800
PNG_MAX_BYTES = 500_000  # hard cap per spec

# Shared style preamble — fed to every prompt so all 10 SKUs look like a set.
STYLE_PREAMBLE = (
    "Photorealistic e-commerce product mockup, 1:1 square 800x800, clean "
    "minimalist studio photography in the style of a premium Printful / "
    "Everpress product listing. Single garment, no people, no extra text "
    "outside what is explicitly specified, no watermarks. The garment is "
    "shot on an invisible mannequin OR perfectly flat-laid against a soft "
    "off-white seamless backdrop with gentle natural shadow. Crisp focus, "
    "balanced exposure, true-to-fabric color. The chest design must read "
    "clearly at thumbnail size."
)

# IP boundary preamble — appended verbatim to every prompt.
IP_NOTE = (
    "CRITICAL: Do NOT render the words 'rvddw' or 'reversal' or any "
    "Reversal Co. logotype anywhere in the image. The chest mark must be "
    "an ORIGINAL abstract symbol: a capital letter R glyph (sans-serif, "
    "monoline) inscribed inside a thin perfect circle ring. No additional "
    "lettering near it. No real people, no real fighters."
)

# letter → (label, design prompt fragment, tee color description)
JOBS: list[tuple[str, str, str, str]] = [
    (
        "a", "朔 (New Moon)",
        "Center chest design: a metallic gold thin-line circle ring (about 18% of garment width) "
        "with a capital R glyph inscribed in matte gold, OVERLAPPED on its right side by a solid "
        "matte black filled circle of the same size representing a new moon — the two circles "
        "intersect by about 30%. Beneath the symbol, two tiny gold kanji characters '朔' centered. "
        "Minimal, jewelry-like restraint.",
        "a single black premium heavyweight cotton t-shirt (Comfort Colors 1717 or Gildan 5000 "
        "weight), 100% cotton, slight visible cotton texture, on an invisible mannequin against a "
        "soft off-white backdrop",
    ),
    (
        "b", "上弦 (Waxing Half Moon)",
        "Center chest design: a metallic gold thin-line circle ring (about 18% of garment width) "
        "with a capital R glyph inscribed in matte gold, OVERLAPPED on its right side by a half "
        "moon shape (right half filled bright off-white, left half empty/transparent) of the same "
        "diameter — the two circles intersect by about 30%. Beneath the symbol, two tiny gold "
        "kanji characters '上弦' centered. Minimal, jewelry-like restraint.",
        "a single black premium heavyweight cotton t-shirt, on an invisible mannequin against a "
        "soft off-white backdrop",
    ),
    (
        "c", "望 (Full Moon)",
        "Center chest design: a black thin-line circle ring (about 18% of garment width) with a "
        "capital R glyph inscribed in solid black, OVERLAPPED on its right side by a solid bright "
        "off-white filled circle (full moon) with a very faint cratered surface texture, same "
        "diameter — the two circles intersect by about 30%. Beneath the symbol, one tiny black "
        "kanji '望' centered. Minimal, premium feel.",
        "a single natural / unbleached cream-colored heavyweight cotton t-shirt (Comfort Colors "
        "Ivory style), visible cotton slub texture, on an invisible mannequin against a soft "
        "off-white backdrop",
    ),
    (
        "d", "下弦 (Waning Half Moon)",
        "Center chest design: a metallic gold thin-line circle ring (about 18% of garment width) "
        "with a capital R glyph inscribed in matte gold, OVERLAPPED on its right side by a half "
        "moon shape (LEFT half filled bright off-white, right half empty/transparent) of the same "
        "diameter — the two circles intersect by about 30%. Beneath the symbol, two tiny gold "
        "kanji characters '下弦' centered. Minimal, jewelry-like restraint.",
        "a single dark heather charcoal grey premium heavyweight cotton t-shirt with visible "
        "heather flecks, on an invisible mannequin against a soft off-white backdrop",
    ),
    (
        "e", "Event Drop · Tap",
        "Center chest design, top to bottom in a tight vertical stack: "
        "(1) a horizontal sans-serif typographic strip in matte off-white reading exactly "
        "'2026.05.24 · 22:13 · Tap' — letterspaced, machine-stamp aesthetic, all on one line; "
        "(2) a 6-pixel gap; "
        "(3) a thin-line off-white circle ring (~14% garment width) with a capital R inscribed in "
        "matte off-white; "
        "(4) below that, a small MU monogram: a short horizontal bar, a thin circle ring, another "
        "short horizontal bar, then the letters 'MU' — rendered like '━◯━ MU' in matte off-white. "
        "Looks like a fight-night memorial drop.",
        "a single black premium heavyweight cotton t-shirt, on an invisible mannequin against a "
        "soft off-white backdrop",
    ),
    (
        "f", "Event Drop · KO",
        "Center chest design, top to bottom in a tight vertical stack: "
        "(1) a horizontal sans-serif typographic strip in matte off-white reading exactly "
        "'2026.05.24 · 22:08 · KO' — letterspaced, machine-stamp aesthetic; "
        "(2) a thin-line off-white circle ring with a capital R inscribed in matte off-white; "
        "(3) below it, the MU monogram '━◯━ MU' in matte off-white. "
        "Same memorial-drop aesthetic as the rest of the Event Drop series.",
        "a single black premium heavyweight cotton t-shirt, on an invisible mannequin against a "
        "soft off-white backdrop",
    ),
    (
        "g", "Event Drop · Decision",
        "Center chest design, top to bottom in a tight vertical stack: "
        "(1) a horizontal sans-serif typographic strip in matte off-white reading exactly "
        "'2026.05.24 · 30:00 · Dec' — letterspaced, machine-stamp aesthetic; "
        "(2) a thin-line off-white circle ring with a capital R inscribed in matte off-white; "
        "(3) below it, the MU monogram '━◯━ MU' in matte off-white. "
        "Same memorial-drop aesthetic as the rest of the Event Drop series.",
        "a single black premium heavyweight cotton t-shirt, on an invisible mannequin against a "
        "soft off-white backdrop",
    ),
    (
        "h", "Event Drop · Submission",
        "Center chest design, top to bottom in a tight vertical stack: "
        "(1) a horizontal sans-serif typographic strip in matte off-white reading exactly "
        "'2026.05.24 · 21:13 · Sub' — letterspaced, machine-stamp aesthetic; "
        "(2) a thin-line off-white circle ring with a capital R inscribed in matte off-white; "
        "(3) below it, the MU monogram '━◯━ MU' in matte off-white. "
        "Same memorial-drop aesthetic as the rest of the Event Drop series.",
        "a single black premium heavyweight cotton t-shirt, on an invisible mannequin against a "
        "soft off-white backdrop",
    ),
    (
        "i", "Last Fight 100 Limited",
        "Center chest design, top to bottom: "
        "(1) the words 'THE LAST FIGHT' in elegant condensed serif typography, matte black, "
        "letterspaced wide, centered on one line; "
        "(2) below it, a tiny moon-phase icon row — five small circles ranging from new moon "
        "(black filled) to full moon (open ring) — about 1cm tall total; "
        "(3) below the moons, a small machine-stamped serial number '№ 042 / 100' in matte black. "
        "Premium minimalist museum-grade restraint. No other graphics.",
        "a single natural / unbleached premium ivory heavyweight cotton t-shirt with visible "
        "slub texture and softly rolled hems, hung against a soft off-white backdrop",
    ),
    (
        "j", "Heart Rate One-Off",
        "Center chest design, top to bottom: "
        "(1) a single thin matte black line-art ECG / heart-rate waveform spanning roughly 60% of "
        "the chest width — mostly flat baseline with three sharp peaks (QRS-like) clustered toward "
        "the right side, suggesting an in-fight heart rate trace; "
        "(2) immediately below the right end of the waveform, a tiny matte black typographic tag "
        "reading exactly 'PEAK 187 bpm'; "
        "(3) below that, a smaller line reading exactly '2026.05.24'. "
        "No other graphics, no logos. Looks like a one-of-one biometric artifact.",
        "a single bright white premium heavyweight cotton t-shirt (Comfort Colors White or Gildan "
        "5000 weight), on an invisible mannequin against a soft off-white backdrop",
    ),
]


def build_prompt(label: str, design: str, garment: str) -> str:
    return (
        f"{STYLE_PREAMBLE}\n\n"
        f"Product: {garment}.\n\n"
        f"Design (printed on the front chest area, centered, occupying roughly 22% of garment "
        f"width unless otherwise noted): {design}\n\n"
        f"{IP_NOTE}\n\n"
        f"Render this as the hero product photo for a SKU named '{label}'."
    )


def gemini_render(client: genai.Client, prompt: str) -> bytes | None:
    """Text-only prompt → 800x800 PNG bytes (or None on failure)."""
    try:
        resp = client.models.generate_content(
            model=GEMINI_MODEL,
            contents=[prompt],
            config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"]),
        )
    except Exception as exc:  # noqa: BLE001
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
        except Exception as exc:  # noqa: BLE001
            print(f"    pillow decode error: {exc}")
            return None
        if im.size != (OUT_SIZE, OUT_SIZE):
            im = im.resize((OUT_SIZE, OUT_SIZE), Image.LANCZOS)
        # Save as PNG with progressive quality reduction if oversize.
        for quality in (None,):  # PNG is lossless; we'll fall back to JPEG-in-PNG-suffix never.
            buf = io.BytesIO()
            im.save(buf, format="PNG", optimize=True)
            data_out = buf.getvalue()
            if len(data_out) <= PNG_MAX_BYTES:
                return data_out
        # Oversize — re-encode at lower resolution.
        im2 = im.resize((640, 640), Image.LANCZOS)
        buf = io.BytesIO()
        im2.save(buf, format="PNG", optimize=True)
        return buf.getvalue()
    print("    gemini returned text-only (no image part)")
    return None


# ---------- Pillow fallback ----------

def _draw_r_in_circle(draw: ImageDraw.ImageDraw, cx: int, cy: int, radius: int,
                      color: tuple) -> None:
    """Thin-line ring with a capital R inscribed."""
    draw.ellipse(
        [cx - radius, cy - radius, cx + radius, cy + radius],
        outline=color, width=max(2, radius // 18),
    )
    # R glyph via font.
    try:
        font = ImageFont.truetype(
            "/System/Library/Fonts/Helvetica.ttc", int(radius * 1.3),
        )
    except Exception:
        font = ImageFont.load_default()
    text = "R"
    bbox = draw.textbbox((0, 0), text, font=font)
    tw = bbox[2] - bbox[0]
    th = bbox[3] - bbox[1]
    draw.text((cx - tw / 2 - bbox[0], cy - th / 2 - bbox[1]),
              text, fill=color, font=font)


def _draw_moon(draw: ImageDraw.ImageDraw, cx: int, cy: int, radius: int,
               phase: str, light_color: tuple, dark_color: tuple) -> None:
    """phase = new | waxing | full | waning"""
    box = [cx - radius, cy - radius, cx + radius, cy + radius]
    if phase == "new":
        draw.ellipse(box, fill=dark_color, outline=dark_color)
    elif phase == "full":
        draw.ellipse(box, fill=light_color, outline=light_color)
    elif phase == "waxing":
        draw.ellipse(box, fill=dark_color)
        # Right half white.
        draw.pieslice(box, -90, 90, fill=light_color)
    elif phase == "waning":
        draw.ellipse(box, fill=dark_color)
        # Left half white.
        draw.pieslice(box, 90, 270, fill=light_color)


def pillow_fallback(letter: str, label: str, garment_color: tuple,
                    accent_color: tuple, phase: str | None,
                    kanji: str, event_stamp: str | None,
                    serial: str | None, heartrate: str | None) -> bytes:
    """Minimal but legible mockup: tee silhouette + chest mark."""
    bg = (245, 244, 240)
    img = Image.new("RGB", (OUT_SIZE, OUT_SIZE), bg)
    draw = ImageDraw.Draw(img)

    # Tee silhouette — simple polygon centered.
    cx, cy = OUT_SIZE // 2, OUT_SIZE // 2 + 30
    tee_w = 460
    tee_h = 540
    left = cx - tee_w // 2
    top = cy - tee_h // 2
    # Body rectangle (rounded).
    draw.rounded_rectangle(
        [left, top + 80, left + tee_w, top + tee_h],
        radius=28, fill=garment_color, outline=(0, 0, 0, 0),
    )
    # Sleeves (triangles).
    draw.polygon([
        (left, top + 80), (left - 90, top + 200), (left + 30, top + 240),
    ], fill=garment_color)
    draw.polygon([
        (left + tee_w, top + 80), (left + tee_w + 90, top + 200),
        (left + tee_w - 30, top + 240),
    ], fill=garment_color)
    # Neckline (curve).
    draw.chord(
        [cx - 70, top + 50, cx + 70, top + 150],
        start=20, end=160, fill=bg,
    )

    # Chest mark area — center it about 1/3 down the body.
    mark_cy = top + 280
    radius = 56

    if phase is not None:
        # Two overlapping circles: R-ring on left, moon on right, ~30% overlap.
        overlap = int(radius * 0.6)
        rcx = cx - overlap
        mcx = cx + overlap - int(radius * 0.6)
        _draw_r_in_circle(draw, rcx, mark_cy, radius, accent_color)
        _draw_moon(draw, mcx, mark_cy, radius, phase, (245, 244, 240), (10, 10, 10))
        if kanji:
            try:
                font = ImageFont.truetype(
                    "/System/Library/Fonts/ヒラギノ角ゴシック W6.ttc", 22,
                )
            except Exception:
                try:
                    font = ImageFont.truetype(
                        "/System/Library/Fonts/Hiragino Sans GB.ttc", 22,
                    )
                except Exception:
                    font = ImageFont.load_default()
            bbox = draw.textbbox((0, 0), kanji, font=font)
            tw = bbox[2] - bbox[0]
            draw.text((cx - tw / 2 - bbox[0], mark_cy + radius + 16),
                      kanji, fill=accent_color, font=font)
    else:
        # Stack layout for event/special drops.
        try:
            font_stamp = ImageFont.truetype(
                "/System/Library/Fonts/Menlo.ttc", 18,
            )
            font_small = ImageFont.truetype(
                "/System/Library/Fonts/Menlo.ttc", 14,
            )
            font_title = ImageFont.truetype(
                "/System/Library/Fonts/Times.ttc", 28,
            )
        except Exception:
            font_stamp = font_small = font_title = ImageFont.load_default()

        y = mark_cy - 60
        if event_stamp:
            bbox = draw.textbbox((0, 0), event_stamp, font=font_stamp)
            tw = bbox[2] - bbox[0]
            draw.text((cx - tw / 2 - bbox[0], y), event_stamp,
                      fill=accent_color, font=font_stamp)
            y += 32
            _draw_r_in_circle(draw, cx, y + 40, 38, accent_color)
            y += 90
            # MU monogram strip: ━◯━ MU
            strip_y = y
            draw.line([(cx - 70, strip_y), (cx - 30, strip_y)],
                      fill=accent_color, width=3)
            draw.ellipse([cx - 22, strip_y - 12, cx + 2, strip_y + 12],
                         outline=accent_color, width=3)
            draw.line([(cx + 10, strip_y), (cx + 50, strip_y)],
                      fill=accent_color, width=3)
            draw.text((cx + 58, strip_y - 14), "MU",
                      fill=accent_color, font=font_stamp)
        elif serial:
            # Last Fight Limited
            title = "THE LAST FIGHT"
            bbox = draw.textbbox((0, 0), title, font=font_title)
            tw = bbox[2] - bbox[0]
            draw.text((cx - tw / 2 - bbox[0], y), title,
                      fill=accent_color, font=font_title)
            # Moon-phase row (5 small circles).
            mrow_y = y + 50
            for i, ph in enumerate(["new", "waxing", "full", "waning", "new"]):
                mx = cx - 60 + i * 30
                _draw_moon(draw, mx, mrow_y, 9, ph,
                           (245, 244, 240), accent_color)
            # Serial.
            bbox = draw.textbbox((0, 0), serial, font=font_small)
            tw = bbox[2] - bbox[0]
            draw.text((cx - tw / 2 - bbox[0], mrow_y + 24), serial,
                      fill=accent_color, font=font_small)
        elif heartrate:
            # ECG waveform.
            base_y = y + 30
            xs = list(range(cx - 140, cx + 140, 4))
            pts = []
            for i, x in enumerate(xs):
                t = (x - (cx - 140)) / 280
                # mostly flat with three QRS spikes near the right.
                spike = 0.0
                for s_center, s_amp in [(0.55, 32), (0.68, 26), (0.82, 38)]:
                    spike += s_amp * math.exp(-((t - s_center) ** 2) / 0.0006)
                pts.append((x, base_y - spike + 4 * math.sin(t * 8 * math.pi) * (0.4 if t < 0.4 else 0.1)))
            draw.line(pts, fill=accent_color, width=2)
            # PEAK tag.
            draw.text((cx + 60, base_y + 10), "PEAK 187 bpm",
                      fill=accent_color, font=font_small)
            draw.text((cx - 50, base_y + 32), "2026.05.24",
                      fill=accent_color, font=font_small)

    # Subtle label tag at bottom (for human QA, not on shirt).
    try:
        font_meta = ImageFont.truetype(
            "/System/Library/Fonts/Helvetica.ttc", 12,
        )
    except Exception:
        font_meta = ImageFont.load_default()
    tag = f"reversal · {label}"
    bbox = draw.textbbox((0, 0), tag, font=font_meta)
    tw = bbox[2] - bbox[0]
    draw.text((OUT_SIZE - tw - 16 - bbox[0], OUT_SIZE - 24), tag,
              fill=(160, 156, 148), font=font_meta)

    buf = io.BytesIO()
    img.save(buf, format="PNG", optimize=True)
    return buf.getvalue()


# Per-letter pillow-fallback config (drives _draw_moon / event_stamp / etc).
FALLBACK_CFG: dict[str, dict] = {
    "a": dict(garment=(20, 20, 20), accent=(196, 162, 80),
              phase="new", kanji="朔"),
    "b": dict(garment=(20, 20, 20), accent=(196, 162, 80),
              phase="waxing", kanji="上弦"),
    "c": dict(garment=(232, 224, 204), accent=(20, 20, 20),
              phase="full", kanji="望"),
    "d": dict(garment=(70, 70, 76), accent=(196, 162, 80),
              phase="waning", kanji="下弦"),
    "e": dict(garment=(20, 20, 20), accent=(232, 232, 224),
              event_stamp="2026.05.24 · 22:13 · Tap"),
    "f": dict(garment=(20, 20, 20), accent=(232, 232, 224),
              event_stamp="2026.05.24 · 22:08 · KO"),
    "g": dict(garment=(20, 20, 20), accent=(232, 232, 224),
              event_stamp="2026.05.24 · 30:00 · Dec"),
    "h": dict(garment=(20, 20, 20), accent=(232, 232, 224),
              event_stamp="2026.05.24 · 21:13 · Sub"),
    "i": dict(garment=(232, 224, 204), accent=(30, 30, 30),
              serial="№ 042 / 100"),
    "j": dict(garment=(248, 248, 248), accent=(20, 20, 20),
              heartrate="PEAK 187 bpm"),
}


def render_letter(client: genai.Client, letter: str, label: str,
                  design_prompt: str, garment: str,
                  force: bool) -> tuple[int, str]:
    """Returns (bytes_written, method). 0 bytes = skipped."""
    out_path = PROPOSALS_DIR / f"reversal-mockup-{letter}.png"
    if out_path.exists() and not force:
        return out_path.stat().st_size, "skip-exists"

    prompt = build_prompt(label, design_prompt, garment)
    method = "gemini"
    png_bytes: bytes | None = None
    for attempt in range(2):
        png_bytes = gemini_render(client, prompt)
        if png_bytes and len(png_bytes) >= 40_000:
            break
        size = len(png_bytes) if png_bytes else 0
        print(f"    attempt {attempt + 1}: {size} bytes — {'retry' if attempt == 0 else 'fallback'}")
        if attempt == 0:
            time.sleep(4)
    if not png_bytes:
        method = "pillow-fallback"
        cfg = FALLBACK_CFG[letter]
        png_bytes = pillow_fallback(
            letter, label,
            garment_color=cfg.get("garment", (20, 20, 20)),
            accent_color=cfg.get("accent", (232, 232, 224)),
            phase=cfg.get("phase"),
            kanji=cfg.get("kanji", ""),
            event_stamp=cfg.get("event_stamp"),
            serial=cfg.get("serial"),
            heartrate=cfg.get("heartrate"),
        )

    out_path.write_bytes(png_bytes)
    return len(png_bytes), method


def main(argv: list[str]) -> int:
    selected = {a.lower() for a in argv[1:]} if len(argv) > 1 else None
    force = bool(os.environ.get("FORCE"))
    api_key = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
    if not api_key:
        print("WARN: GEMINI_API_KEY not set — using pillow fallback for ALL letters",
              file=sys.stderr)
        client = None
    else:
        client = genai.Client(api_key=api_key)

    PROPOSALS_DIR.mkdir(parents=True, exist_ok=True)
    results: list[tuple[str, str, int, str]] = []

    for letter, label, design_prompt, garment in JOBS:
        if selected and letter not in selected:
            continue
        print(f"[{letter}] {label}")
        try:
            if client is None:
                cfg = FALLBACK_CFG[letter]
                png_bytes = pillow_fallback(
                    letter, label,
                    garment_color=cfg.get("garment", (20, 20, 20)),
                    accent_color=cfg.get("accent", (232, 232, 224)),
                    phase=cfg.get("phase"),
                    kanji=cfg.get("kanji", ""),
                    event_stamp=cfg.get("event_stamp"),
                    serial=cfg.get("serial"),
                    heartrate=cfg.get("heartrate"),
                )
                out_path = PROPOSALS_DIR / f"reversal-mockup-{letter}.png"
                if out_path.exists() and not force:
                    print(f"  skip-exists ({out_path.stat().st_size} bytes)")
                    results.append((letter, label, out_path.stat().st_size, "skip-exists"))
                    continue
                out_path.write_bytes(png_bytes)
                size, method = len(png_bytes), "pillow-fallback"
            else:
                size, method = render_letter(
                    client, letter, label, design_prompt, garment, force,
                )
        except Exception as exc:  # noqa: BLE001
            print(f"  FAIL: {type(exc).__name__}: {exc}")
            results.append((letter, label, 0, f"fail:{type(exc).__name__}"))
            continue
        print(f"  {size:>7} bytes  [{method}]")
        results.append((letter, label, size, method))

    print("\n=== summary ===")
    for letter, label, size, method in results:
        print(f"  {letter}  {size:>7}  {method:<18}  {label}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
