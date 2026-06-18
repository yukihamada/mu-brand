#!/usr/bin/env python3
"""Generate @wearMUcom profile + banner images via Gemini 3 Pro Image."""
import base64
import io
import os
import sys
from pathlib import Path

from google import genai
from google.genai import types
from PIL import Image

API_KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not API_KEY:
    sys.exit("GEMINI_API_KEY required (source /Users/yuki/.env)")

MODEL = "gemini-3-pro-image-preview"
OUT_DIR = Path(__file__).resolve().parent.parent / "static" / "x"
OUT_DIR.mkdir(parents=True, exist_ok=True)

PROMPTS = {
    "profile": (
        "Minimalist gold-on-black brand mark. Two giant bold sans-serif "
        "letters 'MU' in pure gold (#e6c449) on a pure black (#000) "
        "background. Perfectly centered. Generous whitespace. Ultra-modern "
        "typography reminiscent of A24 film posters, Aesop, Our Legacy, "
        "Acne Studios. No shadows, no embellishment, no other text or "
        "symbols. Sharp, monastic, almost mathematically precise. "
        "Square 1024x1024 format. High contrast."
    ),
    "banner": (
        "Wide cinematic photograph of frozen Lake Mashu in Hokkaido at "
        "golden hour dawn. Deep teal water visible through thin morning "
        "mist. Snow-dusted dark spruce forest at the edges. Subtle haze "
        "rolling over the water. Single ultra-thin horizontal gold line "
        "(#e6c449) across the lower third, suggesting a horizon mark. "
        "Very small text 'MU' in light gray in the bottom-right corner. "
        "Moody dark teal and graphite palette with the gold accent. "
        "Fashion-brand banner aesthetic (Aesop, COS, Our Legacy). "
        "Ultra detailed photographic look. Wide aspect ratio 3:1, "
        "1500x500 framing."
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
        print(f"[gen] {name}: requesting from {MODEL}")
        raw = generate(prompt)
        out = OUT_DIR / f"{name}.png"
        out.write_bytes(raw)

        # Sanity: check dimensions and convert to expected size if needed.
        img = Image.open(io.BytesIO(raw))
        print(f"  → {out} ({len(raw):,} bytes, {img.size})")

        # X profile expects square 400-1024px, banner 1500x500.
        if name == "profile":
            if img.size != (1024, 1024):
                img = img.convert("RGB").resize((1024, 1024), Image.LANCZOS)
                img.save(out, "PNG")
                print(f"  resized → 1024x1024 ({out.stat().st_size:,} bytes)")
        elif name == "banner":
            if img.size != (1500, 500):
                # Center-crop to 3:1 then resize.
                w, h = img.size
                target_ratio = 3.0
                if w / h > target_ratio:
                    new_w = int(h * target_ratio)
                    left = (w - new_w) // 2
                    img = img.crop((left, 0, left + new_w, h))
                else:
                    new_h = int(w / target_ratio)
                    top = (h - new_h) // 2
                    img = img.crop((0, top, w, top + new_h))
                img = img.convert("RGB").resize((1500, 500), Image.LANCZOS)
                img.save(out, "PNG")
                print(f"  cropped+resized → 1500x500 ({out.stat().st_size:,} bytes)")

    print(f"\n✓ done. files in {OUT_DIR}/")


if __name__ == "__main__":
    main()
