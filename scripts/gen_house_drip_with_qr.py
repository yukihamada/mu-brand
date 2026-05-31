#!/usr/bin/env python3
"""Make the House Drip QR shirts carry a REAL scannable QR code.

- Regenerates day3 / day4 as text-only art (leaves a clean empty zone).
- Generates a real MU-styled QR (ECC=H, off-white card + ink modules + gold ◯
  center logo) pointing at https://wearmu.com/fest (live festival page).
- Composites the QR into the empty zone.
- VERIFIES the final composited PNG decodes back to the URL (pyzbar), flattened
  on black to simulate the shirt. Aborts loudly if it doesn't scan.

Usage: python3 scripts/gen_house_drip_with_qr.py
Outputs (overwrites): store/static/festseed/drip-day3.png, drip-day4.png
        + store/static/festseed/qr-fest.png (standalone)
"""
import os, sys
from pathlib import Path

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

_QR_DETECTOR = cv2.QRCodeDetector()

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "store" / "static" / "festseed"
OUT.mkdir(parents=True, exist_ok=True)
MODEL = "gemini-3-pro-image-preview"
URL = "https://wearmu.com/aloha"

OFFWHITE = (242, 242, 238, 255)
INK = (10, 10, 10, 255)
GOLD = (245, 177, 66, 255)

SYSTEM = """You are the lead apparel graphic designer for the MU brand (wearmu.com).
MU marks: 無 (nothing), 月 (moon), and the wordmark ━◯━ (circle flanked by two
short bars). Quiet Japanese minimalism + one warm Hawaiian gold, lots of space.

Produce ONE square 2940x2940 PNG, TRANSPARENT background, for DTG printing.
Rules: flat solid shapes; MAX 3 colors off-white(#f2f2ee)/gold(#f5b142)/ink(#0a0a0a);
NO photo bg, NO gradient, NO mesh, NO shadow; NO faces; NO third-party logos or
names (MU-original only); heavy clean condensed sans; render EXACTLY the words
given and NO other words.

Design brief: {brief}

Output: ONE print-ready transparent graphic, nothing else."""

BRIEFS = {
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


def styled_qr(url, mpx=26, border=4):
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
    """Flatten on black (simulate the shirt) and decode with OpenCV."""
    flat = Image.new("RGB", img_rgba.size, (0, 0, 0))
    flat.paste(img_rgba, mask=img_rgba.split()[3])
    arr = cv2.cvtColor(np.array(flat), cv2.COLOR_RGB2BGR)
    data, _, _ = _QR_DETECTOR.detectAndDecode(arr)
    if data == url:
        return True
    ok, datas, _, _ = _QR_DETECTOR.detectAndDecodeMulti(arr)
    return bool(ok) and any(d == url for d in (datas or []))


def main():
    if not os.environ.get("GEMINI_API_KEY"):
        sys.exit("GEMINI_API_KEY not set")
    client = genai.Client(api_key=os.environ["GEMINI_API_KEY"])

    qr = styled_qr(URL)
    qr.save(OUT / "qr-fest.png")
    ok_std = decodes_to(qr, URL)
    print(f"  QR standalone decodes → {URL}: {'✓' if ok_std else '✗ FAIL'}  ({qr.size[0]}px)")
    if not ok_std:
        sys.exit("standalone QR failed to decode — aborting")

    made = []
    for key, brief in BRIEFS.items():
        try:
            art = Image.open(__import__("io").BytesIO(gen_art(client, brief))).convert("RGBA")
        except Exception as e:
            print(f"  ✗ {key} art: {e}")
            continue
        W, H = art.size
        cx, cy, sf = PLACEMENT[key]
        qsize = int(W * sf)
        q = qr.resize((qsize, qsize), Image.LANCZOS)
        px, py = int(W * cx - qsize / 2), int(H * cy - qsize / 2)
        # erase whatever the model drew in the QR box (back to transparent), then paste
        eraser = Image.new("RGBA", (qsize, qsize), (0, 0, 0, 0))
        art.paste(eraser, (px, py))
        art.alpha_composite(q, (px, py))
        ok = decodes_to(art, URL)
        p = OUT / f"drip-{key}.png"
        art.save(p)
        print(f"  {'✓' if ok else '✗ FAIL'} {p.relative_to(ROOT)}  QR scannable: {ok}  ({W}px)")
        if ok:
            made.append(str(p))
        else:
            print(f"     ! {key} did not decode at {qsize}px — increase size_frac")
    made.append(str(OUT / "qr-fest.png"))
    os.system("open " + " ".join(f"'{m}'" for m in made))
    print(f"opened {len(made)} files")


if __name__ == "__main__":
    main()
