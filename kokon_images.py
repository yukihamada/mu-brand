#!/usr/bin/env python3
"""
MU × SWEEP — product image generator.

For each of the 5 collab_products (partner='sweep'), generate a product
mockup or lifestyle photo via Gemini 3 Pro Image, upload to R2 bucket
wearmu-lifestyle (path sweep/<slug>.jpg, served at lifestyle.wearmu.com),
and PATCH /api/admin/collab_image to set image_url on the row.
"""
import os, sys, io, base64, tempfile, subprocess, requests
from PIL import Image

os.environ.pop("GOOGLE_API_KEY", None)
from google import genai
from google.genai import types

GEMINI_API_KEY = os.environ["GEMINI_API_KEY"]
GEMINI_MODEL   = "gemini-3-pro-image-preview"
STORE_URL      = os.environ.get("MU_STORE_URL", "https://wearmu.com")
ADMIN_TOKEN    = os.environ.get("MU_ADMIN_TOKEN", "mu-admin-2026")
WRANGLER_BIN   = os.environ.get("WRANGLER_BIN", "/opt/homebrew/bin/wrangler")
R2_BUCKET      = "wearmu-lifestyle"
PUBLIC_HOST    = "lifestyle.wearmu.com"

# KOKON v2 — 焼肉古今ブランド世界観
# 純但馬牛・田村牧場直送・完全個室・専属焼き師・炭火
# パレット: 黒 (炭) × 金 (#A67843 焦げ目 / Old Gold) × 白 (純粋)
# キャラ無し、ミニマル KOKON wordmark のみ。Michelin 級の重み。
BRAND_SPEC = (
    "Brand: minimal 'KOKON' wordmark only (uppercase modern sans-serif, Helvetica Now style, tight kerning) "
    "in warm metallic Old Gold color (hex #A67843, not bright). Below it the kanji '焼肉古今' optionally. "
    "Pure black product surfaces dominate. Michelin-star restaurant brand mark vibe. "
    "Premium minimalist editorial. Photographic realism. No other text, no characters, no graphics."
)
PROMPTS = [
    ("kokon-apron",
     "Editorial product photo, black canvas chef's strap apron (full bib + tie back), "
     "small Old Gold 'KOKON' wordmark printed center-chest, worn over a black collarless "
     "shirt by a 40 year-old Japanese yakiniku chef ('焼き師') with rolled sleeves, "
     "behind a glowing charcoal grill in a high-end Tokyo private-room yakiniku restaurant, "
     "dramatic ember-orange light from the grill, soft tungsten fill, slight smoke, "
     "half-body 3/4 view, 4:5 portrait, photographic, Michelin-star magazine editorial, "
     "no other props in focus. " + BRAND_SPEC),
    ("kokon-mug",
     "Premium product still life, black glossy ceramic mug (11oz) with small Old Gold "
     "'KOKON' wordmark printed on the side, sitting on a dark walnut counter next to a "
     "small ceramic dish with a single piece of Tajima beef tongue ('タン'), "
     "warm soft tungsten light from above, deep shadows, slight steam, 4:5 portrait, "
     "shallow depth of field, photographic, no models, magazine editorial. " + BRAND_SPEC),
    ("kokon-tee",
     "Editorial portrait product photo, plain matte black heavy cotton crewneck t-shirt "
     "with a small Old Gold tonal 'KOKON' wordmark DTG-printed on the left chest, "
     "worn by a 35 year-old Japanese man with refined posture at a yakiniku counter, "
     "ember light from the charcoal grill, half-body candid, soft tungsten fill, "
     "4:5 portrait, photographic, magazine editorial, premium minimalist styling. " + BRAND_SPEC),
    ("kokon-crewneck",
     "Editorial product photo, heavy black Champion crewneck sweatshirt with small "
     "Old Gold tonal 'KOKON' wordmark DTG-printed on the left chest, "
     "worn by a 32 year-old Japanese man leaning at a charcoal-blackened brick wall "
     "outside a Tokyo yakiniku restaurant at night, single red paper lantern in soft "
     "background bokeh, half-body, 4:5 portrait, photographic, magazine editorial. " + BRAND_SPEC),
    ("kokon-snapback",
     "Editorial portrait of a 30 year-old Japanese man wearing a black flat-brim "
     "snapback cap with Old Gold (hex #A67843) thread-embroidered 'KOKON' wordmark on "
     "the front panel, half-body candid in a dim warm-lit yakiniku restaurant "
     "private room corridor, deep shadows, embers glowing in background bokeh, "
     "4:5 portrait, photographic, magazine editorial, refined. " + BRAND_SPEC),
    ("kokon-stickers",
     "Macro top-down product photo, a kiss-cut sticker sheet on pure black background, "
     "featuring multiple Old Gold 'KOKON' wordmarks, a small charcoal grill grid icon, "
     "an ember silhouette, and a Tajima beef cattle outline, all in monochrome Old Gold "
     "(#A67843) on black, arranged in a tight refined grid, studio side lighting, "
     "4:5 portrait, photographic, premium minimal product photography. " + BRAND_SPEC),
]



def gen_image(prompt: str) -> bytes:
    client = genai.Client(api_key=GEMINI_API_KEY)
    response = client.models.generate_content(
        model=GEMINI_MODEL,
        contents=[prompt],
        config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"]),
    )
    for part in response.candidates[0].content.parts:
        if hasattr(part, "inline_data") and part.inline_data:
            data = part.inline_data.data
            if isinstance(data, str):
                return base64.b64decode(data)
            return data
    raise RuntimeError("Gemini returned no image")


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
            capture_output=True, text=True, timeout=90,
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
    targets = sys.argv[1:] or [s for s, _ in PROMPTS]
    ok = 0
    for slug, prompt in PROMPTS:
        if slug not in targets:
            continue
        print(f"\n[{slug}]")
        try:
            print(f"  prompt: {prompt[:90]}…")
            img_bytes = gen_image(prompt)
            url = upload_to_r2(slug, img_bytes)
            patch_db(slug, url)
            print(f"  ✓ {url}")
            ok += 1
        except Exception as e:
            print(f"  ✗ {type(e).__name__}: {e}")
    print(f"\nDone — {ok}/{len(targets) if targets else len(PROMPTS)} succeeded.")


if __name__ == "__main__":
    main()
