#!/usr/bin/env python3
"""MU × ELE × POTE — generate mascot illustrations from the real dog photos.

Image-conditioned generation via gemini-3-pro-image-preview: photo + text
prompt → stylized mascot character on pure white (print-ready / preview).

Out:
  /tmp/elepote/ele_char.png    Ele (Bichon-Poo) mascot
  /tmp/elepote/pote_char.png   Pote (Frenchie) mascot
  /tmp/elepote/duo_char.png    The two friends together (hero)
"""
import os, sys, pathlib
from google import genai
from google.genai import types

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY: sys.exit("GEMINI_API_KEY not set")

OUT = pathlib.Path("/tmp/elepote"); OUT.mkdir(parents=True, exist_ok=True)

ELE_PHOTO  = pathlib.Path("/Users/yuki/workspace/ele-blog/images/ele-1.jpg")
POTE_FACE  = pathlib.Path("/Users/yuki/Downloads/S__12755045.jpg")  # face close-up
POTE_BODY  = pathlib.Path("/Users/yuki/Downloads/S__12755044.jpg")  # tongue-out sleepy

STYLE = (
    "Clean, premium, modern editorial illustration. Confident monoline outlines + "
    "soft flat fills, ONE or TWO warm accent colors maximum, no photo-realism, no "
    "heavy gradients. Iconic and instantly readable at chest-print size — like a "
    "high-end Japanese streetwear character graphic. Centered with generous margin "
    "on PURE SOLID WHITE (#FFFFFF) background, no shadow, no frame, no text."
)

JOBS = {
    "ele_char": {
        "refs": [ELE_PHOTO],
        "prompt": (
            "Create an illustrated MASCOT CHARACTER of THIS exact dog — a small "
            "white Bichon-Poodle mix called ELE, with characteristic fluffy curly "
            "white fur, round black eyes, small black nose, ears hidden in the "
            "fluff. Pose: relaxed sitting front view, head slightly tilted, gentle "
            "smile, paws together. Keep the unmistakable Bichon-Poo silhouette — "
            "round fluffy cloud-like body, no tail showing. " + STYLE),
    },
    "pote_char": {
        "refs": [POTE_FACE, POTE_BODY],
        "prompt": (
            "Create an illustrated MASCOT CHARACTER of THIS exact puppy — a "
            "BLUE-AND-TAN French Bulldog puppy called POTE, blue/silver-gray coat "
            "with tan markings above the eyes, on the muzzle and the chest, big "
            "upright bat-ears, classic squished Frenchie face, big round black "
            "eyes, dark nose, pink tongue sticking out a little. Pose: sitting "
            "front view, slightly sleepy / dopey expression with a small tongue "
            "poking out, paws together. Keep the unmistakable Frenchie silhouette "
            "— stout body, no tail. " + STYLE),
    },
    "duo_char": {
        "refs": [ELE_PHOTO, POTE_FACE],
        "prompt": (
            "Create a HERO illustration of TWO best-friend dog mascots SITTING "
            "SIDE BY SIDE — on the LEFT, ELE the small white fluffy Bichon-Poodle "
            "mix (round cloud of white curls, black eyes, gentle smile); on the "
            "RIGHT, POTE the blue-and-tan French Bulldog puppy (blue/silver coat "
            "with tan markings above the eyes / on the muzzle / on the chest, big "
            "upright bat-ears, squished face, little pink tongue out). Same scale, "
            "their shoulders touching warmly, both facing forward, friendly. "
            "Composition centered, balanced left-right. " + STYLE),
    },
}

def main():
    client = genai.Client(api_key=KEY)
    for key, job in JOBS.items():
        print(f"[elepote] {key} …")
        parts = [types.Part.from_text(text=job["prompt"])]
        for ref in job["refs"]:
            if not ref.exists():
                print(f"  REF MISSING: {ref}"); continue
            mime = "image/jpeg" if ref.suffix.lower() in (".jpg",".jpeg") else "image/png"
            parts.append(types.Part.from_bytes(data=ref.read_bytes(), mime_type=mime))
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
        if not data:
            print(f"  NO IMAGE"); continue
        (OUT / f"{key}.png").write_bytes(data)
        print(f"  saved {OUT/key}.png ({len(data)//1024} KB)")

if __name__ == "__main__":
    main()
