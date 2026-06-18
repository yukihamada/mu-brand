#!/usr/bin/env python3
"""Generate PRINT-READY House Drip tees (transparent, ~3600px, DTG-ready).

Extends gen_house_drip_with_qr.py. Produces ALL FOUR days as
store/static/festseed/drip-dayN-print.png:

  Day1  무言 — wordmark ━◯━ + 無 gold ring, NO words (transparent).
  Day2  "THIS SHIRT SAYS NOTHING." + tiny "無 = nothing".
  Day3  "DAY THREE. CURIOUS YET?" + real scannable QR → wearmu.com/aloha.
  Day4  "TAKE ME HOME." + MU FESTIVAL · HAWAII (no date) + real QR → /aloha.

Design art is generated with Gemini (transparent), then upscaled to ~3600px
(≥150 DPI at a 12-in DTG front placement). The QR for day3/day4 is rendered
crisply at native target px (NOT upscaled) so it stays scannable, composited
into the reserved empty zone, and VERIFIED to decode back to the URL via
OpenCV (and pyzbar fallback). Aborts loudly on any QR scan failure.

The existing 1024px mockup PNGs (drip-dayN.png) are LEFT UNTOUCHED — they
stay as the shop-grid display image. These -print.png files are the print
masters referenced by printful_files.

Usage: python3 scripts/gen_house_drip_print.py
"""
import io
import os
import sys
from pathlib import Path

# Global GEMINI_API_KEY in zshrc is revoked — always load /Users/yuki/.env.
os.environ.pop("GOOGLE_API_KEY", None)
_env = Path("/Users/yuki/.env")
if _env.exists():
    for ln in _env.read_text().splitlines():
        ln = ln.strip()
        if "=" in ln and not ln.startswith("#"):
            k, v = ln.split("=", 1)
            if k.strip() == "GEMINI_API_KEY":
                os.environ["GEMINI_API_KEY"] = v.strip().strip('"').strip("'")

import qrcode
from qrcode.constants import ERROR_CORRECT_H
from PIL import Image, ImageDraw
import cv2
import numpy as np
from google import genai
from google.genai import types

try:
    from pyzbar.pyzbar import decode as zbar_decode
except Exception:
    zbar_decode = None

_QR_DETECTOR = cv2.QRCodeDetector()

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "store" / "static" / "festseed"
OUT.mkdir(parents=True, exist_ok=True)
MODEL = "gemini-3-pro-image-preview"
URL = "https://wearmu.com/aloha"
TARGET = 3600  # final print master px (≥150 DPI @ 12in front placement)

OFFWHITE = (242, 242, 238, 255)
INK = (10, 10, 10, 255)
GOLD = (245, 177, 66, 255)

SYSTEM = """You are the lead apparel graphic designer for the MU brand (wearmu.com).
MU marks: 無 (nothing), 月 (moon), and the wordmark ━◯━ (circle flanked by two
short bars). Quiet Japanese minimalism + one warm Hawaiian gold, lots of space.

Produce ONE square 2048x2048 PNG, TRANSPARENT background, for DTG printing.
Rules: flat solid shapes; MAX 3 colors off-white(#f2f2ee)/gold(#f5b142)/ink(#0a0a0a);
NO photo bg, NO gradient, NO mesh, NO shadow; NO faces; NO third-party logos or
names (MU-original only); heavy clean condensed sans; render EXACTLY the words
given and NO other words.

Design brief: {brief}

Output: ONE print-ready transparent graphic, nothing else."""

BRIEFS = {
 "day1": "ABSOLUTELY NO WORDS, NO TEXT, NO LETTERS of any kind. Centered, the MU "
   "wordmark ━◯━ (a circle flanked by two short horizontal bars) rendered with a "
   "warm gold (#f5b142) ring, and directly below it a single large off-white 無 "
   "(Japanese kanji for 'nothing'). That is the ENTIRE design — just the gold ━◯━ "
   "mark and the 無 glyph, lots of empty transparent space around them. No headline, "
   "no slogan, no other glyphs.",
 "day2": "Centered headline THIS SHIRT SAYS NOTHING. in heavy condensed off-white "
   "sans (all caps, the period included). Near the very bottom, a small gold line: "
   "無 = nothing. A tiny ━◯━ mark may sit just above the headline. Lots of empty "
   "transparent space. Words allowed: only 'THIS SHIRT SAYS NOTHING.' and "
   "'無 = nothing'.",
 "day3": "Place ONLY in the TOP THIRD: the headline DAY THREE. CURIOUS YET? in "
   "heavy condensed off-white sans, and just under it a tiny gold lowercase word: "
   "scan. The ENTIRE CENTER AND LOWER PORTION must be completely EMPTY and "
   "transparent — absolutely no QR, no squares, no glyphs, no shapes there (a real "
   "QR code will be added into that empty space later). Words allowed: only "
   "'DAY THREE. CURIOUS YET?' and 'scan'.",
 "day4": "Place ONLY in the TOP HALF: a big TAKE ME HOME. in off-white heavy sans, "
   "a thin gold horizontal rule beneath it, then a smaller line MU FESTIVAL · "
   "HAWAII, and a small ━◯━ mark near the very top. The ENTIRE LOWER HALF must be "
   "completely EMPTY and transparent — no QR, no glyph, no shapes (a real QR code "
   "will be placed there later). No dates. Words allowed: only 'TAKE ME HOME.' and "
   "'MU FESTIVAL · HAWAII'.",
}

# QR composite target as (center_x_frac, center_y_frac, size_frac) of the design.
PLACEMENT = {
 "day3": (0.50, 0.62, 0.46),
 "day4": (0.50, 0.74, 0.34),
}


def gen_art(client, brief):
    resp = client.models.generate_content(
        model=MODEL, contents=SYSTEM.format(brief=brief),
        config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"]),
    )
    for cand in resp.candidates or []:
        for part in (cand.content.parts if cand.content else []):
            if getattr(part, "inline_data", None) and part.inline_data.data:
                return part.inline_data.data
    raise RuntimeError("no image returned")


def styled_qr(url, mpx, border=4):
    """Render a crisp MU-styled QR at `mpx` pixels per module (native, no upscale)."""
    qr = qrcode.QRCode(error_correction=ERROR_CORRECT_H, box_size=1, border=border)
    qr.add_data(url)
    qr.make(fit=True)
    m = qr.get_matrix()
    n = len(m)
    size = n * mpx
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # off-white rounded card (includes the quiet zone) → guarantees normal polarity
    d.rounded_rectangle([0, 0, size - 1, size - 1], radius=mpx * 3, fill=OFFWHITE)
    for y, row in enumerate(m):
        for x, val in enumerate(row):
            if val:
                d.rectangle([x * mpx, y * mpx, (x + 1) * mpx - 1, (y + 1) * mpx - 1], fill=INK)
    # center ◯ logo (ECC=H tolerates ~30% occlusion; this is ~22%)
    c = size // 2
    r = int(size * 0.11)
    d.ellipse([c - r - mpx, c - r - mpx, c + r + mpx, c + r + mpx], fill=OFFWHITE)
    d.ellipse([c - r, c - r, c + r, c + r], fill=GOLD)
    ri = int(r * 0.5)
    d.ellipse([c - ri, c - ri, c + ri, c + ri], fill=OFFWHITE)
    return img


def decodes_to(img_rgba, url):
    """Flatten on black (simulate the shirt) and decode with OpenCV + pyzbar."""
    flat = Image.new("RGB", img_rgba.size, (0, 0, 0))
    flat.paste(img_rgba, mask=img_rgba.split()[3])
    # Downscale a large image for the detector (mimics a phone capture; also faster).
    if max(flat.size) > 1600:
        scale = 1600 / max(flat.size)
        flat_small = flat.resize((int(flat.size[0] * scale), int(flat.size[1] * scale)), Image.LANCZOS)
    else:
        flat_small = flat
    for cand in (flat, flat_small):
        arr = cv2.cvtColor(np.array(cand), cv2.COLOR_RGB2BGR)
        data, _, _ = _QR_DETECTOR.detectAndDecode(arr)
        if data == url:
            return True
        ok, datas, _, _ = _QR_DETECTOR.detectAndDecodeMulti(arr)
        if ok and any(d == url for d in (datas or [])):
            return True
    if zbar_decode is not None:
        for cand in (flat, flat_small):
            for res in zbar_decode(cand):
                if res.data.decode("utf-8", "ignore") == url:
                    return True
    return False


def strip_background(art, tol=42):
    """Make the (baked-in, edge-connected) background transparent.

    Gemini ignores the 'transparent background' instruction and paints a
    dark gray / checker backdrop (~rgb 30-120) instead, so .convert('RGBA')
    yields a fully-opaque image that would DTG-print a gray box. We flood-fill
    from all four edges over pixels that are close to their local background
    color, turning the connected backdrop transparent while preserving the
    interior off-white / gold / ink artwork (ink #0a0a0a is darker than the
    gray bg, so it is NOT swallowed; flood-fill only eats the connected
    backdrop, not isolated dark glyphs).
    """
    import collections
    a = np.array(art.convert("RGBA"))
    h, w = a.shape[:2]
    rgb = a[:, :, :3].astype(np.int16)

    # Seed colors = the actual edge pixels (the checker varies, so compare each
    # pixel to the running set of seen background colors via BFS, allowing tol).
    visited = np.zeros((h, w), dtype=bool)
    out_alpha = a[:, :, 3].copy()

    dq = collections.deque()
    for x in range(w):
        dq.append((0, x)); dq.append((h - 1, x))
    for y in range(h):
        dq.append((y, 0)); dq.append((y, w - 1))

    def is_bg(r, g, b):
        # Background is a neutral gray/black: low saturation, mid-low value,
        # clearly not the off-white (~242) or gold (R>>B) artwork.
        mx, mn = max(r, g, b), min(r, g, b)
        if mx - mn > 28:          # saturated → gold/colored artwork, keep
            return False
        if mx >= 200:             # off-white artwork, keep
            return False
        return True               # neutral and < 200 → backdrop (incl. dark gray + black checker)

    while dq:
        y, x = dq.popleft()
        if y < 0 or y >= h or x < 0 or x >= w or visited[y, x]:
            continue
        visited[y, x] = True
        r, g, b = int(rgb[y, x, 0]), int(rgb[y, x, 1]), int(rgb[y, x, 2])
        if not is_bg(r, g, b):
            continue
        out_alpha[y, x] = 0
        dq.append((y + 1, x)); dq.append((y - 1, x))
        dq.append((y, x + 1)); dq.append((y, x - 1))

    a[:, :, 3] = out_alpha
    return Image.fromarray(a, "RGBA")


def upscale_to(art, target):
    """Upscale a transparent RGBA design to a square `target` px (LANCZOS)."""
    if art.size == (target, target):
        return art
    return art.resize((target, target), Image.LANCZOS)


def main():
    if not os.environ.get("GEMINI_API_KEY"):
        sys.exit("GEMINI_API_KEY not set")
    client = genai.Client(api_key=os.environ["GEMINI_API_KEY"])

    made = []
    failures = []

    for key in ("day1", "day2", "day3", "day4"):
        brief = BRIEFS[key]
        try:
            raw = gen_art(client, brief)
            art = Image.open(io.BytesIO(raw)).convert("RGBA")
        except Exception as e:
            print(f"  x {key} art generation failed: {e}")
            failures.append(key)
            continue

        # Gemini bakes in a gray/checker backdrop → strip it to REAL alpha,
        # then upscale the cleaned transparent art to the print master size.
        art = strip_background(art)
        art = upscale_to(art, TARGET)
        W, H = art.size

        if key in PLACEMENT:
            cx, cy, sf = PLACEMENT[key]
            qsize = int(W * sf)
            # Render the QR crisply at module-pixel granularity near qsize, then
            # snap to whole modules so the composite stays sharp (no resampling blur).
            tmp = qrcode.QRCode(error_correction=ERROR_CORRECT_H, box_size=1, border=4)
            tmp.add_data(URL)
            tmp.make(fit=True)
            n_modules = len(tmp.get_matrix())
            mpx = max(8, qsize // n_modules)
            q = styled_qr(URL, mpx=mpx)
            qsize = q.size[0]
            px, py = int(W * cx - qsize / 2), int(H * cy - qsize / 2)
            px = max(0, min(px, W - qsize))
            py = max(0, min(py, H - qsize))
            # erase whatever the model drew in the QR box (back to transparent), then paste
            eraser = Image.new("RGBA", (qsize, qsize), (0, 0, 0, 0))
            art.paste(eraser, (px, py))
            art.alpha_composite(q, (px, py))
            ok = decodes_to(art, URL)
        else:
            ok = True  # no QR on day1/day2

        p = OUT / f"drip-{key}-print.png"
        art.save(p)
        has_alpha = art.mode == "RGBA"
        # Guard: real transparency required (a >92%-opaque master = a gray box).
        alpha_arr = np.array(art)[:, :, 3]
        transparent_pct = float((alpha_arr == 0).mean() * 100)
        if transparent_pct < 8.0:
            print(f"  x {key}: only {transparent_pct:.1f}% transparent — background not stripped (would print a box)")
            failures.append(key)
        if key in PLACEMENT:
            status = "PASS" if ok else "FAIL"
            print(f"  [{status}] {p.relative_to(ROOT)}  {art.size[0]}x{art.size[1]} mode={art.mode} transparent={transparent_pct:.1f}% QR_scannable={ok}")
            if not ok:
                failures.append(key)
        else:
            print(f"  [OK]   {p.relative_to(ROOT)}  {art.size[0]}x{art.size[1]} mode={art.mode} transparent={transparent_pct:.1f}% (no QR)")
        if ok:
            made.append(str(p))

    print(f"\nGenerated {len(made)} print files.")
    if failures:
        sys.exit(f"FAILED for: {failures} — fix before using (do not ship a non-scannable / failed file)")


if __name__ == "__main__":
    main()
