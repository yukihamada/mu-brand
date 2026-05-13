#!/usr/bin/env python3
"""
MU × KOKON v3 — Honoo-kun character-integrated product mockups.

kokon.tokyo の公式マスコット「炎くん (Honoo-kun)」を商品にあしらった新世代。
- 炭火の精霊として生まれた橙〜赤の炎キャラ
- VERSION B 公式: 霜降りをまとい炭火鋏を握る焼肉師スタイル
- VERSION A 伝統: 注連縄と藍染め前掛け

Generates: 6 SKU mockups featuring Honoo-kun + KOKON wordmark,
uploads to R2, PATCHes image_url on collab_products rows.
"""
import os, sys, io, base64, tempfile, subprocess, requests
from PIL import Image

os.environ.pop("GOOGLE_API_KEY", None)
from google import genai
from google.genai import types

GEMINI_API_KEY = os.environ["GEMINI_API_KEY"]
GEMINI_MODEL   = "gemini-3-pro-image-preview"
STORE_URL      = os.environ.get("MU_STORE_URL", "https://wearmu.com")
ADMIN_TOKEN    = os.environ["MU_ADMIN_TOKEN"]
WRANGLER_BIN   = os.environ.get("WRANGLER_BIN", "/opt/homebrew/bin/wrangler")
R2_BUCKET      = "wearmu-lifestyle"
PUBLIC_HOST    = "lifestyle.wearmu.com"

# Honoo-kun (炎くん) — 焼肉古今 公式マスコット
# Visual: a small anthropomorphic orange-red flame spirit, friendly chibi style
# Worn elements (Version B 公式): wagyu marbling cape + tongs in hand
HONOO_SPEC = (
    "Featuring 'Honoo-kun' (炎くん), the official mascot of Yakiniku Kokon: "
    "a small chibi character, body made of warm orange-red flame (gradient from "
    "deep amber #B85C00 base to brighter orange #FF8C1A tips), big expressive "
    "round black eyes with a small white highlight, friendly smile, no nose, "
    "small flame-shaped tail at top of head, wearing a tiny wagyu-marbling "
    "patterned cape (cream/white with thin red marbling lines) and holding "
    "miniature charcoal tongs. He is the spirit of charcoal fire that guards "
    "Kokon's grilling tradition. The mascot is integrated naturally into each "
    "product photo as a small accent — not the main focus, but a charming "
    "signature of the Kokon brand."
)

BRAND_SPEC = (
    "Kokon (焼肉古今) brand: Nishi-Azabu premium private-room yakiniku restaurant. "
    "Palette: pure black (#0A0A0A) dominant, warm metallic Old Gold (#A67843) "
    "for the 'KOKON' wordmark, cream (#F5F5F0) accents from wagyu marbling. "
    "Vibe: Michelin-level minimalism with the warmth of charcoal fire. "
    "Premium minimalist editorial product photography. Photographic realism "
    "for the product itself; Honoo-kun rendered in clean illustration style "
    "that contrasts pleasantly with the photo (similar to mascot integration "
    "on premium Japanese brand merchandise — e.g., Kumamon on a black product)."
)

PROMPTS = [
    ("kokon-apron",
     "Editorial product photo. Foreground: black canvas chef's strap apron (full bib + tie back), "
     "small Old Gold 'KOKON' wordmark printed center-chest, with Honoo-kun illustration printed "
     "below the wordmark holding tiny tongs and grinning. The apron is worn over a black "
     "collarless shirt by a 40-year-old Japanese yakiniku chef behind a glowing charcoal grill "
     "in a high-end Tokyo private-room restaurant. Soft warm fire-lit ambience, sharp focus on "
     "the apron print. Style: premium Japanese restaurant brand merchandise."),
    ("kokon-mug",
     "Editorial product photo, black glossy ceramic mug (11oz) on a slate countertop next to "
     "a glass of Japanese whisky. The mug surface shows 'KOKON' wordmark in Old Gold above "
     "Honoo-kun who is winking and waving with tiny tongs. Behind: soft bokeh of a private "
     "yakiniku room. Premium product photography, dramatic warm side-light from a charcoal grill. "
     "Sharp focus on the wordmark and the mascot illustration."),
    ("kokon-tee",
     "Editorial product photo. Premium heavy black cotton tee, folded neatly on a dark slate "
     "table next to a small charcoal lantern. Left chest: Old Gold 'KOKON' wordmark with "
     "Honoo-kun standing below it in a confident pose holding miniature tongs. Soft amber "
     "side-light, premium Japanese restaurant merchandise vibe. Sharp focus on the chest print."),
    ("kokon-crewneck",
     "Editorial product photo. Black Champion heavy fleece crewneck sweatshirt, worn by a "
     "40-year-old Japanese yakiniku chef removing his apron at end of service. Old Gold 'KOKON' "
     "wordmark across the chest with Honoo-kun illustrated below, looking sleepy/satisfied after "
     "a busy night. Warm interior lighting, premium minimalist editorial framing."),
    ("kokon-snapback",
     "Editorial product photo, black flat-brim snapback hat displayed on a black wooden "
     "pedestal. Front panel: 'KOKON' wordmark embroidered in Old Gold (#A67843) with a small "
     "Honoo-kun embroidered patch below, gripping miniature tongs and grinning. Side panel: "
     "subtle gold pinstripe. Dramatic top-down studio lighting, sharp focus on the embroidery."),
    ("kokon-stickers",
     "Editorial product photo of a flat-lay kiss-cut sticker sheet (5.83 x 8.27 inch). "
     "On the sheet: 'KOKON' wordmark in Old Gold center top, plus 6 different Honoo-kun "
     "expressions/poses as individual kiss-cut stickers — Wave (greeting), Grill (with tongs), "
     "Cool (with sunglasses), Happy (eyes closed smiling), Bow (formal greeting), Wagyu "
     "(hugging a tiny steak). Backdrop: dark slate. Premium product photography, top-down."),
]


def generate(slug: str, prompt: str) -> bytes:
    client = genai.Client(api_key=GEMINI_API_KEY)
    full_prompt = f"{prompt}\n\n{HONOO_SPEC}\n\n{BRAND_SPEC}"
    print(f"  Gemini[{GEMINI_MODEL}]: {slug}...")
    response = client.models.generate_content(
        model=GEMINI_MODEL,
        contents=full_prompt,
        config=types.GenerateContentConfig(
            response_modalities=["IMAGE", "TEXT"],
        ),
    )
    for part in response.candidates[0].content.parts:
        if hasattr(part, "inline_data") and part.inline_data:
            data = part.inline_data.data
            if isinstance(data, str):
                return base64.b64decode(data)
            return data
    raise RuntimeError(f"Gemini returned no image for {slug}")


def upload_to_r2(slug: str, jpg_bytes: bytes) -> str:
    with tempfile.NamedTemporaryFile(suffix=".jpg", delete=False) as f:
        img = Image.open(io.BytesIO(jpg_bytes)).convert("RGB")
        img.save(f.name, format="JPEG", quality=88, optimize=True)
        tmp = f.name
    try:
        result = subprocess.run(
            [WRANGLER_BIN, "r2", "object", "put",
             f"{R2_BUCKET}/kokon/{slug}.jpg",
             f"--file={tmp}",
             "--remote",
             "--content-type=image/jpeg"],
            capture_output=True, text=True, timeout=120,
        )
        if result.returncode != 0:
            raise RuntimeError(f"wrangler: {result.stderr[-400:]}")
        return f"https://{PUBLIC_HOST}/kokon/{slug}.jpg"
    finally:
        try: os.unlink(tmp)
        except: pass


def patch_db(slug: str, image_url: str):
    r = requests.patch(
        f"{STORE_URL}/api/admin/collab_image?token={ADMIN_TOKEN}",
        json={"slug": slug, "image_url": image_url}, timeout=20,
    )
    print(f"  PATCH /api/admin/collab_image → {r.status_code} {r.text[:120]}")


def main():
    only = sys.argv[1] if len(sys.argv) > 1 else None
    for slug, prompt in PROMPTS:
        if only and only != slug:
            continue
        try:
            img_bytes = generate(slug, prompt)
            url = upload_to_r2(slug, img_bytes)
            print(f"  → {url}")
            patch_db(slug, url)
        except Exception as e:
            print(f"  ! {slug} failed: {e}", file=sys.stderr)


if __name__ == "__main__":
    main()
