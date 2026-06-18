#!/usr/bin/env python3
"""Add a single Nakamura SKU end-to-end.

Each invocation:
  1. Calls Printful mockup-generator with the "中" gold kanji at 30pt
     chest-patch placement (small, premium brand-badge style).
  2. Saves the resulting mockup to store/static/nakamura/<slug>.jpg.
  3. Prints the Rust seed line + image_map line you paste into
     store/src/main.rs.

Usage:
  set -a; source /Users/yuki/.env; set +a
  python3 scripts/add_nakamura_sku.py \\
      --slug nakamura-polo \\
      --product 181 --variant 6483 \\
      --cat "ポロ / Polo" \\
      --name "MU × NAKAMURA Black Polo" \\
      --desc "胸に小さく金色「中」刺繍。トレーニング外の装い。" \\
      --price 9800 \\
      --placement embroidery_chest_left

Or batch via STDIN (TSV):
  cat skus.tsv | python3 scripts/add_nakamura_sku.py --batch

ENV: PRINTFUL_API_KEY

Design: scripts/gen_nakamura_logo.py renders the source kanji once.
Placement size: small chest patch (~30pt @ 300 DPI ≈ 600x600 on the
1800x2400 Printful print bed).
"""
import argparse, os, sys, json, time
from pathlib import Path
from urllib.request import Request, urlopen
from urllib.error import HTTPError

ROOT = Path(__file__).resolve().parent.parent
OUT_DIR = ROOT / "store" / "static" / "nakamura"
OUT_DIR.mkdir(parents=True, exist_ok=True)

DESIGN_URL = "https://wearmu.com/static/nakamura/_logo_v2.png"

# 30pt @ 300 DPI ≈ 125px native, but for visible-but-elegant chest-patch
# we want ~5cm = 2" ≈ 600px on the 1800x2400 print bed. Position is
# left-chest, slightly above mid (classic polo logo placement).
CHEST_PATCH = {
    "area_width": 1800, "area_height": 2400,
    "width": 600, "height": 600,
    "top": 380, "left": 1000,  # right-chest (more natural for embroidery)
}

# Front-center (larger) for tees / totes / bags
FRONT_CENTER = {
    "area_width": 1800, "area_height": 2400,
    "width": 900, "height": 900,
    "top": 600, "left": 450,
}

# Hat / cap front (smaller)
CAP_FRONT = {
    "area_width": 2000, "area_height": 880,
    "width": 600, "height": 600,
    "top": 140, "left": 700,
}

PLACEMENT_SIZES = {
    "embroidery_chest_left":  CHEST_PATCH,
    "embroidery_chest_right": CHEST_PATCH,
    "embroidery_front":       CAP_FRONT,
    "embroidery_front_large": CAP_FRONT,
    "front":                  FRONT_CENTER,
    "default":                FRONT_CENTER,
}


# ───────── Printful ─────────
PRINTFUL_BASE = "https://api.printful.com"
def pf_post(path, body):
    key = os.environ["PRINTFUL_API_KEY"]
    req = Request(
        f"{PRINTFUL_BASE}{path}",
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {key}"},
    )
    try:
        with urlopen(req, timeout=60) as r:
            return json.load(r)
    except HTTPError as e:
        print(f"  ✗ {path} → {e.code}: {e.read().decode()[:300]}")
        return None

def pf_get(path):
    key = os.environ["PRINTFUL_API_KEY"]
    req = Request(f"{PRINTFUL_BASE}{path}",
                  headers={"Authorization": f"Bearer {key}"})
    try:
        with urlopen(req, timeout=60) as r:
            return json.load(r)
    except HTTPError:
        return None

def make_mockup(slug, product_id, variant_id, placement, design_url):
    position = PLACEMENT_SIZES.get(placement, CHEST_PATCH)
    body = {
        "variant_ids": [variant_id],
        "format": "png",
        "files": [{
            "placement": placement,
            "image_url": design_url,
            "position": position,
        }],
    }
    print(f"  → mockup task for {slug} (placement={placement})…", end=" ", flush=True)
    res = pf_post(f"/mockup-generator/create-task/{product_id}", body)
    if not res:
        return None
    task = res.get("result", {}).get("task_key")
    if not task:
        print(f"no task_key")
        return None
    for attempt in range(30):
        time.sleep(4 if attempt > 0 else 2)
        poll = pf_get(f"/mockup-generator/task?task_key={task}")
        if not poll:
            continue
        status = poll.get("result", {}).get("status")
        if status == "completed":
            mu = poll["result"].get("mockups", [])
            if mu:
                url = mu[0].get("mockup_url")
                print("done")
                return url
        if status == "failed":
            print(f"failed: {json.dumps(poll)[:200]}")
            return None
    print("timeout")
    return None

def download(url, out_path):
    req = Request(url)
    with urlopen(req, timeout=60) as r:
        data = r.read()
    out_path.write_bytes(data)


# ───────── SQL helpers ─────────
def emit_seed_line(slug, cat, name, desc, price, product_id, variant_id, placement, sizes=None):
    """Print a Rust tuple matching the nakamura_items shape in main.rs."""
    files_json = json.dumps([{
        "type": placement,
        "url": "https://wearmu.com/static/nakamura/_logo_v2.png",
    }])
    # variant_map: most products are M=variant_id; we pass through.
    var_map = json.dumps({"M": variant_id, "OS": variant_id, "ONE SIZE": variant_id,
                          "S": variant_id, "L": variant_id, "XL": variant_id})
    sizes_csv = sizes or 'XS,S,M,L,XL,2XL,OS'
    sizes_arr = '\",\"'.join(sizes_csv.split(','))

    print()
    print("─── Rust seed line (paste into nakamura_items in main.rs) ───")
    rust = (
        f'        ("{slug}", "{cat}",\n'
        f'         "{name}",\n'
        f'         "{desc}",\n'
        f'         {price}, "printful", Some({product_id}), Some({variant_id}),\n'
        f'         Some(r#"{var_map}"#),\n'
        f'         Some(r#"{files_json}"#),\n'
        f'         Some(r##"[{{"id":"thread_colors","value":["#FFD700"]}}]"##), 14, 1),'
    )
    print(rust)
    print()
    print("─── image_map line (paste into nakamura_image_map in main.rs) ───")
    print(f'        ("{slug}",  "/static/nakamura/{slug}.jpg"),')
    print()


# ───────── CLI ─────────
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--slug", required=True)
    ap.add_argument("--product", type=int, required=True, help="Printful product id")
    ap.add_argument("--variant", type=int, required=True, help="Printful variant id")
    ap.add_argument("--name", required=True, help="Public product name")
    ap.add_argument("--cat", default="", help="Category label (Japanese)")
    ap.add_argument("--desc", default="", help="Description")
    ap.add_argument("--price", type=int, required=True, help="JPY price")
    ap.add_argument("--placement", default="embroidery_chest_left",
                    choices=list(PLACEMENT_SIZES.keys()))
    ap.add_argument("--sizes", default="XS,S,M,L,XL,2XL,OS")
    args = ap.parse_args()

    if "PRINTFUL_API_KEY" not in os.environ:
        sys.exit("PRINTFUL_API_KEY missing — `set -a; source /Users/yuki/.env; set +a`")

    print(f"== Adding SKU: {args.slug} ==")
    print(f"  product={args.product} variant={args.variant} placement={args.placement}")
    print(f"  price=¥{args.price:,}  size policy: 30pt chest-patch (600x600 on print bed)")
    print()

    url = make_mockup(args.slug, args.product, args.variant, args.placement, DESIGN_URL)
    if not url:
        sys.exit("Failed to generate mockup.")

    out = OUT_DIR / f"{args.slug}.jpg"
    try:
        download(url, out)
        print(f"  ✓ saved {out} ({out.stat().st_size:,} bytes)")
    except Exception as e:
        sys.exit(f"Download failed: {e}")

    emit_seed_line(args.slug, args.cat, args.name, args.desc, args.price,
                   args.product, args.variant, args.placement, args.sizes)

    print("Next: paste the two lines above into store/src/main.rs, then:")
    print("  git add store/src/main.rs store/static/nakamura/" + args.slug + ".jpg")
    print(f'  git commit -m "feat(nakamura): add {args.slug}"')
    print("  git push")

if __name__ == "__main__":
    main()
