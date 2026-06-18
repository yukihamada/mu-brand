#!/usr/bin/env python3
"""
MU Brand — Lifestyle Photo Generator

For each MUGEN drop, generates a "person wearing this design" lifestyle
image via Gemini 3 Pro Image (image-to-image). Uploads to R2 at
  lifestyle.wearmu.com/<product_id>.jpg
and PATCHes products.lifestyle_url through /api/admin/lifestyle.

Usage:
  python generate_lifestyle.py 6                # generate 6 lifestyle photos
  python generate_lifestyle.py <product_id>     # regenerate one
"""
import os, sys, io, base64, sqlite3, tempfile, subprocess, requests, hashlib, random
from pathlib import Path
from PIL import Image

os.environ.pop("GOOGLE_API_KEY", None)  # expired key takes precedence otherwise
from google import genai
from google.genai import types

GEMINI_API_KEY = os.environ["GEMINI_API_KEY"]
DB_PATH        = Path(__file__).parent / "products.db"
DESIGNS_DIR    = Path(__file__).parent / "designs"
GEMINI_MODEL   = "gemini-3-pro-image-preview"
STORE_URL      = os.environ.get("MU_STORE_URL", "https://wearmu.com")
ADMIN_TOKEN    = os.environ.get("MU_ADMIN_TOKEN", "mu-admin-2026")
WRANGLER_BIN   = os.environ.get("WRANGLER_BIN", "/opt/homebrew/bin/wrangler")
R2_BUCKET      = "wearmu-lifestyle"
PUBLIC_HOST    = "lifestyle.wearmu.com"

# Diverse model directions so the gallery doesn't look like one person clones.
SCENES = [
    ("editorial half-body portrait of a 24 year-old Japanese woman in soft natural light in a Hokkaido cafe, wearing the t-shirt", "Yuna"),
    ("street photo of a 31 year-old Japanese man in front of a Fukuoka concrete wall at golden hour, wearing the t-shirt, candid", "Ren"),
    ("seaside shot in Kamakura, 28 year-old woman walking on the beach at sunrise, wearing the t-shirt, soft fog", "Emi"),
    ("forest cabin morning, 45 year-old Japanese man with a coffee, wearing the t-shirt, wood textures", "Kazu"),
    ("Kyoto alley night with paper lanterns, 22 year-old woman with friends, wearing the t-shirt, film grain", "Mio"),
    ("minimalist Tokyo apartment, 27 year-old man on a chair near window, wearing the t-shirt, monochrome mood", "Haruto"),
    ("gallery interior in Sendai, 33 year-old woman against a white wall, wearing the t-shirt, museum lighting", "Aoi"),
    ("night car interior, 38 year-old Japanese man driver, wearing the t-shirt, dashboard glow, Osaka", "Taka"),
    ("Okinawa skate park sunset, 19 year-old with a board, wearing the t-shirt, palm shadows", "Sora"),
    ("Nagano farmhouse renovation, 41 year-old in a workshop, wearing the t-shirt, wood shavings", "Nao"),
    ("rainy Kanazawa morning, 35 year-old in a teahouse doorway, wearing the t-shirt, gentle rain on stone", "Rui"),
    ("Kobe pier at dusk, 29 year-old after a run, wearing the t-shirt, breath visible in cool air", "Mika"),
    ("Yokohama bayside afternoon, 52 year-old with a camera, wearing the t-shirt, warm reflective tones", "Jun"),
    ("Berlin bookstore window seat, 26 year-old Japanese-German woman, wearing the t-shirt, late espresso", "Nina"),
    ("Naha night market, 30 year-old playing sanshin to friends, wearing the t-shirt, lantern bokeh", "Io"),
]

PROMPT_TEMPLATE = """
A natural, photographic editorial fashion lifestyle shot.
{scene}.
The t-shirt features the exact graphic shown in the reference image — preserve the design as-is, centered on the chest.
Casual, candid, not posed. Soft natural light. Cinematic but not over-stylized.
Plain white-T base with the graphic printed. No logos, no text other than what's in the graphic.
Shot on 50mm prime, slight grain.
Output: a single photograph, 9:16 vertical or 4:5 portrait. No collage. No text overlays.
""".strip()


def db():
    con = sqlite3.connect(DB_PATH)
    con.row_factory = sqlite3.Row
    return con


def pick_targets(n: int):
    """Choose n MUGEN drops to lifestyle-photo. Skip ones already done."""
    con = db()
    rows = con.execute(
        """SELECT p.id, p.brand, p.drop_num, p.name, p.prompt_hash
           FROM products p
           WHERE p.brand='mugen' AND p.active=1
             AND (p.sold IS NULL OR p.sold < p.inventory)
             AND (p.lifestyle_url IS NULL OR p.lifestyle_url = '')
           ORDER BY p.drop_num DESC LIMIT ?""",
        (n * 3,),
    ).fetchall()
    con.close()
    random.shuffle(rows)
    return rows[:n]


def find_design_png(brand: str, drop_num: int, prompt_hash: str) -> Path | None:
    if not prompt_hash:
        return None
    cand = DESIGNS_DIR / f"{brand}_{drop_num:04d}_{prompt_hash[:8]}.png"
    if cand.exists():
        return cand
    # Look for any file matching the hash prefix
    for p in DESIGNS_DIR.glob(f"{brand}_{drop_num:04d}_*.png"):
        return p
    return None


def gen_lifestyle(design_png_path: Path, scene_prompt: str) -> bytes:
    """Pass the design image as a reference + a lifestyle prompt to Gemini.
    Returns generated image bytes (PNG/JPEG)."""
    client = genai.Client(api_key=GEMINI_API_KEY)
    img_bytes = design_png_path.read_bytes()
    # Pass image as inline_data (base64-encoded) so Gemini uses it as visual reference
    content_parts = [
        types.Part.from_bytes(data=img_bytes, mime_type="image/png"),
        types.Part.from_text(text=PROMPT_TEMPLATE.format(scene=scene_prompt)),
    ]
    response = client.models.generate_content(
        model=GEMINI_MODEL,
        contents=[types.Content(role="user", parts=content_parts)],
        config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"]),
    )
    for part in response.candidates[0].content.parts:
        if hasattr(part, "inline_data") and part.inline_data:
            data = part.inline_data.data
            if isinstance(data, str):
                return base64.b64decode(data)
            return data
    raise RuntimeError("Gemini returned no image")


def upload_to_r2(product_id: int, jpg_bytes: bytes) -> str:
    """Upload to R2 bucket wearmu-lifestyle and return the public URL."""
    with tempfile.NamedTemporaryFile(suffix=".jpg", delete=False) as f:
        # Ensure we serve JPEG (smaller, faster)
        img = Image.open(io.BytesIO(jpg_bytes)).convert("RGB")
        img.save(f.name, format="JPEG", quality=88, optimize=True)
        tmp = f.name
    try:
        result = subprocess.run(
            [
                WRANGLER_BIN, "r2", "object", "put",
                f"{R2_BUCKET}/{product_id}.jpg",
                f"--file={tmp}",
                "--remote",
                "--content-type=image/jpeg",
            ],
            capture_output=True, text=True, timeout=90,
        )
        if result.returncode != 0:
            raise RuntimeError(f"wrangler: {result.stderr[-400:]}")
        return f"https://{PUBLIC_HOST}/{product_id}.jpg"
    finally:
        try: os.unlink(tmp)
        except: pass


def patch_db(product_id: int, lifestyle_url: str):
    r = requests.patch(
        f"{STORE_URL}/api/admin/lifestyle?token={ADMIN_TOKEN}",
        json={"product_id": product_id, "lifestyle_url": lifestyle_url},
        timeout=20,
    )
    print(f"  PATCH /api/admin/lifestyle → {r.status_code} {r.text[:140]}")


def process_one(row, scene_idx: int):
    pid = row["id"]
    drop = row["drop_num"]
    prompt_hash = row["prompt_hash"] or ""
    print(f"\n[#{drop} id={pid}] lifestyle generate")
    design_path = find_design_png("mugen", drop, prompt_hash)
    if not design_path:
        print("  no local design png; skip")
        return False
    scene, persona = SCENES[scene_idx % len(SCENES)]
    print(f"  scene: {persona} ({scene[:70]}…)")
    try:
        out = gen_lifestyle(design_path, scene)
        url = upload_to_r2(pid, out)
        patch_db(pid, url)
        print(f"  ✓ {url}")
        return True
    except Exception as e:
        print(f"  ✗ {type(e).__name__}: {e}")
        return False


def main():
    if not DB_PATH.exists():
        print(f"products.db not found at {DB_PATH}")
        sys.exit(1)

    arg = sys.argv[1] if len(sys.argv) > 1 else "6"
    if arg.isdigit() and int(arg) <= 100:
        n = int(arg)
        rows = pick_targets(n)
    else:
        # Treat as product_id
        con = db()
        rows = [con.execute("SELECT id, brand, drop_num, name, prompt_hash FROM products WHERE id=?", (int(arg),)).fetchone()]
        con.close()
        rows = [r for r in rows if r is not None]

    print(f"Generating {len(rows)} lifestyle photos…")
    ok = 0
    for i, r in enumerate(rows):
        if process_one(r, i):
            ok += 1
    print(f"\nDone — {ok}/{len(rows)} succeeded.")


if __name__ == "__main__":
    main()
