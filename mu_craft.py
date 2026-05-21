#!/usr/bin/env python3
"""mu_craft.py — MU CRAFT one-click SKU creator.

The "Tシャツにして" primitive made into a single web app.

Pipeline per craft request:
  1. Identify user (anon cookie or registered email)
  2. Check MP balance (anon 5 free, +5 after signup, then 1 MP/craft)
  3. Gemini text → JSON brief (catchphrase, kanji, accent, subtitle)
  4. Render MU brutalist SVG → PNG (2940x2940 RGBA)
  5. Upload design PNG to R2
  6. Printful mockup generator (white + black tee) → 2 mockup JPGs
  7. Save SKU row + permanent /c/<slug> URL
  8. Return mockup URLs to UI

Economy:
  Earn: anon visit +5 MP, signup +5 MP, Tee purchase: buyer +30 MP / creator +5 MP, referral +5 MP, cash ¥30=1 MP
  Spend: craft 1 MP, publish (real SUZURI+Printful) 3 MP

Run:
  source /Users/yuki/.env && python3 mu_craft.py
  open http://localhost:8788
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import hmac
import json
import os
import re
import secrets
import sqlite3
import subprocess
import sys
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Optional
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from fastapi import FastAPI, Form, HTTPException, Request as FastRequest, Response
from fastapi.responses import HTMLResponse, JSONResponse, RedirectResponse
from fastapi.staticfiles import StaticFiles
import uvicorn


# ───────────────────────────────────────────────────────────── env / paths
def _autoload_dotenv():
    """Load /Users/yuki/.env. Critically: FORCE-overrides API keys because
    ~/.zshrc has a revoked GEMINI_API_KEY that would otherwise win
    (per feedback_gemini_key_env.md)."""
    p = Path(os.path.expanduser("~/.env"))
    if not p.exists():
        return
    FORCE_OVERRIDE = {"GEMINI_API_KEY", "GOOGLE_API_KEY", "PRINTFUL_API_KEY", "SUZURI_ACCESS_TOKEN"}
    for line in p.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        k, v = line.split("=", 1)
        k = k.strip()
        v = v.strip().strip('"').strip("'")
        if not k:
            continue
        if k in FORCE_OVERRIDE or k not in os.environ:
            os.environ[k] = v


_autoload_dotenv()

ROOT = Path(__file__).resolve().parent
# In prod (Fly), DATA_DIR points at the mounted volume.
DATA_DIR = Path(os.environ.get("MU_CRAFT_DATA_DIR", str(ROOT / "data")))
STATIC_DIR = Path(os.environ.get("MU_CRAFT_STATIC_DIR", str(ROOT / "static_craft")))
DB_PATH = DATA_DIR / "mu_craft.db"
DESIGNS_DIR = STATIC_DIR / "designs"
MOCKUPS_DIR = STATIC_DIR / "mockups"
for d in (DATA_DIR, STATIC_DIR, DESIGNS_DIR, MOCKUPS_DIR):
    d.mkdir(parents=True, exist_ok=True)

SECRET_KEY = os.environ.get("MU_CRAFT_SECRET", "dev-secret-change-in-prod-please")
ADMIN_TOKEN = os.environ.get("MU_ADMIN_TOKEN", "mu-admin-2026")
GEMINI_API_KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY") or ""
PRINTFUL_API_KEY = os.environ.get("PRINTFUL_API_KEY") or ""
SUZURI_ACCESS_TOKEN = os.environ.get("SUZURI_ACCESS_TOKEN") or ""
WRANGLER_BIN = os.environ.get("WRANGLER_BIN", "/opt/homebrew/bin/wrangler")
PORT = int(os.environ.get("PORT", "8788"))

# SUZURI: item 148 = ヘビーウェイトTシャツ, margin ¥1,400 → retail ¥4,900 JP
SUZURI_API = "https://suzuri.jp/api/v1/materials"
SUZURI_ITEM_ID_TEE = 148
SUZURI_MARGIN_YEN = 1400

# Storage backend: "local" serves files from STATIC_DIR via FastAPI; "r2" uses wrangler.
# Local mode requires MU_CRAFT_PUBLIC_BASE so Printful can fetch design via public URL.
STORAGE = os.environ.get("MU_CRAFT_STORAGE", "local")  # local | r2
PUBLIC_BASE = os.environ.get("MU_CRAFT_PUBLIC_BASE", f"http://localhost:{PORT}").rstrip("/")

# Printful Bella+Canvas 3001 product/variant IDs (from prior BJJ mockup work)
PRINTFUL_PRODUCT_ID = 71
PRINTFUL_VARIANT_WHITE = 4012   # White / M
PRINTFUL_VARIANT_BLACK = 4017   # Black / M
PRINTFUL_PLACEMENT = "front"
PRINTFUL_BASE = "https://api.printful.com"

R2_BUCKET_MOCKUPS = "wearmu-mockups"
R2_PUBLIC_MOCKUPS = "https://mockups.wearmu.com"
R2_PREFIX_CRAFT = "craft"

DEFAULT_POSITION = {
    "area_width": 1800, "area_height": 2400,
    "width": 1700, "height": 1700,
    "top": 350, "left": 50,
}

# ───────────────────────────────────────────────────────────── economy
ANON_FREE_MP = 5
SIGNUP_BONUS_MP = 5
TEE_BUYER_MP = 30
TEE_CREATOR_MP = 5
REFERRAL_MP = 5
CASH_RATE_YEN_PER_MP = 30
CRAFT_COST_MP = 1
PUBLISH_COST_MP = 3


# ───────────────────────────────────────────────────────────── db
SCHEMA = """
CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  email TEXT UNIQUE,
  anon_id TEXT UNIQUE,
  display_name TEXT,
  mp_balance INTEGER NOT NULL DEFAULT 0,
  referrer_id INTEGER,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (referrer_id) REFERENCES users(id)
);

CREATE TABLE IF NOT EXISTS skus (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  slug TEXT UNIQUE NOT NULL,
  creator_user_id INTEGER NOT NULL,
  topic TEXT NOT NULL,
  catchphrase TEXT,
  kanji TEXT,
  accent_color TEXT,
  subtitle TEXT,
  design_png_path TEXT,
  design_png_url TEXT,
  mockup_white_url TEXT,
  mockup_black_url TEXT,
  status TEXT NOT NULL DEFAULT 'draft',
  suzuri_url TEXT,
  printful_id TEXT,
  view_count INTEGER NOT NULL DEFAULT 0,
  sale_count INTEGER NOT NULL DEFAULT 0,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  published_at TIMESTAMP,
  FOREIGN KEY (creator_user_id) REFERENCES users(id)
);

CREATE TABLE IF NOT EXISTS mp_ledger (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL,
  delta INTEGER NOT NULL,
  balance_after INTEGER NOT NULL,
  reason TEXT NOT NULL,
  ref_sku_id INTEGER,
  ref_purchase_id INTEGER,
  note TEXT,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (user_id) REFERENCES users(id)
);

CREATE TABLE IF NOT EXISTS purchases (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  sku_id INTEGER NOT NULL,
  buyer_user_id INTEGER,
  amount_yen INTEGER NOT NULL,
  source TEXT NOT NULL,
  external_order_id TEXT,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (sku_id) REFERENCES skus(id),
  FOREIGN KEY (buyer_user_id) REFERENCES users(id)
);

CREATE TABLE IF NOT EXISTS magic_links (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  email TEXT NOT NULL,
  code TEXT NOT NULL,
  expires_at TIMESTAMP NOT NULL,
  used INTEGER NOT NULL DEFAULT 0,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_skus_slug ON skus(slug);
CREATE INDEX IF NOT EXISTS idx_skus_creator ON skus(creator_user_id);
CREATE INDEX IF NOT EXISTS idx_ledger_user ON mp_ledger(user_id);
CREATE INDEX IF NOT EXISTS idx_magic_email ON magic_links(email);
"""


def db():
    conn = sqlite3.connect(DB_PATH, isolation_level=None)  # autocommit
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA foreign_keys=ON")
    return conn


def init_db():
    with db() as conn:
        conn.executescript(SCHEMA)
        # seed Yuki
        c = conn.execute("SELECT id FROM users WHERE email=?", ("mail@yukihamada.jp",))
        if not c.fetchone():
            conn.execute(
                "INSERT INTO users (email, display_name, mp_balance) VALUES (?,?,?)",
                ("mail@yukihamada.jp", "Yuki Hamada (Founding Author)", 1000000),
            )
        # seed BJJ Founding Day Drop if not present
        yuki = conn.execute("SELECT id FROM users WHERE email=?", ("mail@yukihamada.jp",)).fetchone()
        yuki_id = yuki["id"]
        bjj_seeds = [
            ("angle-gt-length-white", "三角絞めの理論 (ANGLE > LENGTH 黒インク, 白T)",
             "ANGLE > LENGTH", "角度", "#0a0a0a", "TRIANGLE CHOKE / BJJ THEORY",
             "https://mockups.wearmu.com/bjj-triangle/angle-gt-length-white-tee.jpg",
             "https://mockups.wearmu.com/bjj-triangle/angle-gt-length-white-tee.jpg",
             "https://mockups.wearmu.com/bjj-triangle/angle-gt-length-black-tee.jpg"),
            ("triangle-diagram-white", "三角絞めの理論 (Triangle Diagram 黒, 白T)",
             "TRIANGLE", "三角", "#0a0a0a", "TRIANGLE CHOKE / BJJ",
             "https://mockups.wearmu.com/bjj-triangle/_designs/triangle-diagram_black.png",
             "https://mockups.wearmu.com/bjj-triangle/triangle-diagram-white-tee.jpg",
             "https://mockups.wearmu.com/bjj-triangle/triangle-diagram-black-tee.jpg"),
            ("physics-formula-white", "三角絞めの理論 (POSITION > POWER, 白T)",
             "POSITION > POWER", "θ", "#0a0a0a", "F = sin(θ)·μ",
             "https://mockups.wearmu.com/bjj-triangle/_designs/physics-formula_black.png",
             "https://mockups.wearmu.com/bjj-triangle/physics-formula-white-tee.jpg",
             "https://mockups.wearmu.com/bjj-triangle/physics-formula-black-tee.jpg"),
            ("kakudo-kanji-white", "三角絞めの理論 (角度 漢字ミニマル, 白T)",
             "ANGLE OVER SIZE", "角度", "#0a0a0a", "TRIANGLE CHOKE / BJJ",
             "https://mockups.wearmu.com/bjj-triangle/_designs/kakudo-kanji_black.png",
             "https://mockups.wearmu.com/bjj-triangle/kakudo-kanji-white-tee.jpg",
             "https://mockups.wearmu.com/bjj-triangle/kakudo-kanji-black-tee.jpg"),
        ]
        for slug, topic, catch, kanji, accent, sub, design_url, mw, mb in bjj_seeds:
            existing = conn.execute("SELECT id FROM skus WHERE slug=?", (slug,)).fetchone()
            if existing:
                continue
            conn.execute(
                """INSERT INTO skus
                   (slug, creator_user_id, topic, catchphrase, kanji, accent_color, subtitle,
                    design_png_url, mockup_white_url, mockup_black_url, status)
                   VALUES (?,?,?,?,?,?,?,?,?,?,?)""",
                (slug, yuki_id, topic, catch, kanji, accent, sub,
                 design_url, mw, mb, "draft")
            )


# ───────────────────────────────────────────────────────────── auth / cookies
def _sign(value: str) -> str:
    sig = hmac.new(SECRET_KEY.encode(), value.encode(), hashlib.sha256).hexdigest()[:24]
    return f"{value}.{sig}"


def _verify_signed(signed: str) -> Optional[str]:
    if not signed or "." not in signed:
        return None
    value, sig = signed.rsplit(".", 1)
    expected = hmac.new(SECRET_KEY.encode(), value.encode(), hashlib.sha256).hexdigest()[:24]
    if not hmac.compare_digest(sig, expected):
        return None
    return value


def get_or_create_user(request: FastRequest, response: Response) -> dict:
    """Return current user dict. Creates anon user if no cookie."""
    # Try registered session
    sess = request.cookies.get("mu_session")
    if sess:
        user_id = _verify_signed(sess)
        if user_id and user_id.isdigit():
            row = db().execute("SELECT * FROM users WHERE id=?", (int(user_id),)).fetchone()
            if row:
                return dict(row)

    # Try anon cookie
    anon = request.cookies.get("mu_anon")
    if anon:
        anon_id = _verify_signed(anon)
        if anon_id:
            row = db().execute("SELECT * FROM users WHERE anon_id=?", (anon_id,)).fetchone()
            if row:
                return dict(row)

    # Create new anon user
    anon_id = secrets.token_urlsafe(16)
    with db() as conn:
        cur = conn.execute(
            "INSERT INTO users (anon_id, mp_balance) VALUES (?, ?)",
            (anon_id, ANON_FREE_MP)
        )
        new_user_id = cur.lastrowid
        conn.execute(
            "INSERT INTO mp_ledger (user_id, delta, balance_after, reason, note) VALUES (?,?,?,?,?)",
            (new_user_id, ANON_FREE_MP, ANON_FREE_MP, "free_anon", "anonymous first visit")
        )
    response.set_cookie("mu_anon", _sign(anon_id), max_age=60*60*24*365*5, samesite="lax")
    row = db().execute("SELECT * FROM users WHERE id=?", (new_user_id,)).fetchone()
    return dict(row)


# ───────────────────────────────────────────────────────────── MP ledger
def mp_change(user_id: int, delta: int, reason: str, *,
              ref_sku_id=None, ref_purchase_id=None, note=None,
              allow_negative: bool = False) -> tuple[bool, int]:
    """Atomically change MP. Returns (success, new_balance).
    If delta < 0 and balance would go negative, returns (False, current_balance)
    unless allow_negative=True."""
    with db() as conn:
        conn.execute("BEGIN IMMEDIATE")
        try:
            row = conn.execute("SELECT mp_balance FROM users WHERE id=?", (user_id,)).fetchone()
            if not row:
                conn.execute("ROLLBACK")
                return False, 0
            current = row["mp_balance"]
            new_balance = current + delta
            if new_balance < 0 and not allow_negative:
                conn.execute("ROLLBACK")
                return False, current
            conn.execute("UPDATE users SET mp_balance=? WHERE id=?", (new_balance, user_id))
            conn.execute(
                "INSERT INTO mp_ledger (user_id, delta, balance_after, reason, ref_sku_id, ref_purchase_id, note) VALUES (?,?,?,?,?,?,?)",
                (user_id, delta, new_balance, reason, ref_sku_id, ref_purchase_id, note)
            )
            conn.execute("COMMIT")
            return True, new_balance
        except Exception as e:
            conn.execute("ROLLBACK")
            raise


# ───────────────────────────────────────────────────────────── Gemini brief
GEMINI_MODEL = "gemini-2.5-flash"  # Flash + thinkingBudget=0 → ~1s vs 30s for Pro.
GEMINI_URL = f"https://generativelanguage.googleapis.com/v1beta/models/{GEMINI_MODEL}:generateContent"

BRIEF_PROMPT = """You are designing a MU-brand T-shirt for the given topic.

MU's aesthetic: brutalist sans-serif typography, monochrome (mostly black ink on white tee, or vice versa), confident, quotable, slightly intellectual.

Topic: {topic}

Output a strict JSON object with these fields ONLY (no markdown, no prose, no surrounding text):

{{
  "catchphrase": "3-12 character bold English phrase, ALL CAPS, punchy. Examples: ANGLE > LENGTH, FAST AND CLEAN, NO LIMITS, BUILD < SHIP, MUDA.",
  "kanji": "1-3 Japanese kanji (NOT hiragana/katakana) that captures the essence of the topic. Common readable kanji preferred. Examples: 角度, 無, 速, 静, 闘.",
  "accent_color": "hex color like #e6c449 that matches the topic mood",
  "subtitle": "very short tagline 5-30 chars, in English or Japanese. Examples: TRIANGLE CHOKE / BJJ THEORY, COFFEE / FIRST PRINCIPLES, 道 / THE WAY"
}}

Output ONLY the JSON object."""


def gemini_brief(topic: str) -> dict:
    if not GEMINI_API_KEY:
        return _fallback_brief(topic)
    body = {
        "contents": [{"parts": [{"text": BRIEF_PROMPT.format(topic=topic[:300])}]}],
        "generationConfig": {
            "temperature": 0.7,
            "maxOutputTokens": 500,
            # thinkingBudget=0 disables internal CoT → 1s instead of 30s. Quality
            # for this 4-field JSON task is unaffected.
            "thinkingConfig": {"thinkingBudget": 0},
        },
    }
    try:
        req = Request(
            f"{GEMINI_URL}?key={GEMINI_API_KEY}",
            data=json.dumps(body).encode("utf-8"),
            headers={"Content-Type": "application/json"},
        )
        with urlopen(req, timeout=30) as r:
            resp = json.load(r)
        text = resp["candidates"][0]["content"]["parts"][0]["text"]
        m = re.search(r"\{[^{}]*\"catchphrase\"[^{}]*\}", text, re.DOTALL)
        if not m:
            m = re.search(r"\{.*\}", text, re.DOTALL)
        if not m:
            print(f"  gemini_brief: no JSON in response text={text!r}", file=sys.stderr)
            return _fallback_brief(topic)
        brief = json.loads(m.group())
    except HTTPError as e:
        body_text = ""
        try:
            body_text = e.read().decode("utf-8", "replace")[:500]
        except Exception:
            pass
        print(f"  gemini_brief HTTP {e.code}: {body_text}", file=sys.stderr)
        return _fallback_brief(topic)
    except Exception as e:
        print(f"  gemini_brief failed: {type(e).__name__}: {e}", file=sys.stderr)
        return _fallback_brief(topic)

    brief.setdefault("catchphrase", "MU")
    brief.setdefault("kanji", "無")
    brief.setdefault("accent_color", "#e6c449")
    brief.setdefault("subtitle", topic[:30].upper())
    # sanitize
    brief["catchphrase"] = brief["catchphrase"][:24]
    brief["kanji"] = brief["kanji"][:4]
    if not re.match(r"^#[0-9a-fA-F]{6}$", brief["accent_color"]):
        brief["accent_color"] = "#e6c449"
    brief["subtitle"] = brief["subtitle"][:40]
    return brief


def _fallback_brief(topic: str) -> dict:
    safe = re.sub(r"[^\w\s\-]", "", topic).upper()[:24] or "MU"
    return {
        "catchphrase": safe,
        "kanji": "無",
        "accent_color": "#e6c449",
        "subtitle": topic[:30].upper(),
    }


# ───────────────────────────────────────────────────────────── SVG render
FAMILY_SANS = "Helvetica Neue, Arial Black, Helvetica, sans-serif"
FAMILY_JP = "Hiragino Mincho ProN, Yu Mincho, serif"
SIZE = 2940


def render_svg(brief: dict, ink: str = "#0a0a0a") -> str:
    catch = _xml_escape(brief["catchphrase"])
    kanji = _xml_escape(brief["kanji"])
    subtitle = _xml_escape(brief["subtitle"])
    accent = brief["accent_color"]
    # auto-fit catchphrase
    catch_size = min(540, int(2400 / max(1, len(catch)) * 1.0))
    catch_size = max(140, catch_size)
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {SIZE} {SIZE}">
  <text x="1470" y="1000" text-anchor="middle" font-family="{FAMILY_SANS}" font-weight="900" font-size="{catch_size}" letter-spacing="20" fill="{ink}">{catch}</text>
  <text x="1470" y="2050" text-anchor="middle" font-family="{FAMILY_JP}" font-size="1000" font-weight="900" fill="{accent}">{kanji}</text>
  <rect x="320" y="2380" width="2300" height="6" fill="{ink}"/>
  <text x="320" y="2480" font-family="{FAMILY_SANS}" font-size="76" font-weight="700" letter-spacing="18" fill="{ink}">{subtitle}</text>
  <text x="2620" y="2480" text-anchor="end" font-family="{FAMILY_SANS}" font-size="76" font-weight="700" letter-spacing="14" fill="{ink}">— MU —</text>
</svg>"""


def _xml_escape(s: str) -> str:
    return (s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;"))


def rasterize_svg(svg_text: str, out_png: Path):
    with tempfile.NamedTemporaryFile("w", suffix=".svg", delete=False) as f:
        f.write(svg_text)
        svg_path = f.name
    try:
        subprocess.run(
            ["rsvg-convert", "-w", str(SIZE), "-h", str(SIZE), "-o", str(out_png), svg_path],
            check=True, capture_output=True,
        )
    finally:
        os.unlink(svg_path)


# ───────────────────────────────────────────────────────────── Printful + R2
def store_file(local_src: Path, key: str, content_type: str) -> str:
    """Store a generated asset and return its publicly fetchable URL.

    STORAGE=local  → copy into STATIC_DIR, serve via /_static/<key>; URL =
                     PUBLIC_BASE/_static/<key>. Suitable for Fly with public
                     domain. Requires MU_CRAFT_PUBLIC_BASE in prod so Printful
                     can fetch the design.
    STORAGE=r2     → wrangler r2 object put to wearmu-mockups bucket; URL =
                     https://mockups.wearmu.com/<key>. Suitable for local dev
                     when public localhost isn't reachable from Printful.
    """
    if STORAGE == "r2":
        result = subprocess.run(
            [WRANGLER_BIN, "r2", "object", "put",
             f"{R2_BUCKET_MOCKUPS}/{key}", f"--file={local_src}",
             "--remote", f"--content-type={content_type}"],
            capture_output=True, text=True, timeout=120,
        )
        if result.returncode != 0:
            raise RuntimeError(f"wrangler upload failed: {result.stderr[-300:]}")
        return f"{R2_PUBLIC_MOCKUPS}/{key}"

    # local mode
    dst = STATIC_DIR / key
    dst.parent.mkdir(parents=True, exist_ok=True)
    if dst.resolve() != local_src.resolve():
        dst.write_bytes(local_src.read_bytes())
    return f"{PUBLIC_BASE}/_static/{key}"


def printful_post(path: str, body: dict) -> Optional[dict]:
    req = Request(f"{PRINTFUL_BASE}{path}",
                  data=json.dumps(body).encode(),
                  headers={"Content-Type": "application/json",
                           "Authorization": f"Bearer {PRINTFUL_API_KEY}"},
                  method="POST")
    try:
        with urlopen(req, timeout=60) as r:
            return json.load(r)
    except HTTPError as e:
        print(f"  printful POST {path} {e.code}: {e.read().decode()[:300]}", file=sys.stderr)
        return None


def printful_get(path: str) -> Optional[dict]:
    req = Request(f"{PRINTFUL_BASE}{path}",
                  headers={"Authorization": f"Bearer {PRINTFUL_API_KEY}"})
    try:
        with urlopen(req, timeout=60) as r:
            return json.load(r)
    except HTTPError as e:
        print(f"  printful GET {path} {e.code}: {e.read().decode()[:300]}", file=sys.stderr)
        return None


def suzuri_publish(png_path: Path, title: str) -> Optional[dict]:
    """Upload a design PNG to SUZURI as a material, create a product on item 148
    (ヘビーウェイトTシャツ) with the configured margin. Returns the public product
    URL + material_id on success, None on failure."""
    if not SUZURI_ACCESS_TOKEN:
        print("  suzuri_publish: SUZURI_ACCESS_TOKEN missing — skipping live publish",
              file=sys.stderr)
        return None
    if not png_path.exists():
        print(f"  suzuri_publish: png not found {png_path}", file=sys.stderr)
        return None
    raw = png_path.read_bytes()
    b64 = base64.b64encode(raw).decode()
    body = {
        "texture": f"data:image/png;base64,{b64}",
        "title": title[:120],
        "price": SUZURI_MARGIN_YEN,
        "products": [
            {"itemId": SUZURI_ITEM_ID_TEE, "published": True, "resaleEnabled": False}
        ],
    }
    req = Request(
        SUZURI_API,
        data=json.dumps(body).encode(),
        headers={
            "Authorization": f"Bearer {SUZURI_ACCESS_TOKEN}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    try:
        with urlopen(req, timeout=60) as r:
            j = json.load(r)
    except HTTPError as e:
        body_text = ""
        try:
            body_text = e.read().decode("utf-8", "replace")[:300]
        except Exception:
            pass
        print(f"  suzuri_publish HTTP {e.code}: {body_text}", file=sys.stderr)
        return None
    except Exception as e:
        print(f"  suzuri_publish failed: {type(e).__name__}: {e}", file=sys.stderr)
        return None

    material_id = j.get("material", {}).get("id")
    products = j.get("products") or []
    pretty_url = ""
    if products:
        url_template = products[0].get("url", "")
        pretty_url = url_template.replace("{size}", "m").replace("{color}", "black")
    return {"material_id": material_id, "url": pretty_url}


def printful_mockup(design_url: str, variant_id: int) -> Optional[str]:
    body = {
        "variant_ids": [variant_id],
        "format": "jpg",
        "files": [{"placement": PRINTFUL_PLACEMENT, "image_url": design_url, "position": DEFAULT_POSITION}],
    }
    res = printful_post(f"/mockup-generator/create-task/{PRINTFUL_PRODUCT_ID}", body)
    if not res:
        return None
    task_key = res.get("result", {}).get("task_key")
    if not task_key:
        return None
    for attempt in range(30):
        time.sleep(4 if attempt > 0 else 2)
        poll = printful_get(f"/mockup-generator/task?task_key={task_key}")
        if not poll:
            continue
        status = poll.get("result", {}).get("status")
        if status == "completed":
            mockups = poll["result"].get("mockups", [])
            if mockups:
                return mockups[0].get("mockup_url")
            return None
        if status == "failed":
            return None
    return None


def download(url: str, out_path: Path) -> int:
    with urlopen(Request(url), timeout=60) as r:
        data = r.read()
    out_path.write_bytes(data)
    return len(data)


# ───────────────────────────────────────────────────────────── slug
def make_slug(topic: str, brief: dict) -> str:
    # ASCII-only slug for URL + filename safety (Japanese in path breaks some
    # static-file servers).
    base = re.sub(r"[^A-Za-z0-9\s-]", "", brief.get("catchphrase", ""))
    base = re.sub(r"\s+", "-", base).lower().strip("-")
    if not base:
        base = "sku"
    base = base[:40]
    suffix = secrets.token_urlsafe(4).replace("-", "").replace("_", "").lower()[:5]
    return f"{base}-{suffix}"


# ───────────────────────────────────────────────────────────── core craft pipeline
def run_craft_pipeline(user_id: int, topic: str) -> dict:
    """Generate one SKU: gemini brief → svg → png → r2 → printful mockups → save.

    Two design variants are rendered: black-ink for white tee, white-ink for
    black tee. Accent (brand color) stays the same."""
    t0 = time.time()
    brief = gemini_brief(topic)
    slug = make_slug(topic, brief)

    # render two PNGs: black-ink (for white tee) and white-ink (for black tee)
    png_black = DESIGNS_DIR / f"{slug}_black.png"
    png_white = DESIGNS_DIR / f"{slug}_white.png"
    rasterize_svg(render_svg(brief, ink="#0a0a0a"), png_black)
    rasterize_svg(render_svg(brief, ink="#ffffff"), png_white)

    # store designs (local volume or R2)
    design_black_url = store_file(png_black, f"designs/{slug}_black.png", "image/png")
    design_white_url = store_file(png_white, f"designs/{slug}_white.png", "image/png")
    design_url = design_black_url  # primary canonical = black ink

    # Printful mockups (white tee uses black-ink design, black tee uses white-ink)
    with ThreadPoolExecutor(max_workers=2) as ex:
        fut_w = ex.submit(printful_mockup, design_black_url, PRINTFUL_VARIANT_WHITE)
        fut_b = ex.submit(printful_mockup, design_white_url, PRINTFUL_VARIANT_BLACK)
        white_url = fut_w.result()
        black_url = fut_b.result()

    # mirror Printful presigned URLs (24h TTL) to permanent storage
    mockup_white_r2 = mockup_black_r2 = None
    with tempfile.TemporaryDirectory() as td:
        td = Path(td)
        if white_url:
            tmp = td / f"{slug}-white.jpg"
            download(white_url, tmp)
            mockup_white_r2 = store_file(tmp, f"mockups/{slug}-white-tee.jpg", "image/jpeg")
        if black_url:
            tmp = td / f"{slug}-black.jpg"
            download(black_url, tmp)
            mockup_black_r2 = store_file(tmp, f"mockups/{slug}-black-tee.jpg", "image/jpeg")

    # save SKU
    with db() as conn:
        conn.execute(
            """INSERT INTO skus
               (slug, creator_user_id, topic, catchphrase, kanji, accent_color, subtitle,
                design_png_path, design_png_url, mockup_white_url, mockup_black_url, status)
               VALUES (?,?,?,?,?,?,?,?,?,?,?,?)""",
            (slug, user_id, topic, brief["catchphrase"], brief["kanji"],
             brief["accent_color"], brief["subtitle"],
             str(png_black), design_url, mockup_white_r2, mockup_black_r2, "draft")
        )
        sku_id = conn.execute("SELECT id FROM skus WHERE slug=?", (slug,)).fetchone()["id"]

    return {
        "sku_id": sku_id,
        "slug": slug,
        "topic": topic,
        "brief": brief,
        "design_url": design_url,
        "mockup_white": mockup_white_r2,
        "mockup_black": mockup_black_r2,
        "elapsed_sec": round(time.time() - t0, 1),
    }


# ───────────────────────────────────────────────────────────── FastAPI app
from contextlib import asynccontextmanager


@asynccontextmanager
async def lifespan(app: FastAPI):
    init_db()
    print(f"== MU CRAFT booted ==")
    print(f"   DB:        {DB_PATH}")
    print(f"   STATIC:    {STATIC_DIR}")
    print(f"   STORAGE:   {STORAGE}")
    print(f"   PUBLIC:    {PUBLIC_BASE}")
    print(f"   Gemini:    {'YES' if GEMINI_API_KEY else 'NO (fallback)'}")
    print(f"   Printful:  {'YES' if PRINTFUL_API_KEY else 'NO (mockups will fail)'}")
    print(f"   open:      {PUBLIC_BASE}")
    yield


app = FastAPI(title="MU CRAFT", version="0.1.0", lifespan=lifespan)
app.mount("/_static", StaticFiles(directory=str(STATIC_DIR)), name="static")


@app.get("/healthz")
def healthz():
    return {"ok": True, "storage": STORAGE, "public_base": PUBLIC_BASE}


@app.get("/robots.txt", response_class=HTMLResponse)
def robots_txt():
    return HTMLResponse(content=(
        f"User-agent: *\n"
        f"Allow: /\n"
        f"Disallow: /api/\n"
        f"Sitemap: {PUBLIC_BASE}/sitemap.xml\n"
    ), media_type="text/plain")


@app.get("/sitemap.xml", response_class=HTMLResponse)
def sitemap_xml():
    rows = db().execute(
        "SELECT slug, COALESCE(published_at, created_at) AS lastmod "
        "FROM skus ORDER BY id DESC LIMIT 500"
    ).fetchall()
    static = ["/", "/gallery"]
    urls = []
    for u in static:
        urls.append(f"<url><loc>{PUBLIC_BASE}{u}</loc><changefreq>hourly</changefreq><priority>1.0</priority></url>")
    for r in rows:
        urls.append(
            f"<url><loc>{PUBLIC_BASE}/c/{r['slug']}</loc>"
            f"<lastmod>{r['lastmod']}</lastmod><priority>0.6</priority></url>"
        )
    xml = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n'
        + "\n".join(urls)
        + "\n</urlset>\n"
    )
    return HTMLResponse(content=xml, media_type="application/xml")


@app.get("/", response_class=HTMLResponse)
def home(request: FastRequest, response: Response):
    user = get_or_create_user(request, response)
    # Fetch latest 3 SKUs that have a mockup — render as hero for cold visitors
    rows = db().execute(
        "SELECT slug, catchphrase, kanji, mockup_white_url, mockup_black_url "
        "FROM skus WHERE mockup_white_url IS NOT NULL OR mockup_black_url IS NOT NULL "
        "ORDER BY id DESC LIMIT 3"
    ).fetchall()
    hero_cards = []
    for r in rows:
        mock = r["mockup_black_url"] or r["mockup_white_url"]
        if not mock:
            continue
        hero_cards.append(
            f'<a class="hero-card" href="/c/{r["slug"]}" title="{_xml_escape(r["catchphrase"] or "")}">'
            f'<img src="{mock}" loading="eager" alt="{_xml_escape(r["catchphrase"] or "")}">'
            f'<div class="hero-cap">{_xml_escape(r["catchphrase"] or "")}'
            f'{(" · " + _xml_escape(r["kanji"])) if r["kanji"] else ""}</div>'
            f'</a>'
        )
    hero_html = ('<div class="hero-row">' + "".join(hero_cards) + "</div>") if hero_cards else ""
    html = (HTML_INDEX
            .replace("{{mp_balance}}", str(user["mp_balance"]))
            .replace("{{hero}}", hero_html))
    return HTMLResponse(content=html)


@app.get("/api/me")
def api_me(request: FastRequest, response: Response):
    user = get_or_create_user(request, response)
    is_anon = user.get("email") is None
    return {
        "id": user["id"],
        "email": user.get("email"),
        "display_name": user.get("display_name"),
        "mp_balance": user["mp_balance"],
        "is_anon": is_anon,
        "rates": {
            "craft": CRAFT_COST_MP,
            "publish": PUBLISH_COST_MP,
            "tee_buyer": TEE_BUYER_MP,
            "tee_creator": TEE_CREATOR_MP,
            "signup_bonus": SIGNUP_BONUS_MP,
            "yen_per_mp": CASH_RATE_YEN_PER_MP,
        },
    }


@app.post("/api/craft")
def api_craft(request: FastRequest, response: Response, topic: str = Form(...)):
    user = get_or_create_user(request, response)
    topic = topic.strip()
    if not topic:
        return JSONResponse({"error": "empty_topic"}, status_code=400)
    if len(topic) > 300:
        return JSONResponse({"error": "topic_too_long", "max": 300}, status_code=400)

    # deduct MP first (refund on failure)
    ok, new_bal = mp_change(user["id"], -CRAFT_COST_MP, "craft", note=topic[:100])
    if not ok:
        return JSONResponse({
            "error": "insufficient_mp",
            "balance": new_bal,
            "cost": CRAFT_COST_MP,
            "topup_options": {
                "signup_bonus": SIGNUP_BONUS_MP if user.get("email") is None else 0,
                "cash_rate": f"¥{CASH_RATE_YEN_PER_MP}/MP",
                "tee_purchase": f"Tシャツ1枚購入で {TEE_BUYER_MP} MP",
            },
        }, status_code=402)

    try:
        result = run_craft_pipeline(user["id"], topic)
        result["mp_balance"] = new_bal
        result["mp_spent"] = CRAFT_COST_MP
        return result
    except Exception as e:
        # refund
        mp_change(user["id"], CRAFT_COST_MP, "refund_craft_error", note=str(e)[:100])
        return JSONResponse({"error": "craft_failed", "detail": str(e)[:300]}, status_code=500)


@app.post("/api/publish")
def api_publish(request: FastRequest, response: Response, sku_id: int = Form(...)):
    user = get_or_create_user(request, response)
    sku = db().execute("SELECT * FROM skus WHERE id=?", (sku_id,)).fetchone()
    if not sku:
        return JSONResponse({"error": "sku_not_found"}, status_code=404)
    if sku["creator_user_id"] != user["id"]:
        return JSONResponse({"error": "not_creator"}, status_code=403)
    if sku["status"] == "published":
        return JSONResponse({"error": "already_published"}, status_code=400)

    ok, new_bal = mp_change(user["id"], -PUBLISH_COST_MP, "publish",
                            ref_sku_id=sku_id, note=sku["slug"])
    if not ok:
        return JSONResponse({"error": "insufficient_mp", "balance": new_bal, "cost": PUBLISH_COST_MP}, status_code=402)

    # Locate the design PNG (black-ink variant is the canonical one)
    png_path = Path(sku["design_png_path"] or "")
    if not png_path.exists():
        # Fallback: maybe path is stale (different volume) — derive from STATIC_DIR
        derived = DESIGNS_DIR / f"{sku['slug']}_black.png"
        if derived.exists():
            png_path = derived
    if not png_path.exists():
        mp_change(user["id"], PUBLISH_COST_MP, "refund_publish_no_design",
                  ref_sku_id=sku_id, note="design png missing")
        return JSONResponse({"error": "design_file_missing", "expected": str(png_path)}, status_code=500)

    # SUZURI live publish
    title = f"MU CRAFT — {sku['catchphrase'] or sku['topic'][:40]}"
    suzuri_result = suzuri_publish(png_path, title)
    if not suzuri_result:
        mp_change(user["id"], PUBLISH_COST_MP, "refund_publish_suzuri_fail",
                  ref_sku_id=sku_id, note="suzuri api failed")
        return JSONResponse({"error": "suzuri_publish_failed",
                             "hint": "SUZURI_ACCESS_TOKEN set? token valid?"}, status_code=502)

    with db() as conn:
        conn.execute(
            "UPDATE skus SET status='published', suzuri_url=?, published_at=CURRENT_TIMESTAMP WHERE id=?",
            (suzuri_result["url"], sku_id),
        )

    return {
        "ok": True,
        "sku_id": sku_id,
        "status": "published",
        "mp_balance": new_bal,
        "suzuri_url": suzuri_result["url"],
        "suzuri_material_id": suzuri_result["material_id"],
    }


@app.post("/api/signup")
def api_signup(request: FastRequest, response: Response, email: str = Form(...)):
    """Frictionless email-only signup — instant register + bonus, no magic link.

    Magic-link verification was a CVR killer for paid traffic (users see
    "console output" placeholder and bounce). For our MVP scale, abuse risk
    is acceptable; rate limits on craft itself (1 MP/SKU) cap damage."""
    email = email.strip().lower()
    if not re.match(r"^[^@\s]+@[^@\s]+\.[^@\s]+$", email):
        return JSONResponse({"error": "invalid_email"}, status_code=400)
    with db() as conn:
        existing = conn.execute("SELECT id FROM users WHERE email=?", (email,)).fetchone()
        if existing:
            user_id = existing["id"]
        else:
            # Upgrade anon cookie user if present
            anon = request.cookies.get("mu_anon")
            anon_id = _verify_signed(anon) if anon else None
            anon_user = conn.execute(
                "SELECT id FROM users WHERE anon_id=?", (anon_id,)
            ).fetchone() if anon_id else None
            if anon_user:
                user_id = anon_user["id"]
                conn.execute("UPDATE users SET email=? WHERE id=?", (email, user_id))
            else:
                cur = conn.execute("INSERT INTO users (email, mp_balance) VALUES (?,?)", (email, 0))
                user_id = cur.lastrowid

    # signup bonus (once per user_id)
    already = db().execute(
        "SELECT id FROM mp_ledger WHERE user_id=? AND reason='signup_bonus'", (user_id,)
    ).fetchone()
    if not already:
        mp_change(user_id, SIGNUP_BONUS_MP, "signup_bonus", note=email)

    response.set_cookie("mu_session", _sign(str(user_id)), max_age=60*60*24*365, samesite="lax")
    new_bal = db().execute("SELECT mp_balance FROM users WHERE id=?", (user_id,)).fetchone()["mp_balance"]
    return {"ok": True, "user_id": user_id, "mp_balance": new_bal,
            "message": f"登録完了。+{SIGNUP_BONUS_MP} MP 付与。"}


# Legacy magic-link endpoint kept as fallback (not surfaced in UI anymore)
@app.post("/api/verify")
def api_verify(request: FastRequest, response: Response,
               email: str = Form(...), code: str = Form(...)):
    email = email.strip().lower()
    code = code.strip()
    now_iso = datetime.now(timezone.utc).isoformat()
    row = db().execute(
        "SELECT * FROM magic_links WHERE email=? AND code=? AND used=0 AND expires_at>? ORDER BY id DESC LIMIT 1",
        (email, code, now_iso)
    ).fetchone()
    if not row:
        return JSONResponse({"error": "invalid_or_expired"}, status_code=401)
    with db() as conn:
        conn.execute("UPDATE magic_links SET used=1 WHERE id=?", (row["id"],))
        existing = conn.execute("SELECT id FROM users WHERE email=?", (email,)).fetchone()
        if existing:
            user_id = existing["id"]
        else:
            cur = conn.execute("INSERT INTO users (email, mp_balance) VALUES (?,?)", (email, 0))
            user_id = cur.lastrowid
    already_bonused = db().execute(
        "SELECT id FROM mp_ledger WHERE user_id=? AND reason='signup_bonus'", (user_id,)
    ).fetchone()
    if not already_bonused:
        mp_change(user_id, SIGNUP_BONUS_MP, "signup_bonus", note=email)
    response.set_cookie("mu_session", _sign(str(user_id)), max_age=60*60*24*365, samesite="lax")
    new_bal = db().execute("SELECT mp_balance FROM users WHERE id=?", (user_id,)).fetchone()["mp_balance"]
    return {"ok": True, "user_id": user_id, "mp_balance": new_bal}


@app.post("/api/topup")
def api_topup(request: FastRequest, response: Response, yen: int = Form(...)):
    """MVP stub: in real flow, Stripe Checkout. Here we just credit immediately for testing."""
    user = get_or_create_user(request, response)
    if yen < 30 or yen > 100000:
        return JSONResponse({"error": "yen_out_of_range", "min": 30, "max": 100000}, status_code=400)
    mp = yen // CASH_RATE_YEN_PER_MP
    ok, new_bal = mp_change(user["id"], mp, "topup", note=f"¥{yen} → {mp} MP (MVP stub, no Stripe)")
    return {"ok": ok, "mp_added": mp, "mp_balance": new_bal,
            "note": "MVP stub: no real payment processed"}


@app.get("/c/{slug}")
def sku_page(slug: str):
    sku = db().execute("SELECT * FROM skus WHERE slug=?", (slug,)).fetchone()
    if not sku:
        return HTMLResponse("<h1>404 — SKU not found</h1>", status_code=404)
    with db() as conn:
        conn.execute("UPDATE skus SET view_count=view_count+1 WHERE id=?", (sku["id"],))
    creator = db().execute("SELECT display_name, email FROM users WHERE id=?", (sku["creator_user_id"],)).fetchone()
    creator_name = (creator["display_name"] or "Anonymous") if creator else "Anonymous"
    return HTMLResponse(content=render_sku_page(dict(sku), creator_name))


@app.get("/api/skus")
def api_skus(limit: int = 20):
    rows = db().execute(
        "SELECT id, slug, topic, catchphrase, kanji, mockup_white_url, mockup_black_url, status, view_count, created_at "
        "FROM skus ORDER BY id DESC LIMIT ?",
        (max(1, min(100, limit)),)
    ).fetchall()
    return [dict(r) for r in rows]


FOUNDING_DOC_PATH = ROOT / "docs" / "founding" / "01_declaration_2026-05-20.md"


@app.get("/founding", response_class=HTMLResponse)
def founding_page():
    """Public render of the MU Founding Declaration (markdown → minimal HTML)."""
    if not FOUNDING_DOC_PATH.exists():
        return HTMLResponse("<h1>404 — founding declaration not bundled in this build</h1>", status_code=404)
    md = FOUNDING_DOC_PATH.read_text(encoding="utf-8")
    # Minimal markdown → HTML (heads, bold, blockquote, hr, list, code, paragraphs)
    html_body = _md_to_html(md)
    return HTMLResponse(content=f"""<!DOCTYPE html>
<html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>MU 創立宣言 / The MU Founding Declaration</title>
<meta name="description" content="2026年5月20日、東京。MUは「物として世に出す」を空気にする。">
<meta property="og:title" content="MU 創立宣言 — The MU Founding Declaration">
<meta property="og:description" content="2026年5月20日、東京。MU は「物として世に出す」を空気にする。">
<meta property="og:type" content="article">
<meta property="og:image" content="https://mockups.wearmu.com/bjj-triangle/kakudo-kanji-black-tee.jpg">
<link rel="canonical" href="{PUBLIC_BASE}/founding">
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
<style>
body {{ font-family: -apple-system, "Helvetica Neue", "Hiragino Mincho ProN", serif;
       background: #0a0a0a; color: #f5f5f0; padding: 60px 24px; max-width: 760px;
       margin: 0 auto; line-height: 1.8; font-size: 16px; }}
h1 {{ font-size: 36px; font-weight: 900; letter-spacing: -1px; margin: 40px 0 16px;
      font-family: -apple-system, "Helvetica Neue", sans-serif; }}
h2 {{ font-size: 24px; font-weight: 900; margin: 48px 0 12px; letter-spacing: -0.5px;
      font-family: -apple-system, "Helvetica Neue", sans-serif; color: #e6c449; }}
h3 {{ font-size: 18px; font-weight: 700; margin: 32px 0 8px; opacity: 0.9; }}
strong {{ color: #e6c449; }}
hr {{ border: 0; border-top: 1px solid #2a2a2a; margin: 32px 0; }}
blockquote {{ border-left: 3px solid #e6c449; padding: 12px 0 12px 20px;
              margin: 24px 0; opacity: 0.85; font-style: italic; }}
ul, ol {{ padding-left: 24px; }}
li {{ margin: 6px 0; }}
code {{ background: #1a1a1a; padding: 2px 6px; border-radius: 3px; font-size: 14px;
        font-family: ui-monospace, monospace; }}
table {{ border-collapse: collapse; margin: 16px 0; font-size: 14px; }}
th, td {{ padding: 8px 12px; border: 1px solid #2a2a2a; }}
th {{ background: #1a1a1a; }}
.back {{ display: inline-block; margin-bottom: 24px; color: #e6c449; text-decoration: none;
         font-family: -apple-system, sans-serif; font-size: 14px; }}
em {{ opacity: 0.7; }}
footer {{ margin-top: 80px; opacity: 0.4; font-size: 12px; text-align: center; }}
</style></head><body>
<a class="back" href="/">← MU CRAFT</a>
{html_body}
<footer>Letter #00 — MU Annual Letters · 2026-05-20 · Tokyo</footer>
</body></html>""")


def _md_to_html(md: str) -> str:
    """Minimal markdown renderer — enough for the founding doc, no external dep."""
    lines = md.split("\n")
    out = []
    in_code = False
    in_table = False
    in_list = False
    for ln in lines:
        if ln.startswith("```"):
            in_code = not in_code
            out.append("<pre><code>" if in_code else "</code></pre>")
            continue
        if in_code:
            out.append(_xml_escape(ln))
            continue
        if ln.strip().startswith("|") and ln.strip().endswith("|"):
            cells = [c.strip() for c in ln.strip().strip("|").split("|")]
            if all(set(c) <= set("-:") for c in cells if c):
                continue  # separator row
            tag = "th" if not in_table else "td"
            row = "".join(f"<{tag}>{_md_inline(c)}</{tag}>" for c in cells)
            if not in_table:
                out.append("<table>")
                in_table = True
            out.append(f"<tr>{row}</tr>")
            continue
        if in_table and not (ln.strip().startswith("|")):
            out.append("</table>")
            in_table = False
        if ln.startswith("# "):
            out.append(f"<h1>{_md_inline(ln[2:])}</h1>")
        elif ln.startswith("## "):
            out.append(f"<h2>{_md_inline(ln[3:])}</h2>")
        elif ln.startswith("### "):
            out.append(f"<h3>{_md_inline(ln[4:])}</h3>")
        elif ln.startswith("> "):
            out.append(f"<blockquote>{_md_inline(ln[2:])}</blockquote>")
        elif ln.strip() in ("---", "***"):
            out.append("<hr>")
        elif ln.startswith("- ") or ln.startswith("* "):
            if not in_list:
                out.append("<ul>")
                in_list = True
            out.append(f"<li>{_md_inline(ln[2:])}</li>")
        elif ln.strip() == "":
            if in_list:
                out.append("</ul>")
                in_list = False
            out.append("")
        else:
            if in_list:
                out.append("</ul>")
                in_list = False
            out.append(f"<p>{_md_inline(ln)}</p>")
    if in_list:
        out.append("</ul>")
    if in_table:
        out.append("</table>")
    return "\n".join(out)


def _md_inline(s: str) -> str:
    s = _xml_escape(s)
    s = re.sub(r"\*\*(.+?)\*\*", r"<strong>\1</strong>", s)
    s = re.sub(r"\*(.+?)\*", r"<em>\1</em>", s)
    s = re.sub(r"`(.+?)`", r"<code>\1</code>", s)
    s = re.sub(r"\[\[([^\]]+)\]\]", r"\1", s)  # strip wiki-links
    s = re.sub(r"\[([^\]]+)\]\(([^)]+)\)", r'<a href="\2">\1</a>', s)
    return s


@app.get("/admin", response_class=HTMLResponse)
def admin_dashboard(token: str = ""):
    """Live funnel + ledger dashboard. Token in URL query: /admin?token=XXX"""
    if not token or token != ADMIN_TOKEN:
        return HTMLResponse("<h1>403 — ?token= required</h1>", status_code=403)
    with db() as conn:
        stats = {
            "skus_total": conn.execute("SELECT COUNT(*) AS c FROM skus").fetchone()["c"],
            "skus_published": conn.execute("SELECT COUNT(*) AS c FROM skus WHERE status='published'").fetchone()["c"],
            "skus_last_24h": conn.execute("SELECT COUNT(*) AS c FROM skus WHERE created_at > datetime('now','-1 day')").fetchone()["c"],
            "skus_last_1h": conn.execute("SELECT COUNT(*) AS c FROM skus WHERE created_at > datetime('now','-1 hour')").fetchone()["c"],
            "users_total": conn.execute("SELECT COUNT(*) AS c FROM users").fetchone()["c"],
            "users_registered": conn.execute("SELECT COUNT(*) AS c FROM users WHERE email IS NOT NULL").fetchone()["c"],
            "users_last_1h": conn.execute("SELECT COUNT(*) AS c FROM users WHERE created_at > datetime('now','-1 hour')").fetchone()["c"],
        }
        ledger = conn.execute(
            "SELECT reason, COUNT(*) AS n, SUM(delta) AS net "
            "FROM mp_ledger GROUP BY reason ORDER BY n DESC"
        ).fetchall()
        latest = conn.execute(
            "SELECT id, slug, topic, catchphrase, status, suzuri_url, view_count, created_at "
            "FROM skus ORDER BY id DESC LIMIT 30"
        ).fetchall()
        publishes = conn.execute(
            "SELECT COUNT(*) AS c FROM skus WHERE status='published'"
        ).fetchone()["c"]
    cvr_signup = (stats["users_registered"] / stats["users_total"] * 100) if stats["users_total"] else 0
    cvr_publish = (publishes / stats["skus_total"] * 100) if stats["skus_total"] else 0

    ledger_rows = "".join(
        f"<tr><td>{_xml_escape(r['reason'])}</td><td>{r['n']}</td><td>{r['net']:+d}</td></tr>"
        for r in ledger
    )
    def _sku_row(r):
        suzuri = f'<a href="{r["suzuri_url"]}" target="_blank">SUZURI ↗</a>' if r["suzuri_url"] else "-"
        return (
            f"<tr><td><a href='/c/{r['slug']}' target='_blank'>{r['id']}</a></td>"
            f"<td>{_xml_escape(r['catchphrase'] or '')}</td>"
            f"<td>{_xml_escape(r['topic'][:40])}</td>"
            f"<td>{r['status']}</td><td>{r['view_count']}</td>"
            f"<td>{r['created_at']}</td>"
            f"<td>{suzuri}</td></tr>"
        )
    sku_rows = "".join(_sku_row(r) for r in latest)

    return HTMLResponse(content=f"""<!DOCTYPE html>
<html lang="ja"><head><meta charset="utf-8"><title>MU CRAFT — Admin</title>
<style>
body {{ font-family: -apple-system, "Helvetica Neue", sans-serif; background: #0a0a0a;
       color: #f5f5f0; padding: 24px; max-width: 1100px; margin: 0 auto; }}
h1 {{ font-size: 24px; font-weight: 900; }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
         gap: 12px; margin: 20px 0 40px; }}
.kpi {{ background: #1a1a1a; padding: 16px; border-radius: 8px;
        border: 1px solid #2a2a2a; }}
.kpi .v {{ font-size: 28px; font-weight: 900; color: #e6c449; }}
.kpi .l {{ font-size: 11px; opacity: 0.6; letter-spacing: 0.5px; }}
h2 {{ margin-top: 32px; font-size: 16px; opacity: 0.8; }}
table {{ width: 100%; border-collapse: collapse; font-size: 13px;
         background: #1a1a1a; border-radius: 8px; overflow: hidden; }}
th, td {{ padding: 8px 12px; border-bottom: 1px solid #2a2a2a; text-align: left; }}
th {{ background: #2a2a2a; font-weight: 700; font-size: 11px; letter-spacing: 1px; }}
a {{ color: #e6c449; text-decoration: none; }}
</style></head><body>
<h1>MU CRAFT — Admin Dashboard</h1>
<p style="opacity:0.5; font-size:12px;">Generated: {datetime.now(timezone.utc).isoformat()}</p>

<div class="grid">
  <div class="kpi"><div class="v">{stats['skus_total']}</div><div class="l">SKUS TOTAL</div></div>
  <div class="kpi"><div class="v">{stats['skus_published']}</div><div class="l">SKUS PUBLISHED</div></div>
  <div class="kpi"><div class="v">{stats['skus_last_24h']}</div><div class="l">SKUS / 24H</div></div>
  <div class="kpi"><div class="v">{stats['skus_last_1h']}</div><div class="l">SKUS / 1H</div></div>
  <div class="kpi"><div class="v">{stats['users_total']}</div><div class="l">USERS</div></div>
  <div class="kpi"><div class="v">{stats['users_registered']}</div><div class="l">REGISTERED</div></div>
  <div class="kpi"><div class="v">{stats['users_last_1h']}</div><div class="l">USERS / 1H</div></div>
  <div class="kpi"><div class="v">{cvr_signup:.1f}%</div><div class="l">CVR SIGNUP</div></div>
  <div class="kpi"><div class="v">{cvr_publish:.1f}%</div><div class="l">CVR PUBLISH</div></div>
</div>

<h2>MP Ledger</h2>
<table><thead><tr><th>reason</th><th>n events</th><th>net MP</th></tr></thead>
<tbody>{ledger_rows}</tbody></table>

<h2>Latest SKUs (30)</h2>
<table><thead><tr><th>id</th><th>catch</th><th>topic</th><th>status</th><th>views</th><th>created</th><th>suzuri</th></tr></thead>
<tbody>{sku_rows}</tbody></table>

<p style="margin-top:40px; opacity:0.4; font-size:11px;">
  <a href="/api/stats" target="_blank">/api/stats JSON</a> ·
  <a href="/api/skus" target="_blank">/api/skus JSON</a> ·
  <a href="/gallery" target="_blank">/gallery</a>
</p>
</body></html>""")


@app.get("/api/stats")
def api_stats():
    """Public aggregate stats — no PII. Lets users see scale + me monitor without ssh."""
    with db() as conn:
        sku_total = conn.execute("SELECT COUNT(*) AS c FROM skus").fetchone()["c"]
        sku_published = conn.execute("SELECT COUNT(*) AS c FROM skus WHERE status='published'").fetchone()["c"]
        user_total = conn.execute("SELECT COUNT(*) AS c FROM users").fetchone()["c"]
        user_registered = conn.execute("SELECT COUNT(*) AS c FROM users WHERE email IS NOT NULL").fetchone()["c"]
        mp_burned_total = conn.execute("SELECT COALESCE(SUM(-delta), 0) AS c FROM mp_ledger WHERE delta < 0").fetchone()["c"]
        mp_earned_total = conn.execute("SELECT COALESCE(SUM(delta), 0) AS c FROM mp_ledger WHERE delta > 0").fetchone()["c"]
        last_24h_skus = conn.execute("SELECT COUNT(*) AS c FROM skus WHERE created_at > datetime('now','-1 day')").fetchone()["c"]
    return {
        "skus_total": sku_total,
        "skus_published": sku_published,
        "skus_last_24h": last_24h_skus,
        "users_total": user_total,
        "users_registered": user_registered,
        "mp_burned_total": mp_burned_total,
        "mp_earned_total": mp_earned_total,
    }


@app.get("/gallery", response_class=HTMLResponse)
def gallery():
    """Public gallery — social proof for visitors, viral growth for creators."""
    rows = db().execute(
        "SELECT slug, topic, catchphrase, kanji, mockup_white_url, mockup_black_url, view_count "
        "FROM skus WHERE mockup_white_url IS NOT NULL OR mockup_black_url IS NOT NULL "
        "ORDER BY id DESC LIMIT 60"
    ).fetchall()
    cards = []
    for r in rows:
        mock = r["mockup_black_url"] or r["mockup_white_url"]
        if not mock:
            continue
        cards.append(f"""
        <a class="card" href="/c/{r['slug']}">
          <img src="{mock}" loading="lazy" alt="{_xml_escape(r['catchphrase'] or '')}">
          <div class="cap">{_xml_escape(r['catchphrase'] or '')} {('· ' + _xml_escape(r['kanji'])) if r['kanji'] else ''}</div>
          <div class="cap-sub">{_xml_escape(r['topic'][:50])}</div>
        </a>""")
    cards_html = "\n".join(cards) if cards else '<p style="opacity:0.5">まだ SKU がありません。<a href="/">最初の 1 個を作る ↗</a></p>'
    return HTMLResponse(GALLERY_HTML.replace("{{cards}}", cards_html).replace("{{count}}", str(len(rows))))


GALLERY_HTML = """<!DOCTYPE html>
<html lang="ja"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Gallery · MU CRAFT</title>
<meta name="description" content="MU CRAFT で生成された最新のTシャツデザイン">
<meta property="og:title" content="MU CRAFT Gallery">
<meta property="og:description" content="作るを空気にする — 生成済み SKU 一覧">
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
<style>
* { box-sizing: border-box; }
body { font-family: -apple-system, "Helvetica Neue", "Hiragino Kaku Gothic ProN", sans-serif;
       background: #0a0a0a; color: #f5f5f0; margin: 0; padding: 24px 16px; }
.wrap { max-width: 1200px; margin: 0 auto; }
.topbar { display: flex; justify-content: space-between; align-items: center;
          padding: 14px 18px; background: #1a1a1a; border-radius: 8px;
          margin-bottom: 24px; border: 1px solid #2a2a2a; }
.brand { font-weight: 900; letter-spacing: 2px; font-size: 18px; }
.brand a { color: #e6c449; text-decoration: none; }
h1 { font-size: 36px; font-weight: 900; margin: 24px 0 8px; letter-spacing: -1px; }
.sub { opacity: 0.5; margin-bottom: 32px; font-size: 14px; }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 16px; }
.card { background: #1a1a1a; border: 1px solid #2a2a2a; border-radius: 8px;
        text-decoration: none; color: #f5f5f0; transition: transform 0.1s;
        display: block; overflow: hidden; }
.card:hover { transform: translateY(-2px); border-color: #e6c449; }
.card img { width: 100%; aspect-ratio: 1; object-fit: cover; background: #fff; display: block; }
.cap { padding: 10px 12px 4px; font-weight: 900; font-size: 14px; letter-spacing: 0.5px; }
.cap-sub { padding: 0 12px 12px; font-size: 11px; opacity: 0.5; }
.cta-bottom { text-align: center; margin: 40px 0 80px; }
.cta-bottom a { display: inline-block; background: #e6c449; color: #0a0a0a;
                padding: 16px 32px; font-weight: 900; border-radius: 8px;
                text-decoration: none; letter-spacing: 1px; }
</style></head>
<body><div class="wrap">
<div class="topbar">
  <span class="brand"><a href="/">← MU CRAFT</a></span>
  <a href="/" style="color: #e6c449; text-decoration: none;">自分も作る →</a>
</div>
<h1>Gallery</h1>
<p class="sub">最新 {{count}} 件 · 作るを空気にする</p>
<div class="grid">{{cards}}</div>
<div class="cta-bottom"><a href="/">何かを Tシャツにする →</a></div>
</div></body></html>"""


# ───────────────────────────────────────────────────────────── HTML
HTML_INDEX = """<!DOCTYPE html>
<html lang="ja">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>MU CRAFT — 作るを空気にする</title>
  <meta name="description" content="思考をTシャツに、発話1行で。MU CRAFTはトピックを1行入れるだけで、MUブランドスタイルのTシャツデザインとモックアップを10秒で生成します。">
  <meta property="og:title" content="MU CRAFT — 作るを空気にする">
  <meta property="og:description" content="トピック1行 → Tシャツデザイン + モックアップ。10秒。">
  <meta property="og:type" content="website">
  <meta property="og:image" content="https://mockups.wearmu.com/bjj-triangle/kakudo-kanji-black-tee.jpg">
  <meta name="twitter:card" content="summary_large_image">
  <link rel="icon" href="data:image/svg+xml;utf8,&lt;svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'&gt;&lt;rect width='100' height='100' fill='%230a0a0a'/&gt;&lt;text x='50' y='72' text-anchor='middle' font-family='serif' font-size='80' font-weight='900' fill='%23e6c449'&gt;無&lt;/text&gt;&lt;/svg&gt;">
  <script defer src="https://enabler-analytics.fly.dev/t.js"></script>
  <style>
    * { box-sizing: border-box; }
    body { font-family: -apple-system, "Helvetica Neue", "Hiragino Kaku Gothic ProN", sans-serif;
           background: #0a0a0a; color: #f5f5f0; margin: 0; padding: 24px 16px;
           min-height: 100vh; line-height: 1.5; }
    .wrap { max-width: 720px; margin: 0 auto; }
    .topbar { display: flex; justify-content: space-between; align-items: center;
              padding: 14px 18px; background: #1a1a1a; border-radius: 8px;
              margin-bottom: 32px; border: 1px solid #2a2a2a; }
    .brand { font-weight: 900; letter-spacing: 2px; font-size: 18px; }
    .mp { font-weight: 700; color: #e6c449; font-variant-numeric: tabular-nums; }
    h1 { font-size: clamp(40px, 8vw, 72px); font-weight: 900; letter-spacing: -2px;
         margin: 0 0 24px; line-height: 1.0; }
    .sub { opacity: 0.6; font-size: 14px; margin-bottom: 32px; }
    textarea { width: 100%; height: 110px; font-size: 18px; padding: 16px;
               background: #1a1a1a; color: #f5f5f0; border: 1px solid #2a2a2a;
               border-radius: 8px; resize: vertical; font-family: inherit;
               line-height: 1.4; }
    textarea:focus { outline: none; border-color: #e6c449; }
    .actions { display: flex; gap: 12px; align-items: center; margin-top: 16px; flex-wrap: wrap; }
    button { background: #e6c449; color: #0a0a0a; border: none;
             padding: 16px 28px; font-size: 17px; font-weight: 900;
             cursor: pointer; border-radius: 8px; letter-spacing: 1px;
             font-family: inherit; transition: transform 0.05s; }
    button:hover { background: #f3d56f; }
    button:active { transform: translateY(1px); }
    button:disabled { opacity: 0.4; cursor: not-allowed; }
    button.ghost { background: transparent; color: #f5f5f0; border: 1px solid #333;
                   padding: 10px 16px; font-size: 14px; }
    button.ghost:hover { border-color: #e6c449; color: #e6c449; }
    .result { margin-top: 40px; }
    .result-card { background: #1a1a1a; border: 1px solid #2a2a2a; border-radius: 12px;
                   padding: 24px; margin-bottom: 24px; }
    .result-card h2 { margin: 0 0 8px; font-size: 28px; }
    .result-card .meta { opacity: 0.6; font-size: 13px; margin-bottom: 16px; font-family: monospace; }
    .mockups { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
    .mockups img { width: 100%; border-radius: 8px; display: block; background: #fff; }
    .spinner { padding: 60px; text-align: center; opacity: 0.6; font-size: 14px; }
    .err { color: #ff6b6b; padding: 16px; border: 1px solid #ff6b6b40; border-radius: 8px; background: #ff6b6b10; }
    .signup-box { background: #1a1a1a; border: 1px solid #2a2a2a; padding: 20px; border-radius: 8px; margin-top: 16px; }
    .signup-box input { width: 100%; padding: 12px; background: #0a0a0a; color: #f5f5f0;
                        border: 1px solid #2a2a2a; border-radius: 6px; font-size: 16px; margin-bottom: 8px; }
    .signup-box label { font-size: 13px; opacity: 0.8; display: block; margin-bottom: 4px; }
    footer { margin-top: 80px; text-align: center; opacity: 0.4; font-size: 12px; }
    footer a { color: #e6c449; text-decoration: none; }
    code { background: #2a2a2a; padding: 2px 6px; border-radius: 3px; font-size: 13px; }
    .pills { margin-top: 20px; display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
    .pill-label { opacity: 0.5; font-size: 12px; margin-right: 4px; letter-spacing: 1px; }
    .pill { background: #1a1a1a; color: #f5f5f0; border: 1px solid #2a2a2a; border-radius: 999px;
            padding: 8px 14px; font-size: 13px; font-weight: 600; cursor: pointer;
            font-family: inherit; transition: all 0.1s; }
    .pill:hover { background: #e6c449; color: #0a0a0a; border-color: #e6c449; }
    .pill:disabled { opacity: 0.4; cursor: not-allowed; }
    .hero-row { display: grid; grid-template-columns: repeat(3, 1fr); gap: 8px;
                margin: 0 0 32px; }
    .hero-card { background: #1a1a1a; border: 1px solid #2a2a2a; border-radius: 8px;
                 text-decoration: none; color: #f5f5f0; overflow: hidden;
                 transition: transform 0.1s, border-color 0.1s; display: block; }
    .hero-card:hover { transform: translateY(-2px); border-color: #e6c449; }
    .hero-card img { width: 100%; aspect-ratio: 1; object-fit: cover; background: #fff;
                     display: block; }
    .hero-cap { padding: 6px 10px; font-weight: 900; font-size: 11px;
                letter-spacing: 0.3px; white-space: nowrap; overflow: hidden;
                text-overflow: ellipsis; }
    @media (max-width: 540px) {
      .hero-row { gap: 4px; }
      .hero-cap { padding: 4px 6px; font-size: 9px; }
    }
  </style>
</head>
<body>
<div class="wrap">

  <div class="topbar">
    <span class="brand">MU CRAFT</span>
    <span class="mp" id="mp-display">残量: -- MP</span>
  </div>

  {{hero}}

  <h1>何を<br>Tシャツに<br>する？</h1>

  <div class="sub">トピックを1行で。10秒でデザイン + モックアップが生成されます。</div>

  <textarea id="topic" placeholder="例: 柔術の三角絞めの理論 / 朝のコーヒーの哲学 / 静寂の重要性"></textarea>

  <div class="actions">
    <button id="craft-btn" onclick="craft()">作る (1 MP)</button>
    <button class="ghost" id="random-btn" onclick="craftRandom()">🎲 ランダム (1 MP)</button>
    <button class="ghost" onclick="showSignup()" id="signup-btn">登録して +5 MP</button>
    <button class="ghost" onclick="topup()">チャージ (¥30/MP)</button>
  </div>

  <div class="pills" id="pills">
    <span class="pill-label">ワンクリック:</span>
    <button class="pill" onclick="craftWith('柔術の三角絞めの理論')">三角絞め</button>
    <button class="pill" onclick="craftWith('朝のコーヒーの哲学')">朝のコーヒー</button>
    <button class="pill" onclick="craftWith('東京の夜')">東京の夜</button>
    <button class="pill" onclick="craftWith('静寂の重要性')">静寂</button>
    <button class="pill" onclick="craftWith('Rustの所有権')">Rust 所有権</button>
    <button class="pill" onclick="craftWith('柔術 黒帯への道')">黒帯への道</button>
    <button class="pill" onclick="craftWith('Mercariの設計思想')">Mercari 思想</button>
    <button class="pill" onclick="craftWith('禅の本質')">禅</button>
    <button class="pill" onclick="craftWith('海の波の構造')">波の構造</button>
    <button class="pill" onclick="craftWith('深夜のデプロイ')">深夜デプロイ</button>
    <button class="pill" onclick="craftWith('無')">無</button>
    <button class="pill" onclick="craftWith('物として世に出す')">物として世に出す</button>
  </div>

  <div id="signup-area"></div>
  <div id="result" class="result"></div>

  <footer>
    MU CRAFT v0.1 — 作るを空気にする<br>
    <a href="/gallery">Gallery</a> · <a href="/api/stats" target="_blank">Stats</a> · <a href="/api/skus" target="_blank">SKU 一覧 (JSON)</a>
  </footer>
</div>

<script>
let me_cache = null;

async function loadMe() {
  const r = await fetch('/api/me');
  me_cache = await r.json();
  document.getElementById('mp-display').textContent =
    `残量: ${me_cache.mp_balance} MP` + (me_cache.is_anon ? '' : ` · ${me_cache.email}`);
  if (!me_cache.is_anon) {
    document.getElementById('signup-btn').style.display = 'none';
  }
}

const RANDOM_TOPICS = [
  '柔術の三角絞めの理論', '朝のコーヒーの哲学', '東京の夜', '静寂の重要性',
  'Rustの所有権', '柔術 黒帯への道', 'Mercariの設計思想', '禅の本質',
  '海の波の構造', '深夜のデプロイ', '無', '物として世に出す',
  '猫の歩き方', '味噌汁の温度', 'Vim の指の記憶', '弟子屈の冬',
  '日本酒の余韻', '原稿用紙の手触り', 'AI の倫理', '京都の路地',
  '柔術紫帯の壁', 'コードレビューの礼儀', '一期一会', '満員電車の哲学'
];

function setBtnsDisabled(d) {
  document.querySelectorAll('button').forEach(b => { if (b.id !== 'signup-btn') b.disabled = d; });
}

async function craftWith(topic) {
  document.getElementById('topic').value = topic;
  await craft();
}

async function craftRandom() {
  const t = RANDOM_TOPICS[Math.floor(Math.random() * RANDOM_TOPICS.length)];
  await craftWith(t);
}

async function craft() {
  const topic = document.getElementById('topic').value.trim();
  if (!topic) { alert('トピックを入力するか、ワンクリックピルを押してください'); return; }
  setBtnsDisabled(true);
  document.getElementById('result').innerHTML =
    '<div class="spinner">生成中 (Gemini brief → SVG → Printful mockup × 2)... 約10秒<br><br><span style="opacity:0.5">topic: ' + escapeHtml(topic) + '</span></div>';

  const fd = new FormData();
  fd.append('topic', topic);
  const r = await fetch('/api/craft', { method: 'POST', body: fd });
  const data = await r.json();

  if (r.status === 402) {
    document.getElementById('result').innerHTML = `
      <div class="err">
        <p><strong>無料枠を使い切りました</strong> (残量 ${data.balance} MP)</p>
        <p>下のオプションのいずれかでチャージしてください:</p>
        <ul>
          ${data.topup_options.signup_bonus ? `<li>登録すれば +${data.topup_options.signup_bonus} MP 即時付与</li>` : ''}
          <li>現金チャージ: ${data.topup_options.cash_rate}</li>
          <li>${data.topup_options.tee_purchase}</li>
        </ul>
      </div>`;
    setBtnsDisabled(false);
    return;
  }
  if (r.status >= 400) {
    document.getElementById('result').innerHTML = `<div class="err">エラー: ${data.error || 'unknown'} — ${data.detail || ''}</div>`;
    setBtnsDisabled(false);
    return;
  }

  document.getElementById('result').innerHTML = `
    <div class="result-card">
      <h2>${escapeHtml(data.brief.catchphrase)} ${data.brief.kanji ? '· ' + escapeHtml(data.brief.kanji) : ''}</h2>
      <div class="meta">topic: ${escapeHtml(data.topic)} · slug: <code>${data.slug}</code> · ${data.elapsed_sec}s · 残量 ${data.mp_balance} MP</div>
      <div class="mockups">
        ${data.mockup_white ? `<img src="${data.mockup_white}" alt="white tee" />` : '<div>白T mockup 生成失敗</div>'}
        ${data.mockup_black ? `<img src="${data.mockup_black}" alt="black tee" />` : '<div>黒T mockup 生成失敗</div>'}
      </div>
      <div class="actions" style="margin-top: 16px;">
        <button onclick="publish(${data.sku_id})">公開する (3 MP)</button>
        <button class="ghost" onclick="window.open('/c/${data.slug}','_blank')">永久 URL ↗</button>
        <button class="ghost" data-slug="${data.slug}" data-catch="${escapeHtml(data.brief.catchphrase)}" onclick="shareX(this.dataset.slug, this.dataset.catch)">𝕏 シェア</button>
        <button class="ghost" data-topic="${escapeHtml(data.topic)}" onclick="craftWith(this.dataset.topic)">↻ もう一回 (1 MP)</button>
        <button class="ghost" onclick="craftRandom()">🎲 別のランダム (1 MP)</button>
      </div>
    </div>`;
  setBtnsDisabled(false);
  loadMe();
}

async function publish(sku_id) {
  if (!confirm('SUZURI + Printful に公開しますか？ (3 MP)')) return;
  const fd = new FormData(); fd.append('sku_id', sku_id);
  const r = await fetch('/api/publish', { method: 'POST', body: fd });
  const data = await r.json();
  if (r.status >= 400) { alert(data.error || 'failed'); return; }
  alert(`公開完了: ${data.note || ''}`);
  loadMe();
}

function showSignup() {
  document.getElementById('signup-area').innerHTML = `
    <div class="signup-box">
      <label>メールアドレス (確認なし・即時 +5 MP)</label>
      <input id="email-input" type="email" placeholder="you@example.com" autocomplete="email" />
      <button onclick="signupSubmit()">登録 (+5 MP)</button>
      <div style="font-size:11px;opacity:0.4;margin-top:8px;">※ 確認メール無し。後から購入連携で本人認証されます。</div>
    </div>`;
  setTimeout(() => document.getElementById('email-input').focus(), 50);
}

async function signupSubmit() {
  const email = document.getElementById('email-input').value.trim();
  if (!email) return;
  const fd = new FormData(); fd.append('email', email);
  const r = await fetch('/api/signup', { method: 'POST', body: fd });
  const d = await r.json();
  if (r.status >= 400) { alert(d.error || 'failed'); return; }
  document.getElementById('signup-area').innerHTML =
    `<div class="signup-box">✓ ${d.message || '登録完了'} 残量 ${d.mp_balance} MP</div>`;
  loadMe();
}

async function topup() {
  const yen = parseInt(prompt('チャージ金額 (¥) — 30の倍数推奨', '300'), 10);
  if (!yen || yen < 30) return;
  const fd = new FormData(); fd.append('yen', yen);
  const r = await fetch('/api/topup', { method: 'POST', body: fd });
  const d = await r.json();
  alert(`+${d.mp_added} MP (新残量 ${d.mp_balance} MP) ${d.note ? '· ' + d.note : ''}`);
  loadMe();
}

function escapeHtml(s) {
  return String(s || '').replace(/[&<>"']/g, m => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[m]));
}

function shareX(slug, catchphrase) {
  const url = `${location.origin}/c/${slug}`;
  const text = `${catchphrase} · MU CRAFT で作りました\\n\\n「作る」を空気にする`;
  const intent = `https://twitter.com/intent/tweet?text=${encodeURIComponent(text)}&url=${encodeURIComponent(url)}&hashtags=MUCRAFT`;
  window.open(intent, '_blank', 'noopener,width=550,height=420');
}

loadMe();
</script>
</body>
</html>
"""


def render_sku_page(sku: dict, creator_name: str) -> str:
    return f"""<!DOCTYPE html>
<html lang="ja"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{_xml_escape(sku['catchphrase'] or sku['topic'])} · MU CRAFT</title>
<meta name="description" content="{_xml_escape(sku['subtitle'] or sku['topic'])} — MU CRAFT で生成">
<meta property="og:title" content="{_xml_escape(sku['catchphrase'] or '')} · MU">
<meta property="og:description" content="{_xml_escape(sku['subtitle'] or sku['topic'])}">
<meta property="og:image" content="{sku.get('mockup_black_url') or sku.get('mockup_white_url') or ''}">
<meta name="twitter:card" content="summary_large_image">
<link rel="canonical" href="{PUBLIC_BASE}/c/{sku['slug']}">
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
<style>
body {{ font-family: -apple-system, "Helvetica Neue", "Hiragino Kaku Gothic ProN", sans-serif;
       background: #0a0a0a; color: #f5f5f0; margin: 0; padding: 24px;
       max-width: 820px; margin: 0 auto; }}
h1 {{ font-size: 48px; letter-spacing: -1px; font-weight: 900; margin: 24px 0 8px; }}
.kanji {{ font-size: 96px; font-family: "Hiragino Mincho ProN", serif; color: #e6c449;
         margin: 16px 0; }}
.meta {{ font-family: monospace; opacity: 0.5; font-size: 13px; }}
.mockups {{ display: grid; grid-template-columns: 1fr 1fr; gap: 16px; margin: 32px 0; }}
.mockups img {{ width: 100%; border-radius: 8px; background: #fff; }}
.topic-block {{ background: #1a1a1a; padding: 20px; border-radius: 8px; margin: 24px 0; }}
.back {{ display: inline-block; margin-top: 32px; color: #e6c449; text-decoration: none; }}
</style></head>
<body>
<a class="back" href="/">← MU CRAFT</a>
<h1>{_xml_escape(sku['catchphrase'] or '')}</h1>
{f'<div class="kanji">{_xml_escape(sku["kanji"])}</div>' if sku.get('kanji') else ''}
<div class="meta">slug: <code>{sku['slug']}</code> · creator: {_xml_escape(creator_name)} · views: {sku.get('view_count', 0)} · status: {sku['status']}</div>
<div class="topic-block">
  <strong>{_xml_escape(sku.get('subtitle') or '')}</strong>
  <p style="opacity:0.7; margin: 8px 0 0;">topic: {_xml_escape(sku['topic'])}</p>
</div>
<div class="mockups">
  {f'<img src="{sku["mockup_white_url"]}" alt="white tee">' if sku.get('mockup_white_url') else ''}
  {f'<img src="{sku["mockup_black_url"]}" alt="black tee">' if sku.get('mockup_black_url') else ''}
</div>
<a class="back" href="/">← 自分も作る</a>
</body></html>"""


# ───────────────────────────────────────────────────────────── main
if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--host", default="0.0.0.0")
    ap.add_argument("--port", type=int, default=PORT)
    args = ap.parse_args()
    uvicorn.run(app, host=args.host, port=args.port, log_level="info")
