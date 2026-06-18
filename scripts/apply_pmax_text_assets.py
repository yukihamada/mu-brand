#!/usr/bin/env python3
"""
MU Google Ads — add text assets (HEADLINE / LONG_HEADLINE / DESCRIPTION) to the
MU-PMax campaign's asset group, sourced from
`ads/PMAX_ASSET_DRAFT_20260521.md` §3.

Reason: MU-PMax-group-1 (id=6713500500) currently has ad_strength=POOR with
text inventory HEADLINE 4/15, LONG_HEADLINE 2/5, DESCRIPTION 4/5. The draft md
proposes 18 HEADLINE + 5 LONG_HEADLINE + 7 DESCRIPTION candidates. This script
uploads them and links them to the asset group, capping per-type counts at
Google's hard limits (15 / 5 / 5).

Scope (strictly bounded by user task spec):
  YES  create new TEXT assets and link to asset_group=6713500500
  YES  enforce per-type caps (15 / 5 / 5) by trimming the extras
  YES  skip text already present in the asset group (idempotent)
  NO   touch the bidding strategy
  NO   delete / disable any existing asset
  NO   upload image / video / logo assets
  NO   touch any other campaign
  NO   add asset_group_signal (audience)
  NO   modify final_url / final_mobile_url

Usage:
  python3 scripts/apply_pmax_text_assets.py --dry-run
  python3 scripts/apply_pmax_text_assets.py

Auth: reads ~/.config/google-ads/google-ads.yaml. customer_id is hard-coded.
"""

from __future__ import annotations

import argparse
import sys
import unicodedata
from pathlib import Path

try:
    from google.ads.googleads.client import GoogleAdsClient
    from google.ads.googleads.errors import GoogleAdsException
except ImportError:
    sys.exit(
        "google-ads SDK missing. Install:\n"
        "  pip install --upgrade google-ads"
    )

CUSTOMER_ID = "9591303572"
YAML_PATH = str(Path.home() / ".config" / "google-ads" / "google-ads.yaml")
API_VERSION = "v22"

CAMPAIGN_ID = "23858152693"   # MU-PMax
ASSET_GROUP_ID = "6713500500"  # MU-PMax-group-1
ASSET_GROUP_RN = f"customers/{CUSTOMER_ID}/assetGroups/{ASSET_GROUP_ID}"

# Google Ads PMax per-type caps (hard limits).
CAPS = {
    "HEADLINE": 15,
    "LONG_HEADLINE": 5,
    "DESCRIPTION": 5,
}

# Per-field character ceilings (Google Ads spec).
MAX_LEN = {
    "HEADLINE": 30,
    "LONG_HEADLINE": 90,
    "DESCRIPTION": 90,
}

# ─── candidate text, lifted from ads/PMAX_ASSET_DRAFT_20260521.md §3 ────────

HEADLINES: list[str] = [
    "AI が毎時間 Tシャツを描く",
    "1 of 1、 二度と作られない",
    "世界に 1 着だけのデザイン",
    "あなたの名前で生成する Tシャツ",
    "利益の 50% を寄付する Tシャツ",
    "弟子屈の気象を着る",
    "北海道発、 AI 生成 アパレル",
    # NB: original draft had longer headlines here that Google rejected on
    # pixel-width (visible_len passes 30 but pixel measurement fails).
    # Replaced with shorter ones from the same draft trim list.
    "1 サイクル終了で永久終売",   # was: "1 時間で生まれて、 永遠に終わる" (rejected)
    "月相と気温から生成される",   # was: "国内発送 ¥4,900 / Printful 海外" (rejected)
    "¥6,800、 海外発送込み",
    "値引きしない、 透明原価設計",
    "在庫を持たない DTC ブランド",
    "コミュニティに 10% 還元",
    "100 着限定、 14 日チャレンジ",
    "AI が運営する Tシャツ ブランド",
    "Made in Japan、 国内印刷",
]

LONG_HEADLINES: list[str] = [
    "1 時間に 1 着、 1 サイクルで永久終了。 AI が描く一点物の Tシャツ ブランド MU。",
    "弟子屈町の気温と月相を seed に AI が毎時間生成。 利益の 50% は地域に寄付。",
    "あなたの名前を入れた世界に 1 着だけの Tシャツを、 ¥6,800 で海外配送。",
    "国内 ¥4,900 (SUZURI) / 海外 ¥7,800 (Printful EU)。 二重 fulfillment で世界へ。",
    "AI が運営する自律ブランド MU。 14 日で 100 着完売を目指す build-in-public チャレンジ実施中。",
]

DESCRIPTIONS: list[str] = [
    "¥4,900 から (国内発送) / ¥7,800 (海外発送込)。 7 日以内なら未着用に限り交換可能。",
    "AI が弟子屈町の気象から自動生成、 1 着 1 着が世界に 1 つ。 利益 50% を地域に寄付。",
    "Stanley/Stella SATU001 (GOTS organic 認証、 リブ襟)。 国内 2-3 日 / 海外 7-10 日。",
    "値引きなし、 原価ベース透明設計。 §28 利益分配 (寄付 50% / コミュニティ 10%)。",
    "1 of 1。 デザイン重複なし、 同じものは二度と生まれない。 1 サイクル終了で永久終売。",
    "在庫を持たない受注生産。 注文後 3-5 日で印刷、 EU 工場から直接発送。",
    "14 日 100 枚チャレンジ 実施中 (5/18-5/31)。 build-in-public、 進捗は wearmu.com/100 で公開。",
]

CANDIDATES = {
    "HEADLINE": HEADLINES,
    "LONG_HEADLINE": LONG_HEADLINES,
    "DESCRIPTION": DESCRIPTIONS,
}


# ─── helpers ───────────────────────────────────────────────────────────────

def visible_len(s: str) -> int:
    """Approximate the way Google Ads measures text length: count Unicode
    codepoints, treating combining marks as part of the base. This is closer
    to Google's "visible characters" rule than len(s.encode()) or len(s)
    raw. Spaces and punctuation count normally.

    Google's actual rule is pixel-based for HEADLINE, but the API rejects on
    codepoint count beyond the documented limit, so codepoint is the safer
    pre-flight gate."""
    # Drop combining marks (cosmetic – none in our strings, but defensive).
    normalized = unicodedata.normalize("NFC", s)
    return sum(1 for ch in normalized if unicodedata.category(ch) != "Mn")


def normalize(s: str) -> str:
    """For dedupe matching against existing assets — NFC + strip."""
    return unicodedata.normalize("NFC", s).strip()


def get_client() -> GoogleAdsClient:
    if not Path(YAML_PATH).exists():
        sys.exit(f"google-ads.yaml not found at {YAML_PATH}")
    return GoogleAdsClient.load_from_storage(YAML_PATH, version=API_VERSION)


def existing_text_assets_in_group(
    client: GoogleAdsClient,
) -> dict[str, set[str]]:
    """Return {field_type_name: {normalized_text, ...}} for non-REMOVED
    asset_group_asset rows that are TEXT assets on our asset group."""
    ga = client.get_service("GoogleAdsService")
    q = (
        "SELECT asset_group_asset.field_type, "
        "asset_group_asset.status, "
        "asset.type, "
        "asset.text_asset.text "
        "FROM asset_group_asset "
        f"WHERE asset_group_asset.asset_group = '{ASSET_GROUP_RN}' "
        "AND asset_group_asset.status != 'REMOVED'"
    )
    out: dict[str, set[str]] = {ft: set() for ft in CAPS}
    for row in ga.search(customer_id=CUSTOMER_ID, query=q):
        ft = row.asset_group_asset.field_type.name
        if ft not in CAPS:
            continue
        # Only TEXT-type assets carry text_asset.text. Image/video assets in
        # the same field_type would be 0 (won't happen for HEADLINE etc but
        # be defensive).
        text = row.asset.text_asset.text or ""
        if text:
            out[ft].add(normalize(text))
    return out


def asset_group_ad_strength(client: GoogleAdsClient) -> str:
    ga = client.get_service("GoogleAdsService")
    q = (
        "SELECT asset_group.id, asset_group.name, asset_group.ad_strength "
        f"FROM asset_group WHERE asset_group.id = {ASSET_GROUP_ID}"
    )
    for row in ga.search(customer_id=CUSTOMER_ID, query=q):
        return row.asset_group.ad_strength.name
    return "UNKNOWN"


# ─── planning ──────────────────────────────────────────────────────────────

def plan(
    client: GoogleAdsClient,
) -> tuple[
    dict[str, list[tuple[str, str]]],  # field_type → [(text, status)] where status ∈ {ADD, SKIP_DUP, SKIP_LEN, TRIM_CAP}
    dict[str, int],                    # existing counts per field_type
]:
    existing = existing_text_assets_in_group(client)
    existing_counts = {ft: len(existing[ft]) for ft in CAPS}

    plan_rows: dict[str, list[tuple[str, str]]] = {ft: [] for ft in CAPS}

    for ft, texts in CANDIDATES.items():
        cap = CAPS[ft]
        room = max(0, cap - existing_counts[ft])
        adds_used = 0
        for raw in texts:
            t = normalize(raw)
            if not t:
                plan_rows[ft].append((raw, "SKIP_EMPTY"))
                continue
            vlen = visible_len(t)
            if vlen == 0 or vlen > MAX_LEN[ft]:
                plan_rows[ft].append((raw, f"SKIP_LEN({vlen}>{MAX_LEN[ft]})"))
                continue
            if t in existing[ft]:
                plan_rows[ft].append((raw, "SKIP_DUP"))
                continue
            if adds_used >= room:
                plan_rows[ft].append((raw, f"TRIM_CAP(>={cap})"))
                continue
            plan_rows[ft].append((raw, "ADD"))
            adds_used += 1

    return plan_rows, existing_counts


def print_plan(
    plan_rows: dict[str, list[tuple[str, str]]],
    existing_counts: dict[str, int],
    strength: str,
) -> None:
    print("=" * 78)
    print(f"PLAN — customer={CUSTOMER_ID} campaign={CAMPAIGN_ID} "
          f"asset_group={ASSET_GROUP_ID}")
    print(f"current ad_strength={strength}")
    print("=" * 78)
    for ft in ("HEADLINE", "LONG_HEADLINE", "DESCRIPTION"):
        cap = CAPS[ft]
        existing = existing_counts[ft]
        rows = plan_rows[ft]
        will_add = sum(1 for _, s in rows if s == "ADD")
        print(f"\n[{ft}] existing={existing}/{cap}  candidates={len(rows)}  "
              f"will_add={will_add}  (max_len={MAX_LEN[ft]})")
        for text, status in rows:
            vlen = visible_len(normalize(text))
            mark = {
                "ADD": "ADD ",
                "SKIP_DUP": "SKIP",
                "SKIP_EMPTY": "SKIP",
                "TRIM_CAP": "TRIM",
            }.get(status, "SKIP" if status.startswith("SKIP") else "TRIM")
            print(f"  [{mark}] ({vlen:2d}c) \"{text}\"  — {status}")
    print("=" * 78)


# ─── apply ─────────────────────────────────────────────────────────────────

def create_text_asset(client: GoogleAdsClient, text: str) -> str:
    """Create a single Asset of type TEXT. Returns the new resource_name."""
    op = client.get_type("AssetOperation")
    op.create.text_asset.text = text
    # asset.name is optional — leave blank, Google auto-generates.
    resp = client.get_service("AssetService").mutate_assets(
        customer_id=CUSTOMER_ID, operations=[op]
    )
    return resp.results[0].resource_name


def link_to_asset_group(
    client: GoogleAdsClient, asset_rn: str, field_type: str
) -> str:
    op = client.get_type("AssetGroupAssetOperation")
    op.create.asset_group = ASSET_GROUP_RN
    op.create.asset = asset_rn
    op.create.field_type = getattr(
        client.enums.AssetFieldTypeEnum, field_type
    )
    resp = client.get_service("AssetGroupAssetService").mutate_asset_group_assets(
        customer_id=CUSTOMER_ID, operations=[op]
    )
    return resp.results[0].resource_name


def apply(
    client: GoogleAdsClient,
    plan_rows: dict[str, list[tuple[str, str]]],
) -> dict[str, dict[str, int]]:
    """Apply ADD rows. Returns per-type {added, skipped, errors}."""
    summary: dict[str, dict[str, int]] = {
        ft: {"added": 0, "skipped": 0, "errors": 0} for ft in CAPS
    }
    for ft, rows in plan_rows.items():
        for text, status in rows:
            if status != "ADD":
                summary[ft]["skipped"] += 1
                continue
            t = normalize(text)
            try:
                asset_rn = create_text_asset(client, t)
                link_rn = link_to_asset_group(client, asset_rn, ft)
                # asset_rn format: customers/<cid>/assets/<id>
                asset_id = asset_rn.rsplit("/", 1)[-1]
                print(f"[asset created] {ft} \"{t}\" → id={asset_id} "
                      f"(link={link_rn.rsplit('/', 1)[-1]})")
                summary[ft]["added"] += 1
            except GoogleAdsException as e:
                msg = str(e).splitlines()[0]
                # If link fails because a duplicate text asset already exists
                # under the same asset group, treat as skip rather than error.
                if "DUPLICATE" in msg.upper() or "already" in msg.lower():
                    print(f"[skip] {ft} \"{t}\" — race duplicate")
                    summary[ft]["skipped"] += 1
                else:
                    print(f"[error] {ft} \"{t}\" — {msg}")
                    summary[ft]["errors"] += 1
    return summary


# ─── verification ──────────────────────────────────────────────────────────

def verify(client: GoogleAdsClient) -> dict[str, int]:
    existing = existing_text_assets_in_group(client)
    return {ft: len(existing[ft]) for ft in CAPS}


# ─── main ──────────────────────────────────────────────────────────────────

def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--dry-run", action="store_true",
                   help="print the plan only, do not mutate")
    args = p.parse_args()

    client = get_client()

    print(f"→ customer_id={CUSTOMER_ID}")
    print(f"→ campaign={CAMPAIGN_ID}  asset_group={ASSET_GROUP_ID}")
    print(f"→ collecting current state ...")
    plan_rows, existing_counts = plan(client)
    strength_before = asset_group_ad_strength(client)
    print_plan(plan_rows, existing_counts, strength_before)

    if args.dry_run:
        print("\n(dry-run mode — no mutations performed)")
        return

    print("\n--- applying text assets ---")
    summary = apply(client, plan_rows)
    print("\n--- summary ---")
    for ft in ("HEADLINE", "LONG_HEADLINE", "DESCRIPTION"):
        s = summary[ft]
        print(f"  {ft}: added={s['added']}  skipped={s['skipped']}  "
              f"errors={s['errors']}")

    print("\n--- verification (re-select asset_group_asset) ---")
    after = verify(client)
    for ft in ("HEADLINE", "LONG_HEADLINE", "DESCRIPTION"):
        print(f"  {ft}: {existing_counts[ft]} → {after[ft]} / cap {CAPS[ft]}")

    strength_after = asset_group_ad_strength(client)
    print(f"\nad_strength: {strength_before} → {strength_after} "
          f"(may take several minutes to refresh)")


if __name__ == "__main__":
    main()
