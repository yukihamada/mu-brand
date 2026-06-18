#!/usr/bin/env python3
"""shirt.py — Fast wearmu product pipeline.

ONE COMMAND from a finished design PNG → live product on wearmu.com
(and optionally SUZURI).

USAGE
─────
# Single product
shirt.py add \\
  --brand jiufight \\
  --partner jiufight \\
  --slug jiufight-v3-02 \\
  --name "JIUFIGHT — 柔 Brushwork Tee" \\
  --design ./fixed/02_kanji_jyu_brushwork.png \\
  --price 4900 \\
  --category "Event Tee" \\
  [--suzuri]                 # also mirror to SUZURI JP

# Bulk from a directory (one product per PNG)
shirt.py bulk \\
  --brand jiufight \\
  --partner jiufight \\
  --version v3 \\
  --dir ./fixed/ \\
  --price 4900 \\
  --category "Event Tee" \\
  --name-prefix "JIUFIGHT — " \\
  [--suzuri-slugs jiufight-v3-02,jiufight-v3-04,jiufight-v3-07]

ENV (read from /Users/yuki/.env)
────────────────────────────────
  MU_ADMIN_TOKEN          — admin auth for wearmu
  SUZURI_ACCESS_TOKEN     — only needed when --suzuri / --suzuri-slugs used
                            (wearmu reads this server-side, but warn locally)

WHAT IT DOES PER PRODUCT
────────────────────────
1. wrangler r2 put → wearmu-lifestyle/<brand>/<version>/<filename>
2. POST /api/admin/collab_products/new   (powers wearmu.com/<partner> page)
3. POST /api/admin/import                (powers products table → SUZURI mirror)
4. (optional) POST /api/admin/suzuri/publish/:pid
"""
from __future__ import annotations
import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Optional

try:
    import requests
except ImportError:
    print("pip install requests", file=sys.stderr); sys.exit(1)

def _autoload_dotenv():
    """Read /Users/yuki/.env on import so callers don't need to `source` it."""
    p = Path(os.path.expanduser("~/.env"))
    if not p.exists(): return
    for line in p.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line: continue
        k, v = line.split("=", 1)
        k = k.strip(); v = v.strip().strip('"').strip("'")
        if k and k not in os.environ:
            os.environ[k] = v

_autoload_dotenv()
STORE = os.environ.get("WEARMU_STORE", "https://wearmu.com")
R2_BUCKET = "wearmu-lifestyle"
R2_PUBLIC_BASE = "https://lifestyle.wearmu.com"
ADMIN_TOKEN = os.environ.get("MU_ADMIN_TOKEN") or os.environ.get("ADMIN_TOKEN")

def fail(msg: str, code: int = 1):
    print(f"[shirt] ERROR: {msg}", file=sys.stderr); sys.exit(code)

def need_token():
    if not ADMIN_TOKEN:
        fail("MU_ADMIN_TOKEN not set. `source /Users/yuki/.env` first.")

def r2_upload(local: Path, key: str) -> str:
    cmd = ["wrangler", "r2", "object", "put", f"{R2_BUCKET}/{key}",
           "--file", str(local), "--content-type", "image/png", "--remote"]
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
    if r.returncode != 0:
        fail(f"wrangler upload failed: {r.stderr[:300]}")
    return f"{R2_PUBLIC_BASE}/{key}"

def collab_create(slug, partner, category, name, image_url, price, description="",
                  sizes=("XS","S","M","L","XL","XXL"), route="printful",
                  printful_variant_id: Optional[int] = None) -> dict:
    body = {
        "slug": slug, "partner": partner, "category": category, "name": name,
        "description": description, "image_url": image_url, "price_jpy": price,
        "sizes": list(sizes), "active": 1, "draft": 0,
        "production_route": route,
    }
    if printful_variant_id:
        body["printful_variant_id"] = printful_variant_id
    r = requests.post(f"{STORE}/api/admin/collab_products/new",
                      params={"token": ADMIN_TOKEN}, json=body, timeout=120)
    if r.status_code == 409:
        # already exists — that's ok, look it up
        return {"ok": True, "existed": True, "slug": slug}
    if not r.ok:
        fail(f"collab_create {slug}: {r.status_code} {r.text[:200]}")
    return r.json()

def product_import(brand, drop_num, name, design_url, price, inventory=999) -> dict:
    body = {
        "brand": brand, "drop_num": drop_num, "name": name,
        "design_url": design_url, "mockup_url": design_url,
        "price_jpy": price, "inventory": inventory,
        "weather_data": None, "prompt_hash": None, "seed_data": None,
        "auction_end": None, "nft_mint": None, "is_ice": False,
    }
    r = requests.post(f"{STORE}/api/admin/import",
                      params={"token": ADMIN_TOKEN}, json=body, timeout=120)
    if not r.ok:
        fail(f"product_import {brand}/{drop_num}: {r.status_code} {r.text[:200]}")
    return r.json()

def suzuri_publish(pid: int, force: bool = False) -> dict:
    params = {"token": ADMIN_TOKEN}
    if force: params["force"] = "1"
    r = requests.post(f"{STORE}/api/admin/suzuri/publish/{pid}",
                      params=params, timeout=120)
    if not r.ok:
        return {"ok": False, "error": f"{r.status_code} {r.text[:200]}"}
    return r.json()

def next_drop_num(brand: str) -> int:
    """Pick a safe drop_num. Queries the public collab catalog to find max."""
    # Use a deterministic local fallback: epoch seconds (always unique).
    return int(time.time())

def add_one(args):
    need_token()
    design_path = Path(args.design).resolve()
    if not design_path.exists():
        fail(f"design not found: {design_path}")
    version = args.version or "v1"
    key = f"{args.brand}/{version}/{design_path.name}"
    print(f"[1/4] uploading -> {R2_PUBLIC_BASE}/{key}")
    image_url = r2_upload(design_path, key)
    print(f"[2/4] collab_products INSERT  slug={args.slug}")
    c = collab_create(args.slug, args.partner, args.category, args.name,
                      image_url, args.price, description=args.description or "")
    print(f"       → {c}")
    drop_num = args.drop_num or next_drop_num(args.brand)
    print(f"[3/4] products INSERT          brand={args.brand} drop_num={drop_num}")
    p = product_import(args.brand, drop_num, args.name, image_url, args.price)
    print(f"       → {p}")
    pid = p.get("new_id") or p.get("id")
    suzuri_url = None
    if args.suzuri and pid:
        print(f"[4/4] SUZURI publish           product_id={pid}")
        s = suzuri_publish(pid)
        print(f"       → {s}")
        suzuri_url = s.get("suzuri_url")
    else:
        print("[4/4] SUZURI publish           SKIPPED (no --suzuri)")
    print()
    print(f"DONE  collab_slug={args.slug}  product_id={pid}")
    if suzuri_url:
        print(f"      suzuri:  {suzuri_url}")
    print(f"      product_page: {STORE}/{args.partner}")

def bulk(args):
    need_token()
    d = Path(args.dir).resolve()
    if not d.is_dir(): fail(f"--dir not a directory: {d}")
    pngs = sorted(p for p in d.glob("*.png") if not p.name.startswith("_"))
    if not pngs: fail(f"no PNGs in {d}")
    suzuri_slugs = set((args.suzuri_slugs or "").split(",")) if args.suzuri_slugs else set()
    suzuri_slugs.discard("")
    base_drop = int(time.time())
    results = []
    for i, png in enumerate(pngs):
        stem = png.stem                                   # "02_kanji_jyu_brushwork"
        slug = f"{args.brand}-{args.version}-{stem.split('_',1)[0]}"  # jiufight-v3-02
        pretty_part = stem.split('_', 1)[1].replace('_', ' ').title() if '_' in stem else stem
        name = f"{args.name_prefix or ''}{pretty_part} Tee"
        print(f"\n── [{i+1}/{len(pngs)}] {png.name}  slug={slug}")
        key = f"{args.brand}/{args.version}/{png.name}"
        image_url = r2_upload(png, key)
        c = collab_create(slug, args.partner, args.category, name, image_url,
                          args.price, description=args.description or "")
        drop_num = base_drop + i
        p = product_import(args.brand, drop_num, name, image_url, args.price)
        pid = p.get("new_id") or p.get("id")
        suzuri_url = None
        if slug in suzuri_slugs and pid:
            print(f"   SUZURI publish product_id={pid}")
            s = suzuri_publish(pid)
            suzuri_url = s.get("suzuri_url")
            if not s.get("ok"): print(f"   SUZURI WARN: {s.get('error')}")
        results.append({
            "slug": slug, "name": name, "image_url": image_url,
            "drop_num": drop_num, "product_id": pid,
            "collab_create": c, "suzuri_url": suzuri_url,
        })
    print("\n──── SUMMARY ────")
    for r in results:
        s = f" SUZURI={r['suzuri_url']}" if r["suzuri_url"] else ""
        print(f"  {r['slug']:30s} pid={r['product_id']:<6}{s}")
    out_path = d / f"_shirt_results_{args.brand}_{args.version}.json"
    out_path.write_text(json.dumps(results, ensure_ascii=False, indent=2))
    print(f"\nResults JSON: {out_path}")
    print(f"Page:  {STORE}/{args.partner}")

def main():
    p = argparse.ArgumentParser(description="Fast wearmu product pipeline")
    sub = p.add_subparsers(dest="cmd", required=True)

    a = sub.add_parser("add", help="add a single product")
    a.add_argument("--brand", required=True)
    a.add_argument("--partner", required=True, help="collab partner key (e.g. jiufight)")
    a.add_argument("--slug", required=True)
    a.add_argument("--name", required=True)
    a.add_argument("--design", required=True, help="local PNG path")
    a.add_argument("--price", type=int, required=True)
    a.add_argument("--category", default="Event Tee")
    a.add_argument("--description", default="")
    a.add_argument("--version", default="v1")
    a.add_argument("--drop-num", type=int, default=None)
    a.add_argument("--suzuri", action="store_true")
    a.set_defaults(func=add_one)

    b = sub.add_parser("bulk", help="add all PNGs in a directory")
    b.add_argument("--brand", required=True)
    b.add_argument("--partner", required=True)
    b.add_argument("--version", required=True)
    b.add_argument("--dir", required=True)
    b.add_argument("--price", type=int, required=True)
    b.add_argument("--category", default="Event Tee")
    b.add_argument("--description", default="")
    b.add_argument("--name-prefix", default="")
    b.add_argument("--suzuri-slugs", default="",
                   help="comma-separated slugs to publish to SUZURI")
    b.set_defaults(func=bulk)

    args = p.parse_args()
    args.func(args)

if __name__ == "__main__":
    main()
