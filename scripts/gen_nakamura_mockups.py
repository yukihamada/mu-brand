#!/usr/bin/env python3
"""Generate MU × Nakamura Brothers product mockups end-to-end.

Pipeline:
  1. Generate the brand design (gold "中"/"道" on transparent) using Gemini
     3 Pro image-preview (nano banana). Save to
     store/static/nakamura/_logo_v1.png.
  2. For each Nakamura SKU in the seed (8 items), call Printful's mockup-
     generator with the design URL, get back a presigned mockup URL, fetch
     the PNG, save to store/static/nakamura/<slug>.jpg.
  3. Write a Rust source patch hint (printed) telling main.rs to set
     image_url = /static/nakamura/<slug>.jpg in the seed.

Run:
  source /Users/yuki/.env
  python3 scripts/gen_nakamura_mockups.py

ENV required: GEMINI_API_KEY, PRINTFUL_API_KEY
"""
import os, sys, json, time, base64, io
from pathlib import Path
from urllib.request import Request, urlopen
from urllib.error import HTTPError

ROOT = Path(__file__).resolve().parent.parent
OUT_DIR = ROOT / "store" / "static" / "nakamura"
OUT_DIR.mkdir(parents=True, exist_ok=True)

# ─── Design generation via Gemini (nano banana) ─────────────────────────
GEMINI_MODEL = "gemini-3-pro-image-preview"
GEMINI_URL = f"https://generativelanguage.googleapis.com/v1beta/models/{GEMINI_MODEL}:generateContent"

DESIGN_PROMPT = """A single, bold Japanese kanji character "中" (naka) rendered in
elegant brush-stroke calligraphy. Pure metallic gold color (#FFD700) with
subtle inner glow and crisp edges. Transparent background. Centered in a
2940×2940 square print bed (DTG-ready). Style: minimal, traditional Japanese
calligraphy meets modern luxury streetwear. No outline, no shadow, no
additional elements. Just the gold character, photograph-quality detail,
print-ready resolution. The character should feel weighty and confident."""

def gemini_generate(prompt: str, out_path: Path):
    key = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
    if not key:
        sys.exit("GEMINI_API_KEY missing — source /Users/yuki/.env first")
    body = {
        "contents": [{"role": "user", "parts": [{"text": prompt}]}],
        "generationConfig": {"responseModalities": ["IMAGE", "TEXT"]},
    }
    req = Request(
        f"{GEMINI_URL}?key={key}",
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urlopen(req, timeout=120) as r:
            j = json.load(r)
    except HTTPError as e:
        sys.exit(f"Gemini error: {e.code} {e.read().decode()[:200]}")
    # Walk parts looking for inline image data.
    for cand in j.get("candidates", []):
        for part in cand.get("content", {}).get("parts", []):
            data = part.get("inlineData") or part.get("inline_data")
            if data and "data" in data:
                png = base64.b64decode(data["data"])
                out_path.write_bytes(png)
                print(f"  ✓ saved {out_path} ({len(png)} bytes)")
                return out_path
    sys.exit(f"Gemini response had no image: {json.dumps(j)[:300]}")


# ─── Printful mockup-generator ───────────────────────────────────────────
PRINTFUL_BASE = "https://api.printful.com"

# Match the seed in main.rs:
#   slug, printful_product_id, printful_variant_id, placement-type
NAKAMURA_SKUS = [
    # (slug, product_id, variant_id, placement)
    ("nakamura-camp-tee",          71,  4011, "front"),
    ("nakamura-dojo-hoodie",       146, 5530, "front"),
    # tenugui is hand-towel — use generic tee mockup as placeholder (real
    # hand-towel is hand-printed by Kyoto atelier, no Printful product fit)
    ("nakamura-brothers-tenugui",  71,  4011, "front"),
    ("nakamura-michi-cap",         140, 5277, "embroidery_front"),
    # crewneck = Gildan 18000, variant 5435 (Black / M) — verified live
    ("nakamura-recovery-crewneck", 145, 5435, "front"),
    # tote = All-Over Tote product 84, variant 4533 (Black) — verified live
    ("nakamura-dojo-tote",         84,  4533, "default"),
    # stickers — use Kiss-cut sticker product 358 variant 10164
    ("nakamura-sticker-pack",      358, 10164, "default"),
    # family-teabowl is pre-order, no Printful variant
]

def printful_post(path, body):
    key = os.environ["PRINTFUL_API_KEY"]
    req = Request(
        f"{PRINTFUL_BASE}{path}",
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {key}"},
        method="POST",
    )
    try:
        with urlopen(req, timeout=60) as r:
            return json.load(r)
    except HTTPError as e:
        print(f"  ✗ Printful {path} → {e.code}: {e.read().decode()[:300]}")
        return None

def printful_get(path):
    key = os.environ["PRINTFUL_API_KEY"]
    req = Request(
        f"{PRINTFUL_BASE}{path}",
        headers={"Authorization": f"Bearer {key}"},
    )
    try:
        with urlopen(req, timeout=60) as r:
            return json.load(r)
    except HTTPError as e:
        return None

def generate_printful_mockup(slug, product_id, variant_id, placement, design_url):
    """Returns the mockup PNG URL (presigned, ~24h expiry)."""
    # Position spec — same logic as printful_mockup_config_for in main.rs.
    if "hoodie" in slug or "crewneck" in slug:
        position = {"area_width": 1800, "area_height": 2400,
                    "width": 1100, "height": 1100, "top": 560, "left": 350}
    else:
        position = {"area_width": 1800, "area_height": 2400,
                    "width": 1260, "height": 1260, "top": 380, "left": 270}

    body = {
        "variant_ids": [variant_id],
        "format": "png",
        "files": [{
            "placement": placement,
            "image_url": design_url,
            "position": position,
        }],
    }
    print(f"  → creating mockup task for {slug}…", end=" ", flush=True)
    res = printful_post(f"/mockup-generator/create-task/{product_id}", body)
    if not res:
        return None
    task_key = res.get("result", {}).get("task_key")
    if not task_key:
        print(f"no task_key in {res}")
        return None

    for attempt in range(30):
        time.sleep(4 if attempt > 0 else 2)
        poll = printful_get(f"/mockup-generator/task?task_key={task_key}")
        if not poll:
            continue
        status = poll.get("result", {}).get("status")
        if status == "completed":
            mockups = poll["result"].get("mockups", [])
            if mockups:
                url = mockups[0].get("mockup_url")
                print(f"done")
                return url
        if status == "failed":
            print(f"failed")
            return None
    print(f"timeout")
    return None

def download(url, out_path):
    req = Request(url)
    with urlopen(req, timeout=60) as r:
        data = r.read()
    out_path.write_bytes(data)
    print(f"    ✓ saved {out_path} ({len(data)} bytes)")


# ─── Main ───────────────────────────────────────────────────────────────
def main():
    print("== MU × Nakamura mockup pipeline ==")
    print()
    print("Step 1: Generate brand design via Gemini (nano banana)")
    design_path = OUT_DIR / "_logo_v1.png"
    if design_path.exists() and design_path.stat().st_size > 1000:
        print(f"  ⏩ {design_path} already exists ({design_path.stat().st_size} bytes), skip")
    else:
        gemini_generate(DESIGN_PROMPT, design_path)

    # We need a PUBLIC HTTPS URL for Printful to fetch.
    # In production this will be served at https://wearmu.com/static/nakamura/_logo_v1.png
    # — but we need to commit + push first before that's live. So we use the
    # existing kokon logo as a fallback for the design URL during initial
    # generation, then swap to the nakamura logo on next run.
    public_design_url = "https://wearmu.com/static/nakamura/_logo_v1.png"
    # Probe whether the URL is live yet:
    try:
        with urlopen(Request(public_design_url, method="HEAD"), timeout=10) as r:
            if r.status == 200:
                print(f"  ✓ design URL live: {public_design_url}")
            else:
                public_design_url = "https://lifestyle.wearmu.com/kokon/_logo_v2.png"
                print(f"  ⚠ design URL not live yet, falling back to {public_design_url}")
    except Exception:
        public_design_url = "https://lifestyle.wearmu.com/kokon/_logo_v2.png"
        print(f"  ⚠ design URL not live yet, falling back to {public_design_url}")
        print(f"     → commit + push, then re-run this script to use the real Nakamura logo.")

    print()
    print(f"Step 2: Generate Printful mockups (design = {public_design_url})")
    results = {}
    for slug, product_id, variant_id, placement in NAKAMURA_SKUS:
        url = generate_printful_mockup(slug, product_id, variant_id, placement, public_design_url)
        if url:
            out_path = OUT_DIR / f"{slug}.jpg"
            try:
                download(url, out_path)
                results[slug] = f"/static/nakamura/{slug}.jpg"
            except Exception as e:
                print(f"    ✗ download failed: {e}")

    print()
    print("Step 3: Image URL mapping (commit to main.rs):")
    print()
    for slug, path in results.items():
        print(f'  ("{slug}", "https://wearmu.com{path}"),')

    # Save mapping JSON for the Rust seed to read.
    mapping = OUT_DIR / "_mockup_map.json"
    mapping.write_text(json.dumps(results, indent=2, ensure_ascii=False))
    print(f"\n  ✓ mapping saved to {mapping}")
    print()
    print(f"Generated {len(results)}/{len(NAKAMURA_SKUS)} mockups")

if __name__ == "__main__":
    main()
