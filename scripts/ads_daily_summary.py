#!/usr/bin/env python3
"""Daily summary across all Google Ads accounts → Telegram + stdout.

Usage:
  python3 scripts/ads_daily_summary.py [--period today|yesterday|7d]

Default: 'today'.
"""
import os, sys, json, urllib.request, urllib.parse
from pathlib import Path
from datetime import datetime, timedelta

for ln in Path("/Users/yuki/.env").read_text().splitlines():
    if "=" in ln and not ln.startswith("#"):
        k, v = ln.split("=", 1)
        os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))

import yaml as _y
from google.ads.googleads.client import GoogleAdsClient

ACCTS = [
    ("4070111170", "JiuFlow"),
    ("5408218744", "BANTO"),
    ("8516735301", "misebanai"),
    ("9591303572", "MU"),
]
TG_TOKEN = os.environ.get("TELEGRAM_BOT_TOKEN", "")
TG_CHAT = "1136442501"

PERIOD = sys.argv[1] if len(sys.argv) > 1 else "today"
DATE_FILTER = {
    "today": "DURING TODAY",
    "yesterday": "DURING YESTERDAY",
    "7d": "DURING LAST_7_DAYS",
}.get(PERIOD, "DURING TODAY")


def tg(msg: str):
    if not TG_TOKEN: return
    try:
        urllib.request.urlopen(
            f"https://api.telegram.org/bot{TG_TOKEN}/sendMessage",
            data=urllib.parse.urlencode({"chat_id": TG_CHAT, "text": msg}).encode(),
            timeout=15,
        )
    except Exception as e:
        print(f"[tg-err] {e}")


def main():
    base = _y.safe_load(open(str(Path.home()/".config/google-ads/google-ads.yaml")))
    lines = [f"📊 MU Ads {PERIOD} @ {datetime.now().strftime('%Y-%m-%d %H:%M')}"]
    grand = {"cost": 0, "conv": 0, "val": 0}
    alerts = []

    for cid, label in ACCTS:
        cfg = dict(base); cfg["login_customer_id"] = cid
        c = GoogleAdsClient.load_from_dict(cfg, version="v22")
        svc = c.get_service("GoogleAdsService")

        # Account totals
        a_cost = a_conv = a_val = a_impr = a_clk = 0
        for r in svc.search(customer_id=cid, query=f"SELECT metrics.impressions, metrics.clicks, metrics.cost_micros, metrics.conversions, metrics.conversions_value FROM customer WHERE segments.date {DATE_FILTER}"):
            a_impr = r.metrics.impressions
            a_clk = r.metrics.clicks
            a_cost = r.metrics.cost_micros / 1e6
            a_conv = r.metrics.conversions
            a_val = r.metrics.conversions_value

        grand["cost"] += a_cost
        grand["conv"] += a_conv
        grand["val"] += a_val
        roas = (a_val / a_cost) if a_cost > 0 else 0
        cpa = (a_cost / a_conv) if a_conv > 0 else 0

        if a_cost > 0 or a_conv > 0:
            mark = "🟢" if roas >= 1.0 else ("🟡" if a_conv > 0 else "🔴")
            lines.append(f"\n{mark} {label}")
            lines.append(f"  impr {a_impr:,} | clk {a_clk} | cost ¥{a_cost:,.0f}")
            lines.append(f"  conv {a_conv:.1f} | val ¥{a_val:,.0f}")
            lines.append(f"  ROAS {roas:.2f}x | CPA ¥{cpa:,.0f}")

            # Per-campaign breakdown (only converters)
            for r in svc.search(customer_id=cid, query=f"SELECT campaign.name, metrics.cost_micros, metrics.conversions, metrics.conversions_value FROM campaign WHERE campaign.status='ENABLED' AND segments.date {DATE_FILTER} AND metrics.cost_micros > 1000000 ORDER BY metrics.cost_micros DESC LIMIT 5"):
                c_cost = r.metrics.cost_micros/1e6
                c_conv = r.metrics.conversions
                c_val = r.metrics.conversions_value
                c_roas = (c_val/c_cost) if c_cost > 0 else 0
                c_flag = "🟢" if c_roas >= 1.0 else ("🟡" if c_conv > 0 else "🔴")
                lines.append(f"    {c_flag} {r.campaign.name[:30]}: ¥{c_cost:,.0f} → {c_conv:.0f}c ROAS {c_roas:.2f}x")
                if c_cost > 5000 and c_conv == 0:
                    alerts.append(f"⚠️ {label}/{r.campaign.name[:25]}: ¥{c_cost:,.0f} 0conv")
        elif a_cost == 0 and a_conv == 0:
            lines.append(f"\n⚫ {label}: dormant")

    g_roas = (grand["val"] / grand["cost"]) if grand["cost"] > 0 else 0
    lines.append(f"\n━━━━━━━━━━━━━━━━━━━━")
    lines.append(f"TOTAL: ¥{grand['cost']:,.0f} → {grand['conv']:.0f}c ¥{grand['val']:,.0f}")
    lines.append(f"ROAS {g_roas:.2f}x")
    if alerts:
        lines.append(f"\n🚨 Alerts:")
        for a in alerts[:5]: lines.append(f"  {a}")

    msg = "\n".join(lines)
    print(msg)
    tg(msg)


if __name__ == "__main__":
    main()
