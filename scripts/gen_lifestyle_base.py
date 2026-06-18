#!/usr/bin/env python3
"""Generate BLANK worn-garment base photos for the lifestyle composite pipeline.

The whole point: the person wears a SOLID, PRINT-FREE garment whose chest faces
the camera straight-on. We later composite the *real* transparent design PNG onto
the chest box (luminance-multiplied so it reads as printed, not pasted). Because
the base has no graphic, there is nothing for the model to "redraw" → zero design
drift, unlike the old Gemini-redrawn lifestyle photos.

Output: store/static/lifestyle_base/{kind}_{n}.png  (4:5 portrait)
"""
import os, sys, json, base64, urllib.request, urllib.error
from pathlib import Path

# load ~/.env for GEMINI_API_KEY
envf = Path.home() / ".env"
if envf.exists():
    for line in envf.read_text().splitlines():
        if "=" in line and not line.strip().startswith("#"):
            k, _, v = line.partition("=")
            os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY:
    sys.exit("GEMINI_API_KEY missing")

OUT = Path(__file__).resolve().parent.parent / "store" / "static" / "lifestyle_base"
OUT.mkdir(parents=True, exist_ok=True)

MODEL = "gemini-3-pro-image-preview"

COMMON = (
    "Photorealistic editorial lifestyle photograph, 4:5 portrait, shot on Sony A7IV 35mm f/2.0, "
    "soft natural window light, slight film grain, magazine-cover quality. "
    "A Japanese person in their late 20s. "
    "CRITICAL framing: the torso faces the camera straight-on; the chest is flat, centered, and "
    "completely unobstructed (no crossed arms, no bag straps, no hands over the chest). "
    "The garment is a SOLID PLAIN BLACK {garment} with absolutely NO print, NO graphic, NO logo, "
    "NO text, NO pocket on the chest — a totally blank black {garment}. "
    "Hide the face: crop the frame at the chin or turn the head, so no face is visible. "
    "No watermark, no added text overlay. The blank chest area should occupy the central third of the frame."
)

SCENES = {
    "tee": [
        "Subject standing relaxed in a bright Tokyo BJJ dojo lobby with tatami mats softly out of focus behind.",
        "Subject seated upright at a wooden cafe table, hands resting on the table away from the chest, minimal Daikanyama coffee bar behind.",
        "Subject standing against a pale concrete wall in a minimal Aoyama studio, calm editorial mood.",
    ],
    "hoodie": [
        "Subject standing on a quiet Tokyo side street at golden hour, hood DOWN, chest facing camera.",
        "Subject seated upright on a wooden bench, hood down, soft afternoon light, mat texture behind.",
    ],
    "crewneck": [
        "Subject standing in a minimal home office, dark walnut shelves softly blurred behind, calm light.",
        "Subject standing against a warm off-white wall, Kinfolk editorial mood, chest facing camera.",
    ],
}

GARMENT = {"tee": "crew-neck T-shirt", "hoodie": "pullover hoodie", "crewneck": "crewneck sweatshirt"}


def gen(kind: str, idx: int, scene: str) -> bool:
    out = OUT / f"{kind}_{idx}.png"
    if out.exists():
        print(f"  skip {out.name} (exists)")
        return True
    prompt = COMMON.format(garment=GARMENT[kind]) + " Scene: " + scene
    url = f"https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:generateContent?key={KEY}"
    body = json.dumps({
        "contents": [{"parts": [{"text": prompt}]}],
        "generationConfig": {"responseModalities": ["IMAGE", "TEXT"]},
    }).encode()
    req = urllib.request.Request(url, data=body, headers={"content-type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=180) as r:
            j = json.loads(r.read())
    except urllib.error.HTTPError as e:
        print(f"  [HTTP {e.code}] {kind}_{idx}: {e.read().decode(errors='replace')[:300]}")
        return False
    parts = j.get("candidates", [{}])[0].get("content", {}).get("parts", [])
    for p in parts:
        d = p.get("inlineData") or p.get("inline_data")
        if d and d.get("data"):
            out.write_bytes(base64.b64decode(d["data"]))
            print(f"  OK {out.name} ({out.stat().st_size:,}B)")
            return True
    print(f"  [empty] {kind}_{idx}: {json.dumps(j)[:200]}")
    return False


if __name__ == "__main__":
    only = sys.argv[1] if len(sys.argv) > 1 else None
    n = 0
    for kind, scenes in SCENES.items():
        if only and kind != only:
            continue
        for i, sc in enumerate(scenes, 1):
            if gen(kind, i, sc):
                n += 1
    print(f"done. {n} base photos in {OUT}")
