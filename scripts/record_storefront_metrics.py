"""Append one read-only production observation to metrics-history.jsonl.

Usage: python3 scripts/record_storefront_metrics.py
No writes to production; only Fly read-only sqlite queries.
"""
import json
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "store/target/storefront-preview"
OUT.mkdir(exist_ok=True)

QUERIES = {
    "browser_cohort": Path(ROOT / "scripts/storefront_metrics.sql").read_text(),
    "events_since": "SELECT event,COUNT(*) FROM funnel_events WHERE CAST(created_at AS INTEGER)>{since} GROUP BY event;",
    "orders_since": "SELECT status,COUNT(*),COALESCE(SUM(amount_jpy),0) FROM catalog_orders WHERE CAST(strftime('%s',created_at) AS INTEGER)>{since} GROUP BY status;",
}


def run(sql: str) -> str:
    # stdin keeps multi-line SQL out of the remote shell quoting layer.
    result = subprocess.run(
        ["fly", "ssh", "console", "-a", "mu-store", "-C",
         "sqlite3 -readonly /data/products.db"],
        input=sql, capture_output=True, text=True, check=True)
    return result.stdout.strip()


def rows(text: str):
    return [line.split("|") for line in text.splitlines() if line.strip()]


def main():
    now = int(time.time())
    since = now - 24 * 3600
    cohort = rows(run(QUERIES["browser_cohort"]))
    events = {r[0]: int(r[1]) for r in rows(run(QUERIES["events_since"].format(since=since)))}
    orders = {r[0]: {"count": int(r[1]), "amount_jpy": int(r[2])}
              for r in rows(run(QUERIES["orders_since"].format(since=since)))}
    record = {
        "observed_at": time.strftime("%Y-%m-%dT%H:%M:%S%z", time.localtime(now)),
        "window_hours": 24,
        "browser_cohort_14d": {
            "landing_sessions": int(cohort[0][0]) if cohort else 0,
            "product_view_sessions": int(cohort[0][1]) if cohort else 0,
            "checkout_intent_sessions": int(cohort[0][2]) if cohort else 0,
            "product_view_pct": float(cohort[0][3]) if cohort else 0.0,
            "checkout_intent_pct": float(cohort[0][4]) if cohort else 0.0,
        },
        "events_24h": events,
        "orders_24h": orders,
    }
    with (OUT / "metrics-history.jsonl").open("a") as fh:
        fh.write(json.dumps(record, ensure_ascii=False) + "\n")
    print(json.dumps(record, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
