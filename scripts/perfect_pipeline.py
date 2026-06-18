#!/usr/bin/env python3
"""High-speed parallel pipeline: design + mockup + lifestyle per SKU.

Flow (per SKU, inheriting the 'perfect 10' rules):
  1. Ensure concept design exists at store/static/<brand>/d/design_<concept>.png
     (shared across color/size variants of the same concept)
  2. Generate mockup: Gemini compose (Printful product + design) with the
     STRICT prompt that produces clean white BG, no watermark, proper ink
     inversion, exact typography, no overflow.
     → store/static/<brand>/m/perfect_<sku>.jpg
  3. Generate lifestyle: Gemini editorial photo, product-type aware.
     → store/static/<brand>/lifestyle/perfect_<sku>.jpg

Parallelism: ThreadPoolExecutor with N workers. Each worker independently
runs steps 1-3 for one SKU. Designs are concept-shared so concurrent SKUs
within the same concept may both try to generate it — file existence check
guards against duplicate writes.

Output map: /tmp/wearmu_perfect_pipeline.json — {sku: {design, mockup, lifestyle}}

Usage:
    python3 scripts/perfect_pipeline.py --skus MU-BJJ-02-TEE-BLACK ...
    python3 scripts/perfect_pipeline.py --brand bjj --limit 20
    python3 scripts/perfect_pipeline.py --concept MU-BJJ-02
    python3 scripts/perfect_pipeline.py --all --workers 16
"""
from __future__ import annotations
import argparse
import base64
import concurrent.futures as cf
import json
import os
import re
import sqlite3
import sys
import threading
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DB = ROOT / "store" / "products.db"
# Pipeline state lives in repo (persistent across reboots / re-clones).
# /tmp/ is a symlink to here for backward compat.
STATE_DIR = ROOT / "data" / "pipeline_state"
STATE_DIR.mkdir(parents=True, exist_ok=True)
MAP_PATH = STATE_DIR / "wearmu_perfect_pipeline.json"
LOG_PATH = ROOT / "logs" / "perfect_pipeline.log"
LOG_PATH.parent.mkdir(parents=True, exist_ok=True)

KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
if not KEY:
    env_f = Path("/Users/yuki/.env")
    if env_f.exists():
        for line in env_f.read_text().splitlines():
            if line.startswith(("GEMINI_API_KEY=", "GOOGLE_API_KEY=")):
                KEY = line.split("=", 1)[1].strip().strip("'\"")
                break
if not KEY:
    sys.exit("GEMINI_API_KEY missing")

PRINTFUL = json.loads((STATE_DIR / "wearmu_printful_variants.json").read_text())
MODEL = "gemini-3-pro-image-preview"

# ── thread-safe state ──────────────────────────────────────────────────────
MAP_LOCK = threading.Lock()
LOG_LOCK = threading.Lock()
RESULT: dict[str, dict] = {}
if MAP_PATH.exists():
    try:
        RESULT = json.loads(MAP_PATH.read_text())
    except Exception:
        pass

PRODUCT_CACHE: dict[str, bytes] = {}
PRODUCT_LOCK = threading.Lock()

DESIGN_CACHE: dict[tuple[str, str], bytes] = {}
DESIGN_LOCK = threading.Lock()


def log(event: dict):
    event["ts"] = time.strftime("%H:%M:%S")
    with LOG_LOCK:
        with LOG_PATH.open("a") as f:
            f.write(json.dumps(event, ensure_ascii=False) + "\n")


def save_map():
    with MAP_LOCK:
        MAP_PATH.write_text(json.dumps(RESULT, indent=2))


# ── concept helpers ────────────────────────────────────────────────────────
def extract_concept(sku: str) -> str:
    m = re.match(r"^MU-([A-Z0-9]+)-(\d+)-", sku)
    if m:
        return f"MU-{m.group(1)}-{m.group(2)}"
    return re.sub(r"-(?:XS|S|M|L|XL|2XL|3XL|4XL|one|os)$", "", sku, flags=re.IGNORECASE)


def design_path(brand: str, concept: str) -> Path:
    return ROOT / "store" / "static" / brand / "d" / f"design_{concept}.png"


def mockup_path(brand: str, sku: str) -> Path:
    return ROOT / "store" / "static" / brand / "m" / f"perfect_{sku}.jpg"


def lifestyle_path(brand: str, sku: str) -> Path:
    return ROOT / "store" / "static" / brand / "lifestyle" / f"perfect_{sku}.jpg"


# ── product-kind classifier (drives ink, scene, area) ──────────────────────
def classify(sku: str, label: str) -> tuple[str, str, str]:
    """Return (kind_desc, ink_color, lifestyle_scene)."""
    L = (sku + " " + (label or "")).upper()
    if "HOODIE" in L or "PULLOVER" in L:
        kind, ink = "black heavyweight pullover hoodie", "white"
    elif "LONG-SLEEVE" in L or "LONG SLEEVE" in L or "LS" in L:
        kind, ink = "black long-sleeve T-shirt", "white"
    elif "RASH" in L:
        kind, ink = "white long-sleeve rashguard (BJJ athletic shirt)", "black"
    elif "TEE-BLACK" in L or "TEE BLACK" in L:
        kind, ink = "black short-sleeve T-shirt", "white"
    elif "TEE-WHITE" in L or "TEE WHITE" in L:
        kind, ink = "white short-sleeve T-shirt", "black"
    elif "TEE-NAVY" in L:
        kind, ink = "navy short-sleeve T-shirt", "white"
    elif "APRON" in L:
        kind, ink = "natural-cotton chef apron with neck strap and waist tie", "gold-thread embroidered"
    elif "BEANIE" in L or "BEAN" in L:
        kind, ink = "ribbed knit beanie", "embroidered"
    elif "CAP" in L or "SNAPBACK" in L:
        kind, ink = "snapback baseball cap", "embroidered"
    elif "TANK" in L:
        kind, ink = "black tank top", "white"
    elif "TOTE" in L or "BAG" in L:
        kind, ink = "natural canvas tote bag", "black"
    elif "MUG" in L:
        kind, ink = "11oz ceramic mug", "color"
    elif "STICKER" in L:
        kind, ink = "die-cut vinyl sticker", "color"
    elif "CANVAS" in L or "POSTER" in L:
        kind, ink = "stretched canvas wall print", "color"
    elif "JOGGER" in L or "JOG" in L or "SWEAT" in L:
        kind, ink = "black sweatpants/joggers", "white"
    elif "LEG" in L or "SPAT" in L:
        kind, ink = "black leggings (no-gi BJJ spats)", "white"
    elif "SHORT" in L:
        kind, ink = "athletic shorts", "white"
    else:
        kind, ink = "apparel item", "high contrast"

    brand_scene = {
        "bjj": "BJJ academy lobby, late afternoon, person with folded gi over arm",
        "code": "Tokyo developer cafe, person at MacBook, soft window light",
        "coffee": "specialty espresso bar, person at counter",
        "zen": "minimalist tatami room with single zafu cushion, dawn",
        "moon": "rooftop at twilight, lone figure, deep blue gradient sky",
        "mu": "minimalist white gallery room, single figure centered",
        "tokyo": "Shibuya crossing at dusk, person in tee, blurred neon",
        "jiuflow": "BJJ tournament backstage, athlete on bench preparing",
        "kokon": "yakiniku restaurant interior, server at counter, charcoal grill smoke",
        "roll": "BJJ academy after roll, towel over shoulder, mat behind",
    }
    return kind, ink, "{brand_scene}"  # scene filled at use time


# ── Gemini call (raw) ──────────────────────────────────────────────────────
def gemini_image(prompt: str, refs: list[bytes], retries=2) -> bytes | None:
    parts = [{"text": prompt}]
    for b in refs:
        mt = "image/png"
        if b[:3] == b"\xff\xd8\xff":
            mt = "image/jpeg"
        parts.append({"inlineData": {"mimeType": mt, "data": base64.b64encode(b).decode()}})
    url = f"https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:generateContent?key={KEY}"
    body = json.dumps({
        "contents": [{"parts": parts}],
        "generationConfig": {"responseModalities": ["IMAGE", "TEXT"], "temperature": 0.6},
    }).encode()

    for attempt in range(retries + 1):
        try:
            req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
            with urllib.request.urlopen(req, timeout=180) as r:
                j = json.load(r)
            for cand in j.get("candidates", []):
                for part in cand.get("content", {}).get("parts", []):
                    d = part.get("inlineData") or part.get("inline_data")
                    if d and d.get("data"):
                        return base64.b64decode(d["data"])
            return None
        except urllib.error.HTTPError as e:
            if e.code in (429, 503) and attempt < retries:
                time.sleep(2 + attempt * 3)
                continue
            log({"event": "gemini_http", "code": e.code, "attempt": attempt})
            return None
        except Exception as e:
            if attempt < retries:
                time.sleep(2)
                continue
            log({"event": "gemini_err", "err": str(e), "attempt": attempt})
            return None


# ── fetchers ───────────────────────────────────────────────────────────────
def fetch_product(url: str) -> bytes | None:
    with PRODUCT_LOCK:
        if url in PRODUCT_CACHE:
            return PRODUCT_CACHE[url]
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0 wearmu/1"})
        with urllib.request.urlopen(req, timeout=20) as r:
            data = r.read()
        with PRODUCT_LOCK:
            PRODUCT_CACHE[url] = data
        return data
    except Exception as e:
        return None


def load_design(brand: str, concept: str) -> bytes | None:
    key = (brand, concept)
    with DESIGN_LOCK:
        if key in DESIGN_CACHE:
            return DESIGN_CACHE[key]
    p = design_path(brand, concept)
    if p.exists():
        b = p.read_bytes()
        with DESIGN_LOCK:
            DESIGN_CACHE[key] = b
        return b
    return None


# ── generators ─────────────────────────────────────────────────────────────
DESIGN_GEN_LOCKS: dict[tuple[str, str], threading.Lock] = {}
DGL_LOCK = threading.Lock()


def get_design_lock(brand: str, concept: str) -> threading.Lock:
    key = (brand, concept)
    with DGL_LOCK:
        if key not in DESIGN_GEN_LOCKS:
            DESIGN_GEN_LOCKS[key] = threading.Lock()
        return DESIGN_GEN_LOCKS[key]


# Brand metadata loaded from catalog_brands.config_json on first use.
# Adding a new brand is one INSERT into catalog_brands (CLAUDE.md contract);
# nothing in this file needs to change.
_BRAND_CACHE: dict[str, dict] = {}
_BRAND_LOCK = threading.Lock()


def brand_config(brand: str) -> dict:
    with _BRAND_LOCK:
        if brand in _BRAND_CACHE:
            return _BRAND_CACHE[brand]
    conn = sqlite3.connect(str(DB))
    row = conn.execute(
        "SELECT config_json FROM catalog_brands WHERE slug=?", (brand,)).fetchone()
    conn.close()
    cfg = {}
    if row and row[0]:
        try:
            cfg = json.loads(row[0])
        except Exception:
            cfg = {}
    # safe fallbacks so unknown brand still renders
    cfg.setdefault("design_style",
        "Clean editorial single-color screen-print on transparent background.")
    cfg.setdefault("lifestyle_scene",
        f"editorial photograph in a setting that fits the {brand} brand")
    cfg.setdefault("ink_default", "high contrast")
    with _BRAND_LOCK:
        _BRAND_CACHE[brand] = cfg
    return cfg


def gen_design_concept(brand: str, concept: str, rep_label: str, rep_desc: str) -> bytes | None:
    """Generate one design per concept, lock-guarded so 98 SKUs don't all generate."""
    p = design_path(brand, concept)
    lock = get_design_lock(brand, concept)
    with lock:
        if p.exists() and p.stat().st_size > 30_000:
            return p.read_bytes()
        style = brand_config(brand)["design_style"]
        prompt = f"""Print-ready apparel artwork. Transparent background (alpha channel, NOT white box).

Brand: MU × {brand}
Concept #{concept}: {rep_label}
Description: {rep_desc}
Style: {style}

Requirements:
- Single-color screen-print friendly.
- Crisp edges, legible at 100mm.
- Centered square composition, max 80% canvas width.
- 1024x1024 PNG with TRANSPARENT background. No tee/mockup, no model.
"""
        b = gemini_image(prompt, [])
        if b:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(b)
            log({"event": "design_ok", "concept": concept, "bytes": len(b)})
        else:
            log({"event": "design_fail", "concept": concept})
        return b


def gen_mockup(sku: str, brand: str, label: str, product_url: str, design_b: bytes) -> bytes | None:
    out = mockup_path(brand, sku)
    if out.exists() and out.stat().st_size > 50_000:
        return out.read_bytes()
    kind, ink, _ = classify(sku, label)
    product_b = fetch_product(product_url)
    if not product_b:
        return None
    prompt = f"""E-commerce product mockup. CRITICAL: clean SOLID WHITE background.

REFERENCES:
  Image 1: a {kind} on a neutral background. Keep garment color/shape/drape exactly.
  Image 2: a transparent PNG print artwork. IGNORE the checkered transparency
           pattern — that is the file's alpha channel, NOT part of the design.

TASK: produce a single product photograph: the {kind} centered on SOLID WHITE
#FFFFFF studio background, with the artwork from Image 2 printed on FRONT
CENTER CHEST (or apron front panel) only.

ABSOLUTE RULES:
  1. BACKGROUND = pure flat white. NO checkered pattern, NO duplicate artwork
     behind, NO scattered ink, NO watermark, NO decorative shapes. Just the
     garment on white.
  2. INK = {ink} so it is CLEARLY READABLE. If dark-on-dark, INVERT to white.
  3. PRINT SIZE = at most 60% of the garment's widest visible width. Within
     the chest panel only. NO part touches the side seams or sleeves. Clean
     fabric margin both sides.
  4. TYPOGRAPHY = copy every letter EXACTLY. No typos. All letters visible
     (no truncation).
  5. FRAMING = entire garment visible, centered. If a model: full head and
     shoulders, no cropping. Otherwise ghost-mannequin / flat-lay.

Output: 1024x1024 PNG, photoreal product-shot quality.
"""
    b = gemini_image(prompt, [product_b, design_b])
    if b:
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(b)
        log({"event": "mockup_ok", "sku": sku, "bytes": len(b)})
    else:
        log({"event": "mockup_fail", "sku": sku})
    return b


def gen_lifestyle(sku: str, brand: str, label: str, design_b: bytes, mockup_b: bytes | None) -> bytes | None:
    out = lifestyle_path(brand, sku)
    if out.exists() and out.stat().st_size > 80_000:
        return out.read_bytes()
    kind, ink, _ = classify(sku, label)
    scene = brand_config(brand)["lifestyle_scene"]
    prompt = f"""Editorial lifestyle photograph (NOT a product flat-lay).

Subject: a Japanese person 20s-30s wearing the {kind} that has the design
from the reference artwork (concept "{label}") printed on the chest in {ink}
ink. The design must be visible and readable on the chest.

Scene: {scene}.

Style: photojournalistic 35mm, magazine cover quality, natural light, soft
depth-of-field, slightly desaturated, 3:4 portrait composition with subject
mid-frame. Not a studio flat lay.

Output 1024x1024 photographic PNG.
"""
    refs = [design_b]
    if mockup_b:
        refs.append(mockup_b)
    b = gemini_image(prompt, refs)
    if b:
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(b)
        log({"event": "lifestyle_ok", "sku": sku, "bytes": len(b)})
    else:
        log({"event": "lifestyle_fail", "sku": sku})
    return b


# ── per-SKU worker ─────────────────────────────────────────────────────────
CONCEPT_REPR_LOCK = threading.Lock()
CONCEPT_REPR: dict[str, tuple[str, str]] = {}


def get_concept_repr(conn: sqlite3.Connection, concept: str) -> tuple[str, str]:
    """(label, description) for a concept; prefer TEE-BLACK variant for clean label."""
    with CONCEPT_REPR_LOCK:
        if concept in CONCEPT_REPR:
            return CONCEPT_REPR[concept]
    rows = conn.execute(
        "SELECT sku, label, description_ja FROM catalog_products WHERE sku LIKE ?",
        (concept + "%",)).fetchall()
    if not rows:
        rows = conn.execute(
            "SELECT sku, label, description_ja FROM catalog_products WHERE sku=?",
            (concept,)).fetchall()
    best = None
    for sku, label, desc in rows:
        if "TEE-BLACK" in sku and "canvas" not in (label or "").lower():
            best = (label, desc); break
    if not best and rows:
        best = (rows[0][1], rows[0][2])
    if not best:
        best = ("", "")
    with CONCEPT_REPR_LOCK:
        CONCEPT_REPR[concept] = best
    return best


def process_sku(sku: str) -> dict:
    conn = sqlite3.connect(str(DB))
    r = conn.execute(
        "SELECT brand, label, description_ja, printful_variant_id FROM catalog_products WHERE sku=?",
        (sku,)).fetchone()
    if not r:
        conn.close()
        return {"sku": sku, "err": "not in db"}
    brand, label, desc, vid = r
    concept = extract_concept(sku)
    rep_label, rep_desc = get_concept_repr(conn, concept)
    rep_label = rep_label or label or ""
    rep_desc = rep_desc or desc or ""

    purl = PRINTFUL.get(str(vid))
    if not purl:
        conn.close()
        return {"sku": sku, "err": "no printful url"}

    t0 = time.time()
    # 1. design (shared per concept) — must complete first since lifestyle uses it
    design_b = load_design(brand, concept) or gen_design_concept(brand, concept, rep_label, rep_desc)
    if not design_b:
        conn.close()
        return {"sku": sku, "err": "no design"}

    # 2 & 3. mockup and lifestyle run IN PARALLEL — both depend only on design.
    # Each is a Gemini call (~20s). Sequential was wasted time; concurrent
    # halves per-SKU latency. Lifestyle previously took the mockup as an
    # optional reference; drop that to enable true parallelism.
    with cf.ThreadPoolExecutor(max_workers=2) as inner:
        fut_mockup = inner.submit(gen_mockup, sku, brand, label, purl, design_b)
        fut_life   = inner.submit(gen_lifestyle, sku, brand, label, design_b, None)
        mockup_b = fut_mockup.result()
        life_b   = fut_life.result()

    res = {
        "sku": sku, "brand": brand,
        "design": design_path(brand, concept).resolve().as_uri(),
        "mockup": mockup_path(brand, sku).resolve().as_uri() if mockup_b else None,
        "lifestyle": lifestyle_path(brand, sku).resolve().as_uri() if life_b else None,
        "elapsed_s": round(time.time() - t0, 1),
    }
    with MAP_LOCK:
        RESULT[sku] = res
    save_map()
    conn.close()
    return res


# ── main ───────────────────────────────────────────────────────────────────
def select_skus(args) -> list[str]:
    conn = sqlite3.connect(str(DB))
    where = "WHERE status='live'"
    params: list = []
    if args.brand:
        where += " AND brand=?"
        params.append(args.brand)
    if args.concept:
        where += " AND sku LIKE ?"
        params.append(args.concept + "%")
    if args.skus:
        placeholders = ",".join("?" * len(args.skus))
        sql = f"SELECT sku FROM catalog_products WHERE sku IN ({placeholders})"
        rows = conn.execute(sql, args.skus).fetchall()
    else:
        rows = conn.execute(f"SELECT sku FROM catalog_products {where} ORDER BY brand, sku", params).fetchall()
    conn.close()
    skus = [r[0] for r in rows]
    if args.limit:
        skus = skus[: args.limit]
    return skus


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--skus", nargs="*", default=[])
    ap.add_argument("--brand")
    ap.add_argument("--concept")
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--limit", type=int)
    ap.add_argument("--workers", type=int, default=20,
                    help="outer SKU-level workers. Each spawns 2 inner threads "
                         "(mockup + lifestyle in parallel), so effective Gemini "
                         "concurrency = workers * 2.")
    args = ap.parse_args()

    skus = select_skus(args)
    if not skus:
        sys.exit("no SKUs selected")
    print(f"processing {len(skus):,} SKUs with {args.workers} workers")
    print(f"est cost: ~¥{len(skus)*12:,} (mockup ¥6 + lifestyle ¥6 per SKU, design shared)")

    started = time.time()
    ok = fail = 0
    with cf.ThreadPoolExecutor(max_workers=args.workers) as ex:
        futures = {ex.submit(process_sku, s): s for s in skus}
        for i, fut in enumerate(cf.as_completed(futures), start=1):
            sku = futures[fut]
            try:
                res = fut.result()
                if res.get("mockup") and res.get("lifestyle"):
                    ok += 1
                else:
                    fail += 1
                # progress
                elapsed = time.time() - started
                rate = i / elapsed
                eta = (len(skus) - i) / rate if rate else 0
                print(f"  [{i:4d}/{len(skus)}] {sku:34s}  ok={ok} fail={fail}  rate={rate:.2f}/s  ETA={eta/60:.1f}min  ({res.get('elapsed_s','?')}s)")
            except Exception as e:
                fail += 1
                print(f"  [err] {sku}: {e}")

    total = time.time() - started
    print(f"\ndone. ok={ok} fail={fail}  total={total/60:.1f}min  avg={total/len(skus):.1f}s/SKU")
    print(f"map → {MAP_PATH}")


if __name__ == "__main__":
    main()
