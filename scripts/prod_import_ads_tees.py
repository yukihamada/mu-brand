#!/usr/bin/env python3
"""Import 20 ads_* SKUs into PROD via wearmu.com admin API.

Two-step per SKU:
  1. POST /api/admin/products/new — creates row (generates throwaway Gemini design)
  2. POST /api/admin/products/:id/update — overrides design_url/mockup_url with
     our pre-made /static/ads/*.png + sets name/price/inventory/active correctly

Why not flyctl ssh: we don't have a Fly session here. Admin API works from anywhere
with MU_ADMIN_TOKEN.

The throwaway Gemini call on step 1 costs ~$0.04 × 20 = ~$0.80 in API tokens
but unblocks end-to-end automation. Acceptable.

Idempotent: checks prod for (brand, drop_num) before insert; if exists, only PATCHes.
"""
import os, sys, time, json
from pathlib import Path
from urllib.parse import quote

# Load .env
_env = Path("/Users/yuki/.env")
if _env.exists():
    for ln in _env.read_text().splitlines():
        ln = ln.strip()
        if "=" in ln and not ln.startswith("#"):
            k, v = ln.split("=", 1)
            os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))

import requests

PROD = os.environ.get("MU_STORE_URL", "https://wearmu.com")
TOK = os.environ.get("MU_ADMIN_TOKEN")
if not TOK:
    sys.exit("MU_ADMIN_TOKEN not set")

# Same definitions as scripts/add_ads_targeted_tees.py SKUS
SKUS = [
    # (brand, drop_num, name, price_jpy, inventory, design_filename)
    ("ads_jujitsu", 1, "NOGI 柔術家 Tシャツ — Black", 4900, 30, "ads_jujitsu_001_fc57fb98.png"),
    ("ads_jujitsu", 2, "NOGI 柔術家 Tシャツ — White", 4900, 30, "ads_jujitsu_002_bc4272ba.png"),
    ("ads_jujitsu", 3, "白帯 Day One Tee", 4900, 20, "ads_jujitsu_003_40d5b32c.png"),
    ("ads_jujitsu", 4, "青帯 Tee — One Bar Down", 4900, 20, "ads_jujitsu_004_3ffc031d.png"),
    ("ads_jujitsu", 5, "紫帯 Tee — The Long Game", 4900, 20, "ads_jujitsu_005_088e1ea8.png"),
    ("ads_jujitsu", 6, "黒帯 Tee — Decade", 5500, 15, "ads_jujitsu_006_1a754c32.png"),
    ("ads_jujitsu", 7, "Tap or Nap Tee", 4900, 30, "ads_jujitsu_007_fb74aca7.png"),
    ("ads_jujitsu", 8, "Berimbolo Specialist Tee", 4900, 20, "ads_jujitsu_008_69d070b8.png"),
    ("ads_regional", 1, "三田 / MITA Tee", 4900, 20, "ads_regional_001_03e47956.png"),
    ("ads_regional", 2, "北参道 BJJ Tee", 4900, 20, "ads_regional_002_284829db.png"),
    ("ads_regional", 3, "弟子屈 / TESHIKAGA Tee", 4900, 20, "ads_regional_003_314b3b91.png"),
    ("ads_regional", 4, "川湯温泉 ONSEN Tee", 4900, 20, "ads_regional_004_9358b4a7.png"),
    ("ads_kokon", 1, "焼肉古今 Logo Tee", 4900, 30, "ads_kokon_001_de3ac57b.png"),
    ("ads_kokon", 2, "焼肉好き Gift Tee — 'Meat is Life'", 4900, 30, "ads_kokon_002_a17080aa.png"),
    ("ads_kokon", 3, "肉 FRIDAY Tee", 4900, 20, "ads_kokon_003_43cad215.png"),
    ("ads_profession", 1, "ICU Night Shift Tee 看護師", 4900, 20, "ads_profession_001_9050405e.png"),
    ("ads_profession", 2, "Coffee → Code → Compile Tee", 4900, 20, "ads_profession_002_7b2f6a30.png"),
    ("ads_event", 1, "SOLUNA FEST HAWAII 2026 Tee", 5500, 50, "ads_event_001_b67b717d.png"),
    ("ads_event", 2, "父の日 釣り好きDad Tee", 4900, 20, "ads_event_002_b861899e.png"),
    ("ads_event", 3, "東日本グラップリングオープン 2026 記念Tee", 5500, 30, "ads_event_003_031145c4.png"),
]

# Reasonable cost estimate for Bella+Canvas 3001 DTG via Printful (¥-converted)
COST_JPY = 2200


def find_existing(brand: str, drop_num: int) -> int | None:
    """Look up existing product id by (brand, drop_num) via /api/admin/products."""
    try:
        r = requests.get(
            f"{PROD}/api/admin/products",
            params={"token": TOK, "brand": brand, "limit": 200},
            timeout=20,
        )
        if r.status_code != 200:
            return None
        items = r.json() if isinstance(r.json(), list) else r.json().get("items", [])
        for it in items:
            if it.get("brand") == brand and it.get("drop_num") == drop_num:
                return it.get("id")
    except Exception:
        pass
    return None


def create_product(brand: str, name: str, price: int, inventory: int) -> int | None:
    """POST /api/admin/products/new. Returns new id or None."""
    body = {
        "brand": brand,
        "name": name,
        "prompt": f"Placeholder for {name}; design_url overridden post-create",
        "price_jpy": price,
        "cost_jpy": COST_JPY,
        "inventory": inventory,
        "force_loss": False,
    }
    r = requests.post(
        f"{PROD}/api/admin/products/new",
        params={"token": TOK},
        json=body,
        timeout=180,  # Gemini gen + R2 upload can be slow
    )
    if r.status_code != 200:
        print(f"    ✗ create failed {r.status_code}: {r.text[:200]}")
        return None
    return r.json().get("new_id")


def update_product(pid: int, name: str, price: int, inventory: int, design_url: str) -> bool:
    body = {
        "name": name,
        "price_jpy": price,
        "inventory": inventory,
        "active": 1,
        "design_url": design_url,
        "mockup_url": design_url,
        "force_loss": False,
    }
    r = requests.post(
        f"{PROD}/api/admin/products/{pid}/update",
        params={"token": TOK},
        json=body,
        timeout=30,
    )
    if r.status_code != 200:
        print(f"    ✗ update failed {r.status_code}: {r.text[:200]}")
        return False
    return True


def main():
    print(f"Importing {len(SKUS)} ads_* SKUs into {PROD}")
    print(f"Cost estimate: ~$0.80 (Gemini throwaway designs) + ~¥0 (admin API)\n")
    created, updated, skipped, failed = 0, 0, 0, 0
    for brand, drop, name, price, inv, fname in SKUS:
        design_url = f"/static/ads/{fname}"
        existing = find_existing(brand, drop)
        if existing:
            print(f"  ◯ exists: {brand} #{drop} (prod id={existing})")
            if update_product(existing, name, price, inv, design_url):
                updated += 1
                print(f"    ↑ patched: design_url={design_url}, active=1")
            else:
                failed += 1
            continue
        print(f"  ↻ creating: {brand} #{drop} — {name}")
        pid = create_product(brand, name, price, inv)
        if not pid:
            failed += 1
            continue
        time.sleep(1)
        if update_product(pid, name, price, inv, design_url):
            created += 1
            print(f"    ✓ id={pid} → design_url={design_url}, ¥{price:,} × {inv}")
        else:
            failed += 1
    print()
    print(f"📊 Created: {created}, Updated: {updated}, Skipped: {skipped}, Failed: {failed}")
    if created or updated:
        print(f"\nVerify: curl -s {PROD}/api/products/ads_jujitsu | jq '. | length'")


if __name__ == "__main__":
    main()
