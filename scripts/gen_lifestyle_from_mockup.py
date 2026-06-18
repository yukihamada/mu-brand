#!/usr/bin/env python3
"""Generate lifestyle/wearing photos from existing Printful mockups.

Input:  product_id (uses https://wearmu.com/api/products/<id>/mockup.png as base)
Output: store/static/jiufight/lifestyle/<id>.png — model wearing the same tee
        in a BJJ venue / casual setting

Usage:
    python3 scripts/gen_lifestyle_from_mockup.py 379 374 379 1028 1029
"""
import os, sys, json, base64, urllib.request, urllib.error, subprocess
from pathlib import Path

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY:
    sys.exit("GEMINI_API_KEY missing (source /Users/yuki/.env)")

OUT = Path("/Users/yuki/workspace/mu-brand/store/static/jiufight/lifestyle")
OUT.mkdir(parents=True, exist_ok=True)

LIFESTYLE_PROMPT = """Reference image: a flat-lay product photo of a black Bella+Canvas 3001
T-shirt with a printed design on the chest.

Generate an editorial lifestyle photograph: a Japanese man in his 20s-30s
wearing the EXACT SAME T-shirt with the EXACT SAME chest design, standing
casually in a martial-arts gym/dojo lobby just after training. Natural
afternoon light, slightly desaturated color, photojournalistic, 35mm lens
look. He's relaxed, mid-conversation, holding a Gi over one arm. Background
is softly out of focus — wood floor, mat texture, a hint of a banner.

Critical: keep the T-shirt design IDENTICAL in shape, size, position, and
ink color to the reference. Do not redesign. Composition is portrait-oriented
3:4, subject occupies center-right. Output 1024×1024 photographic PNG.
"""

def fetch_mockup(product_id: int) -> bytes:
    url = f"https://wearmu.com/api/products/{product_id}/mockup.png"
    with urllib.request.urlopen(url, timeout=30) as r:
        if r.status != 200:
            raise SystemExit(f"id={product_id}: mockup fetch failed {r.status}")
        return r.read()

def generate(product_id: int) -> bool:
    out_path = OUT / f"{product_id}.png"
    if out_path.exists():
        print(f"  - {product_id}: already exists ({out_path.stat().st_size:,}B), skipping")
        return True
    try:
        mockup_bytes = fetch_mockup(product_id)
    except Exception as e:
        print(f"  [fetch err] {product_id}: {e}")
        return False
    url = f"https://generativelanguage.googleapis.com/v1beta/models/gemini-3-pro-image-preview:generateContent?key={KEY}"
    body = json.dumps({
        "contents": [{
            "parts": [
                {"text": LIFESTYLE_PROMPT},
                {"inlineData": {
                    "mimeType": "image/png",
                    "data": base64.b64encode(mockup_bytes).decode()
                }}
            ]
        }],
        "generationConfig": {"responseModalities": ["IMAGE", "TEXT"]}
    }).encode()
    req = urllib.request.Request(url, data=body, headers={"content-type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=180) as r:
            j = json.loads(r.read())
    except urllib.error.HTTPError as e:
        msg = e.read().decode(errors='replace')[:400]
        print(f"  [HTTP {e.code}] {product_id}: {msg}")
        return False
    except Exception as e:
        print(f"  [err] {product_id}: {e}")
        return False
    parts = j.get("candidates", [{}])[0].get("content", {}).get("parts", [])
    for p in parts:
        d = p.get("inlineData") or p.get("inline_data")
        if d and d.get("data"):
            png = base64.b64decode(d["data"])
            out_path.write_bytes(png)
            print(f"  ✓ {product_id} → {out_path} ({len(png):,}B)")
            return True
    print(f"  [empty] {product_id}: no inline_data; resp keys={list(j.keys())}")
    return False

if __name__ == "__main__":
    ids = sys.argv[1:] or ["379", "374", "1028", "1029"]
    print(f"generating lifestyle for {len(ids)} products...")
    successes = []
    for pid in ids:
        if generate(int(pid)):
            successes.append(OUT / f"{pid}.png")
    print(f"\ndone. {len(successes)}/{len(ids)} succeeded.")
    if successes:
        subprocess.run(["open"] + [str(p) for p in successes])
