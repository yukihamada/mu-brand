#!/usr/bin/env python3
import os, sys, base64, pathlib
from google import genai
from google.genai import types

OUT = pathlib.Path(__file__).parent
client = genai.Client(api_key=os.environ.get("GEMINI_API_KEY") or os.environ["GOOGLE_API_KEY"])
MODEL = "gemini-3-pro-image-preview"

STYLE = (" — kamishibai illustration, cinematic wide 4:3, soft film grain, "
         "muted premium palette of charcoal, warm off-white and a single warm gold accent, "
         "minimal composition, emotional, painterly, no text, no letters, no words in the image")

PANELS = {
 1: "A single tiny luminous two-letter monogram floating alone in vast deep-charcoal space, like the last rare gem in the world, museum spotlight, reverent and singular" + STYLE,
 2: "Neat stacks of paper money quietly dissolving into pale ash and drifting smoke, beside a long airstrip runway fading into cold fog, a sense of loss, melancholic dawn" + STYLE,
 3: "Two open hands meeting over a small glowing seed of warm light instead of exchanging cash, an alliance being formed, intimate and hopeful" + STYLE,
 4: "A lone figure stepping from a worn wooden dock onto the deck of a great ship that is rising and sailing upward into golden dawn light, leaving the harbor of the past" + STYLE,
 5: "One precise thin sliver of warm gold light cleanly separated from a large calm luminous circle, geometric, exact, restrained, the smallest possible piece" + STYLE,
 6: "A glowing open doorway of a warm home set in an endless twilight landscape, streams of soft light flowing in from every direction of the world and becoming folded garments inside, poetic and global" + STYLE,
 7: "A pristine premium folded garment on a clean studio surface under soft light, an empty woven clothing tag resting on it, calm, aspirational, room for a logo" + STYLE,
}

def gen(pid, prompt):
    r = client.models.generate_content(
        model=MODEL, contents=prompt,
        config=types.GenerateContentConfig(response_modalities=["IMAGE","TEXT"]),
    )
    for part in r.candidates[0].content.parts:
        if getattr(part, "inline_data", None) and part.inline_data.data:
            d = part.inline_data.data
            if isinstance(d, str): d = base64.b64decode(d)
            p = OUT / f"panel{pid}.png"
            p.write_bytes(d)
            return f"OK panel{pid} {len(d)//1024}KB"
    return f"NO IMAGE panel{pid}"

ids = [int(x) for x in sys.argv[1:]] or list(PANELS)
for pid in ids:
    try: print(gen(pid, PANELS[pid]), flush=True)
    except Exception as e: print(f"ERR panel{pid}: {e}", flush=True)
