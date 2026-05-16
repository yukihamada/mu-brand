#!/usr/bin/env python3
"""
MU Google Ads — ad-targeted tees campaign (2026-05-16).

Creates ONE campaign "MU-AdsTees-Search" with 5 ad groups, one per niche:
  - jujitsu   (柔術 / 帯色 / 技名)
  - regional  (三田 / 北参道 / 弟子屈 / 川湯)
  - kokon     (焼肉古今)
  - profession (看護師 / エンジニア)
  - event     (SOLUNA FEST / 父の日)

Budget: ¥500/day (¥15K/mo). Status: PAUSED on creation — user must
manually enable in Google Ads UI after reviewing keywords + bids.

Prereq: OAuth bootstrap complete (refresh_token + customer_id),
~/.config/google-ads/google-ads.yaml populated. See google_ads_README.md.

Run:
  python3 scripts/google_ads_setup_ads_tees.py --dry-run    # show plan only
  python3 scripts/google_ads_setup_ads_tees.py --create-all # create (PAUSED)
  python3 scripts/google_ads_setup_ads_tees.py --pause-all  # pause everything
  python3 scripts/google_ads_setup_ads_tees.py --status     # current stats
"""
from __future__ import annotations
import argparse, os, sys
from pathlib import Path

CAMPAIGN_NAME = "MU-AdsTees-Search"
DAILY_BUDGET_MICROS = 500_000_000  # ¥500/day → ¥15K/mo
CUSTOMER_ID_ENV = "GOOGLE_ADS_CUSTOMER_ID"

# Landing pages: /products/<brand>/<id> — once products live on wearmu.com
LANDING_BASE = "https://wearmu.com/products"

# ─── Ad groups: (ad_group_name, keywords, headlines, descriptions, product_ids) ───
AD_GROUPS = [
    {
        "name": "jujitsu",
        "keywords": [
            "\"ノーギ Tシャツ\"", "\"ノーギ Tシャツ メンズ\"",
            "\"黒帯 Tシャツ\"", "\"黒帯 Tシャツ 柔術\"",
            "\"青帯 Tシャツ\"", "\"紫帯 Tシャツ\"", "\"白帯 Tシャツ\"",
            "\"柔術 Tシャツ\"", "\"柔術 Tシャツ プレゼント\"",
            "\"ベリンボロ Tシャツ\"", "\"柔術 Tシャツ 面白い\"",
        ],
        "headlines": [
            "ノーギ柔術 Tシャツ", "黒帯まで歩む人へ", "MUの柔術ライン",
            "白帯Day One ¥4,900", "青帯/紫帯/黒帯 揃ってる", "BJJ 内輪Tシャツ",
            "DTG 1点ずつプリント", "柔術ギフトに最適",
        ],
        "descriptions": [
            "ノーギ・帯色・技名。柔術家のための1着、MUがDTGで1点ずつ。",
            "黒帯 ¥5,500 プレミア、白/青/紫帯 ¥4,900。試合・道場・ギフトに。",
            "在庫ゼロ受注生産、利益50%は弟子屈町へ。MU公式 wearmu.com。",
            "Tap or Nap / Berimbolo Specialist 等の柔術文化ネタTもあります。",
        ],
        # All 8 jujitsu SKUs (#195-202) — Google PMax could optimize, here we
        # link to a brand listing page that filters ads_jujitsu
        "final_url": f"{LANDING_BASE}/ads_jujitsu/195",
    },
    {
        "name": "regional",
        "keywords": [
            "\"三田 Tシャツ\"", "\"三田 Tシャツ 港区\"",
            "\"北参道 Tシャツ\"", "\"北参道 BJJ Tシャツ\"",
            "\"弟子屈 Tシャツ\"", "\"弟子屈 Tシャツ 北海道\"",
            "\"川湯温泉 Tシャツ\"", "\"川湯温泉 Tシャツ お土産\"",
        ],
        "headlines": [
            "三田/MITA Tee ¥4,900", "北参道BJJ Tシャツ",
            "弟子屈 Vintage Badge", "川湯温泉 お土産Tee",
            "地域プライドの1着", "MU 地域シリーズ",
            "Made in Japan DTG", "MU公式 wearmu.com",
        ],
        "descriptions": [
            "三田・北参道・弟子屈・川湯温泉。地名を着る、MUの地域シリーズ。",
            "1着 ¥4,900、DTG プリント1枚ずつ。地元の友人/お客様への手土産にも。",
            "在庫ゼロ受注生産、利益50%は弟子屈町に寄付。原価公開ブランド。",
        ],
        "final_url": f"{LANDING_BASE}/ads_regional/203",
    },
    {
        "name": "kokon",
        "keywords": [
            "\"焼肉 Tシャツ\"", "\"焼肉 Tシャツ プレゼント\"",
            "\"焼肉 Tシャツ ギフト\"", "\"焼肉好き プレゼント\"",
            "\"肉の日 Tシャツ\"", "\"焼肉古今\"",
            "\"焼肉 ロゴ Tシャツ\"",
        ],
        "headlines": [
            "焼肉好きへのギフト", "焼肉古今 公式コラボ",
            "肉の日 (29日) Tシャツ", "焼肉Tシャツ ¥4,900",
            "彼氏/夫/お父さんへ", "MU × 焼肉古今",
            "kokon.tokyo Official", "MU公式 wearmu.com",
        ],
        "descriptions": [
            "西麻布の名店 焼肉古今 公式コラボ。肉好き、焼肉好きのギフトに。",
            "「焼肉好き」「肉の日 29」「焼肉古今ロゴ」3 SKU、各 ¥4,900。",
            "1点ずつ DTG プリント、受注生産。お父さん/彼氏/夫の誕生日に。",
        ],
        "final_url": f"{LANDING_BASE}/ads_kokon/207",
    },
    {
        "name": "profession",
        "keywords": [
            "\"看護師 Tシャツ\"", "\"看護師 Tシャツ ICU\"",
            "\"看護師 Tシャツ プレゼント\"", "\"ナース Tシャツ\"",
            "\"エンジニア Tシャツ\"", "\"プログラマー Tシャツ\"",
            "\"プログラマー Tシャツ ギフト\"",
        ],
        "headlines": [
            "ICU Night Shift Tee", "看護師さんへのギフト",
            "エンジニア Tee 内輪ネタ", "Caffeinate→Execute→Build",
            "職業プライド ¥4,900", "Made in Japan DTG",
            "受注生産・在庫ゼロ", "MU公式 wearmu.com",
        ],
        "descriptions": [
            "ICU夜勤・エンジニア。職業の内輪ネタを着るMUの部族シリーズ。",
            "1着 ¥4,900、DTGプリント。誕生日/退職祝い/同僚へのプレゼントに。",
            "MU 公式 wearmu.com、利益50%は弟子屈町に寄付。透明原価。",
        ],
        "final_url": f"{LANDING_BASE}/ads_profession/210",
    },
    {
        "name": "event",
        "keywords": [
            "\"SOLUNA FEST Tシャツ\"", "\"SOLUNA FEST 2026\"",
            "\"父の日 釣り Tシャツ\"", "\"父の日 プレゼント Tシャツ\"",
            "\"グラップリング 大会 Tシャツ\"", "\"グラップリング 記念\"",
            "\"釣り好き Tシャツ ギフト\"",
        ],
        "headlines": [
            "SOLUNA FEST 公式Tee", "父の日 釣り好きDad",
            "Worlds OK-est Fishing Dad", "East Japan Grappling 2026",
            "イベント記念 ¥5,500", "ギフト/お土産に最適",
            "DTG 1枚ずつ印刷", "MU公式 wearmu.com",
        ],
        "descriptions": [
            "SOLUNA FEST HAWAII 2026 公式Tee。フェス参加/記念ギフトに。",
            "父の日に — World's OK-est Fishing Dad badge tee ¥4,900。",
            "東日本グラップリングオープン 2026 記念Tee ¥5,500。",
        ],
        "final_url": f"{LANDING_BASE}/ads_event/212",
    },
]

NEGATIVE_KEYWORDS = [
    "無料", "free", "ダウンロード", "素材", "フリー素材",
    "中古", "古着", "セカンドハンド",
    "uniqlo", "gu", "muji", "無印", "shein", "temu",
    "子供", "キッズ", "ベビー", "幼児",
    "印刷", "オリジナル Tシャツ 作成", "オリジナルプリント",
]


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
    print("Status on create: PAUSED (you must enable in Google Ads UI)\n")
    total_kw = 0
    for ag in AD_GROUPS:
        kw_count = len(ag["keywords"])
        total_kw += kw_count
        print(f"  ┌─ ad_group: {ag['name']:<12} ({kw_count} keywords)")
        print(f"  │  landing: {ag['final_url']}")
        print(f"  │  keywords: {', '.join(ag['keywords'][:3])}{', ...' if kw_count > 3 else ''}")
        print(f"  │  headlines: {len(ag['headlines'])}, descriptions: {len(ag['descriptions'])}")
        print(f"  └─")
    print(f"\n  Total: {len(AD_GROUPS)} ad groups, {total_kw} keywords")
    print(f"  Negative keywords: {len(NEGATIVE_KEYWORDS)}")
    print()


def run_create():
    """Actually create the campaign via Google Ads SDK."""
    try:
        from google.ads.googleads.client import GoogleAdsClient
        from google.ads.googleads.errors import GoogleAdsException
    except ImportError:
        sys.exit("Install: pip install --upgrade google-ads")

    cust_id = os.environ.get(CUSTOMER_ID_ENV)
    if not cust_id:
        sys.exit(f"{CUSTOMER_ID_ENV} not set. Run scripts/google_ads_bootstrap.py first.")

    cfg = os.environ.get("GOOGLE_ADS_CONFIGURATION_FILE_PATH") or str(
        Path.home() / ".config" / "google-ads" / "google-ads.yaml"
    )
    if not Path(cfg).exists():
        sys.exit(f"google-ads.yaml not found at {cfg}.\nSee scripts/google_ads_README.md.")

    print(f"→ Loading client (cfg={cfg}, customer={cust_id})")
    print("⚠️  Campaign creation NOT YET IMPLEMENTED in this script.")
    print()
    print("Reuse helpers from scripts/google_ads_setup.py:")
    print("  - upsert_budget(client, cust_id, name, micros)")
    print("  - find_campaign_by_name(client, cust_id, name)")
    print("  - mutate operations for: Campaign / AdGroup / AdGroupCriterion / AdGroupAd")
    print()
    print("Plan is documented above (--dry-run). Either:")
    print("  (a) Extend google_ads_setup.py to import AD_GROUPS from this file")
    print("  (b) Or paste the keywords/headlines/descriptions into Google Ads UI manually")
    print("      (15 min for 5 ad groups × 7 KWs × 8 headlines × 4 desc each)")


def main():
    ap = argparse.ArgumentParser()
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--dry-run", action="store_true", help="show plan only")
    g.add_argument("--create-all", action="store_true", help="create campaign + ad groups (PAUSED)")
    g.add_argument("--pause-all", action="store_true", help="pause everything")
    g.add_argument("--status", action="store_true", help="show current stats")
    args = ap.parse_args()
    load_env()

    if args.dry_run:
        print_plan()
        return
    if args.create_all:
        print_plan()
        run_create()
        return
    if args.pause_all or args.status:
        sys.exit("Not yet implemented — use Google Ads UI for now.")


if __name__ == "__main__":
    main()
