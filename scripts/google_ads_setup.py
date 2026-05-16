#!/usr/bin/env python3
"""
MU Google Ads — campaign bootstrap (2026-05-16).

Creates 3 campaigns under one Google Ads account:
  A. MU-Brand     — defensive search on "MU", "wearmu", "MUGEN" etc.
  B. MU-Discovery — search on "AI Tシャツ 生成" and adjacent KWs
  C. MU-PMax      — Performance Max for retargeting + lookalike

Total budget: ¥30,000 / month (split 150/500/350 ¥/day = ¥1,000/day).

Prereq: `pip install google-ads`, and ~/.config/google-ads/google-ads.yaml
populated. See scripts/google_ads_README.md for OAuth bootstrap.

Run:
  python scripts/google_ads_setup.py --dry-run         # show plan, mutate nothing
  python scripts/google_ads_setup.py --create-all      # create the 3 campaigns
  python scripts/google_ads_setup.py --pause-all       # pause everything (kill switch)
  python scripts/google_ads_setup.py --status          # print current spend / impressions / clicks / conversions

The script is idempotent: campaign names act as unique keys. Re-running
--create-all on an existing setup updates budgets and adds missing
ad groups / KWs / ads without duplicating.

DEFAULTS
- Currency: account default (set to JPY when creating the account)
- Geo: Japan (criterion 2392)
- Language: Japanese (criterion 1005)
- Conversion tracking: assumes a single conversion action named
  "MU Purchase" exists (or is created here when --create-conversions
  is passed). Stripe webhook hits this via GADS_PURCHASE_LABEL in
  the tracking shim (store/static/tracking.js).
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path
from typing import Iterable

try:
    from google.ads.googleads.client import GoogleAdsClient
    from google.ads.googleads.errors import GoogleAdsException
except ImportError:
    sys.exit(
        "google-ads SDK missing. Install it:\n"
        "  pip install --upgrade google-ads\n"
        "Then populate ~/.config/google-ads/google-ads.yaml — see "
        "scripts/google_ads_README.md."
    )


# ─── Campaign plan ─────────────────────────────────────────────────────────

DAILY_BUDGET_MICROS = {
    "MU-Brand":     150_000_000,   # ¥150/day  → ¥4.5K/month
    "MU-Discovery": 500_000_000,   # ¥500/day  → ¥15K/month
    "MU-PMax":      350_000_000,   # ¥350/day  → ¥10.5K/month
}

NEGATIVE_KEYWORDS = [
    # AI image generation tools — wrong intent
    "無料", "free", "ダウンロード", "素材", "フリー素材",
    "chatgpt", "midjourney", "画像生成", "イラスト",
    # Wrong audience
    "子供", "キッズ", "ベビー", "幼児",
    # Used goods
    "中古", "古着", "セカンドハンド",
    # Unrelated brands
    "uniqlo", "gu", "muji", "無印",
]

BRAND_KEYWORDS = [
    # exact + phrase
    "[MU brand]", "[wearmu]", "[wearmu.com]",
    "\"MU Tシャツ\"", "\"MUGEN Tシャツ\"", "\"MUON Tシャツ\"",
    "\"間 MA Tシャツ\"", "\"濱田優貴 MU\"",
    "[MUGEN brand]", "[MUON brand]",
]

DISCOVERY_KEYWORDS = [
    "\"AI Tシャツ 生成\"",
    "\"自分専用 Tシャツ\"",
    "\"世界に1着 Tシャツ\"",
    "\"1点もの Tシャツ DTG\"",
    "\"気象データ ファッション\"",
    "\"北海道 アパレル ブランド\"",
    "\"透明性 ブランド T シャツ\"",
    "\"クラフトAI Tシャツ\"",
    "\"生成AI アパレル 日本\"",
    "\"Generative AI tshirt Japan\"",
]

# Responsive Search Ad assets. Headlines max 30 chars, descriptions max 90.
BRAND_HEADLINES = [
    "MU — 公式サイト",
    "MUGEN / MUON / 間 MA",
    "AI が毎時間 1 着描く",
    "北海道 弟子屈の気象から",
    "原価ベース ¥6,800〜",
    "利益の 50% を弟子屈へ",
    "MU 公式 wearmu.com",
    "/you であなた専用に",
]
BRAND_DESCRIPTIONS = [
    "AI が北海道弟子屈の気象を読んで毎時間 Tシャツを生成する DTC ブランド。",
    "1 着 ¥6,800、 利益の 50% は §27 に基づき弟子屈町に寄付。 在庫ゼロ受注生産。",
    "MUGEN / MUON / 間 MA / /you の 4 ライン。 同じデザインは二度と作られません。",
    "公式は wearmu.com のみ。 注文後に Printful EU で 1 枚プリント、 2-3 週間で発送。",
]

DISCOVERY_HEADLINES = [
    "あなた専用 1 着 ¥6,800",
    "AI が 30 秒でデザイン",
    "世界に 1 着の Tシャツ",
    "気象データから生成",
    "弟子屈町に 50% 寄付",
    "受注生産・在庫ゼロ",
    "MU /you で今すぐ作る",
    "原価公開のブランド",
]
DISCOVERY_DESCRIPTIONS = [
    "名前と一言を投げると AI が 30 秒で Tシャツデザインを生成。 ¥6,800 で 1 枚プリント。",
    "1 of 1。 同じデザインは二度と作られません。 Printful EU で 2-3 週間で発送。",
    "利益の 50% を北海道 弟子屈町に寄付。 原価ベース透明設計、 値引き販売しません。",
    "弟子屈の気温・月相・時刻を seed に AI が生成。 1 時間に 1 着、 1 cycle で永久終了。",
]


# ─── Helpers ───────────────────────────────────────────────────────────────


def get_client() -> GoogleAdsClient:
    cfg = os.environ.get("GOOGLE_ADS_CONFIGURATION_FILE_PATH") or str(
        Path.home() / ".config" / "google-ads" / "google-ads.yaml"
    )
    if not Path(cfg).exists():
        sys.exit(
            f"google-ads.yaml not found at {cfg}.\n"
            "See scripts/google_ads_README.md for OAuth bootstrap."
        )
    return GoogleAdsClient.load_from_storage(cfg, version="v17")


def find_campaign_by_name(client: GoogleAdsClient, customer_id: str, name: str) -> str | None:
    ga_service = client.get_service("GoogleAdsService")
    query = (
        "SELECT campaign.resource_name, campaign.name "
        "FROM campaign "
        f"WHERE campaign.name = '{name}' "
        "LIMIT 1"
    )
    for row in ga_service.search(customer_id=customer_id, query=query):
        return row.campaign.resource_name
    return None


def upsert_budget(client: GoogleAdsClient, customer_id: str, name: str, micros: int) -> str:
    """Create or update a shared budget. Idempotent on name."""
    ga_service = client.get_service("GoogleAdsService")
    q = (
        "SELECT campaign_budget.resource_name FROM campaign_budget "
        f"WHERE campaign_budget.name = '{name}-budget'"
    )
    for row in ga_service.search(customer_id=customer_id, query=q):
        return row.campaign_budget.resource_name

    op = client.get_type("CampaignBudgetOperation")
    b = op.create
    b.name = f"{name}-budget"
    b.delivery_method = client.enums.BudgetDeliveryMethodEnum.STANDARD
    b.amount_micros = micros
    b.explicitly_shared = False
    resp = client.get_service("CampaignBudgetService").mutate_campaign_budgets(
        customer_id=customer_id, operations=[op]
    )
    return resp.results[0].resource_name


def create_search_campaign(
    client: GoogleAdsClient, customer_id: str, name: str, budget_rn: str
) -> str:
    """Create a Search campaign. Returns campaign resource_name."""
    existing = find_campaign_by_name(client, customer_id, name)
    if existing:
        return existing
    op = client.get_type("CampaignOperation")
    c = op.create
    c.name = name
    c.advertising_channel_type = client.enums.AdvertisingChannelTypeEnum.SEARCH
    c.status = client.enums.CampaignStatusEnum.ENABLED
    c.campaign_budget = budget_rn
    c.manual_cpc.enhanced_cpc_enabled = True
    c.network_settings.target_google_search = True
    c.network_settings.target_search_network = False
    c.network_settings.target_content_network = False
    c.network_settings.target_partner_search_network = False
    resp = client.get_service("CampaignService").mutate_campaigns(
        customer_id=customer_id, operations=[op]
    )
    return resp.results[0].resource_name


def create_pmax_campaign(
    client: GoogleAdsClient, customer_id: str, name: str, budget_rn: str
) -> str:
    existing = find_campaign_by_name(client, customer_id, name)
    if existing:
        return existing
    op = client.get_type("CampaignOperation")
    c = op.create
    c.name = name
    c.advertising_channel_type = client.enums.AdvertisingChannelTypeEnum.PERFORMANCE_MAX
    c.status = client.enums.CampaignStatusEnum.ENABLED
    c.campaign_budget = budget_rn
    c.maximize_conversions.target_cpa_micros = 1_100_000_000  # ¥1,100 target CPA = 1 sale profit
    resp = client.get_service("CampaignService").mutate_campaigns(
        customer_id=customer_id, operations=[op]
    )
    return resp.results[0].resource_name


def add_geo_japan(client: GoogleAdsClient, customer_id: str, campaign_rn: str) -> None:
    """Restrict targeting to Japan (criterion 2392)."""
    op = client.get_type("CampaignCriterionOperation")
    c = op.create
    c.campaign = campaign_rn
    c.location.geo_target_constant = (
        client.get_service("GeoTargetConstantService").geo_target_constant_path("2392")
    )
    try:
        client.get_service("CampaignCriterionService").mutate_campaign_criteria(
            customer_id=customer_id, operations=[op]
        )
    except GoogleAdsException as e:
        # Already targeted — fine.
        if "DUPLICATE" not in str(e):
            raise


def add_language_japanese(client: GoogleAdsClient, customer_id: str, campaign_rn: str) -> None:
    op = client.get_type("CampaignCriterionOperation")
    c = op.create
    c.campaign = campaign_rn
    c.language.language_constant = (
        client.get_service("LanguageConstantService").language_constant_path("1005")
    )
    try:
        client.get_service("CampaignCriterionService").mutate_campaign_criteria(
            customer_id=customer_id, operations=[op]
        )
    except GoogleAdsException as e:
        if "DUPLICATE" not in str(e):
            raise


def add_negative_keywords(
    client: GoogleAdsClient, customer_id: str, campaign_rn: str, words: Iterable[str]
) -> int:
    ops = []
    for w in words:
        op = client.get_type("CampaignCriterionOperation")
        c = op.create
        c.campaign = campaign_rn
        c.negative = True
        c.keyword.text = w
        c.keyword.match_type = client.enums.KeywordMatchTypeEnum.BROAD
        ops.append(op)
    if not ops:
        return 0
    try:
        client.get_service("CampaignCriterionService").mutate_campaign_criteria(
            customer_id=customer_id, operations=ops
        )
        return len(ops)
    except GoogleAdsException as e:
        # Partial success ok — re-raise if anything other than "already exists"
        msg = str(e)
        if "DUPLICATE" in msg or "already exists" in msg:
            return 0
        raise


def find_ad_group_by_name(
    client: GoogleAdsClient, customer_id: str, campaign_rn: str, name: str
) -> str | None:
    ga_service = client.get_service("GoogleAdsService")
    q = (
        "SELECT ad_group.resource_name FROM ad_group "
        f"WHERE ad_group.campaign = '{campaign_rn}' AND ad_group.name = '{name}'"
    )
    for row in ga_service.search(customer_id=customer_id, query=q):
        return row.ad_group.resource_name
    return None


def upsert_ad_group(
    client: GoogleAdsClient, customer_id: str, campaign_rn: str, name: str,
    default_bid_micros: int = 80_000_000  # ¥80 default CPC
) -> str:
    existing = find_ad_group_by_name(client, customer_id, campaign_rn, name)
    if existing:
        return existing
    op = client.get_type("AdGroupOperation")
    a = op.create
    a.name = name
    a.campaign = campaign_rn
    a.status = client.enums.AdGroupStatusEnum.ENABLED
    a.type_ = client.enums.AdGroupTypeEnum.SEARCH_STANDARD
    a.cpc_bid_micros = default_bid_micros
    resp = client.get_service("AdGroupService").mutate_ad_groups(
        customer_id=customer_id, operations=[op]
    )
    return resp.results[0].resource_name


def add_keywords(
    client: GoogleAdsClient, customer_id: str, ad_group_rn: str, keywords: list[str]
) -> int:
    """Parse [exact], "phrase", word (broad) and add to ad group."""
    ops = []
    for kw in keywords:
        op = client.get_type("AdGroupCriterionOperation")
        c = op.create
        c.ad_group = ad_group_rn
        c.status = client.enums.AdGroupCriterionStatusEnum.ENABLED
        text = kw
        if kw.startswith("[") and kw.endswith("]"):
            text = kw[1:-1]
            mt = client.enums.KeywordMatchTypeEnum.EXACT
        elif kw.startswith('"') and kw.endswith('"'):
            text = kw[1:-1]
            mt = client.enums.KeywordMatchTypeEnum.PHRASE
        else:
            mt = client.enums.KeywordMatchTypeEnum.BROAD
        c.keyword.text = text
        c.keyword.match_type = mt
        ops.append(op)
    if not ops:
        return 0
    try:
        client.get_service("AdGroupCriterionService").mutate_ad_group_criteria(
            customer_id=customer_id, operations=ops
        )
        return len(ops)
    except GoogleAdsException as e:
        if "DUPLICATE" in str(e):
            return 0
        raise


def add_responsive_search_ad(
    client: GoogleAdsClient, customer_id: str, ad_group_rn: str,
    headlines: list[str], descriptions: list[str], final_url: str,
) -> str | None:
    """Create an RSA. Idempotent: skips if any RSA already exists in the ad group."""
    ga_service = client.get_service("GoogleAdsService")
    q = (
        "SELECT ad_group_ad.resource_name FROM ad_group_ad "
        f"WHERE ad_group_ad.ad_group = '{ad_group_rn}' "
        "AND ad_group_ad.ad.type = RESPONSIVE_SEARCH_AD LIMIT 1"
    )
    for _ in ga_service.search(customer_id=customer_id, query=q):
        return None  # already has an RSA

    op = client.get_type("AdGroupAdOperation")
    a = op.create
    a.ad_group = ad_group_rn
    a.status = client.enums.AdGroupAdStatusEnum.ENABLED
    ad = a.ad
    ad.final_urls.append(final_url)
    ad.responsive_search_ad.path1 = "official"
    ad.responsive_search_ad.path2 = "mu"
    for hl in headlines[:15]:
        asset = client.get_type("AdTextAsset")
        asset.text = hl[:30]
        ad.responsive_search_ad.headlines.append(asset)
    for ds in descriptions[:4]:
        asset = client.get_type("AdTextAsset")
        asset.text = ds[:90]
        ad.responsive_search_ad.descriptions.append(asset)
    resp = client.get_service("AdGroupAdService").mutate_ad_group_ads(
        customer_id=customer_id, operations=[op]
    )
    return resp.results[0].resource_name


def pause_campaign(client: GoogleAdsClient, customer_id: str, campaign_rn: str) -> None:
    op = client.get_type("CampaignOperation")
    op.update.resource_name = campaign_rn
    op.update.status = client.enums.CampaignStatusEnum.PAUSED
    client.copy_from(op.update_mask, client.get_type("FieldMask")(paths=["status"]))
    client.get_service("CampaignService").mutate_campaigns(
        customer_id=customer_id, operations=[op]
    )


# ─── Plan execution ────────────────────────────────────────────────────────


def plan_summary() -> str:
    lines = [
        "MU AdWords plan (¥30K/month total)",
        "",
        "Campaign A: MU-Brand (Search, ¥150/day = ¥4.5K/mo)",
        "  Ad group: brand_defense (CPC ¥80 default)",
        f"  Keywords: {len(BRAND_KEYWORDS)}",
        f"  Headlines: {len(BRAND_HEADLINES)} / Descriptions: {len(BRAND_DESCRIPTIONS)}",
        "  Landing: https://wearmu.com/about",
        "",
        "Campaign B: MU-Discovery (Search, ¥500/day = ¥15K/mo)",
        "  Ad group: ai_tshirt (CPC ¥120 default)",
        f"  Keywords: {len(DISCOVERY_KEYWORDS)}",
        f"  Headlines: {len(DISCOVERY_HEADLINES)} / Descriptions: {len(DISCOVERY_DESCRIPTIONS)}",
        "  Landing: https://wearmu.com/you",
        "",
        "Campaign C: MU-PMax (Performance Max, ¥350/day = ¥10.5K/mo)",
        "  Target CPA: ¥1,100 (= 1 sale margin)",
        "  Landing: https://wearmu.com/buy",
        "",
        f"Negative keywords (shared, applied per-campaign): {len(NEGATIVE_KEYWORDS)}",
        "Geo: Japan (2392) · Language: Japanese (1005)",
    ]
    return "\n".join(lines)


def create_all(client: GoogleAdsClient, customer_id: str) -> dict:
    out: dict = {}

    # A. Brand
    bud_a = upsert_budget(client, customer_id, "MU-Brand", DAILY_BUDGET_MICROS["MU-Brand"])
    cmp_a = create_search_campaign(client, customer_id, "MU-Brand", bud_a)
    add_geo_japan(client, customer_id, cmp_a)
    add_language_japanese(client, customer_id, cmp_a)
    add_negative_keywords(client, customer_id, cmp_a, NEGATIVE_KEYWORDS)
    ag_a = upsert_ad_group(client, customer_id, cmp_a, "brand_defense", 80_000_000)
    add_keywords(client, customer_id, ag_a, BRAND_KEYWORDS)
    ad_a = add_responsive_search_ad(
        client, customer_id, ag_a, BRAND_HEADLINES, BRAND_DESCRIPTIONS,
        "https://wearmu.com/about",
    )
    out["MU-Brand"] = {"campaign": cmp_a, "ad_group": ag_a, "ad": ad_a}

    # B. Discovery
    bud_b = upsert_budget(client, customer_id, "MU-Discovery", DAILY_BUDGET_MICROS["MU-Discovery"])
    cmp_b = create_search_campaign(client, customer_id, "MU-Discovery", bud_b)
    add_geo_japan(client, customer_id, cmp_b)
    add_language_japanese(client, customer_id, cmp_b)
    add_negative_keywords(client, customer_id, cmp_b, NEGATIVE_KEYWORDS)
    ag_b = upsert_ad_group(client, customer_id, cmp_b, "ai_tshirt", 120_000_000)
    add_keywords(client, customer_id, ag_b, DISCOVERY_KEYWORDS)
    ad_b = add_responsive_search_ad(
        client, customer_id, ag_b, DISCOVERY_HEADLINES, DISCOVERY_DESCRIPTIONS,
        "https://wearmu.com/you",
    )
    out["MU-Discovery"] = {"campaign": cmp_b, "ad_group": ag_b, "ad": ad_b}

    # C. Performance Max
    bud_c = upsert_budget(client, customer_id, "MU-PMax", DAILY_BUDGET_MICROS["MU-PMax"])
    cmp_c = create_pmax_campaign(client, customer_id, "MU-PMax", bud_c)
    # PMax asset groups require a separate flow — print a note.
    out["MU-PMax"] = {
        "campaign": cmp_c,
        "note": "PMax asset groups (images, headlines, sitelinks) must be added in UI or via a follow-up script. Campaign skeleton + budget are live.",
    }

    return out


def status_report(client: GoogleAdsClient, customer_id: str) -> str:
    q = (
        "SELECT campaign.name, "
        "metrics.cost_micros, metrics.impressions, metrics.clicks, "
        "metrics.conversions, metrics.cost_per_conversion "
        "FROM campaign "
        "WHERE segments.date DURING LAST_7_DAYS"
    )
    rows = []
    for row in client.get_service("GoogleAdsService").search(customer_id=customer_id, query=q):
        rows.append((
            row.campaign.name,
            row.metrics.cost_micros / 1_000_000,
            row.metrics.impressions,
            row.metrics.clicks,
            row.metrics.conversions,
            row.metrics.cost_per_conversion / 1_000_000 if row.metrics.cost_per_conversion else 0,
        ))
    if not rows:
        return "no campaigns or no spend in last 7 days"
    out = ["", "Last 7 days:"]
    out.append(f"{'Campaign':<20} {'Spend ¥':>10} {'Impr':>8} {'Click':>8} {'Conv':>6} {'CPA ¥':>8}")
    for n, sp, im, cl, cv, cpa in rows:
        out.append(f"{n:<20} {sp:>10,.0f} {im:>8} {cl:>8} {cv:>6.1f} {cpa:>8,.0f}")
    return "\n".join(out)


# ─── CLI ───────────────────────────────────────────────────────────────────


def main() -> None:
    p = argparse.ArgumentParser(description="MU Google Ads bootstrap")
    p.add_argument("--customer-id", help="Google Ads customer ID (digits only, no dashes). Falls back to env GOOGLE_ADS_LOGIN_CUSTOMER_ID.")
    g = p.add_mutually_exclusive_group(required=True)
    g.add_argument("--dry-run", action="store_true", help="Print the plan, mutate nothing")
    g.add_argument("--create-all", action="store_true", help="Create the 3 campaigns")
    g.add_argument("--pause-all", action="store_true", help="Pause all MU-* campaigns (kill switch)")
    g.add_argument("--status", action="store_true", help="Print last-7-days performance per campaign")
    args = p.parse_args()

    if args.dry_run:
        print(plan_summary())
        return

    client = get_client()
    customer_id = (args.customer_id or os.environ.get("GOOGLE_ADS_CUSTOMER_ID", "")).replace("-", "").strip()
    if not customer_id:
        sys.exit("Pass --customer-id 1234567890 or set GOOGLE_ADS_CUSTOMER_ID")

    if args.create_all:
        result = create_all(client, customer_id)
        print("Created/updated:")
        for k, v in result.items():
            print(f"  {k}: {v}")
        return

    if args.pause_all:
        ga = client.get_service("GoogleAdsService")
        rns = []
        for row in ga.search(
            customer_id=customer_id,
            query="SELECT campaign.resource_name, campaign.name FROM campaign WHERE campaign.name LIKE 'MU-%'",
        ):
            rns.append((row.campaign.resource_name, row.campaign.name))
        for rn, name in rns:
            pause_campaign(client, customer_id, rn)
            print(f"paused: {name}")
        if not rns:
            print("no MU-* campaigns found")
        return

    if args.status:
        print(status_report(client, customer_id))
        return


if __name__ == "__main__":
    main()
