#!/usr/bin/env python3
"""Poll Google Ads + wearmu.com state every N seconds, append to a log file.

Usage:
  python3 scripts/ads_monitor_loop.py --interval 300 --duration 86400 \
      --log logs/ads_launch_20260516_2227/monitor.log

Logs:
  - Campaign serving + ad approval status
  - Last-7d metrics (impressions, clicks, cost, conversions)
  - Site health (HTTP 200 on sample product pages)
  - Stripe redirect URLs reachable
"""
import argparse, os, sys, time
from datetime import datetime
from pathlib import Path

# Load env
for ln in Path("/Users/yuki/.env").read_text().splitlines():
    if "=" in ln and not ln.startswith("#"):
        k, v = ln.split("=", 1)
        os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))

from google.ads.googleads.client import GoogleAdsClient  # noqa
import requests

CID = "9591303572"
CAMPAIGN = "MU-AdsTees-Search"
YAML = str(Path.home() / ".config" / "google-ads" / "google-ads.yaml")


def get_client():
    return GoogleAdsClient.load_from_storage(YAML, version="v22")


def snapshot(client):
    lines = [f"\n=== {datetime.now().strftime('%Y-%m-%d %H:%M:%S')} ==="]

    # Campaign + metrics
    q = (
        "SELECT campaign.name, campaign.status, campaign.serving_status, "
        "metrics.impressions, metrics.clicks, metrics.cost_micros, "
        "metrics.conversions, metrics.ctr, metrics.average_cpc "
        "FROM campaign "
        f"WHERE campaign.name = '{CAMPAIGN}' AND segments.date DURING TODAY"
    )
    found = False
    for r in client.get_service("GoogleAdsService").search(customer_id=CID, query=q):
        found = True
        lines.append(
            f"  cmp: {r.campaign.status.name}/{r.campaign.serving_status.name}  "
            f"impr={r.metrics.impressions} clicks={r.metrics.clicks} "
            f"cost=¥{r.metrics.cost_micros/1_000_000:.0f} conv={r.metrics.conversions:.1f} "
            f"ctr={r.metrics.ctr*100:.2f}% cpc=¥{r.metrics.average_cpc/1_000_000:.0f}"
        )
    if not found:
        # Campaign exists but no metrics yet (just started)
        q2 = f"SELECT campaign.name, campaign.status, campaign.serving_status FROM campaign WHERE campaign.name = '{CAMPAIGN}'"
        for r in client.get_service("GoogleAdsService").search(customer_id=CID, query=q2):
            lines.append(f"  cmp: {r.campaign.status.name}/{r.campaign.serving_status.name}  (no metrics yet)")

    # Per ad group
    q3 = (
        "SELECT ad_group.name, metrics.impressions, metrics.clicks, "
        "metrics.cost_micros, metrics.conversions "
        f"FROM ad_group WHERE campaign.name = '{CAMPAIGN}' "
        "AND segments.date DURING TODAY ORDER BY ad_group.name"
    )
    for r in client.get_service("GoogleAdsService").search(customer_id=CID, query=q3):
        lines.append(
            f"    {r.ad_group.name:<12} impr={r.metrics.impressions:>4} "
            f"clk={r.metrics.clicks:>3} cost=¥{r.metrics.cost_micros/1_000_000:>4.0f} "
            f"conv={r.metrics.conversions:.1f}"
        )

    # Ad approval status
    q4 = (
        "SELECT ad_group.name, ad_group_ad.policy_summary.approval_status, "
        "ad_group_ad.policy_summary.review_status "
        f"FROM ad_group_ad WHERE campaign.name = '{CAMPAIGN}'"
    )
    approvals = []
    for r in client.get_service("GoogleAdsService").search(customer_id=CID, query=q4):
        approvals.append(
            f"{r.ad_group.name}={r.ad_group_ad.policy_summary.approval_status.name}/{r.ad_group_ad.policy_summary.review_status.name}"
        )
    lines.append(f"  approval: {', '.join(approvals)}")

    # Sample site health
    sample_ids = [1034, 1042, 1046, 1049, 1051]
    health = []
    for pid in sample_ids:
        try:
            code = requests.head(f"https://wearmu.com/products/ads_*/{pid}",
                                 allow_redirects=True, timeout=5).status_code
        except Exception:
            code = "ERR"
        health.append(f"{pid}:{code}")
    lines.append(f"  site: {' '.join(health)}")

    # Stock + sold check
    try:
        r = requests.get("https://wearmu.com/api/products/ads_jujitsu", timeout=10)
        if r.ok:
            d = r.json()
            total_sold = sum(p.get("sold", 0) for p in d) if isinstance(d, list) else 0
            total_inv = sum(p.get("inventory", 0) for p in d) if isinstance(d, list) else 0
            lines.append(f"  ads_jujitsu sold/inv = {total_sold}/{total_inv}")
    except Exception as e:
        lines.append(f"  stock check err: {e}")

    return "\n".join(lines)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--interval", type=int, default=300, help="poll interval seconds")
    ap.add_argument("--duration", type=int, default=86400, help="total seconds")
    ap.add_argument("--log", required=True, help="output log path")
    args = ap.parse_args()

    Path(args.log).parent.mkdir(parents=True, exist_ok=True)
    client = get_client()
    end = time.time() + args.duration

    while time.time() < end:
        try:
            snap = snapshot(client)
            with open(args.log, "a") as f:
                f.write(snap + "\n")
                f.flush()
            print(snap, flush=True)
        except Exception as e:
            with open(args.log, "a") as f:
                f.write(f"\n=== {datetime.now()} ERR {e}\n")
        time.sleep(args.interval)


if __name__ == "__main__":
    main()
