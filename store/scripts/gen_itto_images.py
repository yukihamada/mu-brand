#!/usr/bin/env python3
"""Generate cinematic images for the MU × 焼肉古今 'itto' campaign page.
Saves under store/static/itto/<name>.jpg."""
import base64, io, os, sys
from pathlib import Path

from google import genai
from google.genai import types
from PIL import Image

API_KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not API_KEY:
    sys.exit("GEMINI_API_KEY required (source /Users/yuki/.env)")

MODEL = "gemini-3-pro-image-preview"
OUT_DIR = Path(__file__).resolve().parent.parent / "static" / "itto"
OUT_DIR.mkdir(parents=True, exist_ok=True)


# Each prompt is tuned for the MU brand aesthetic (monastic, dark, north Japan
# clarity) + the yakiniku-kokon visual world.
PROMPTS = {
    # Hero — calf in winter Tajima farm. Soft, restrained, almost reverent.
    "hero_calf": (
        "Ultra-cinematic wide photograph. A single black Tajima Wagyu calf "
        "standing alone in a snow-dusted Hyogo farm at dawn. Soft pale-gold "
        "light catches its breath in cold air. Dark spruce treeline in the "
        "blurred distance. Deeply monastic mood — quiet, sober, no sentimentality. "
        "Shot on Hasselblad H6D, 100mm, f/2, very shallow depth of field. "
        "Earth tones: charcoal, soft ivory, faded gold. No text. "
        "Wide aspect 16:9, 1920x1080."
    ),
    # Kokon interior — intimate counter, charcoal grill, low light.
    "kokon_counter": (
        "Cinematic photograph of a high-end Tokyo yakiniku private room, "
        "Nishi-Azabu style. Black lacquered counter, a single binchotan "
        "charcoal grill glowing deep orange in the foreground. A single thin "
        "slice of marbled wagyu being placed by leather-aproned hands of a "
        "yakishi (grill master), out of focus. Background: dim warm amber "
        "indirect lighting, blurred shoji screens. Mood: monastic, exact, "
        "almost church-like. No people's faces. Shot on Leica Q3, 28mm, "
        "f/2.8, ISO 800. 16:9, 1920x1080."
    ),
    # Communal — long dark counter, many quiet shadowed seats, charcoal smoke.
    "communal": (
        "Cinematic photograph from above, top-down 75-degree angle. A long "
        "dark wood counter extending into shadow, with twelve binchotan "
        "grill stations glowing soft orange along it. Steam and smoke rising "
        "in straight columns. Small Wagyu slices being grilled by unseen "
        "hands. Tokyo Nishi-Azabu yakiniku private room aesthetic. Background "
        "deep black. Mood: ritualistic, monastic, slow. 16:9, 1920x1080."
    ),
    # Trace — the cow data + photo composition, like an archive scientific page.
    "trace": (
        "Quiet still-life photograph. A black ceramic tray on aged dark "
        "wood. On the tray: a single small printed monochrome portrait of a "
        "Tajima calf, a thin gold-foil serial number tag '#0001', and a "
        "single hand-folded paper card with sumi calligraphy of the kanji "
        "「命」(life). Soft north-light from the left. Shot on Hasselblad, "
        "80mm, f/4. Mood: monastic, archival, reverent. 16:9, 1920x1080."
    ),
    # Plate — final dish, just one slice — restrained, monastic.
    "plate": (
        "Macro cinematic photograph. A single perfectly grilled slice of "
        "Tajima Wagyu sirloin (sear marks visible, faint pink center), "
        "placed alone on a hand-thrown black ceramic plate. Single grain of "
        "salt. Tiny smear of nikiri shoyu reflecting amber light. Top-down, "
        "very shallow depth of field. Background black with single warm "
        "highlight. Restrained, monastic, no garnish, no parsley, no garnish "
        "of any kind. 16:9, 1920x1080."
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
            print(f"[skip] {out} already exists (use --force to regenerate)")
            continue
        print(f"[gen] {name}: requesting from {MODEL}")
        raw = generate(prompt)
        img = Image.open(io.BytesIO(raw)).convert("RGB")
        # Center-crop to 16:9.
        target_ratio = 16 / 9
        w, h = img.size
        if w / h > target_ratio:
            new_w = int(h * target_ratio)
            left = (w - new_w) // 2
            img = img.crop((left, 0, left + new_w, h))
        elif w / h < target_ratio:
            new_h = int(w / target_ratio)
            top = (h - new_h) // 2
            img = img.crop((0, top, w, top + new_h))
        # Downsize to 1920x1080 max.
        img.thumbnail((1920, 1080), Image.LANCZOS)
        img.save(out, "JPEG", quality=88, optimize=True)
        print(f"  → {out} ({out.stat().st_size:,} bytes, {img.size})")
    print(f"\n✓ done. files in {OUT_DIR}/")


if __name__ == "__main__":
    main()
