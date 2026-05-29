#!/usr/bin/env python3
"""
MU Brand — Autonomous AI Design Generator
MA: weekly (Mon) × 1piece, 7-day auction from ¥30k | MUON: daily × temp°C pieces | MUGEN: hourly × drop# pieces (cycle 1-108)
"""

import os, sys, json, random, sqlite3, requests, base64, hashlib, time, io, struct
from datetime import datetime, date
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont
import numpy as np

os.environ.pop("GOOGLE_API_KEY", None)  # expired key takes precedence otherwise
# Force-override GEMINI_API_KEY from /Users/yuki/.env when present —
# .zshrc has a revoked key that wins via shell-export precedence (see
# feedback_gemini_key_env memory). cron.sh does `set -a && source $ENV_FILE`
# which handles this, but manual interactive runs need the explicit reload.
_env_path = Path("/Users/yuki/.env") if False else None  # placeholder for type
try:
    from pathlib import Path as _P
    _env = _P("/Users/yuki/.env")
    if _env.exists():
        for _ln in _env.read_text().splitlines():
            _ln = _ln.strip()
            if "=" in _ln and not _ln.startswith("#"):
                _k, _v = _ln.split("=", 1)
                if _k.strip() in ("GEMINI_API_KEY", "PRINTFUL_API_KEY",
                                  "MU_ADMIN_TOKEN", "HELIUS_API_KEY",
                                  "CLOUDFLARE_R2_ACCESS_KEY_ID",
                                  "CLOUDFLARE_R2_SECRET_ACCESS_KEY"):
                    os.environ[_k.strip()] = _v.strip().strip('"').strip("'")
except Exception:
    pass

from google import genai
from google.genai import types

# Optional: sold/bid-driven design steerer. Lives in scripts/winner_picker.py.
# Guarded so generate.py keeps running on a fresh checkout without the file.
try:
    from scripts.winner_picker import pick_winners as _pick_winners  # type: ignore
except Exception:
    _pick_winners = None  # type: ignore

GEMINI_API_KEY = os.environ["GEMINI_API_KEY"]
PRINTFUL_KEY   = os.environ["PRINTFUL_API_KEY"]
DB_PATH        = Path(__file__).parent / "products.db"
DESIGNS_DIR    = Path(__file__).parent / "designs"
DESIGNS_DIR.mkdir(exist_ok=True)
GEMINI_MODEL   = "gemini-3-pro-image-preview"
PF_BASE        = "https://api.printful.com"
PF_HDR         = {"Authorization": f"Bearer {PRINTFUL_KEY}", "Content-Type": "application/json"}
STORE_URL      = os.environ.get("MU_STORE_URL", "https://wearmu.com")
ADMIN_TOKEN    = os.environ.get("MU_ADMIN_TOKEN", "mu-admin")

# Printful product IDs
PF_PRODUCT     = 71   # Bella+Canvas 3001 Unisex Tee
PF_VARIANT_BLK = 4017   # Black / M
PF_VARIANT_WHT = 4011   # White / M
PF_VARIANT_BGE = 4014   # Natural / M (closest to beige)
PF_VARIANT_NVY = 4015   # Navy / M (Printful Bella+Canvas 3001 navy variant — verify before use)
PF_VARIANT_HTR = 4019   # Heather Grey / M
PF_VARIANT_RED = 4013   # Red / M
PF_VARIANT_DHR = 4020   # Dark Heather / M
COLOR_MAP = {"BLK": PF_VARIANT_BLK, "WHT": PF_VARIANT_WHT, "BGE": PF_VARIANT_BGE,
             "NVY": PF_VARIANT_NVY, "HTR": PF_VARIANT_HTR, "RED": PF_VARIANT_RED, "DHR": PF_VARIANT_DHR}

# ── Database ─────────────────────────────────────────────
def init_db():
    con = sqlite3.connect(DB_PATH)
    con.execute("""
        CREATE TABLE IF NOT EXISTS products (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            brand        TEXT NOT NULL,
            drop_num     INTEGER NOT NULL,
            name         TEXT NOT NULL,
            design_url   TEXT,
            mockup_url   TEXT,
            price_jpy    INTEGER NOT NULL,
            inventory    INTEGER NOT NULL,
            sold         INTEGER DEFAULT 0,
            created_at   TEXT NOT NULL,
            active       INTEGER DEFAULT 1,
            weather_data TEXT,
            prompt_text  TEXT,
            prompt_hash  TEXT,
            seed_data    TEXT,
            auction_end  TEXT,
            current_bid  INTEGER DEFAULT 0,
            bid_count    INTEGER DEFAULT 0,
            nft_mint     TEXT,
            parent_design TEXT
        )
    """)
    # Additive migrations so the local mirror matches the store schema. Older
    # products.db files predate these columns; ALTER ADD COLUMN errors if the
    # column already exists, so each is best-effort. The local DB is only a
    # convenience mirror — the authoritative insert is POST /api/admin/import.
    for col, decl in (("print_url", "TEXT"), ("color", "TEXT"), ("size", "TEXT")):
        try:
            con.execute(f"ALTER TABLE products ADD COLUMN {col} {decl}")
        except sqlite3.OperationalError:
            pass  # column already present
    con.execute("""
        CREATE TABLE IF NOT EXISTS bids (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            product_id INTEGER NOT NULL,
            amount     INTEGER NOT NULL,
            email      TEXT NOT NULL,
            wallet     TEXT,
            created_at TEXT NOT NULL
        )
    """)
    con.execute("""
        CREATE TABLE IF NOT EXISTS prompt_votes (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            drop_date  TEXT NOT NULL,
            word       TEXT NOT NULL,
            voter_nft  TEXT NOT NULL,
            created_at TEXT NOT NULL
        )
    """)
    con.execute("""
        CREATE TABLE IF NOT EXISTS fragments (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            product_id INTEGER NOT NULL,
            owner_email TEXT NOT NULL,
            burned      INTEGER DEFAULT 0,
            burned_at   TEXT
        )
    """)
    con.commit()
    return con

def next_drop_num(con, brand):
    # Local working DB authority (when present).
    row = con.execute("SELECT MAX(drop_num) FROM products WHERE brand=?", (brand,)).fetchone()
    local_max = row[0] or 0
    # On a stateless runner (GH Actions) the local DB starts empty, which
    # would collide with the production DB. Prefer the higher of:
    #   - local DB MAX
    #   - production /api/admin/next_drop?brand=… (authoritative)
    # Explicit env override (MU_NEXT_DROP_NUM) wins both.
    env_override = os.environ.get("MU_NEXT_DROP_NUM")
    if env_override and env_override.isdigit():
        return int(env_override)
    remote_next = 0
    try:
        r = requests.get(
            f"{STORE_URL}/api/admin/next_drop",
            params={"brand": brand, "token": ADMIN_TOKEN},
            timeout=10,
        )
        if r.ok:
            remote_next = int(r.json().get("next", 0)) or 0
    except Exception as e:
        print(f"  next_drop API unreachable ({e}); falling back to local DB only")
    return max(local_max + 1, remote_next)

def get_last_design(con, brand):
    row = con.execute(
        "SELECT design_url FROM products WHERE brand=? ORDER BY created_at DESC LIMIT 1", (brand,)
    ).fetchone()
    return row[0] if row else None

# ── Weather ───────────────────────────────────────────────
def get_hokkaido_weather():
    try:
        r = requests.get("https://wttr.in/Teshikaga?format=j1", timeout=5)
        d = r.json()["current_condition"][0]
        return {
            "temp_c":    int(d["temp_C"]),
            "humidity":  int(d["humidity"]),
            "wind_kmh":  int(d["windspeedKmph"]),
            "wind_dir":  d["winddir16Point"],
            "condition": d["weatherDesc"][0]["value"],
            "location":  "Teshikaga, Hokkaido",
            "timestamp": datetime.now().isoformat(),
        }
    except:
        return {"temp_c": 10, "humidity": 60, "wind_kmh": 5, "wind_dir": "N",
                "condition": "Unknown", "location": "Teshikaga, Hokkaido",
                "timestamp": datetime.now().isoformat()}

def time_mood():
    h = datetime.now().hour
    moods = [
        "midnight — 深夜の静けさ",      # 0
        "1am — 眠れない誠実さ",          # 1
        "2am — 告白の時間",              # 2
        "3am — 正直な闇",               # 3
        "4am — 夜明け前の冷気",          # 4
        "5am — 最初の音",               # 5
        "6am — 世界が再起動する",         # 6
        "7am — 急ぎ足の朝",             # 7
        "8am — ルーティンという幻想",     # 8
        "9am — 意図する時間",            # 9
        "10am — 加速",                  # 10
        "11am — 最後の澄んだ時間",        # 11
        "noon — 灼熱の静点",             # 12
        "1pm — 光の後の重さ",            # 13
        "2pm — 誰も認めない緩慢な時間",   # 14
        "3pm — 誰も予定しなかった転換点", # 15
        "4pm — 解放が始まる",            # 16
        "5pm — 都市の息継ぎ",            # 17
        "6pm — コンクリートに黄金の光",   # 18
        "7pm — 日々の間のブルーアワー",   # 19
        "8pm — ネオンが太陽に取って代わる",# 20
        "9pm — 第二のエネルギーか降伏か", # 21
        "10pm — 個人的な時間への降下",    # 22
        "11pm — すべてのラストコール",    # 23
    ]
    return moods[h]

# ── Gemini Image Generation ───────────────────────────────
def generate_design(prompt: str) -> bytes:
    client = genai.Client(api_key=GEMINI_API_KEY)
    response = client.models.generate_content(
        model=GEMINI_MODEL,
        contents=[prompt],
        config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"])
    )
    for part in response.candidates[0].content.parts:
        if hasattr(part, "inline_data") and part.inline_data:
            data = part.inline_data.data
            if isinstance(data, str):
                return base64.b64decode(data)
            return data
    raise RuntimeError("Gemini returned no image")

# ── Printful ─────────────────────────────────────────────
def upload_to_imgur(image_bytes: bytes, filename: str = "design.png") -> str:
    """POST image to imgur with 3x exponential backoff on 429/503/connection.

    imgur api.imgur.com has been flapping on 503/429 in 2026-05; that used to
    block 100% of design generation. Caller (upload_design_anywhere) catches
    the final raise and falls through to other upload providers.
    """
    b64 = base64.b64encode(image_bytes).decode()
    last_exc: Exception | None = None
    backoffs = [5, 15, 45]
    for attempt, sleep_s in enumerate(backoffs, start=1):
        try:
            r = requests.post(
                "https://api.imgur.com/3/image",
                headers={"Authorization": "Client-ID 546c25a59c58ad7"},
                json={"image": b64, "type": "base64", "name": filename},
                timeout=30,
            )
            # Retry on 429 / 5xx; fail fast on other 4xx
            if r.status_code in (429,) or 500 <= r.status_code < 600:
                last_exc = requests.HTTPError(
                    f"imgur HTTP {r.status_code}", response=r
                )
                if attempt < len(backoffs):
                    print(f"  imgur {r.status_code}; retry {attempt}/{len(backoffs)} in {sleep_s}s")
                    time.sleep(sleep_s)
                    continue
                r.raise_for_status()  # raises last_exc-equivalent
            r.raise_for_status()
            return r.json()["data"]["link"]
        except (requests.ConnectionError, requests.Timeout) as e:
            last_exc = e
            if attempt < len(backoffs):
                print(f"  imgur conn-err ({type(e).__name__}); retry {attempt}/{len(backoffs)} in {sleep_s}s")
                time.sleep(sleep_s)
                continue
            raise
    if last_exc:
        raise last_exc
    raise RuntimeError("imgur upload: unreachable")


def upload_to_r2_design(image_bytes: bytes, filename: str) -> str:
    """Upload raw design bytes to Cloudflare R2 (wearmu-mockups/designs/<filename>).
    Returns public URL https://mockups.wearmu.com/designs/<filename>.

    Uses wrangler CLI via subprocess with an explicit node PATH so cron's
    minimal PATH still finds node (previous failure: `env: node: No such
    file or directory`). Raises on any failure so the caller can fall
    through to imgur / printful-direct.
    """
    import subprocess, tempfile
    wrangler_bin = os.environ.get("WRANGLER_BIN", "/opt/homebrew/bin/wrangler")
    # Ensure `node` is reachable for wrangler (cron PATH is minimal).
    node_dir = os.environ.get(
        "NODE_BIN_DIR", "/Users/yuki/.nvm/versions/node/v22.22.0/bin"
    )
    env = {**os.environ, "PATH": f"{node_dir}:{os.environ.get('PATH', '')}"}

    suffix = ".png" if filename.lower().endswith(".png") else ".jpg"
    content_type = "image/png" if suffix == ".png" else "image/jpeg"
    with tempfile.NamedTemporaryFile(suffix=suffix, delete=False) as f:
        f.write(image_bytes)
        tmp_path = f.name
    try:
        key = f"designs/{filename}"
        result = subprocess.run(
            [
                wrangler_bin, "r2", "object", "put",
                f"wearmu-mockups/{key}",
                f"--file={tmp_path}",
                "--remote",
                f"--content-type={content_type}",
            ],
            capture_output=True, text=True, timeout=90, env=env,
        )
        if result.returncode != 0:
            raise RuntimeError(
                f"wrangler exit {result.returncode}: {result.stderr[-300:]}"
            )
        return f"https://mockups.wearmu.com/{key}"
    finally:
        try: os.unlink(tmp_path)
        except OSError: pass


def upload_to_printful_direct(image_bytes: bytes, filename: str) -> str:
    """Final-fallback: POST design bytes directly to Printful v1 files API
    via multipart. Printful hosts the file itself and returns a usable URL.
    Used when both R2 and imgur are down.
    """
    files = {"file": (filename, image_bytes, "image/png")}
    # Printful v1 multipart needs Authorization but not Content-Type
    hdr = {"Authorization": f"Bearer {PRINTFUL_KEY}"}
    r = requests.post(
        f"{PF_BASE}/files", headers=hdr, files=files, timeout=60,
    )
    r.raise_for_status()
    data = r.json()
    # v1 response shape: {"code":200,"result":{"url":"..."}}
    return (
        data.get("result", {}).get("url")
        or data.get("data", {}).get("url")
        or ""
    )


def upload_design_anywhere(image_bytes: bytes, filename: str) -> str:
    """Return a public URL for `image_bytes`, trying in order:
      1. Cloudflare R2 (primary, permanent, free)
      2. imgur (with retry+backoff)
      3. Printful v1 /files direct multipart (final safety net)

    The first provider that returns a usable URL wins. SPOF eliminated.
    """
    # 1. R2 primary
    try:
        url = upload_to_r2_design(image_bytes, filename)
        print(f"  uploaded via R2: {url}")
        return url
    except Exception as e:
        print(f"  R2 upload failed ({e}); trying imgur")
    # 2. imgur fallback
    try:
        url = upload_to_imgur(image_bytes, filename)
        print(f"  uploaded via imgur: {url}")
        return url
    except Exception as e:
        print(f"  imgur upload failed ({e}); trying printful direct")
    # 3. Printful direct multipart (last resort)
    url = upload_to_printful_direct(image_bytes, filename)
    print(f"  uploaded via printful direct: {url}")
    return url

def make_transparent_bg(image_bytes: bytes, threshold: int = 35) -> bytes:
    """Auto-detect bg color (sample 4 corners) and key it out.

    Handles dark-bg AND light-bg designs (Gemini emits both depending on prompt).
    Uses a smooth alpha ramp at the design edge so anti-aliased halos stay
    crisp on shirts of any color. Falls back to chroma-distance from the
    sampled corner median for ambiguous mid-tone bgs.

    `threshold` is kept as a no-op kwarg for backward-compat with callers.
    """
    img = Image.open(io.BytesIO(image_bytes)).convert("RGBA")
    arr = np.array(img).astype(np.int16)
    h, w = arr.shape[:2]

    # 4-corner median (more robust than mean against marks that touch the corner)
    patches = np.concatenate([
        arr[0:20, 0:20, :3].reshape(-1, 3),
        arr[0:20, w-20:w, :3].reshape(-1, 3),
        arr[h-20:h, 0:20, :3].reshape(-1, 3),
        arr[h-20:h, w-20:w, :3].reshape(-1, 3),
    ])
    bg_color = np.median(patches, axis=0)
    bg_brightness = float(bg_color.mean())

    rgb = arr[..., :3]
    if bg_brightness > 180:    # white-ish
        dist = np.linalg.norm(rgb - 255, axis=-1)
    elif bg_brightness < 60:   # black-ish
        dist = np.linalg.norm(rgb, axis=-1)
    else:                      # mid-tone
        dist = np.linalg.norm(rgb - bg_color, axis=-1)

    # Smooth ramp: 0..15 → transparent, 15..40 → linear, >40 → opaque
    alpha = np.clip((dist - 15) / 25.0, 0.0, 1.0) * 255
    arr[..., 3] = alpha.astype(np.int16)

    out = Image.fromarray(arr.astype(np.uint8), "RGBA")
    buf = io.BytesIO()
    out.save(buf, format="PNG")
    return buf.getvalue()

def embed_serial_number(image_bytes: bytes, brand: str, drop_num: int, quantity: int) -> bytes:
    """Stamp a small serial number and verification QR onto the bottom edge
    of the T-shirt mockup, *outside* the chest-graphic safe zone.

    Layout (top-down T-shirt photo, ~4:3 canvas):
      - The chest graphic occupies roughly the upper-middle of the shirt
        (centre, ~15–35% of the shirt width, top half of canvas).
      - We anchor QR + serial to the very bottom strip of the canvas
        (below the visible shirt hem, on the surrounding "table" area).
      - No solid-fill backplates — QR is rendered as transparent + dark
        modules only; serial number is drawn directly with a subtle
        shadow for legibility on either a light- or dark-colored table.

    Bias 2026-05-13: previously the serial sat at bottom-RIGHT with a
    semi-opaque black box, which collided with chest graphics that
    extended into the lower-right shirt area. The new layout places
    both elements far below the shirt body and removes the backplate.
    """
    import qrcode as _qrcode
    from qrcode.image.styledpil import StyledPilImage
    from qrcode.image.styles.moduledrawers.pil import SquareModuleDrawer

    img = Image.open(io.BytesIO(image_bytes))
    has_alpha = img.mode == "RGBA"
    rgba = img.convert("RGBA")
    w, h = rgba.size

    overlay = Image.new("RGBA", rgba.size, (0, 0, 0, 0))
    draw    = ImageDraw.Draw(overlay)

    now = datetime.now()
    if brand == "mugen":
        cycle = ((drop_num - 1) % 108) + 1
        line1 = f"MUGEN #{drop_num:04d}"
        line2 = f"{cycle} / 108 · {now.strftime('%Y.%m.%d')}"
    elif brand == "muon":
        line1 = f"MUON {now.strftime('%Y.%m.%d')}"
        line2 = f"1 of {quantity} · {now.strftime('%H:%M')} JST"
    elif brand == "ma":
        iso = now.isocalendar()
        line1 = f"MA {iso.year}.W{iso.week:02d}"
        line2 = f"1 of 1 · {now.strftime('%Y.%m.%d')} · 7-day auction"
    elif brand == "staple":
        line1 = f"STAPLE #{drop_num:04d}"
        line2 = f"1 of {quantity} · {now.strftime('%Y.%m.%d')}"
    else:
        line1 = f"NOUNS × MU #{drop_num:04d}"
        line2 = f"1 of {quantity} · {now.strftime('%Y.%m.%d')}"

    fsize = max(14, h // 80)
    for font_path in ["/System/Library/Fonts/HelveticaNeue.ttc",
                      "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"]:
        try:
            font  = ImageFont.truetype(font_path, fsize)
            font2 = ImageFont.truetype(font_path, max(10, fsize - 4))
            break
        except Exception:
            font = font2 = ImageFont.load_default()

    pad = int(w * 0.025)

    # Bottom strip lives in the lowest 10% of the canvas — the
    # Gemini prompt places the shirt within the upper ~85%, so this
    # area is the "table" / margin and never overlaps the chest design.
    bottom_strip_top = int(h * 0.92)

    # ── QR (bottom-left of canvas, in the safe bottom strip) ──
    verify_url = f"https://wearmu.com/v/{brand}/{drop_num:04d}"
    qr = _qrcode.QRCode(
        version=3,
        error_correction=_qrcode.constants.ERROR_CORRECT_H,
        box_size=max(3, h // 480),  # slightly smaller to fit safe strip
        border=2,
    )
    qr.add_data(verify_url)
    qr.make(fit=True)
    qr_img = qr.make_image(
        image_factory=StyledPilImage,
        module_drawer=SquareModuleDrawer(),
        # Fully transparent background; only the dark modules carry pixels.
        # When printed/composited the shirt or table colour shows through.
        back_color=(0, 0, 0, 0),
        # Mid-grey modules so the QR reads on both light and dark surfaces.
        fill_color=(180, 180, 180),
    ).convert("RGBA")
    qw, qh = qr_img.size
    qr_x = pad
    qr_y = h - qh - pad
    # Ensure QR is fully inside the bottom safe strip; if it would intrude
    # into the shirt body, shrink it.
    if qr_y < bottom_strip_top:
        scale = max(0.6, (h - bottom_strip_top - pad) / qh)
        new_qh = max(48, int(qh * scale))
        new_qw = max(48, int(qw * scale))
        qr_img = qr_img.resize((new_qw, new_qh), Image.LANCZOS)
        qw, qh = new_qw, new_qh
        qr_y = h - qh - pad
    overlay.paste(qr_img, (qr_x, qr_y), qr_img)

    # ── Serial text (bottom-right of canvas, same safe strip) ──
    bb1 = draw.textbbox((0, 0), line1, font=font)
    bb2 = draw.textbbox((0, 0), line2, font=font2)
    tw  = max(bb1[2] - bb1[0], bb2[2] - bb2[0])
    th  = (bb1[3] - bb1[1]) + 4 + (bb2[3] - bb2[1])
    sx = w - tw - pad
    sy = h - th - pad
    # No solid backplate — use a subtle dark shadow + light fill so the
    # text reads on either substrate colour without painting a rectangle.
    for dx, dy in ((1, 1), (-1, 1), (1, -1), (-1, -1)):
        draw.text((sx + dx, sy + dy), line1, font=font, fill=(0, 0, 0, 110))
        draw.text((sx + dx, sy + bb1[3] - bb1[1] + 4 + dy),
                  line2, font=font2, fill=(0, 0, 0, 90))
    draw.text((sx, sy),                       line1, font=font,  fill=(235, 235, 235, 230))
    draw.text((sx, sy + bb1[3] - bb1[1] + 4), line2, font=font2, fill=(195, 195, 195, 195))

    out = Image.alpha_composite(rgba, overlay)
    buf = io.BytesIO()
    if has_alpha:
        out.save(buf, "PNG")
    else:
        out.convert("RGB").save(buf, "PNG")
    return buf.getvalue()


def _wm_bits(brand: str, drop_num: int, prompt_hash: str) -> list[int]:
    """Encode 32 bits: brand(2) + drop_num(14) + hash_check(16)."""
    brand_bits = {"mugen": [0,0], "muon": [0,1], "ma": [1,0], "nouns": [1,1]}.get(brand, [0,0])
    drop_bits  = [(drop_num >> i) & 1 for i in range(13, -1, -1)]  # 14-bit big-endian
    h = int(prompt_hash[:4], 16) if len(prompt_hash) >= 4 else 0
    hash_bits  = [(h >> i) & 1 for i in range(15, -1, -1)]  # 16-bit check
    return brand_bits + drop_bits + hash_bits

def _bits_to_info(bits: list[int]) -> dict:
    brand_map = {(0,0): "mugen", (0,1): "muon", (1,0): "ma", (1,1): "nouns"}
    brand    = brand_map.get(tuple(bits[:2]), "unknown")
    drop_num = sum(b << (13-i) for i, b in enumerate(bits[2:16]))
    h_check  = sum(b << (15-i) for i, b in enumerate(bits[16:32]))
    return {"brand": brand, "drop_num": drop_num, "hash_check": hex(h_check)}

def embed_watermark(image_bytes: bytes, brand: str, drop_num: int, prompt_hash: str) -> bytes:
    """Embed RivaGAN invisible watermark (32-bit, JPEG+noise robust) + dwtDctSvd fallback."""
    try:
        from imwatermark import WatermarkEncoder
    except ImportError:
        return image_bytes

    img     = Image.open(io.BytesIO(image_bytes)).convert("RGB")
    img_np  = np.array(img)
    bits    = _wm_bits(brand, drop_num, prompt_hash)

    enc = WatermarkEncoder()
    try:
        enc.loadModel()
        enc.set_watermark("bits", bits)
        encoded = enc.encode(img_np, "rivaGan")
    except Exception:
        # fallback to dwtDctSvd (more bits, but less robust to print-scan)
        brand_code = {"mugen": b"G", "muon": b"O", "ma": b"A", "nouns": b"N"}.get(brand, b"?")
        hash_bytes = bytes.fromhex(prompt_hash[:6]) if len(prompt_hash) >= 6 else b"\x00\x00\x00"
        payload = b"MU" + brand_code + struct.pack(">H", drop_num % 65535) + hash_bytes
        enc2 = WatermarkEncoder()
        enc2.set_watermark("bytes", payload)
        encoded = enc2.encode(img_np, "dwtDctSvd")

    buf = io.BytesIO()
    Image.fromarray(encoded).save(buf, "PNG")
    return buf.getvalue()


def decode_watermark(image_bytes: bytes) -> dict | None:
    """Decode MU watermark. Tries RivaGAN first (JPEG/photo robust), then dwtDctSvd."""
    try:
        from imwatermark import WatermarkDecoder
    except ImportError:
        return None

    img_np = np.array(Image.open(io.BytesIO(image_bytes)).convert("RGB"))

    # Try RivaGAN (32-bit)
    try:
        dec = WatermarkDecoder("bits", 32)
        dec.loadModel()
        bits = dec.decode(img_np, "rivaGan")
        info = _bits_to_info(bits)
        if info["brand"] != "unknown" and 0 < info["drop_num"] < 10000:
            info["method"] = "rivaGan"
            return info
    except Exception:
        pass

    # Fallback: dwtDctSvd (8-byte)
    dec2 = WatermarkDecoder("bytes", 64)
    payload = dec2.decode(img_np, "dwtDctSvd")
    if payload[:2] == b"MU":
        brand_map = {b"G": "mugen", b"O": "muon", b"A": "ma", b"N": "nouns"}
        return {
            "brand":      brand_map.get(payload[2:3], "unknown"),
            "drop_num":   struct.unpack(">H", payload[3:5])[0],
            "hash_check": payload[5:8].hex(),
            "method":     "dwtDctSvd",
        }
    return None


def upload_to_printful(image_bytes: bytes, filename: str, transparent: bool = False) -> str:
    """Upload design via multi-provider chain (R2 → imgur → Printful direct)
    and register with Printful v2 files API. SPOF on imgur eliminated 2026-05.
    """
    if transparent:
        image_bytes = make_transparent_bg(image_bytes)
    public_url = upload_design_anywhere(image_bytes, filename)
    # Register with Printful v2 (non-fatal if it fails)
    try:
        r = requests.post(f"{PF_BASE}/v2/files", headers=PF_HDR,
                          json={"type": "front", "url": public_url}, timeout=15)
        if r.ok:
            return r.json().get("data", {}).get("url", public_url)
    except Exception:
        pass
    return public_url

def get_mockup(product_id: int, variant_id: int, file_url: str) -> str | None:
    r = requests.post(f"{PF_BASE}/mockup-generator/create-task/{product_id}", headers=PF_HDR, json={
        "variant_ids": [variant_id],
        "format": "jpg",
        "files": [{"placement": "front", "image_url": file_url, "position": {
            "area_width": 1800, "area_height": 2400,
            "width": 1600, "height": 2000, "top": 200, "left": 100,
        }}]
    })
    if not r.ok:
        return None
    task_key = r.json()["result"]["task_key"]
    for _ in range(40):  # 40 × 5s = 200s
        time.sleep(5)
        t = requests.get(f"{PF_BASE}/mockup-generator/task?task_key={task_key}", headers=PF_HDR)
        data = t.json()["result"]
        if data["status"] == "completed":
            return data["mockups"][0]["mockup_url"]
        if data["status"] == "failed":
            return None
    return None

# ── Solana NFT (certificate) ─────────────────────────────
def mint_nft_certificate(product_id: int, metadata: dict) -> str | None:
    """Mint standard Metaplex NFT on Solana mainnet (no Helius needed). Returns mint address."""
    import struct
    try:
        import base58 as _b58
        from solders.keypair import Keypair as SolKP
        from solders.pubkey import Pubkey as SolPK
        from solders.instruction import Instruction as SolIx, AccountMeta as SolAM
        from solders.transaction import Transaction as SolTx
        from solders.system_program import create_account, CreateAccountParams, ID as SYS
        from solana.rpc.api import Client
        from solana.rpc.types import TxOpts
        from solana.rpc.commitment import Confirmed
    except ImportError as e:
        print(f"  nft: solders/solana import error: {e}")
        return None

    SOLANA_RPC = os.environ.get("SOLANA_RPC", "https://api.mainnet-beta.solana.com")
    TOKEN_PROG = SolPK.from_string("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
    META_PROG  = SolPK.from_string("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s")
    ATA_PROG   = SolPK.from_string("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJe1bRS")
    RENT_SYS   = SolPK.from_string("SysvarRent111111111111111111111111111111111")

    keypair_b58 = os.environ.get("MU_WALLET_KEYPAIR")
    if not keypair_b58:
        new_kp = SolKP()
        addr   = str(new_kp.pubkey())
        encoded = _b58.b58encode(bytes(new_kp)).decode()
        print(f"  nft: no wallet — add to ~/.env then fund with 0.05 SOL:")
        print(f"    MU_WALLET_KEYPAIR={encoded}")
        print(f"    MU_TREASURY_WALLET={addr}")
        return None

    try:
        wallet = SolKP.from_bytes(_b58.b58decode(keypair_b58))
    except Exception as e:
        print(f"  nft: bad keypair: {e}")
        return None

    client  = Client(SOLANA_RPC)
    balance = client.get_balance(wallet.pubkey()).value
    if balance < 20_000_000:
        print(f"  nft: wallet {wallet.pubkey()} needs SOL (have {balance/1e9:.4f}, need 0.02+)")
        return None

    mint_kp = SolKP()
    mint    = mint_kp.pubkey()
    owner   = wallet.pubkey()

    meta_pda, _    = SolPK.find_program_address(
        [b"metadata", bytes(META_PROG), bytes(mint)], META_PROG)
    edition_pda, _ = SolPK.find_program_address(
        [b"metadata", bytes(META_PROG), bytes(mint), b"edition"], META_PROG)
    ata, _         = SolPK.find_program_address(
        [bytes(owner), bytes(TOKEN_PROG), bytes(mint)], ATA_PROG)

    mint_rent = client.get_minimum_balance_for_rent_exemption(82).value
    blockhash  = client.get_latest_blockhash().value.blockhash

    # NFT metadata URI served by our store
    attrs     = metadata.get("attributes", [])
    brand_val = next((a["value"].lower() for a in attrs if a.get("trait_type") == "Brand"), "mu")
    drop_val  = next((a["value"] for a in attrs if a.get("trait_type") == "Drop"), product_id)
    uri = f"https://wearmu.com/api/nft/{brand_val}/{drop_val}"

    def bs(s: str) -> bytes:
        b = s.encode(); return struct.pack('<I', len(b)) + b

    # 1. Create mint account (system program)
    ix_create = create_account(CreateAccountParams(
        from_pubkey=owner, to_pubkey=mint, lamports=mint_rent, space=82, owner=TOKEN_PROG))

    # 2. InitializeMint (tag=0, decimals=0, authority, no freeze)
    ix_init = SolIx(TOKEN_PROG,
        bytes([0, 0]) + bytes(owner) + struct.pack('<I', 0),   # COption None
        [SolAM(mint, True, True), SolAM(RENT_SYS, False, False)])

    # 3. Create ATA (idempotent, tag=1)
    ix_ata = SolIx(ATA_PROG, bytes([1]),
        [SolAM(owner, True, True), SolAM(ata, True, False),
         SolAM(owner, False, False), SolAM(mint, False, False),
         SolAM(SYS, False, False), SolAM(TOKEN_PROG, False, False)])

    # 4. MintTo 1 (tag=7)
    ix_mint_to = SolIx(TOKEN_PROG,
        bytes([7]) + struct.pack('<Q', 1),
        [SolAM(mint, False, True), SolAM(ata, False, True), SolAM(owner, False, True)])

    # 5. CreateMetadataAccountV3 (disc=33)
    name_str = metadata["name"][:32]
    ix_meta_data = (
        bytes([33])
        + bs(name_str) + bs("MU") + bs(uri)
        + struct.pack('<H', 500)          # seller_fee_bps
        + b'\x01' + struct.pack('<I', 1) # creators: Some([1])
        + bytes(owner) + b'\x00\x64'     # pubkey, unverified, 100% share
        + b'\x00'                         # collection: None
        + b'\x00'                         # uses: None
        + b'\x01'                         # is_mutable: true
        + b'\x00'                         # collection_details: None
    )
    ix_meta = SolIx(META_PROG, ix_meta_data,
        [SolAM(meta_pda, True, False), SolAM(mint, False, False),
         SolAM(owner, False, True), SolAM(owner, True, True),
         SolAM(owner, False, False), SolAM(SYS, False, False),
         SolAM(RENT_SYS, False, False)])

    # 6. CreateMasterEditionV3 (disc=17, max_supply=Some(0) → 1/1)
    ix_edition = SolIx(META_PROG,
        bytes([17]) + b'\x01' + struct.pack('<Q', 0),
        [SolAM(edition_pda, True, False), SolAM(mint, False, True),
         SolAM(owner, False, True), SolAM(owner, False, True),
         SolAM(owner, True, True), SolAM(meta_pda, False, True),
         SolAM(TOKEN_PROG, False, False), SolAM(SYS, False, False),
         SolAM(RENT_SYS, False, False)])

    tx = SolTx.new_signed_with_payer(
        [ix_create, ix_init, ix_ata, ix_mint_to, ix_meta, ix_edition],
        owner, [wallet, mint_kp], blockhash)

    try:
        resp = client.send_transaction(tx, opts=TxOpts(skip_preflight=False, preflight_commitment=Confirmed))
        sig  = str(resp.value)
        client.confirm_transaction(resp.value, Confirmed)
        mint_addr = str(mint)
        print(f"  nft minted: {mint_addr}")
        print(f"  tx: https://solscan.io/tx/{sig}")
        return mint_addr
    except Exception as e:
        print(f"  nft mint failed: {e}")
        return None

# ── Brand Prompts ─────────────────────────────────────────

def prompt_ma(weather: dict, last_design_url: str | None) -> tuple[str, str]:
    now = datetime.now()
    themes = [
        "the void before sound begins",
        "an empty room someone just left",
        "the space between two people not touching",
        "a breath held underwater",
        "fog dissolving over still water",
        "a door half-open onto nothing",
        "the pause between two heartbeats",
        "a word decided against",
        "the gap in a broken circle",
        "snow falling onto snow",
        "the space a shadow leaves",
        "silence after a bell",
    ]
    theme = themes[(now.month - 1) % len(themes)]
    mutation_note = ""
    if last_design_url:
        mutation_note = f"""
IMPORTANT — Generational DNA:
Previous month's design was at: {last_design_url}
Your design must carry ONE visual gene from it — a similar line weight, a similar void ratio,
or a similar spatial tension — but transformed. Evolution, not repetition.
"""
    name = f"間 {now.strftime('%Y.%m')}"
    prompt = f"""
FLAT PRINT ARTWORK. Black ink on pure white background. No clothing. No t-shirt. No garment shape. No model. No product photo. Just the graphic artwork itself — as if it will be screen-printed.

Brand: MA (間) — ultra-premium Japanese fashion. MA means negative space.
Month: {now.strftime('%B %Y')} / Theme: "{theme}"
Today: {weather['temp_c']}°C, {weather['condition']}, wind {weather['wind_dir']}
{mutation_note}

Design rules:
- ONE element only. Pure black ink on pure white background.
- Japanese sumi-e abstraction OR strict geometric reduction.
- Element occupies 20–30% of the canvas. Vast white void surrounds it.
- No text. No logo. No border. No t-shirt outline. No clothing silhouette.
- OUTPUT: flat artwork only, 2400×3200px, black on white, ready to screen-print.
"""
    return name, prompt


def prompt_muon(weather: dict, drop_num: int) -> tuple[str, str, int, bool]:
    today = date.today()
    temp = weather["temp_c"]

    # ICE Edition: temperature at or below 0°C
    if temp <= 0:
        is_ice = True
        quantity = max(1, min(3, abs(temp))) if temp < 0 else 1
        name = f"MUON ICE {today.strftime('%Y.%m.%d')}"
        prompt = """FLAT PRINT ARTWORK. ULTRA-RARE ICE EDITION — temperature hit 0°C or below. Pure white artwork on jet black background. The design must feel frozen, crystalline, or glacial — not metaphorical but literally cold: frost fractals, ice crystal geometry, frozen breath patterns, or permafrost cracks rendered as graphic art. Stark white on black. No clothing. No t-shirt. Flat 2D graphic, 2400×3200px."""
        return name, prompt, quantity, is_ice

    is_ice = False
    quantity = max(1, abs(temp)) if temp != 0 else 1

    concepts = [
        "An audio waveform that flatlines mid-graph — the exact moment sound becomes silence",
        "A mobile signal display with all bars absent — perfect no-reception",
        "A spectrogram showing only the noise floor — the frequency of nothing",
        "Concentric circles dissolving before reaching the canvas edge",
        "A single horizontal line, perfectly centered, 1px thick. Nothing else.",
        "Binary string: 00000000 — eight zeros. Silence encoded.",
        "A vinyl record's inner groove spiral — the locked groove, infinite silence",
        "Oscilloscope flatline with one micro-disturbance, then return to zero",
        "A barcode where every bar is identical — unreadable, meaningless, silent",
        "A perfect circle with a hairline fracture that does not reach the edge",
    ]
    concept = concepts[today.day % len(concepts)]
    name = f"MUON {today.strftime('%Y.%m.%d')}"
    prompt = f"""
FLAT PRINT ARTWORK. White graphic elements on pure black (#000000) background. THIS IS PURELY A 2D GRAPHIC — NOT A PHOTO OF A T-SHIRT. No t-shirt. No clothing. No garment silhouette. No fabric. No model. No product photo. Flat graphic only, like a poster or vinyl record sleeve.

Brand: MUON (無音) — silence recorded.
Date: {today.isoformat()} / {temp}°C, {weather['humidity']}% humidity, {weather['condition']}
Quantity today: {quantity} pieces
Design concept: {concept}

Execution:
- Pure 2D graphic composition. Imagine a poster, not a photograph of clothing.
- White marks/lines/numbers on solid black rectangle filling the entire canvas.
- Clinical and minimal — documentary, not decorative.
- Composition centered, compact, fits within a 12cm area.
- Tiny text: date {today.strftime('%Y.%m.%d')} and {temp}°C rendered as data annotation.
- ABSOLUTELY NO T-SHIRT SHAPE OR CLOTHING FORM. If you draw a garment you have failed.
- OUTPUT: 2400×3200px flat digital artwork, pure white-on-black graphic.
"""
    return name, prompt, quantity, is_ice


def prompt_nouns(weather: dict, drop_num: int, track: str) -> tuple[str, str, int]:
    """NOUNS collab drops — three tracks sharing the nouns brand."""
    today = date.today()
    temp = weather["temp_c"]

    if track == "mugen":
        # Weekly streetwear — ⌐◨-◨ as structural input
        name = f"MUGEN × NOUNS #{drop_num:04d}"
        quantity = drop_num % 108 or 108
        price = 9800
        prompt = f"""
FLAT PRINT ARTWORK. Bold streetwear graphic on solid background. THIS IS A 2D POSTER GRAPHIC — NOT A PHOTO OF CLOTHING. No t-shirt shape. No clothing. No garment silhouette. No model. Flat graphic only.

Brand: MUGEN × NOUNS — weekly AI streetwear collab with Nouns DAO.
Date: {today.isoformat()} / {temp}°C, {weather['condition']}

Design concept: The Nouns glasses (⌐◨-◨) are a pixel-art icon — two square frames, one with a red lens and one missing. Use this GEOMETRIC STRUCTURE as the foundation of the composition:
- Two squares / rectangles with strict asymmetry (one filled, one outline or absent)
- Blocky pixel-grid aesthetic — hard edges, no curves, no organic forms
- Black on white OR white on black solid background
- Add bold text: "MUGEN × NOUNS" and "#{drop_num}" in compact uppercase block letters
- The overall mood: precision, digital permanence, one-of-a-kind

Execution:
- Pure 2D graphic. Pixel-grid geometry dominates.
- The two-square motif (⬛⬜ asymmetry) must be visible in the final composition.
- High contrast, flat color only. No gradients, no shadows, no 3D.
- OUTPUT: 2400×3200px flat digital artwork, solid background, screen-print ready.
"""
    elif track == "muon":
        quantity = max(1, abs(temp)) if temp != 0 else 1
        name = f"MUON × NOUNS {today.strftime('%Y.%m.%d')}"
        price = 15000
        prompt = f"""
FLAT PRINT ARTWORK. White graphic elements on pure black (#000000) background. THIS IS A 2D GRAPHIC — NOT A PHOTO OF A T-SHIRT. No clothing. No garment silhouette. No model. Flat graphic only, like a vinyl record sleeve.

Brand: MUON × NOUNS — daily silence collab with Nouns DAO. {quantity} pieces only (= today's temperature).
Date: {today.isoformat()} / {temp}°C, {weather['humidity']}% humidity, {weather['condition']}

Design concept: Combine MUON's silence aesthetic with Nouns' pixel geometry:
- Two squares side by side — one white, one a thin outline only — suspended in black void
- Around them: the data annotation "{today.strftime('%Y.%m.%d')} / {temp}°C / {quantity} PCS" in tiny monospace
- Minimal. Clinical. Like a readout from a machine that monitors silence.
- The two-square motif (⌐◨-◨ structure) must anchor the composition.

Execution:
- White marks on solid black. No clothing shape. Pure 2D data graphic.
- Strict geometry. Hard pixel edges. No curves. No organic forms.
- Compact centered composition.
- OUTPUT: 2400×3200px flat digital artwork, white on black, screen-print ready.
"""
    else:  # ma
        quantity = 1
        # 2026-05-11: MA cadence monthly → weekly, start price ¥120k → ¥30k.
        price = 30000
        iso = today.isocalendar()
        name = f"間 MA × NOUNS {iso.year}.W{iso.week:02d}"
        prompt = f"""
FLAT PRINT ARTWORK. Single element. Black ink on pure white background. THIS IS A GRAPHIC — NOT A PHOTO OF A GARMENT. No t-shirt. No clothing shape. No model. Flat artwork only.

Brand: MA × NOUNS — weekly 7-day auction collab with Nouns DAO. 1 piece only. Highest bid wins.
Week: {iso.year} W{iso.week:02d} / {temp}°C, {weather['condition']}

Design concept: MA (間) is Japanese negative space. Nouns is pixel geometry. Fuse them:
- Two squares (⬛⬜) rendered in sumi-e ink brush style — one filled with a single deliberate brushstroke, one left as a ghost outline
- Vast white void surrounds them — MA principle: the emptiness IS the design
- No text. No border. No additional elements.
- The pixel-square structure made organic by ink — geometry surrendering to wabi-sabi.

Execution:
- Pure black ink on white. The two-square motif recognizable but not mechanical.
- Single brushstroke fills one square; the adjacent square is implied, incomplete.
- Element occupies 20–30% of canvas. White space dominates.
- OUTPUT: 2400×3200px flat digital artwork, black on white, museum-quality print.
"""
    return name, prompt, quantity, price


# ── STAPLE: timeless single-character concepts on a rotating cadence ────────
# Brand = "staple". Daily cron picks the next concept by drop_num mod len.
# Higher-value concepts (marked HIGH) ship at ¥6,800; the rest at ¥5,400.
# All are bilingual: big kanji + tiny romaji + English meaning + date.
#
# Curation: these are concepts proven to sell in minimalist Japanese apparel
# (一文字 kanji tees, philosophical brands like Visvim / Kapital / Beams).
# The list is deliberately curated, NOT randomly generated — staple = bestseller.
STAPLE_CONCEPTS: list[dict] = [
    {"k": "無",   "r": "mu",       "en": "nothing / void",      "tier": "HIGH"},
    {"k": "間",   "r": "ma",       "en": "gap / interval",      "tier": "HIGH"},
    {"k": "静",   "r": "shizuka",  "en": "silence / quiet",     "tier": "HIGH"},
    {"k": "道",   "r": "michi",    "en": "the way / path",      "tier": "HIGH"},
    {"k": "風",   "r": "kaze",     "en": "wind",                "tier": "STD"},
    {"k": "月",   "r": "tsuki",    "en": "moon",                "tier": "STD"},
    {"k": "雨",   "r": "ame",      "en": "rain",                "tier": "STD"},
    {"k": "山",   "r": "yama",     "en": "mountain",            "tier": "STD"},
    {"k": "海",   "r": "umi",      "en": "sea / ocean",         "tier": "STD"},
    {"k": "森",   "r": "mori",     "en": "forest",              "tier": "STD"},
    {"k": "光",   "r": "hikari",   "en": "light",               "tier": "STD"},
    {"k": "影",   "r": "kage",     "en": "shadow",              "tier": "STD"},
    {"k": "音",   "r": "oto",      "en": "sound",               "tier": "STD"},
    {"k": "線",   "r": "sen",      "en": "line",                "tier": "STD"},
    {"k": "点",   "r": "ten",      "en": "dot / point",         "tier": "STD"},
    {"k": "円",   "r": "en",       "en": "circle",              "tier": "STD"},
    {"k": "火",   "r": "hi",       "en": "fire",                "tier": "STD"},
    {"k": "水",   "r": "mizu",     "en": "water",               "tier": "STD"},
    {"k": "雪",   "r": "yuki",     "en": "snow",                "tier": "STD"},
    {"k": "空",   "r": "sora",     "en": "sky / empty",         "tier": "STD"},
    {"k": "侘",   "r": "wabi",     "en": "subdued beauty",      "tier": "HIGH"},
    {"k": "寂",   "r": "sabi",     "en": "patina of time",      "tier": "HIGH"},
    {"k": "禅",   "r": "zen",      "en": "meditation",          "tier": "HIGH"},
    {"k": "0",    "r": "zero",     "en": "the zero",            "tier": "STD"},
    {"k": "1",    "r": "ichi",     "en": "one",                 "tier": "STD"},
    {"k": "7",    "r": "shichi",   "en": "seven",               "tier": "STD"},
    {"k": "47",   "r": "yon-juu-nana", "en": "47 prefectures",  "tier": "STD"},
    {"k": "108",  "r": "hyaku-hachi", "en": "Buddhist passions","tier": "HIGH"},
    {"k": "∞",    "r": "mugen",    "en": "infinity",            "tier": "HIGH"},
    {"k": "今",   "r": "ima",      "en": "now",                 "tier": "STD"},
]


def prompt_staple(weather: dict, drop_num: int) -> tuple[str, str, int, int]:
    """Pick concept by drop_num mod list-length. Deterministic so re-runs
    of the same drop produce identical art (within Gemini stochasticity)."""
    idx = (drop_num - 1) % len(STAPLE_CONCEPTS)
    concept = STAPLE_CONCEPTS[idx]
    today = date.today()
    quantity = 47  # 47 都道府県 — philosophical constant
    price = 6800 if concept["tier"] == "HIGH" else 5400
    name = f"STAPLE — 「{concept['k']}」{concept['r']} #{drop_num:04d}"

    # Two visual treatments rotate by drop parity for variety:
    treatment = "black-on-cream"  # the default
    if drop_num % 2 == 0:
        treatment = "white-on-charcoal"

    if treatment == "black-on-cream":
        bg = "off-white / cream / unbleached cotton color (#F5F0E6)"
        ink = "deep black (#0A0A0A) ink, brushwork allowed"
        text_color = "deep black"
    else:
        bg = "deep charcoal (#1A1A1A)"
        ink = "warm off-white (#F0EDE3) ink, brushwork allowed"
        text_color = "warm off-white"

    prompt = f"""
FLAT PRINT ARTWORK — high-fidelity museum-quality minimalist typography.
THIS IS A 2D GRAPHIC, not a photo of a t-shirt. No clothing shape, no model, no fabric in the output. Solid background fills the entire canvas.

Brand: STAPLE — MU's daily timeless edition. Drop #{drop_num}.
Date stamp: {today.isoformat()}.
Concept: 「{concept['k']}」 — {concept['r']} — {concept['en']}.

Background: {bg} (solid, fills entire canvas, no texture except very faint cotton-grain implied).
Ink: {ink}.

Composition (strict):
  1. CENTER (occupies 55-65% of canvas height):
     A single character "{concept['k']}" — rendered in DEEP, HEAVY, contemplative
     brushstroke style (think 書道 by a master calligrapher, 1 single fluid
     stroke or composed of 2-3 deliberate strokes). The character must feel
     ALIVE — slight asymmetry, ink-bleed where natural, dry-brush texture
     at the stroke end. NOT a digital font. NOT computer-perfect.

  2. ABOVE THE CHARACTER (very small {text_color} sans-serif, ~10mm equivalent height):
     {concept['r'].upper()}

  3. BELOW THE CHARACTER (very small {text_color} sans-serif, ~10mm equivalent):
     {concept['en']}

  4. BOTTOM-RIGHT CORNER (very small {text_color} monospace, ~6mm equivalent):
     {today.isoformat()} · STAPLE #{drop_num:04d}

Anti-requirements (do NOT include):
- No watermarks, no logos other than the kanji.
- No additional symbols, no borders, no frames, no decorations.
- No gradients, no shadows, no 3D effect.
- No multiple colors — strictly the 2 colors specified above.

Output: 2400×2400 flat artwork. Calligraphy must read as "made by a human
master, captured by a machine" — quiet, confident, museum-grade.
"""
    return name, prompt, quantity, price


# ── NEWS: daily "号外" tee — abstract theme of the day, sold on MUGEN ────────
# Counts down to MU FESTIVAL HAWAII (2026-10-29). The shirt is styled like a
# broadsheet "号外" (extra edition) but carries NO real news headline — only an
# abstract single word capturing the day's mood. This keeps the line free of
# news-org rights / misinformation / sensitive-reporting risk while still
# reading as a "today's news" tee. Theme is picked by day-of-year so it shifts
# daily and re-runs of the same day are deterministic.
NEWS_THEMES: list[dict] = [
    {"w": "兆",   "r": "kizashi",  "en": "a sign of what comes"},
    {"w": "騒",   "r": "sawagi",   "en": "the noise of the world"},
    {"w": "静",   "r": "sei",      "en": "stillness underneath"},
    {"w": "流",   "r": "nagare",   "en": "the current"},
    {"w": "報",   "r": "hou",      "en": "word that arrives"},
    {"w": "速",   "r": "soku",     "en": "breaking, fast"},
    {"w": "渦",   "r": "uzu",      "en": "the vortex"},
    {"w": "境",   "r": "sakai",    "en": "the borderline"},
    {"w": "灯",   "r": "tomoshibi","en": "a light kept on"},
    {"w": "潮",   "r": "shio",     "en": "the turning tide"},
    {"w": "響",   "r": "hibiki",   "en": "what echoes"},
    {"w": "刻",   "r": "koku",     "en": "the hour, marked"},
    {"w": "波",   "r": "nami",     "en": "the wave"},
    {"w": "報せ", "r": "shirase",  "en": "the message"},
    {"w": "今",   "r": "ima",      "en": "now, only now"},
    {"w": "声",   "r": "koe",      "en": "a voice rising"},
    {"w": "変",   "r": "hen",      "en": "the change"},
    {"w": "縁",   "r": "en",       "en": "the connection"},
    {"w": "際",   "r": "kiwa",     "en": "the edge"},
    {"w": "灯火", "r": "touka",    "en": "lamplight"},
    {"w": "風",   "r": "kaze",     "en": "the wind shifts"},
    {"w": "兆し", "r": "kizashi",  "en": "omen of the day"},
    {"w": "鼓動", "r": "kodou",    "en": "a heartbeat"},
    {"w": "余白", "r": "yohaku",   "en": "the white space"},
    {"w": "閃",   "r": "sen",      "en": "the flash"},
    {"w": "潜",   "r": "sen",      "en": "what's beneath"},
    {"w": "巡",   "r": "meguri",   "en": "the cycle returns"},
    {"w": "音",   "r": "oto",      "en": "the sound of today"},
    {"w": "明",   "r": "akari",    "en": "first light"},
    {"w": "無",   "r": "mu",       "en": "and then, nothing"},
]


def prompt_news(weather: dict, drop_num: int) -> tuple[str, str, int, int]:
    """Daily 号外 tee. Abstract theme by day-of-year (deterministic per day).
    Sold on MUGEN as a draft; Yuki approves at /admin/news before it goes
    live. quantity=29 / price ¥4,800 are editorial defaults (set by us, not a
    quoted external fact) — tune in /admin/news before approving if needed."""
    today = date.today()
    idx = today.timetuple().tm_yday % len(NEWS_THEMES)
    t = NEWS_THEMES[idx]
    quantity = 29          # nod to the 10/29 festival date
    price = 4800           # standard MU tee tier
    # Days remaining until MU FESTIVAL HAWAII (2026-10-29), floored at 0.
    fest = date(2026, 10, 29)
    days_left = max(0, (fest - today).days)
    name = f"MUGEN 号外 — 「{t['w']}」{t['r']} · {today.isoformat()} #{drop_num:04d}"

    prompt = f"""
FLAT PRINT ARTWORK — high-fidelity minimalist broadsheet ("号外" / newspaper EXTRA) typography.
THIS IS A 2D GRAPHIC, not a photo of a t-shirt. No clothing shape, no model, no fabric. Solid background fills the entire canvas.

Brand: MU — daily "号外" (extra edition) tee. This is an ABSTRACT mood piece; it must contain NO real news, NO headlines, NO real people, NO brand names, NO flags, NO logos. Only the abstract word below.

Background: off-white / newsprint cream (#F2EFE6), solid, very faint paper grain only.
Ink: deep black (#0A0A0A), with one single accent in MU red (#C8362C) allowed for the masthead rule line only.

Composition (strict, like a stripped-down newspaper front page):
  1. TOP: the masthead word "号外" in a heavy condensed serif/gothic, small-to-medium, with a thin red horizontal rule directly beneath it spanning the width.
  2. CENTER (occupies 50-60% of canvas height): a SINGLE character "{t['w']}" rendered in heavy contemplative 書道 brushwork — alive, slight asymmetry, dry-brush at the stroke ends. NOT a digital font.
  3. Just below the big character, very small sans-serif: {t['r'].upper()} — {t['en']}.
  4. BOTTOM, very small monospace, like a dateline: "{today.isoformat()} · HAWAII 10.29 · あと{days_left}日 · MUGEN #{drop_num:04d}".

Anti-requirements (do NOT include):
- No real or fake news headlines, no paragraphs of body text, no photos.
- No additional symbols, no borders/frames beyond the single red masthead rule.
- No gradients, no shadows, no 3D. Strictly the 2 colors above (black + one red rule).

Output: 2400×2400 flat artwork. Must read as "a quiet daily broadsheet reduced to one word" — museum-grade, confident, calm.
"""
    return name, prompt, quantity, price


def prompt_mugen(weather: dict, drop_num: int) -> tuple[str, str, int, int]:
    now = datetime.now()
    cycle_num = ((drop_num - 1) % 108) + 1  # 1-108 cycle
    quantity = cycle_num
    mood = time_mood()

    # MUGEN #108 — Chapter End: special rules for the 108th drop of each cycle
    if cycle_num == 108:
        name = f"MUGEN #108 — CHAPTER END (cycle {drop_num // 108 + 1})"
        quantity = 1
        prompt = f"""FLAT PRINT ARTWORK. THIS IS MUGEN #108 — THE CHAPTER END. One piece. Never again in this exact form. The design must feel like a conclusion: a circle closing, a count reaching zero, a final mark. Bold. Definitive. Include '108' prominently. Include the full date {now.strftime('%Y.%m.%d')}. Black on white or white on black. Flat 2D, 2400×3200px, screen-print ready."""
        return name, prompt, quantity, cycle_num

    directions = [
        f"Time document: {mood}",
        f"Japanese concept: {random.choice(['侘び寂び wabi-sabi','物の哀れ mono no aware','一期一会 ichigo ichie','木漏れ日 komorebi','余白 yohaku','間合い maai'])}",
        f"Data poetry: temperature {weather['temp_c']}°C wind from {weather['wind_dir']} at {weather['wind_kmh']}km/h — these numbers as graphic composition",
        f"Bold kanji: single character full-chest, meaning chosen for drop #{cycle_num}",
        f"Garment contract: THIS IS #{cycle_num}. MADE {now.strftime('%Y.%m.%d')} {now.strftime('%H')}:00. NEVER AGAIN.",
        f"Number study: {cycle_num} — its shape, weight, and meaning as the entire design",
    ]
    direction = random.choice(directions)
    name = f"MUGEN #{drop_num:04d} ({cycle_num}/108)"
    prompt = f"""
FLAT PRINT ARTWORK. Bold graphic on solid background. THIS IS A 2D GRAPHIC DESIGN — NOT A PHOTO OF CLOTHING. No t-shirt shape. No clothing. No garment silhouette. No fabric. No model. No product photo. Flat graphic only, like a concert poster or album cover.

Brand: MUGEN (無限) — drop #{drop_num}, cycle {cycle_num}/108. {quantity} pieces only.
Timestamp: {now.strftime('%Y.%m.%d %H:00')} JST
Today: {weather['temp_c']}°C, {weather['condition']}, {weather['wind_dir']} wind

Design direction: {direction}

Execution:
- Bold typography or geometric graphic. Readable from 5 meters.
- Black on white (#ffffff) OR white on black (#000000) — solid, flat background filling the entire canvas.
- Must include: "{now.strftime('%Y.%m.%d')}" and "{cycle_num}/108" in the composition.
- No gradients. No shadows. No 3D. No clothing outline. Flat art only.
- OUTPUT: 2400×3200px flat digital artwork, solid background, screen-print ready.
"""
    return name, prompt, quantity, cycle_num

# ── Main Runner ───────────────────────────────────────────

def random_delay(brand: str):
    """Sleep a random duration before generating — makes timing unpredictable."""
    if os.environ.get("NO_DELAY"):
        return
    delays = {
        "mugen":  (0, 55 * 60),      # 0–55 min: fires at random minute within the hour
        "muon":   (0, 8 * 3600),     # 0–8 h: appears at a random time of day
        "nouns":  (0, 30 * 60),      # 0–30 min
        "staple": (0, 4 * 3600),     # 0–4 h: 朝〜昼の間に上がる気軽さ
    }
    lo, hi = delays.get(brand.split("_")[0], (0, 0))
    if hi == 0:
        return
    secs = random.randint(lo, hi)
    h, m = divmod(secs, 3600)
    m, s = divmod(m, 60)
    print(f"  sleeping {h}h {m}m {s}s before generating...")
    time.sleep(secs)


def run(brand: str):
    random_delay(brand)

    con = init_db()
    drop_num = next_drop_num(con, brand)
    weather  = get_hokkaido_weather()
    now_iso  = datetime.now().isoformat()

    print(f"[{brand.upper()}] drop #{drop_num}")
    print(f"  weather: {weather['temp_c']}°C {weather['condition']} {weather['wind_dir']}")

    # Draft drops land hidden (active=0) and skip SNS/SUZURI until approved at
    # /admin/news. Only the daily news-tee uses this; everything else is live.
    is_draft = False

    if brand == "ma":
        last = get_last_design(con, "ma")
        name, prompt = prompt_ma(weather, last)
        quantity = 1
        # MA cadence: 2026-05-11 changed monthly → weekly. Start price ¥120k → ¥30k.
        price = 30000
        cycle_num = None
        is_ice = False
        # Auction ends 7 days from now.
        from datetime import timedelta
        auction_end = (datetime.now() + timedelta(days=7)).isoformat()

    elif brand == "muon":
        name, prompt, quantity, is_ice = prompt_muon(weather, drop_num)
        # Temperature × ¥1,000 — same oracle drives both quantity and price
        price = max(3000, round(weather["temp_c"] * 1000 / 1000) * 1000) if not is_ice else 50000
        cycle_num = None
        auction_end = None

    elif brand == "mugen":
        name, prompt, quantity, cycle_num = prompt_mugen(weather, drop_num)
        # Wind speed as price driver — calm=cheap, storm=expensive
        wind = weather.get("wind_kmh", 5)
        price = max(2000, round((3000 + wind * 150) / 1000) * 1000)
        if cycle_num == 108:
            price = 30000
        auction_end = None
        is_ice = False

    elif brand.startswith("nouns"):
        track = brand.split("_")[1] if "_" in brand else "mugen"
        name, prompt, quantity, price = prompt_nouns(weather, drop_num, track)
        cycle_num = None
        is_ice = False
        auction_end = None
        brand = "nouns"  # normalize so all go to same API endpoint

    elif brand == "staple":
        name, prompt, quantity, price = prompt_staple(weather, drop_num)
        cycle_num = None
        is_ice = False
        auction_end = None

    elif brand == "news":
        # Daily 号外 tee — abstract theme of the day, numbered + sold as MUGEN,
        # but inserted as a DRAFT (active=0). Yuki approves at /admin/news.
        drop_num = next_drop_num(con, "mugen")  # real MUGEN numbering
        name, prompt, quantity, price = prompt_news(weather, drop_num)
        cycle_num = None
        is_ice = False
        auction_end = None
        is_draft = True
        brand = "mugen"  # normalize → sells on the MUGEN line as requested

    else:
        print(f"Unknown brand: {brand}")
        sys.exit(1)

    print(f"  name: {name}, qty: {quantity}, price: ¥{price:,}")

    # Steer toward proven sellers in this brand: top 3 by (sold*10 + bids*3 +
    # current_bid/1000). On cold start the list is empty and we keep the
    # original prompt as-is. parent_design records the lineage of the #1.
    parent_id: str | None = None
    if _pick_winners is not None:
        try:
            winners = _pick_winners(brand, 3) or []
        except Exception as e:
            print(f"  winner_picker failed ({e}); skipping")
            winners = []
        if winners:
            names = ", ".join(w["name"] for w in winners if w.get("name"))
            hint = (
                f"Style direction (proven sellers from this brand): {names}. "
                f"Stay in the same visual family.\n\n"
            )
            prompt = hint + prompt
            parent_id = str(winners[0]["id"])
            print(f"  winners: parent={parent_id} ({names})")

    print(f"  generating design...")

    prompt_hash = hashlib.sha256(prompt.encode()).hexdigest()[:16]
    image_bytes = generate_design(prompt)
    print(f"  generated {len(image_bytes)//1024}KB")

    # MUON and NOUNS use dark-background designs — remove black bg for seamless shirt integration
    use_transparent = brand in ("muon", "nouns")
    if use_transparent:
        image_bytes = make_transparent_bg(image_bytes)

    # Burn serial number into the design (physical shirt will have it)
    image_bytes = embed_serial_number(image_bytes, brand, drop_num, quantity)
    print(f"  serial: embedded")

    # Invisible watermark — save PNG locally as authenticity proof
    wm_bytes = embed_watermark(image_bytes, brand, drop_num, prompt_hash)
    design_path = DESIGNS_DIR / f"{brand}_{drop_num:04d}_{prompt_hash[:8]}.png"
    design_path.write_bytes(wm_bytes)
    print(f"  watermark: saved to {design_path.name}")

    filename = f"{brand}_{datetime.now().strftime('%Y%m%d%H%M%S')}.png"
    # upload_to_printful now receives pre-processed bytes (transparent already applied above)
    file_url  = upload_to_printful(wm_bytes, filename, transparent=False)
    print(f"  uploaded: {file_url}")

    mockup_url = get_mockup(PF_PRODUCT, PF_VARIANT_BLK, file_url)
    print(f"  mockup: {mockup_url or 'pending'}")

    seed_data = json.dumps({
        "weather": weather,
        "mood": time_mood(),
        "drop_num": drop_num,
        "cycle": cycle_num,
        "is_ice": is_ice,
        "timestamp": now_iso,
    })

    # Mint NFT certificate
    nft_mint = mint_nft_certificate(drop_num, {
        "name": name,
        "description": prompt[:200],
        "image": mockup_url or file_url,
        "attributes": [
            {"trait_type": "Brand",       "value": brand.upper()},
            {"trait_type": "Drop",        "value": drop_num},
            {"trait_type": "Quantity",    "value": quantity},
            {"trait_type": "Temperature", "value": f"{weather['temp_c']}°C"},
            {"trait_type": "Location",    "value": "Teshikaga, Hokkaido"},
            {"trait_type": "Timestamp",   "value": now_iso},
            {"trait_type": "Prompt Hash", "value": prompt_hash},
        ]
    })
    print(f"  nft: {nft_mint or 'pending (no HELIUS_API_KEY)'}")

    # print_url = the high-resolution print-ready asset that ships to the
    # printer. For Gemini-generated drops the design_url (transparent /
    # white-bg PNG uploaded to R2) IS the print file, so we mirror it into
    # print_url. ADMIN-ONLY column — never surfaced by public APIs.
    print_url = file_url
    con.execute("""
        INSERT INTO products
        (brand, drop_num, name, design_url, mockup_url, print_url, price_jpy, inventory,
         created_at, weather_data, prompt_text, prompt_hash, seed_data, auction_end, nft_mint,
         parent_design, color, size)
        VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
    """, (brand, drop_num, name, file_url, mockup_url, print_url, price,
          quantity, now_iso, json.dumps(weather), prompt, prompt_hash,
          seed_data, auction_end, nft_mint, parent_id, "BLK", "M"))
    con.commit()
    print(f"  saved locally.")

    # Push to deployed store. Insert WITHOUT mockup_url first (we'll set the
    # permanent R2 URL in a follow-up call) so the row is the source of truth
    # for the new product id.
    new_pid = None
    try:
        payload = {
            "brand": brand, "drop_num": drop_num, "name": name,
            "design_url": file_url, "mockup_url": None,
            "price_jpy": price, "inventory": quantity,
            "weather_data": json.dumps(weather), "prompt_hash": prompt_hash,
            "seed_data": seed_data, "auction_end": auction_end, "nft_mint": nft_mint,
            "is_ice": is_ice, "is_draft": is_draft,
        }
        r = requests.post(f"{STORE_URL}/api/admin/import?token={ADMIN_TOKEN}", json=payload, timeout=10)
        print(f"  pushed to store: {r.status_code}")
        if r.ok:
            new_pid = r.json().get("id")
    except Exception as e:
        print(f"  store push failed: {e}")

    # Upload mockup to Cloudflare R2 (wearmu-mockups bucket) so the public URL
    # https://mockups.wearmu.com/<id>.jpg is permanent. Falls back to leaving
    # the Printful tmp URL on the row if R2 upload fails — the Rust server
    # will then auto-persist it onto the Fly volume on the next admin call.
    if new_pid and mockup_url:
        try:
            push_mockup_to_r2(new_pid, mockup_url)
        except Exception as e:
            print(f"  R2 push failed ({e}); falling back to Printful tmp URL")
            try:
                requests.patch(
                    f"{STORE_URL}/api/admin/mockup?token={ADMIN_TOKEN}",
                    json={"product_id": new_pid, "mockup_url": mockup_url},
                    timeout=15,
                )
            except Exception as e2:
                print(f"  fallback patch failed: {e2}")

    print(f"  done.")
    return drop_num


def push_mockup_to_r2(product_id: int, source_url: str) -> None:
    """Download bytes from source_url and upload to R2 bucket wearmu-mockups
    via wrangler CLI. Updates the wearmu DB to point at mockups.wearmu.com."""
    import subprocess, tempfile
    img = requests.get(source_url, timeout=30)
    if img.status_code != 200:
        raise RuntimeError(f"download {source_url} → HTTP {img.status_code}")
    with tempfile.NamedTemporaryFile(suffix=".jpg", delete=False) as f:
        f.write(img.content)
        tmp_path = f.name
    try:
        # Cron's PATH is minimal; resolve wrangler explicitly.
        wrangler_bin = os.environ.get("WRANGLER_BIN", "/opt/homebrew/bin/wrangler")
        result = subprocess.run(
            [
                wrangler_bin, "r2", "object", "put",
                f"wearmu-mockups/{product_id}.jpg",
                f"--file={tmp_path}",
                "--remote",
                "--content-type=image/jpeg",
            ],
            capture_output=True, text=True, timeout=60,
        )
        if result.returncode != 0:
            raise RuntimeError(f"wrangler exit {result.returncode}: {result.stderr[-300:]}")
        public_url = f"https://mockups.wearmu.com/{product_id}.jpg"
        # Cloudflare may have cached a 404 for this URL — purge to be safe.
        cf_token = os.environ.get("CLOUDFLARE_API_KEY")
        cf_email = os.environ.get("CLOUDFLARE_EMAIL", "mail@yukihamada.jp")
        zone_id = os.environ.get("WEARMU_ZONE_ID", "0d0b88e1d5c4cea8713cda1744fcc713")
        if cf_token:
            try:
                requests.post(
                    f"https://api.cloudflare.com/client/v4/zones/{zone_id}/purge_cache",
                    headers={"X-Auth-Email": cf_email, "X-Auth-Key": cf_token,
                             "Content-Type": "application/json"},
                    json={"files": [public_url]},
                    timeout=15,
                )
            except Exception as e:
                print(f"  cache purge skipped: {e}")
        # Point DB at R2
        r = requests.patch(
            f"{STORE_URL}/api/admin/mockup?token={ADMIN_TOKEN}",
            json={"product_id": product_id, "mockup_url": public_url},
            timeout=15,
        )
        print(f"  R2: {public_url}  (DB update {r.status_code})")
    finally:
        try: os.unlink(tmp_path)
        except OSError: pass

if __name__ == "__main__":
    brand = sys.argv[1] if len(sys.argv) > 1 else "mugen"
    valid = ("ma", "muon", "mugen", "nouns", "nouns_mugen", "nouns_muon", "nouns_ma", "staple", "news")
    if brand not in valid:
        print(f"usage: python generate.py [{' | '.join(valid)}]")
        sys.exit(1)
    run(brand)
