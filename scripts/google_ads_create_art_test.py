#!/usr/bin/env python3
"""
Create the MU /lp/art test campaign on Google Ads.

Spec (small live test, designed to be killable in 1 click):
  - Budget: ¥1,500 / day (UNSHARED) — 3-day spend cap = ¥4,500
  - Bidding: MANUAL_CPC (JiuFlow learned: TARGET_SPEND burns money)
  - Targeting: Japan, language 'ja', all devices
  - Geo: country-level (no zip granularity)
  - Network: Google Search only (no Display, no Search Partners)
  - Ad group: single "art" ad group
  - Keywords: phrase-match seed list (see KEYWORDS below)
  - Negative keywords: shared list applied at campaign level
  - RSA: 15 headlines + 4 descriptions
  - Final URL: https://wearmu.com/lp/art?utm_source=google&utm_medium=cpc
               &utm_campaign=mu_art_test&utm_content={creative}&utm_term={keyword}
  - All assets marked PAUSED at creation — user un-pauses after eyeball check.

Auth: reads ~/.config/google-ads/google-ads.yaml (use the bootstrap script
to create one). Requires `pip install google-ads` (>= v17).

Run:
  python scripts/google_ads_create_art_test.py --customer-id 1234567890
  python scripts/google_ads_create_art_test.py --customer-id 1234567890 --dry-run

Output: prints campaign_id, ad_group_id, ad_id. The campaign starts PAUSED.
Enable it from ads.google.com → MU → /lp/art test → status → Enabled.
"""
from __future__ import annotations
import argparse, sys, time

BUDGET_JPY_PER_DAY = 1500
BIDDING = "MANUAL_CPC"
DEFAULT_CPC_JPY = 110   # exact bid in micros = JPY × 1_000_000
DEFAULT_CPC_MICROS = DEFAULT_CPC_JPY * 1_000_000
COUNTRY_ID_JP = 2392    # geoTargetConstant for Japan
LANG_ID_JA = 1005       # languageConstant for Japanese
FINAL_URL = (
    "https://wearmu.com/lp/art"
    "?utm_source=google&utm_medium=cpc&utm_campaign=mu_art_test"
    "&utm_content={creative}&utm_term={keyword}"
)

# ── Phrase-match keywords (each becomes a Keyword resource with PHRASE match).
KEYWORDS_PHRASE = [
    "現代アート Tシャツ",
    "アート ファッション",
    "コンセプチュアルアート ファッション",
    "ジェネレーティブアート Tシャツ",
    "generative art apparel",
    "Sol LeWitt fan",
    "草間彌生 似た アート Tシャツ",
    "アート 着る",
    "日付 アート",
    "河原温 ファン",
    "アート ブランド ジャパン",
    "現代美術 グッズ",
]

KEYWORDS_BROAD_MATCH = [
    "コンセプチュアルアート",
    "ジェネラティブアート",
    "generative art",
    "現代アート 着る",
]

NEGATIVE_KEYWORDS = [
    "無料", "タダ", "torrent", "クラック", "中古", "リユース", "古着",
    "似たような", "パクリ", "コピー", "偽物", "レプリカ", "名入れ",
    "ZOZO", "GU", "ユニクロ", "H&M", "SHEIN",
    "キャラクター", "アニメ", "鬼滅", "推し活", "パロディ",
    "アダルト", "求人", "転職", "バイト", "副業", "仕事",
    "量産",
]

HEADLINES = [
    "着れるアート、 毎日 一つの絵。",
    "ジェネラティブアートの Tシャツ",
    "日付が、 絵になる。 ¥7,800",
    "人間は介在しない 1点もの",
    "毎日 一つの絵が生まれる",
    "コンセプチュアルアート × Tシャツ",
    "弟子屈の気象を、 絵に落とす",
    "AI 生成 1-of-1 Tシャツ",
    "¥7,800 / 原価 ¥5,700 公開",
    "EU プリント / GOTS organic",
    "1日 24 枚 → 採用 1 枚",
    "服に残る、 今日の存在証明",
    "利益の 50% を生まれた町へ",
    "注文後にだけ刷る、 廃棄 0",
    "デザイナー 0 のアートブランド",
]

DESCRIPTIONS = [
    "「日付ごとの存在」 を一枚の T シャツに落としたら、 毎日 1 つだけ残るデザインになった。 ¥7,800。",
    "北海道弟子屈の気温と月相を、 AI が読む。 同じ柄は二度と作られない。 EU プリント、 GOTS organic。",
    "1日 24 枚生まれて、 採用は 1 枚だけ。 注文が来てから刷る。 在庫廃棄 0。",
    "デザイナー 0 人のブランド。 利益の 50% を生まれた町 (北海道弟子屈) に寄付。",
]


def micros(jpy: int) -> int:
    return jpy * 1_000_000


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--customer-id", required=True,
                        help="Google Ads customer ID without dashes (10 digits)")
    parser.add_argument("--login-customer-id",
                        help="MCC login customer ID (digits only). If omitted, "
                             "google-ads.yaml's login_customer_id is used.")
    parser.add_argument("--dry-run", action="store_true",
                        help="Print everything, do not call the API")
    parser.add_argument("--name", default="MU · /lp/art · test",
                        help="Campaign name")
    args = parser.parse_args()

    print(f"== Plan ==")
    print(f"  Campaign:    {args.name}  (PAUSED at creation)")
    print(f"  Customer:    {args.customer_id}")
    print(f"  Budget:      ¥{BUDGET_JPY_PER_DAY}/day  (3-day cap ≈ ¥{BUDGET_JPY_PER_DAY*3})")
    print(f"  Bidding:     {BIDDING} @ ¥{DEFAULT_CPC_JPY} default CPC")
    print(f"  Geo:         JP only  (geoTargetConstant {COUNTRY_ID_JP})")
    print(f"  Language:    ja  (languageConstant {LANG_ID_JA})")
    print(f"  Network:     Search only (no Display, no Search Partners)")
    print(f"  Keywords:    {len(KEYWORDS_PHRASE)} phrase + {len(KEYWORDS_BROAD_MATCH)} broad")
    print(f"  Negatives:   {len(NEGATIVE_KEYWORDS)}")
    print(f"  Final URL:   {FINAL_URL}")
    print()

    if args.dry_run:
        print("(--dry-run: not calling Google Ads API)")
        return 0

    try:
        from google.ads.googleads.client import GoogleAdsClient
        from google.ads.googleads.errors import GoogleAdsException
    except ImportError:
        print("ERROR: google-ads SDK not installed.  pip install google-ads", file=sys.stderr)
        return 1

    cfg = {"use_proto_plus": True}
    if args.login_customer_id:
        cfg["login_customer_id"] = args.login_customer_id
    client = GoogleAdsClient.load_from_storage()  # reads ~/.config/google-ads/google-ads.yaml

    customer_id = args.customer_id

    # 1) Campaign budget
    budget_svc = client.get_service("CampaignBudgetService")
    budget_op = client.get_type("CampaignBudgetOperation")
    budget = budget_op.create
    budget.name = f"{args.name} budget {int(time.time())}"
    budget.amount_micros = micros(BUDGET_JPY_PER_DAY)
    budget.delivery_method = client.enums.BudgetDeliveryMethodEnum.STANDARD
    budget.explicitly_shared = False
    budget_resp = budget_svc.mutate_campaign_budgets(
        customer_id=customer_id, operations=[budget_op])
    budget_rn = budget_resp.results[0].resource_name
    print(f"[ok] budget: {budget_rn}")

    # 2) Campaign  (PAUSED, Search only, manual CPC)
    camp_svc = client.get_service("CampaignService")
    camp_op = client.get_type("CampaignOperation")
    camp = camp_op.create
    camp.name = args.name
    camp.status = client.enums.CampaignStatusEnum.PAUSED
    camp.advertising_channel_type = client.enums.AdvertisingChannelTypeEnum.SEARCH
    camp.manual_cpc.enhanced_cpc_enabled = False
    camp.campaign_budget = budget_rn
    camp.network_settings.target_google_search = True
    camp.network_settings.target_search_network = False
    camp.network_settings.target_content_network = False
    camp.network_settings.target_partner_search_network = False
    camp_resp = camp_svc.mutate_campaigns(customer_id=customer_id, operations=[camp_op])
    camp_rn = camp_resp.results[0].resource_name
    print(f"[ok] campaign: {camp_rn}")

    # 3) Geo + Language criteria (campaign-level)
    cc_svc = client.get_service("CampaignCriterionService")
    crit_ops = []

    geo_op = client.get_type("CampaignCriterionOperation")
    geo = geo_op.create
    geo.campaign = camp_rn
    geo.location.geo_target_constant = f"geoTargetConstants/{COUNTRY_ID_JP}"
    crit_ops.append(geo_op)

    lang_op = client.get_type("CampaignCriterionOperation")
    lang = lang_op.create
    lang.campaign = camp_rn
    lang.language.language_constant = f"languageConstants/{LANG_ID_JA}"
    crit_ops.append(lang_op)

    # Campaign-level negative keywords
    for neg in NEGATIVE_KEYWORDS:
        nop = client.get_type("CampaignCriterionOperation")
        n = nop.create
        n.campaign = camp_rn
        n.negative = True
        n.keyword.text = neg
        n.keyword.match_type = client.enums.KeywordMatchTypeEnum.PHRASE
        crit_ops.append(nop)

    cc_svc.mutate_campaign_criteria(customer_id=customer_id, operations=crit_ops)
    print(f"[ok] geo + language + {len(NEGATIVE_KEYWORDS)} negatives applied")

    # 4) Ad group
    ag_svc = client.get_service("AdGroupService")
    ag_op = client.get_type("AdGroupOperation")
    ag = ag_op.create
    ag.name = "art / phrase + broad"
    ag.campaign = camp_rn
    ag.status = client.enums.AdGroupStatusEnum.ENABLED
    ag.type_ = client.enums.AdGroupTypeEnum.SEARCH_STANDARD
    ag.cpc_bid_micros = DEFAULT_CPC_MICROS
    ag_resp = ag_svc.mutate_ad_groups(customer_id=customer_id, operations=[ag_op])
    ag_rn = ag_resp.results[0].resource_name
    print(f"[ok] ad group: {ag_rn}")

    # 5) Keywords
    agc_svc = client.get_service("AdGroupCriterionService")
    kw_ops = []
    for kw in KEYWORDS_PHRASE:
        op = client.get_type("AdGroupCriterionOperation")
        c = op.create
        c.ad_group = ag_rn
        c.status = client.enums.AdGroupCriterionStatusEnum.ENABLED
        c.keyword.text = kw
        c.keyword.match_type = client.enums.KeywordMatchTypeEnum.PHRASE
        kw_ops.append(op)
    for kw in KEYWORDS_BROAD_MATCH:
        op = client.get_type("AdGroupCriterionOperation")
        c = op.create
        c.ad_group = ag_rn
        c.status = client.enums.AdGroupCriterionStatusEnum.ENABLED
        c.keyword.text = kw
        c.keyword.match_type = client.enums.KeywordMatchTypeEnum.BROAD
        kw_ops.append(op)
    agc_svc.mutate_ad_group_criteria(customer_id=customer_id, operations=kw_ops)
    print(f"[ok] {len(kw_ops)} keywords created")

    # 6) Responsive Search Ad
    aga_svc = client.get_service("AdGroupAdService")
    aga_op = client.get_type("AdGroupAdOperation")
    a = aga_op.create
    a.ad_group = ag_rn
    a.status = client.enums.AdGroupAdStatusEnum.PAUSED
    rsa = a.ad.responsive_search_ad
    for h in HEADLINES:
        asset = client.get_type("AdTextAsset")
        asset.text = h
        rsa.headlines.append(asset)
    for d in DESCRIPTIONS:
        asset = client.get_type("AdTextAsset")
        asset.text = d
        rsa.descriptions.append(asset)
    a.ad.final_urls.append(FINAL_URL)
    a.ad.tracking_url_template = (
        "{lpurl}?utm_source=google&utm_medium=cpc"
        "&utm_campaign=mu_art_test&utm_term={keyword}&gclid={gclid}"
    )
    aga_resp = aga_svc.mutate_ad_group_ads(customer_id=customer_id, operations=[aga_op])
    print(f"[ok] RSA ad: {aga_resp.results[0].resource_name}")

    print()
    print("== Done — campaign is PAUSED ==")
    print(f"  ads.google.com → Campaigns → {args.name} → status → Enabled")
    print(f"  Budget cap ≈ ¥{BUDGET_JPY_PER_DAY * 3} over 3 days.")
    print(f"  Watch /lp/art impressions + CTR after enable.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
