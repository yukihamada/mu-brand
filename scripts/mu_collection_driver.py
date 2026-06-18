#!/usr/bin/env python3
"""MU collection driver — create themed products via the agent API and report
sku + status + design image URL. Reads MU_AGENT_API_KEY from .secrets.local.

Usage:
  mu_collection_driver.py create <json-spec-file>   # batch create from JSON list
  mu_collection_driver.py list                       # list my products (sku,kind,status,price,pdp,design)
"""
import json, os, re, sys, time, urllib.request, urllib.error

BASE = "https://wearmu.com"
SEC = os.path.join(os.path.dirname(__file__), "..", ".secrets.local")

def key():
    for ln in open(SEC):
        m = re.match(r'^MU_AGENT_API_KEY=(.+)$', ln.strip())
        if m: return m.group(1).strip().strip('"').strip("'")
    sys.exit("no MU_AGENT_API_KEY")

def call(method, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(BASE+path, data=data, method=method,
        headers={"Authorization": "Bearer "+key(), "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=180) as r:
            return r.status, json.loads(r.read().decode())
    except urllib.error.HTTPError as e:
        try: return e.code, json.loads(e.read().decode())
        except Exception: return e.code, {"error": "http "+str(e.code)}

def create_batch(specs):
    out = []
    for s in specs:
        st, resp = call("POST", "/api/agent/products", s)
        row = {"store": s["store"], "label": s["label"], "kind": s["kind"],
               "http": st, "sku": resp.get("sku"), "status": resp.get("status"),
               "risk": resp.get("risk"), "pdp": resp.get("pdp_url"),
               "error": resp.get("error")}
        out.append(row)
        print(f"  [{st}] {s['store']}/{s['kind']:8} {s['label'][:34]:34} -> {resp.get('sku') or resp.get('error')} ({resp.get('status')})")
        time.sleep(1.5)  # be gentle on Gemini gen
    return out

def list_mine():
    st, resp = call("GET", "/api/agent/products")
    items = resp if isinstance(resp, list) else resp.get("products", resp.get("items", []))
    return items

if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "list"
    if cmd == "create":
        specs = json.load(open(sys.argv[2]))
        res = create_batch(specs)
        json.dump(res, open("/tmp/mu_create_result.json","w"), ensure_ascii=False, indent=2)
        print("\nsaved /tmp/mu_create_result.json")
    elif cmd == "list":
        items = list_mine()
        for it in items:
            print(json.dumps({k: it.get(k) for k in ("sku","store","kind","label","status","retail_price_jpy","design_file","pdp_url")}, ensure_ascii=False))
