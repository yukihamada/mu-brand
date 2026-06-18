#!/usr/bin/env python3
"""Republish jiufight: marked × MU + QR + timestamp designs across 3 SUZURI items.

For each of the 10 marked front designs (01..10):
  1. Upsert a proposal_skus row (slug=jiufight, letter=m01..m10, drop_num=4001..)
     with design_url pointing to the marked PNG already deployed under
     /static/jiufight/products/<n>_front_marked.png.
  2. Look up the resulting products row id via /api/admin/products.
  3. POST /api/admin/suzuri/publish/<pid>?items=149,152,8&force=1 — creates a
     SUZURI material with three product variants:
        149 オーバーサイズTシャツ  (bigger print frame — "もうちょっと大きく")
        152 ヘビーウェイトパーカー (variety: hoodie)
          8 フルグラフィックTシャツ (max-area sublimation)
  4. Collect the resulting URLs and emit a JSON manifest.

Run AFTER the marked PNGs are deployed (commit 0614e1d). Requires MU_ADMIN_TOKEN.
"""
from __future__ import annotations
import json, os, sys, time, urllib.request, urllib.error

BASE  = os.environ.get("MU_BASE", "https://wearmu.com")
TOKEN = os.environ["MU_ADMIN_TOKEN"]
ITEMS = os.environ.get("SUZURI_ITEMS", "149,152,8")

DESIGN_BASE = f"{BASE}/static/jiufight/products"
DROP_BASE = 4000   # offsetting from existing jiufight drops to avoid collision

def fetch_json(url, headers=None, body=None, method="GET"):
    req = urllib.request.Request(url, headers=headers or {}, method=method,
                                 data=(json.dumps(body).encode() if body is not None else None))
    if body is not None:
        req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.status, json.loads(r.read().decode("utf-8"))

def upsert_proposal():
    """One POST that upserts all 10 m01..m10 SKUs into proposal_skus + products."""
    skus = []
    for n in range(1, 11):
        skus.append({
            "letter":      f"m{n:02d}",
            "drop_num":    DROP_BASE + n,
            "price_jpy":   4900,
            "label":       f"JIUFIGHT × MU · drop {n:02d} · marked tee",
            "kind":        "tee",
            "design_slug": f"m{n:02d}",
            "design_url":  f"{DESIGN_BASE}/{n:02d}_front_marked.png",
        })
    body = {
        "slug": "jiufight",
        "name": "JIUFIGHT × MU",
        "ip_owner": "JiuFight Tournament / yuki",
        "skus": skus,
    }
    status, j = fetch_json(
        f"{BASE}/admin/proposal?admin_token={TOKEN}",
        method="POST", body=body,
    )
    print(f"upsert: HTTP {status} · {j.get('skus_total','?')} SKUs · {j.get('products_inserted','?')} new products")
    return j

def find_product_ids():
    """Return {letter: product_id} for the newly seeded drops 4001..4010."""
    status, d = fetch_json(
        f"{BASE}/api/admin/products?search=jiufight_tee_sample&limit=400&no_summary=1",
        headers={"X-Admin-Token": TOKEN},
    )
    out = {}
    for p in d.get("products") or []:
        dn = p.get("drop_num") or 0
        if DROP_BASE < dn <= DROP_BASE + 10:
            out[f"m{dn - DROP_BASE:02d}"] = p["id"]
    return out

def suzuri_publish(pid: int, items: str):
    url = f"{BASE}/api/admin/suzuri/publish/{pid}?token={TOKEN}&items={items}&force=1"
    req = urllib.request.Request(url, method="POST", data=b"{}",
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=180) as r:
            return r.status, json.loads(r.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read().decode("utf-8", "ignore") or "{}")

def main():
    print(f"BASE={BASE}  ITEMS={ITEMS}\n")
    print("=== upsert ==="); upsert_proposal(); time.sleep(1)
    print("\n=== resolve product_ids ==="); ids = find_product_ids()
    for letter, pid in sorted(ids.items()):
        print(f"  {letter} → pid {pid}")
    print(f"\n=== publishing to SUZURI items={ITEMS} (per design — material w/ multi-variant) ===")
    manifest = []
    for letter in sorted(ids):
        pid = ids[letter]
        status, j = suzuri_publish(pid, ITEMS)
        if status == 200 and j.get("ok"):
            print(f"  ✓ {letter} pid={pid:>4} material={j['suzuri_material_id']} canonical={j['suzuri_url']}")
            manifest.append({
                "letter": letter,
                "drop_num": DROP_BASE + int(letter[1:]),
                "product_id": pid,
                "design_url": f"{DESIGN_BASE}/{int(letter[1:]):02d}_front_marked.png",
                "suzuri_material_id": j["suzuri_material_id"],
                "suzuri_url": j["suzuri_url"],
            })
        else:
            print(f"  ✗ {letter} pid={pid}: HTTP {status} {j.get('error') or j}")
        time.sleep(0.5)
    print()
    out = f"/tmp/jiufight_manifest_{int(time.time())}.json"
    with open(out, "w") as f:
        json.dump(manifest, f, indent=2, ensure_ascii=False)
    print(f"manifest: {out}")
    print(f"OK: {len(manifest)}/10")

if __name__ == "__main__":
    main()
