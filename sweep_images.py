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

# All 17 prompts share a brand spec: small SIIIEEP wordmark (s III ≡ ≡ p,
# the official SWEEP brand logo) embroidered on left chest, and a small
# MU × SIIIEEP serial number stitched on the inside neck label. No big
# graphics — minimalist, editorial.
BRAND_SPEC = (
    "Brand: small embroidered SIIIEEP wordmark (s III equals-three-bars p logo) "
    "on left chest in matching tonal thread, and a tiny MU × SIIIEEP serial number "
    "on the inside neck. Both logos minimal and tonal, never loud. No other text or graphics."
)
PROMPTS = [
    # ── BJJ 専用品 (SWEEP社 手動生産) ──
    ("sweep-rashguard-ls",
     "Editorial fashion product photo, fitted long-sleeve BJJ rashguard, "
     "navy with subtle off-white pinstripes (echoing Hokkaido weather radar lines), "
     "worn by a 30 year-old Japanese man in a Tokyo dojo at dawn, candid posture, "
     "soft natural light, 50mm lens, 4:5 portrait. " + BRAND_SPEC +
     " Clean minimalist composition, slight grain, magazine quality."),
    ("sweep-fight-shorts",
     "Studio product shot, BJJ MMA fight shorts, all-black with one subtle "
     "yellow gradient line on the side seam (representing temperature data), "
     "worn by an Asian athlete mid-stance, plain concrete background, "
     "dramatic side lighting, 4:5 portrait, photographic realism. " + BRAND_SPEC),
    ("sweep-spats",
     "Half-body action shot of a BJJ practitioner wearing compression "
     "grappling spats in matte black, faint serial-number stitching down the "
     "calf, knee bent in low guard pose on tatami, monochrome, dojo, "
     "natural light, 4:5 portrait, photographic, slight motion blur. " + BRAND_SPEC),
    ("sweep-gi-classic",
     "Studio product photo of a folded BJJ gi (kimono) in raw white 550gsm pearl weave cotton, "
     "neat stack on a concrete plinth, dark backdrop, dramatic side light, "
     "lapel facing the camera with a small embroidered SIIIEEP wordmark, "
     "4:5 portrait, photographic, premium quality, no models. " + BRAND_SPEC),
    ("sweep-belt-promo",
     "Macro product shot of a black BJJ jiu-jitsu belt with one red bar, "
     "tightly coiled on a dark wood floor, end-tag visible with embroidered "
     "MU × SIIIEEP serial number, top-down 4:5 portrait, natural light, "
     "shallow depth of field, no models, magazine quality. " + BRAND_SPEC),
    ("sweep-bjj-tape",
     "Three rolls of white BJJ finger tape stacked on a black concrete surface, "
     "side label visible with small SIIIEEP wordmark printed in dark tonal ink, "
     "studio still life, dramatic top-down side-light, 4:5 portrait, "
     "photographic, minimalist composition, no models, no other text. " + BRAND_SPEC),
    ("sweep-mouthguard",
     "Premium product shot of a brushed anodized aluminum mouthguard case, "
     "rectangular tin with small ventilation holes, top engraved with MU × SIIIEEP "
     "wordmark, sitting on a sheet of folded tatami, soft directional light, "
     "shallow depth of field, 4:5 portrait, no models, editorial product photography. " + BRAND_SPEC),

    # ── ライフスタイル (Printful 系) ──
    ("sweep-hoodie",
     "Lifestyle editorial: 32 year-old Japanese man wearing a heavy loop-back "
     "cotton hoodie in dim charcoal grey, late-afternoon Tokyo apartment, "
     "leaning on a wood-panel wall, soft window light, 4:5 portrait, "
     "no text overlays, magazine candid. " + BRAND_SPEC),
    ("sweep-tee",
     "Heavy cotton white T-shirt on a 28 year-old Japanese woman, plain studio "
     "background, half-body shot, natural lighting, 4:5 portrait, photographic, "
     "premium minimalist styling. " + BRAND_SPEC),
    ("sweep-tee-classic",
     "Classic-fit lightweight T-shirt in black, worn by an Asian woman in her late 20s, "
     "front view, half-body, plain off-white studio background, soft natural light, "
     "4:5 portrait, photographic, minimalist styling. " + BRAND_SPEC),
    ("sweep-longsleeve",
     "Long-sleeve heavy cotton tee in faded olive, worn casually by a 30 year-old "
     "Japanese man, leaning against a concrete wall, side window light, half-body, "
     "4:5 portrait, magazine candid. " + BRAND_SPEC),
    ("sweep-sweatpants",
     "Tapered loop-back cotton sweatpants in heather grey, worn by an Asian woman "
     "from waist down, sitting on a wooden stool, plain backdrop, soft natural light, "
     "4:5 portrait, editorial product photography. " + BRAND_SPEC),
    ("sweep-cap",
     "Dark charcoal grey 5-panel strapback cap, low crown, flat brim, "
     "front view product shot floating on a plain concrete background, "
     "small embroidered SIIIEEP wordmark on front center, dramatic side light, "
     "4:5 portrait, no models, premium product photography. " + BRAND_SPEC),
    ("sweep-beanie",
     "Ribbed knit beanie in deep charcoal black, folded cuff, "
     "studio product shot on a plain concrete plinth, small woven SIIIEEP label "
     "on the cuff, 4:5 portrait, dramatic side light, no models, editorial product photography. " + BRAND_SPEC),
    ("sweep-tote",
     "Heavy raw canvas tote bag in natural off-white, large and structured, "
     "hanging from a black metal hook against a concrete wall, "
     "small black SIIIEEP wordmark screen-printed near the bottom hem, "
     "4:5 portrait, soft directional light, no models, magazine product still. " + BRAND_SPEC),
    ("sweep-socks-3pack",
     "Three pairs of crew athletic socks in white, black, and heather grey, "
     "stacked neatly on a black concrete surface, with the cuff showing a woven "
     "SIIIEEP wordmark in tonal thread, top-down 4:5 portrait, dramatic side light, "
     "no models, premium product photography. " + BRAND_SPEC),
    ("sweep-windbreaker",
     "Lightweight pull-over nylon windbreaker in matte black, worn by a 28 year-old "
     "Japanese man, looking down adjusting the hem, urban setting (concrete + glass), "
     "overcast natural light, 4:5 portrait, magazine candid. " + BRAND_SPEC),

    # ── 第二弾 (2026-05-11) ──
    ("sweep-tank-top",
     "Athletic tank top in solid black cotton, worn by an Asian woman in her late 20s, "
     "half-body front view, gym setting with concrete walls, natural side light, "
     "small embroidered SIIIEEP wordmark on left chest, 4:5 portrait, "
     "minimal styling, photographic. " + BRAND_SPEC),
    ("sweep-zip-hoodie",
     "Heavy black zip-up hoodie in cotton blend, worn open over a black tank by "
     "a 30 year-old Japanese man, leaning against a concrete wall, natural side light, "
     "small SIIIEEP wordmark embroidery on left chest, half-body 4:5 portrait, "
     "magazine candid styling. " + BRAND_SPEC),
    ("sweep-crewneck",
     "Heavy fleece crewneck sweatshirt in black, worn by a 28 year-old Asian man, "
     "front view, half-body, plain warm-toned studio background, natural soft light, "
     "small chest embroidery of SIIIEEP wordmark, 4:5 portrait, photographic. " + BRAND_SPEC),
    ("sweep-snapback",
     "Classic flat-brim snapback cap in solid black wool, front view product shot "
     "floating against a plain concrete background, large embroidered SIIIEEP wordmark "
     "in white thread on the front panel, dramatic side light, 4:5 portrait, "
     "no models, premium product photography. " + BRAND_SPEC),
    ("sweep-mug",
     "Black glossy ceramic coffee mug on a dark wood table, soft window light from "
     "the side, a small SIIIEEP wordmark sublimated in white on the side of the mug, "
     "steam rising faintly, 4:5 portrait, editorial product photography, no other text. " + BRAND_SPEC),
    ("sweep-bottle",
     "Matte black stainless steel water bottle, 17oz, standing upright on a concrete "
     "plinth against a dark backdrop, side label showing SIIIEEP wordmark in white, "
     "dramatic top-down light, 4:5 portrait, premium product photography. " + BRAND_SPEC),
    ("sweep-stickers",
     "A stack of sticker sheets featuring the SIIIEEP wordmark in various sizes and "
     "monochrome variants, laying flat on a black concrete surface, top-down 4:5 view, "
     "soft directional light, premium product still life, no other text. " + BRAND_SPEC),
    ("sweep-duffle",
     "All-over print black duffle bag with an abstract grid pattern of SIIIEEP wordmarks "
     "subtly tiled across the fabric, hanging from a metal rack against a concrete wall, "
     "4:5 portrait, soft directional light, no models, magazine product still. " + BRAND_SPEC),
    ("sweep-gym-bag",
     "All-over print black gym bag with a subtle tonal pattern of the SIIIEEP wordmark, "
     "sitting on a tatami floor next to folded BJJ gear, soft window light, "
     "4:5 portrait, no models, editorial product photography. " + BRAND_SPEC),
    ("sweep-cotton-shorts",
     "All-over print cotton shorts in solid black with a faint tonal SIIIEEP pattern "
     "across the fabric, worn casually by an Asian woman in her late 20s, "
     "lower-half shot only, plain warm studio background, natural soft light, "
     "4:5 portrait, photographic, minimalist styling. " + BRAND_SPEC),

    # ── 第三弾 (2026-05-12): バッグ / 帽子 / ジャケット / ケース ──
    ("sweep-bomber",
     "Editorial product photo of an all-over print bomber jacket, white satin shell with a "
     "subtle tonal pattern of the SIIIEEP wordmark repeating across the body, "
     "ribbed cuffs and hem in black, worn by a 30 year-old Japanese man on a "
     "Tokyo rooftop at dusk, hands in pockets, 3/4 view, 4:5 portrait, "
     "soft cinematic light, photographic, magazine candid. " + BRAND_SPEC),
    ("sweep-track-jacket",
     "Recycled poly track jacket in off-white with full-zip front and tonal SIIIEEP wordmark "
     "repeating subtly across the chest, raglan sleeves, worn by a 28 year-old "
     "Asian woman, half-body front view, plain warm studio background, "
     "natural soft light, 4:5 portrait, photographic, minimalist styling. " + BRAND_SPEC),
    ("sweep-backpack",
     "Premium product still life of an all-over print backpack, mid-size daypack with "
     "a tonal SIIIEEP wordmark pattern repeating across the white shell, "
     "standing upright on a polished concrete floor, dramatic side lighting, "
     "4:5 portrait, no models, editorial product photography. " + BRAND_SPEC),
    ("sweep-fanny-pack",
     "All-over print fanny pack in white with a subtle tonal SIIIEEP wordmark pattern, "
     "worn cross-body by a 27 year-old Japanese man over a charcoal hoodie, "
     "half-body candid in a Tokyo backstreet at golden hour, "
     "4:5 portrait, photographic, magazine candid. " + BRAND_SPEC),
    ("sweep-iphone-case",
     "Macro product shot of a clear iPhone 15 case with the SIIIEEP wordmark "
     "(s III equals-three-bars p logo) printed in matte black on the back panel, "
     "case lying flat on a brushed-aluminum desk next to a small notebook, "
     "soft directional light, 4:5 portrait, photographic, minimalist composition, no models. " + BRAND_SPEC),
    ("sweep-bucket-hat",
     "Editorial portrait of a 26 year-old Japanese woman wearing a black "
     "bucket hat with a small flat white-thread embroidered SIIIEEP wordmark on the front, "
     "half-body candid in soft natural daylight on a Tokyo street, "
     "4:5 portrait, photographic, magazine candid. " + BRAND_SPEC),
    ("sweep-joggers",
     "Recycled poly joggers in white with a subtle tonal SIIIEEP wordmark "
     "printed vertically down the right leg, worn by a 30 year-old Asian man, "
     "lower-half candid shot, plain warm studio background, natural soft light, "
     "4:5 portrait, photographic, minimalist styling. " + BRAND_SPEC),
    ("sweep-baseball-jersey",
     "Recycled poly baseball jersey, button-front in white with a subtle tonal "
     "SIIIEEP wordmark pattern repeating across the chest, classic ringer collar, "
     "worn by a 28 year-old Japanese man, half-body front view, plain warm studio "
     "background, natural soft light, 4:5 portrait, photographic, magazine candid. " + BRAND_SPEC),
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
