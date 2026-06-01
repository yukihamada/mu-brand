"""`mu-batch` — parallel multi-product MU drop from a briefs.json."""
import sys, json, time
from concurrent.futures import ThreadPoolExecutor
from .client import MuClient, MuError

def main():
    args = sys.argv[1:]
    gen_only = "--gen-only" in args; transparent = "--transparent" in args
    workers = int(args[args.index("--workers")+1]) if "--workers" in args else 6
    path = next((a for a in args if not a.startswith("--") and a != str(workers)), None)
    if not path: sys.exit("usage: mu-batch briefs.json [--gen-only] [--workers N] [--transparent]")
    briefs = json.load(open(path)); mu = MuClient()
    def make(b):
        try:
            png = mu.gen_design(b["prompt"]);  png = mu.to_transparent(png) if transparent else png
            return b, mu.host_image(png), None
        except Exception as e: return b, None, str(e)[:90]
    t0 = time.time(); print(f"⚡ generating {len(briefs)} in parallel (workers={workers})…")
    with ThreadPoolExecutor(max_workers=workers) as ex: results = list(ex.map(make, briefs))
    print(f"   gen+host {time.time()-t0:.1f}s")
    if gen_only:
        for b, u, e in results: print(f"  {b['label']:14s} -> {u or 'FAIL: '+str(e)}")
        return
    live = 0
    for b, u, e in results:
        if not u: print(f"✗ {b['label']}: {e}"); continue
        try:
            r = mu.create_product(b.get("store","mu-lab"), b["label"], b.get("description",b["label"]), b["kind"], design_url=u)
            mu.approve(r["sku"]); live += 1; print(f"✔ {b['kind']:9s} {r['sku']}  {b['label']}")
        except MuError as ex: print(f"✗ {b['label']}: {ex}")
    print(f"\n{live}/{len(briefs)} live · {time.time()-t0:.1f}s")

if __name__ == "__main__": main()
