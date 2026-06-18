#!/usr/bin/env python3
"""Seed the public sample catalog at wearmu.com/samples.

Posts ─◯─ (marker_zero) once per supported product kind via the public
/api/v1/sku/create endpoint. The default-price table inside the Rust
handler decides per-kind retail pricing; we just provide label + kind +
design_url.

Usage:
    export WEARMU_API_KEY=mu_...
    python3 scripts/seed_marker_zero_catalog.py          # dry-run preview
    python3 scripts/seed_marker_zero_catalog.py --apply  # actually POST

The script is idempotent in spirit: re-running it will create *new* SKUs
each time (the API auto-assigns letters m001 / m002 / …), so only run
--apply once per environment unless you intentionally want duplicates.
A future enhancement: add a /api/v1/sku?slug=samples&kind=tee GET to
allow real upsert.
"""

import argparse
import json
import os
import sys
import urllib.request
import urllib.error

# 19 kinds the Rust /api/v1/sku/create handler knows pricing for.
# Mirrors the match arm at store/src/main.rs:29390 (default price fallback).
KINDS = [
    ("tee",        "Tシャツ"),
    ("longsleeve", "ロンT"),
    ("tank_top",   "タンクトップ"),
    ("hoodie",     "パーカー"),
    ("crewneck",   "クルーネック"),
    ("zip_hoodie", "ジップパーカー"),
    ("cap",        "キャップ"),
    ("beanie",     "ビーニー"),
    ("tote",       "トートバッグ"),
    ("drawstring", "ドローストリングバッグ"),
    ("mug",        "マグカップ"),
    ("socks",      "ソックス"),
    ("sticker",    "ステッカー"),
    ("pin",        "ピンバッジ"),
    ("magnet",     "マグネット"),
    ("postcard",   "ポストカード"),
    ("notebook",   "ノート"),
    ("phonecase",  "スマホケース"),
    ("kids_tee",   "キッズTシャツ"),
]

BASE = os.environ.get("WEARMU_API_BASE", "https://wearmu.com")
DESIGN_URL = f"{BASE}/static/designs/marker_zero.png"
SLUG = "samples"  # bucket the SKUs under this slug → wearmu.com/samples

def post(api_key, kind, label_jp):
    body = {
        "label":      f"━◯━ sample · {label_jp}",
        "kind":       kind,
        "design_url": DESIGN_URL,
        "slug":       SLUG,
    }
    req = urllib.request.Request(
        f"{BASE}/api/v1/sku/create",
        data=json.dumps(body).encode(),
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type":  "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        try:
            payload = json.loads(e.read())
        except Exception:
            payload = {"raw": str(e)}
        return e.code, payload
    except Exception as e:
        return 0, {"error": str(e)}

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--apply", action="store_true",
                    help="Actually POST. Without this, prints the plan only.")
    ap.add_argument("--kinds", type=str, default="",
                    help="Comma-separated subset of kinds (default: all 19).")
    args = ap.parse_args()

    api_key = os.environ.get("WEARMU_API_KEY", "")
    if args.apply and not api_key:
        print("ERROR: WEARMU_API_KEY env var required for --apply", file=sys.stderr)
        sys.exit(2)

    selected = KINDS
    if args.kinds:
        wanted = {k.strip() for k in args.kinds.split(",") if k.strip()}
        selected = [k for k in KINDS if k[0] in wanted]
        if not selected:
            print(f"ERROR: no kinds matched {wanted}", file=sys.stderr)
            sys.exit(2)

    print(f"design: {DESIGN_URL}")
    print(f"slug:   {SLUG}")
    print(f"target: {BASE}")
    print(f"kinds:  {len(selected)}")
    print()

    if not args.apply:
        print("(dry-run — pass --apply to POST)")
        for kind, label in selected:
            print(f"  • POST /api/v1/sku/create  kind={kind:<11}  label={label}")
        return

    ok, fail = 0, 0
    for kind, label in selected:
        code, payload = post(api_key, kind, label)
        if 200 <= code < 300 and payload.get("ok") is not False:
            ok += 1
            print(f"  ✓ {kind:<11} → drop={payload.get('drop_num')}  "
                  f"letter={payload.get('letter')}  ¥{payload.get('price_jpy')}  "
                  f"cost_pt={payload.get('cost_pt')}  "
                  f"lp={payload.get('lp_url')}")
        else:
            fail += 1
            err = payload.get("error") or payload
            print(f"  ✗ {kind:<11} HTTP {code} — {err}")

    print()
    print(f"done: {ok} ok / {fail} fail / {len(selected)} total")
    if ok:
        print(f"view: {BASE}/{SLUG}")
    sys.exit(0 if fail == 0 else 1)

if __name__ == "__main__":
    main()
