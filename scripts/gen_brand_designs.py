#!/usr/bin/env python3
"""Generate brand design PNGs (wordmark / monogram / stacked / stripe) for a
new collab proposal — same naming convention used by NOJIMAHAL & RYOZO so
the LP renderer and Printful order flow pick them up automatically.

Two backends are tried in order:

  1. Gemini 3 (GEMINI_API_KEY in env) — model writes 4 SVG strings tailored
     to the brand's monogram + accent color, we rasterize each with
     rsvg-convert at 2940×2940 (the print-bed dimensions our DTG pipeline
     expects). Best fidelity; needs API access.

  2. Local template fallback — purely deterministic SVG built from the
     brand's monogram + accent color, no external API. Lower fidelity but
     guarantees the pipeline always produces files.

Usage:
    python3 scripts/gen_brand_designs.py <slug> --monogram MG --accent '#7be57b'
    # outputs store/static/proposals/<slug>-design-{wordmark,monogram,stacked,stripe}.png

The Gemini path can also be skipped explicitly with --no-gemini for offline
runs (e.g. CI without secrets).
"""
import argparse, base64, json, os, re, subprocess, sys, tempfile, urllib.request

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), os.pardir))
OUT_DIR = os.path.join(ROOT, "store", "static", "proposals")

VARIANTS = ["wordmark", "monogram", "stacked", "stripe"]

GEMINI_MODEL = "gemini-2.5-flash"  # text-only; SVG is plain XML (matches store/src/gemini.rs TEXT_MODEL)
GEMINI_URL_TMPL = "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"

PROMPT_TMPL = """You are an apparel-brand designer.
Brand: {name}
Slug: {slug}
Monogram (max 4 chars): {monogram}
Accent hex: {accent}
Background: black (#000000)

Output exactly 4 SVG documents, separated by the line `===` (three equals,
on its own line, nothing else). Each SVG must:
  - Use viewBox="0 0 2940 2940" (square print bed)
  - Use white (#ffffff) and the brand accent for fills/strokes
  - NO background fill (transparent print bed)
  - Be self-contained (no external <image> or <use> refs)
  - Be ≤ 2KB

The 4 variants, in order:
  1. wordmark  — full brand name centered, heavy sans (Helvetica Neue, Arial Black)
  2. monogram  — the {monogram} mark, big, framed
  3. stacked   — name stacked over a descriptor + 2 thin rules
  4. stripe    — vertical stripe of monogram, repeated, gold/accent accent

Output ONLY the 4 SVGs separated by `===`. No prose. No markdown fences."""


def fetch_gemini(slug, name, monogram, accent):
    key = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
    if not key:
        return None
    url = GEMINI_URL_TMPL.format(model=GEMINI_MODEL) + f"?key={key}"
    body = {
        "contents": [{"parts": [{"text": PROMPT_TMPL.format(
            name=name, slug=slug, monogram=monogram, accent=accent,
        )}]}],
        "generationConfig": {"temperature": 0.4, "maxOutputTokens": 4096},
    }
    req = urllib.request.Request(
        url, data=json.dumps(body).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            resp = json.loads(r.read())
    except Exception as e:
        print(f"  ! gemini http: {e}", file=sys.stderr)
        return None
    try:
        text = resp["candidates"][0]["content"]["parts"][0]["text"]
    except (KeyError, IndexError, TypeError):
        print(f"  ! gemini bad response: {str(resp)[:200]}", file=sys.stderr)
        return None
    parts = re.split(r"^={3,}\s*$", text, flags=re.MULTILINE)
    parts = [p.strip() for p in parts if "<svg" in p]
    if len(parts) < 4:
        print(f"  ! gemini returned only {len(parts)} SVGs", file=sys.stderr)
        return None
    return parts[:4]


def template_svgs(slug, name, monogram, accent):
    """Deterministic offline fallback. Visually closer to RYOZO patch style."""
    upper = name.upper()
    def grad():
        return f'<linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#f3d56f"/><stop offset="0.5" stop-color="{accent}"/><stop offset="1" stop-color="#a07820"/></linearGradient>'
    wordmark = f'''<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2940 2940"><defs>{grad()}</defs>
  <text x="1470" y="1600" text-anchor="middle" font-family="Helvetica Neue, Arial Black, sans-serif" font-weight="900" font-size="540" letter-spacing="36" fill="url(#g)">{upper}</text>
</svg>'''
    monogram_svg = f'''<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2940 2940"><defs>{grad()}</defs>
  <rect x="370" y="370" width="2200" height="2200" fill="none" stroke="url(#g)" stroke-width="32"/>
  <text x="1470" y="1740" text-anchor="middle" font-family="Helvetica Neue, Arial Black, sans-serif" font-weight="900" font-size="900" letter-spacing="40" fill="url(#g)">{monogram.upper()}</text>
</svg>'''
    stacked = f'''<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2940 2940"><defs>{grad()}</defs>
  <text x="1470" y="1280" text-anchor="middle" font-family="Helvetica Neue, Arial Black, sans-serif" font-weight="900" font-size="480" letter-spacing="20" fill="url(#g)">{upper}</text>
  <rect x="370" y="1380" width="2200" height="8" fill="url(#g)"/>
  <text x="1470" y="1720" text-anchor="middle" font-family="Helvetica Neue, Arial, sans-serif" font-weight="700" font-size="80" letter-spacing="56" fill="{accent}">— {monogram.upper()} —</text>
  <rect x="370" y="1820" width="2200" height="8" fill="url(#g)"/>
</svg>'''
    stripe = f'''<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2940 2940"><defs>{grad()}</defs>
  <rect x="1170" y="0" width="160" height="2940" fill="url(#g)"/>
  <rect x="1410" y="0" width="28" height="2940" fill="{accent}"/>
  <g transform="translate(1470,1470) rotate(-90)">
    <text x="0" y="0" text-anchor="middle" font-family="Helvetica Neue, Arial Black, sans-serif" font-weight="900" font-size="280" letter-spacing="60" fill="url(#g)">{upper}</text>
  </g>
</svg>'''
    return [wordmark, monogram_svg, stacked, stripe]


def rasterize(svg_text, out_png, size=2940):
    with tempfile.NamedTemporaryFile("w", suffix=".svg", delete=False) as f:
        f.write(svg_text)
        svg_path = f.name
    try:
        subprocess.run([
            "rsvg-convert", "-w", str(size), "-h", str(size),
            "-o", out_png, svg_path,
        ], check=True, capture_output=True)
    finally:
        os.unlink(svg_path)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("slug", help="brand slug (matches proposals.slug)")
    ap.add_argument("--name", required=True, help="full brand name (for wordmark)")
    ap.add_argument("--monogram", required=True, help="2-4 char monogram")
    ap.add_argument("--accent", default="#7be57b", help="accent hex (#rrggbb)")
    ap.add_argument("--no-gemini", action="store_true", help="skip Gemini, use template only")
    ap.add_argument("--size", type=int, default=2940, help="output PNG side (px)")
    args = ap.parse_args()

    slug = args.slug.strip().lower()
    name = args.name.strip()
    mg = args.monogram.strip()[:4].upper()
    accent = args.accent if re.match(r"^#[0-9a-fA-F]{6}$", args.accent) else "#7be57b"

    os.makedirs(OUT_DIR, exist_ok=True)
    svgs = None if args.no_gemini else fetch_gemini(slug, name, mg, accent)
    source = "gemini"
    if not svgs:
        svgs = template_svgs(slug, name, mg, accent)
        source = "template"
    print(f"━◯━ designs source: {source}")

    for variant, svg in zip(VARIANTS, svgs):
        out = os.path.join(OUT_DIR, f"{slug}-design-{variant}.png")
        try:
            rasterize(svg, out, size=args.size)
            kb = os.path.getsize(out) // 1024
            print(f"  + {variant:9s} → {out}  ({kb} KB)")
        except subprocess.CalledProcessError as e:
            print(f"  ! {variant} rsvg-convert failed: {e.stderr.decode()[:200]}", file=sys.stderr)
            sys.exit(1)
    print(f"━◯━ done. all 4 PNGs at {OUT_DIR}/{slug}-design-*.png")


if __name__ == "__main__":
    main()
