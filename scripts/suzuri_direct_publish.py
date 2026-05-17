#!/usr/bin/env python3
"""suzuri_direct_publish.py — Publish a design PNG to SUZURI directly.

Bypasses wearmu's /api/admin/suzuri/publish — uploads raw bytes to
suzuri.jp/api/v1/materials and creates a product on item #148
(ヘビーウェイトTシャツ) with ¥1,400 creator margin (¥4,900 retail).

USAGE
─────
  python3 suzuri_direct_publish.py path/to/design.png --title "JIUFIGHT 柔" \\
    [--margin 1400] [--item 148]

  # Bulk: feed a JSON file with [{"file":"…","title":"…"}, …]
  python3 suzuri_direct_publish.py --bulk bulk.json

ENV
───
  SUZURI_ACCESS_TOKEN  (read from /Users/yuki/.env if present)
"""
from __future__ import annotations
import argparse, base64, json, os, sys, time
from pathlib import Path

import requests

def _autoload_dotenv():
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
TOKEN = os.environ.get("SUZURI_ACCESS_TOKEN")

SUZURI_API = "https://suzuri.jp/api/v1/materials"

def publish_one(file_path: Path, title: str, margin: int = 1400,
                item_id: int = 148, retries: int = 2) -> dict:
    if not TOKEN: raise RuntimeError("SUZURI_ACCESS_TOKEN not set")
    raw = file_path.read_bytes()
    b64 = base64.b64encode(raw).decode()
    body = {
        "texture": f"data:image/png;base64,{b64}",
        "title": title,
        "price": margin,
        "products": [{"itemId": item_id, "published": True, "resaleEnabled": False}],
    }
    headers = {"Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json"}
    last_err = None
    for attempt in range(retries + 1):
        try:
            r = requests.post(SUZURI_API, json=body, headers=headers, timeout=120)
            if r.status_code == 429:
                wait = int(r.headers.get("Retry-After", "5"))
                print(f"  [429] rate limited, sleeping {wait}s")
                time.sleep(wait); continue
            if not r.ok:
                last_err = f"{r.status_code} {r.text[:300]}"
                if attempt < retries:
                    print(f"  [err] {last_err} — retry {attempt+1}/{retries}")
                    time.sleep(3); continue
                raise RuntimeError(last_err)
            j = r.json()
            material_id = j["material"]["id"]
            first = j["products"][0]
            spid = first["id"]
            url_template = first.get("url", "")
            pretty = url_template.replace("{size}", "m").replace("{color}", "black")
            return {"ok": True, "material_id": material_id,
                    "suzuri_product_id": spid, "url": pretty}
        except requests.RequestException as e:
            last_err = str(e)
            if attempt < retries:
                print(f"  [err] {last_err} — retry {attempt+1}/{retries}")
                time.sleep(3); continue
            return {"ok": False, "error": last_err}
    return {"ok": False, "error": last_err}

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("file", nargs="?")
    ap.add_argument("--title")
    ap.add_argument("--margin", type=int, default=1400)
    ap.add_argument("--item", type=int, default=148)
    ap.add_argument("--bulk", help="JSON file: [{file,title}, …]")
    ap.add_argument("--out", help="Write results JSON to this path")
    args = ap.parse_args()

    items: list[dict] = []
    if args.bulk:
        items = json.loads(Path(args.bulk).read_text())
    else:
        if not args.file or not args.title:
            print("Need file + --title (or --bulk)", file=sys.stderr); sys.exit(1)
        items = [{"file": args.file, "title": args.title}]

    results = []
    for i, it in enumerate(items, 1):
        fp = Path(it["file"]).resolve()
        title = it["title"]
        print(f"[{i}/{len(items)}] {title}  ({fp.name})")
        if not fp.exists():
            print(f"  SKIP: file missing"); continue
        r = publish_one(fp, title, args.margin, args.item)
        rec = {"file": str(fp), "title": title, **r}
        results.append(rec)
        if r.get("ok"):
            print(f"  ✓ material={r['material_id']}  product={r['suzuri_product_id']}")
            print(f"    {r['url']}")
        else:
            print(f"  ✗ {r.get('error')}")
        time.sleep(1)

    if args.out:
        Path(args.out).write_text(json.dumps(results, ensure_ascii=False, indent=2))
        print(f"\nResults JSON: {args.out}")

if __name__ == "__main__":
    main()
