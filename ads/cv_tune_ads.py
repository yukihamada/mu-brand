#!/usr/bin/env python3
"""Nightly Google Ads CPC nudge for MU_YOU_Search_Registration_2026-05.

Pulls /api/cv/config + /api/admin/cv_pulse (read-only fields) to decide:
- 0 signups in 24h    → +20% CPC (push harder)
- 1-4 signups in 24h  → keep
- 5-9 signups in 24h  → -10% CPC (room to be cheaper)
- ≥10 signups in 24h  → -20% CPC (we're winning at this price)

Caps: ¥80 floor, ¥250 ceiling. Logs to Telegram.

Run via cron: `0 1 * * * python3 ads/cv_tune_ads.py` (JST 10:00).
"""
import os, sys, json, time, urllib.request, urllib.parse, yaml
from pathlib import Path

YAML = Path.home() / "google-ads.yaml"
CUSTOMER_ID = "5408218744"
CAMPAIGN_ID = "23835252377"
QUOTA = "gen-lang-client-0101706386"

CPC_FLOOR_MICROS  =  80_000_000   # ¥80
CPC_CEIL_MICROS   = 250_000_000   # ¥250

TG_TOKEN = os.environ.get("TELEGRAM_BOT_TOKEN", "")
TG_CHAT  = os.environ.get("TELEGRAM_CHAT_ID", "1136442501")
ADMIN_TOKEN = os.environ.get("MU_ADMIN_TOKEN", "mu-admin-2026")


def telegram(msg):
    if not TG_TOKEN:
        print("(no TELEGRAM_BOT_TOKEN, skipping)")
        return
    try:
        url = f"https://api.telegram.org/bot{TG_TOKEN}/sendMessage"
        urllib.request.urlopen(
            urllib.request.Request(url,
                data=json.dumps({"chat_id": TG_CHAT, "text": msg}).encode(),
                headers={"Content-Type": "application/json"}),
            timeout=15).read()
    except Exception as e:
        print(f"telegram: {e}")


def cv_pulse():
    # Force a fresh pulse so we read today's numbers, not 30-min-stale.
    req = urllib.request.Request(
        "https://wearmu.com/api/admin/cv_pulse",
        data=json.dumps({"admin_token": ADMIN_TOKEN}).encode(),
        headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.loads(r.read())["metrics"]


def ads_token():
    cfg = yaml.safe_load(open(YAML))
    data = urllib.parse.urlencode({
        "client_id": cfg["client_id"], "client_secret": cfg["client_secret"],
        "refresh_token": cfg["refresh_token"], "grant_type": "refresh_token",
    }).encode()
    with urllib.request.urlopen(
        urllib.request.Request("https://oauth2.googleapis.com/token", data=data),
        timeout=30) as r:
        return json.loads(r.read())["access_token"], cfg["developer_token"]


def ads_call(method, path, body, tok, dev_tok):
    url = f"https://googleads.googleapis.com/v22{path}"
    req = urllib.request.Request(url, method=method,
        data=json.dumps(body).encode() if body else None)
    req.add_header("Authorization", f"Bearer {tok}")
    req.add_header("developer-token", dev_tok)
    req.add_header("login-customer-id", CUSTOMER_ID)
    req.add_header("x-goog-user-project", QUOTA)
    if body: req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read() or b"{}")


def current_ad_groups(tok, dev_tok):
    q = {"query": f"SELECT ad_group.id, ad_group.name, ad_group.cpc_bid_micros FROM ad_group WHERE campaign.id = {CAMPAIGN_ID} AND ad_group.status = 'ENABLED'"}
    res = ads_call("POST", f"/customers/{CUSTOMER_ID}/googleAds:searchStream", q, tok, dev_tok)
    out = []
    for chunk in (res if isinstance(res, list) else [res]):
        for r in chunk.get("results", []):
            out.append((r["adGroup"]["id"], r["adGroup"]["name"],
                        int(r["adGroup"].get("cpcBidMicros", 0))))
    return out


def update_cpc(group_id, new_micros, tok, dev_tok):
    body = {"operations": [{
        "update": {
            "resourceName": f"customers/{CUSTOMER_ID}/adGroups/{group_id}",
            "cpcBidMicros": str(new_micros),
        },
        "updateMask": "cpc_bid_micros",
    }]}
    return ads_call("POST", f"/customers/{CUSTOMER_ID}/adGroups:mutate", body, tok, dev_tok)


def decide_multiplier(signups_24h):
    if signups_24h == 0: return 1.20, "no signups → push harder"
    if signups_24h < 5:  return 1.00, "trickle → hold"
    if signups_24h < 10: return 0.90, "growing → save 10%"
    return 0.80, "winning → save 20%"


def main():
    metrics = cv_pulse()
    signups_24h = metrics.get("signups_24h", 0)
    mult, reason = decide_multiplier(signups_24h)

    tok, dev_tok = ads_token()
    groups = current_ad_groups(tok, dev_tok)
    if not groups:
        telegram(f"MU ads tune: no ad groups in campaign {CAMPAIGN_ID}")
        return

    changes = []
    for (gid, name, cur_micros) in groups:
        new_micros = int(cur_micros * mult)
        new_micros = max(CPC_FLOOR_MICROS, min(CPC_CEIL_MICROS, new_micros))
        if abs(new_micros - cur_micros) < 5_000_000:  # < ¥5 change → skip
            continue
        update_cpc(gid, new_micros, tok, dev_tok)
        changes.append(f"{name}: ¥{cur_micros//1_000_000} → ¥{new_micros//1_000_000}")

    if changes:
        body = "MU ads tune (signups_24h=" + str(signups_24h) + "): " + reason + "\n" + "\n".join(changes)
    else:
        body = f"MU ads tune (signups_24h={signups_24h}): {reason} — no change (within ¥5 hysteresis)"
    print(body)
    telegram(body)


if __name__ == "__main__":
    main()
