#!/usr/bin/env python3
"""
MU Brand — Bulk Lifestyle Photo Generator (Gemini batch)

For every active product with an empty `lifestyle_url`, compose a
photo-realistic on-body shot of the exact mockup via
gemini-3-pro-image-preview, upload to R2 bucket `wearmu-lifestyle`
(public host `lifestyle.wearmu.com`), and PATCH `products.lifestyle_url`
through /api/admin/lifestyle.

Why this exists:
- generate_lifestyle.py only targets the MUGEN brand and reads design
  PNGs from `designs/`. Collab / sample brands (jiufight_*, ryozo_*,
  nojimahal_*, ele_*, asoview_*, nouns, ma, muon, rashguard …) had no
  lifestyle hero, so the storefront CVR boost was capped to MUGEN.
- We now seed lifestyle hero shots for the whole storefront, fairly
  capped per brand so one big brand doesn't eat the budget.

Source of the design reference:
- We pull the live Printful/served mockup from
  https://wearmu.com/api/products/<id>/mockup.png (the same image the
  product page shows). This works for every brand that has a mockup,
  including collab samples whose design PNGs are not on disk.

Fallback when Gemini fails (auth / quota / safety reject):
- Pillow ambient blur on the same mockup ("low-quality but no-cost"
  background) so the product page never crashes. Marked in the log
  with `mode=fallback`.

Usage:
    python scripts/bulk_lifestyle_gen.py --max-products 100
    python scripts/bulk_lifestyle_gen.py --max-products 20 --brand jiufight
    python scripts/bulk_lifestyle_gen.py --dry-run            # plan only

Cost: Gemini 3 Pro Image ≈ US$0.04 / image, so 100 ≈ $4, 1,500 ≈ $60.
"""
from __future__ import annotations

import argparse
import base64
import io
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

from PIL import Image, ImageFilter, ImageEnhance

# ── Env / paths ────────────────────────────────────────────────────────────
ROOT = Path(__file__).resolve().parent.parent
LOG_DIR = ROOT / "logs"
LOG_DIR.mkdir(parents=True, exist_ok=True)
LOG_PATH = LOG_DIR / "bulk_lifestyle.log"

GEMINI_API_KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
GEMINI_MODEL = "gemini-3-pro-image-preview"
STORE_URL = os.environ.get("MU_STORE_URL", "https://wearmu.com")
ADMIN_TOKEN = os.environ.get("MU_ADMIN_TOKEN", "mu-admin-2026")
WRANGLER_BIN = os.environ.get("WRANGLER_BIN", "/opt/homebrew/bin/wrangler")
R2_BUCKET = os.environ.get("MU_LIFESTYLE_BUCKET", "wearmu-lifestyle")
PUBLIC_HOST = os.environ.get("MU_LIFESTYLE_HOST", "lifestyle.wearmu.com")

# ── Brand-aware scene rotation ─────────────────────────────────────────────
# For each brand prefix, a short list of (scene, persona-hint) tuples. We
# rotate per product so a single brand doesn't get 20 identical shots.
SCENES_BY_KIND: dict[str, list[tuple[str, str]]] = {
    "jiufight": [
        ("inside a Tokyo BJJ academy lobby after class, soft afternoon light, mat texture in the background", "Ren"),
        ("at a tournament weigh-in area, ambient gym lighting, slight motion blur in the background", "Sho"),
        ("sitting on a bench outside a dojo at golden hour with a rolled gi", "Kazu"),
        ("warm-up corner of a martial arts gym, breath visible in cool air", "Mika"),
        ("ribbon-cutting moment near a competition mat, faces softly out of focus", "Aoi"),
    ],
    "mugen": [
        ("editorial half-body portrait in a Hokkaido cafe with soft natural light", "Yuna"),
        ("Kyoto alley at dusk with paper-lantern bokeh, film grain", "Mio"),
        ("seaside walk in Kamakura at sunrise, light fog", "Emi"),
        ("minimalist Tokyo apartment near a window, monochrome mood", "Haruto"),
        ("Nagano farmhouse workshop, wood shavings catching light", "Nao"),
    ],
    "ma": [
        ("quiet zen rock garden in Kyoto, mid-morning soft sun", "Sora"),
        ("tea house doorway in Kanazawa during gentle rain", "Rui"),
        ("monochrome studio with a single window, contemplative pose", "Jun"),
    ],
    "muon": [
        ("dark recording-studio booth, faint LED glow on the chest design", "Taka"),
        ("late-night Tokyo rooftop, neon reflections softly diffused", "Io"),
        ("Berlin underground passage with diffuse fluorescent light", "Nina"),
    ],
    "nouns": [
        ("colourful Tokyo street-art alley at midday, candid stride", "Ren"),
        ("indoor skate ramp, sneakers visible, mid-laugh", "Sora"),
    ],
    "rashguard": [
        ("post-roll cooldown on the mat, towel around the neck, BJJ gym", "Ren"),
        ("warming up in a Brazilian-style academy, mid-stretch", "Sho"),
    ],
    "ryozo": [
        ("competition training camp warm-up corner, gi top draped on the shoulder", "Sho"),
        ("athlete locker-room candid moment, post-training", "Kazu"),
    ],
    "nojimahal": [
        ("Setouchi-island co-working cafe with sea breeze through the window", "Mika"),
        ("seaside terrace lunch break at golden hour", "Emi"),
    ],
    "elsoul": [
        ("intimate live-music venue green-room, warm tungsten light", "Io"),
        ("vinyl record shop in Shimokitazawa, browsing a sleeve", "Taka"),
    ],
    "ele": [
        ("modern Aoyama cafe interior, latte and notebook on the table", "Jun"),
        ("bright airy gallery space, mid-conversation", "Aoi"),
    ],
    "asoview": [
        ("trailhead at the start of an autumn hike, daypack on shoulder", "Nao"),
        ("hot-spring town main street, warm dusk lights", "Rui"),
    ],
    "mu": [
        ("quiet white-walled studio, single softbox light", "Yuna"),
        ("rooftop at blue hour with city lights in soft bokeh", "Haruto"),
    ],
    "default": [
        ("natural daylight studio with a soft white backdrop", "model"),
        ("urban Tokyo street at golden hour, candid frame", "model"),
        ("airy cafe near a window, warm interior tones", "model"),
        ("seaside terrace at sunset, breeze in the air", "model"),
    ],
}

PROMPT_TEMPLATE = """A natural, photographic editorial lifestyle shot.
{scene}.
The subject is an anonymous adult wearing the EXACT garment shown in the
reference image — preserve the chest graphic, print position, ink colour,
fabric colour, and silhouette exactly. Do not redesign the graphic, do
not add extra logos or text. No identifiable face details — keep the
person ambient (turned slightly, side angle, or face softly out of
frame). Casual, candid, not posed. Soft natural light, slight 35mm grain,
photojournalistic feel. Output a single square 800x800 photograph, no
collage, no overlays.""".strip()


# ── Helpers ────────────────────────────────────────────────────────────────


def brand_kind(brand: str) -> str:
    """Return the bucket key for SCENES_BY_KIND (first underscore segment)."""
    root = brand.split("_", 1)[0]
    return root if root in SCENES_BY_KIND else "default"


def pick_scene(brand: str, idx: int) -> tuple[str, str]:
    pool = SCENES_BY_KIND.get(brand_kind(brand), SCENES_BY_KIND["default"])
    return pool[idx % len(pool)]


def log_line(rec: dict) -> None:
    rec = {"ts": datetime.now(timezone.utc).isoformat(timespec="seconds"), **rec}
    LOG_PATH.open("a").write(json.dumps(rec, ensure_ascii=False) + "\n")


def fetch_mockup_bytes(product_id: int) -> bytes:
    """Pull the live storefront mockup PNG for use as visual reference."""
    url = f"{STORE_URL}/api/products/{product_id}/mockup.png"
    req = urllib.request.Request(url, headers={"User-Agent": "mu-bulk-lifestyle/1"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.read()


def gen_lifestyle_gemini(mockup_bytes: bytes, scene_prompt: str) -> bytes:
    """Call Gemini 3 Pro Image with the mockup as visual reference + scene prompt.

    Uses the raw REST endpoint (no SDK) for fewer moving parts in batch mode.
    Returns the generated image bytes (PNG)."""
    if not GEMINI_API_KEY:
        raise RuntimeError("GEMINI_API_KEY not set; cannot call Gemini")
    url = (
        f"https://generativelanguage.googleapis.com/v1beta/models/"
        f"{GEMINI_MODEL}:generateContent?key={GEMINI_API_KEY}"
    )
    body = json.dumps({
        "contents": [{
            "parts": [
                {"text": PROMPT_TEMPLATE.format(scene=scene_prompt)},
                {"inlineData": {
                    "mimeType": "image/png",
                    "data": base64.b64encode(mockup_bytes).decode(),
                }},
            ]
        }],
        "generationConfig": {"responseModalities": ["IMAGE", "TEXT"]},
    }).encode()
    req = urllib.request.Request(
        url, data=body, headers={"content-type": "application/json"}
    )
    with urllib.request.urlopen(req, timeout=180) as r:
        j = json.loads(r.read())
    parts = j.get("candidates", [{}])[0].get("content", {}).get("parts", [])
    for p in parts:
        d = p.get("inlineData") or p.get("inline_data")
        if d and d.get("data"):
            return base64.b64decode(d["data"])
    raise RuntimeError(f"Gemini returned no image; keys={list(j.keys())}")


def fallback_ambient(mockup_bytes: bytes) -> bytes:
    """No-cost fallback: blur + darken the mockup so the storefront has *some*
    ambient hero image even when Gemini fails. Returns JPEG bytes."""
    img = Image.open(io.BytesIO(mockup_bytes)).convert("RGB")
    # Heavy blur for "ambient" feel, slight darkening
    img = img.filter(ImageFilter.GaussianBlur(radius=12))
    img = ImageEnhance.Brightness(img).enhance(0.78)
    img = ImageEnhance.Color(img).enhance(0.85)
    out = io.BytesIO()
    img.save(out, format="JPEG", quality=82, optimize=True)
    return out.getvalue()


def to_jpeg(raw: bytes, size: int = 800) -> bytes:
    img = Image.open(io.BytesIO(raw)).convert("RGB")
    # Centre-crop to square then resize.
    w, h = img.size
    s = min(w, h)
    img = img.crop(((w - s) // 2, (h - s) // 2, (w + s) // 2, (h + s) // 2))
    img = img.resize((size, size), Image.LANCZOS)
    out = io.BytesIO()
    img.save(out, format="JPEG", quality=88, optimize=True)
    return out.getvalue()


def upload_r2(key: str, jpg_bytes: bytes) -> str:
    """Upload bytes to R2 at <bucket>/<key>, return the public URL."""
    with tempfile.NamedTemporaryFile(suffix=".jpg", delete=False) as f:
        f.write(jpg_bytes)
        tmp = f.name
    try:
        result = subprocess.run(
            [
                WRANGLER_BIN, "r2", "object", "put",
                f"{R2_BUCKET}/{key}",
                f"--file={tmp}",
                "--remote",
                "--content-type=image/jpeg",
            ],
            capture_output=True, text=True, timeout=120,
        )
        if result.returncode != 0:
            raise RuntimeError(f"wrangler r2 put failed: {result.stderr[-400:]}")
        return f"https://{PUBLIC_HOST}/{key}"
    finally:
        try:
            os.unlink(tmp)
        except OSError:
            pass


def patch_lifestyle_url(product_id: int, lifestyle_url: str) -> bool:
    """PATCH wearmu.com production via /api/admin/lifestyle.
    Returns True on a 2xx response with `ok` set."""
    body = json.dumps({"product_id": product_id, "lifestyle_url": lifestyle_url}).encode()
    url = f"{STORE_URL}/api/admin/lifestyle?token={ADMIN_TOKEN}"
    req = urllib.request.Request(
        url, data=body, method="PATCH",
        headers={"content-type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=20) as r:
            if r.status >= 300:
                return False
            data = json.loads(r.read())
            return bool(data.get("ok"))
    except Exception as e:
        log_line({"event": "patch_fail", "product_id": product_id, "err": str(e)[:200]})
        return False


# ── Target selection ──────────────────────────────────────────────────────


def _fetch_admin_brands() -> list[str]:
    """Return the full brand catalogue from /api/admin/products (the cheapest
    way to enumerate brands without paginating the whole product table)."""
    url = f"{STORE_URL}/api/admin/products?token={ADMIN_TOKEN}&limit=1&offset=0"
    req = urllib.request.Request(url, headers={"User-Agent": "mu-bulk/1"})
    with urllib.request.urlopen(req, timeout=30) as r:
        data = json.loads(r.read())
    return list(data.get("brands") or [])


def _fetch_brand_rows(brand: str, limit: int = 500) -> list[dict]:
    """Return all active products in a brand via the public per-brand listing.
    `lifestyle_url` is omitted when None (serde skip_serializing_if). We treat
    that as "missing"."""
    url = f"{STORE_URL}/api/products/{brand}?limit={limit}"
    req = urllib.request.Request(url, headers={"User-Agent": "mu-bulk/1"})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            rows = json.loads(r.read())
    except urllib.error.HTTPError as e:
        # 404 means no such brand or empty — treat as empty list.
        if e.code == 404:
            return []
        raise
    if not isinstance(rows, list):
        return []
    return rows


def select_targets(
    max_total: int,
    per_brand: int,
    brand_filter: str | None = None,
) -> list[dict]:
    """Pick targets fairly across brands.

    Source of truth: the wearmu.com production API (the local products.db
    has drifted IDs and is no longer authoritative — same brand may have
    a different `id` on production).

    Order within a brand: sold DESC, drop_num DESC. Brand fair-share:
    each root family contributes up to `per_brand` rows, then we
    round-robin across roots until we reach `max_total`.
    """
    brands = _fetch_admin_brands()
    if brand_filter:
        brands = [b for b in brands if b == brand_filter or b.startswith(brand_filter)]

    rows: list[dict] = []
    for b in brands:
        for r in _fetch_brand_rows(b):
            # Skip if already has a lifestyle_url
            if r.get("lifestyle_url"):
                continue
            # Must have a mockup we can fetch (else Gemini has nothing to reference)
            if not r.get("mockup_url"):
                continue
            rows.append({
                "id": r["id"],
                "brand": r.get("brand", b),
                "drop_num": r.get("drop_num", 0),
                "name": r.get("name", ""),
                "serial_code": r.get("serial_code") or "",
                "sold": r.get("sold", 0) or 0,
                "created_at": r.get("created_at", ""),
            })

    # Sort: highest-velocity (sold) first, then most recent.
    rows.sort(key=lambda r: (-(r["sold"] or 0), r["created_at"] or ""), reverse=False)
    # Note: -(sold) sorts higher sold first; created_at "" sorts last. Re-sort
    # cleanly:
    rows.sort(key=lambda r: (-(r["sold"] or 0), -(r["drop_num"] or 0)))

    # Group by brand-root (e.g. "jiufight_tee_sample" → "jiufight") so the
    # per-brand cap applies to the whole collab family, not each variant.
    def root(b: str) -> str:
        return b.split("_", 1)[0]

    by_root: dict[str, list[dict]] = {}
    for r in rows:
        by_root.setdefault(root(r["brand"]), []).append(r)
    # Cap each root family
    for b in by_root:
        by_root[b] = by_root[b][:per_brand]

    # Round-robin across roots so big families don't dominate the run.
    picked: list[dict] = []
    pointers = {b: 0 for b in by_root}
    roots_cycle = list(by_root.keys())
    while roots_cycle and len(picked) < max_total:
        next_roots: list[str] = []
        for b in roots_cycle:
            i = pointers[b]
            if i < len(by_root[b]):
                picked.append(by_root[b][i])
                pointers[b] = i + 1
                if len(picked) >= max_total:
                    break
                if pointers[b] < len(by_root[b]):
                    next_roots.append(b)
        roots_cycle = next_roots
    return picked


# ── Worker ────────────────────────────────────────────────────────────────


def process_one(row: dict, scene_idx: int, retries: int = 3) -> dict:
    pid: int = row["id"]
    brand: str = row["brand"]
    drop: int = row["drop_num"]
    # serial_code looks like "20260510-#090" — strip the "#" so the resulting
    # URL doesn't get parsed as a fragment by browsers / curl. Fall back to
    # the product id when no serial is set.
    raw_serial = row.get("serial_code") or str(pid)
    serial = raw_serial.replace("#", "").replace("/", "_").replace(" ", "_")
    scene, persona = pick_scene(brand, scene_idx)
    key = f"lifestyle/{brand}/{serial}.jpg"

    rec = {
        "event": "product",
        "product_id": pid,
        "brand": brand,
        "drop": drop,
        "scene": persona,
        "r2_key": key,
    }

    # Fetch reference mockup
    try:
        mockup_bytes = fetch_mockup_bytes(pid)
    except Exception as e:
        rec.update(ok=False, stage="fetch_mockup", err=str(e)[:200])
        log_line(rec)
        return rec

    # Try Gemini with retries
    img_bytes: bytes | None = None
    mode = "gemini"
    last_err: str | None = None
    for attempt in range(retries):
        try:
            raw = gen_lifestyle_gemini(mockup_bytes, scene)
            img_bytes = to_jpeg(raw, size=800)
            break
        except urllib.error.HTTPError as e:
            msg = ""
            try:
                msg = e.read().decode(errors="replace")[:200]
            except Exception:
                pass
            last_err = f"HTTP {e.code}: {msg}"
        except Exception as e:
            last_err = f"{type(e).__name__}: {str(e)[:200]}"
        # exponential-ish backoff
        time.sleep(2 + attempt * 3)

    if img_bytes is None:
        # No-cost fallback so the row still gets *something* surfaced
        try:
            img_bytes = fallback_ambient(mockup_bytes)
            mode = "fallback"
        except Exception as e:
            rec.update(ok=False, stage="gemini", err=last_err, fallback_err=str(e)[:200])
            log_line(rec)
            return rec

    # Upload
    try:
        url = upload_r2(key, img_bytes)
    except Exception as e:
        rec.update(ok=False, stage="upload", err=str(e)[:200], mode=mode)
        log_line(rec)
        return rec

    # DB write
    if not patch_lifestyle_url(pid, url):
        rec.update(ok=False, stage="patch", url=url, mode=mode)
        log_line(rec)
        return rec

    rec.update(
        ok=True,
        mode=mode,
        url=url,
        bytes=len(img_bytes),
        gemini_err=(last_err if mode == "fallback" else None),
    )
    log_line(rec)
    return rec


# ── Main ──────────────────────────────────────────────────────────────────


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Bulk lifestyle generator")
    ap.add_argument("--max-products", type=int, default=100,
                    help="Total products to process this run (default 100)")
    ap.add_argument("--per-brand", type=int, default=20,
                    help="Max products per brand for fairness (default 20)")
    ap.add_argument("--batch-size", type=int, default=5,
                    help="Items per batch before sleeping (default 5)")
    ap.add_argument("--batch-sleep", type=float, default=10.0,
                    help="Seconds to sleep between batches (default 10)")
    ap.add_argument("--brand", type=str, default=None,
                    help="Only this brand prefix (e.g. jiufight, mugen)")
    ap.add_argument("--dry-run", action="store_true",
                    help="Only print the plan; do not call Gemini / R2")
    args = ap.parse_args(argv)

    if not ADMIN_TOKEN or ADMIN_TOKEN == "mu-admin-2026":
        print("[warn] MU_ADMIN_TOKEN env var not set; using the placeholder "
              "default which production will reject. Source /Users/yuki/.env "
              "before running.", file=sys.stderr)

    targets = select_targets(
        max_total=args.max_products,
        per_brand=args.per_brand,
        brand_filter=args.brand,
    )

    # Brand distribution summary (rolled-up by root family)
    dist: dict[str, int] = {}
    root_dist: dict[str, int] = {}
    for r in targets:
        dist[r["brand"]] = dist.get(r["brand"], 0) + 1
        root_key = r["brand"].split("_", 1)[0]
        root_dist[root_key] = root_dist.get(root_key, 0) + 1
    print(f"Plan: {len(targets)} products across {len(dist)} brands "
          f"/ {len(root_dist)} root families")
    for b, n in sorted(root_dist.items(), key=lambda x: -x[1]):
        print(f"  {b:20s} {n}")
    log_line({"event": "run_start", "total": len(targets),
              "brands": dist, "root_families": root_dist,
              "max_products": args.max_products, "per_brand": args.per_brand,
              "gemini_available": bool(GEMINI_API_KEY)})

    if args.dry_run:
        return 0

    if not GEMINI_API_KEY:
        print("[warn] GEMINI_API_KEY not set — every product will use the "
              "Pillow fallback (blurred mockup). Source /Users/yuki/.env if "
              "you want real Gemini generation.", file=sys.stderr)

    ok = 0
    fb = 0
    fail = 0
    started = time.time()
    for i, row in enumerate(targets):
        rec = process_one(row, scene_idx=i)
        if rec.get("ok") and rec.get("mode") == "gemini":
            ok += 1
        elif rec.get("ok") and rec.get("mode") == "fallback":
            fb += 1
        else:
            fail += 1
        # Brief per-row line (no key leakage)
        status = "ok" if rec.get("ok") else "FAIL"
        mode = rec.get("mode", "-")
        print(f"  [{i+1:3d}/{len(targets)}] {status:4s} {mode:8s} "
              f"id={rec['product_id']} brand={rec['brand']} "
              f"{rec.get('url') or rec.get('err','')[:80]}")
        # Batch sleep to dodge rate limits
        if (i + 1) % args.batch_size == 0 and (i + 1) < len(targets):
            time.sleep(args.batch_sleep)

    elapsed = time.time() - started
    # Gemini 3 Pro Image ≈ $0.04/img (text+image generation)
    cost_est = round(ok * 0.04, 2)
    summary = {
        "event": "run_end",
        "total": len(targets),
        "ok_gemini": ok,
        "ok_fallback": fb,
        "fail": fail,
        "elapsed_s": round(elapsed, 1),
        "est_cost_usd": cost_est,
    }
    log_line(summary)
    print(f"\nDone — gemini={ok} fallback={fb} fail={fail} "
          f"({elapsed:.0f}s, ~${cost_est:.2f} estimated Gemini cost)")
    return 0 if fail == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
