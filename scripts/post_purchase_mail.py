#!/usr/bin/env python3
"""
post_purchase_mail — 2-stage delayed mail to paid customers.

Reads `post_purchase_queue` rows (populated by the Stripe webhook in
store/src/main.rs) and fires two delayed Resend mails per customer:

  1. SHIPPING  (paid + 1d .. +7d, shipping_mailed_at IS NULL)
       "ご注文ありがとうございます。 5〜7 営業日でお届け"
  2. REVIEW    (paid + 7d .. +30d, review_mailed_at IS NULL)
       "商品どうでしたか? 1 分で ★ レビュー" → /reviews/<session_id>

# Safety model

- DEFAULT IS DRY_RUN. The script prints "DRY_RUN: would send ..." and
  leaves *_mailed_at NULL. Cron installs this script with no extra env
  so a misfiring cron only fills the log — never the customer inbox.
- Set env `MU_POSTPURCHASE_LIVE=1` to actually POST to Resend. This is
  the human-OK gate documented in feedback_email_blast_radius.md.
- Hard caps to keep blast radius small even when live:
    * max 50 emails per sweep (shipping + review counted separately)
    * shipping window:  -7d .. -1d  (skips < 24h old, skips > 7d cold)
    * review window:    -30d .. -7d
    * skip rows already mailed (column-specific IS NULL check)
- All exceptions are swallowed and we exit 0 so a flaky run never breaks
  the surrounding cron chain (selfimprove → cart_abandon → post_purchase
  → sitemap_ping).

# Schema

The `post_purchase_queue` table is created by the Stripe webhook
(store/src/main.rs). If it does not yet exist this script exits 0
with "table not present" — webhook hasn't fired yet, nothing to do.
We never CREATE the table here; that's webhook's responsibility.

Expected columns:
    email TEXT, session_id TEXT, amount_jpy INTEGER, paid_at TEXT,
    shipping_mailed_at TEXT, review_mailed_at TEXT

# Run modes

    # DRY_RUN (default — safe to run anywhere, what cron uses):
    python scripts/post_purchase_mail.py

    # PRODUCTION (only via human-flipped env):
    MU_POSTPURCHASE_LIVE=1 python scripts/post_purchase_mail.py
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
LIVE = os.environ.get("MU_POSTPURCHASE_LIVE") == "1"
MAX_PER_SWEEP = 50
SITE_BASE = os.environ.get("MU_SITE_BASE", "https://wearmu.com").rstrip("/")
FROM_ADDR = os.environ.get("MU_POSTPURCHASE_FROM", "MU <noreply@enablerdao.com>")
REPLY_TO = os.environ.get("MU_POSTPURCHASE_REPLY_TO", "info@enablerdao.com")


def table_present(conn: sqlite3.Connection) -> bool:
    row = conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='post_purchase_queue'"
    ).fetchone()
    return bool(row)


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
        # Do NOT include body — may echo api key in error context.
        return False, f"http {e.code}"
    except URLError as e:
        return False, f"network {e.reason}"
    except Exception as e:  # pragma: no cover
        return False, f"err {type(e).__name__}"


def render_shipping_html(amount_jpy: int | None, session_id: str) -> str:
    amt = f"¥{int(amount_jpy):,}" if amount_jpy else "ご購入"
    return (
        "<p>こんにちは,</p>"
        f"<p>このたびは MU をお選びいただきありがとうございます ({amt})。</p>"
        "<p>商品は <strong>5〜7 営業日</strong>でお届け予定です。"
        "Printful 印刷工場から直送されますので、発送通知メールに記載の追跡番号で配送状況をご確認いただけます。</p>"
        "<p>ご質問は本メールへの返信、または "
        '<a href="mailto:info@enablerdao.com">info@enablerdao.com</a> まで。</p>'
        f'<p style="color:#888;font-size:12px">注文ID: {session_id}<br>'
        "— MU / 株式会社イネブラ</p>"
    )


def render_review_html(session_id: str) -> str:
    url = f"{SITE_BASE}/reviews/{session_id}"
    return (
        "<p>こんにちは,</p>"
        "<p>1 週間前にご購入いただいた MU プロダクトはお手元に届きましたでしょうか?</p>"
        "<p>もしよろしければ <strong>1 分</strong>で ★ レビューをお寄せください。"
        "あなたの一言が次のお客様の判断を助けます。</p>"
        f'<p><a href="{url}" style="display:inline-block;padding:12px 24px;background:#000;color:#fff;text-decoration:none;border-radius:4px">★ レビューを書く</a></p>'
        f'<p style="font-size:12px;color:#888">リンクが開けない場合は: {url}</p>'
        "<p>ご質問は本メールへの返信、または "
        '<a href="mailto:info@enablerdao.com">info@enablerdao.com</a> まで。<br>'
        "— MU / 株式会社イネブラ</p>"
    )


def sweep_shipping(conn: sqlite3.Connection, api_key: str) -> tuple[int, int, int, int]:
    rows = conn.execute(
        """
        SELECT rowid, email, session_id, amount_jpy, paid_at
        FROM post_purchase_queue
        WHERE shipping_mailed_at IS NULL
          AND email IS NOT NULL
          AND email != ''
          AND paid_at < datetime('now', '-1 day')
          AND paid_at > datetime('now', '-7 days')
        ORDER BY paid_at ASC
        LIMIT ?
        """,
        (MAX_PER_SWEEP,),
    ).fetchall()

    considered = len(rows)
    sent = failed = skipped = 0
    if not rows:
        print("[post-purchase:shipping] no rows to process")
        return considered, sent, failed, skipped

    for rowid, email, session_id, amount_jpy, paid_at in rows:
        try:
            subject = "ご注文ありがとうございます — 5〜7 営業日でお届けします"
            html = render_shipping_html(amount_jpy, session_id or "")

            if not LIVE:
                print(f"DRY_RUN: would send shipping to {email} [row#{rowid} paid={paid_at} amt={amount_jpy}]")
                skipped += 1
                continue

            ok, info = send_via_resend(api_key, email, subject, html)
            if ok:
                conn.execute(
                    "UPDATE post_purchase_queue SET shipping_mailed_at = datetime('now') WHERE rowid=?",
                    (rowid,),
                )
                conn.commit()
                sent += 1
                print(f"[post-purchase:shipping] sent row#{rowid} to {email} ({info})")
            else:
                failed += 1
                print(f"[post-purchase:shipping] FAILED row#{rowid} to {email}: {info}")
        except Exception as inner:  # never let one row break the batch
            failed += 1
            print(f"[post-purchase:shipping] row#{rowid} unexpected: {inner}")

    return considered, sent, failed, skipped


def sweep_review(conn: sqlite3.Connection, api_key: str) -> tuple[int, int, int, int]:
    rows = conn.execute(
        """
        SELECT rowid, email, session_id, paid_at
        FROM post_purchase_queue
        WHERE review_mailed_at IS NULL
          AND email IS NOT NULL
          AND email != ''
          AND paid_at < datetime('now', '-7 days')
          AND paid_at > datetime('now', '-30 days')
        ORDER BY paid_at ASC
        LIMIT ?
        """,
        (MAX_PER_SWEEP,),
    ).fetchall()

    considered = len(rows)
    sent = failed = skipped = 0
    if not rows:
        print("[post-purchase:review] no rows to process")
        return considered, sent, failed, skipped

    for rowid, email, session_id, paid_at in rows:
        try:
            subject = "MU プロダクトはどうでしたか? 1 分で ★ レビュー"
            html = render_review_html(session_id or "")

            if not LIVE:
                print(f"DRY_RUN: would send review to {email} [row#{rowid} paid={paid_at} session={session_id}]")
                skipped += 1
                continue

            ok, info = send_via_resend(api_key, email, subject, html)
            if ok:
                conn.execute(
                    "UPDATE post_purchase_queue SET review_mailed_at = datetime('now') WHERE rowid=?",
                    (rowid,),
                )
                conn.commit()
                sent += 1
                print(f"[post-purchase:review] sent row#{rowid} to {email} ({info})")
            else:
                failed += 1
                print(f"[post-purchase:review] FAILED row#{rowid} to {email}: {info}")
        except Exception as inner:
            failed += 1
            print(f"[post-purchase:review] row#{rowid} unexpected: {inner}")

    return considered, sent, failed, skipped


def main() -> int:
    if not os.path.exists(DB_PATH):
        print(f"[post-purchase] db not found at {DB_PATH}; nothing to do")
        return 0
    conn = sqlite3.connect(DB_PATH)
    try:
        if not table_present(conn):
            print("[post-purchase] post_purchase_queue table not present yet (webhook hasn't fired); nothing to do")
            return 0

        api_key = os.environ.get("RESEND_API_KEY", "")
        if LIVE and not api_key:
            print("[post-purchase] MU_POSTPURCHASE_LIVE=1 but RESEND_API_KEY unset — refusing to send")
            return 0

        mode = "LIVE" if LIVE else "DRY_RUN"

        s_considered, s_sent, s_failed, s_skipped = sweep_shipping(conn, api_key)
        r_considered, r_sent, r_failed, r_skipped = sweep_review(conn, api_key)

        print(
            f"[post-purchase] mode={mode} "
            f"shipping(considered={s_considered} sent={s_sent} dry={s_skipped} failed={s_failed}) "
            f"review(considered={r_considered} sent={r_sent} dry={r_skipped} failed={r_failed})"
        )
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
