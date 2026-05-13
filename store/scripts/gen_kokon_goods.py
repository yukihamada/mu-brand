#!/usr/bin/env python3
"""Generate mockup images for the MU × 焼肉古今 goods line proposal.
Saves under store/static/itto/goods/<name>.jpg."""
import base64, io, os, sys
from pathlib import Path

from google import genai
from google.genai import types
from PIL import Image

API_KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not API_KEY:
    sys.exit("GEMINI_API_KEY required")

MODEL = "gemini-3-pro-image-preview"
OUT_DIR = Path(__file__).resolve().parent.parent / "static" / "itto" / "goods"
OUT_DIR.mkdir(parents=True, exist_ok=True)


PROMPTS = {
    # 1. Daily wear T-shirt: kokon × MU
    "tshirt_daily": (
        "Product photograph. Black Bella+Canvas 3001 unisex T-shirt laid flat "
        "on a dark walnut surface. Centered chest: a small thin gold-foil "
        "kanji 「古今」 (kokon) above a thin horizontal gold line above a "
        "monospaced text 'wearmu.com × kokon.tokyo'. Restrained, monastic, "
        "high-end DTC aesthetic (Aesop / Our Legacy / Acne Studios). Soft "
        "north-window light from upper left. Shot on Hasselblad H6D, 80mm, "
        "f/4. 16:9, 1920x1080."
    ),
    # 2. Apron — chef-style indigo
    "apron": (
        "Product photograph. Heavy indigo canvas yakiniku apron with brass "
        "rivets, hanging on a black hook against a dark concrete wall. Chest "
        "panel embroidered in faded white thread with the kanji 「焼肉古今」 "
        "vertically. Bottom hem subtle gold thread line. Slight texture, "
        "real use suggested. Mood: monastic, professional, restrained. "
        "Shot on Leica Q3, 28mm, f/4. 16:9, 1920x1080."
    ),
    # 3. Tenugui — hand towel
    "tenugui": (
        "Product photograph. Traditional Japanese cotton tenugui hand towel "
        "in deep indigo, partially unfolded on a black ceramic plate. The "
        "tenugui has a vertical sumi-brush calligraphy 「古今」 in faded "
        "white, with a tiny gold MU mark at the corner. Charcoal-grey "
        "wooden tray underneath. Soft single overhead light. Mood: tea "
        "ceremony aesthetic, north Japan quiet. Shot on Hasselblad, "
        "80mm, f/4. 16:9, 1920x1080."
    ),
    # 4. Stack — all 3 items together
    "stack": (
        "Product photograph from 45-degree top angle. Three items stacked "
        "on dark walnut: a folded black T-shirt (top), folded indigo apron "
        "(middle), folded indigo tenugui (bottom). All have small gold-foil "
        "kanji marks visible. Brand kit feel. Subtle warm indirect light. "
        "Mood: capsule collection unboxing aesthetic, monastic, high-end. "
        "Shot on Hasselblad, 80mm, f/4. 16:9, 1920x1080."
    ),
}


def generate(prompt: str) -> bytes:
    client = genai.Client(api_key=API_KEY)
    resp = client.models.generate_content(
        model=MODEL,
        contents=[prompt],
        config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"]),
    )
    for part in resp.candidates[0].content.parts:
        if hasattr(part, "inline_data") and part.inline_data:
            data = part.inline_data.data
            if isinstance(data, str):
                return base64.b64decode(data)
            return data
    raise RuntimeError("no image returned")


def main() -> None:
    for name, prompt in PROMPTS.items():
        out = OUT_DIR / f"{name}.jpg"
        if out.exists() and "--force" not in sys.argv:
            print(f"[skip] {out}")
            continue
        print(f"[gen] {name}")
        raw = generate(prompt)
        img = Image.open(io.BytesIO(raw)).convert("RGB")
        w, h = img.size
        target = 16 / 9
        if w / h > target:
            nw = int(h * target); left = (w - nw) // 2
            img = img.crop((left, 0, left + nw, h))
        elif w / h < target:
            nh = int(w / target); top = (h - nh) // 2
            img = img.crop((0, top, w, top + nh))
        img.thumbnail((1920, 1080), Image.LANCZOS)
        img.save(out, "JPEG", quality=88, optimize=True)
        print(f"  → {out} ({out.stat().st_size:,} bytes)")
    print(f"\n✓ done. files in {OUT_DIR}/")


if __name__ == "__main__":
    main()
