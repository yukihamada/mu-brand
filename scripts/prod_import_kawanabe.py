#!/usr/bin/env python3
"""Import 10 kawanabe_personal designs to prod + trigger Printful mockups.

For each: POST /api/admin/products/new → override design_url via update →
POST /api/admin/products/:id/regen_mockup → poll /api/products/item/:id
until mockup_url is no longer the local /static/ads/ URL (= Printful done).
"""
import os, sys, time, json
from pathlib import Path

_env = Path("/Users/yuki/.env")
if _env.exists():
    for ln in _env.read_text().splitlines():
        ln = ln.strip()
        if "=" in ln and not ln.startswith("#"):
            k, v = ln.split("=", 1)
            os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))

import requests

PROD = "https://wearmu.com"
TOK = os.environ["MU_ADMIN_TOKEN"]

DESIGNS = [
    (1, "MU × TOKYO TAXI 1928 — Heritage Badge", "kawanabe_001_9a83eb76.png"),
    (2, "MU × 行灯 ANDON", "kawanabe_002_8a82e652.png"),
    (3, "MU × 四代目 / Fourth Generation", "kawanabe_003_ef7597f9.png"),
    (4, "MU × 黒タク KURO TAKU", "kawanabe_004_64a1021e.png"),
    (5, "MU × 流し RUNNER", "kawanabe_005_36438f9d.png"),
    (6, "MU × メーター METER ¥1,928", "kawanabe_006_9c653787.png"),
    (7, "MU × おもてなし OMOTENASHI", "kawanabe_007_064cdfef.png"),
    (8, "MU × NEXT STOP TESHIKAGA", "kawanabe_008_5ee356b6.png"),
    (9, "MU × DRIVER 0001", "kawanabe_009_3e84aefc.png"),
    (10, "MU × 東京 TOKYO Vertical Wordmark", "kawanabe_010_1e94d796.png"),
]

COST_JPY = 2200  # Printful Bella+Canvas 3001 DTG estimate


def find_existing(brand, drop):
    """Look for existing prod row via /api/products/<brand> list."""
    try:
        r = requests.get(f"{PROD}/api/products/{brand}", timeout=15)
        if r.status_code != 200: return None
        for p in r.json():
            if p.get("brand") == brand and p.get("drop_num") == drop:
                return p.get("id")
    except Exception:
        pass
    return None


def create(name, design_path):
    body = {
        "brand": "kawanabe_personal",
        "name": name,
        "prompt": f"Placeholder for {name}; design_url overridden post-create",
        "price_jpy": 5000,
        "cost_jpy": COST_JPY,
        "inventory": 1,
        "force_loss": False,
    }
    r = requests.post(f"{PROD}/api/admin/products/new",
                      params={"token": TOK}, json=body, timeout=180)
    if r.status_code != 200:
        print(f"    ✗ create failed {r.status_code}: {r.text[:200]}")
        return None
    return r.json().get("new_id")


def update(pid, name, design_url):
    body = {
        "name": name,
        "price_jpy": 5000,
        "inventory": 1,
        "active": 1,
        "design_url": design_url,
        "mockup_url": design_url,
        "force_loss": False,
    }
    r = requests.post(f"{PROD}/api/admin/products/{pid}/update",
                      params={"token": TOK}, json=body, timeout=30)
    return r.status_code == 200


def trigger_mockup(pid):
    r = requests.post(f"{PROD}/api/admin/products/{pid}/regen_mockup",
                      params={"token": TOK}, timeout=30)
    return r.status_code in (200, 202)


def poll_mockup(pid, design_local_url, timeout=180):
    """Wait until mockup_url changes away from the local /static/ads URL."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            r = requests.get(f"{PROD}/api/products/item/{pid}", timeout=10)
            if r.status_code == 200:
                mu = r.json().get("mockup_url", "") or ""
                # Printful mockups are usually under printful CDN or our R2/lifestyle bucket
                if mu and "/static/ads/" not in mu and "kawanabe_" not in mu:
                    return mu
        except Exception:
            pass
        time.sleep(5)
    return None


def main():
    results = []
    for drop, name, fname in DESIGNS:
        design_local = f"/static/ads/{fname}"
        print(f"\n=== #{drop} {name} ===")
        pid = find_existing("kawanabe_personal", drop)
        if pid:
            print(f"  ◯ exists: id={pid}")
        else:
            print(f"  ↻ creating...")
            pid = create(name, design_local)
            if not pid:
                results.append({"drop": drop, "status": "create_failed"})
                continue
            time.sleep(1)
            print(f"  ✓ created id={pid}")

        if not update(pid, name, design_local):
            print(f"  ✗ update failed")
            results.append({"drop": drop, "pid": pid, "status": "update_failed"})
            continue

        if not trigger_mockup(pid):
            print(f"  ✗ mockup trigger failed")
            results.append({"drop": drop, "pid": pid, "status": "mockup_trigger_failed"})
            continue
        print(f"  ⏳ mockup task triggered, polling...")
        mockup_url = poll_mockup(pid, design_local, timeout=180)
        if mockup_url:
            print(f"  ✓ MOCKUP READY: {mockup_url}")
            results.append({"drop": drop, "pid": pid, "status": "ok", "mockup_url": mockup_url})
        else:
            print(f"  ⚠ mockup not ready in time (continues in bg)")
            results.append({"drop": drop, "pid": pid, "status": "mockup_pending"})

    print("\n" + "=" * 60)
    print(json.dumps(results, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
