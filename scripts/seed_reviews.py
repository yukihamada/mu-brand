#!/usr/bin/env python3
"""Seed *legitimate* customer reviews into wearmu.com via POST /api/admin/reviews.

Why this exists
---------------
The `reviews` table backs the star ratings + schema.org AggregateRating JSON-LD
on every /p/<sku> page. An empty table means no rich-snippet stars in Google
results, which kills the SEO upside of having the schema in the first place.

This script lets the operator dump *real* reviews (founder usage notes, opt-in
customer feedback, verified press quotes) declaratively in
`scripts/seed_reviews.yaml`, then ship them in one idempotent batch.

Hard rules (mirrored in seed_reviews.yaml docstring)
----------------------------------------------------
* REAL REVIEWS ONLY. Fake reviews are a hard NO — Google penalises them
  and the user has explicitly forbidden fabrication. Even a "founder
  initial impression" must reflect actual wear/use.
* IDEMPOTENT. Same (product_id, reviewer_name, rating) tuple is skipped on
  re-run, so the file can be safely committed and re-applied.
* DRY-RUN BY DEFAULT. You must pass `--commit` to actually POST. The dry-run
  output mirrors what would be sent, minus the admin token.
* NEVER LOG THE ADMIN TOKEN. The token is read from `~/.env`
  (`MU_ADMIN_TOKEN=…`) and only sent in the `Authorization: Bearer …`
  header — never printed.

Resolution of `product_sku`
---------------------------
If `product_sku` is a string, the script resolves it to `product_id` via
`SELECT id FROM products WHERE serial_code=?` against `store/products.db`.
If it is an integer, it is used as `product_id` directly.

Exit code is always 0 unless argparse itself bails — per-entry failures are
logged on a single line each and the loop continues, so a typo in one entry
doesn't block the rest.
"""

from __future__ import annotations

import argparse
import json
import os
import sqlite3
import sys
import urllib.error
import urllib.request
from pathlib import Path

try:
    import yaml  # type: ignore
except ImportError:
    sys.stderr.write(
        "missing dep: pip install pyyaml  (or `python3 -m pip install --user pyyaml`)\n"
    )
    sys.exit(0)


REPO_ROOT = Path(__file__).resolve().parent.parent
YAML_PATH = REPO_ROOT / "scripts" / "seed_reviews.yaml"
PRODUCTS_DB = REPO_ROOT / "store" / "products.db"
ENV_PATH = Path.home() / ".env"
DEFAULT_BASE_URL = "https://wearmu.com"


def load_admin_token() -> str | None:
    """Read MU_ADMIN_TOKEN from environment or ~/.env. Never log the value."""
    tok = os.environ.get("MU_ADMIN_TOKEN")
    if tok:
        return tok.strip()
    if not ENV_PATH.is_file():
        return None
    try:
        for line in ENV_PATH.read_text(encoding="utf-8", errors="ignore").splitlines():
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            if line.startswith("MU_ADMIN_TOKEN="):
                return line.split("=", 1)[1].strip().strip('"').strip("'")
    except OSError:
        return None
    return None


def resolve_product_id(sku_or_id, conn: sqlite3.Connection) -> int | None:
    """Accept int (use as-is) or str (resolve via serial_code)."""
    if isinstance(sku_or_id, int) and sku_or_id > 0:
        return sku_or_id
    if isinstance(sku_or_id, str) and sku_or_id.strip():
        # Defence in depth: if the string is purely digits, treat as id.
        s = sku_or_id.strip()
        if s.isdigit():
            return int(s)
        row = conn.execute(
            "SELECT id FROM products WHERE serial_code=?", (s,)
        ).fetchone()
        if row:
            return int(row[0])
    return None


def already_seeded(conn: sqlite3.Connection, product_id: int, reviewer_name: str, rating: int) -> bool:
    """Idempotency check: same (product_id, reviewer_name, rating) already in db."""
    try:
        row = conn.execute(
            "SELECT 1 FROM reviews WHERE product_id=? AND COALESCE(reviewer_name,'')=? AND rating=? LIMIT 1",
            (product_id, reviewer_name or "", rating),
        ).fetchone()
        return row is not None
    except sqlite3.OperationalError:
        # Table doesn't exist yet — first run, nothing to skip.
        return False


def post_review(base_url: str, token: str, payload: dict) -> tuple[bool, str]:
    """POST a single review. Return (ok, short_message). Token is never logged."""
    url = base_url.rstrip("/") + "/api/admin/reviews"
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=data,
        method="POST",
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {token}",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=20) as resp:
            body = resp.read().decode("utf-8", errors="replace")[:300]
            return (200 <= resp.status < 300, f"{resp.status} {body}")
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", errors="replace")[:300] if e.fp else ""
        return (False, f"{e.code} {body}")
    except (urllib.error.URLError, TimeoutError, OSError) as e:
        return (False, f"net-err: {e}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument(
        "--commit", action="store_true",
        help="actually POST to /api/admin/reviews (default: dry-run)",
    )
    ap.add_argument(
        "--dry-run", action="store_true",
        help="explicit dry-run (default behaviour; kept for clarity)",
    )
    ap.add_argument(
        "--base-url", default=os.environ.get("BASE_URL", DEFAULT_BASE_URL),
        help=f"target host (default: {DEFAULT_BASE_URL} or $BASE_URL)",
    )
    ap.add_argument(
        "--yaml", default=str(YAML_PATH),
        help=f"yaml file with reviews (default: {YAML_PATH})",
    )
    args = ap.parse_args()

    commit = bool(args.commit) and not args.dry_run
    mode = "COMMIT" if commit else "DRY-RUN"

    yaml_path = Path(args.yaml)
    if not yaml_path.is_file():
        print(f"[seed_reviews] yaml not found: {yaml_path}")
        return 0

    try:
        doc = yaml.safe_load(yaml_path.read_text(encoding="utf-8")) or {}
    except yaml.YAMLError as e:
        print(f"[seed_reviews] yaml parse error: {e}")
        return 0

    entries = doc.get("reviews") or []
    if not isinstance(entries, list) or not entries:
        print(f"[seed_reviews] nothing to seed ({mode}; yaml has 0 entries)")
        return 0

    token = load_admin_token() if commit else None
    if commit and not token:
        print("[seed_reviews] MU_ADMIN_TOKEN not found in env or ~/.env — aborting commit")
        return 0

    if not PRODUCTS_DB.is_file():
        print(f"[seed_reviews] products.db not found at {PRODUCTS_DB}; sku resolution + dedupe disabled")
        conn = sqlite3.connect(":memory:")
    else:
        conn = sqlite3.connect(str(PRODUCTS_DB))

    posted = 0
    skipped = 0
    failed = 0
    planned = 0

    print(f"[seed_reviews] mode={mode} base={args.base_url} entries={len(entries)}")

    for i, entry in enumerate(entries):
        if not isinstance(entry, dict):
            print(f"  ! entry #{i}: not a dict, skipping")
            failed += 1
            continue

        sku_or_id = entry.get("product_sku") or entry.get("product_id")
        rating = entry.get("rating")
        body = (entry.get("body") or "").strip()
        reviewer_name = (entry.get("reviewer_name") or "").strip()
        verified = bool(entry.get("verified_purchase"))
        approved = entry.get("approved", True)
        approved = bool(approved) if approved is not None else True

        # Validate
        if sku_or_id is None:
            print(f"  ! entry #{i}: missing product_sku/product_id")
            failed += 1
            continue
        if not isinstance(rating, int) or not (1 <= rating <= 5):
            print(f"  ! entry #{i} ({sku_or_id}): rating must be int 1..5, got {rating!r}")
            failed += 1
            continue
        if not reviewer_name:
            print(f"  ! entry #{i} ({sku_or_id}): reviewer_name is required (legit-only policy)")
            failed += 1
            continue

        product_id = resolve_product_id(sku_or_id, conn)
        if product_id is None:
            print(f"  ! entry #{i}: cannot resolve product_sku/id {sku_or_id!r}")
            failed += 1
            continue

        if already_seeded(conn, product_id, reviewer_name, rating):
            print(f"  - skip dup: pid={product_id} name={reviewer_name!r} rating={rating}")
            skipped += 1
            continue

        payload = {
            "product_id": product_id,
            "rating": rating,
            "body": body,
            "reviewer_name": reviewer_name,
            "verified_purchase": verified,
            "approved": approved,
        }
        planned += 1

        if not commit:
            # Print exactly what would be sent (token-free).
            print(f"  + plan: POST /api/admin/reviews  payload={json.dumps(payload, ensure_ascii=False)}")
            continue

        ok, msg = post_review(args.base_url, token or "", payload)
        if ok:
            posted += 1
            print(f"  + posted: pid={product_id} rating={rating} name={reviewer_name!r}  ->  {msg}")
        else:
            failed += 1
            print(f"  ! failed: pid={product_id} rating={rating} name={reviewer_name!r}  ->  {msg}")

    if commit:
        print(f"[seed_reviews] done — posted={posted} skipped={skipped} failed={failed}")
    else:
        print(f"[seed_reviews] dry-run — planned={planned} skipped={skipped} failed={failed}  (use --commit to send)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
