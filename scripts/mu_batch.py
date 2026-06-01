#!/usr/bin/env python3
"""mu_batch — fast, PARALLEL multi-product MU drop (free design_url path).

Speed wins vs the one-at-a-time flow:
  • Reads creds from .secrets.local (no per-call `fly ssh` round-trips).
  • Generates all designs CONCURRENTLY with Gemini (ThreadPoolExecutor),
    then uploads + creates + approves. N designs take ~max(one) not ~sum(N).

Usage:
  scripts/mu_batch.py briefs.json            # create + approve
  scripts/mu_batch.py briefs.json --gen-only # just generate+host (timing/no DB)
  scripts/mu_batch.py briefs.json --workers 8

briefs.json = [{"kind":"tee","label":"月","description":"...","prompt":"..."}, ...]
Needs GEMINI_API_KEY in env (source /Users/yuki/.env). MU_AGENT_KEY +
MU_ADMIN_TOKEN come from .secrets.local (fall back to ~/.claude.json / fly).
"""
import os, sys, json, base64, time, tempfile, subprocess, urllib.request, urllib.error
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BASE = "https://wearmu.com"
GMODEL = "gemini-3-pro-image-preview"


def secrets():
    s = {}
    p = os.path.join(ROOT, ".secrets.local")
    if os.path.exists(p):
        for ln in open(p):
            ln = ln.strip()
            if ln and not ln.startswith("#") and "=" in ln:
                k, v = ln.split("=", 1); s[k.strip()] = v.strip()
    return s


def http(method, url, token=None, body=None, t=180):
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(url, data=data, method=method)
    r.add_header("Content-Type", "application/json")
    if token:
        r.add_header("Authorization", "Bearer " + token)
    try:
        with urllib.request.urlopen(r, timeout=t) as x:
            return x.status, json.loads(x.read().decode() or "{}")
    except urllib.error.HTTPError as e:
        try:
            return e.code, json.loads(e.read().decode() or "{}")
        except Exception:
            return e.code, {}


def gen_and_host(brief, gk):
    """Gemini image -> catbox https url. Returns (brief, url|None, err)."""
    url = (f"https://generativelanguage.googleapis.com/v1beta/models/"
           f"{GMODEL}:generateContent?key={gk}")
    body = {"contents": [{"parts": [{"text": brief["prompt"]}]}],
            "generationConfig": {"responseModalities": ["IMAGE", "TEXT"]}}
    try:
        st, r = http("POST", url, None, body, t=180)
        png = None
        for c in r.get("candidates", []):
            for p in c.get("content", {}).get("parts", []):
                if "inlineData" in p:
                    png = base64.b64decode(p["inlineData"]["data"])
        if not png:
            return brief, None, "no image"
        f = tempfile.NamedTemporaryFile(suffix=".png", delete=False); f.write(png); f.close()
        out = subprocess.run(["curl", "-s", "-m", "60", "-F", "reqtype=fileupload",
                              "-F", f"fileToUpload=@{f.name}", "https://catbox.moe/user/api.php"],
                             capture_output=True, text=True).stdout.strip()
        os.unlink(f.name)
        return (brief, out, None) if out.startswith("http") else (brief, None, "host fail")
    except Exception as e:
        return brief, None, str(e)[:80]


def main():
    args = sys.argv[1:]
    gen_only = "--gen-only" in args
    workers = 6
    if "--workers" in args:
        workers = int(args[args.index("--workers") + 1])
    path = next((a for a in args if not a.startswith("--") and a != str(workers)), None)
    if not path:
        sys.exit("usage: mu_batch.py briefs.json [--gen-only] [--workers N]")
    briefs = json.load(open(path))
    gk = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
    if not gk:
        sys.exit("GEMINI_API_KEY not set (source /Users/yuki/.env)")
    s = secrets()
    key = s.get("MU_AGENT_KEY"); admin = s.get("MU_ADMIN_TOKEN")

    t0 = time.time()
    print(f"⚡ generating {len(briefs)} designs in parallel (workers={workers})…")
    with ThreadPoolExecutor(max_workers=workers) as ex:
        results = list(ex.map(lambda b: gen_and_host(b, gk), briefs))
    print(f"   generation+host done in {time.time()-t0:.1f}s")

    if gen_only:
        for b, url, err in results:
            print(f"  {b['label']:12s} -> {url or 'FAIL: '+str(err)}")
        return

    live = 0
    for b, url, err in results:
        if not url:
            print(f"✗ {b['label']}: gen/host failed ({err})"); continue
        st, r = http("POST", BASE + "/api/agent/products", key, {
            "store": b.get("store", "mu-lab"), "label": b["label"],
            "description": b.get("description", b["label"]),
            "kind": b["kind"], "design_url": url})
        if st != 200 or not r.get("ok"):
            print(f"✗ {b['label']}: create [{st}] {r.get('error', r)}"); continue
        sku = r["sku"]
        if admin:
            http("POST", f"{BASE}/api/ma/review/{sku}/approve", admin)
        live += 1
        print(f"✔ {b['kind']:9s} {sku}  {b['label']}")
    print(f"\n{live}/{len(briefs)} live · total {time.time()-t0:.1f}s")


if __name__ == "__main__":
    main()
