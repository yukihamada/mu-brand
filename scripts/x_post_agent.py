#!/usr/bin/env python3
"""
MU — X (Twitter) auto-post agent.

Polls products.db every 10 min (via cron) for newly inserted designs and
posts ONE tweet per fresh product to X. Image-first (v1.1 media/upload)
+ text via v2 /2/tweets.

Triggered design generation already happens hourly (MUGEN), daily (MUON,
NOUNS variants), monthly (MA). With PV 208/7d we want every drop to ping
the timeline so this agent is the org's primary distribution lever until
we set up webhooks.

Hard constraints (per task spec + MEMORY):
  - DRY_RUN by default; only posts when MU_X_LIVE=1 is set in the env
  - x_posts(product_id, posted_at, tweet_id) tracks idempotency — same
    product never tweets twice even if cron runs back to back
  - self-mention guard: tweet text must NEVER contain the org's own
    handle (memory: feedback_x_self_mention.md). We strip @wearMUcom
    and @wearmu before composing.
  - failures (rate limit, auth) are logged and the loop continues
  - secrets (api keys, bearer) never appear in stdout/stderr

Env (all read from ~/.env via the cron shell wrapper):
  X_API_KEY        (consumer key, OAuth 1.0a)        — or TWITTER_API_KEY
  X_API_SECRET     (consumer secret)                  — or TWITTER_API_SECRET
  X_ACCESS_TOKEN   (user access token)                — or TWITTER_ACCESS_TOKEN
  X_ACCESS_SECRET  (user access token secret)         — or TWITTER_ACCESS_TOKEN_SECRET
  X_BEARER_TOKEN   (optional, for v2 GET endpoints)
  MU_X_LIVE        ("1" to actually post; anything else = DRY_RUN)
  MU_X_WINDOW_MIN  (lookback window in minutes, default 10)
  MU_X_LIMIT       (max tweets per run, default 3)
  MU_X_SELF_HANDLE (default "wearMUcom"; used for self-mention guard)

Local DRY_RUN:
  python scripts/x_post_agent.py
  # → logs "[DRY_RUN] would tweet: …" without touching X
"""
from __future__ import annotations

import io
import json
import logging
import math
import os
import sqlite3
import sys
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

ROOT     = Path(__file__).resolve().parent.parent
DB_PATH  = Path(os.environ.get("MU_PRODUCTS_DB", str(ROOT / "products.db")))
STORE_URL = os.environ.get("MU_STORE_URL", "https://wearmu.com")

WINDOW_MIN = int(os.environ.get("MU_X_WINDOW_MIN", "10"))
LIMIT      = int(os.environ.get("MU_X_LIMIT", "3"))
SELF_HANDLE = os.environ.get("MU_X_SELF_HANDLE", "wearMUcom").lstrip("@").lower()
LIVE       = os.environ.get("MU_X_LIVE", "") == "1"

X_KEY    = os.environ.get("X_API_KEY")        or os.environ.get("TWITTER_API_KEY", "")
X_SEC    = os.environ.get("X_API_SECRET")     or os.environ.get("TWITTER_API_SECRET", "")
X_TOKEN  = os.environ.get("X_ACCESS_TOKEN")   or os.environ.get("TWITTER_ACCESS_TOKEN", "")
X_TSEC   = os.environ.get("X_ACCESS_SECRET")  or os.environ.get("TWITTER_ACCESS_TOKEN_SECRET", "")
X_BEARER = os.environ.get("X_BEARER_TOKEN", "")

# Tweet text budget. X counts most Japanese chars as weight 2 but practically
# we cap by len() ≤ 280 (the engine truncates well below the weighted limit).
TWEET_MAX = 280

LOG = logging.getLogger("x_post_agent")
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(message)s",
    datefmt="%Y-%m-%d %H:%M:%S",
)


# ── DB helpers ─────────────────────────────────────────────────────────────

def open_db() -> sqlite3.Connection:
    conn = sqlite3.connect(DB_PATH, timeout=30)
    conn.row_factory = sqlite3.Row
    ensure_schema(conn)
    return conn


def ensure_schema(conn: sqlite3.Connection) -> None:
    conn.executescript(
        """
        CREATE TABLE IF NOT EXISTS x_posts (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            product_id  INTEGER NOT NULL UNIQUE,
            tweet_id    TEXT,
            posted_at   TEXT NOT NULL,
            text        TEXT,
            status      TEXT NOT NULL DEFAULT 'posted',
            error       TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_x_posts_posted_at ON x_posts(posted_at DESC);
        """
    )
    conn.commit()


def fetch_candidates(conn: sqlite3.Connection, window_min: int, limit: int) -> list[sqlite3.Row]:
    """New designs in the last `window_min` minutes that have not been tweeted."""
    cutoff = (datetime.now(timezone.utc) - timedelta(minutes=window_min)).strftime("%Y-%m-%dT%H:%M:%S")
    # Skip products that have already been posted ('posted') OR that the
    # CURRENT run-mode already attempted ('dry_run' rows block LIVE only
    # via prompt_hash dedup below; for the join below we just exclude
    # rows in a terminal state for this mode).
    skip_statuses = ("posted", "error") if LIVE else ("posted", "dry_run", "error")
    placeholders = ",".join("?" for _ in skip_statuses)
    rows = conn.execute(
        f"""
        SELECT p.id, p.brand, p.drop_num, p.name, p.price_jpy, p.serial_code,
               p.design_url, p.mockup_url, p.weather_data, p.seed_data,
               p.prompt_hash, p.created_at, p.active
          FROM products p
          LEFT JOIN x_posts x
            ON x.product_id = p.id AND x.status IN ({placeholders})
         WHERE x.product_id IS NULL
           AND (p.active IS NULL OR p.active = 1)
           AND p.created_at >= ?
           AND COALESCE(p.mockup_url, p.design_url, '') != ''
         ORDER BY p.id DESC
         LIMIT ?
        """,
        (*skip_statuses, cutoff, limit),
    ).fetchall()
    return rows


def record_post(conn: sqlite3.Connection, product_id: int, tweet_id: str | None,
                text: str, status: str, error: str | None = None) -> None:
    now = datetime.now(timezone.utc).isoformat(timespec="seconds")
    # UPSERT semantics: a dry_run row is allowed to be promoted to 'posted'
    # on a later LIVE call, but once a real tweet exists we never overwrite
    # the tweet_id (CASE guard below).
    conn.execute(
        """INSERT INTO x_posts (product_id, tweet_id, posted_at, text, status, error)
           VALUES (?, ?, ?, ?, ?, ?)
           ON CONFLICT(product_id) DO UPDATE SET
             tweet_id  = COALESCE(x_posts.tweet_id, excluded.tweet_id),
             posted_at = CASE WHEN x_posts.status = 'posted' THEN x_posts.posted_at
                              ELSE excluded.posted_at END,
             text      = CASE WHEN x_posts.status = 'posted' THEN x_posts.text
                              ELSE excluded.text END,
             status    = CASE WHEN x_posts.status = 'posted' THEN x_posts.status
                              ELSE excluded.status END,
             error     = CASE WHEN x_posts.status = 'posted' THEN x_posts.error
                              ELSE excluded.error END""",
        (product_id, tweet_id, now, text[:1024], status, (error or "")[:512]),
    )
    conn.commit()


# ── Lunar phase (synodic, JMA-quality enough for a tweet) ──────────────────

_LUNAR_LABELS = [
    (0.03, "朔"),    # new
    (0.22, "三日月"),
    (0.28, "上弦"),
    (0.47, "十三夜"),
    (0.53, "望"),    # full
    (0.72, "十六夜"),
    (0.78, "下弦"),
    (0.97, "有明"),
    (1.00, "朔"),
]


def lunar_phase_label(now: datetime | None = None) -> str:
    """Return JP moon-phase glyph for `now` (UTC). Synodic ≈ 29.530589 d."""
    now = now or datetime.now(timezone.utc)
    # Reference new moon: 2000-01-06 18:14 UTC.
    ref = datetime(2000, 1, 6, 18, 14, tzinfo=timezone.utc)
    days = (now - ref).total_seconds() / 86400.0
    frac = (days % 29.530589) / 29.530589
    for cutoff, label in _LUNAR_LABELS:
        if frac < cutoff:
            return label
    return "朔"


# ── Tweet composition ──────────────────────────────────────────────────────

_HASHTAG_POOLS: dict[str, list[str]] = {
    "mugen":   ["#MU", "#無", "#月", "#MUGEN", "#弟子屈"],
    "muon":    ["#MU", "#無", "#月", "#MUON"],
    "ma":      ["#MU", "#無", "#月", "#間"],
    "nouns":   ["#MU", "#無", "#nouns", "#nounsDAO"],
    "mu":      ["#MU", "#無", "#月", "#brand"],
    "jiufight":["#MU", "#JIUFIGHT", "#柔術", "#BJJ"],
    "sweep":   ["#MU", "#SWEEP", "#柔術", "#BJJ"],
}


def _rotate_hashtags(brand: str, drop_num: int) -> str:
    pool = _HASHTAG_POOLS.get(brand.lower(), ["#MU", "#無", "#月"])
    # rotate by drop_num so consecutive tweets look different in feed
    n = max(1, len(pool))
    start = (drop_num or 0) % n
    rotated = pool[start:] + pool[:start]
    return " ".join(rotated[:4])


def _strip_self_mention(text: str) -> str:
    """Remove @SELF_HANDLE from the tweet body — never self-mention.

    The standard X mention-responder pitfall (memory: feedback_x_self_mention)
    is that the org's own quote-tweets surface in /mentions. We are the
    poster here, not the responder, but we still defensively strip the
    handle so a future template change never tweets at us by accident.
    """
    if not SELF_HANDLE:
        return text
    needles = (f"@{SELF_HANDLE}", f"@{SELF_HANDLE.upper()}")
    out = text
    for n in needles:
        # case-insensitive replace
        idx = out.lower().find(n.lower())
        while idx >= 0:
            out = out[:idx] + out[idx + len(n):]
            idx = out.lower().find(n.lower())
    return out


def _buy_link(row: sqlite3.Row) -> str:
    """Prefer serial_code path, fall back to id."""
    serial = (row["serial_code"] or "").strip()
    if serial:
        return f"{STORE_URL}/p/{serial}"
    return f"{STORE_URL}/p/{row['id']}"


def _brand_label(brand: str) -> str:
    return {
        "mugen":   "━◯━ MUGEN",
        "muon":    "━●━ MUON",
        "ma":      "間 MA",
        "nouns":   "MUGEN × NOUNS",
        "mu":      "MU",
        "jiufight":"MU × JIUFIGHT",
        "sweep":   "MU × SWEEP",
    }.get(brand.lower(), brand.upper())


def _weather_blob(row: sqlite3.Row) -> str:
    try:
        wd = json.loads(row["weather_data"] or "{}")
    except Exception:
        return ""
    temp = wd.get("temp_c")
    loc = (wd.get("location") or "").split(",")[0].strip()
    bits = []
    if loc:
        bits.append(loc)
    if temp is not None:
        bits.append(f"{temp}°C")
    return " / ".join(bits)


def _short_name(name: str, max_chars: int = 36) -> str:
    name = (name or "").strip()
    if len(name) <= max_chars:
        return name
    return name[: max_chars - 1] + "…"


def compose_tweet(row: sqlite3.Row) -> str:
    """Compose ≤280-char Japanese tweet for one product row."""
    brand = (row["brand"] or "mu").lower()
    name  = _short_name(row["name"] or f"{brand.upper()} #{row['drop_num']:04d}")
    price = int(row["price_jpy"] or 0)
    jp    = f"¥{price:,}" if price else ""
    intl  = f" / 海外 ¥{price * 16 // 10:,}" if price else ""  # rough EU markup
    link  = _buy_link(row)
    weather = _weather_blob(row)
    phase = lunar_phase_label()
    tags  = _rotate_hashtags(brand, row["drop_num"] or 0)

    label = _brand_label(brand)
    # avoid "MUGEN MUGEN #…" / "MUGEN × NOUNS MUGEN × NOUNS …" duplication when
    # the row name already starts with the brand label's plain prefix.
    label_plain = label.replace("━◯━ ", "").replace("━●━ ", "").strip()
    if label_plain and name.lower().startswith(label_plain.lower()):
        head = f"{label} drop {name[len(label_plain):].strip()}。".strip()
    else:
        head = f"{label} {name} drop。"
    mid_bits = [b for b in (weather, phase) if b]
    mid = " / ".join(mid_bits)
    if jp:
        price_line = f"{jp} 国内{intl}。"
    else:
        price_line = ""

    text = f"{head}\n{mid}\n{price_line}\n{link}\n{tags}".strip()
    text = _strip_self_mention(text)
    # collapse any double blank lines, then trim to TWEET_MAX
    while "\n\n\n" in text:
        text = text.replace("\n\n\n", "\n\n")
    if len(text) > TWEET_MAX:
        # trim hashtags first, then name
        text = text[: TWEET_MAX - 1] + "…"
    return text


# ── X API (lazy import; agent stays installable without tweepy) ────────────

def _load_tweepy():
    try:
        import tweepy  # noqa: WPS433
        return tweepy
    except Exception as exc:
        LOG.warning("tweepy unavailable (%s) — switching to DRY_RUN.", exc)
        return None


def _have_secrets() -> bool:
    return bool(X_KEY and X_SEC and X_TOKEN and X_TSEC)


def _upload_image(api_v1, url: str, product_id: int) -> str | None:
    if not url or not url.startswith("http"):
        return None
    try:
        import requests
    except Exception:
        LOG.warning("requests not installed — skipping image upload.")
        return None
    try:
        resp = requests.get(url, timeout=30)
    except Exception as exc:
        LOG.warning("[%s] image fetch failed: %s", product_id, exc)
        return None
    if resp.status_code != 200 or len(resp.content) < 1000:
        LOG.warning("[%s] image fetch returned %s (%d bytes)", product_id, resp.status_code, len(resp.content))
        return None
    try:
        media = api_v1.media_upload(filename=f"{product_id}.jpg", file=io.BytesIO(resp.content))
        return media.media_id_string
    except Exception as exc:
        # tweepy raises lots of subclasses; log type + first 200 chars only
        LOG.warning("[%s] media_upload failed: %s: %s", product_id, type(exc).__name__, str(exc)[:200])
        return None


def post_tweet(text: str, image_url: str | None, product_id: int) -> tuple[str | None, str | None]:
    """Return (tweet_id, error). Either may be None."""
    tweepy = _load_tweepy()
    if tweepy is None:
        return None, "tweepy_missing"
    auth = tweepy.OAuth1UserHandler(X_KEY, X_SEC, X_TOKEN, X_TSEC)
    api_v1 = tweepy.API(auth)
    client = tweepy.Client(
        consumer_key=X_KEY, consumer_secret=X_SEC,
        access_token=X_TOKEN, access_token_secret=X_TSEC,
        bearer_token=X_BEARER or None,
    )
    media_id = _upload_image(api_v1, image_url or "", product_id) if image_url else None
    kw: dict[str, Any] = {"text": text}
    if media_id:
        kw["media_ids"] = [media_id]
    try:
        res = client.create_tweet(**kw)
    except Exception as exc:
        return None, f"{type(exc).__name__}: {str(exc)[:200]}"
    tid = None
    try:
        tid = str(res.data.get("id")) if res and res.data else None
    except Exception:
        pass
    return tid, None


# ── Main loop ──────────────────────────────────────────────────────────────

def run() -> int:
    if not DB_PATH.exists():
        LOG.error("products.db not found at %s", DB_PATH)
        return 1

    conn = open_db()
    rows = fetch_candidates(conn, WINDOW_MIN, LIMIT)
    LOG.info("candidates=%d window=%dmin limit=%d live=%s db=%s",
             len(rows), WINDOW_MIN, LIMIT, LIVE, DB_PATH)

    if not rows:
        return 0

    have_secrets = _have_secrets()
    if not have_secrets and LIVE:
        LOG.warning("MU_X_LIVE=1 but X_API_* env not set — falling back to DRY_RUN.")
    live_mode = LIVE and have_secrets

    posted = 0
    for row in rows:
        pid = row["id"]
        try:
            text = compose_tweet(row)
        except Exception as exc:
            LOG.exception("[%s] compose failed: %s", pid, exc)
            continue

        # never log secrets — just text + image url
        LOG.info("[%s][%s] text=%r len=%d image=%s",
                 pid, row["brand"], text, len(text),
                 (row["mockup_url"] or row["design_url"] or "")[:120])

        if not live_mode:
            LOG.info("[%s] DRY_RUN — would tweet (no API call)", pid)
            record_post(conn, pid, None, text, "dry_run", None)
            continue

        image_url = row["mockup_url"] or row["design_url"]
        tweet_id, err = post_tweet(text, image_url, pid)
        if tweet_id:
            LOG.info("[%s] posted: https://x.com/i/status/%s", pid, tweet_id)
            record_post(conn, pid, tweet_id, text, "posted", None)
            posted += 1
            time.sleep(2)  # gentle pacing between posts
        else:
            LOG.warning("[%s] post failed: %s", pid, err)
            record_post(conn, pid, None, text, "error", err)

    LOG.info("done: posted=%d candidates=%d", posted, len(rows))
    return 0


if __name__ == "__main__":
    sys.exit(run())
