#!/usr/bin/env python3
"""
Photoreal mockup of MU × JiuFlow Sponsored Gi — front / back / detail.
Uses gemini-3-pro-image-preview ONLY.
"""
import os, sys, io, base64, tempfile, subprocess
from PIL import Image
from concurrent.futures import ThreadPoolExecutor, as_completed

os.environ.pop("GOOGLE_API_KEY", None)
from google import genai
from google.genai import types

API_KEY = os.environ["GEMINI_API_KEY"]
MODEL   = "gemini-3-pro-image-preview"
OUT     = os.path.dirname(os.path.abspath(__file__)) + "/mockups"
os.makedirs(OUT, exist_ok=True)

STYLE = (
    "Editorial fashion photography. Dramatic side lighting from one side, deep shadows, "
    "premium black studio background. Sharp focus on the embroidery details. "
    "Shot on a Phase One or Hasselblad with an 80mm portrait lens. f/4. "
    "Cinematic, high-contrast, magazine quality. "
    "The black gi fabric should look like premium pearl weave 350GSM with subtle texture visible. "
    "All embroidery should look HAND-EMBROIDERED with thread shine — not printed."
)

GI_BASE = (
    "A premium black Brazilian Jiu-Jitsu gi (jacket and pants), pearl weave cotton, "
    "deep matte black color. The gi is professionally tailored with reinforced stitching. "
    "A black belt is tied around the waist. The wearer is a 38-year-old Japanese male martial artist "
    "with short black hair, athletic build (170cm, 75kg), serious calm expression, hands at his sides."
)

PROMPTS = {
    "01_front": f"""
{GI_BASE}

FRONT VIEW. Standing facing camera, full body from knees up.

PRECISE EMBROIDERY LAYOUT (all white thread unless noted, look like real embroidered patches):
- LEFT CHEST (his left, our right): 8x8cm white embroidered logo with text 'MU' in bold modern sans-serif
- RIGHT CHEST: 8x8cm white embroidered logo with text 'JiuFlow' in clean lowercase
- LEFT SLEEVE OUTSIDE (his left, our right): 4 small 6x6cm square embroidered patches stacked vertically from shoulder to wrist, white thread, reading top-to-bottom: 'SOLUNA', 'Koe', 'KAGI', 'PASHA'
- RIGHT SLEEVE OUTSIDE: 4 patches stacked vertically: 'NOT A HOTEL', 'FiNANCiE', 'NEWT', and at the bottom in OLD GOLD thread (#A67843): 'KOKON 焼肉古今'
- LEFT LOWER HEM (skirt area): 2 patches: 'ATSUME' and 'GIFTMALL'
- RIGHT LOWER HEM: 2 patches: 'VUILD' and 'NESTING'
- BLACK BELT TAIL: small 4x4cm gold-thread monogram 'MU' embroidered on the tail of the black belt

{STYLE}

The gi has many sponsor logos but arranged elegantly, not chaotic — feels like a
high-end Japanese craftsmanship statement, NOT a NASCAR shirt. White thread on
matte black creates strong but refined contrast.
""",

    "02_back": f"""
{GI_BASE}

BACK VIEW. Standing facing away from camera, full body from knees up.
Hands at sides, calm posture.

BACK EMBROIDERY (this is the showpiece view):
- CENTER UPPER BACK: A large 24x12cm HERALDIC CREST embroidered in white thread.
  The crest is a shield-shape with the text "CYBRIDGE × ENABLER" prominently displayed
  on two horizontal lines, separated by an ornate '×' that resembles two crossed katana.
  Below the names: "EST. HAMADA YUKI" in small caps. Around the shield is a subtle
  laurel-wreath border, also embroidered. Premium heraldry style. Pure white thread.
- CENTER MID-BACK: a 10x10cm QR CODE embroidered in OLD GOLD thread (#A67843) on a
  small black raised patch. The QR code has 4 corner position markers that are subtly
  stylized as letters M, J, E, C (one in each corner). Below the QR in small white
  thread: "Scan → wearmu.com/gi/01"
- LEFT SLEEVE OUTSIDE (visible from back): same 4 stacked patches as front (SOLUNA, Koe, KAGI, PASHA)
- RIGHT SLEEVE OUTSIDE: same 4 patches (NOT A HOTEL, FiNANCiE, NEWT, KOKON in gold)

{STYLE}

The back is the MAIN visual statement. The heraldic crest commands attention as the
primary brand mark. The QR code below is unusual and creates intrigue — viewers will
want to scan it. The overall composition is symmetrical, balanced, premium.
""",

    "03_detail_crest": f"""
EXTREME CLOSE-UP DETAIL SHOT, macro photography. Square crop.

Subject: A heraldic shield crest embroidered in pure white thread on black pearl-weave
BJJ gi fabric. The shield contains the text "CYBRIDGE × ENABLER" on two lines,
separated by an ornate '×' formed by two crossed katana swords. The lettering uses
a refined serif typeface. Around the shield is a delicate laurel wreath border.
Below: "EST. HAMADA YUKI / 濱田優貴" in tiny caps.

Photography: macro lens, you can see individual white silk threads catching light,
each stitch slightly raised from the fabric. Crisp shadow detail. The black gi
fabric weave is sharp and tactile. Dramatic single-source side lighting.
Premium editorial product photography for a Japanese high-end martial arts brand.

Style: Like a Hayabusa, Shoyoroll, or Storm Kimonos catalog hero shot — the kind
of image that sells $400 limited-edition gis.
""",

    "04_detail_qr": f"""
EXTREME CLOSE-UP DETAIL SHOT, macro photography. Square crop.

Subject: A 10x10cm QR CODE embroidered in OLD GOLD (#A67843, warm metallic) thread
on a small slightly-raised black patch sewn onto a black BJJ gi (pearl weave fabric).
The QR code is rendered as actual embroidered stitching — you can see individual
gold threads catching the light. The 4 corner position-marker squares of the QR
code are subtly stylized: each corner contains a different small letter ('M', 'J',
'E', 'C') woven into the marker pattern, only visible on close inspection.
Below the QR code, in small white embroidered text: "Scan → wearmu.com/gi/01".

Photography: macro lens, golden thread shimmer prominent, fabric weave texture sharp.
Side lighting reveals stitching depth and the slightly raised quality of embroidery.
Premium fashion / sports luxury aesthetic. Like a Louis Vuitton x BJJ collab.
""",
}


def gen_one(name: str, prompt: str) -> str:
    print(f"  [{name}] generating...")
    client = genai.Client(api_key=API_KEY)
    response = client.models.generate_content(
        model=MODEL,
        contents=prompt,
        config=types.GenerateContentConfig(
            response_modalities=["IMAGE", "TEXT"],
        ),
    )
    for part in response.candidates[0].content.parts:
        if hasattr(part, "inline_data") and part.inline_data:
            data = part.inline_data.data
            if isinstance(data, str):
                data = base64.b64decode(data)
            path = f"{OUT}/{name}.jpg"
            img = Image.open(io.BytesIO(data)).convert("RGB")
            img.save(path, format="JPEG", quality=92, optimize=True)
            return path
    raise RuntimeError(f"no image for {name}")


def main():
    only = sys.argv[1] if len(sys.argv) > 1 else None
    tasks = [(k, v) for k, v in PROMPTS.items() if (only is None or k.startswith(only))]
    print(f"Generating {len(tasks)} mockups → {OUT}")
    with ThreadPoolExecutor(max_workers=4) as pool:
        futures = {pool.submit(gen_one, k, v): k for k, v in tasks}
        for f in as_completed(futures):
            try:
                p = f.result()
                print(f"  ✅ {p}")
            except Exception as e:
                print(f"  ❌ {futures[f]}: {e}", file=sys.stderr)


if __name__ == "__main__":
    main()
