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

# All 8 prompts share a brand spec: small "kokon.tokyo" wordmark printed/embroidered
# on the product. Editorial yakiniku-restaurant brand aesthetic: warm minimalist,
# Tokyo backstreet vibes, off-charcoal palette.
BRAND_SPEC = (
    "Brand: small clean 'kokon.tokyo' wordmark (lowercase Helvetica, kerning tight) "
    "printed or embroidered on the product as the only branding. Warm minimalist editorial "
    "Japanese restaurant aesthetic. Photographic realism, no other text or graphics."
)
PROMPTS = [
    ("kokon-tee",
     "Editorial product photo, plain black heavy cotton t-shirt with a small "
     "'kokon.tokyo' wordmark printed in cream-white ink on the left chest, "
     "worn by a 32 year-old Japanese man in a warm-lit Tokyo back-alley yakiniku restaurant, "
     "half-body candid, soft tungsten light, 4:5 portrait, photographic, magazine candid. " + BRAND_SPEC),
    ("kokon-crewneck",
     "Heavy black Champion crewneck sweatshirt with a small 'kokon.tokyo' wordmark "
     "DTG-printed cream-white on the left chest, worn casually by a 28 year-old Japanese "
     "woman seated at a yakiniku counter, soft tungsten and ember light from grill, "
     "half-body, 4:5 portrait, photographic, editorial. " + BRAND_SPEC),
    ("kokon-tote",
     "Black canvas tote bag with a clean cream-white 'kokon.tokyo' wordmark "
     "centered on the front, hanging from a hook next to the entrance of a Tokyo yakiniku "
     "restaurant at dusk, warm street lantern glow, 4:5 portrait still life, "
     "photographic, no models. " + BRAND_SPEC),
    ("kokon-stickers",
     "Macro top-down product photo, a kiss-cut sticker sheet (white background) "
     "featuring multiple 'kokon.tokyo' wordmarks and small Japanese yakiniku icons "
     "(grill grid, ember, chopsticks silhouette), arranged in a grid, "
     "studio side lighting, 4:5 portrait, photographic, premium product photography. " + BRAND_SPEC),
    ("kokon-mug-enamel",
     "White enamel camping mug with a small 'kokon.tokyo' wordmark printed in "
     "ember-orange on the side, sitting on a wooden counter next to a small ceramic "
     "sake cup, soft warm light from a yakiniku restaurant, 4:5 portrait, "
     "shallow depth of field, no models. " + BRAND_SPEC),
    ("kokon-cap",
     "Editorial portrait of a 30 year-old Japanese man wearing a black dad hat "
     "with a small flat cream-white embroidered 'kokon.tokyo' wordmark on the front, "
     "half-body candid in a Tokyo back-street at golden hour with red paper lantern bokeh, "
     "4:5 portrait, photographic, magazine candid. " + BRAND_SPEC),
    ("kokon-apron",
     "White canvas chef's apron with a small black 'kokon.tokyo' wordmark printed "
     "on the chest, worn over a black shirt by a 35 year-old Japanese yakiniku chef "
     "behind a charcoal grill, slight smoke, dramatic ember light from the grill, "
     "half-body, 4:5 portrait, photographic, magazine editorial. " + BRAND_SPEC),
    ("kokon-can-cooler",
     "White neoprene can cooler wrapped around a 350ml beer can, small "
     "'kokon.tokyo' wordmark printed in ember-orange on the side, sitting on a "
     "yakiniku counter next to a small ceramic dish of namul, warm tungsten light, "
     "4:5 portrait, shallow depth of field, no models, editorial product photography. " + BRAND_SPEC),
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
