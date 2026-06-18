#!/usr/bin/env python3
"""Retry Printful mockup generation for products with null mockup_url."""
import os, sys, sqlite3, requests, time, json
from pathlib import Path

PRINTFUL_KEY = os.environ["PRINTFUL_API_KEY"]
ADMIN_TOKEN  = os.environ.get("MU_ADMIN_TOKEN", "mu-admin-2026")
STORE_URL    = os.environ.get("MU_STORE_URL", "https://wearmu.com")
DB_PATH      = Path(__file__).parent / "products.db"
PF_BASE      = "https://api.printful.com"
PF_HDR       = {"Authorization": f"Bearer {PRINTFUL_KEY}", "Content-Type": "application/json"}
PF_PRODUCT   = 71
PF_VARIANT   = 4017  # Black / M

def make_mockup(design_url: str) -> str | None:
    r = requests.post(f"{PF_BASE}/mockup-generator/create-task/{PF_PRODUCT}", headers=PF_HDR, json={
        "variant_ids": [PF_VARIANT],
        "format": "jpg",
        "files": [{"placement": "front", "image_url": design_url, "position": {
            "area_width": 1800, "area_height": 2400,
            "width": 1600, "height": 2000, "top": 200, "left": 100,
        }}]
    })
    if not r.ok:
        print(f"  mockup task error: {r.status_code} {r.text[:200]}")
        return None
    task_key = r.json()["result"]["task_key"]
    print(f"  task: {task_key} — polling...", end="", flush=True)
    for i in range(40):  # 40 × 5s = 200s max
        time.sleep(5)
        t = requests.get(f"{PF_BASE}/mockup-generator/task?task_key={task_key}", headers=PF_HDR)
        data = t.json()["result"]
        status = data["status"]
        print(f".", end="", flush=True)
        if status == "completed":
            url = data["mockups"][0]["mockup_url"]
            print(f" done")
            return url
        if status == "failed":
            print(f" FAILED")
            return None
    print(f" timeout")
    return None

def push_mockup(product_id: int, mockup_url: str):
    """Update mockup on deployed store via admin API."""
    r = requests.patch(
        f"{STORE_URL}/api/admin/mockup?token={ADMIN_TOKEN}",
        json={"product_id": product_id, "mockup_url": mockup_url},
        timeout=10
    )
    return r.status_code == 200

con = sqlite3.connect(DB_PATH)
rows = con.execute(
    "SELECT id, brand, name, design_url FROM products WHERE mockup_url IS NULL AND design_url IS NOT NULL"
).fetchall()

print(f"Found {len(rows)} products without mockups")

for pid, brand, name, design_url in rows:
    print(f"\n[{pid}] {name}")
    print(f"  design: {design_url}")

    mockup_url = make_mockup(design_url)
    if not mockup_url:
        continue

    print(f"  mockup: {mockup_url}")

    # Update local DB
    con.execute("UPDATE products SET mockup_url=? WHERE id=?", (mockup_url, pid))
    con.commit()
    print(f"  local DB updated")

    # Push to deployed store
    try:
        r = requests.patch(
            f"{STORE_URL}/api/admin/mockup?token={ADMIN_TOKEN}",
            json={"product_id": pid, "mockup_url": mockup_url},
            timeout=10
        )
        print(f"  store push: {r.status_code}")
    except Exception as e:
        print(f"  store push failed: {e}")

print("\nDone.")
