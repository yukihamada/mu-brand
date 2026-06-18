#!/usr/bin/env python3
"""Register 12 TAXIGEN samples in prod + trigger Printful mockups.

Same pattern as prod_import_kawanabe.py.
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

import requests, sqlite3

PROD = "https://wearmu.com"
TOK = os.environ["MU_ADMIN_TOKEN"]
COST_JPY = 2200

# Pull from local DB (already synced with files)
DB = Path(__file__).resolve().parent.parent / "store" / "products.db"
db = sqlite3.connect(DB)
rows = db.execute(
    "SELECT drop_num, name, design_url FROM products WHERE brand='taxigen' ORDER BY drop_num"
).fetchall()


def find_existing(drop):
    try:
        r = requests.get(f"{PROD}/api/products/taxigen", timeout=15)
        if r.status_code != 200: return None
        for p in r.json():
            if p.get("drop_num") == drop: return p.get("id")
    except Exception: pass
    return None


def create(name):
    body = {"brand": "taxigen", "name": name,
            "prompt": "Placeholder; design_url overridden post-create",
            "price_jpy": 5000, "cost_jpy": COST_JPY, "inventory": 1, "force_loss": False}
    r = requests.post(f"{PROD}/api/admin/products/new",
                      params={"token": TOK}, json=body, timeout=180)
    if r.status_code != 200:
        print(f"    ✗ create failed {r.status_code}: {r.text[:200]}")
        return None
    return r.json().get("new_id")


def update(pid, name, design_url):
    body = {"name": name, "price_jpy": 5000, "inventory": 1, "active": 1,
            "design_url": design_url, "mockup_url": design_url, "force_loss": False}
    r = requests.post(f"{PROD}/api/admin/products/{pid}/update",
                      params={"token": TOK}, json=body, timeout=30)
    return r.status_code == 200


def trigger_mockup(pid):
    r = requests.post(f"{PROD}/api/admin/products/{pid}/regen_mockup",
                      params={"token": TOK}, timeout=30)
    return r.status_code in (200, 202)


def poll(pid, timeout=180):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            r = requests.get(f"{PROD}/api/products/item/{pid}", timeout=10)
            if r.status_code == 200:
                mu = r.json().get("mockup_url", "") or ""
                if mu and "/static/ads/" not in mu and "taxigen_" not in mu:
                    return mu
        except Exception: pass
        time.sleep(5)
    return None


def main():
    results = []
    for drop, name, design_local in rows:
        print(f"\n=== #{drop} {name[:50]}... ===")
        pid = find_existing(drop)
        if pid:
            print(f"  ◯ exists id={pid}")
        else:
            pid = create(name)
            if not pid:
                results.append({"drop": drop, "status": "create_fail"}); continue
            time.sleep(1)
            print(f"  ✓ created id={pid}")
        if not update(pid, name, design_local):
            print(f"  ✗ update failed")
            results.append({"drop": drop, "pid": pid, "status": "update_fail"}); continue
        if not trigger_mockup(pid):
            print(f"  ✗ mockup trigger failed")
            results.append({"drop": drop, "pid": pid, "status": "mockup_trigger_fail"}); continue
        print(f"  ⏳ polling mockup...")
        m = poll(pid)
        if m:
            print(f"  ✓ MOCKUP: {m}")
            results.append({"drop": drop, "pid": pid, "status": "ok", "mockup": m})
        else:
            results.append({"drop": drop, "pid": pid, "status": "mockup_pending"})

    print("\n" + "=" * 60)
    print(json.dumps(results, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
