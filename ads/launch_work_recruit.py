#!/usr/bin/env python3
"""Create + launch the wearmu /work worker-recruitment Search campaign.

Adapted from launch_search_campaign.py (same REST plumbing / account).
Lands on https://wearmu.com/work — the server randomly serves one of the 6
recruitment patterns (work_recruit) and mu-funnel.js logs per-pattern CVR
(work_view{variant} → work_apply_v{n}). So the 6-pattern A/B split happens
server-side on the SAME ad traffic (cleaner than 6 ad groups competing).

Budget: ¥1,000/day, endDate = today+3 → ~¥3,000 total. Stop anytime by
pausing the campaign (AdFlow set_status or Google Ads UI).

Usage:  python3 ads/launch_work_recruit.py --dry-run   # validate, no spend
        python3 ads/launch_work_recruit.py             # LIVE (real spend)
"""
import os, sys, time, json, urllib.request, urllib.error, urllib.parse, yaml
from pathlib import Path

YAML = Path.home() / "google-ads.yaml"
LOGIN_CID = "5408218744"      # BANTO — working billing + JPY
OPERATING_CID = "5408218744"
QUOTA_PROJECT = "gen-lang-client-0101706386"

CAMPAIGN_NAME = "MU_WORK_Recruit_Search_2026-06"
DAILY_BUDGET_MICROS = 1_000 * 1_000_000   # ¥1,000/day
RUN_DAYS = 3                               # endDate = today + RUN_DAYS → ~¥3,000
FINAL_URL = "https://wearmu.com/work"      # server splits the 6 patterns

AD_GROUPS = [
    {
        "name": "在宅副業",
        "cpc_bid_micros": 120_000_000,     # ¥120 cap
        "keywords": [
            ("在宅ワーク 副業", "PHRASE"),
            ("スキマ時間 副業", "PHRASE"),
            ("おうちで 仕事", "PHRASE"),
            ("在宅 軽作業", "PHRASE"),
            ("スマホ 副業 安全", "PHRASE"),
            ("主婦 在宅 仕事", "PHRASE"),
            ("すきま時間 内職", "BROAD"),
            ("在宅 ワーク 簡単", "BROAD"),
        ],
        "path1": "work", "path2": "recruit",
    },
    {
        "name": "梱包発送",
        "cpc_bid_micros": 120_000_000,
        "keywords": [
            ("梱包 在宅 仕事", "PHRASE"),
            ("発送作業 在宅", "PHRASE"),
            ("シール貼り 内職", "PHRASE"),
            ("検品 在宅 仕事", "PHRASE"),
            ("ハンドメイド 副業", "BROAD"),
            ("内職 在宅 手作業", "BROAD"),
        ],
        "path1": "work", "path2": "home",
    },
]

# Shared responsive-ad assets (recruitment). RSA limits: headline ≤30, desc ≤90.
HEADLINES = [
    "スマホで、おうちで副業",
    "MUの仕上げ・発送のお仕事",
    "スキマ時間に、やった分だけ",
    "ノルマなし・いつでも辞めOK",
    "住所は見えない安心設計",
    "前払いプール＋写真で承認",
    "特別なスキル不要・1件10分",
    "送料は当社負担・立替なし",
    "在宅でできる軽作業",
    "AIブランドMUの“届ける人”",
    "報酬は月末締め翌月振込",
    "全国どこでも・自分のペース",
]
DESCRIPTIONS = [  # 日本語RSAは説明文45字以内
    "MUの商品を仕上げて届ける在宅ワーク。スマホで完結、1件10分ほど。",
    "住所は作業者に見せないブラインド配送。報酬は写真で承認後に支払い。",
    "特別なスキル不要。やった分だけ報酬・翌月振込。全国どこでも自分のペース。",
    "AIブランドMUの“届ける人”になる仕事。まずは応募（30秒）。",
]

NEGATIVE_KEYWORDS = [
    "正社員", "転職", "アルバイト 高校生", "求人 ボックス",
    "詐欺", "稼げる 怪しい", "高収入 即日", "副業 詐欺",
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


def call(method, path, token, dev_token, body=None, dry_run=False):
    url = f"https://googleads.googleapis.com/v22{path}"
    if dry_run and method != "GET":
        print(f"  DRY RUN {method} {path}")
        if body: print(f"    body: {json.dumps(body, ensure_ascii=False)[:240]}")
        return {"results": [{"resourceName": "DRY_RUN_RESOURCE"}]}
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Authorization", f"Bearer {token}")
    req.add_header("developer-token", dev_token)
    req.add_header("login-customer-id", LOGIN_CID)
    req.add_header("x-goog-user-project", QUOTA_PROJECT)
    if data: req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return json.loads(r.read() or b"{}")
    except urllib.error.HTTPError as e:
        msg = e.read().decode("utf-8", errors="replace")
        sys.stderr.write(f"\n!! {method} {path} → HTTP {e.code}\n   body: {msg[:1200]}\n")
        raise


def mutate(method, body, token, dev_token, dry_run):
    return call("POST", f"/customers/{OPERATING_CID}/{method}", token, dev_token, body, dry_run=dry_run)


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
    end_dt = time.strftime("%Y%m%d", time.gmtime(time.time() + RUN_DAYS * 86400))
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
    op = {"create": {"campaign": campaign_rn, "location": {"geoTargetConstant": "geoTargetConstants/2392"}}}
    return mutate("campaignCriteria:mutate", {"operations": [op]}, token, dev_token, dry_run)


def add_language_target(token, dev_token, campaign_rn, dry_run):
    op = {"create": {"campaign": campaign_rn, "language": {"languageConstant": "languageConstants/1005"}}}
    return mutate("campaignCriteria:mutate", {"operations": [op]}, token, dev_token, dry_run)


def add_negative_keywords(token, dev_token, campaign_rn, kws, dry_run):
    ops = [{"create": {"campaign": campaign_rn, "negative": True,
                       "keyword": {"text": k, "matchType": "PHRASE"}}} for k in kws]
    return mutate("campaignCriteria:mutate", {"operations": ops}, token, dev_token, dry_run)


def create_ad_group(token, dev_token, campaign_rn, group, dry_run):
    op = {"create": {
        "name": f"MU_WORK_{group['name']}",
        "campaign": campaign_rn,
        "status": "ENABLED",
        "type": "SEARCH_STANDARD",
        "cpcBidMicros": str(group["cpc_bid_micros"]),
    }}
    res = mutate("adGroups:mutate", {"operations": [op]}, token, dev_token, dry_run)
    return res["results"][0]["resourceName"]


def add_keywords(token, dev_token, ag_rn, keywords, dry_run):
    ops = [{"create": {"adGroup": ag_rn, "status": "ENABLED",
                       "keyword": {"text": t, "matchType": m}}} for t, m in keywords]
    return mutate("adGroupCriteria:mutate", {"operations": ops}, token, dev_token, dry_run)


def add_ad(token, dev_token, ag_rn, group, dry_run):
    op = {"create": {
        "adGroup": ag_rn,
        "status": "ENABLED",
        "ad": {
            "responsiveSearchAd": {
                "headlines": [{"text": h} for h in HEADLINES[:15]],
                "descriptions": [{"text": d} for d in DESCRIPTIONS[:4]],
                "path1": group["path1"], "path2": group["path2"],
            },
            "finalUrls": [FINAL_URL],
        },
    }}
    return mutate("adGroupAds:mutate", {"operations": [op]}, token, dev_token, dry_run)


def main():
    dry_run = "--dry-run" in sys.argv
    cid, csec, refresh, dev_token = load_creds()
    token = access_token(cid, csec, refresh)
    print(f"Got access token: {token[:18]}…  dry_run={dry_run}")

    print("\n[1/6] Create campaign budget…")
    budget_rn = create_budget(token, dev_token, dry_run); print(f"  → {budget_rn}")
    print("\n[2/6] Create campaign…")
    campaign_rn = create_campaign(token, dev_token, budget_rn, dry_run); print(f"  → {campaign_rn}")
    print("\n[3/6] Geo + language + negative keywords…")
    add_geo_targets(token, dev_token, campaign_rn, dry_run)
    add_language_target(token, dev_token, campaign_rn, dry_run)
    add_negative_keywords(token, dev_token, campaign_rn, NEGATIVE_KEYWORDS, dry_run)
    print("  done")
    for i, group in enumerate(AD_GROUPS, start=1):
        print(f"\n[4/6] Ad group {i}: {group['name']}…")
        ag_rn = create_ad_group(token, dev_token, campaign_rn, group, dry_run)
        print(f"  ad group → {ag_rn}")
        print(f"[5/6]   {len(group['keywords'])} keywords…")
        add_keywords(token, dev_token, ag_rn, group["keywords"], dry_run)
        print(f"[6/6]   responsive search ad → {FINAL_URL}")
        add_ad(token, dev_token, ag_rn, group, dry_run)

    print(f"\n✅ {'DRY-RUN ok' if dry_run else 'Campaign LIVE'}: {CAMPAIGN_NAME}")
    print(f"   Budget: ¥{DAILY_BUDGET_MICROS // 1_000_000}/日 × {RUN_DAYS}日 ≈ ¥{DAILY_BUDGET_MICROS * RUN_DAYS // 1_000_000}")
    print(f"   Account: {OPERATING_CID} / Landing: {FINAL_URL} (server splits 6 patterns)")


if __name__ == "__main__":
    main()
