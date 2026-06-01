#!/usr/bin/env python3
"""mu_drop — one command: AI-generate an MU product → approve it live →
(optionally) order a physical sample shipped to your home.

This is the "shortest path" wrapper around the wearmu.com agent API + MA-council
approval + Printful sample order that we ran by hand once. Run it and you get a
live, buyable SKU (and a real shirt on the way) in a single invocation.

Usage:
  scripts/mu_drop.py "<design brief>" [--kind tee] [--size L] [--label "月"] \
      [--store mu-lab] [--price 4900] [--ship] [--draft]

  --ship   also place a CONFIRMED Printful order to the SHIP_* address
  --draft  with --ship, stop at a Printful draft (validate address + cost, no charge)

Secrets live in mu-brand/.secrets.local (gitignored — never committed):
  MU_AGENT_KEY=...        # agent API key (rotates if you re-verify; refresh then)
  MU_ADMIN_TOKEN=...      # optional — else fetched via `fly ssh ... printenv`
  PRINTFUL_API_KEY=...    # optional (only for --ship) — else via fly ssh
  SHIP_NAME=Yuki Hamada   # only for --ship
  SHIP_ADDR1=...
  SHIP_CITY=Minato-ku
  SHIP_STATE=13           # Printful state_code (Tokyo=13)
  SHIP_ZIP=108-0073
  SHIP_COUNTRY=JP
Missing MU_AGENT_KEY falls back to the `mu` server key in ~/.claude.json.
"""
import argparse, json, os, re, subprocess, sys, urllib.request, urllib.error

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SECRETS = os.path.join(ROOT, ".secrets.local")
BASE = "https://wearmu.com"
FLY_APP = "mu-store"

# kind -> Printful catalog product id (for --ship variant lookup)
KIND_PRODUCT = {"tee": 71, "hoodie": 146, "crewneck": 145}


def load_secrets():
    s = {}
    if os.path.exists(SECRETS):
        for line in open(SECRETS):
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, v = line.split("=", 1)
                s[k.strip()] = v.strip()
    return s


def claude_agent_key():
    p = os.path.expanduser("~/.claude.json")
    if os.path.exists(p):
        m = re.search(r'Bearer ([0-9a-f]{16,})', open(p).read())
        if m:
            return m.group(1)
    return None


def fly_env(name):
    """Read a secret off the running Fly machine (no local storage)."""
    tok = None
    cfg = os.path.expanduser("~/.fly/config.yml")
    if os.path.exists(cfg):
        m = re.search(r'access_token:\s*(\S+)', open(cfg).read())
        if m:
            tok = m.group(1)
    env = dict(os.environ)
    if tok:
        env["FLY_API_TOKEN"] = tok
    try:
        out = subprocess.run(
            ["fly", "ssh", "console", "-a", FLY_APP, "-C", f"printenv {name}"],
            capture_output=True, text=True, env=env, timeout=60).stdout
        # fly prepends a "Connecting to <ip>..." line; pick the last line that
        # looks like a bare secret token (no spaces, long enough).
        toks = [ln.strip() for ln in out.replace("\r", "").splitlines()
                if re.fullmatch(r"[A-Za-z0-9_\-]{20,}", ln.strip())]
        return toks[-1] if toks else None
    except Exception:
        return None


def api(method, path, token=None, body=None, base=BASE, timeout=130):
    url = path if path.startswith("http") else base + path
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("Authorization", "Bearer " + token)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, json.loads(r.read().decode() or "{}")
    except urllib.error.HTTPError as e:
        try:
            return e.code, json.loads(e.read().decode() or "{}")
        except Exception:
            return e.code, {}


def die(msg):
    print("✗ " + msg, file=sys.stderr)
    sys.exit(1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("prompt", help="AI design brief (becomes ai_prompt)")
    ap.add_argument("--kind", default="tee", choices=list(KIND_PRODUCT) + ["rashguard_ls", "rashguard_black"])
    ap.add_argument("--size", default="L")
    ap.add_argument("--label", default=None)
    ap.add_argument("--store", default="mu-lab")
    ap.add_argument("--price", type=int, default=None)
    ap.add_argument("--ship", action="store_true")
    ap.add_argument("--draft", action="store_true", help="with --ship: validate only, no charge")
    a = ap.parse_args()

    s = load_secrets()
    key = s.get("MU_AGENT_KEY") or claude_agent_key()
    if not key:
        die("no MU_AGENT_KEY in .secrets.local and no `mu` key in ~/.claude.json")
    label = a.label or a.prompt[:60]

    # 1. create (AI-gen)
    print(f"① AI生成で作成中… ({a.kind}, ¥{'floor' if not a.price else a.price})")
    st, r = api("POST", "/api/agent/products", key, {
        "store": a.store, "label": label, "description": label,
        "kind": a.kind, "ai_prompt": a.prompt,
        **({"price_jpy": a.price} if a.price else {}),
    })
    if st == 401:
        die("MU_AGENT_KEY invalid/expired. Re-verify and update .secrets.local "
            "(register→verify gives a new key; it rotates each time).")
    if st != 200 or not r.get("ok"):
        die(f"create failed [{st}]: {r}")
    sku = r["sku"]
    print(f"   ✔ {sku}  ({r['pdp_url']})")

    # 2. approve → live  (ADMIN_TOKEN)
    admin = s.get("MU_ADMIN_TOKEN") or fly_env("ADMIN_TOKEN")
    if not admin:
        die("no MU_ADMIN_TOKEN (and fly ssh fallback failed) — cannot approve")
    print("② 承認 (review→live)…")
    st, r = api("POST", f"/api/ma/review/{sku}/approve", admin)
    if st != 200 or not r.get("ok"):
        die(f"approve failed [{st}]: {r}")
    print(f"   ✔ LIVE → {BASE}/shop/{sku}")

    if not a.ship:
        print(f"\n完了。 buyable: {BASE}/shop/{sku}")
        return

    # 3. ship a sample (Printful)
    if a.kind not in KIND_PRODUCT:
        die(f"--ship not supported for kind={a.kind} (no product map)")
    pf = s.get("PRINTFUL_API_KEY") or fly_env("PRINTFUL_API_KEY")
    if not pf:
        die("no PRINTFUL_API_KEY — cannot ship")
    need = ["SHIP_NAME", "SHIP_ADDR1", "SHIP_CITY", "SHIP_STATE", "SHIP_ZIP", "SHIP_COUNTRY"]
    if any(k not in s for k in need):
        die("missing SHIP_* in .secrets.local: " + ", ".join(k for k in need if k not in s))

    # design file = the live PDP's agent artwork
    import urllib.request as u
    html = u.urlopen(f"{BASE}/shop/{sku}", timeout=30).read().decode()
    m = re.search(r'https://mockups\.wearmu\.com/catalog/agent/[^"\')]+\.png', html)
    if not m:
        die("could not resolve design_file from PDP")
    design = m.group(0)

    # Black + size variant of the kind's Printful product
    pid = KIND_PRODUCT[a.kind]
    st, pr = api("GET", f"https://api.printful.com/products/{pid}", pf, timeout=30)
    variant = next((v["id"] for v in pr["result"]["variants"]
                    if v.get("size") == a.size and v.get("color") == "Black"), None)
    if not variant:
        die(f"no Black/{a.size} variant for Printful product {pid}")

    order = {
        "recipient": {
            "name": s["SHIP_NAME"], "address1": s["SHIP_ADDR1"], "city": s["SHIP_CITY"],
            "state_code": s["SHIP_STATE"], "zip": s["SHIP_ZIP"], "country_code": s["SHIP_COUNTRY"],
        },
        "items": [{"variant_id": variant, "quantity": 1,
                   "name": f"{label} / Black / {a.size}",
                   "files": [{"type": "front", "url": design}]}],
    }
    confirm = "false" if a.draft else "true"
    print(f"③ Printful発注 (confirm={confirm}) → {s['SHIP_NAME']} / {s['SHIP_CITY']}…")
    st, od = api("POST", f"https://api.printful.com/orders?confirm={confirm}",
                 pf, order, timeout=60)
    if st != 200:
        die(f"order failed [{st}]: {json.dumps(od.get('result'), ensure_ascii=False)[:300]}")
    o = od["result"]
    c = o.get("costs", {})
    print(f"   ✔ order #{o['id']}  status={o['status']}  "
          f"total ¥{c.get('total')} {c.get('currency')}")
    print(f"   dashboard: https://www.printful.com/dashboard/order/{o['id']}")
    print(f"\n完了。 live={BASE}/shop/{sku}  order=#{o['id']}")


if __name__ == "__main__":
    main()
