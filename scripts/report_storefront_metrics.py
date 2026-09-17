"""Read-only trend report from metrics-history.jsonl.

Usage: python3 scripts/report_storefront_metrics.py
"""
import json
from pathlib import Path

HISTORY = Path(__file__).resolve().parents[1] / "store/target/storefront-preview/metrics-history.jsonl"


def load():
    if not HISTORY.exists():
        return []
    return [json.loads(line) for line in HISTORY.read_text().splitlines() if line.strip()]


def main():
    rows = load()
    if not rows:
        print("no observations yet")
        return
    print(f"observations: {len(rows)}  ({rows[0]['observed_at']} → {rows[-1]['observed_at']})")
    header = f"{'observed':>26} {'land':>5} {'pdp':>4} {'intent':>6} {'pv%':>6} {'pv24':>5} {'cta24':>6} {'start24':>7} {'paid24':>6} {'orders':>6}"
    print(header)
    for r in rows:
        c = r["browser_cohort_14d"]
        e = r["events_24h"]
        orders = sum(v["count"] for v in r["orders_24h"].values())
        print(f"{r['observed_at']:>26} {c['landing_sessions']:>5} {c['product_view_sessions']:>4} "
              f"{c['checkout_intent_sessions']:>6} {c['product_view_pct']:>6} {e.get('pageview',0):>5} "
              f"{e.get('cta_click',0):>6} {e.get('checkout_start',0):>7} {e.get('checkout_paid',0):>6} {orders:>6}")
    first, last = rows[0], rows[-1]
    print("\nDelta first → last (24h event counts only):")
    for key in ("pageview", "cta_click", "checkout_start", "checkout_paid"):
        a = first["events_24h"].get(key, 0)
        b = last["events_24h"].get(key, 0)
        print(f"  {key:<16} {a:>6} → {b:>6}  ({b - a:+d})")
    print("\nNotes: browser_cohort_14d is a 14-day rolling window of the release cohort "
          "(pageview on / with ab=storefront-20260917), not purchase CVR. "
          "events_24h counts are event rows, not unique customers or net revenue. "
          "checkout_paid includes server-synthesised events with unrelated identities.")


if __name__ == "__main__":
    main()
