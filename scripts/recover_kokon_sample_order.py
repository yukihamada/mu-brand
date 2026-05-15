#!/usr/bin/env python3
"""Recover the 2026-05-15 harley1801cc kokon sample order (id 21,22,23).

The /api/kokon/sample-checkout webhook ran but didn't submit to Printful
because of the ?expand[]=shipping_details Stripe quirk (collab_orders had
empty ship_name/address). The 3 items were charged via Stripe but stuck
at status='sample_received'.

This script:
  1. Re-fetches Stripe session (without expand) to get the correct address
  2. Submits a single consolidated Printful order with all 3 line items
  3. Updates collab_orders.printful_order_id + status='sample_printful_draft'

Usage:
  STRIPE_SECRET_KEY=sk_live_... PRINTFUL_API_KEY=... python3 recover_kokon_sample_order.py [--dry-run]
"""
import argparse, json, os, sys, subprocess, sqlite3
import urllib.request, urllib.error
from datetime import datetime

SESSION_ID = "cs_live_b10sUIwHbPD9CPNqTmXN9kKbIHCVgNOSo5D3PkYdIfNpSr0J8diPtgM5kY"
ORDER_IDS  = [21, 22, 23]

# Printful product/variant IDs from seed (src/main.rs lines 37088-37104)
LINE_ITEMS = [
    # slug             variant_id  files_url
    ("kokon-tee",      4017, "https://lifestyle.wearmu.com/kokon/_logo_v2.png", None),
    ("kokon-apron",    23723,"https://lifestyle.wearmu.com/kokon/_logo_v2.png", [{"id":"stitch_color","value":"black"}]),
    ("kokon-snapback", 4792, "https://lifestyle.wearmu.com/kokon/_logo_v2.png", [{"id":"thread_colors_front_large","value":["#A67843"]}]),
]

# Printful file `type` per product
FILE_TYPE = {
    "kokon-tee":      "default",
    "kokon-apron":    "front",
    "kokon-snapback": "embroidery_front_large",
}

JP_PREFECTURE_ISO = {
    "Hokkaido":"JP-01","Aomori":"JP-02","Iwate":"JP-03","Miyagi":"JP-04","Akita":"JP-05","Yamagata":"JP-06",
    "Fukushima":"JP-07","Ibaraki":"JP-08","Tochigi":"JP-09","Gunma":"JP-10","Saitama":"JP-11","Chiba":"JP-12",
    "Tokyo":"JP-13","Kanagawa":"JP-14","Niigata":"JP-15","Toyama":"JP-16","Ishikawa":"JP-17","Fukui":"JP-18",
    "Yamanashi":"JP-19","Nagano":"JP-20","Gifu":"JP-21","Shizuoka":"JP-22","Aichi":"JP-23","Mie":"JP-24",
    "Shiga":"JP-25","Kyoto":"JP-26","Osaka":"JP-27","Hyogo":"JP-28","Nara":"JP-29","Wakayama":"JP-30",
    "Tottori":"JP-31","Shimane":"JP-32","Okayama":"JP-33","Hiroshima":"JP-34","Yamaguchi":"JP-35",
    "Tokushima":"JP-36","Kagawa":"JP-37","Ehime":"JP-38","Kochi":"JP-39","Fukuoka":"JP-40","Saga":"JP-41",
    "Nagasaki":"JP-42","Kumamoto":"JP-43","Oita":"JP-44","Miyazaki":"JP-45","Kagoshima":"JP-46","Okinawa":"JP-47",
}

def http_get_basic(url, user):
    req = urllib.request.Request(url)
    import base64
    auth = base64.b64encode(f"{user}:".encode()).decode()
    req.add_header("Authorization", f"Basic {auth}")
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r)

def http_post_bearer(url, token, payload):
    req = urllib.request.Request(url, data=json.dumps(payload).encode(),
                                  method="POST")
    req.add_header("Authorization", f"Bearer {token}")
    req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return r.status, json.load(r)
    except urllib.error.HTTPError as e:
        try:
            body = json.load(e)
        except Exception:
            body = {"error": str(e), "raw": e.read()[:500].decode(errors="replace")}
        return e.code, body

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    sk = os.environ.get("STRIPE_SECRET_KEY")
    pk = os.environ.get("PRINTFUL_API_KEY")
    if not sk:
        print("ERR: STRIPE_SECRET_KEY env var required", file=sys.stderr); sys.exit(1)
    if not pk:
        print("ERR: PRINTFUL_API_KEY env var required", file=sys.stderr); sys.exit(1)

    print(f"▶ Fetching Stripe session {SESSION_ID}")
    sess = http_get_basic(f"https://api.stripe.com/v1/checkout/sessions/{SESSION_ID}", sk)

    print(f"  amount_total = ¥{sess.get('amount_total')}")
    print(f"  payment_status = {sess.get('payment_status')}")
    email = (sess.get("customer_details") or {}).get("email") or sess.get("customer_email") or ""
    print(f"  email = {email}")

    # Pick address from shipping_details first, fall back to customer_details
    def pick(obj):
        a = (obj or {}).get("address")
        if not a or not a.get("line1"): return None
        return a
    shipping = sess.get("shipping_details") or {}
    cust = sess.get("customer_details") or {}
    addr = pick(shipping) or pick(cust)
    name = shipping.get("name") or cust.get("name") or ""

    if not addr or not name:
        print("ERR: shipping address still empty after both lookups", file=sys.stderr)
        print(json.dumps(sess, indent=2, ensure_ascii=False)[:2000])
        sys.exit(2)

    print(f"  ship_name = {name}")
    print(f"  ship_addr = {addr.get('line1')}, {addr.get('city')}, {addr.get('state')}, {addr.get('postal_code')}, {addr.get('country')}")
    state_code = JP_PREFECTURE_ISO.get(addr.get("state",""), addr.get("state",""))

    # Build Printful order
    items = []
    for slug, vid, file_url, opts in LINE_ITEMS:
        item = {"variant_id": vid, "quantity": 1,
                "files": [{"type": FILE_TYPE.get(slug, "default"), "url": file_url}]}
        if opts: item["options"] = opts
        items.append(item)

    short_sess = SESSION_ID.replace("cs_live_", "").replace("cs_test_", "")[:24]
    payload = {
        "recipient": {
            "name": name,
            "address1": addr.get("line1",""),
            "address2": addr.get("line2","") or "",
            "city": addr.get("city",""),
            "state_code": state_code,
            "country_code": addr.get("country","JP"),
            "zip": addr.get("postal_code",""),
        },
        "items": items,
        "confirm": True,  # auto-confirm = immediate print + ship
        "external_id": f"recover-{short_sess}",
    }
    print()
    print("▶ Printful order payload:")
    print(json.dumps(payload, indent=2, ensure_ascii=False))

    if args.dry_run:
        print()
        print("--dry-run: not submitting. Re-run without --dry-run to send.")
        return

    print()
    print("▶ POST https://api.printful.com/orders ...")
    code, body = http_post_bearer("https://api.printful.com/orders", pk, payload)
    print(f"  HTTP {code}")
    print(json.dumps(body, indent=2, ensure_ascii=False)[:1500])

    if code != 200:
        print("ERR: Printful order not created", file=sys.stderr); sys.exit(3)
    order_id = str(body.get("result", {}).get("id", ""))
    print(f"✓ Printful order_id = {order_id}")

    # Update local DB for verification (production DB needs admin endpoint)
    db_path = "/tmp/prod_products.db"
    if os.path.exists(db_path):
        db = sqlite3.connect(db_path)
        for oid in ORDER_IDS:
            db.execute("UPDATE collab_orders SET printful_order_id=?, status='sample_printful_draft' WHERE id=?",
                       (order_id, oid))
        db.commit()
        print(f"  local snapshot updated (3 rows). Production DB requires manual SQL via admin or next deploy.")
    print()
    print(f"📨 Notify harley1801cc@yahoo.co.jp that order #{order_id} is in production")
    print(f"   Estimated ship: ~10-14 days from Printful's JP/EU facility")

if __name__ == "__main__":
    main()
