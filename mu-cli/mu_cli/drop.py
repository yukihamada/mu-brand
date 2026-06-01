"""`mu` — create an MU product → approve live → optionally ship."""
import argparse, sys
from .client import MuClient, MuError, BASE

def main():
    ap = argparse.ArgumentParser(prog="mu", description="Create an MU product (one command).")
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--ai", help="AI design brief (spends mu_credits)")
    g.add_argument("--design-url", help="ready-made https artwork URL (free)")
    ap.add_argument("--kind", default="tee")
    ap.add_argument("--size", default="L")
    ap.add_argument("--label"); ap.add_argument("--store", default="mu-lab")
    ap.add_argument("--price", type=int); ap.add_argument("--ship", action="store_true")
    ap.add_argument("--draft", action="store_true")
    a = ap.parse_args()
    mu = MuClient(); label = a.label or (a.ai or a.design_url)[:60]
    try:
        r = mu.create_product(a.store, label, label, a.kind, design_url=a.design_url, ai_prompt=a.ai, price_jpy=a.price)
        sku = r["sku"]; print(f"✔ {sku}  {r['pdp_url']}")
        if mu.approve(sku).get("status") == "live": print(f"✔ LIVE → {BASE}/shop/{sku}")
        if a.ship:
            design = a.design_url
            if not design:
                import urllib.request, re
                html = urllib.request.urlopen(f"{BASE}/shop/{sku}", timeout=30).read().decode()
                m = re.search(r"https://mockups\.wearmu\.com/catalog/agent/[^\"')]+\.png", html); design = m and m.group(0)
            if not design: raise MuError("no design URL for shipping")
            o = mu.ship_sample(a.kind, a.size, design, name=f"{label} / Black / {a.size}", confirm=not a.draft)
            c = o.get("costs", {}); print(f"✔ order #{o['id']} {o['status']} ¥{c.get('total')} {c.get('currency')}")
    except MuError as e:
        sys.exit(f"✗ {e}")

if __name__ == "__main__": main()
