#!/usr/bin/env python3
"""
cart_abandon_mail — 1-hour-delayed recovery email for abandoned carts.

Reads `cart_abandons` rows from products.db (the store backend writes them
via `INSERT INTO cart_abandons` in store/src/main.rs) and emails the
customer a short reminder with the product name, JPY price, and a /p/<id>
link back to the product page.

# Safety model

- DEFAULT IS DRY_RUN. The script will print "DRY_RUN: would send to ..."
  lines and leave `notified_at` NULL. Cron installs this script with no
  extra env, so a misfiring cron only fills the log — it never touches
  the real Resend account or a real customer inbox.
- Set env `MU_ABANDON_LIVE=1` to actually POST to Resend. This flag is
  the human-OK gate documented in feedback_email_blast_radius.md.
- Hard caps to keep blast radius small even when live:
    * max 50 emails per run
    * only rows created 1h..72h ago
    * skip rows already notified (`notified_at IS NULL`)
- All exceptions are swallowed and we exit 0 so a flaky run never breaks
  the surrounding cron chain (cv_pulse, twitter_post, etc).

# Schema

If `cart_abandons` does not yet have a `notified_at` column we add it via
`ALTER TABLE` (idempotent — PRAGMA table_info checked first). The
existing rows are otherwise treated as read-only; we only write
`notified_at` after a successful Resend 2xx.

# Run modes

    # DRY_RUN (default — safe to run anywhere):
    python scripts/cart_abandon_mail.py

    # PRODUCTION (only via human-flipped env):
    MU_ABANDON_LIVE=1 python scripts/cart_abandon_mail.py
"""
from __future__ import annotations

import json
import os
import sqlite3
import sys
import traceback
from pathlib import Path
from urllib import request as urlrequest
from urllib.error import HTTPError, URLError

ROOT = Path(__file__).resolve().parent.parent
DB_PATH = os.environ.get("MU_DB", str(ROOT / "store" / "products.db"))
LIVE = os.environ.get("MU_ABANDON_LIVE") == "1"
MAX_PER_RUN = 50
SITE_BASE = os.environ.get("MU_SITE_BASE", "https://wearmu.com").rstrip("/")
FROM_ADDR = os.environ.get("MU_ABANDON_FROM", "MU <noreply@enablerdao.com>")
REPLY_TO = os.environ.get("MU_ABANDON_REPLY_TO", "info@enablerdao.com")


def ensure_notified_at(conn: sqlite3.Connection) -> bool:
    """Add notified_at column if missing. Returns False if the table itself is absent."""
    tbl = conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='cart_abandons'"
    ).fetchone()
    if not tbl:
        return False
    cols = {row[1] for row in conn.execute("PRAGMA table_info(cart_abandons)").fetchall()}
    if "notified_at" not in cols:
        conn.execute("ALTER TABLE cart_abandons ADD COLUMN notified_at TEXT")
        conn.commit()
    return True


def parse_product_ids(raw) -> list:
    """cart_abandons.product_ids may be JSON array, CSV, or single int."""
    if raw is None:
        return []
    s = str(raw).strip()
    if not s:
        return []
    try:
        v = json.loads(s)
        if isinstance(v, list):
            return [str(x) for x in v if str(x).strip()]
        return [str(v)]
    except Exception:
        return [p.strip() for p in s.split(",") if p.strip()]


def lookup_products(conn: sqlite3.Connection, ids: list) -> list[dict]:
    """Fetch (name, price_jpy, serial_code, id) for the cart ids, in cart order."""
    out: list[dict] = []
    for raw in ids:
        row = None
        try:
            pid = int(raw)
            row = conn.execute(
                "SELECT id, name, price_jpy, serial_code FROM products WHERE id=?",
                (pid,),
            ).fetchone()
        except (TypeError, ValueError):
            pass
        if row is None:
            # try serial_code match
            row = conn.execute(
                "SELECT id, name, price_jpy, serial_code FROM products WHERE serial_code=?",
                (str(raw),),
            ).fetchone()
        if row:
            pid, name, price_jpy, serial = row
            link_key = serial or str(pid)
            out.append({
                "id": pid,
                "name": name or "MU プロダクト",
                "price_jpy": int(price_jpy or 0),
                "url": f"{SITE_BASE}/p/{link_key}",
            })
    return out


def render_html(products: list[dict]) -> str:
    if not products:
        body = (
            "<p>カートに入れたままの MU プロダクトがあります。<br>"
            f'<a href="{SITE_BASE}">こちら</a> からもう一度ご覧ください。</p>'
        )
    else:
        lis = "".join(
            f'<li><a href="{p["url"]}">{p["name"]}</a> — ¥{p["price_jpy"]:,}</li>'
            for p in products
        )
        body = (
            "<p>こんにちは,</p>"
            "<p>カートに入れたままの MU プロダクトがあります。在庫は 1 点限りで、"
            "他のお客様にお譲りされる前にご確認ください。</p>"
            f"<ul>{lis}</ul>"
            f'<p>サイト: <a href="{SITE_BASE}">{SITE_BASE}</a><br>'
            "ご質問は本メールへの返信、または info@enablerdao.com まで。<br>"
            "— MU / 株式会社イネブラ</p>"
        )
    return body


def send_via_resend(api_key: str, to_addr: str, subject: str, html: str) -> tuple[bool, str]:
    payload = json.dumps({
        "from": FROM_ADDR,
        "to": [to_addr],
        "subject": subject,
        "html": html,
        "reply_to": REPLY_TO,
    }).encode("utf-8")
    req = urlrequest.Request(
        "https://api.resend.com/emails",
        data=payload,
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    try:
        with urlrequest.urlopen(req, timeout=15) as resp:
            ok = 200 <= resp.status < 300
            return ok, f"http {resp.status}"
    except HTTPError as e:
        # Do NOT include body in case it echoes the api key in error context.
        return False, f"http {e.code}"
    except URLError as e:
        return False, f"network {e.reason}"
    except Exception as e:  # pragma: no cover
        return False, f"err {type(e).__name__}"


def main() -> int:
    if not os.path.exists(DB_PATH):
        print(f"[cart-abandon] db not found at {DB_PATH}; nothing to do")
        return 0
    conn = sqlite3.connect(DB_PATH)
    try:
        if not ensure_notified_at(conn):
            print("[cart-abandon] cart_abandons table does not exist yet; nothing to do")
            return 0

        rows = conn.execute(
            """
            SELECT id, email, product_ids, created_at
            FROM cart_abandons
            WHERE notified_at IS NULL
              AND email IS NOT NULL
              AND email != ''
              AND created_at < datetime('now', '-1 hour')
              AND created_at > datetime('now', '-72 hours')
            ORDER BY created_at ASC
            LIMIT ?
            """,
            (MAX_PER_RUN,),
        ).fetchall()

        if not rows:
            print("[cart-abandon] no abandoned carts to notify")
            return 0

        api_key = os.environ.get("RESEND_API_KEY", "")
        if LIVE and not api_key:
            print("[cart-abandon] MU_ABANDON_LIVE=1 but RESEND_API_KEY unset — refusing to send")
            return 0

        mode = "LIVE" if LIVE else "DRY_RUN"
        sent, failed, skipped = 0, 0, 0
        for cab_id, email, product_ids_raw, created_at in rows:
            try:
                ids = parse_product_ids(product_ids_raw)
                products = lookup_products(conn, ids)
                subject = "カートに入れたままの MU プロダクトがあります"
                html = render_html(products)

                if not LIVE:
                    names = ", ".join(p["name"] for p in products) or "(no products resolved)"
                    print(f"DRY_RUN: would send to {email} [cab#{cab_id} created={created_at}] items: {names}")
                    skipped += 1
                    continue

                ok, info = send_via_resend(api_key, email, subject, html)
                if ok:
                    conn.execute(
                        "UPDATE cart_abandons SET notified_at = datetime('now') WHERE id=?",
                        (cab_id,),
                    )
                    conn.commit()
                    sent += 1
                    print(f"[cart-abandon] sent cab#{cab_id} to {email} ({info})")
                else:
                    failed += 1
                    print(f"[cart-abandon] FAILED cab#{cab_id} to {email}: {info}")
            except Exception as inner:  # never let one row break the batch
                failed += 1
                print(f"[cart-abandon] row#{cab_id} unexpected: {inner}")

        print(f"[cart-abandon] mode={mode} considered={len(rows)} sent={sent} dry_skipped={skipped} failed={failed}")
        return 0
    finally:
        try:
            conn.close()
        except Exception:
            pass


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception:
        # Cron-friendly: log and exit 0 so we never poison downstream jobs.
        traceback.print_exc()
        sys.exit(0)
