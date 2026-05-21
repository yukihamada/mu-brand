#!/usr/bin/env python3
"""Generate MU Rashguard line — 10 premium BJJ rashguard designs.

Internal MU brand (NOT a collab). Pure no-gi grappling / BJJ market line:
- dye-sublimation full-coverage long-sleeve compression rashguard
- chest center main graphic + sleeve auxiliary pattern
- Japanese motifs (koi / sakura / bushido / tiger / daruma / seigaiha /
  asanoha / shark) + grappling motifs (triangle choke silhouette / belt)
- premium black-and-gold palette dominant
- ¥7,800 / inventory 50 per SKU / 50% donation MU philosophy maintained

Model: gemini-3-pro-image-preview (per ~/.claude memory: keys/image).
Key: GEMINI_API_KEY from /Users/yuki/.env.

Idempotent: existing numbered files are skipped unless FORCE=1.

Usage:
    cd /Users/yuki/workspace/mu-brand
    python3 scripts/gen_rashguard_line.py                # all 10
    python3 scripts/gen_rashguard_line.py 1 2 3          # subset
    FORCE=1 python3 scripts/gen_rashguard_line.py        # overwrite

After generation:
- 10 PNG files at /store/static/proposals/rashguard-mockup-{1..10}.png
- 10 rows inserted into /products.db with brand='rashguard'
- /store/partner_specs/rashguard.json written with 10 prompt templates
- /merch/bjj category map updated separately (one-line main.rs edit)

IP boundary: pure MU internal brand. No partner trademarks. No real
fighter likenesses. Original motifs only. Generic Japanese cultural
motifs (sakura / koi / daruma) are public-domain inspirations.
"""
from __future__ import annotations
import base64
import hashlib
import io
import json
import os
import sqlite3
import sys
import time
from pathlib import Path

# Force-override GEMINI_API_KEY from /Users/yuki/.env (per feedback memory).
_ENV_FILE = Path("/Users/yuki/.env")
if _ENV_FILE.exists():
    for _line in _ENV_FILE.read_text().splitlines():
        _line = _line.strip()
        if not _line or _line.startswith("#") or "=" not in _line:
            continue
        _k, _, _v = _line.partition("=")
        if _k.strip() == "GEMINI_API_KEY":
            os.environ["GEMINI_API_KEY"] = _v.strip().strip("'\"")
os.environ.pop("GOOGLE_API_KEY", None)

from google import genai
from google.genai import types
from PIL import Image

PROJECT_ROOT = Path("/Users/yuki/workspace/mu-brand")
PROPOSALS_DIR = PROJECT_ROOT / "store" / "static" / "proposals"
FACTORY_DB = PROJECT_ROOT / "products.db"
STORE_DB = PROJECT_ROOT / "store" / "products.db"
SPEC_PATH = PROJECT_ROOT / "store" / "partner_specs" / "rashguard.json"

GEMINI_MODEL = "gemini-3-pro-image-preview"
OUT_SIZE = 800
PNG_MAX_BYTES = 600_000

# Shared style preamble — feeds every prompt so all 10 SKUs look like a set.
STYLE_PREAMBLE = (
    "Premium black BJJ rashguard, long sleeve, athletic compression fit, "
    "dye-sublimation full-coverage print, invisible mannequin or flat lay, "
    "800x800 e-commerce mockup, off-white seamless backdrop with gentle "
    "natural shadow. Single garment, no people, no extra text outside what "
    "is explicitly specified, no watermarks. Crisp focus, balanced exposure, "
    "true-to-fabric color. The chest design must read clearly at thumbnail size."
)

IP_NOTE = (
    "CRITICAL: do NOT render the words 'rvddw', 'reversal', 'shoyoroll', "
    "'tatami', 'hayabusa', or any third-party brand logotype. Do NOT depict "
    "real fighters, real teams, or real championship belts. Use only "
    "original motifs and generic Japanese cultural / animal symbolism."
)

# Each entry: (number, design_slug, jp_name, en_name, palette, motif_prompt)
JOBS: list[tuple[int, str, str, str, str, str]] = [
    (
        1, "koi-ascending", "鯉昇竜", "Koi Ascending",
        "deep matte black base with metallic gold accents",
        "Chest center: a powerful koi fish mid-transformation into a dragon, "
        "rendered in flowing brush-stroke metallic gold lines on black, "
        "occupying roughly 45% of the chest area. Scales transition from fish "
        "to dragon scales as the eye moves upward. Water splashes around the "
        "base. Both sleeves: continuous water-current pattern in subtle gold "
        "linework on black, flowing from shoulder to cuff. Bold yet refined.",
    ),
    (
        2, "cherry-snow", "桜雪", "Cherry Snow",
        "off-white base with soft cherry-pink and faint charcoal accents",
        "Chest center: delicate cherry blossoms (sakura) caught mid-fall, "
        "scattered minimalist composition occupying ~40% of chest. Petals "
        "rendered in soft pink with faint charcoal outlines, suggesting a "
        "single quiet moment. Both sleeves: a few drifting petals in sparse, "
        "elegant arrangement, mostly empty space. Minimal, contemplative.",
    ),
    (
        3, "sankaku", "三角", "Sankaku",
        "pure matte black with bright off-white geometric linework",
        "Chest center: an abstract geometric triangle-choke silhouette "
        "rendered as three interlocking triangles in clean off-white line art, "
        "~35% of chest. Strictly technical / minimalist / architectural feel — "
        "no figurative bodies, only pure geometry suggesting the technique. "
        "Both sleeves: thin off-white triangular tessellation pattern, "
        "small-scale repeat. Modern, technical, restrained.",
    ),
    (
        4, "bushido", "武士道", "Bushido",
        "matte black base with metallic gold kanji",
        "Chest center: the two kanji characters '武士道' (bushido) rendered "
        "large in elegant brushwork-style metallic gold calligraphy, ~50% of "
        "chest, stacked vertically or balanced horizontally with traditional "
        "presence. Below the kanji a small thin gold horizontal bar with a "
        "thin gold circle and another bar — the MU '━◯━' monogram — at about "
        "10% width. Both sleeves: subtle gold seigaiha-wave linework along "
        "the outer seam. Traditional, dignified.",
    ),
    (
        5, "tiger-eye", "虎の眼", "Tiger Eye",
        "matte black with deep crimson red and metallic gold accents",
        "Chest center: a photorealistic-illustrated tiger face staring "
        "directly at the viewer, rendered in detailed gold and crimson "
        "linework on black, occupying ~50% of chest. Intense eyes glow gold. "
        "Black tiger stripes integrate with the black fabric, gold fur "
        "highlights, red shadow accents. Both sleeves: subtle tiger-stripe "
        "claw-mark slashes in red and gold along the forearm. Aggressive, "
        "predatory.",
    ),
    (
        6, "daruma", "達磨", "Daruma",
        "matte black base with deep crimson red and white accents",
        "Chest center: a stylized Daruma doll (round, eyebrowed, one eye "
        "painted) in bold red and white illustration, ~40% of chest, with "
        "the four kanji '七転八起' (nana-korobi ya-oki — 'fall seven, rise "
        "eight') in elegant black calligraphy beneath. Both sleeves: tiny "
        "repeating Daruma silhouettes in red along the outer sleeve. "
        "Unbreakable spirit, traditional folk-art energy.",
    ),
    (
        7, "seigaiha", "青海波", "Seigaiha",
        "deep navy blue base with metallic silver linework",
        "All-over base: a full-coverage seigaiha (青海波 — overlapping wave) "
        "pattern in thin metallic silver lines on navy across the entire "
        "rashguard body, including front, back, and sleeves. Chest center: "
        "a thin silver moon-phase row (new → waxing → full → waning → new) "
        "subtly emerging from the wave pattern, ~25% of chest width. Flowing, "
        "endless, meditative.",
    ),
    (
        8, "asanoha", "麻の葉", "Asanoha",
        "matte black base with metallic gold linework",
        "All-over base: a precise asanoha (麻の葉 — hemp leaf) geometric "
        "lattice pattern in thin matte gold lines on black, small repeat, "
        "covering body and sleeves uniformly. Chest center: the MU monogram "
        "'━◯━' rendered larger in metallic gold (a short horizontal bar, a "
        "thin circle ring, another bar) at ~20% chest width, sitting cleanly "
        "atop the asanoha lattice. Minimal, traditional, modern.",
    ),
    (
        9, "same-shark", "鮫", "Same",
        "deep navy blue base with metallic silver and pale slate accents",
        "Chest center: a sleek silhouetted shark cutting diagonally across "
        "the chest in metallic silver linework on navy, ~50% of chest, with "
        "streamlined motion lines trailing behind it suggesting speed and "
        "dominance. Both sleeves: thin silver streamline pattern flowing "
        "from shoulder to cuff like cutting water. Fast, controlling, "
        "predatory.",
    ),
    (
        10, "kintai-gold-belt", "金帯", "Kintai",
        "matte black with metallic gold gradient",
        "Chest center: a horizontal belt-shape graphic spanning ~60% of chest "
        "width, gradient from matte black on the left to brilliant metallic "
        "gold on the right, with a single small black bar at the gold end "
        "suggesting a belt stripe. Below the belt the single kanji '黒' (kuro "
        "/ black) in subtle dark-gold calligraphy, ~10% chest. Both sleeves: "
        "thin gold horizontal pinstripe along the outer seam, suggesting "
        "belt ranks. Achievement, ascension.",
    ),
]


def build_prompt(design_slug: str, jp: str, en: str, palette: str, motif: str) -> str:
    return (
        f"{STYLE_PREAMBLE}\n\n"
        f"Color palette: {palette}.\n\n"
        f"Design (printed dye-sublimation full-coverage on the rashguard, "
        f"chest center primary + sleeve auxiliary):\n{motif}\n\n"
        f"{IP_NOTE}\n\n"
        f"Render as the hero e-commerce product photo for SKU "
        f"'rashguard-{design_slug}' ({jp} / {en})."
    )


def gemini_render(client: genai.Client, prompt: str) -> bytes | None:
    """Text-only prompt → 800x800 PNG bytes (or None on failure)."""
    try:
        resp = client.models.generate_content(
            model=GEMINI_MODEL,
            contents=[prompt],
            config=types.GenerateContentConfig(
                response_modalities=["IMAGE", "TEXT"],
            ),
        )
    except Exception as exc:  # noqa: BLE001
        print(f"    gemini error: {type(exc).__name__}: {exc}")
        return None
    if not resp.candidates:
        print("    gemini returned no candidates")
        return None
    for part in resp.candidates[0].content.parts:
        inline = getattr(part, "inline_data", None)
        if not inline:
            continue
        data = inline.data
        if isinstance(data, str):
            data = base64.b64decode(data)
        try:
            im = Image.open(io.BytesIO(data)).convert("RGB")
        except Exception as exc:  # noqa: BLE001
            print(f"    pillow decode error: {exc}")
            return None
        if im.size != (OUT_SIZE, OUT_SIZE):
            im = im.resize((OUT_SIZE, OUT_SIZE), Image.LANCZOS)
        buf = io.BytesIO()
        im.save(buf, format="PNG", optimize=True)
        data_out = buf.getvalue()
        if len(data_out) <= PNG_MAX_BYTES:
            return data_out
        # Oversize — re-encode at lower resolution.
        im2 = im.resize((640, 640), Image.LANCZOS)
        buf = io.BytesIO()
        im2.save(buf, format="PNG", optimize=True)
        return buf.getvalue()
    print("    gemini returned text-only (no image part)")
    return None


def render_one(client: genai.Client, num: int, design_slug: str, jp: str,
               en: str, palette: str, motif: str, force: bool
               ) -> tuple[int, str, str]:
    """Returns (bytes_written, method, prompt_text). 0 bytes = skipped."""
    out_path = PROPOSALS_DIR / f"rashguard-mockup-{num}.png"
    prompt = build_prompt(design_slug, jp, en, palette, motif)
    if out_path.exists() and not force:
        return out_path.stat().st_size, "skip-exists", prompt

    method = "gemini"
    png_bytes: bytes | None = None
    for attempt in range(2):
        png_bytes = gemini_render(client, prompt)
        if png_bytes and len(png_bytes) >= 40_000:
            break
        size = len(png_bytes) if png_bytes else 0
        print(f"    attempt {attempt + 1}: {size} bytes — "
              f"{'retry' if attempt == 0 else 'fail'}")
        if attempt == 0:
            time.sleep(4)

    if not png_bytes:
        return 0, "fail", prompt

    out_path.write_bytes(png_bytes)
    return len(png_bytes), method, prompt


def insert_db(num: int, design_slug: str, jp: str, en: str,
              prompt_text: str, mockup_url: str, db_path: Path) -> int:
    """Insert one rashguard row. Returns rowid (or existing id if dup)."""
    prompt_hash = hashlib.sha256(prompt_text.encode("utf-8")).hexdigest()
    serial = f"MU-RG-{num:02d}-LS-BLK-L"
    name = f"━◯━ MU · Rashguard {num:02d} · {jp} ({en})"
    now = time.strftime("%Y-%m-%dT%H:%M:%S")

    conn = sqlite3.connect(str(db_path))
    try:
        # Idempotency: skip if drop_num already exists for brand=rashguard.
        cur = conn.execute(
            "SELECT id FROM products WHERE brand='rashguard' AND drop_num=?",
            (num,),
        )
        row = cur.fetchone()
        if row:
            return row[0]

        cur = conn.execute(
            """INSERT INTO products
                 (brand, drop_num, name, design_url, mockup_url,
                  price_jpy, inventory, sold, created_at, active,
                  prompt_text, prompt_hash, city_slug, color, size,
                  serial_code)
               VALUES
                 (?, ?, ?, ?, ?, ?, ?, 0, ?, 1, ?, ?, 'teshikaga',
                  'BLK', 'L', ?)""",
            (
                "rashguard", num, name, mockup_url, mockup_url,
                7800, 50, now, prompt_text, prompt_hash, serial,
            ),
        )
        new_id = cur.lastrowid
        conn.commit()
        return new_id
    finally:
        conn.close()


def write_partner_spec(jobs: list[tuple[int, str, str, str, str, str]]) -> None:
    """Write /store/partner_specs/rashguard.json so admin/collabs can list it."""
    spec = {
        "slug": "rashguard",
        "name": "MU Rashguard",
        "ip_owner": "MU (internal brand, 株式会社イネブラ)",
        "design": {
            "monogram": "━◯━",
            "accent": "#e6c449",
        },
        "meta": {
            "display_name": "MU Rashguard",
            "tagline": "Premium BJJ rashguard line — 月相 + 和柄 + grappling",
            "h1": "MU Rashguard",
            "subtitle": "10 designs · dye-sublimation full-coverage · ¥7,800",
            "accent_hex": "#e6c449",
            "ai_prompt": STYLE_PREAMBLE,
            "cadence_hours": 24,
            "lede": (
                "MU 自社 BJJ ライン。 黒 + 金 を基調に、 和柄 (鯉 / 桜 / 武士道 / "
                "達磨 / 青海波 / 麻の葉) と grappling motif (三角 / 金帯) を "
                "10 design で展開。 dye-sublimation 全面プリント、 Beyond "
                "Premium fabric。 ¥7,800、 在庫 50/SKU、 利益 50% 寄付。"
            ),
            "hero_kv": [
                ["10", "designs"],
                ["¥7,800", "price"],
                ["50/SKU", "inventory"],
                ["50%", "donation"],
            ],
        },
        "skus": [
            {
                "num": num,
                "drop_num": num,
                "design_slug": slug,
                "jp_name": jp,
                "en_name": en,
                "palette": palette,
                "price_jpy": 7800,
                "label": f"━◯━ MU · Rashguard {num:02d} · {jp} ({en})",
                "kind": "rashguard_ls",
                "color": "BLK",
                "size": "L",
                "ai_prompt": build_prompt(slug, jp, en, palette, motif),
                "published": True,
            }
            for (num, slug, jp, en, palette, motif) in jobs
        ],
    }
    SPEC_PATH.write_text(
        json.dumps(spec, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )


def main(argv: list[str]) -> int:
    selected: set[int] | None = None
    if len(argv) > 1:
        try:
            selected = {int(a) for a in argv[1:]}
        except ValueError:
            print("usage: gen_rashguard_line.py [num ...]   (numbers 1-10)",
                  file=sys.stderr)
            return 2

    force = bool(os.environ.get("FORCE"))
    api_key = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
    if not api_key:
        print("ERROR: GEMINI_API_KEY not set (checked /Users/yuki/.env).",
              file=sys.stderr)
        return 1

    client = genai.Client(api_key=api_key)
    PROPOSALS_DIR.mkdir(parents=True, exist_ok=True)

    results: list[tuple[int, str, str, int, str, int | None]] = []
    any_fail = False
    for num, design_slug, jp, en, palette, motif in JOBS:
        if selected and num not in selected:
            continue
        print(f"[{num:02d}] {jp} ({en}) — {design_slug}")
        try:
            size, method, prompt = render_one(
                client, num, design_slug, jp, en, palette, motif, force,
            )
        except Exception as exc:  # noqa: BLE001
            print(f"  FAIL: {type(exc).__name__}: {exc}")
            any_fail = True
            results.append((num, design_slug, jp, 0, "fail", None))
            continue

        if size == 0:
            print("  FAIL: no image generated")
            any_fail = True
            results.append((num, design_slug, jp, 0, "fail", None))
            continue

        mockup_url = f"/static/proposals/rashguard-mockup-{num}.png"
        # DB insert — factory only. Store DB sync handled by separate
        # merch-bridge cron / admin POST as needed.
        try:
            pid = insert_db(num, design_slug, jp, en, prompt, mockup_url,
                            FACTORY_DB)
        except Exception as exc:  # noqa: BLE001
            print(f"  DB FAIL: {type(exc).__name__}: {exc}")
            any_fail = True
            results.append((num, design_slug, jp, size, method, None))
            continue

        print(f"  {size:>7} bytes  [{method}]  id={pid}  {mockup_url}")
        results.append((num, design_slug, jp, size, method, pid))

    # Always write the partner spec (10 prompt templates) — it's static.
    write_partner_spec(JOBS)
    print(f"\nwrote {SPEC_PATH}")

    print("\n=== summary ===")
    for num, slug, jp, size, method, pid in results:
        pid_s = f"id={pid}" if pid else "id=-"
        print(f"  {num:02d}  {slug:<22}  {size:>7}  {method:<14}  {pid_s}  {jp}")

    return 1 if any_fail else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
