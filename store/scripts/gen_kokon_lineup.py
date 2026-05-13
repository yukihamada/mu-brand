#!/usr/bin/env python3
"""Generate the full kokon × MU goods lineup with logo + cow character.

Includes:
  - One mascot cow design (re-used across products)
  - 10 product mockups (T-shirts, hoodie, caps, aprons, tenugui, tote, mug, stickers)

Saves to store/static/itto/goods/lineup/<slug>.jpg.
"""
import base64, io, os, sys
from pathlib import Path

from google import genai
from google.genai import types
from PIL import Image

API_KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not API_KEY:
    sys.exit("GEMINI_API_KEY required")

MODEL = "gemini-3-pro-image-preview"
OUT_DIR = Path(__file__).resolve().parent.parent / "static" / "itto" / "goods" / "lineup"
OUT_DIR.mkdir(parents=True, exist_ok=True)


# Verbal description of the kokon logo (so Gemini can reproduce it consistently).
# Real logo at https://kokon.tokyo/static/img/logo.png — bold black geometric:
# circle containing a diamond, divided by a cross into four triangle quadrants.
LOGO = (
    "the kokon logo: a bold black filled circle, containing a diamond/rhombus, "
    "subdivided by a single cross (horizontal + vertical) into four triangular "
    "quadrants. Highly geometric, sumi-ink monogram, no shading"
)

# The cow character — minimalist sumi-brushwork silhouette in profile.
COW = (
    "a small minimalist sumi-brushwork cow character: a Japanese black cow "
    "(Wagyu) in side profile, single confident brush strokes for body and "
    "head, almost calligraphic, very restrained, no facial detail except a "
    "tiny dot for the eye. Like a hanko stamp"
)

PROMPTS = {
    # ── core mascot reference (used later in marketing too) ──
    "mascot_cow": (
        "Product image: a single 1024x1024 reference plate showing the kokon "
        "cow mascot character — " + COW + " — centered on a pure white "
        "background. Above it, the kokon logo: " + LOGO + ". Both elements "
        "rendered in solid black ink, monastic and minimalist. Square."
    ),

    # ── Apparel ──
    "tshirt_logo_black": (
        "Product photograph. Black Bella+Canvas 3001 unisex T-shirt laid flat "
        "on dark walnut. Chest center: " + LOGO + ", small (~7cm), in gold "
        "foil. Below it, tiny monospaced text 'kokon × wearmu.com'. Restrained "
        "high-end DTC aesthetic. Soft north-window light from upper-left. "
        "Hasselblad H6D, 80mm, f/4. 16:9, 1920x1080."
    ),
    "tshirt_cow_white": (
        "Product photograph. Natural off-white Bella+Canvas 3001 T-shirt "
        "laid flat on light wood. Chest center: " + COW + ", roughly 10cm "
        "tall, ink black. Tiny kokon logo (" + LOGO + ") in the bottom-right "
        "of the chest as a signature. Mood: tea-ceremony quiet, minimal. "
        "Leica Q3, 28mm, f/4. 16:9, 1920x1080."
    ),
    "tshirt_kanji_grey": (
        "Product photograph. Heather-grey Bella+Canvas 3001 T-shirt on a "
        "dark concrete surface. Centered chest: vertical sumi-brush "
        "calligraphy 「焼肉古今」 in deep black, ~12cm tall. Tiny " + LOGO + " "
        "on the bottom hem at the side. Mood: monastic, professional. "
        "Phase One, 80mm, f/4. 16:9, 1920x1080."
    ),
    "hoodie_back_logo": (
        "Product photograph from the back. Heavy black 425gsm hoodie hanging "
        "on a dark walnut hanger against a black brick wall. Centered back, "
        "large: " + LOGO + ", ~25cm in faded gold-foil print. Tiny " + COW + " "
        "on the lower right hem as a signature. Mood: monastic high-end "
        "streetwear. Leica Q3, 28mm, f/4. 16:9, 1920x1080."
    ),
    "cap_logo_black": (
        "Product photograph. 5-panel unstructured black cotton cap on a "
        "dark walnut table. Center front panel: " + LOGO + " in gold "
        "embroidery, small (~4cm). Slight scratchy fabric texture visible. "
        "Soft single overhead light. Mood: capsule kit minimalism. "
        "Hasselblad, 80mm, f/4. 16:9, 1920x1080."
    ),
    "cap_cow_beige": (
        "Product photograph. Natural canvas beige 5-panel cap. Center "
        "front panel: " + COW + ", small (~4cm), black thread embroidery. "
        "Top-down 45-degree angle, on a dark wood plate. Soft warm light. "
        "Mood: artisanal, restrained, hanko-like. 16:9, 1920x1080."
    ),

    # ── Kitchen / apron ──
    "apron_denim_kanji": (
        "Product photograph. Heavy indigo selvedge-denim yakiniku apron "
        "hanging on a black hook against a dark concrete wall. Chest panel: "
        "vertical sumi calligraphy 「焼肉古今」 in faded white thread "
        "embroidery, with " + LOGO + " (small, gold thread) in the bottom-"
        "right of the chest panel. Brass rivets. Slight real-use patina. "
        "Mood: professional yakiniku house. Leica Q3, 28mm. 16:9."
    ),
    "apron_canvas_cow": (
        "Product photograph. Natural canvas chef apron in cream, hanging "
        "on a wooden hook. Chest panel: " + COW + " as the centerpiece, "
        "black ink-print, ~15cm tall. Small " + LOGO + " in the bottom-"
        "right corner as a hanko signature. Wooden hanger detail. Mood: "
        "Aesop-style minimal craft. 16:9, 1920x1080."
    ),

    # ── Tenugui (Japanese hand towel) ──
    "tenugui_kanji_indigo": (
        "Product photograph from 45-degree angle. Traditional Japanese cotton "
        "tenugui hand towel in deep indigo, partially unfolded on a black "
        "ceramic plate. Vertical sumi-calligraphy 「焼肉古今」 in faded "
        "white runs down the center. " + LOGO + " (small, gold) at the "
        "top-right corner. Single overhead light. Mood: tea-ceremony. "
        "Hasselblad, 80mm. 16:9, 1920x1080."
    ),
    "tenugui_cow_white": (
        "Product photograph. White cotton tenugui hand towel on a dark "
        "charcoal-stained wooden plank, partially folded. Pattern: repeating "
        "small " + COW + " characters in faded black, like a hanko pattern "
        "across the cloth. " + LOGO + " in one corner. Mood: minimal, "
        "tea-ceremony. 16:9."
    ),

    # ── Tote ──
    "tote_logo_natural": (
        "Product photograph. Natural canvas heavy tote bag, large size, "
        "lying flat on a dark wood table. Centered face: " + LOGO + " in "
        "solid black ink, ~15cm. Bottom-right corner of the face: small "
        "monospaced text 'kokon × MU'. Mood: capsule-kit minimal. "
        "Hasselblad, 80mm. 16:9, 1920x1080."
    ),
    "tote_cow_black": (
        "Product photograph. Heavy black canvas tote on a dark wood "
        "surface. Centered: " + COW + " in faded white ink-print, ~12cm. "
        "Small " + LOGO + " in faded gold on the bottom strap. Mood: "
        "monastic high-end. 16:9, 1920x1080."
    ),

    # ── Other ──
    "mug_logo_white": (
        "Product photograph. Pure-white enamel camp mug with steel rim, "
        "sitting on a dark wood plank with a charcoal grill visible blurred "
        "in the background. Front: " + LOGO + " in matte black, centered. "
        "Soft warm side light. Mood: yakiniku-restaurant souvenir but "
        "elevated. 16:9, 1920x1080."
    ),
    "stickers_pack": (
        "Product photograph. Three vinyl stickers laid flat on a dark wood "
        "surface, slightly overlapping like a fan: (1) " + LOGO + " in solid "
        "black, circle die-cut; (2) " + COW + " in black ink on white "
        "ground, hanko-shape die-cut; (3) sumi calligraphy 「焼肉古今」 "
        "in vertical, rectangle die-cut. Soft north-window light. Mood: "
        "deluxe sticker pack. 16:9, 1920x1080."
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
    only = sys.argv[1] if len(sys.argv) > 1 and not sys.argv[1].startswith("--") else None
    for name, prompt in PROMPTS.items():
        if only and only != name:
            continue
        out = OUT_DIR / f"{name}.jpg"
        if out.exists() and "--force" not in sys.argv:
            print(f"[skip] {out}")
            continue
        print(f"[gen] {name}")
        try:
            raw = generate(prompt)
        except Exception as e:
            print(f"  ✗ failed: {e}")
            continue
        img = Image.open(io.BytesIO(raw)).convert("RGB")
        # Center-crop to 16:9, downsize.
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
