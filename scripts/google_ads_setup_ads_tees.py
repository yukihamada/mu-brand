#!/usr/bin/env python3
"""
MU Google Ads — ad-targeted tees campaign.

Creates ONE campaign "MU-AdsTees-Search" with 5 ad groups, one per niche.
PAUSED on creation. Uses helpers from google_ads_setup.py.

Run:
  python3 scripts/google_ads_setup_ads_tees.py --dry-run
  python3 scripts/google_ads_setup_ads_tees.py --create-all
  python3 scripts/google_ads_setup_ads_tees.py --pause-all
"""
from __future__ import annotations
import argparse, os, sys
from pathlib import Path

# Import helpers from sibling script
sys.path.insert(0, str(Path(__file__).resolve().parent))
import google_ads_setup as base  # noqa: E402

CAMPAIGN_NAME = "MU-AdsTees-Search"
DAILY_BUDGET_MICROS = 500_000_000  # ¥500/day

LANDING_BASE = "https://wearmu.com/products"

# Production product IDs (assigned 2026-05-16, see prod_import_ads_tees.py output)
AD_GROUPS = [
    {
        "name": "jujitsu",
        "cpc_micros": 100_000_000,  # ¥100 max CPC
        "keywords": [
            "ノーギ Tシャツ", "ノーギ Tシャツ メンズ",
            "黒帯 Tシャツ", "黒帯 Tシャツ 柔術",
            "青帯 Tシャツ", "紫帯 Tシャツ", "白帯 Tシャツ",
            "柔術 Tシャツ", "柔術 Tシャツ プレゼント",
            "ベリンボロ Tシャツ", "柔術 Tシャツ 面白い",
        ],
        "headlines": [
            "ノーギ柔術 Tシャツ", "黒帯まで歩む人へ", "MUの柔術ライン",
            "白帯Day One ¥5,000", "青帯/紫帯/黒帯", "BJJ 内輪Tシャツ",
            "DTG 1点ずつプリント", "柔術ギフトに最適",
        ],
        "descriptions": [
            "ノーギ・帯色・技名。柔術家のための1着、MUがDTGで1点ずつ。",
            "黒帯 ¥5,500 プレミア、白/青/紫帯 ¥5,000。試合・道場・ギフトに。",
            "在庫ゼロ受注生産、利益50%は弟子屈町へ。MU公式 wearmu.com。",
            "Tap or Nap / Berimbolo Specialist 等の柔術文化Tもあります。",
        ],
        "final_url": f"{LANDING_BASE}/ads_jujitsu/1034",
    },
    {
        "name": "regional",
        "cpc_micros": 80_000_000,
        "keywords": [
            "三田 Tシャツ", "三田 Tシャツ 港区",
            "北参道 Tシャツ", "北参道 BJJ Tシャツ",
            "弟子屈 Tシャツ", "弟子屈 Tシャツ 北海道",
            "川湯温泉 Tシャツ", "川湯温泉 Tシャツ お土産",
        ],
        "headlines": [
            "三田/MITA Tee ¥5,000", "北参道BJJ Tシャツ",
            "弟子屈 Vintage Badge", "川湯温泉 お土産Tee",
            "地域プライドの1着", "MU 地域シリーズ",
            "Made in Japan DTG", "MU公式 wearmu.com",
        ],
        "descriptions": [
            "三田・北参道・弟子屈・川湯温泉。地名を着る、MUの地域シリーズ。",
            "1着 ¥5,000、DTG プリント1枚ずつ。地元の友人/お客様への手土産にも。",
            "在庫ゼロ受注生産、利益50%は弟子屈町に寄付。原価公開ブランド。",
        ],
        "final_url": f"{LANDING_BASE}/ads_regional/1042",
    },
    {
        "name": "kokon",
        "cpc_micros": 100_000_000,
        "keywords": [
            "焼肉 Tシャツ", "焼肉 Tシャツ プレゼント",
            "焼肉 Tシャツ ギフト", "焼肉好き プレゼント",
            "肉の日 Tシャツ", "焼肉古今",
            "焼肉 ロゴ Tシャツ",
        ],
        "headlines": [
            "焼肉好きへのギフト", "焼肉古今 公式コラボ",
            "肉の日 (29日) Tシャツ", "焼肉Tシャツ ¥5,000",
            "彼氏/夫/お父さんへ", "MU × 焼肉古今",
            "kokon.tokyo Official", "MU公式 wearmu.com",
        ],
        "descriptions": [
            "西麻布の名店 焼肉古今 公式コラボ。肉好き、焼肉好きのギフトに。",
            "「焼肉好き」「肉の日 29」「焼肉古今ロゴ」3 SKU、各 ¥5,000。",
            "1点ずつ DTG プリント、受注生産。お父さん/彼氏/夫の誕生日に。",
        ],
        "final_url": f"{LANDING_BASE}/ads_kokon/1046",
    },
    {
        "name": "profession",
        "cpc_micros": 100_000_000,
        "keywords": [
            "看護師 Tシャツ", "看護師 Tシャツ ICU",
            "看護師 Tシャツ プレゼント", "ナース Tシャツ",
            "エンジニア Tシャツ", "プログラマー Tシャツ",
            "プログラマー Tシャツ ギフト",
        ],
        "headlines": [
            "ICU Night Shift Tee", "看護師さんへのギフト",
            "エンジニア Tee 内輪ネタ", "Coffee Code Compile Tee",
            "職業プライド ¥5,000", "Made in Japan DTG",
            "受注生産・在庫ゼロ", "MU公式 wearmu.com",
        ],
        "descriptions": [
            "ICU夜勤・エンジニア。職業の内輪ネタを着るMUの部族シリーズ。",
            "1着 ¥5,000、DTGプリント。誕生日/退職祝い/同僚へのプレゼントに。",
            "MU 公式 wearmu.com、利益50%は弟子屈町に寄付。透明原価。",
        ],
        "final_url": f"{LANDING_BASE}/ads_profession/1049",
    },
    {
        "name": "event",
        "cpc_micros": 120_000_000,
        "keywords": [
            "SOLUNA FEST Tシャツ", "SOLUNA FEST 2026",
            "父の日 釣り Tシャツ", "父の日 プレゼント Tシャツ",
            "グラップリング 大会 Tシャツ", "グラップリング 記念",
            "釣り好き Tシャツ ギフト",
        ],
        "headlines": [
            "SOLUNA FEST 公式Tee", "父の日 釣り好きDad",
            "Worlds OK-est Fishing Dad", "East Japan Grappling 2026",
            "イベント記念 ¥5,500", "ギフト/お土産に最適",
            "DTG 1枚ずつ印刷", "MU公式 wearmu.com",
        ],
        "descriptions": [
            "SOLUNA FEST HAWAII 2026 公式Tee。フェス参加/記念ギフトに。",
            "父の日に — World's OK-est Fishing Dad badge tee ¥5,000。",
            "東日本グラップリングオープン 2026 記念Tee ¥5,500。",
        ],
        "final_url": f"{LANDING_BASE}/ads_event/1051",
    },
]

# Reuse same negatives as base setup, plus a couple more specific to ads_tees
EXTRA_NEGATIVE = ["印刷", "オリジナル Tシャツ 作成", "オリジナルプリント"]


def load_env():
    env = Path("/Users/yuki/.env")
    if env.exists():
        for line in env.read_text().splitlines():
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            k, v = line.split("=", 1)
            os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))


def print_plan():
    print(f"\n=== {CAMPAIGN_NAME} (¥{DAILY_BUDGET_MICROS // 1_000_000}/day = ¥{DAILY_BUDGET_MICROS * 30 // 1_000_000:,}/mo) ===")
    print("Status on create: PAUSED\n")
    total_kw = 0
    for ag in AD_GROUPS:
        kw_count = len(ag["keywords"])
        total_kw += kw_count
        print(f"  ┌─ ad_group: {ag['name']:<12} ({kw_count} keywords, max CPC ¥{ag['cpc_micros']//1_000_000})")
        print(f"  │  landing: {ag['final_url']}")
        print(f"  │  KWs: {', '.join(ag['keywords'][:3])}...")
        print(f"  │  ads: {len(ag['headlines'])} headlines / {len(ag['descriptions'])} descriptions")
        print(f"  └─")
    print(f"\n  Total: {len(AD_GROUPS)} ad groups, {total_kw} keywords")
    print(f"  Negative keywords: {len(base.NEGATIVE_KEYWORDS) + len(EXTRA_NEGATIVE)}")
    print()


def get_customer_id() -> str:
    cid = (os.environ.get("GOOGLE_ADS_CUSTOMER_ID")
           or os.environ.get("GOOGLE_ADS_LOGIN_CUSTOMER_ID"))
    if cid:
        return cid.replace("-", "")
    # Last fallback: read from google-ads.yaml
    yaml_path = Path.home() / ".config" / "google-ads" / "google-ads.yaml"
    if yaml_path.exists():
        for line in yaml_path.read_text().splitlines():
            if line.startswith("login_customer_id:"):
                return line.split(":", 1)[1].strip()
    sys.exit("Customer ID not found. Set GOOGLE_ADS_CUSTOMER_ID or run google_ads_bootstrap.py")


def create_all():
    client = base.get_client()
    cid = get_customer_id()
    print(f"→ Customer ID: {cid}")
    print(f"→ Creating {CAMPAIGN_NAME} ...")

    budget = base.upsert_budget(client, cid, CAMPAIGN_NAME, DAILY_BUDGET_MICROS)
    cmp_rn = base.create_search_campaign(client, cid, CAMPAIGN_NAME, budget)
    print(f"  ✓ campaign: {cmp_rn}")

    base.add_geo_japan(client, cid, cmp_rn)
    base.add_language_japanese(client, cid, cmp_rn)
    base.add_negative_keywords(client, cid, cmp_rn, base.NEGATIVE_KEYWORDS + EXTRA_NEGATIVE)
    print(f"  ✓ geo: JP, lang: JP, negatives: {len(base.NEGATIVE_KEYWORDS) + len(EXTRA_NEGATIVE)}")

    for ag in AD_GROUPS:
        ag_rn = base.upsert_ad_group(client, cid, cmp_rn, ag["name"], ag["cpc_micros"])
        # Keywords need to be wrapped in match-type syntax (phrase = "")
        kws_phrase = [f'"{k}"' for k in ag["keywords"]]
        base.add_keywords(client, cid, ag_rn, kws_phrase)
        ad_rn = base.add_responsive_search_ad(
            client, cid, ag_rn, ag["headlines"], ag["descriptions"], ag["final_url"]
        )
        ad_label = ad_rn.split('/')[-1][:20] if ad_rn else "exists"
        print(f"  ✓ ad_group: {ag['name']:<12} ({len(ag['keywords'])} KWs, ad: {ad_label})")

    # Force PAUSED for safety — user must enable in UI
    base.pause_campaign(client, cid, cmp_rn)
    print(f"\n✅ Campaign {CAMPAIGN_NAME} created PAUSED.")
    print(f"   Enable: https://ads.google.com/aw/campaigns?ocid={cid}")


def pause_all():
    client = base.get_client()
    cid = get_customer_id()
    rn = base.find_campaign_by_name(client, cid, CAMPAIGN_NAME)
    if not rn:
        sys.exit(f"Campaign {CAMPAIGN_NAME} not found.")
    base.pause_campaign(client, cid, rn)
    print(f"✅ Paused {CAMPAIGN_NAME}")


def status():
    client = base.get_client()
    cid = get_customer_id()
    q = (
        "SELECT campaign.name, campaign.status, "
        "metrics.cost_micros, metrics.impressions, metrics.clicks, "
        "metrics.conversions FROM campaign "
        f"WHERE campaign.name = '{CAMPAIGN_NAME}' "
        "AND segments.date DURING LAST_7_DAYS"
    )
    found = False
    for row in client.get_service("GoogleAdsService").search(customer_id=cid, query=q):
        found = True
        print(f"  {row.campaign.name}: {row.campaign.status.name}")
        print(f"    spend ¥{row.metrics.cost_micros/1_000_000:,.0f}, impr {row.metrics.impressions}, "
              f"clicks {row.metrics.clicks}, conv {row.metrics.conversions:.1f}")
    if not found:
        print(f"  {CAMPAIGN_NAME}: no data in last 7 days (probably just created)")


def main():
    ap = argparse.ArgumentParser()
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--dry-run", action="store_true")
    g.add_argument("--create-all", action="store_true")
    g.add_argument("--pause-all", action="store_true")
    g.add_argument("--status", action="store_true")
    args = ap.parse_args()
    load_env()
    if args.dry_run:
        print_plan()
    elif args.create_all:
        print_plan()
        create_all()
    elif args.pause_all:
        pause_all()
    elif args.status:
        status()


if __name__ == "__main__":
    main()
