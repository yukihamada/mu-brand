#!/usr/bin/env python3
"""MU × ELE × POTE — product mockups for the LP / shop cards.

Image-conditioned via gemini-3-pro-image-preview: feeds the character art
back in as a reference so the chest print stays on-character.

Out: /tmp/elepote/{duo,ele,pote}_tee.png, duo_hoodie.png
"""
import os, sys, pathlib
from google import genai
from google.genai import types

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY: sys.exit("GEMINI_API_KEY not set")

CHARS = pathlib.Path("/tmp/elepote")
OUT = CHARS  # same dir

JOBS = {
    "duo_tee": {
        "ref": "duo_char.png",
        "prompt": (
            "Flat-lay product photograph, top-down, of a SINGLE WHITE heather "
            "unisex t-shirt laid flat on a soft cream / pale-peach fabric "
            "background with gentle natural shadow. Centered on the chest, "
            "print THIS exact illustration as the only graphic — two best-friend "
            "puppy mascots (white fluffy Bichon-Poo on the left + blue-and-tan "
            "French Bulldog with little pink tongue on the right, sitting side by "
            "side). Premium minimal aesthetic, no other text, no labels, no props."),
    },
    "ele_tee": {
        "ref": "ele_char.png",
        "prompt": (
            "Flat-lay product photograph, top-down, of a SINGLE WHITE heather "
            "unisex t-shirt on a soft cream background, gentle natural shadow. "
            "Centered on the chest, print THIS exact illustration as the only "
            "graphic — a small fluffy white Bichon-Poodle mix puppy mascot "
            "sitting front view. Premium minimal aesthetic, no text, no labels, "
            "no props."),
    },
    "pote_tee": {
        "ref": "pote_char.png",
        "prompt": (
            "Flat-lay product photograph, top-down, of a SINGLE WHITE heather "
            "unisex t-shirt on a soft cream background, gentle natural shadow. "
            "Centered on the chest, print THIS exact illustration as the only "
            "graphic — a blue-and-tan French Bulldog puppy mascot sitting front "
            "view with a little pink tongue. Premium minimal, no text, no props."),
    },
    "duo_hoodie": {
        "ref": "duo_char.png",
        "prompt": (
            "Flat-lay product photograph, top-down, of a SINGLE WHITE pullover "
            "hoodie laid flat with the hood neatly arranged, on a soft cream / "
            "pale-peach background with gentle shadow. Centered on the chest, "
            "print THIS exact illustration — two best-friend puppy mascots (white "
            "Bichon-Poo + blue-tan French Bulldog) sitting side by side. "
            "Premium minimal, no text, no labels, no props."),
    },
}

def main():
    client = genai.Client(api_key=KEY)
    for key, job in JOBS.items():
        ref = CHARS / job["ref"]
        if not ref.exists(): print(f"  REF MISSING: {ref}"); continue
        print(f"[mock] {key} …")
        parts = [
            types.Part.from_text(text=job["prompt"]),
            types.Part.from_bytes(data=ref.read_bytes(), mime_type="image/png"),
        ]
        resp = client.models.generate_content(
            model="gemini-3-pro-image-preview",
            contents=[types.Content(role="user", parts=parts)],
            config=types.GenerateContentConfig(response_modalities=["IMAGE","TEXT"]))
        data = None
        for c in resp.candidates or []:
            for p in (c.content.parts or []):
                if getattr(p,"inline_data",None) and p.inline_data.data:
                    data = p.inline_data.data; break
            if data: break
        if not data: print("  NO IMAGE"); continue
        (OUT / f"{key}.png").write_bytes(data)
        print(f"  saved {OUT/key}.png ({len(data)//1024} KB)")

if __name__ == "__main__":
    main()
