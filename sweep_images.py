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

PROMPTS = [
    ("sweep-rashguard-ls",
     "Editorial fashion product photo, fitted long-sleeve BJJ rashguard, "
     "navy with subtle off-white pinstripes (echoing Hokkaido weather radar lines), "
     "worn by a 30 year-old Japanese man in a Tokyo dojo at dawn, candid posture, "
     "soft natural light, 50mm lens, 4:5 portrait, no logos or text other than a small chest emblem. "
     "Clean minimalist composition, slight grain, magazine quality."),
    ("sweep-fight-shorts",
     "Studio product shot, BJJ MMA fight shorts, all-black with one subtle "
     "yellow gradient line on the side seam (representing temperature data), "
     "worn by an Asian athlete mid-stance, plain concrete background, "
     "dramatic side lighting, 4:5 portrait, photographic realism, no captions."),
    ("sweep-spats",
     "Half-body action shot of a BJJ practitioner wearing compression "
     "grappling spats in matte black, faint serial-number stitching down the "
     "calf, knee bent in low guard pose on tatami, monochrome, dojo, "
     "natural light, 4:5 portrait, no logos, photographic, slight motion blur."),
    ("sweep-hoodie",
     "Lifestyle editorial: 32 year-old Japanese man wearing a heavy loop-back "
     "cotton hoodie in dim charcoal grey, late-afternoon Tokyo apartment, "
     "leaning on a wood-panel wall, small embroidered MU×SWEEP serial on chest, "
     "soft window light, 4:5 portrait, no text overlays, magazine candid."),
    ("sweep-tee",
     "Heavy cotton white T-shirt on a 28 year-old Japanese woman, plain studio "
     "background, half-body shot, small embroidered MU×SWEEP wordmark on left chest "
     "and a bold abstract MUGEN print on center back (partial view), natural "
     "lighting, 4:5 portrait, photographic, premium minimalist styling."),
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
             f"{R2_BUCKET}/sweep/{slug}.jpg",
             f"--file={tmp}",
             "--remote",
             "--content-type=image/jpeg"],
            capture_output=True, text=True, timeout=90,
        )
        if result.returncode != 0:
            raise RuntimeError(f"wrangler: {result.stderr[-400:]}")
        return f"https://{PUBLIC_HOST}/sweep/{slug}.jpg"
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
