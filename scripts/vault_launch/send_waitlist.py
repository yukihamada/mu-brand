#!/usr/bin/env python3
"""Send the vault-launch purchase-incentive email to /you free waitlist.

Targets: you_users WHERE lifetime_free=0 AND subscription_status IS NULL/blank
         AND unsubscribed_at IS NULL — i.e. "signed up, never paid, opted in"

Excludes: existing tee holders (they got DMs instead — see dm_5_holders.md)
          and unsubscribed users.

CRITICAL safety per memory feedback_email_blast_radius.md:
  - Default --dry-run; --send required to actually mail
  - --confirm required for production blast (>1 recipient)

Run on prod (or local with DB_PATH=/data/products.db via Fly SSH):

  flyctl ssh console -a mu-store
  cd /app && DB_PATH=/data/products.db RESEND_API_KEY=$RESEND_API_KEY \\
    python3 scripts/vault_launch/send_waitlist.py --send --confirm

  # Test locally first:
  python3 scripts/vault_launch/send_waitlist.py --send --only mail@yukihamada.jp
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
SUBJ = (ROOT / "scripts" / "vault_launch" / "waitlist_email_subject.txt").read_text().strip()
TXT  = (ROOT / "scripts" / "vault_launch" / "waitlist_email_body.txt").read_text()
HTML = (ROOT / "scripts" / "vault_launch" / "waitlist_email_body.html").read_text()

DB = Path(os.environ.get("DB_PATH") or (ROOT / "store" / "products.db"))
RESEND_KEY = os.environ.get("RESEND_API_KEY")
FROM = "MU <info@wearmu.com>"
REPLY_TO = "info@wearmu.com"


def get_recipients(only=None):
    """Free /you waitlist, not unsubscribed, not already a tee holder."""
    if only:
        return [only]
    db = sqlite3.connect(DB)
    # Free waitlist: lifetime_free=0 AND no active sub AND not unsubscribed.
    # Exclude anyone already in mu_purchases (they got a personal DM).
    rows = db.execute("""
        SELECT DISTINCT u.email FROM you_users u
        WHERE COALESCE(u.lifetime_free, 0) = 0
          AND (u.subscription_status IS NULL OR u.subscription_status = '')
          AND u.unsubscribed_at IS NULL
          AND u.email IS NOT NULL AND u.email != ''
          AND u.email NOT LIKE '%@example.com'
          AND u.email NOT LIKE 'test%'
          AND NOT EXISTS (
              SELECT 1 FROM mu_purchases p WHERE p.email = u.email
          )
    """).fetchall()
    return sorted({r[0] for r in rows})


def render(email):
    name_or_san = email.split("@")[0] if "@" in email else email
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
        json={"from": FROM, "to": [to], "reply_to": REPLY_TO,
              "subject": subj, "text": text, "html": html},
        timeout=30,
    )
    return r.status_code, r.text


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--send", action="store_true", help="actually send (default: dry-run)")
    ap.add_argument("--confirm", action="store_true",
                    help="required for production blast (>1 recipient)")
    ap.add_argument("--only", help="restrict to single email")
    ap.add_argument("--limit", type=int, help="cap recipients (staged rollout)")
    args = ap.parse_args()

    recipients = get_recipients(args.only)
    if args.limit:
        recipients = recipients[: args.limit]

    print(f"Recipients (free /you waitlist, not tee holders, not unsubscribed): {len(recipients)}")
    if len(recipients) <= 25:
        for e in recipients: print(f"  - {e}")
    else:
        for e in recipients[:5]: print(f"  - {e}")
        print(f"  ... +{len(recipients)-5} more")

    if not recipients:
        print("\n(nothing to send — check DB_PATH or you_users state)")
        return

    subj0, text0, html0 = render(recipients[0])
    print(f"\nSubject: {subj0}")
    print(f"---text preview (first 8 lines)---")
    print("\n".join(text0.splitlines()[:8]))

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
        time.sleep(0.5)

    print(f"\n📊 sent: {ok}, failed: {fail}")


if __name__ == "__main__":
    main()
