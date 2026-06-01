#!/usr/bin/env python3
"""mu_batch — fast PARALLEL multi-product MU drop (free design_url path).

Thin CLI over muclient.MuClient. Generates all designs concurrently, then
creates + approves. N designs take ~max(one), not ~sum(N).

  scripts/mu_batch.py briefs.json [--gen-only] [--workers 6] [--transparent]

briefs.json = [{"kind":"tee","label":"月","description":"...","prompt":"..."}, ...]
--transparent : white-ink-on-transparent (floats on black tees, no white panel)
Needs GEMINI_API_KEY (source /Users/yuki/.env). Creds from .secrets.local.
"""
import sys, os, json, time
from concurrent.futures import ThreadPoolExecutor
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from muclient import MuClient, MuError


def main():
    args = sys.argv[1:]
    gen_only = "--gen-only" in args
    transparent = "--transparent" in args
    workers = int(args[args.index("--workers") + 1]) if "--workers" in args else 6
    path = next((a for a in args if not a.startswith("--")
                 and a not in (str(workers),)), None)
    if not path:
        sys.exit("usage: mu_batch.py briefs.json [--gen-only] [--workers N] [--transparent]")
    briefs = json.load(open(path))
    mu = MuClient()

    def make_design(b):
        try:
            png = mu.gen_design(b["prompt"])
            if transparent:
                png = mu.to_transparent(png)
            return b, mu.host_image(png), None
        except Exception as e:
            return b, None, str(e)[:90]

    t0 = time.time()
    print(f"⚡ generating {len(briefs)} designs in parallel (workers={workers}, transparent={transparent})…")
    with ThreadPoolExecutor(max_workers=workers) as ex:
        results = list(ex.map(make_design, briefs))
    print(f"   gen+host done in {time.time()-t0:.1f}s")

    if gen_only:
        for b, url, err in results:
            print(f"  {b['label']:14s} -> {url or 'FAIL: ' + str(err)}")
        return

    live = 0
    for b, url, err in results:
        if not url:
            print(f"✗ {b['label']}: {err}"); continue
        try:
            r = mu.create_product(b.get("store", "mu-lab"), b["label"],
                                  b.get("description", b["label"]), b["kind"], design_url=url)
            mu.approve(r["sku"]); live += 1
            print(f"✔ {b['kind']:9s} {r['sku']}  {b['label']}")
        except MuError as e:
            print(f"✗ {b['label']}: {e}")
    print(f"\n{live}/{len(briefs)} live · total {time.time()-t0:.1f}s")


if __name__ == "__main__":
    main()
