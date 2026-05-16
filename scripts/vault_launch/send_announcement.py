#!/usr/bin/env python3
"""Send the vault launch announcement to all unique mu_purchases.email via Resend.

CRITICAL safety per memory feedback_email_blast_radius.md:
  - Always confirm dry_run results before sending production
  - Default behavior is --dry-run; --send required to actually mail

Usage:
  # 1. Preview the recipient list + sample render
  python3 scripts/vault_launch/send_announcement.py

  # 2. Send to a single test address
  python3 scripts/vault_launch/send_announcement.py --send --only mail@yukihamada.jp

  # 3. Production blast (170 customers, ~¥0 cost on Resend free tier)
  python3 scripts/vault_launch/send_announcement.py --send --confirm
"""
import argparse, os, sys, time
from pathlib import Path

# Load env
_env = Path("/Users/yuki/.env")
if _env.exists():
    for ln in _env.read_text().splitlines():
        ln = ln.strip()
        if "=" in ln and not ln.startswith("#"):
            k, v = ln.split("=", 1)
            os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))

import requests, sqlite3

ROOT = Path(__file__).resolve().parent.parent.parent
SUBJ = (ROOT / "scripts" / "vault_launch" / "email_subject.txt").read_text().strip()
TXT  = (ROOT / "scripts" / "vault_launch" / "email_body.txt").read_text()
HTML = (ROOT / "scripts" / "vault_launch" / "email_body.html").read_text()

# Use local DB to enumerate recipients (matches prod since mu_purchases is the
# canonical buyer list — synced from Stripe via webhook).
DB = ROOT / "store" / "products.db"

RESEND_KEY = os.environ.get("RESEND_API_KEY")
FROM = "MU <info@wearmu.com>"
REPLY_TO = "info@wearmu.com"


def get_recipients(only=None):
    if only:
        return [only]
    db = sqlite3.connect(DB)
    rows = db.execute(
        "SELECT DISTINCT email FROM mu_purchases WHERE email IS NOT NULL AND email != '' "
        "AND email NOT LIKE '%@example.com' AND email NOT LIKE 'test%'"
    ).fetchall()
    return sorted({r[0] for r in rows})


def render(email):
    name_or_san = email.split("@")[0] + "" if "@" in email else email
    return (
        SUBJ,
        TXT.replace("{{name_or_san}}", name_or_san),
        HTML.replace("{{name_or_san}}", name_or_san),
    )


def send_resend(to, subj, text, html):
    if not RESEND_KEY:
        sys.exit("RESEND_API_KEY not set")
    r = requests.post(
        "https://api.resend.com/emails",
        headers={"Authorization": f"Bearer {RESEND_KEY}",
                 "Content-Type": "application/json"},
        json={
            "from": FROM, "to": [to], "reply_to": REPLY_TO,
            "subject": subj, "text": text, "html": html,
        },
        timeout=30,
    )
    return r.status_code, r.text


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--send", action="store_true", help="actually send (default: dry-run)")
    ap.add_argument("--confirm", action="store_true",
                    help="required for production blast (>1 recipient)")
    ap.add_argument("--only", help="restrict to single email")
    ap.add_argument("--limit", type=int, help="cap recipients (for staged rollout)")
    args = ap.parse_args()

    recipients = get_recipients(args.only)
    if args.limit:
        recipients = recipients[: args.limit]

    print(f"Recipients: {len(recipients)}")
    if len(recipients) <= 10:
        for e in recipients: print(f"  - {e}")
    else:
        for e in recipients[:5]: print(f"  - {e}")
        print(f"  ... +{len(recipients)-5} more")

    subj0, text0, html0 = render(recipients[0] if recipients else "test@example.com")
    print(f"\nSubject: {subj0}")
    print(f"---text preview (first 6 lines)---")
    print("\n".join(text0.splitlines()[:6]))

    if not args.send:
        print("\n(dry-run) — pass --send to actually mail")
        return

    if len(recipients) > 1 and not args.confirm:
        sys.exit("\nProduction blast requires --confirm. Re-run with: --send --confirm")

    print(f"\n→ Sending {len(recipients)} emails via Resend...")
    ok, fail = 0, 0
    for i, e in enumerate(recipients, 1):
        subj, text, html = render(e)
        code, msg = send_resend(e, subj, text, html)
        if 200 <= code < 300:
            ok += 1
            print(f"  ✓ {i}/{len(recipients)} {e}")
        else:
            fail += 1
            print(f"  ✗ {i}/{len(recipients)} {e}: {code} {msg[:120]}")
        time.sleep(0.5)  # rate-limit gentleness

    print(f"\n📊 sent: {ok}, failed: {fail}")


if __name__ == "__main__":
    main()
