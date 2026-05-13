#!/usr/bin/env python3
"""
MU × JiuFlow — logo + product image generator.

Generates:
- JiuFlow brand logo (white wordmark on transparent background) for Printful prints
- MU × JiuFlow collab logo (combined wordmark) for collab SKUs
- Lifestyle / product mockup photos for each of the 8 JiuFlow SKUs

Uploads to R2 bucket wearmu-lifestyle, path jiuflow/<filename>,
served at https://lifestyle.wearmu.com/jiuflow/<filename>.

Then PATCHes /api/admin/collab_image to set image_url on collab_products rows.
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

# JiuFlow brand spec
# Palette: 黒 (#0A0A0A) × 白 (#F5F5F0) × ロイヤルブルー (#1E40AF, BJJ belt blue)
# Vibe: 柔術の流れ + minimalist editorial
BRAND_SPEC = (
    "Brand: minimal 'JiuFlow' wordmark only (uppercase modern sans-serif, "
    "Helvetica Now / Inter style, tight kerning, single-line) in clean white color. "
    "Black or neutral product surfaces dominate. Subtle royal blue (#1E40AF) accents allowed. "
    "Premium athletic / BJJ academy aesthetic. No characters, no other text, no graphics."
)

# Logo file generation prompts (PNG with transparent or solid background)
LOGO_PROMPTS = [
    ("_logo",
     "Brand logo lockup: clean white 'JiuFlow' wordmark on pure black background, "
     "tight kerning, modern sans-serif uppercase. Centered, minimum 30% padding around. "
     "No other elements. Square aspect ratio. Print-ready high resolution. "
     "Subtle thin royal blue (#1E40AF) underline below the wordmark, half the width of 'JiuFlow'.",
     "logo"),
    ("_collab_v1",
     "Co-branding logo lockup: white wordmark 'MU × JiuFlow' on pure black background, "
     "'MU' on the left, '×' centered, 'JiuFlow' on the right, all single line, modern "
     "sans-serif uppercase, tight kerning, clean spacing around the '×'. Minimum 30% "
     "padding. Print-ready high resolution. Centered, square aspect ratio.",
     "logo"),
]

# Product mockup prompts — 8 SKUs
PRODUCT_PROMPTS = [
    ("jiuflow-rashguard-ls",
     "Editorial product photo, black long-sleeve compression rashguard with all-over print, "
     "subtle white 'MU × JiuFlow' wordmark across upper chest, royal blue (#1E40AF) "
     "thin accent line along the side seams, worn by a 30-year-old Japanese BJJ purple-belt "
     "athlete preparing on the edge of a tatami mat in a clean Tokyo academy. "
     "Soft natural daylight from large window. Editorial framing. Sharp focus on garment. "),
    ("jiuflow-fight-shorts",
     "Editorial product photo, black athletic long shorts with all-over print, royal blue "
     "(#1E40AF) side stripes, small white 'MU × JiuFlow' wordmark on right leg. "
     "Worn by a Japanese BJJ athlete sitting on the edge of a tatami mat, focused expression, "
     "Gi top removed and folded beside. Clean Tokyo academy interior with white walls. "
     "Soft natural daylight. Editorial product photography."),
    ("jiuflow-tee-classic",
     "Editorial product photo, premium heavy cotton black tee with crisp white 'MU × JiuFlow' "
     "wordmark printed across the chest. Worn by a 30-year-old Japanese BJJ practitioner "
     "outside a Tokyo academy in casual setting, post-training glow, holding a folded Gi. "
     "Sharp focus on garment, soft daylight. Editorial product photography."),
    ("jiuflow-hoodie-fleece",
     "Editorial product photo, premium heavy black fleece pullover hoodie, white "
     "'MU × JiuFlow' wordmark across chest, 'ロール. フロー. リセット.' (Roll Flow Reset) "
     "subtle small text on left sleeve. Worn by a Japanese BJJ practitioner with hood up, "
     "entering a Tokyo academy in cold winter morning. Editorial natural light."),
    ("jiuflow-cap-snapback",
     "Editorial product photo, black flat-brim snapback hat with white embroidered "
     "'JiuFlow' wordmark on front panel, royal blue (#1E40AF) embroidered underline. "
     "Displayed on a clean white pedestal with soft top-down lighting. Studio product "
     "photography, sharp focus, minimalist composition."),
    ("jiuflow-bottle-water",
     "Editorial product photo, white stainless-steel 17oz water bottle with sleek "
     "matte finish, white 'JiuFlow' wordmark and 'Hydrate Like You Train' subtext "
     "printed in royal blue (#1E40AF). Standing on a tatami mat next to a folded "
     "BJJ Gi and a rolled black belt. Soft natural daylight, editorial product framing."),
    ("jiuflow-stickers",
     "Editorial product photo, flat-lay of a kiss-cut sticker sheet, 5.83 x 8.27 inch white sheet. "
     "Contains: 'JiuFlow' wordmark center top, 8 colored belt-grade circles (white through "
     "black gradient), 3 BJJ technique-name stickers ('Triangle', 'Armbar', 'Omoplata') in royal blue. "
     "Photographed top-down on dark gray slate background, soft natural daylight."),
    ("jiuflow-towel-gym",
     "Editorial product photo, white quick-dry microfiber gym towel (15x30 inch), "
     "draped over a tatami mat edge, 'JiuFlow' wordmark sublimated in royal blue (#1E40AF) "
     "across the center, thin royal blue piping along the edges. A folded BJJ Gi visible "
     "in background. Soft natural daylight, editorial product photography."),

    # ── 第二弾追加 12 SKU ──
    ("jiuflow-tank-top",
     "Editorial product photo, black heavyweight cotton tank top, worn by a 28-year-old "
     "Japanese male BJJ practitioner with athletic build, post-training in a Tokyo academy. "
     "'JiuFlow' wordmark in crisp white printed across the chest. He's resting Gi-less on a "
     "tatami mat edge, sweat visible. Soft natural light from window, sharp focus on garment."),
    ("jiuflow-zip-hoodie",
     "Editorial product photo, premium black full-zip cotton hoodie, half-unzipped, worn "
     "by a Japanese BJJ practitioner outside a Tokyo academy. Small 'JiuFlow' wordmark on "
     "left chest in white, large logo print on the back visible in side angle. Soft "
     "natural daylight, editorial product photography."),
    ("jiuflow-longsleeve",
     "Editorial product photo, premium black long-sleeve cotton tee, worn by a Japanese "
     "BJJ practitioner sitting on the edge of a Tokyo academy tatami. 'JiuFlow' wordmark "
     "printed in clean white across the chest. Layered look — folded Gi top beside him. "
     "Soft natural light from large window, editorial framing."),
    ("jiuflow-sweatpants",
     "Editorial product photo, premium black heavyweight sweatpants, displayed on a "
     "tatami mat with a folded BJJ Gi and a black belt. Small 'JiuFlow' wordmark in "
     "white printed on the side thigh. Soft natural daylight, top-down product framing, "
     "editorial style."),
    ("jiuflow-joggers",
     "Editorial product photo, slim tapered black joggers, worn by a 30-year-old Japanese "
     "BJJ practitioner walking out of a Tokyo academy with a Gi bag over the shoulder. "
     "Side panel: small 'JiuFlow' wordmark in white. Casual editorial street style, sharp "
     "focus on the joggers and the worn-look academy interior in background."),
    ("jiuflow-cap-dad",
     "Editorial product photo, navy blue dad-style baseball cap, displayed on a clean "
     "white pedestal with soft top-down lighting. Front panel: 'JiuFlow' wordmark "
     "embroidered in clean white thread, with a small royal blue (#1E40AF) underline. "
     "Studio product photography, sharp focus on the embroidery."),
    ("jiuflow-beanie",
     "Editorial product photo, black cuffed beanie, worn by a Japanese BJJ practitioner "
     "entering a Tokyo academy in cold winter morning, breath visible in the cold air. "
     "Front: 'JiuFlow' wordmark embroidered in white thread. Editorial natural light, "
     "warm winter mood, sharp focus on the beanie."),
    ("jiuflow-tote",
     "Editorial product photo, large black heavy canvas tote bag (16oz), shown with a "
     "folded BJJ Gi inside and a rolled white belt hanging out the top. 'JiuFlow' "
     "wordmark printed in white on the front of the bag. Placed on a tatami floor next "
     "to a wooden academy door. Soft natural daylight, editorial framing."),
    ("jiuflow-duffle",
     "Editorial product photo, large black gym duffle bag (canvas, water-resistant), "
     "packed with rolled BJJ Gi, training shorts, and a folded towel visible through "
     "the open zipper. Side: 'JiuFlow' wordmark in white print with a royal blue "
     "(#1E40AF) accent stripe along the top. Photographed in a Tokyo academy entrance, "
     "soft natural daylight, premium gym bag editorial style."),
    ("jiuflow-mug",
     "Editorial product photo, white glossy ceramic mug (11oz) on a wooden table in a "
     "morning kitchen scene. Surface: 'JiuFlow' wordmark in clean black with 'Daily Roll' "
     "subtext in royal blue (#1E40AF). Steam rising from coffee. A small notebook and a "
     "rolled BJJ belt in soft background bokeh. Editorial product photography, warm morning light."),
    ("jiuflow-iphone-case",
     "Editorial product photo, black matte iPhone case (showing iPhone 15 Pro silhouette). "
     "Back: white 'JiuFlow' wordmark with a thin royal blue (#1E40AF) horizontal line "
     "below it. Photographed on a clean dark slate surface with soft top-down lighting. "
     "Premium accessory product photography, sharp focus on the case."),
    ("jiuflow-laptop-sleeve",
     "Editorial product photo, black canvas laptop sleeve (13-inch), placed on a wooden "
     "desk next to a Macbook, a BJJ Gi folded in the corner of frame, and a black belt "
     "rolled up on top. Front: white 'JiuFlow' wordmark with a small royal blue (#1E40AF) "
     "accent corner. Soft natural daylight from desk window, editorial WFH-BJJ-life vibe."),
]


def generate(slug: str, prompt: str, kind: str = "lifestyle") -> bytes:
    client = genai.Client(api_key=GEMINI_API_KEY)
    full_prompt = f"{prompt}\n\n{BRAND_SPEC}"
    print(f"  Gemini[{GEMINI_MODEL}]: {slug} ({kind})...")
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


def upload_to_r2(filename: str, img_bytes: bytes, content_type: str = "image/jpeg") -> str:
    suffix = ".png" if content_type == "image/png" else ".jpg"
    with tempfile.NamedTemporaryFile(suffix=suffix, delete=False) as f:
        img = Image.open(io.BytesIO(img_bytes))
        if content_type == "image/png":
            img.save(f.name, format="PNG", optimize=True)
        else:
            img = img.convert("RGB")
            img.save(f.name, format="JPEG", quality=88, optimize=True)
        tmp = f.name
    try:
        result = subprocess.run(
            [WRANGLER_BIN, "r2", "object", "put",
             f"{R2_BUCKET}/jiuflow/{filename}",
             f"--file={tmp}",
             "--remote",
             f"--content-type={content_type}"],
            capture_output=True, text=True, timeout=90,
        )
        if result.returncode != 0:
            raise RuntimeError(f"wrangler: {result.stderr[-400:]}")
        return f"https://{PUBLIC_HOST}/jiuflow/{filename}"
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
    mode = sys.argv[1] if len(sys.argv) > 1 else "all"

    if mode in ("all", "logos"):
        print(f"=== Generating brand logos ===")
        for filename, prompt, _ in LOGO_PROMPTS:
            try:
                img_bytes = generate(filename, prompt, kind="logo")
                # logos as PNG for transparency support
                url = upload_to_r2(f"{filename}.png", img_bytes, content_type="image/png")
                print(f"  → {url}")
            except Exception as e:
                print(f"  ! {filename} failed: {e}", file=sys.stderr)

    if mode in ("all", "products"):
        print(f"\n=== Generating product mockups ===")
        for slug, prompt in PRODUCT_PROMPTS:
            try:
                img_bytes = generate(slug, prompt, kind="lifestyle")
                url = upload_to_r2(f"{slug}.jpg", img_bytes, content_type="image/jpeg")
                print(f"  → {url}")
                patch_db(slug, url)
            except Exception as e:
                print(f"  ! {slug} failed: {e}", file=sys.stderr)


if __name__ == "__main__":
    main()
