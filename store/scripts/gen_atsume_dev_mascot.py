#!/usr/bin/env python3
"""Generate the ATSUME Dev mascot for the MU × ATSUME collab.

ATSUME (株式会社アツメ) = "挑戦者の仲間を集める" — gathering challengers.
Brand motif (from the catalog generator): scattered dots condensing into a
single mark, minimal monoline. ATSUME Dev = the engineering team behind their
sports apps (TORASPO / ELEVEN / WeGoFast / BLANK_).

Output:
  static/atsume/d/atsume_dev_white.png  — mascot on pure white (mockup/preview)
  static/atsume/d/design_ATSUME-TEE-DEV.png — white keyed to transparent (print)

Model: gemini-3-pro-image-preview ONLY (per workspace policy).
"""
import os, sys, pathlib
from google import genai
from google.genai import types
from PIL import Image
import io

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY:
    sys.exit("GEMINI_API_KEY not set (source /Users/yuki/.env)")

OUT_DIR = pathlib.Path(__file__).resolve().parent.parent / "static" / "atsume" / "d"
OUT_DIR.mkdir(parents=True, exist_ok=True)

PROMPT = """A clean, premium screen-print-ready MASCOT CHARACTER illustration for a
developer-team apparel collaboration called "ATSUME DEV".

Subject: a friendly but cool engineer-athlete mascot — a character that is half
software developer, half sportsperson. Wearing a hoodie with the sleeves pushed
up, headphones around the neck, holding a glowing controller/laptop in one hand
and a sports ball motif tucked under the other arm. Confident relaxed pose,
three-quarter view. Approachable, modern, a little playful — the spirit of a
small elite dev studio that builds sports apps.

Signature ATSUME motif: scattered small dots/particles in the air around the
character that visibly CONDENSE and flow into a single solid mark — the idea of
"gathering challengers into one team". Let a few dots trail from the edges into
the character's silhouette.

Style: bold modern monoline + flat fills, confident clean linework like a
high-end streetwear / esports graphic. Mostly INK BLACK (#111) with ONE single
warm accent color used sparingly (a warm amber/orange, roughly #F2792B) for the
condensing dots and one or two highlights. No gradients-heavy rendering, no
photo-realism. Iconic, instantly readable at chest-print size.

Composition: single centered character, full figure, generous margin, NOTHING
touching the edges. Absolutely PURE SOLID WHITE (#FFFFFF) background, no shadow
on the ground, no scene, no frame, no border. Do not add any text, wordmark, or
logo lettering — character art only. High resolution, crisp edges suitable for
DTG t-shirt printing."""

def main():
    client = genai.Client(api_key=KEY)
    print("[atsume] requesting mascot from gemini-3-pro-image-preview …")
    resp = client.models.generate_content(
        model="gemini-3-pro-image-preview",
        contents=PROMPT,
        config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"]),
    )
    img_bytes = None
    for cand in resp.candidates or []:
        for part in (cand.content.parts or []):
            if getattr(part, "inline_data", None) and part.inline_data.data:
                img_bytes = part.inline_data.data
                break
        if img_bytes:
            break
    if not img_bytes:
        sys.exit("[atsume] no image returned")

    white_path = OUT_DIR / "atsume_dev_white.png"
    white_path.write_bytes(img_bytes)
    img = Image.open(io.BytesIO(img_bytes)).convert("RGBA")
    print(f"[atsume] saved white-bg mascot {white_path} {img.size}")

    # Key pure-white → transparent for the print file (matches the Rust
    # generate_transparent_print threshold of R,G,B >= 248).
    px = img.load()
    w, h = img.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if r >= 248 and g >= 248 and b >= 248:
                px[x, y] = (r, g, b, 0)
    design_path = OUT_DIR / "design_ATSUME-TEE-DEV.png"
    img.save(design_path)
    print(f"[atsume] saved transparent print {design_path}")

if __name__ == "__main__":
    main()
