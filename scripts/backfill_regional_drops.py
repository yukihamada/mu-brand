#!/usr/bin/env python3
"""
Backfill ~100 historical drops per city for the MU Regional editions.

For each of 6 cities × 100 drop_nums (1001..1100), this script:
  1. Composes a unique prompt by combining (motif × time × season × treatment).
  2. Renders the design via Gemini 3 Pro Image.
  3. Uploads the PNG to R2 at lifestyle.wearmu.com/regional/<city>/<drop>.png.
  4. POSTs to /api/admin/import to register the SKU on wearmu.com.
  5. Logs progress to stdout (use --log /tmp/backfill.log when backgrounded).

Idempotent: skips a drop if its R2 object already exists AND a matching
products row exists. Crashed mid-run? Re-run; only missing ones regenerate.

Pricing ladder (deterministic on drop_num):
  - default     ¥4,900
  - drop % 10 == 0   ¥6,800 (standard)
  - drop % 25 == 0   ¥9,800 (premium / collector)

Run:
  python scripts/backfill_regional_drops.py --cities all --count 100
  python scripts/backfill_regional_drops.py --cities tokyo,kyoto --count 20
  python scripts/backfill_regional_drops.py --resume          # picks up where last run ended
"""
from __future__ import annotations
import argparse
import base64
import datetime as dt
import json
import os
import subprocess
import sys
import time
import urllib.request
import urllib.error
from pathlib import Path

ENV_FILE = Path("/Users/yuki/.env")

# ── Env loading (force /Users/yuki/.env over stale shell exports) ──
env = {}
if ENV_FILE.exists():
    for line in ENV_FILE.read_text().splitlines():
        line = line.strip()
        if "=" in line and not line.startswith("#"):
            k, v = line.split("=", 1)
            env[k.strip()] = v.strip().strip('"').strip("'")
for k, v in env.items():
    if k in ("GEMINI_API_KEY", "MU_ADMIN_TOKEN"):
        os.environ[k] = v

GEMINI_KEY = os.environ.get("GEMINI_API_KEY", "")
ADMIN_TOKEN = os.environ.get("MU_ADMIN_TOKEN", "")
STORE = "https://wearmu.com"
GEMINI_MODEL = "gemini-3-pro-image-preview"

if not GEMINI_KEY:
    sys.exit("missing GEMINI_API_KEY")
if not ADMIN_TOKEN:
    sys.exit("missing MU_ADMIN_TOKEN")

# ── Concept matrices per city ─────────────────────────────────────────────
CITIES = {
    "tokyo": {
        "jp": "東京", "en": "TOKYO", "lat": 35.6762, "lon": 139.6503,
        "motifs": [
            ("skyline 5 lines", "5 staggered horizontal lines representing Tokyo skyline density"),
            ("subway abstract", "an abstract subway-map node — 3 lines crossing at a single point"),
            ("cherry petals",   "5-7 thin curved cherry blossom petals scattered as if mid-fall"),
            ("Shinjuku grid",   "a tight 3x3 grid of small squares — Shinjuku block density"),
            ("Shibuya scramble","6 intersecting diagonals from a single center point — scramble crossing"),
            ("Asakusa torii",   "a single minimalist 鳥居 (torii gate) silhouette outline"),
            ("Tokyo Tower",     "a tall thin triangle, just the silhouette tip of Tokyo Tower"),
            ("neon kanji 都",   "the single kanji 都 (capital) rendered minimal, white ink"),
            ("rain on glass",   "5 vertical thin lines suggesting rain on a window"),
            ("ramen steam",     "3 wavy thin lines rising — steam from a ramen bowl"),
        ],
    },
    "kyoto": {
        "jp": "京都", "en": "KYOTO", "lat": 35.0116, "lon": 135.7681,
        "motifs": [
            ("moss circle",    "a single hand-drawn imperfect circle, moss-asymmetric"),
            ("torii silhouette","a minimalist 鳥居 torii silhouette, thin lines"),
            ("bamboo grove",   "5 vertical thin lines of varying height — bamboo stalks"),
            ("stone ripple",   "concentric thin arcs — stone garden raked ripples"),
            ("tea bowl",       "a small open semi-circle, top edge — tea bowl rim profile"),
            ("maple leaf",     "a single stylized 5-point maple leaf outline"),
            ("Kamogawa river", "a long horizontal wavy line — Kamo river flow"),
            ("Arashiyama",     "soft triangular mountain silhouette, no details"),
            ("Kinkaku gold",   "a small square outlined with a single corner triangle — Kinkaku-ji nod"),
            ("Tetsugaku path", "a single curving thin path line — Philosopher's Walk"),
        ],
    },
    "osaka": {
        "jp": "大阪", "en": "OSAKA", "lat": 34.6937, "lon": 135.5023,
        "motifs": [
            ("eight bridges",  "horizontal wavy line crossed by 8 small vertical strokes"),
            ("tsutenkaku",     "tall thin trapezoid tower silhouette — Tsutenkaku abstract"),
            ("Dotonbori sign", "a single bold horizontal rectangle with 'OSAKA' inside, neon-flat"),
            ("takoyaki 3x3",   "3x3 grid of small circles — takoyaki tray"),
            ("okonomiyaki",    "a large circle with a small spatula triangle on the side"),
            ("Hanshin yellow", "a single thick horizontal stripe — Hanshin team yellow nod (black ink)"),
            ("Osaka Castle",   "a tiered castle roof silhouette, only the curves of 3 roof lines"),
            ("fugu fish",      "a soft puffer-fish outline, simple closed curve"),
            ("manzai bubble",  "a speech bubble shape, blank inside"),
            ("kuidaore figure","a stylized stick-figure with raised chopsticks"),
        ],
    },
    "sapporo": {
        "jp": "札幌", "en": "SAPPORO", "lat": 43.0618, "lon": 141.3545,
        "motifs": [
            ("snowflake",      "6-pointed snowflake mark, clean radial thin lines"),
            ("ramen bowl",     "an oval bowl rim with 2 chopsticks crossed over it"),
            ("clock tower",    "the Sapporo clock tower silhouette, only the roof + clock face"),
            ("Susukino sign",  "a single tall vertical line with 3 horizontal marks — neon abstract"),
            ("Odori park",     "a wide horizontal park layout shown as 3 parallel lines"),
            ("beer mug",       "a tall rectangle with a curved handle on the right — beer mug minimal"),
            ("Mt. Moiwa",      "a soft conical mountain shape with a single rope-line up the side"),
            ("ice crystal",    "an angular asymmetric ice crystal shape, no symmetry"),
            ("wolf face",      "a Hokkaido wolf silhouette face, only the ears + nose"),
            ("salmon stream",  "a horizontal river line with a single fish-shape ascending"),
        ],
    },
    "fukuoka": {
        "jp": "福岡", "en": "FUKUOKA", "lat": 33.5904, "lon": 130.4017,
        "motifs": [
            ("sea + yatai",    "two horizontal lines, a small triangle (yatai) between them"),
            ("Hakata ramen",   "a bowl with 3 thin curving steam lines rising"),
            ("Yamakasa float", "a small pyramidal float silhouette with 2 vertical handles"),
            ("mentaiko grains","a curved oval with several tiny dots inside — mentaiko pattern"),
            ("Itoshima sun",   "a single circle low on the canvas — sunset over Itoshima"),
            ("Hakata-ben word","stylized 'やけん' typography in white ink"),
            ("Genkainada",     "a horizontal wave line with 3 small triangles (boats)"),
            ("Fukuoka castle", "a low square castle outline + small triangle moat"),
            ("Tenjin tower",   "a tall narrow rectangle with one window-square highlighted"),
            ("Hakata doll",    "an oval head silhouette with a small fan triangle"),
        ],
    },
    "okinawa": {
        "jp": "沖縄", "en": "OKINAWA", "lat": 26.2124, "lon": 127.6809,
        "motifs": [
            ("paikaji wind",   "a single thin curving line with one open circle — south breeze"),
            ("shisa face",     "an abstract shisa lion silhouette — only the mane and eyes"),
            ("alpinia bloom",  "a small open flower with 5 thin petals — 月桃"),
            ("ocean wave",     "a long horizontal wave with one small triangle (sail)"),
            ("Shuri castle",   "a low tiered roof silhouette, gentle curves"),
            ("eisa drum",      "a circular drum with 2 vertical sticks crossed"),
            ("hibiscus",       "5 simple petals arranged in a flat star pattern"),
            ("coral branch",   "a branching coral silhouette, 3-pronged"),
            ("Sanshin string", "3 horizontal thin lines + a small triangle (sanshin face)"),
            ("Kerama dot",     "a single small filled circle low on the canvas — distant island"),
        ],
    },
}

TIMES = ["dawn", "noon", "dusk", "night"]
SEASONS = ["spring", "summer", "autumn", "winter"]


def pick(drop_num: int, city: str) -> dict:
    """Deterministic pick based on drop_num so the same drop always
    regenerates the same prompt."""
    motifs = CITIES[city]["motifs"]
    m_idx = (drop_num - 1) % len(motifs)
    t_idx = (drop_num // len(motifs)) % len(TIMES)
    s_idx = (drop_num // (len(motifs) * len(TIMES))) % len(SEASONS)
    treatment = "black-on-cream" if drop_num % 2 == 1 else "white-on-charcoal"
    return {
        "motif": motifs[m_idx],
        "time": TIMES[t_idx],
        "season": SEASONS[s_idx],
        "treatment": treatment,
    }


def price_for(drop_num: int) -> int:
    if drop_num % 25 == 0: return 9800     # premium
    if drop_num % 10 == 0: return 6800     # standard
    return 4900                             # entry


PROMPT_TEMPLATE = """A high-quality lifestyle product photograph of a heavyweight {treatment_label} cotton T-shirt, laid flat on a soft warm neutral background (light beige paper or natural wood). The T-shirt is the ONLY product in frame, centered, slightly above the horizontal midline so the print area is most visible.

Centered on the chest area of the shirt, printed in {ink_label} (DTG print), is the following minimalist design — and ONLY this design, nothing else:

TOP (small {ink_label} sans-serif text, ~8mm tall):
  {name_en} #{drop_num:04d}
  {name_jp} · {season} · {time}

CENTER (the visual mark — clean {ink_label} line art, ~12cm tall, restrained, museum-quality minimalism):
  {motif_desc}

BOTTOM (very small {ink_label} monospaced text, ~5mm tall):
  {coords}

CRITICAL CONSTRAINTS — follow exactly:
- Print uses {ink_label} only on {treatment_label} fabric. Soft contrast.
- No other text, no logos, no MU branding elsewhere on the shirt.
- No model wearing it. Sleeves visible. Relaxed-fit unisex tee.
- Lighting: soft, natural, slightly directional from upper left.
- Background: matte and uncluttered. One subtle shadow under the shirt.
- 1:1 aspect ratio, hi-resolution photographic realism.
- Style: Aesop product photography + COS minimalism + 京焼 catalog quality.
- Do NOT add any patterns, watermarks, color tags, hangtags, or accessories.

Edition: MU Regional · {name_jp} #{drop_num:04d}, drop_num {drop_num}, {season} {time}.
"""


def render_one(city: str, drop_num: int, out_path: Path) -> bool:
    """Render via Gemini, save to out_path. Returns True on success."""
    p = pick(drop_num, city)
    info = CITIES[city]
    if p["treatment"] == "black-on-cream":
        treatment_label = "off-white / cream"
        ink_label = "deep black"
    else:
        treatment_label = "deep charcoal"
        ink_label = "warm off-white"

    motif_name, motif_desc = p["motif"]
    prompt = PROMPT_TEMPLATE.format(
        treatment_label=treatment_label,
        ink_label=ink_label,
        name_en=info["en"],
        name_jp=info["jp"],
        drop_num=drop_num,
        season=p["season"],
        time=p["time"],
        motif_desc=motif_desc,
        coords=f"{info['lat']:.4f}°N · {info['lon']:.4f}°E",
    )

    payload = {
        "contents": [{"parts": [{"text": prompt}]}],
        "generationConfig": {"responseModalities": ["IMAGE", "TEXT"]},
    }
    url = (f"https://generativelanguage.googleapis.com/v1beta/models/"
           f"{GEMINI_MODEL}:generateContent?key={GEMINI_KEY}")
    req = urllib.request.Request(
        url, data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=180) as resp:
            data = json.loads(resp.read())
    except Exception as e:
        print(f"    ! gemini: {type(e).__name__}: {str(e)[:160]}", flush=True)
        return False
    parts = (data.get("candidates", [{}])[0].get("content") or {}).get("parts") or []
    inline = None
    for pa in parts:
        ind = pa.get("inline_data") or pa.get("inlineData")
        if ind and ind.get("data"):
            inline = ind
            break
    if not inline:
        print(f"    ! no image in gemini response", flush=True)
        return False
    out_path.write_bytes(base64.b64decode(inline["data"]))
    return True


def r2_upload(local_path: Path, key: str) -> str:
    """Upload to wearmu-lifestyle bucket via wrangler. Returns the public URL."""
    cmd = [
        "wrangler", "r2", "object", "put",
        f"wearmu-lifestyle/{key}",
        "--file", str(local_path),
        "--content-type", "image/png",
        "--remote",
    ]
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
    if r.returncode != 0:
        raise RuntimeError(f"wrangler upload failed: {r.stderr[:200]}")
    return f"https://lifestyle.wearmu.com/{key}"


def import_product(brand: str, drop_num: int, name: str, img_url: str,
                   price: int, weather_data: dict) -> int | None:
    payload = {
        "brand": brand, "drop_num": drop_num, "name": name,
        "design_url": img_url, "mockup_url": img_url,
        "price_jpy": price, "inventory": 1,           # 1 of 1 per drop in this set
        "weather_data": json.dumps(weather_data),
        "prompt_hash": None, "seed_data": None,
        "auction_end": None, "nft_mint": None, "is_ice": False,
    }
    url = f"{STORE}/api/admin/import?token={ADMIN_TOKEN}"
    req = urllib.request.Request(
        url, data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            body = json.loads(r.read())
            return body.get("id") or body.get("product_id")
    except urllib.error.HTTPError as e:
        print(f"    ! import HTTP {e.code}: {e.read().decode()[:200]}", flush=True)
        return None
    except Exception as e:
        print(f"    ! import: {type(e).__name__}: {str(e)[:160]}", flush=True)
        return None


def already_exists(city: str, drop_num: int) -> tuple[bool, bool]:
    """Returns (r2_exists, db_exists)."""
    img_url = f"https://lifestyle.wearmu.com/regional/{city}/{drop_num:04d}.png"
    r2 = False
    try:
        req = urllib.request.Request(img_url, method="HEAD")
        with urllib.request.urlopen(req, timeout=10) as resp:
            r2 = (resp.status == 200)
    except Exception:
        pass
    # DB check via product lookup. Lightweight: GET /api/products/<brand>?limit=200
    db = False
    try:
        u = f"{STORE}/api/products/regional_{city}?limit=200"
        with urllib.request.urlopen(u, timeout=15) as resp:
            arr = json.loads(resp.read())
            db = any(int(p.get("drop_num", -1)) == drop_num for p in arr)
    except Exception:
        pass
    return r2, db


def process_city(city: str, count: int, start: int = 1001, skip_existing: bool = True) -> dict:
    info = CITIES[city]
    out_dir = Path(f"/tmp/mu_backfill/{city}")
    out_dir.mkdir(parents=True, exist_ok=True)
    stats = {"city": city, "rendered": 0, "uploaded": 0, "imported": 0, "skipped": 0, "errors": 0}

    for i in range(count):
        drop_num = start + i
        local = out_dir / f"{drop_num:04d}.png"
        key = f"regional/{city}/{drop_num:04d}.png"
        r2_ok, db_ok = (False, False)
        if skip_existing:
            r2_ok, db_ok = already_exists(city, drop_num)
        if r2_ok and db_ok:
            stats["skipped"] += 1
            if i % 10 == 0:
                print(f"  [{city} {drop_num}] both exist — skip", flush=True)
            continue

        # Render
        if not local.exists() or local.stat().st_size < 50_000:
            ok = render_one(city, drop_num, local)
            if not ok:
                stats["errors"] += 1
                continue
        stats["rendered"] += 1
        # Upload (idempotent — wrangler put overwrites)
        try:
            img_url = r2_upload(local, key)
            stats["uploaded"] += 1
        except Exception as e:
            print(f"  ! r2 upload {city}/{drop_num}: {e}", flush=True)
            stats["errors"] += 1
            continue
        # Import
        p = pick(drop_num, city)
        motif_name, _ = p["motif"]
        name = f"REGIONAL · {info['jp']} #{drop_num:04d} — {motif_name}"
        weather = {
            "city": city, "lat": info["lat"], "lon": info["lon"],
            "motif": motif_name, "time": p["time"], "season": p["season"],
            "treatment": p["treatment"], "backfill": True,
        }
        price = price_for(drop_num)
        pid = import_product(f"regional_{city}", drop_num, name, img_url, price, weather)
        if pid:
            stats["imported"] += 1
            tier = "PREM" if price == 9800 else ("STD" if price == 6800 else "ENTRY")
            print(f"  ✓ [{city} {drop_num}] {motif_name:<20} {p['time']:<5} {p['season']:<7} {tier:<5} ¥{price:,} pid={pid}", flush=True)
        else:
            stats["errors"] += 1
    return stats


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--cities", default="all", help="comma-separated, or 'all'")
    p.add_argument("--count", type=int, default=100, help="drops per city")
    p.add_argument("--start", type=int, default=1001, help="first drop_num (avoid clash with existing 1+101)")
    p.add_argument("--no-skip", action="store_true", help="regenerate even if exists")
    args = p.parse_args()

    if args.cities == "all":
        cities = list(CITIES.keys())
    else:
        cities = [c.strip() for c in args.cities.split(",") if c.strip() in CITIES]

    start = time.time()
    all_stats = []
    for c in cities:
        print(f"\n=== {c.upper()} ({args.count} drops, start={args.start}) ===", flush=True)
        s = process_city(c, args.count, start=args.start, skip_existing=not args.no_skip)
        all_stats.append(s)
        print(f"  done: rendered={s['rendered']} uploaded={s['uploaded']} imported={s['imported']} skipped={s['skipped']} errors={s['errors']}", flush=True)

    elapsed = time.time() - start
    print(f"\n=== TOTAL ===")
    for s in all_stats:
        print(f"  {s['city']:<8} R={s['rendered']:>3} U={s['uploaded']:>3} I={s['imported']:>3} skip={s['skipped']:>3} err={s['errors']:>2}")
    print(f"elapsed: {elapsed/60:.1f} min")


if __name__ == "__main__":
    main()
