#!/usr/bin/env python3
"""Create + launch the wearmu /shop Search campaign via Google Ads REST API.

Designed for the autonomous-engine catalog at https://wearmu.com/shop
(1,500+ POD SKUs + AI-generated additions). Tuned MUCH tighter than the
/you launcher: single ad group, low CPC ceiling, hard daily budget cap,
DRY_RUN by default so a misfire can't burn the autonomous-engine
budget unattended.

Usage:
    # safe dry-run (default — prints what WOULD be created)
    python3 ads/launch_shop_search.py

    # ACTUALLY launch (real money starts spending immediately):
    python3 ads/launch_shop_search.py --live

Total budget envelope (catalog::BUDGET_TOTAL_JPY = ¥100,000):
    Daily budget: ¥1,000
    Campaign runs 10 days = ¥10,000 of the ¥100K cap
    → leaves ¥90K for design generation + scale-up campaigns once we
      see which keywords convert.

Auth: reads ~/google-ads.yaml (same as ads/launch_search_campaign.py).
Account: 5408218744 (BANTO — billing + JPY).
"""
import os, sys, time, json, urllib.request, urllib.error, urllib.parse, yaml
from pathlib import Path

YAML = Path.home() / "google-ads.yaml"
LOGIN_CID = "5408218744"
OPERATING_CID = "5408218744"
QUOTA_PROJECT = "gen-lang-client-0101706386"

CAMPAIGN_NAME = "MU_SHOP_Search_Catalog_2026-05"
DAILY_BUDGET_MICROS = 1_000 * 1_000_000   # ¥1,000/day (¥10K over 10 days)
CPC_BID_MICROS_DEFAULT = 80 * 1_000_000   # ¥80 cap — start cheap, tune up via cv_tune_ads.py

LANDING_URL = "https://wearmu.com/shop"

# One tight ad group — broad catalog, not subscription. Keywords mix POD
# product intent (Tシャツ ブランド) with the BJJ/MMA audience we have
# inventory for (BJJ × MU 1,073 SKUs).
AD_GROUP = {
    "name": "shop_catalog",
    "cpc_bid_micros": 80_000_000,
    "keywords": [
        # POD product intent
        ("Tシャツ ブランド オンライン",    "EXACT"),
        ("オリジナル Tシャツ 通販",        "EXACT"),
        ("Tシャツ 1500 種",                "EXACT"),
        ("Tシャツ 海外発送",               "PHRASE"),
        # BJJ / fight gear (matches our 1,000+ BJJ SKUs)
        ("BJJ Tシャツ",                    "EXACT"),
        ("柔術 Tシャツ ブランド",          "EXACT"),
        ("ラッシュガード ブランド",        "EXACT"),
        ("BJJ ラッシュガード",             "EXACT"),
        # Broader signal
        ("AI デザイン Tシャツ",            "PHRASE"),
        ("ミニマル Tシャツ ブランド",      "PHRASE"),
    ],
    "headlines": [
        "700+ デザインから選ぶ",
        "MU — BJJ × カフェ × 都市",
        "AI が毎日 新作を生成",
        "Stripe + Printful 国際発送",
        "BJJ ラッシュガード ¥9,800〜",
        "Tシャツ ¥4,900・即購入",
        "1 着でも 海外配送 7-14 日",
        "日本発の AI ブランド MU",
        "コラボ ブランド 10+",
        "wearmu.com 公式ストア",
        "All-Over Print 対応",
        "Bella+Canvas 3001 / DTG",
        "クレカ・コンビニ・暗号通貨",
        "クーポンコード 利用可",
        "新作 30 分ごと",
    ],
    "descriptions": [
        "700+ コラボデザインの Tシャツ・ラッシュガード・フーディ。Printful 国際発送 7-14 日。",
        "MU × BJJ / kokon / jiuflow ほか 10+ ブランド。¥4,900〜、1 着から海外配送対応。",
        "毎 30 分 AI が新作を生成中。クレカ / コンビニ / 暗号通貨 OK。クーポンコード対応。",
        "在庫を持たないオンデマンド印刷。注文ごとに 1 着ずつ刷ります。",
    ],
    "path1": "shop",
    "path2": "catalog",
}

NEGATIVE_KEYWORDS = [
    # exclude commercial-print / supplier intent
    "Tシャツ 印刷 業者", "卸", "卸売",
    "無料 ダウンロード", "イラスト 配布",
    "ライセンス フリー", "フリー素材",
    # exclude non-apparel surfaces
    "プラモデル", "コスプレ", "iPhoneケース",
]


def load_creds():
    cfg = yaml.safe_load(open(YAML))
    return cfg["client_id"], cfg["client_secret"], cfg["refresh_token"], cfg["developer_token"]


def access_token(client_id, client_secret, refresh_token):
    data = urllib.parse.urlencode({
        "client_id": client_id, "client_secret": client_secret,
        "refresh_token": refresh_token, "grant_type": "refresh_token",
    }).encode()
    req = urllib.request.Request("https://oauth2.googleapis.com/token", data=data)
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.loads(r.read())["access_token"]


def call(method, path, token, dev_token, body=None, dry_run=True):
    url = f"https://googleads.googleapis.com/v22{path}"
    if dry_run and method != "GET":
        print(f"  DRY {method} {path}")
        if body:
            print(f"     body: {json.dumps(body, ensure_ascii=False)[:240]}")
        return {"results": [{"resourceName": "DRY_RUN_RESOURCE"}]}
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Authorization", f"Bearer {token}")
    req.add_header("developer-token", dev_token)
    req.add_header("login-customer-id", LOGIN_CID)
    req.add_header("x-goog-user-project", QUOTA_PROJECT)
    if data:
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return json.loads(r.read() or b"{}")
    except urllib.error.HTTPError as e:
        msg = e.read().decode("utf-8", errors="replace")
        sys.stderr.write(f"\n!! {method} {path} → HTTP {e.code}\n   body: {msg[:1000]}\n")
        raise


def mutate(method, body, token, dev_token, dry_run):
    return call("POST", f"/customers/{OPERATING_CID}/{method}",
                token, dev_token, body, dry_run=dry_run)


def create_budget(token, dev_token, dry_run):
    op = {"create": {
        "name": f"{CAMPAIGN_NAME} budget",
        "amountMicros": str(DAILY_BUDGET_MICROS),
        "deliveryMethod": "STANDARD",
        "explicitlyShared": False,
    }}
    res = mutate("campaignBudgets:mutate", {"operations": [op]}, token, dev_token, dry_run)
    return res["results"][0]["resourceName"]


def create_campaign(token, dev_token, budget_rn, dry_run):
    today = time.strftime("%Y%m%d")
    end_dt = time.strftime("%Y%m%d", time.gmtime(time.time() + 10 * 86400))
    op = {"create": {
        "name": CAMPAIGN_NAME,
        "advertisingChannelType": "SEARCH",
        "status": "ENABLED",
        "manualCpc": {"enhancedCpcEnabled": False},
        "campaignBudget": budget_rn,
        "startDate": today,
        "endDate": end_dt,
        "networkSettings": {
            "targetGoogleSearch": True,
            "targetSearchNetwork": True,
            "targetContentNetwork": False,
            "targetPartnerSearchNetwork": False,
        },
        "geoTargetTypeSetting": {
            "positiveGeoTargetType": "PRESENCE",
            "negativeGeoTargetType": "PRESENCE",
        },
        "containsEuPoliticalAdvertising": "DOES_NOT_CONTAIN_EU_POLITICAL_ADVERTISING",
    }}
    res = mutate("campaigns:mutate", {"operations": [op]}, token, dev_token, dry_run)
    return res["results"][0]["resourceName"]


def add_geo_targets(token, dev_token, campaign_rn, dry_run):
    op = {"create": {
        "campaign": campaign_rn,
        "location": {"geoTargetConstant": "geoTargetConstants/2392"},  # Japan
    }}
    return mutate("campaignCriteria:mutate", {"operations": [op]}, token, dev_token, dry_run)


def add_language_target(token, dev_token, campaign_rn, dry_run):
    op = {"create": {
        "campaign": campaign_rn,
        "language": {"languageConstant": "languageConstants/1005"},  # Japanese
    }}
    return mutate("campaignCriteria:mutate", {"operations": [op]}, token, dev_token, dry_run)


def add_negative_keywords(token, dev_token, campaign_rn, kws, dry_run):
    ops = [
        {"create": {
            "campaign": campaign_rn,
            "negative": True,
            "keyword": {"text": k, "matchType": "PHRASE"},
        }} for k in kws
    ]
    return mutate("campaignCriteria:mutate", {"operations": ops}, token, dev_token, dry_run)


def create_ad_group(token, dev_token, campaign_rn, group, dry_run):
    op = {"create": {
        "name": f"MU_SHOP_{group['name']}",
        "campaign": campaign_rn,
        "status": "ENABLED",
        "type": "SEARCH_STANDARD",
        "cpcBidMicros": str(group["cpc_bid_micros"]),
    }}
    res = mutate("adGroups:mutate", {"operations": [op]}, token, dev_token, dry_run)
    return res["results"][0]["resourceName"]


def add_keywords(token, dev_token, ag_rn, keywords, dry_run):
    ops = [
        {"create": {
            "adGroup": ag_rn,
            "status": "ENABLED",
            "keyword": {"text": text, "matchType": match},
        }} for text, match in keywords
    ]
    return mutate("adGroupCriteria:mutate", {"operations": ops}, token, dev_token, dry_run)


def add_ad(token, dev_token, ag_rn, group, dry_run):
    headlines = [{"text": h} for h in group["headlines"][:15]]
    descs = [{"text": d} for d in group["descriptions"][:4]]
    op = {"create": {
        "adGroup": ag_rn,
        "status": "ENABLED",
        "ad": {
            "responsiveSearchAd": {
                "headlines": headlines,
                "descriptions": descs,
                "path1": group["path1"],
                "path2": group["path2"],
            },
            "finalUrls": [LANDING_URL],
        },
    }}
    return mutate("adGroupAds:mutate", {"operations": [op]}, token, dev_token, dry_run)


def telegram(msg):
    tok = os.environ.get("TELEGRAM_BOT_TOKEN", "")
    chat = os.environ.get("TELEGRAM_CHAT_ID", "1136442501")
    if not tok:
        return
    try:
        urllib.request.urlopen(
            urllib.request.Request(
                f"https://api.telegram.org/bot{tok}/sendMessage",
                data=json.dumps({"chat_id": chat, "text": msg}).encode(),
                headers={"Content-Type": "application/json"},
            ), timeout=15
        ).read()
    except Exception:
        pass


def main():
    live = "--live" in sys.argv
    dry_run = not live
    mode = "LIVE (real money)" if live else "DRY RUN (no spend)"
    print(f"=== MU /shop Search Campaign Launcher — {mode} ===")
    print(f"Target:    {LANDING_URL}")
    print(f"Budget:    ¥{DAILY_BUDGET_MICROS // 1_000_000}/day × 10 days = ¥{DAILY_BUDGET_MICROS * 10 // 1_000_000}")
    print(f"CPC cap:   ¥{CPC_BID_MICROS_DEFAULT // 1_000_000}")
    print(f"Account:   {OPERATING_CID} (BANTO)")
    print()
    if live:
        print("⚠️  LIVE mode — campaign will start spending within ~30 min of approval.")
        print("    The existing cron-ads-tune.yml (JST 10:00 daily) will tune CPC up/down")
        print("    based on /api/admin/cv_pulse signal. Pause manually in Google Ads UI")
        print("    if conversions don't materialise by day 3.")
        print()
        time.sleep(3)
        telegram(f"🚀 Launching {CAMPAIGN_NAME} — ¥{DAILY_BUDGET_MICROS // 1_000_000}/day × 10d at ¥{CPC_BID_MICROS_DEFAULT // 1_000_000} CPC. Landing: {LANDING_URL}")

    cid, csec, refresh, dev_token = load_creds()
    token = access_token(cid, csec, refresh)
    print(f"Got access token: {token[:18]}…")

    print("\n[1/6] Create campaign budget…")
    budget_rn = create_budget(token, dev_token, dry_run)
    print(f"  → {budget_rn}")

    print("\n[2/6] Create campaign…")
    campaign_rn = create_campaign(token, dev_token, budget_rn, dry_run)
    print(f"  → {campaign_rn}")

    print("\n[3/6] Geo (JP) + language (JA) + negative keywords…")
    add_geo_targets(token, dev_token, campaign_rn, dry_run)
    add_language_target(token, dev_token, campaign_rn, dry_run)
    add_negative_keywords(token, dev_token, campaign_rn, NEGATIVE_KEYWORDS, dry_run)
    print("  done")

    print(f"\n[4/6] Ad group: {AD_GROUP['name']}…")
    ag_rn = create_ad_group(token, dev_token, campaign_rn, AD_GROUP, dry_run)
    print(f"  → {ag_rn}")
    print(f"[5/6]   {len(AD_GROUP['keywords'])} keywords…")
    add_keywords(token, dev_token, ag_rn, AD_GROUP["keywords"], dry_run)
    print(f"[6/6]   responsive search ad → {LANDING_URL}…")
    add_ad(token, dev_token, ag_rn, AD_GROUP, dry_run)

    if live:
        print(f"\n✅ {CAMPAIGN_NAME} LIVE — money starts within 30 min.")
        telegram(f"✅ {CAMPAIGN_NAME} live — monitor: https://ads.google.com/aw/campaigns")
    else:
        print(f"\n✅ DRY RUN complete — to actually launch:")
        print(f"     python3 ads/launch_shop_search.py --live")


if __name__ == "__main__":
    main()
