#!/usr/bin/env python3
"""MU × ELE × POTE — round 2 design: 2匹が寝てる (sleeping duo)."""
import os, sys, pathlib
from google import genai
from google.genai import types

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY: sys.exit("GEMINI_API_KEY not set")
OUT = pathlib.Path("/tmp/elepote"); OUT.mkdir(parents=True, exist_ok=True)

REFS = [
    pathlib.Path("/Users/yuki/workspace/ele-blog/images/ele-1.jpg"),
    pathlib.Path("/Users/yuki/Downloads/S__12755044.jpg"),  # sleepy pote
]

STYLE = (
    "Clean modern editorial illustration, confident monoline + soft flat fills, "
    "one or two warm accent colors max, no photo-realism. Iconic at chest-print "
    "size. Centered with generous margin on PURE SOLID WHITE (#FFFFFF) background, "
    "no shadow, no frame, no text."
)

JOBS = {
    "sleep_char": {
        "refs": REFS,
        "prompt": (
            "Create an illustrated HERO of the SAME two puppy mascots — ELE the "
            "small white fluffy Bichon-Poodle mix and POTE the blue-and-tan French "
            "Bulldog puppy (blue/silver coat + tan markings above eyes / on muzzle "
            "/ on chest, bat-ears) — but now they are SLEEPING CURLED UP TOGETHER, "
            "side by side, peaceful little smiles, eyes closed, tiny 'zzz' marks "
            "floating above them in soft warm peach. Composition: horizontal, "
            "balanced, both at the same scale, touching warmly. " + STYLE),
    },
    "sleep_tee": {
        "refs": [],  # text-only, no ref needed
        "prompt": (
            "Flat-lay product photograph, top-down, of a SINGLE WHITE heather "
            "unisex t-shirt laid flat on a soft cream / pale-peach background "
            "with gentle natural shadow. Centered on the chest, print a small "
            "illustration of TWO puppy mascots sleeping curled up together — a "
            "fluffy white Bichon-Poodle mix and a blue-and-tan French Bulldog "
            "puppy — with little 'zzz' marks above, single accent peach color. "
            "Premium minimal aesthetic, no other text, no labels, no props."),
    },
}

def gen(client, prompt, refs):
    parts = [types.Part.from_text(text=prompt)]
    for r in refs:
        if r.exists():
            mime = "image/jpeg" if r.suffix.lower() in (".jpg",".jpeg") else "image/png"
            parts.append(types.Part.from_bytes(data=r.read_bytes(), mime_type=mime))
    resp = client.models.generate_content(
        model="gemini-3-pro-image-preview",
        contents=[types.Content(role="user", parts=parts)],
        config=types.GenerateContentConfig(response_modalities=["IMAGE","TEXT"]))
    for c in resp.candidates or []:
        for p in (c.content.parts or []):
            if getattr(p,"inline_data",None) and p.inline_data.data:
                return p.inline_data.data
    return None

def main():
    client = genai.Client(api_key=KEY)
    for name, job in JOBS.items():
        print(f"[sleep] {name} …")
        data = gen(client, job["prompt"], job["refs"])
        if not data: print("  NO IMAGE"); continue
        (OUT / f"{name}.png").write_bytes(data)
        print(f"  saved {OUT/name}.png ({len(data)//1024} KB)")

if __name__ == "__main__":
    main()
