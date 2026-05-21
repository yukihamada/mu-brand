#!/usr/bin/env python3
"""sales_tracker_100k — track MU revenue toward the ¥100,000 goal.

Reads live state from `products.db`:
  SELECT SUM(sold * price_jpy) FROM products WHERE active=1

… plus, where available, paid `cart_abandons` rows (the store backend
inserts these from Stripe webhooks). Never invents numbers — if the DB
returns zero, the log says zero.

Behaviors:
  - 1 record per run appended to logs/sales_100k.jsonl (state snapshot)
  - Telegram alert at every NEW sold-order (compares against the last
    snapshot — `delta_units > 0` triggers).
  - Hourly progress digest (only on the :00 run by default, or when
    PROGRESS_EVERY_RUN=1 is set).
  - Single "GOAL HIT" message when accrued revenue first crosses
    ¥100,000. Also writes logs/sales_100k.goal to mark the threshold so
    it never fires twice.
  - Emits a cadence-suggestion line on stdout ("consider product_creator
    cadence_hours=1") when goal reached — for the human to act on.

NEVER fakes sold counts. NEVER outputs TELEGRAM_BOT_TOKEN.

Cron entry (added by cron.sh):
  0 * * * * $PYTHON $SCRIPT_DIR/scripts/sales_tracker_100k.py
"""
from __future__ import annotations

import json
import os
import sqlite3
import sys
import time
import urllib.parse
import urllib.request
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DB_PATH = Path(os.environ.get("MU_DB", str(ROOT / "products.db")))
LOG_DIR = ROOT / "logs"
LOG_FILE = LOG_DIR / "sales_100k.jsonl"
GOAL_MARKER = LOG_DIR / "sales_100k.goal"
STATE_FILE = LOG_DIR / "sales_100k.state.json"

GOAL_JPY = int(os.environ.get("MU_SALES_GOAL_JPY", "100000"))


# ────────────────────────────────────────────────────────────────────────
# Env load
# ────────────────────────────────────────────────────────────────────────

def _load_env_file() -> None:
    env_path = Path("/Users/yuki/.env")
    if not env_path.exists():
        return
    try:
        for ln in env_path.read_text().splitlines():
            ln = ln.strip()
            if "=" in ln and not ln.startswith("#"):
                k, v = ln.split("=", 1)
                os.environ.setdefault(k.strip(),
                                      v.strip().strip('"').strip("'"))
    except Exception:
        pass


_load_env_file()


# ────────────────────────────────────────────────────────────────────────
# Telegram (token never logged)
# ────────────────────────────────────────────────────────────────────────

def telegram(msg: str) -> bool:
    token = os.environ.get("TELEGRAM_BOT_TOKEN")
    chat_id = os.environ.get("TELEGRAM_CHAT_ID") or "1136442501"
    if not token:
        return False
    try:
        data = urllib.parse.urlencode({
            "chat_id": chat_id,
            "text": msg[:3500],
            "disable_web_page_preview": "true",
        }).encode("utf-8")
        req = urllib.request.Request(
            f"https://api.telegram.org/bot{token}/sendMessage",
            data=data, method="POST",
        )
        urllib.request.urlopen(req, timeout=10).read()
        return True
    except Exception:
        return False


# ────────────────────────────────────────────────────────────────────────
# DB read
# ────────────────────────────────────────────────────────────────────────

def read_state() -> dict:
    con = sqlite3.connect(DB_PATH, timeout=10)
    try:
        # Primary: products.sold × price
        row = con.execute(
            "SELECT COALESCE(SUM(sold * price_jpy), 0), "
            "       COALESCE(SUM(sold), 0), "
            "       COUNT(*) "
            "FROM products WHERE active=1"
        ).fetchone()
        revenue_active = int(row[0] or 0)
        units_active = int(row[1] or 0)
        active_count = int(row[2] or 0)

        # Plus full inventory revenue (incl. inactive) — diagnostic only.
        row2 = con.execute(
            "SELECT COALESCE(SUM(sold * price_jpy), 0), "
            "       COALESCE(SUM(sold), 0) FROM products"
        ).fetchone()
        revenue_all = int(row2[0] or 0)
        units_all = int(row2[1] or 0)

        # cart_abandons paid rows — present on Fly DB, often absent locally.
        paid_count = 0
        paid_revenue = 0
        last_paid_at: str | None = None
        try:
            exists = con.execute(
                "SELECT name FROM sqlite_master WHERE type='table' "
                "AND name='cart_abandons'"
            ).fetchone()
            if exists:
                cols = {c[1] for c in con.execute(
                    "PRAGMA table_info(cart_abandons)")}
                if "paid_at" in cols:
                    pr = con.execute(
                        "SELECT COUNT(*), COALESCE(SUM(amount_jpy),0), "
                        "MAX(paid_at) FROM cart_abandons WHERE paid_at IS NOT NULL"
                    ).fetchone()
                    paid_count = int(pr[0] or 0)
                    paid_revenue = int(pr[1] or 0)
                    last_paid_at = pr[2]
        except Exception as e:
            sys.stderr.write(
                f"[sales_tracker_100k] cart_abandons read: {e}\n")

        # Last 5 sold products (for digest)
        recent_sold = [
            dict(id=r[0], serial=r[1], sold=r[2], price=r[3])
            for r in con.execute(
                "SELECT id, serial_code, sold, price_jpy FROM products "
                "WHERE sold > 0 ORDER BY id DESC LIMIT 5"
            )
        ]
    finally:
        con.close()

    return {
        "revenue_active_jpy": revenue_active,
        "units_active": units_active,
        "active_count": active_count,
        "revenue_all_jpy": revenue_all,
        "units_all": units_all,
        "paid_count": paid_count,
        "paid_revenue_jpy": paid_revenue,
        "last_paid_at": last_paid_at,
        "recent_sold": recent_sold,
    }


# ────────────────────────────────────────────────────────────────────────
# State diff
# ────────────────────────────────────────────────────────────────────────

def load_prev_state() -> dict:
    if not STATE_FILE.exists():
        return {}
    try:
        return json.loads(STATE_FILE.read_text())
    except Exception:
        return {}


def save_state(state: dict) -> None:
    try:
        LOG_DIR.mkdir(parents=True, exist_ok=True)
        STATE_FILE.write_text(json.dumps(state, ensure_ascii=False, indent=2))
    except Exception as e:
        sys.stderr.write(f"[sales_tracker_100k] state save failed: {e}\n")


# ────────────────────────────────────────────────────────────────────────
# Main
# ────────────────────────────────────────────────────────────────────────

def write_log(record: dict) -> None:
    try:
        LOG_DIR.mkdir(parents=True, exist_ok=True)
        with LOG_FILE.open("a", encoding="utf-8") as f:
            f.write(json.dumps(record, ensure_ascii=False) + "\n")
    except Exception as e:
        sys.stderr.write(f"[sales_tracker_100k] log write failed: {e}\n")


def goal_already_hit() -> bool:
    return GOAL_MARKER.exists()


def mark_goal_hit(record: dict) -> None:
    try:
        GOAL_MARKER.write_text(json.dumps(record, ensure_ascii=False, indent=2))
    except Exception as e:
        sys.stderr.write(f"[sales_tracker_100k] goal marker write failed: {e}\n")


def is_top_of_hour() -> bool:
    return datetime.now().minute < 5  # cron typically launches at :00


def run_once(dry_run: bool = False, force_digest: bool = False) -> dict:
    started = time.time()
    ts = datetime.now().isoformat(timespec="seconds")
    record: dict = {"ts": ts, "dry_run": dry_run, "goal_jpy": GOAL_JPY}

    try:
        state = read_state()
    except Exception as e:
        record["ok"] = False
        record["error"] = f"{type(e).__name__}: {e}"
        write_log(record)
        print(f"{ts} ERROR read_state: {record['error']}")
        return record

    record.update(state)
    revenue = state["revenue_active_jpy"]
    record["pct_of_goal"] = round((revenue / GOAL_JPY) * 100, 1) if GOAL_JPY else 0
    record["remaining_jpy"] = max(GOAL_JPY - revenue, 0)
    record["ok"] = True

    # Diff vs last run
    prev = load_prev_state()
    prev_units = int(prev.get("units_active", 0))
    prev_revenue = int(prev.get("revenue_active_jpy", 0))
    prev_paid_count = int(prev.get("paid_count", 0))

    record["delta_units"] = state["units_active"] - prev_units
    record["delta_revenue_jpy"] = revenue - prev_revenue
    record["delta_paid_count"] = state["paid_count"] - prev_paid_count

    alerts: list[str] = []
    # New paid order — instant alert
    if (record["delta_units"] > 0) or (record["delta_paid_count"] > 0):
        rs = state["recent_sold"][:3]
        rs_str = ", ".join(
            f"#{r['id']} {r['serial']} ×{r['sold']}" for r in rs
        ) or "—"
        alerts.append(
            f"[mu sales] +{record['delta_units']} unit, "
            f"+¥{record['delta_revenue_jpy']:,}. "
            f"Total ¥{revenue:,} / ¥{GOAL_JPY:,} ({record['pct_of_goal']}%). "
            f"recent: {rs_str}"
        )

    # Hourly digest — top-of-hour or forced
    if (is_top_of_hour() or force_digest) and not alerts:
        alerts.append(
            f"[mu sales digest] ¥{revenue:,} / ¥{GOAL_JPY:,} "
            f"({record['pct_of_goal']}%) | units={state['units_active']} "
            f"active_skus={state['active_count']} "
            f"paid_orders={state['paid_count']}"
        )

    # Goal hit (one-shot)
    cadence_suggest: str | None = None
    if revenue >= GOAL_JPY and not goal_already_hit():
        msg = (
            f"GOAL HIT ¥{GOAL_JPY:,} reached. Revenue ¥{revenue:,}, "
            f"units {state['units_active']}, "
            f"paid_orders {state['paid_count']}. "
            f"Consider raising product_creator_agent cadence_hours -> 1 "
            f"(faster drops)."
        )
        alerts.append("[mu sales]  " + msg)
        cadence_suggest = "product_creator_agent cadence_hours=1"
        record["goal_hit"] = True
        mark_goal_hit(record)

    record["alerts"] = alerts
    record["cadence_suggest"] = cadence_suggest

    if alerts and not dry_run:
        for a in alerts:
            telegram(a)

    # Persist state for next run
    save_state(state)
    write_log(record)
    record["duration_s"] = round(time.time() - started, 3)

    print(
        f"{ts} revenue=¥{revenue:,}/¥{GOAL_JPY:,} ({record['pct_of_goal']}%) "
        f"units={state['units_active']} Δ+{record['delta_units']} "
        f"paid={state['paid_count']} Δ+{record['delta_paid_count']} "
        f"alerts={len(alerts)}"
        + (f"  → {cadence_suggest}" if cadence_suggest else "")
    )
    return record


def main(argv: list[str] | None = None) -> int:
    import argparse
    p = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    p.add_argument("--dry-run", action="store_true",
                   help="Skip Telegram send.")
    p.add_argument("--digest", action="store_true",
                   help="Force the hourly digest line even off the hour.")
    args = p.parse_args(argv)
    run_once(dry_run=args.dry_run, force_digest=args.digest)
    return 0


if __name__ == "__main__":
    sys.exit(main())
