#!/usr/bin/env python3
"""Poll Google Ads + craft.wearmu.com stats, log to a file.

Runs in background. Each tick (default 15 min) writes:
  - Ads metrics for MU-CRAFT campaign (impressions, clicks, cost, CTR)
  - /api/stats from craft.wearmu.com (skus_total, last_24h, users)
  - Health check (HTTP 200)
  - Diff vs previous tick

Usage:
  nohup python3 scripts/monitor_craft_ads.py \
      --interval 900 --duration 86400 \
      --log /tmp/craft_ads_monitor.log &
"""
from __future__ import annotations
import argparse
import json
import os
import sys
import time
import urllib.request
from datetime import datetime
from pathlib import Path

for ln in Path("/Users/yuki/.env").read_text().splitlines():
    if "=" in ln and not ln.startswith("#"):
        k, v = ln.split("=", 1)
        os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))

from google.ads.googleads.client import GoogleAdsClient

CUSTOMER_ID = "9591303572"
CAMPAIGN_ID = "23862045045"
YAML = str(Path.home() / ".config" / "google-ads" / "google-ads.yaml")
STATS_URL = "https://craft.wearmu.com/api/stats"
HEALTH_URL = "https://craft.wearmu.com/healthz"
GALLERY_URL = "https://craft.wearmu.com/gallery"


def ads_snapshot(client: GoogleAdsClient) -> dict:
    svc = client.get_service("GoogleAdsService")
    q = f"""
        SELECT
          campaign.id, campaign.status, campaign.serving_status,
          metrics.impressions, metrics.clicks, metrics.cost_micros,
          metrics.ctr, metrics.average_cpc, metrics.conversions
        FROM campaign
        WHERE campaign.id = {CAMPAIGN_ID}
          AND segments.date DURING LAST_7_DAYS
    """
    try:
        rows = list(svc.search(customer_id=CUSTOMER_ID, query=q))
        if not rows:
            return {"status": "no_data_yet"}
        r = rows[0]
        return {
            "status": r.campaign.status.name,
            "serving": r.campaign.serving_status.name,
            "impressions": int(r.metrics.impressions),
            "clicks": int(r.metrics.clicks),
            "cost_yen": round(r.metrics.cost_micros / 1_000_000, 1),
            "ctr_pct": round(r.metrics.ctr * 100, 2),
            "avg_cpc_yen": round(r.metrics.average_cpc / 1_000_000, 1),
            "conversions": float(r.metrics.conversions),
        }
    except Exception as e:
        return {"error": str(e)[:120]}


def fetch_json(url: str) -> dict:
    try:
        with urllib.request.urlopen(url, timeout=15) as r:
            return json.load(r)
    except Exception as e:
        return {"error": str(e)[:80]}


def check_health(url: str) -> int:
    try:
        with urllib.request.urlopen(url, timeout=15) as r:
            return r.status
    except Exception:
        return 0


def tick(client: GoogleAdsClient, prev: dict) -> dict:
    now = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    ads = ads_snapshot(client)
    stats = fetch_json(STATS_URL)
    health = check_health(HEALTH_URL)

    lines = [f"\n=== {now} ==="]
    lines.append(f"  health:  {health} (200 = OK)")
    lines.append(f"  ads:     {json.dumps(ads, ensure_ascii=False)}")
    lines.append(f"  stats:   {json.dumps(stats, ensure_ascii=False)}")

    if prev.get("stats") and stats.get("skus_total") is not None:
        d_sku = stats["skus_total"] - prev["stats"].get("skus_total", 0)
        d_users = stats["users_total"] - prev["stats"].get("users_total", 0)
        if d_sku or d_users:
            lines.append(f"  Δ:       +{d_sku} SKU, +{d_users} users since last tick")

    if prev.get("ads", {}).get("clicks") is not None and ads.get("clicks") is not None:
        d_click = ads["clicks"] - prev["ads"]["clicks"]
        d_impr = ads.get("impressions", 0) - prev["ads"].get("impressions", 0)
        if d_click or d_impr:
            lines.append(f"  ad Δ:    +{d_impr} impr, +{d_click} clicks")

    return {"text": "\n".join(lines), "ads": ads, "stats": stats}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--interval", type=int, default=900, help="seconds between ticks")
    ap.add_argument("--duration", type=int, default=86400, help="total run seconds")
    ap.add_argument("--log", default="/tmp/craft_ads_monitor.log")
    args = ap.parse_args()

    client = GoogleAdsClient.load_from_storage(YAML, version="v22")
    deadline = time.time() + args.duration
    prev: dict = {}
    log_path = Path(args.log)

    header = (
        f"\n## MU CRAFT ads monitor — started {datetime.now().isoformat()}\n"
        f"   campaign: {CAMPAIGN_ID}  customer: {CUSTOMER_ID}\n"
        f"   interval: {args.interval}s   duration: {args.duration}s\n"
        f"   log:      {log_path}\n"
    )
    log_path.write_text(header) if not log_path.exists() else log_path.open("a").write(header)
    print(header)

    while time.time() < deadline:
        try:
            cur = tick(client, prev)
            log_path.open("a").write(cur["text"] + "\n")
            print(cur["text"])
            prev = cur
        except Exception as e:
            log_path.open("a").write(f"\n  TICK ERR: {e}\n")
        time.sleep(args.interval)

    return 0


if __name__ == "__main__":
    sys.exit(main())
