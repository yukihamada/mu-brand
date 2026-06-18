#!/usr/bin/env python3
"""Plan A monitor: poll MU-Brand + MU-CRAFT-OneClick every 5min for 1h.

Telegram pings on:
  - first impression after launch (per campaign)
  - first conversion after launch (per campaign)
  - HTTP non-200 from active landing URLs
"""
import os, sys, time, json, urllib.request, urllib.parse
from pathlib import Path
from datetime import datetime

for ln in Path("/Users/yuki/.env").read_text().splitlines():
    if "=" in ln and not ln.startswith("#"):
        k, v = ln.split("=", 1)
        os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))

from google.ads.googleads.client import GoogleAdsClient

CID = "9591303572"
CAMPAIGNS = ["MU-Brand", "MU-CRAFT-OneClick-2026-05"]
LANDINGS = ["https://wearmu.com/", "https://wearmu.com/about"]
TG_TOKEN = os.environ.get("TELEGRAM_BOT_TOKEN", "")
TG_CHAT = "1136442501"
LOG = Path("/Users/yuki/workspace/mu-brand/logs/plan_a_20260523/monitor.log")
LOG.parent.mkdir(parents=True, exist_ok=True)

STATE = {"first_impr": {}, "first_conv": {}}


def tg(msg):
    if not TG_TOKEN:
        return
    try:
        urllib.request.urlopen(
            f"https://api.telegram.org/bot{TG_TOKEN}/sendMessage",
            data=urllib.parse.urlencode({"chat_id": TG_CHAT, "text": msg}).encode(),
            timeout=10,
        )
    except Exception as e:
        log(f"[tg-err] {e}")


def log(s):
    line = f"{datetime.now().strftime('%H:%M:%S')} {s}"
    print(line, flush=True)
    with LOG.open("a") as f:
        f.write(line + "\n")


def snapshot(client):
    svc = client.get_service("GoogleAdsService")
    names = "','".join(CAMPAIGNS)
    q = (
        "SELECT campaign.name, metrics.impressions, metrics.clicks, "
        "metrics.cost_micros, metrics.conversions, metrics.conversions_value, "
        "metrics.search_impression_share "
        f"FROM campaign WHERE campaign.name IN ('{names}') "
        "AND segments.date DURING TODAY"
    )
    found = {}
    for r in svc.search(customer_id=CID, query=q):
        n = r.campaign.name
        m = r.metrics
        found[n] = {
            "impr": m.impressions, "clk": m.clicks,
            "cost": m.cost_micros / 1e6, "conv": m.conversions,
            "val": m.conversions_value, "is": m.search_impression_share,
        }
        line = (f"  {n[:25]:<25} impr={m.impressions:>4} clk={m.clicks:>3} "
                f"cost=¥{m.cost_micros/1e6:>5.0f} conv={m.conversions:>4.1f} "
                f"is={m.search_impression_share:.3f}")
        log(line)

        if m.impressions > 0 and n not in STATE["first_impr"]:
            STATE["first_impr"][n] = datetime.now().isoformat()
            tg(f"🟢 MU Ads: {n} got FIRST impression ({m.impressions}). "
               f"cost=¥{m.cost_micros/1e6:.0f}")
        if m.conversions > 0 and n not in STATE["first_conv"]:
            STATE["first_conv"][n] = datetime.now().isoformat()
            tg(f"💰 MU Ads: {n} FIRST CONVERSION! conv={m.conversions} "
               f"val=¥{m.conversions_value:.0f}")
    for n in CAMPAIGNS:
        if n not in found:
            log(f"  {n[:25]:<25} (no metrics yet)")

    # Landing health
    for u in LANDINGS:
        try:
            req = urllib.request.Request(u, method="HEAD")
            code = urllib.request.urlopen(req, timeout=10).status
        except Exception as ex:
            code = f"ERR:{type(ex).__name__}"
        if code != 200:
            tg(f"⚠️ Landing {u} → {code}")
        log(f"  landing {u} -> {code}")


def main():
    interval = 300  # 5 min
    duration = 3600  # 1 hour
    log(f"=== plan_a_monitor start (interval={interval}s, duration={duration}s) ===")
    tg("⏱ MU Ads Plan A monitor started — polling 5min for 1h "
       "(MU-Brand ¥800/d ¥250cpc, MU-CRAFT ¥2500/d ¥380cpc)")
    yaml = str(Path.home() / ".config" / "google-ads" / "google-ads.yaml")
    client = GoogleAdsClient.load_from_storage(yaml, version="v22")
    start = time.time()
    while time.time() - start < duration:
        log(f"--- tick t+{int(time.time()-start)}s ---")
        try:
            snapshot(client)
        except Exception as e:
            log(f"[snapshot err] {type(e).__name__}: {str(e)[:200]}")
        time.sleep(interval)
    log("=== plan_a_monitor done ===")
    tg("⏹ MU Ads Plan A monitor finished (1h). Check logs/plan_a_20260523/monitor.log")


if __name__ == "__main__":
    main()
