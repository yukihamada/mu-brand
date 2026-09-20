#!/usr/bin/env python3
"""ROLL (武蔵へ捧ぐ / 野島繁昭プロデュース) brand artwork — Gemini 3 image gen.

Outputs (store/static/proposals/):
  roll-logo.png           主ロゴ / ワードマーク (透過)
  roll-print-wheel.png    主役プリント: ホイール/回転モチーフ "MOVE THE WORLD" (透過)
  roll-print-flag.png     旗用グラフィック (透過)
  roll-mock-tee.png       Tee 着用モック (黒T・ストリート)
  roll-mock-rashguard.png ラッシュガード 着用モック (パラ柔術)
  roll-mock-hoodie.png    フーディ 着用モック

Brand: ROLL / MOVE THE WORLD / パラ柔術ストリート / 強さ・エネルギー・タフな笑顔。
"""
import base64, json, os, sys, time, urllib.request, urllib.error

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
MODEL = "gemini-3-pro-image-preview"
OUT = os.path.join(os.path.dirname(__file__), os.pardir, "store", "static", "proposals")
OUT = os.path.abspath(OUT)
ACCENT = "blaze orange (#ff5a1f)"

BRAND = (
    "Brand: ROLL. Tagline: MOVE THE WORLD. A para-jiu-jitsu street apparel brand. "
    "Spirit: overwhelming positivity, a tough confident smile, energy, raw strength — "
    "street, NOT high-fashion-mode, NOT cute, NO pity, NO sad/charity tone. "
    "The word ROLL means three things at once: rolling/sparring on the BJJ mat, "
    "turning a wheelchair's wheel forward, and rolling people up into a movement. "
    f"Core palette: deep black (#0a0a0a) and {ACCENT}, with clean white. Bold, kinetic, rotational."
)

ASSETS = [
    ("roll-logo.png",
     "Brand LOGO / wordmark centered on a SOLID PURE BLACK background (#0a0a0a) that fills the ENTIRE canvas edge to edge. "
     "ABSOLUTELY NO transparency checkerboard pattern, NO gray squares, NO white box — just a flat solid black field behind the mark. "
     + BRAND +
     " Design the wordmark 'ROLL' in a heavy, athletic, condensed sans-serif, slightly italic to feel like forward motion. "
     "Integrate a subtle rotation/wheel motif into the letter O (spokes or a circular sweep). "
     "Below it, the tagline 'MOVE THE WORLD' in small wide-tracked uppercase. "
     "White letters with one blaze-orange accent element. Vector-clean, high contrast, centered, lots of empty transparent margin."),

    ("roll-print-wheel.png",
     "A bold emblem centered on a SOLID PURE BLACK background (#0a0a0a) filling the ENTIRE canvas. "
     "ABSOLUTELY NO transparency checkerboard, NO gray squares, NO white rectangle — flat solid black field only. "
     + BRAND +
     " A bold front-of-shirt graphic: a powerful circular WHEEL / rotation emblem that fuses a jiu-jitsu roll and a wheelchair wheel — "
     "dynamic spokes radiating with motion streaks, energy, forward spin. Wrap the words 'MOVE THE WORLD' around the circle. "
     "Heavy line weight, screen-print feel, white + blaze-orange on transparent. Strong, kinetic, street."),

    ("roll-print-flag.png",
     "A vertical tapestry/flag emblem centered on a SOLID PURE BLACK background (#0a0a0a) filling the ENTIRE canvas. "
     "ABSOLUTELY NO transparency checkerboard, NO gray squares — flat solid black field only. "
     + BRAND +
     " A vertical tapestry-style emblem for a dojo wall: big 'ROLL' wordmark stacked over 'MOVE THE WORLD', "
     "framed by motion lines suggesting relentless rotation. Bold, proud, white + blaze-orange. Leave transparent margins."),

    ("roll-mock-tee.png",
     "Editorial street photograph, NOT a flat-lay. A confident, smiling, energetic athlete wearing a BLACK heavyweight tee that has the "
     "ROLL wheel graphic ('MOVE THE WORLD', blaze-orange + white) printed across the chest. Gritty urban/dojo backdrop, hard directional light, "
     "tough joyful expression. The brand reads as para-jiu-jitsu street: strength and positivity, never pity. Realistic fabric, crisp print."),

    ("roll-mock-rashguard.png",
     "Editorial photograph, NOT a flat-lay. A para jiu-jitsu athlete on the mat wearing a long-sleeve RASHGUARD in deep black with blaze-orange "
     "ROLL rotation graphics and 'MOVE THE WORLD' across it. Mid-roll, dynamic, powerful, joyful intensity. IBJJF-style fit, sublimation print look, "
     "dramatic light. Conveys ROLL: relentless forward spin, tough smile, no charity tone."),

    ("roll-mock-hoodie.png",
     "Editorial street photograph, NOT a flat-lay. A person wearing a BLACK heavy pullover hoodie with a stacked 'ROLL / MOVE THE WORLD' chest print "
     "in white and blaze-orange. Urban night backdrop, neon edge light, confident energetic stance. Street, strong, kinetic — the ROLL spirit."),
]


def gen(prompt: str) -> bytes | None:
    url = f"https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:generateContent?key={KEY}"
    body = json.dumps({
        "contents": [{"parts": [{"text": prompt}]}],
        "generationConfig": {"responseModalities": ["IMAGE", "TEXT"], "temperature": 0.9},
    }).encode()
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=180) as r:
            j = json.load(r)
    except urllib.error.HTTPError as e:
        print(f"  HTTP {e.code}: {e.read()[:200]}", flush=True)
        return None
    except Exception as e:
        print(f"  ERR {e}", flush=True)
        return None
    for cand in j.get("candidates", []):
        for part in cand.get("content", {}).get("parts", []):
            d = part.get("inlineData") or part.get("inline_data")
            if d and d.get("data"):
                return base64.b64decode(d["data"])
    print("  (no image in response)", flush=True)
    return None


def main():
    if not KEY:
        print("no GEMINI_API_KEY / GOOGLE_API_KEY", file=sys.stderr); sys.exit(1)
    os.makedirs(OUT, exist_ok=True)
    for fn, prompt in ASSETS:
        path = os.path.join(OUT, fn)
        if os.path.exists(path) and os.path.getsize(path) > 30_000:
            print(f"skip {fn} (exists)", flush=True); continue
        print(f"gen {fn} ...", flush=True)
        for attempt in range(3):
            data = gen(prompt)
            if data and len(data) > 10_000:
                with open(path, "wb") as f:
                    f.write(data)
                print(f"  OK {fn} ({len(data)//1024} KB)", flush=True)
                break
            print(f"  retry {attempt+1}/3", flush=True); time.sleep(4)
        else:
            print(f"  FAIL {fn}", flush=True)
    print("done", flush=True)


if __name__ == "__main__":
    main()
