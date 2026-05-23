#!/usr/bin/env python3
"""Generate 3 brand-mood lifestyle hero photos per brand.

For brand LP hero on wearmu.com/<brand>. Stored as
store/static/<brand>/lifestyle/lifestyle_NN.png (actually JPEG, .png ext
preserved for compat with existing brands).

Usage:
    python3 scripts/gen_brand_hero_lifestyle.py jiuflow kokon roll
"""
from __future__ import annotations
import base64
import json
import os
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY:
    sys.exit("GEMINI_API_KEY missing (source /Users/yuki/.env)")

ROOT = Path(__file__).resolve().parent.parent
STATIC = ROOT / "store" / "static"
MODEL = "gemini-3-pro-image-preview"

PROMPTS = {
    "jiuflow": [
        "Editorial portrait, Japanese man in his late 20s in a BJJ academy lobby, "
        "wearing a black Jiu-Jitsu rashguard tee, holding a folded white gi over "
        "one arm. Soft afternoon natural light through a window, wood floor, "
        "subtle out-of-focus mat texture behind him. Photojournalistic 35mm, slightly "
        "desaturated, calm confidence. No logo on tee. 1024x1024 photographic PNG.",
        "Wide environmental shot inside a Tokyo BJJ dojo, two athletes in rashguards "
        "and shorts mid-roll on blue mats, motion blur on the limbs, sharp on the "
        "back rider's face. Warm tungsten light from above, banners softly out of "
        "focus on the wall. 35mm reportage, color graded for muted blacks. "
        "1024x1024 photographic PNG.",
        "Close-up flatlay on dark wooden floor, neatly folded black BJJ tee next "
        "to a worn brown leather belt-end (not gi belt), a stopwatch, a coffee cup, "
        "and a wood-handled brush. Top-down 90deg angle, single soft window light "
        "from upper left, deep shadows, magazine-grade composition. 1024x1024 PNG."
    ],
    "kokon": [
        "Editorial interior photograph of a refined Tokyo yakiniku restaurant "
        "(focal: Nishiazabu Kokon vibe), close-up of marbled wagyu beef slices "
        "on a small brass tray, dim warm tungsten light, white smoke rising from "
        "a charcoal grill in soft-focus background. Earthy color palette, deep blacks. "
        "35mm food editorial. 1024x1024 photographic PNG, no text.",
        "Japanese chef in a black apron with subtle gold thread (no logo), backlit "
        "by the soft red glow of a binchotan charcoal grill. He's in profile, mid-30s, "
        "calm expression, holding tongs. Cinematic moody lighting, depth-of-field, "
        "rim light on his shoulder. 1024x1024 photographic PNG.",
        "Top-down composition: a worn dark wood table set with small ceramic dishes "
        "of yakiniku tare sauces, sesame, salt flakes, fresh wasabi, and a black tee "
        "(folded neatly to one corner) as if a bartender draped it. Warm low-key "
        "light from above. Magazine still-life style. 1024x1024 PNG."
    ],
    "roll": [
        "Two BJJ athletes mid-roll on royal blue mats, one in a black rashguard, "
        "the other in white, dynamic motion blur on the sweep transition, sharp on "
        "the top rider's expression of focus. Side lighting from large dojo window, "
        "natural color. Photojournalistic 35mm. 1024x1024 photographic PNG.",
        "Close-up of two interlocked grips on a gi sleeve and collar, hands "
        "calloused and chalked, mid-grip-fight, white gi against navy rashguard. "
        "Slight motion, shallow depth-of-field, dramatic side light. Editorial 1024x1024 PNG.",
        "Wide low-angle shot of a single athlete sitting on the mat catching breath "
        "after a roll, towel over shoulder, rashguard slightly damp, looking off-camera. "
        "Empty dojo behind him in soft focus, warm afternoon light from skylight. "
        "Color: desaturated greens and blues. 1024x1024 photographic PNG."
    ],
}


def gen_one(prompt: str, out_path: Path) -> bool:
    url = f"https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:generateContent?key={KEY}"
    body = json.dumps({
        "contents": [{"parts": [{"text": prompt}]}],
        "generationConfig": {
            "responseModalities": ["IMAGE", "TEXT"],
            "temperature": 0.9,
        }
    }).encode()
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            raw = r.read()
    except urllib.error.HTTPError as e:
        print(f"  [http {e.code}] {out_path.name}: {e.read()[:200].decode(errors='replace')}")
        return False
    except Exception as e:
        print(f"  [err] {out_path.name}: {e}")
        return False
    j = json.loads(raw)
    for cand in j.get("candidates", []):
        for part in cand.get("content", {}).get("parts", []):
            d = part.get("inlineData") or part.get("inline_data")
            if d and d.get("data"):
                png = base64.b64decode(d["data"])
                out_path.parent.mkdir(parents=True, exist_ok=True)
                out_path.write_bytes(png)
                print(f"  ✓ {out_path.name} ({len(png):,}B)")
                return True
    print(f"  [empty] {out_path.name}: resp keys={list(j.keys())}")
    return False


def main():
    brands = sys.argv[1:] or ["jiuflow", "kokon", "roll"]
    started = time.time()
    ok = fail = 0
    for brand in brands:
        if brand not in PROMPTS:
            print(f"!! no prompt set for {brand} — skipping")
            continue
        outdir = STATIC / brand / "lifestyle"
        for i, prompt in enumerate(PROMPTS[brand], start=1):
            out = outdir / f"lifestyle_{i:02d}.png"
            if out.exists() and out.stat().st_size > 50_000:
                print(f"  - {out.name} already exists, skip")
                ok += 1
                continue
            if gen_one(prompt, out):
                ok += 1
            else:
                fail += 1
            time.sleep(2)
    print(f"\ndone. ok={ok} fail={fail} elapsed={time.time()-started:.0f}s")


if __name__ == "__main__":
    main()
