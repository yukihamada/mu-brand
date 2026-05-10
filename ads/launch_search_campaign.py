#!/usr/bin/env python3
"""Create + launch the wearmu /you Search campaign via Google Ads REST API.

Why REST instead of the python client: the python client implicitly attaches
ADC quota-project headers that conflict with the OAuth client we have. REST
lets us set x-goog-user-project explicitly.

Account: 5408218744 (BANTO — chosen for working billing + JPY).
The campaign name carries the `MU_YOU_` prefix so it is unambiguous in BANTO's
report views.
"""
import os, sys, time, json, urllib.request, urllib.error, urllib.parse, yaml
from pathlib import Path

YAML = Path.home() / "google-ads.yaml"
LOGIN_CID = "5408218744"     # operate as itself; the manager 1532515844 is rejected by API for non-manager ops
OPERATING_CID = "5408218744"
QUOTA_PROJECT = "gen-lang-client-0101706386"

CAMPAIGN_NAME = "MU_YOU_Search_Registration_2026-05"
DAILY_BUDGET_MICROS = 1_000 * 1_000_000   # ¥1,000/day
CPC_BID_MICROS_DEFAULT = 150 * 1_000_000  # ¥150 cap

AD_GROUPS = [
    {
        "name": "AI Tシャツ",
        "cpc_bid_micros": 150_000_000,
        "keywords": [
            ("毎日 Tシャツ デザイン", "EXACT"),
            ("AI Tシャツ デザイン", "EXACT"),
            ("Tシャツ サブスク 毎日", "EXACT"),
            ("AI が描く Tシャツ", "PHRASE"),
            ("自分専用 Tシャツ", "PHRASE"),
            ("オーダーメイド Tシャツ AI", "PHRASE"),
            ("毎日 違う Tシャツ", "PHRASE"),
            ("AI ファッション 自動生成", "BROAD"),
            ("パーソナライズ T-shirt", "BROAD"),
        ],
        "headlines": [
            "毎朝 9 時、自分の T シャツが届く",
            "AI があなただけの一着を描く",
            "MU × YOU — 1 日 1 案",
            "30 日無料・登録 1 分",
            "1 着持てば、一生無料",
            "Skip するほど好みに寄る",
            "弟子屈の気象から生まれる服",
            "デザインは AI、肌触りは綿 100%",
            "登録無料・1 日 1 通だけ",
            "NFT 付き Soulbound 証明書",
            "Bella+Canvas 10oz、DTG プリント",
            "今日のあなたを、布に翻訳する",
            "服を「選ぶ」から「育てる」へ",
            "日本発の AI ブランド「MU」",
            "AI Tシャツ サブスク",
        ],
        "descriptions": [
            "AI が毎朝 9:00 にあなた専用の T シャツデザインを生成。30 日間無料で試せます。1 着でも仕立てれば一生無料。",
            "弟子屈の気象データと、あなたが書いた一言から AI が今日の服を描く。Skip するほど明日の案があなたに寄っていく。",
            "ファッションを「選ぶ」のではなく、毎日「育てる」サブスクリプション。NFT 付きの世界に 1 着。",
            "メールで届いて、気に入ったらワンクリック仕立て。Bella+Canvas 10oz、DTG プリント、世界配送。",
        ],
        "path1": "you",
        "path2": "daily",
    },
    {
        "name": "一点もの",
        "cpc_bid_micros": 130_000_000,
        "keywords": [
            ("一点もの Tシャツ", "EXACT"),
            ("オリジナル Tシャツ ブランド", "EXACT"),
            ("世界に 1 着 Tシャツ", "PHRASE"),
            ("作家 Tシャツ ブランド", "PHRASE"),
            ("アート Tシャツ 国内ブランド", "PHRASE"),
            ("AI アート ファッション", "PHRASE"),
            ("ミニマル Tシャツ ブランド", "BROAD"),
            ("日本発 ファッション ブランド", "BROAD"),
        ],
        "headlines": [
            "世界に 1 着、Soulbound NFT 付き",
            "国内発・北海道弟子屈町から",
            "着る人ごとに違うデザイン",
            "Printful 世界配送・受注生産",
            "アート Tシャツとしても着られる",
            "毎朝 9 時、自分の T シャツが届く",
            "AI があなただけの一着を描く",
            "MU × YOU — 1 日 1 案",
            "30 日無料で試して、1 着で永久無料",
            "服を「選ぶ」から「育てる」へ",
            "日本発の AI ブランド「MU」",
        ],
        "descriptions": [
            "一点もの好きへ。AI が描き、Printful が刷る。所有の証明は Solana 上の Soulbound NFT。",
            "ミニマルでありながら、毎日違う。MU × YOU は服のサブスクではなく、自分への手紙。",
            "AI が毎朝 9:00 にあなた専用の T シャツデザインを生成。30 日間無料、1 着仕立てれば一生無料。",
            "弟子屈の気象データと、あなたが書いた一言から AI が今日の服を描く。Skip するほど明日の案があなたに寄っていく。",
        ],
        "path1": "you",
        "path2": "unique",
    },
]

NEGATIVE_KEYWORDS = [
    "無料 ダウンロード", "イラスト 配布", "Tシャツ 印刷 業者",
    "卸", "卸売", "ライセンス フリー", "フリー素材",
    "プラモデル", "コスプレ", "オリジナル iPhone",
]


# ─────────────────────────────────────────────────────────────────────
# REST helpers
# ─────────────────────────────────────────────────────────────────────

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
        if body: print(f"    body: {json.dumps(body)[:200]}")
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
        sys.stderr.write(f"\n!! {method} {path} → HTTP {e.code}\n   body: {msg[:1000]}\n")
        raise


def mutate(method, body, token, dev_token, dry_run):
    return call("POST", f"/customers/{OPERATING_CID}/{method}", token, dev_token, body, dry_run=dry_run)


# ─────────────────────────────────────────────────────────────────────
# Step 1: campaign budget
# ─────────────────────────────────────────────────────────────────────
def create_budget(token, dev_token, dry_run):
    op = {"create": {
        "name": f"{CAMPAIGN_NAME} budget",
        "amountMicros": str(DAILY_BUDGET_MICROS),
        "deliveryMethod": "STANDARD",
        "explicitlyShared": False,
    }}
    res = mutate("campaignBudgets:mutate", {"operations": [op]}, token, dev_token, dry_run)
    return res["results"][0]["resourceName"]


# Step 2: campaign
def create_campaign(token, dev_token, budget_rn, dry_run):
    today = time.strftime("%Y%m%d")
    end_dt = time.strftime("%Y%m%d", time.gmtime(time.time() + 11 * 86400))
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
        # EU DSA compliance: required since 2025. We don't run political ads.
        "containsEuPoliticalAdvertising": "DOES_NOT_CONTAIN_EU_POLITICAL_ADVERTISING",
    }}
    res = mutate("campaigns:mutate", {"operations": [op]}, token, dev_token, dry_run)
    return res["results"][0]["resourceName"]


# Step 2b: target Japan
def add_geo_targets(token, dev_token, campaign_rn, dry_run):
    # 2392 = Japan
    op = {"create": {
        "campaign": campaign_rn,
        "location": {"geoTargetConstant": "geoTargetConstants/2392"},
    }}
    return mutate("campaignCriteria:mutate", {"operations": [op]}, token, dev_token, dry_run)


# Step 2c: target Japanese language (1005 = Japanese)
def add_language_target(token, dev_token, campaign_rn, dry_run):
    op = {"create": {
        "campaign": campaign_rn,
        "language": {"languageConstant": "languageConstants/1005"},
    }}
    return mutate("campaignCriteria:mutate", {"operations": [op]}, token, dev_token, dry_run)


# Step 2d: negative keywords on campaign
def add_negative_keywords(token, dev_token, campaign_rn, kws, dry_run):
    ops = [
        {"create": {
            "campaign": campaign_rn,
            "negative": True,
            "keyword": {"text": k, "matchType": "PHRASE"},
        }} for k in kws
    ]
    return mutate("campaignCriteria:mutate", {"operations": ops}, token, dev_token, dry_run)


# Step 3: ad groups + keywords + ads
def create_ad_group(token, dev_token, campaign_rn, group, dry_run):
    op = {"create": {
        "name": f"MU_YOU_{group['name']}",
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
            "finalUrls": ["https://wearmu.com/you"],
        },
    }}
    return mutate("adGroupAds:mutate", {"operations": [op]}, token, dev_token, dry_run)


# ─────────────────────────────────────────────────────────────────────
def main():
    dry_run = "--dry-run" in sys.argv
    cid, csec, refresh, dev_token = load_creds()
    token = access_token(cid, csec, refresh)
    print(f"Got access token: {token[:18]}…  dry_run={dry_run}")

    print("\n[1/6] Create campaign budget…")
    budget_rn = create_budget(token, dev_token, dry_run)
    print(f"  → {budget_rn}")

    print("\n[2/6] Create campaign…")
    campaign_rn = create_campaign(token, dev_token, budget_rn, dry_run)
    print(f"  → {campaign_rn}")

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
        print(f"[6/6]   responsive search ad…")
        add_ad(token, dev_token, ag_rn, group, dry_run)

    print(f"\n✅ Campaign live: {CAMPAIGN_NAME}")
    print(f"   Budget: ¥{DAILY_BUDGET_MICROS // 1_000_000}/日 × 10 日 = ¥{DAILY_BUDGET_MICROS * 10 // 1_000_000}")
    print(f"   Account: {OPERATING_CID}")


if __name__ == "__main__":
    main()
