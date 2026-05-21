#!/usr/bin/env python3
"""
nl_product_creator — natural-language → product pipeline.

Takes a free-text Japanese / English prompt like

    "黒 hoodie に 鯉が登る design / brand=mu_dragon / ¥7800"

extracts structured fields via Claude (with prompt caching) — kind /
brand / color / size / price / design_concept / tags — then reuses
generate.py's Gemini + R2 + Printful + mockup chain to mint a fully
mocked product directly into `products.db` (factory copy).

Supports 5 product kinds: tee / hoodie / mug / tote / sticker. Each kind
maps to its own Printful product+variant IDs so the mockup is correct.

Usage:
    python scripts/nl_product_creator.py "黒 hoodie に koi design"
    python scripts/nl_product_creator.py --dry-run "白 tee asanoha pattern"
    python scripts/nl_product_creator.py --brand mu_dragon "..."

Output (one line of JSON to stdout):
    {"ok": true, "id": 1234, "serial_code": "MU-MU_DRAGON-0001-HOODIE-BLK-L",
     "kind": "hoodie", "brand": "mu_dragon", "mockup_url": "...",
     "admin_edit_url": "...", "duration_s": 120.5}

Failure modes (all graceful):
- Claude API auth fail → regex fallback (kind/color/price extracted, raw
  text as design_concept).
- Gemini API auth fail → exit with `{"ok": false, "stage": "gemini", ...}`.
- Printful mockup fail → product still inserted with mockup_url=null.
- Duplicate prompt_hash → skipped, returns existing product id.

NEVER edits generate.py / product_creator_agent.py (import-only).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sqlite3
import sys
import time
import traceback
from datetime import datetime
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

# Load secrets from ~/.env BEFORE importing generate.py (which reads keys
# at module top-level). Mirrors the same pattern generate.py uses.
def _load_env_file():
    env_path = Path("/Users/yuki/.env")
    if not env_path.exists():
        return
    try:
        for ln in env_path.read_text().splitlines():
            ln = ln.strip()
            if "=" in ln and not ln.startswith("#"):
                k, v = ln.split("=", 1)
                k = k.strip()
                if k in ("GEMINI_API_KEY", "PRINTFUL_API_KEY", "MU_ADMIN_TOKEN",
                         "ANTHROPIC_API_KEY", "CLOUDFLARE_R2_ACCESS_KEY_ID",
                         "CLOUDFLARE_R2_SECRET_ACCESS_KEY", "HELIUS_API_KEY"):
                    os.environ.setdefault(k, v.strip().strip('"').strip("'"))
    except Exception:
        pass

_load_env_file()
os.environ.pop("GOOGLE_API_KEY", None)  # mirror generate.py's guard

DB_PATH = Path(os.environ.get("MU_DB", str(ROOT / "products.db")))
STORE_URL = os.environ.get("MU_STORE_URL", "https://wearmu.com")
ADMIN_TOKEN = os.environ.get("MU_ADMIN_TOKEN", "mu-admin")

# Product kind → Printful (product_id, variant_id_BLK, variant_id_WHT, default_price_jpy, default_inventory).
# Values verified against Printful catalog. variant_ids fall back to BLK
# when a color variant isn't mapped.
KIND_TABLE = {
    # Bella+Canvas 3001 unisex tee (already used by generate.py)
    "tee":     {"pf_product": 71,  "pf_blk": 4017,  "pf_wht": 4011,  "pf_nvy": 4015,  "price": 4900,  "inv": 50, "size": "M"},
    # Gildan 18500 heavy hoodie
    "hoodie":  {"pf_product": 146, "pf_blk": 5530,  "pf_wht": 5532,  "pf_nvy": 5538,  "price": 7800,  "inv": 30, "size": "L"},
    # AAA 11oz ceramic mug (white only — no color variants on the Printful catalog row)
    "mug":     {"pf_product": 19,  "pf_blk": 1320,  "pf_wht": 1320,  "pf_nvy": 1320,  "price": 2900,  "inv": 50, "size": "11oz"},
    # All-over cotton tote
    "tote":    {"pf_product": 84,  "pf_blk": 5257,  "pf_wht": 5258,  "pf_nvy": 5257,  "price": 3500,  "inv": 50, "size": "OS"},
    # Kiss-cut sticker (4×4)
    "sticker": {"pf_product": 358, "pf_blk": 10165, "pf_wht": 10165, "pf_nvy": 10165, "price": 800,   "inv": 100, "size": "4x4"},
}

VALID_COLORS = ("BLK", "WHT", "NVY", "HTR", "RED", "DHR", "BGE")
SIZE_BY_KIND_DEFAULT = {k: v["size"] for k, v in KIND_TABLE.items()}

# Real-brand / trademark guard (warn, don't block). Caller can override
# with --allow-risky.
TRADEMARK_HINTS = (
    "nike", "adidas", "supreme", "louis vuitton", "gucci", "chanel",
    "ferrari", "porsche", "ferrari", "disney", "marvel", "pokemon",
    "ufc", "nba", "fifa", "olympic", "kardashian", "swift",
    # Public figure names that show up in user prompts
)

ANTHROPIC_MODEL = "claude-sonnet-4-6"  # claude-api skill recommendation
ANTHROPIC_FALLBACK = "claude-haiku-4-5"


# ────────────────────────────────────────────────────────────────────────
# Parsing layer
# ────────────────────────────────────────────────────────────────────────

SYSTEM_PROMPT = """\
You are MU's product-spec extractor. The user submits a free-text idea
for a piece of merchandise (Japanese or English, often mixed). Extract a
strict JSON object with these fields:

  product_kind   : one of "tee" / "hoodie" / "mug" / "tote" / "sticker"
                   (default "tee")
  design_concept : English short phrase (4-15 words) describing the
                   GRAPHIC ONLY — what should be printed. No garment,
                   no model, no background scene. Strip brand prefixes.
                   Translate Japanese to English so Gemini can render it.
  brand          : lowercase ascii (a-z, 0-9, _). Default "mu". If the
                   user types "brand=foo", use "foo". If they say
                   "mu_dragon" / "mu × siiieep", use "mu_dragon" /
                   "mu_siiieep".
  color          : one of "BLK" "WHT" "NVY" "HTR" "RED" "DHR" "BGE"
                   (default "BLK")
  size_default   : one of "S" "M" "L" "XL" "OS" "11oz" "4x4"
                   (default "M" for apparel, kind-specific otherwise)
  price_jpy      : integer JPY. If user gives "¥7800" / "7800円" use
                   that. Otherwise null (caller picks a kind default).
  tags           : array of 1-5 lowercase keywords (e.g. ["koi", "dragon"])
  trademark_risk : true if the design_concept or brand contains a known
                   real-world trademark / public figure name (Nike,
                   UFC, Taylor Swift, etc.). false otherwise.

Output MUST be valid JSON, nothing else. No prose, no markdown, no
code-fence."""


def _regex_fallback(text: str) -> dict[str, Any]:
    """Best-effort field extraction when Claude is unavailable."""
    t = text.lower()
    kind = "tee"
    for k in KIND_TABLE:
        if k in t or {"tee": "tシャツ", "hoodie": "パーカー", "mug": "マグ",
                      "tote": "トート", "sticker": "ステッカー"}[k] in t:
            kind = k
            break
    color = "BLK"
    color_map = {
        "BLK": ("black", "黒", "ブラック"),
        "WHT": ("white", "白", "ホワイト"),
        "NVY": ("navy", "紺", "ネイビー"),
        "RED": ("red", "赤", "レッド"),
        "HTR": ("heather", "ヘザー"),
        "BGE": ("beige", "natural", "ナチュラル", "ベージュ"),
        "DHR": ("dark heather", "dhr"),
    }
    for code, words in color_map.items():
        if any(w in t for w in words):
            color = code
            break
    price = None
    m = re.search(r"[¥￥]\s*([0-9,]+)|([0-9,]+)\s*円|price[=:]\s*([0-9,]+)", text)
    if m:
        raw = next(g for g in m.groups() if g)
        try:
            price = int(raw.replace(",", ""))
        except ValueError:
            price = None
    brand = "mu"
    m = re.search(r"brand\s*[=:]\s*([a-z0-9_]+)", t)
    if m:
        brand = m.group(1)
    return {
        "product_kind": kind,
        "design_concept": text.strip()[:240],
        "brand": brand,
        "color": color,
        "size_default": SIZE_BY_KIND_DEFAULT.get(kind, "M"),
        "price_jpy": price,
        "tags": [],
        "trademark_risk": False,
        "_parser": "regex_fallback",
    }


def _claude_parse(text: str) -> dict[str, Any] | None:
    """Ask Claude for structured extraction. Returns None on auth/network failure."""
    if not os.environ.get("ANTHROPIC_API_KEY"):
        return None
    try:
        import anthropic  # type: ignore
    except ImportError:
        return None

    client = anthropic.Anthropic()
    try:
        resp = client.messages.create(
            model=ANTHROPIC_MODEL,
            max_tokens=600,
            system=[
                {
                    "type": "text",
                    "text": SYSTEM_PROMPT,
                    # Prompt caching — system prompt is stable across calls.
                    "cache_control": {"type": "ephemeral"},
                }
            ],
            messages=[{"role": "user", "content": text.strip()[:1800]}],
        )
    except Exception as e:
        msg = str(e)
        # Fallback to haiku on overloaded / 429
        if any(s in msg.lower() for s in ("overloaded", "429", "rate")):
            try:
                resp = client.messages.create(
                    model=ANTHROPIC_FALLBACK,
                    max_tokens=600,
                    system=[
                        {"type": "text", "text": SYSTEM_PROMPT,
                         "cache_control": {"type": "ephemeral"}}
                    ],
                    messages=[{"role": "user", "content": text.strip()[:1800]}],
                )
            except Exception:
                return None
        else:
            return None

    raw = ""
    for block in resp.content:
        if getattr(block, "type", None) == "text":
            raw += block.text
    raw = raw.strip()
    # Strip code-fence if Claude wrapped it
    if raw.startswith("```"):
        raw = re.sub(r"^```[a-z]*\n?", "", raw)
        raw = re.sub(r"\n?```$", "", raw)
    try:
        data = json.loads(raw)
    except json.JSONDecodeError:
        # Find first {...} block
        m = re.search(r"\{[\s\S]*\}", raw)
        if not m:
            return None
        try:
            data = json.loads(m.group(0))
        except json.JSONDecodeError:
            return None
    data["_parser"] = "claude_" + ANTHROPIC_MODEL
    return data


def parse_nl(text: str) -> dict[str, Any]:
    """Parse → structured dict. Tries Claude, falls back to regex."""
    data = _claude_parse(text)
    if data is None:
        data = _regex_fallback(text)

    # Normalize + defaults
    kind = (data.get("product_kind") or "tee").lower().strip()
    if kind not in KIND_TABLE:
        kind = "tee"
    color = (data.get("color") or "BLK").upper().strip()
    if color not in VALID_COLORS:
        color = "BLK"
    brand = (data.get("brand") or "mu").lower().strip()
    brand = re.sub(r"[^a-z0-9_]+", "_", brand).strip("_") or "mu"
    size_default = (data.get("size_default") or SIZE_BY_KIND_DEFAULT[kind]).strip()
    price = data.get("price_jpy")
    if not isinstance(price, int) or price < 100 or price > 500_000:
        price = KIND_TABLE[kind]["price"]
    concept = (data.get("design_concept") or text.strip())[:400]

    return {
        "product_kind": kind,
        "design_concept": concept,
        "brand": brand,
        "color": color,
        "size_default": size_default,
        "price_jpy": int(price),
        "tags": [str(t).lower() for t in (data.get("tags") or [])][:5],
        "trademark_risk": bool(data.get("trademark_risk", False)),
        "raw_text": text,
        "_parser": data.get("_parser", "unknown"),
    }


def trademark_warn(spec: dict[str, Any]) -> list[str]:
    haystack = (spec["design_concept"] + " " + spec["brand"] + " " + spec["raw_text"]).lower()
    warns = [t for t in TRADEMARK_HINTS if t in haystack]
    return warns


# ────────────────────────────────────────────────────────────────────────
# Design generation (reuses generate.py functions)
# ────────────────────────────────────────────────────────────────────────

def build_gemini_prompt(spec: dict[str, Any]) -> str:
    kind = spec["product_kind"]
    color = spec["color"]
    concept = spec["design_concept"]

    bg_rule = (
        "Pure WHITE background." if color == "BLK"
        else "Pure BLACK background — design printed in light ink." if color in ("BLK",)
        else "Pure WHITE background."
    )
    surround = {
        "tee":     "Will be screen-printed on a t-shirt chest.",
        "hoodie":  "Will be screen-printed on a hoodie chest pocket area.",
        "mug":     "Will be wrapped around an 11oz ceramic mug.",
        "tote":    "Will be screen-printed centered on a cotton tote bag.",
        "sticker": "Will be die-cut as a 4×4 inch kiss-cut sticker.",
    }[kind]

    return f"""\
FLAT PRINT ARTWORK. {bg_rule} No clothing. No model. No product photo. Just the graphic — as if it will be screen-printed.

Concept: {concept}

Constraints:
- ONE element. High contrast. Ready to print.
- Square 1:1 composition, design fills 60-80% of canvas, generous margin.
- No text unless concept explicitly demands it. No border. No mockup.
- Surround context: {surround}
- OUTPUT: flat artwork only, 2400×2400px, transparent-edge-friendly.
"""


def generate_and_upload(spec: dict[str, Any]) -> dict[str, Any]:
    """Generate design via Gemini → R2 → Printful file.

    Returns: { design_url, print_url, image_bytes, prompt_hash, gemini_prompt }
    Raises RuntimeError on Gemini auth/quota failure.
    """
    import generate as gen  # lazy — only when actually executing

    prompt = build_gemini_prompt(spec)
    prompt_hash = hashlib.sha256(prompt.encode()).hexdigest()[:16]

    image_bytes = gen.generate_design(prompt)

    # Transparent-bg pass: helps hoodie/tee on non-white shirts.
    # For sticker / tote / mug we keep the original.
    if spec["product_kind"] in ("tee", "hoodie") and spec["color"] != "WHT":
        try:
            image_bytes = gen.make_transparent_bg(image_bytes)
        except Exception as e:
            print(f"  transparent-bg pass failed ({e}); using original", file=sys.stderr)

    ts = datetime.now().strftime("%Y%m%d%H%M%S")
    filename = f"{spec['brand']}_{spec['product_kind']}_{ts}_{prompt_hash[:8]}.png"
    file_url = gen.upload_design_anywhere(image_bytes, filename)

    # Register with Printful v2 files (non-fatal)
    print_url = file_url
    try:
        import requests
        r = requests.post(
            f"{gen.PF_BASE}/v2/files",
            headers=gen.PF_HDR,
            json={"type": "front", "url": file_url},
            timeout=15,
        )
        if r.ok:
            print_url = r.json().get("data", {}).get("url", file_url)
    except Exception:
        pass

    return {
        "design_url": file_url,
        "print_url": print_url,
        "image_bytes": image_bytes,
        "prompt_hash": prompt_hash,
        "gemini_prompt": prompt,
    }


def fetch_mockup(spec: dict[str, Any], file_url: str) -> str | None:
    """Printful mockup for the chosen (product, variant). Best-effort; None on failure."""
    import generate as gen
    kt = KIND_TABLE[spec["product_kind"]]
    variant_key = {"BLK": "pf_blk", "WHT": "pf_wht", "NVY": "pf_nvy"}.get(spec["color"], "pf_blk")
    variant_id = kt.get(variant_key) or kt["pf_blk"]
    try:
        return gen.get_mockup(kt["pf_product"], variant_id, file_url)
    except Exception as e:
        print(f"  mockup fetch failed ({e})", file=sys.stderr)
        return None


# ────────────────────────────────────────────────────────────────────────
# DB insert
# ────────────────────────────────────────────────────────────────────────

def build_serial(spec: dict[str, Any], drop_num: int) -> str:
    brand_u = spec["brand"].upper()
    kind_u = spec["product_kind"].upper()
    color = spec["color"]
    size = spec["size_default"].upper()
    return f"MU-{brand_u}-{drop_num:04d}-{kind_u}-{color}-{size}"


def next_drop_num(con: sqlite3.Connection, brand: str) -> int:
    row = con.execute(
        "SELECT COALESCE(MAX(drop_num), 0) FROM products WHERE brand=?",
        (brand,),
    ).fetchone()
    return int(row[0] or 0) + 1


def existing_by_hash(con: sqlite3.Connection, prompt_hash: str) -> int | None:
    row = con.execute(
        "SELECT id FROM products WHERE prompt_hash=? LIMIT 1",
        (prompt_hash,),
    ).fetchone()
    return int(row[0]) if row else None


def insert_product(
    spec: dict[str, Any],
    design: dict[str, Any],
    mockup_url: str | None,
) -> dict[str, Any]:
    con = sqlite3.connect(DB_PATH)
    try:
        # Duplicate-skip on prompt_hash
        existing = existing_by_hash(con, design["prompt_hash"])
        if existing:
            return {
                "id": existing,
                "drop_num": None,
                "serial_code": None,
                "duplicate": True,
            }
        drop_num = next_drop_num(con, spec["brand"])
        serial_code = build_serial(spec, drop_num)
        inventory = KIND_TABLE[spec["product_kind"]]["inv"]
        now_iso = datetime.now().isoformat()

        name = spec["design_concept"][:80] or f"{spec['brand']} {spec['product_kind']} #{drop_num}"

        full_prompt = json.dumps({
            "raw_input": spec["raw_text"],
            "extracted": {
                "kind": spec["product_kind"],
                "brand": spec["brand"],
                "color": spec["color"],
                "size": spec["size_default"],
                "price_jpy": spec["price_jpy"],
                "design_concept": spec["design_concept"],
                "tags": spec["tags"],
                "parser": spec["_parser"],
            },
            "gemini_prompt": design["gemini_prompt"],
        }, ensure_ascii=False)

        seed_data = json.dumps({
            "source": "nl_product_creator",
            "kind": spec["product_kind"],
            "tags": spec["tags"],
            "created_at": now_iso,
        }, ensure_ascii=False)

        con.execute(
            """
            INSERT INTO products
              (brand, drop_num, name, design_url, mockup_url, print_url,
               price_jpy, inventory, sold, created_at, active,
               prompt_text, prompt_hash, seed_data, color, size, serial_code)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?, 1, ?, ?, ?, ?, ?, ?)
            """,
            (
                spec["brand"], drop_num, name,
                design["design_url"], mockup_url, design["print_url"],
                spec["price_jpy"], inventory, now_iso,
                full_prompt, design["prompt_hash"], seed_data,
                spec["color"], spec["size_default"], serial_code,
            ),
        )
        con.commit()
        new_id = con.execute("SELECT last_insert_rowid()").fetchone()[0]
        return {
            "id": int(new_id),
            "drop_num": drop_num,
            "serial_code": serial_code,
            "duplicate": False,
        }
    finally:
        con.close()


# ────────────────────────────────────────────────────────────────────────
# CLI
# ────────────────────────────────────────────────────────────────────────

def run_once(text: str, *, dry_run: bool, brand_override: str | None) -> dict[str, Any]:
    started = time.time()
    out: dict[str, Any] = {"ok": False, "input": text}

    try:
        spec = parse_nl(text)
    except Exception as e:
        out["stage"] = "parse"
        out["error"] = f"{type(e).__name__}: {e}"
        return out

    if brand_override:
        spec["brand"] = re.sub(r"[^a-z0-9_]+", "_",
                               brand_override.lower().strip()).strip("_") or "mu"

    warns = trademark_warn(spec)
    if warns:
        out["trademark_warn"] = warns

    out["spec"] = {k: v for k, v in spec.items() if k != "raw_text"}

    if dry_run:
        out["ok"] = True
        out["dry_run"] = True
        out["duration_s"] = round(time.time() - started, 1)
        return out

    # Generation
    try:
        design = generate_and_upload(spec)
    except KeyError as e:
        out["stage"] = "gemini"
        out["error"] = f"missing env: {e}"
        return out
    except Exception as e:
        out["stage"] = "gemini"
        out["error"] = f"{type(e).__name__}: {e}"
        out["trace"] = traceback.format_exc(limit=3)
        return out

    out["design_url"] = design["design_url"]
    out["print_url"] = design["print_url"]
    out["prompt_hash"] = design["prompt_hash"]

    mockup_url = fetch_mockup(spec, design["design_url"])
    out["mockup_url"] = mockup_url

    try:
        ins = insert_product(spec, design, mockup_url)
    except Exception as e:
        out["stage"] = "db"
        out["error"] = f"{type(e).__name__}: {e}"
        return out

    out["id"] = ins["id"]
    out["drop_num"] = ins["drop_num"]
    out["serial_code"] = ins["serial_code"]
    out["duplicate"] = ins["duplicate"]
    out["admin_edit_url"] = f"{STORE_URL}/admin/db?id={ins['id']}&token={ADMIN_TOKEN}"
    out["public_url"] = f"{STORE_URL}/products/{spec['brand']}/{ins['id']}"
    out["ok"] = True
    out["duration_s"] = round(time.time() - started, 1)
    return out


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Natural-language → MU product (Phase 1).")
    parser.add_argument("text", nargs="*",
                        help="Free-text prompt. If omitted, reads stdin.")
    parser.add_argument("--dry-run", action="store_true",
                        help="Parse only, don't call Gemini / write DB.")
    parser.add_argument("--brand", default=None,
                        help="Override the extracted brand (e.g. mu_dragon).")
    parser.add_argument("--json-input", action="store_true",
                        help="Treat stdin as a single JSON object: {\"text\": ...}.")
    args = parser.parse_args(argv)

    if args.json_input:
        try:
            payload = json.loads(sys.stdin.read())
            text = payload.get("text", "")
        except Exception as e:
            print(json.dumps({"ok": False, "stage": "input",
                              "error": f"bad json: {e}"}, ensure_ascii=False))
            return 1
    elif args.text:
        text = " ".join(args.text)
    else:
        text = sys.stdin.read().strip()

    if not text or len(text) < 3:
        print(json.dumps({"ok": False, "stage": "input",
                          "error": "empty text"}, ensure_ascii=False))
        return 1

    result = run_once(text, dry_run=args.dry_run, brand_override=args.brand)
    print(json.dumps(result, ensure_ascii=False))
    return 0 if result.get("ok") else 1


if __name__ == "__main__":
    sys.exit(main())
