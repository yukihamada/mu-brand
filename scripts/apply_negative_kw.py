#!/usr/bin/env python3
"""
MU Google Ads — apply 5 preventive negative keywords across all active
(= non-PAUSED, non-REMOVED) campaigns, AND clean up the stray "Campaign #1"
(PAUSED, contains unrelated "カメラ 防犯" BROAD KWs) by removing all of its
ad_group criteria — without touching the campaign status itself.

Scope (strictly bounded by user task spec):
  YES  add 5 campaign-level NEGATIVE_KEYWORD criteria per ENABLED campaign
  YES  remove every keyword AdGroupCriterion under Campaign #1's ad groups
  NO   max_cpc / bid changes
  NO   budget changes
  NO   ad copy / ad / ad group structure changes
  NO   campaign status changes (Campaign #1 stays PAUSED)

Re-running this script is idempotent:
  - negatives already present on a campaign are skipped
  - keywords already removed in Campaign #1 stay removed

Usage:
  python3 scripts/apply_negative_kw.py --dry-run    # show plan, mutate nothing
  python3 scripts/apply_negative_kw.py              # actually apply

Auth: reads ~/.config/google-ads/google-ads.yaml (developer_token,
client_id/secret, refresh_token, login_customer_id). The customer_id we
mutate is hard-coded to 9591303572 (MU / wearmu.com).
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path
from typing import Iterable

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

# Preventive negatives — added to every active campaign at campaign level.
# (text, match_type) where match_type ∈ {BROAD, PHRASE, EXACT}
NEGATIVES: list[tuple[str, str]] = [
    ("カメラ", "BROAD"),
    ("中古 Tシャツ", "PHRASE"),
    ("古着", "BROAD"),
    ("テンプレート", "BROAD"),
    ("素材", "BROAD"),
]

# Identify the stray campaign by name. Per draft md the stray campaign is
# literally named "Campaign #1" (Google Ads' default name for the first
# campaign created in the account). We match by exact name.
STRAY_CAMPAIGN_NAME = "Campaign #1"


# ─── auth / client ─────────────────────────────────────────────────────────

def get_client() -> GoogleAdsClient:
    if not Path(YAML_PATH).exists():
        sys.exit(f"google-ads.yaml not found at {YAML_PATH}")
    return GoogleAdsClient.load_from_storage(YAML_PATH, version=API_VERSION)


# ─── helpers ───────────────────────────────────────────────────────────────

def list_active_campaigns(client: GoogleAdsClient) -> list[tuple[str, str, str]]:
    """Returns [(resource_name, name, status)] for non-REMOVED campaigns.
    'Active' here = ENABLED. PAUSED is excluded from negative injection (task
    spec: "全 active campaign (PAUSED 除く 5 件)"). But we still return all
    non-removed so the caller can also locate the stray PAUSED Campaign #1."""
    ga = client.get_service("GoogleAdsService")
    q = (
        "SELECT campaign.resource_name, campaign.name, campaign.status "
        "FROM campaign "
        "WHERE campaign.status != 'REMOVED'"
    )
    out: list[tuple[str, str, str]] = []
    for row in ga.search(customer_id=CUSTOMER_ID, query=q):
        out.append((
            row.campaign.resource_name,
            row.campaign.name,
            row.campaign.status.name,
        ))
    return out


def existing_negative_keywords(
    client: GoogleAdsClient, campaign_rn: str
) -> set[tuple[str, str]]:
    """Returns {(lowercased_text, match_type_name)} for negative keyword
    criteria already attached to the campaign."""
    ga = client.get_service("GoogleAdsService")
    q = (
        "SELECT campaign_criterion.keyword.text, "
        "campaign_criterion.keyword.match_type, "
        "campaign_criterion.negative, "
        "campaign_criterion.type "
        "FROM campaign_criterion "
        f"WHERE campaign_criterion.campaign = '{campaign_rn}' "
        "AND campaign_criterion.type = 'KEYWORD' "
        "AND campaign_criterion.negative = TRUE"
    )
    seen: set[tuple[str, str]] = set()
    for row in ga.search(customer_id=CUSTOMER_ID, query=q):
        text = row.campaign_criterion.keyword.text or ""
        mt = row.campaign_criterion.keyword.match_type.name
        seen.add((text.strip().lower(), mt))
    return seen


def add_one_negative(
    client: GoogleAdsClient, campaign_rn: str, text: str, match_type: str
) -> None:
    """Add a single negative keyword criterion at campaign level."""
    op = client.get_type("CampaignCriterionOperation")
    c = op.create
    c.campaign = campaign_rn
    c.negative = True
    c.keyword.text = text
    c.keyword.match_type = getattr(client.enums.KeywordMatchTypeEnum, match_type)
    client.get_service("CampaignCriterionService").mutate_campaign_criteria(
        customer_id=CUSTOMER_ID, operations=[op]
    )


def list_keywords_in_campaign(
    client: GoogleAdsClient, campaign_rn: str
) -> list[tuple[str, str, str, str]]:
    """Returns [(criterion_resource_name, keyword_text, match_type_name,
    ad_group_name)] for POSITIVE keyword criteria under this campaign.

    GAQL doesn't expose `ad_group_criterion.campaign` directly; we filter via
    `campaign.resource_name` which is implicitly joined from ad_group."""
    ga = client.get_service("GoogleAdsService")
    q = (
        "SELECT ad_group_criterion.resource_name, "
        "ad_group_criterion.keyword.text, "
        "ad_group_criterion.keyword.match_type, "
        "ad_group_criterion.negative, "
        "ad_group_criterion.type, "
        "ad_group.name "
        "FROM ad_group_criterion "
        f"WHERE campaign.resource_name = '{campaign_rn}' "
        "AND ad_group_criterion.type = 'KEYWORD' "
        "AND ad_group_criterion.negative = FALSE"
    )
    out: list[tuple[str, str, str, str]] = []
    for row in ga.search(customer_id=CUSTOMER_ID, query=q):
        out.append((
            row.ad_group_criterion.resource_name,
            row.ad_group_criterion.keyword.text or "",
            row.ad_group_criterion.keyword.match_type.name,
            row.ad_group.name,
        ))
    return out


def remove_ad_group_criteria(
    client: GoogleAdsClient, resource_names: list[str]
) -> None:
    """REMOVE a batch of AdGroupCriterion by resource_name."""
    ops = []
    for rn in resource_names:
        op = client.get_type("AdGroupCriterionOperation")
        op.remove = rn
        ops.append(op)
    if not ops:
        return
    client.get_service("AdGroupCriterionService").mutate_ad_group_criteria(
        customer_id=CUSTOMER_ID, operations=ops
    )


# ─── plan + apply ──────────────────────────────────────────────────────────

def plan_negative_additions(
    client: GoogleAdsClient,
) -> list[tuple[str, str, str, str, bool]]:
    """Returns plan rows: (campaign_rn, campaign_name, kw_text, match_type,
    needs_add). needs_add=False means already present → would be skipped."""
    rows = []
    for cmp_rn, cmp_name, status in list_active_campaigns(client):
        if status != "ENABLED":
            continue
        existing = existing_negative_keywords(client, cmp_rn)
        for text, mt in NEGATIVES:
            key = (text.strip().lower(), mt)
            needs = key not in existing
            rows.append((cmp_rn, cmp_name, text, mt, needs))
    return rows


def plan_stray_removals(
    client: GoogleAdsClient,
) -> tuple[str | None, str, list[tuple[str, str, str, str]]]:
    """Locate the stray Campaign #1 and list its positive keywords.
    Returns (resource_name_or_None, status_string, [criterion rows])."""
    for cmp_rn, cmp_name, status in list_active_campaigns(client):
        if cmp_name == STRAY_CAMPAIGN_NAME:
            kws = list_keywords_in_campaign(client, cmp_rn)
            return cmp_rn, status, kws
    return None, "NOT_FOUND", []


def print_plan(
    adds: list[tuple[str, str, str, str, bool]],
    stray_rn: str | None,
    stray_status: str,
    stray_kws: list[tuple[str, str, str, str]],
) -> None:
    print("=" * 72)
    print(f"PLAN — customer_id={CUSTOMER_ID}")
    print("=" * 72)

    print("\n[1/2] Campaign-level NEGATIVE_KEYWORD additions (ENABLED campaigns only)")
    by_cmp: dict[str, list[tuple[str, str, bool]]] = {}
    for _, cmp_name, text, mt, needs in adds:
        by_cmp.setdefault(cmp_name, []).append((text, mt, needs))
    if not by_cmp:
        print("  (no ENABLED campaigns found)")
    for cmp_name in sorted(by_cmp):
        print(f"  campaign={cmp_name}")
        for text, mt, needs in by_cmp[cmp_name]:
            mark = "ADD " if needs else "SKIP"
            print(f"    [{mark}] NEG \"{text}\" {mt}")
    add_total = sum(1 for r in adds if r[4])
    skip_total = sum(1 for r in adds if not r[4])
    print(f"  → would add {add_total}, would skip {skip_total} (already present)")

    print(f"\n[2/2] Stray campaign cleanup ({STRAY_CAMPAIGN_NAME})")
    if stray_rn is None:
        print("  (Campaign #1 not found — skipping)")
    else:
        print(f"  found: {stray_rn} (status={stray_status})")
        if not stray_kws:
            print("  no keyword criteria to remove (already clean)")
        for rn, text, mt, ag in stray_kws:
            print(f"    [REMOVE] ad_group={ag} kw=\"{text}\" {mt} ({rn})")
        print(f"  → would remove {len(stray_kws)} keyword criteria")
    print("=" * 72)


def apply_negatives(
    client: GoogleAdsClient,
    adds: list[tuple[str, str, str, str, bool]],
) -> tuple[int, int, int]:
    """Returns (added, skipped, errors)."""
    added = skipped = errors = 0
    for cmp_rn, cmp_name, text, mt, needs in adds:
        if not needs:
            print(f"[campaign={cmp_name}] +NEG \"{text}\" {mt} skip (already present)")
            skipped += 1
            continue
        try:
            add_one_negative(client, cmp_rn, text, mt)
            print(f"[campaign={cmp_name}] +NEG \"{text}\" {mt} ok")
            added += 1
        except GoogleAdsException as e:
            msg = str(e)
            if "DUPLICATE" in msg or "already exists" in msg.lower():
                print(f"[campaign={cmp_name}] +NEG \"{text}\" {mt} skip (race: duplicate)")
                skipped += 1
            else:
                print(f"[campaign={cmp_name}] +NEG \"{text}\" {mt} ERROR: {msg.splitlines()[0]}")
                errors += 1
    return added, skipped, errors


def apply_stray_removals(
    client: GoogleAdsClient,
    stray_rn: str | None,
    stray_status: str,
    stray_kws: list[tuple[str, str, str, str]],
) -> int:
    if stray_rn is None or not stray_kws:
        return 0
    # Safety: only touch keywords if the campaign is actually PAUSED. We
    # never want to yank live keywords out of an ENABLED campaign.
    if stray_status != "PAUSED":
        print(
            f"[stray] {STRAY_CAMPAIGN_NAME} is not PAUSED (status={stray_status}); "
            f"refusing to remove keywords. No action taken."
        )
        return 0
    rns = [r[0] for r in stray_kws]
    try:
        remove_ad_group_criteria(client, rns)
        for rn, text, mt, ag in stray_kws:
            print(f"[campaign={STRAY_CAMPAIGN_NAME}] -KW ad_group={ag} \"{text}\" {mt} ok")
        return len(stray_kws)
    except GoogleAdsException as e:
        print(f"[campaign={STRAY_CAMPAIGN_NAME}] REMOVE batch failed: {str(e).splitlines()[0]}")
        # Fall back to one-by-one so we know which (if any) succeeded.
        removed = 0
        for rn, text, mt, ag in stray_kws:
            try:
                remove_ad_group_criteria(client, [rn])
                print(f"[campaign={STRAY_CAMPAIGN_NAME}] -KW ad_group={ag} \"{text}\" {mt} ok")
                removed += 1
            except GoogleAdsException as e2:
                err = str(e2).splitlines()[0]
                print(f"[campaign={STRAY_CAMPAIGN_NAME}] -KW ad_group={ag} \"{text}\" {mt} ERROR: {err}")
        return removed


# ─── verification ──────────────────────────────────────────────────────────

def verify(client: GoogleAdsClient) -> dict:
    """Re-select negatives per enabled campaign and stray campaign positives.
    Returns a dict suitable for embedding in the result md."""
    result: dict = {"negatives_per_campaign": {}, "stray_keywords_left": []}
    for cmp_rn, cmp_name, status in list_active_campaigns(client):
        if status != "ENABLED":
            continue
        negs = existing_negative_keywords(client, cmp_rn)
        present = []
        for text, mt in NEGATIVES:
            key = (text.strip().lower(), mt)
            present.append((text, mt, key in negs))
        result["negatives_per_campaign"][cmp_name] = present

    # stray
    stray_rn = None
    for cmp_rn, cmp_name, status in list_active_campaigns(client):
        if cmp_name == STRAY_CAMPAIGN_NAME:
            stray_rn = cmp_rn
            break
    if stray_rn:
        kws = list_keywords_in_campaign(client, stray_rn)
        result["stray_keywords_left"] = [(t, mt, ag) for _, t, mt, ag in kws]
    return result


# ─── main ──────────────────────────────────────────────────────────────────

def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--dry-run", action="store_true",
                   help="print the plan only, do not mutate")
    args = p.parse_args()

    client = get_client()

    print(f"→ customer_id={CUSTOMER_ID}")
    print(f"→ collecting current state ...")
    adds = plan_negative_additions(client)
    stray_rn, stray_status, stray_kws = plan_stray_removals(client)
    print_plan(adds, stray_rn, stray_status, stray_kws)

    if args.dry_run:
        print("\n(dry-run mode — no mutations performed)")
        return

    print("\n--- applying negatives ---")
    added, skipped, errors = apply_negatives(client, adds)
    print(f"\nnegatives: added={added} skipped={skipped} errors={errors}")

    print("\n--- applying stray cleanup ---")
    removed = apply_stray_removals(client, stray_rn, stray_status, stray_kws)
    print(f"\nstray Campaign #1: removed_keywords={removed}")

    print("\n--- verification (re-select) ---")
    v = verify(client)
    for cmp_name, rows in v["negatives_per_campaign"].items():
        ok = sum(1 for _, _, present in rows if present)
        print(f"  {cmp_name}: {ok}/{len(rows)} negatives present")
        for text, mt, present in rows:
            mk = "OK " if present else "MISS"
            print(f"    [{mk}] \"{text}\" {mt}")
    print(f"  stray Campaign #1 positive keywords remaining: {len(v['stray_keywords_left'])}")
    for text, mt, ag in v["stray_keywords_left"]:
        print(f"    LEFT ad_group={ag} \"{text}\" {mt}")


if __name__ == "__main__":
    main()
