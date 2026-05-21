#!/usr/bin/env python3
"""Launch MU-CRAFT Google Ads Search campaign.

Creates budget + campaign + ad group + keywords + responsive search ad
in one shot, all wired to https://craft.wearmu.com.

Budget: ¥1,000/day → ¥10,000 over 10 days (the pre-approved test cap).
Customer: 9591303572 (existing MU account, same as other MU campaigns).
Geo: Japan only.
Status: ENABLED (live immediately).

Usage:
    python3 scripts/launch_craft_campaign.py [--dry-run]
"""
from __future__ import annotations
import argparse
import os
import sys
from datetime import datetime, timedelta
from pathlib import Path

# load /Users/yuki/.env
for ln in Path("/Users/yuki/.env").read_text().splitlines():
    if "=" in ln and not ln.startswith("#"):
        k, v = ln.split("=", 1)
        os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))

from google.ads.googleads.client import GoogleAdsClient
from google.ads.googleads.errors import GoogleAdsException

CUSTOMER_ID = "9591303572"
YAML_PATH = str(Path.home() / ".config" / "google-ads" / "google-ads.yaml")
FINAL_URL = "https://craft.wearmu.com"
DAILY_BUDGET_YEN = 1000
CPC_BID_YEN = 100
END_DATE = (datetime.now() + timedelta(days=10)).strftime("%Y-%m-%d")

CAMPAIGN_NAME = "MU-CRAFT-OneClick-2026-05"
AD_GROUP_NAME = "AI Tシャツ ワンクリック"

# (text, match_type) — match types: EXACT, PHRASE, BROAD
KEYWORDS = [
    ("AI Tシャツ デザイン", "EXACT"),
    ("Tシャツ 自分で デザイン", "EXACT"),
    ("ワンクリック Tシャツ", "EXACT"),
    ("Tシャツ 1分 デザイン", "PHRASE"),
    ("AI Tシャツ 作成", "PHRASE"),
    ("オリジナル Tシャツ AI", "PHRASE"),
    ("POD Tシャツ 簡単", "PHRASE"),
    ("Tシャツ デザイン 即時", "BROAD"),
    ("AI fashion japan", "BROAD"),
]

# Max 15 headlines × 30 chars
HEADLINES = [
    "1行で Tシャツが作れる",
    "MU CRAFT — 作るを空気にする",
    "30秒で AI が描く Tシャツ",
    "ワンクリックで SKU 化",
    "発話1行 → 物に。MU。",
    "AI が描く、あなたの哲学Tee",
    "Tシャツが思考から生まれる",
    "1コマンドで物にする",
    "黒インクと白インク両方届く",
    "5回まで無料でデザイン生成",
    "禅から数式まで何でもTee",
    "MU ブランドで世に出す",
    "日本発・無在庫 POD AI",
    "1分間ブランド体験",
    "「Tシャツにして」と言うだけ",
]

# Max 4 descriptions × 90 chars
DESCRIPTIONS = [
    "「朝のコーヒーの哲学」と入れるだけで、30秒後にTシャツのデザインとモックアップが完成。MU CRAFT。",
    "無料で5回お試し。思考を物にする発話1行プリミティブ。日本発のMUブランドが提供。",
    "AIがあなたの言葉からブランドコピー・漢字・配色を自動生成。白T/黒Tの両モック付き。",
    "MU CRAFT は「作る」を空気にする AI ファッションサービス。1コマンドでSKU追加。",
]


def micros(yen: float) -> int:
    return int(yen * 1_000_000)


def create_budget(client: GoogleAdsClient) -> str:
    svc = client.get_service("CampaignBudgetService")
    op = client.get_type("CampaignBudgetOperation")
    b = op.create
    b.name = f"{CAMPAIGN_NAME}-budget-{int(datetime.now().timestamp())}"
    b.amount_micros = micros(DAILY_BUDGET_YEN)
    b.delivery_method = client.enums.BudgetDeliveryMethodEnum.STANDARD
    b.explicitly_shared = False
    res = svc.mutate_campaign_budgets(customer_id=CUSTOMER_ID, operations=[op])
    return res.results[0].resource_name


def create_campaign(client: GoogleAdsClient, budget_rn: str) -> tuple[str, int]:
    svc = client.get_service("CampaignService")
    op = client.get_type("CampaignOperation")
    c = op.create
    c.name = CAMPAIGN_NAME
    c.advertising_channel_type = client.enums.AdvertisingChannelTypeEnum.SEARCH
    c.status = client.enums.CampaignStatusEnum.ENABLED
    c.campaign_budget = budget_rn
    c.start_date = datetime.now().strftime("%Y-%m-%d")
    c.end_date = END_DATE
    # Manual CPC (simplest, predictable)
    c.manual_cpc.enhanced_cpc_enabled = False
    c.network_settings.target_google_search = True
    c.network_settings.target_search_network = True
    c.network_settings.target_content_network = False
    c.network_settings.target_partner_search_network = False
    # EU political advertising disclosure (required as of 2025)
    c.contains_eu_political_advertising = client.enums.EuPoliticalAdvertisingStatusEnum.DOES_NOT_CONTAIN_EU_POLITICAL_ADVERTISING
    res = svc.mutate_campaigns(customer_id=CUSTOMER_ID, operations=[op])
    rn = res.results[0].resource_name
    campaign_id = int(rn.split("/")[-1])
    return rn, campaign_id


def add_geo_targeting(client: GoogleAdsClient, campaign_rn: str):
    # Geo target: 2392 = Japan
    svc = client.get_service("CampaignCriterionService")
    op = client.get_type("CampaignCriterionOperation")
    c = op.create
    c.campaign = campaign_rn
    c.location.geo_target_constant = client.get_service("GeoTargetConstantService").geo_target_constant_path("2392")
    svc.mutate_campaign_criteria(customer_id=CUSTOMER_ID, operations=[op])


def add_language_targeting(client: GoogleAdsClient, campaign_rn: str):
    # Language: 1005 = Japanese, 1000 = English (cover both)
    # languageConstants/{id} is a flat resource path; no LanguageConstantService in v22.
    svc = client.get_service("CampaignCriterionService")
    ops = []
    for lang_id in ("1005", "1000"):
        op = client.get_type("CampaignCriterionOperation")
        c = op.create
        c.campaign = campaign_rn
        c.language.language_constant = f"languageConstants/{lang_id}"
        ops.append(op)
    svc.mutate_campaign_criteria(customer_id=CUSTOMER_ID, operations=ops)


def create_ad_group(client: GoogleAdsClient, campaign_rn: str) -> str:
    svc = client.get_service("AdGroupService")
    op = client.get_type("AdGroupOperation")
    a = op.create
    a.name = AD_GROUP_NAME
    a.campaign = campaign_rn
    a.status = client.enums.AdGroupStatusEnum.ENABLED
    a.type_ = client.enums.AdGroupTypeEnum.SEARCH_STANDARD
    a.cpc_bid_micros = micros(CPC_BID_YEN)
    res = svc.mutate_ad_groups(customer_id=CUSTOMER_ID, operations=[op])
    return res.results[0].resource_name


def add_keywords(client: GoogleAdsClient, ad_group_rn: str):
    svc = client.get_service("AdGroupCriterionService")
    ops = []
    for text, mt in KEYWORDS:
        op = client.get_type("AdGroupCriterionOperation")
        c = op.create
        c.ad_group = ad_group_rn
        c.status = client.enums.AdGroupCriterionStatusEnum.ENABLED
        c.keyword.text = text
        c.keyword.match_type = getattr(client.enums.KeywordMatchTypeEnum, mt)
        ops.append(op)
    svc.mutate_ad_group_criteria(customer_id=CUSTOMER_ID, operations=ops)


def create_responsive_search_ad(client: GoogleAdsClient, ad_group_rn: str) -> str:
    svc = client.get_service("AdGroupAdService")
    op = client.get_type("AdGroupAdOperation")
    aga = op.create
    aga.ad_group = ad_group_rn
    aga.status = client.enums.AdGroupAdStatusEnum.ENABLED

    ad = aga.ad
    ad.final_urls.append(FINAL_URL)

    for h in HEADLINES[:15]:
        asset = client.get_type("AdTextAsset")
        asset.text = h[:30]
        ad.responsive_search_ad.headlines.append(asset)
    for d in DESCRIPTIONS[:4]:
        asset = client.get_type("AdTextAsset")
        asset.text = d[:90]
        ad.responsive_search_ad.descriptions.append(asset)

    ad.responsive_search_ad.path1 = "craft"
    ad.responsive_search_ad.path2 = "one-click"

    res = svc.mutate_ad_group_ads(customer_id=CUSTOMER_ID, operations=[op])
    return res.results[0].resource_name


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    client = GoogleAdsClient.load_from_storage(YAML_PATH, version="v22")
    print(f"== MU CRAFT campaign launcher ==")
    print(f"   customer:    {CUSTOMER_ID}")
    print(f"   campaign:    {CAMPAIGN_NAME}")
    print(f"   budget:      ¥{DAILY_BUDGET_YEN}/day until {END_DATE}")
    print(f"   final_url:   {FINAL_URL}")
    print(f"   keywords:    {len(KEYWORDS)}")
    print(f"   headlines:   {len(HEADLINES)} (max 15 used)")
    print(f"   descriptions: {len(DESCRIPTIONS)}")

    if args.dry_run:
        print("\n[dry-run] would create budget + campaign + adgroup + keywords + RSA")
        return 0

    try:
        print("\n1/6 budget...")
        budget = create_budget(client)
        print(f"    {budget}")

        print("2/6 campaign...")
        campaign, cid = create_campaign(client, budget)
        print(f"    {campaign} (id {cid})")

        print("3/6 geo (Japan)...")
        add_geo_targeting(client, campaign)

        print("4/6 language (ja+en)...")
        add_language_targeting(client, campaign)

        print("5/6 ad group + keywords...")
        ag = create_ad_group(client, campaign)
        add_keywords(client, ag)
        print(f"    {ag} ({len(KEYWORDS)} keywords)")

        print("6/6 responsive search ad...")
        ad = create_responsive_search_ad(client, ag)
        print(f"    {ad}")

        print(f"\n✓ LAUNCHED  https://ads.google.com/aw/campaigns?campaignId={cid}")
        print(f"  monitor:  python3 scripts/ads_monitor_loop.py")
        return 0

    except GoogleAdsException as e:
        print("\n✗ GoogleAdsException:")
        for err in e.failure.errors:
            print(f"  - {err.error_code} {err.message}")
            if err.location:
                for fpe in err.location.field_path_elements:
                    print(f"      {fpe.field_name} idx={fpe.index}")
        return 1


if __name__ == "__main__":
    sys.exit(main())
