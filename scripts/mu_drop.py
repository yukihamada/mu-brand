#!/usr/bin/env python3
"""mu_drop — one command: create an MU product → approve live → optionally ship.

Thin CLI over muclient.MuClient. Design source is EITHER an AI prompt
(--ai "...", spends mu_credits) OR a ready image (--design-url https://...).

  scripts/mu_drop.py --ai "<brief>" --kind tee --size L --ship
  scripts/mu_drop.py --design-url https://.../art.png --kind hoodie

Creds/address come from mu-brand/.secrets.local (see .secrets.local.example).
"""
import argparse, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from muclient import MuClient, MuError, BASE


def main():
    ap = argparse.ArgumentParser()
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--ai", help="AI design brief (spends mu_credits)")
    g.add_argument("--design-url", help="ready-made https artwork URL (free)")
    ap.add_argument("--kind", default="tee")
    ap.add_argument("--size", default="L")
    ap.add_argument("--label", default=None)
    ap.add_argument("--store", default="mu-lab")
    ap.add_argument("--price", type=int, default=None)
    ap.add_argument("--ship", action="store_true")
    ap.add_argument("--draft", action="store_true", help="with --ship: validate only (no charge)")
    a = ap.parse_args()

    mu = MuClient()
    label = a.label or (a.ai or a.design_url)[:60]
    try:
        print(f"① 作成中… ({a.kind})")
        r = mu.create_product(a.store, label, label, a.kind,
                              design_url=a.design_url, ai_prompt=a.ai, price_jpy=a.price)
        sku = r["sku"]
        print(f"   ✔ {sku}  {r['pdp_url']}")

        print("② 承認 (review→live)…")
        ap_r = mu.approve(sku)
        if ap_r.get("status") != "live":
            raise MuError(f"approve failed: {ap_r}")
        print(f"   ✔ LIVE → {BASE}/shop/{sku}")

        if a.ship:
            # design url for shipping: the one we passed, or fetch from PDP
            design = a.design_url
            if not design:
                import urllib.request, re
                html = urllib.request.urlopen(f"{BASE}/shop/{sku}", timeout=30).read().decode()
                m = re.search(r"https://mockups\.wearmu\.com/catalog/agent/[^\"')]+\.png", html)
                design = m.group(0) if m else None
            if not design:
                raise MuError("could not resolve design URL for shipping")
            print(f"③ Printful発注 → {mu.ship.get('SHIP_NAME')} / {mu.ship.get('SHIP_CITY')}…")
            o = mu.ship_sample(a.kind, a.size, design, name=f"{label} / Black / {a.size}",
                               confirm=not a.draft)
            c = o.get("costs", {})
            print(f"   ✔ order #{o['id']} status={o['status']} ¥{c.get('total')} {c.get('currency')}")
            print(f"   dashboard: https://www.printful.com/dashboard/order/{o['id']}")
        print(f"\n完了 → {BASE}/shop/{sku}")
    except MuError as e:
        sys.exit(f"✗ {e}")


if __name__ == "__main__":
    main()
