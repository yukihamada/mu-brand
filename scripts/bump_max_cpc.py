#!/usr/bin/env python3
"""
MU Google Ads — bump ad_group + keyword cpc_bid_micros to ¥250 (= 250_000_000
micros) across all ENABLED campaigns that use a manual bidding strategy
(MANUAL_CPC or MANUAL_CPC_ENHANCED). This is the "open the tap" step that
follows the negative-keyword + stray-cleanup pass in apply_negative_kw.py.

Background:
  - ads/ROAS_TUNE_DRAFT_20260521.md diagnoses impressions=0 across MU
    campaigns. Hypothesis H1: current bids (¥80–¥120) are under the JP
    apparel auction floor (~¥150–¥300).
  - Daily budgets remain the blast-radius guard: ~¥6,000/day total spend cap
    across the 5 ENABLED campaigns. This script does NOT touch budgets.

Scope (strictly bounded by user task spec):
  YES  ad_group.cpc_bid_micros  → 250_000_000 on MANUAL_CPC campaigns
  YES  ad_group_criterion.cpc_bid_micros → 250_000_000 on KEYWORD criteria
       that currently have an explicit override > 0 and < 250_000_000
  NO   touch ad_group_criterion that inherits (cpc_bid_micros == 0); we
       don't want to convert an inherit into an explicit override
  NO   campaign budget changes
  NO   campaign status changes (ENABLED stays ENABLED, PAUSED stays PAUSED)
  NO   stray "Campaign #1" (PAUSED) is not touched
  NO   bidding strategy switch — TARGET_SPEND / MAXIMIZE_CONVERSIONS /
       TARGET_CPA campaigns are SKIPPED with an explicit warning. We never
       force-flip a smart-bid campaign to MANUAL_CPC.
  NO   ad copy / ad group structure / negative kw changes

Safety gate:
  - If total daily budget across ENABLED campaigns exceeds ¥10,000, abort
    immediately with exit code 1 (no prompts).
  - Idempotent: ad_groups / keywords already at ¥250 are skipped.

Usage:
  python3 scripts/bump_max_cpc.py --dry-run    # print plan, mutate nothing
  python3 scripts/bump_max_cpc.py              # actually apply

Auth: ~/.config/google-ads/google-ads.yaml. customer_id hard-coded to
9591303572 (MU / wearmu.com).
"""

from __future__ import annotations

import argparse
import sys
from dataclasses import dataclass
from pathlib import Path

try:
    from google.ads.googleads.client import GoogleAdsClient
    from google.ads.googleads.errors import GoogleAdsException
    from google.api_core import protobuf_helpers
except ImportError:
    sys.exit(
        "google-ads SDK missing. Install:\n"
        "  pip install --upgrade google-ads"
    )

CUSTOMER_ID = "9591303572"
YAML_PATH = str(Path.home() / ".config" / "google-ads" / "google-ads.yaml")
API_VERSION = "v22"

TARGET_BID_MICROS = 250_000_000  # ¥250
BUDGET_ABORT_DAILY_YEN = 10_000  # total ENABLED daily budget cap

# Manual-bidding strategies where ad_group.cpc_bid_micros is the live ceiling.
ELIGIBLE_BIDDING = {"MANUAL_CPC", "MANUAL_CPC_ENHANCED"}


# ─── data ──────────────────────────────────────────────────────────────────

@dataclass
class CampaignRow:
    resource_name: str
    name: str
    status: str  # ENABLED / PAUSED / ...
    bidding: str  # MANUAL_CPC / TARGET_SPEND / ...
    daily_budget_yen: int

    @property
    def eligible(self) -> bool:
        return self.status == "ENABLED" and self.bidding in ELIGIBLE_BIDDING


@dataclass
class AdGroupRow:
    resource_name: str
    name: str
    campaign_name: str
    current_bid_micros: int


@dataclass
class KeywordRow:
    resource_name: str
    text: str
    match_type: str
    ad_group_name: str
    campaign_name: str
    current_bid_micros: int  # explicit override (0 = inherits ad_group default)


# ─── auth / client ─────────────────────────────────────────────────────────

def get_client() -> GoogleAdsClient:
    if not Path(YAML_PATH).exists():
        sys.exit(f"google-ads.yaml not found at {YAML_PATH}")
    return GoogleAdsClient.load_from_storage(YAML_PATH, version=API_VERSION)


# ─── reads ─────────────────────────────────────────────────────────────────

def list_campaigns(client: GoogleAdsClient) -> list[CampaignRow]:
    ga = client.get_service("GoogleAdsService")
    q = (
        "SELECT campaign.resource_name, campaign.name, campaign.status, "
        "campaign.bidding_strategy_type, campaign_budget.amount_micros "
        "FROM campaign "
        "WHERE campaign.status != 'REMOVED'"
    )
    rows: list[CampaignRow] = []
    for r in ga.search(customer_id=CUSTOMER_ID, query=q):
        rows.append(CampaignRow(
            resource_name=r.campaign.resource_name,
            name=r.campaign.name,
            status=r.campaign.status.name,
            bidding=r.campaign.bidding_strategy_type.name,
            daily_budget_yen=int(r.campaign_budget.amount_micros / 1_000_000),
        ))
    return rows


def list_ad_groups(
    client: GoogleAdsClient, campaign_resource_names: list[str]
) -> list[AdGroupRow]:
    if not campaign_resource_names:
        return []
    ga = client.get_service("GoogleAdsService")
    rn_list = ",".join(f"'{rn}'" for rn in campaign_resource_names)
    q = (
        "SELECT ad_group.resource_name, ad_group.name, "
        "ad_group.cpc_bid_micros, ad_group.status, campaign.name "
        "FROM ad_group "
        f"WHERE campaign.resource_name IN ({rn_list}) "
        "AND ad_group.status != 'REMOVED'"
    )
    rows: list[AdGroupRow] = []
    for r in ga.search(customer_id=CUSTOMER_ID, query=q):
        rows.append(AdGroupRow(
            resource_name=r.ad_group.resource_name,
            name=r.ad_group.name,
            campaign_name=r.campaign.name,
            current_bid_micros=int(r.ad_group.cpc_bid_micros),
        ))
    return rows


def list_keywords(
    client: GoogleAdsClient, campaign_resource_names: list[str]
) -> list[KeywordRow]:
    if not campaign_resource_names:
        return []
    ga = client.get_service("GoogleAdsService")
    rn_list = ",".join(f"'{rn}'" for rn in campaign_resource_names)
    q = (
        "SELECT ad_group_criterion.resource_name, "
        "ad_group_criterion.keyword.text, "
        "ad_group_criterion.keyword.match_type, "
        "ad_group_criterion.cpc_bid_micros, "
        "ad_group_criterion.status, "
        "ad_group.name, campaign.name "
        "FROM ad_group_criterion "
        f"WHERE campaign.resource_name IN ({rn_list}) "
        "AND ad_group_criterion.type = 'KEYWORD' "
        "AND ad_group_criterion.negative = FALSE "
        "AND ad_group_criterion.status != 'REMOVED'"
    )
    rows: list[KeywordRow] = []
    for r in ga.search(customer_id=CUSTOMER_ID, query=q):
        rows.append(KeywordRow(
            resource_name=r.ad_group_criterion.resource_name,
            text=r.ad_group_criterion.keyword.text or "",
            match_type=r.ad_group_criterion.keyword.match_type.name,
            ad_group_name=r.ad_group.name,
            campaign_name=r.campaign.name,
            current_bid_micros=int(r.ad_group_criterion.cpc_bid_micros),
        ))
    return rows


# ─── writes ────────────────────────────────────────────────────────────────

def bump_ad_group_bid(
    client: GoogleAdsClient, resource_name: str, bid_micros: int
) -> None:
    op = client.get_type("AdGroupOperation")
    ag = op.update
    ag.resource_name = resource_name
    ag.cpc_bid_micros = bid_micros
    client.copy_from(op.update_mask, protobuf_helpers.field_mask(None, ag._pb))
    client.get_service("AdGroupService").mutate_ad_groups(
        customer_id=CUSTOMER_ID, operations=[op]
    )


def bump_keyword_bid(
    client: GoogleAdsClient, resource_name: str, bid_micros: int
) -> None:
    op = client.get_type("AdGroupCriterionOperation")
    crit = op.update
    crit.resource_name = resource_name
    crit.cpc_bid_micros = bid_micros
    client.copy_from(op.update_mask, protobuf_helpers.field_mask(None, crit._pb))
    client.get_service("AdGroupCriterionService").mutate_ad_group_criteria(
        customer_id=CUSTOMER_ID, operations=[op]
    )


# ─── plan ──────────────────────────────────────────────────────────────────

def plan(
    campaigns: list[CampaignRow],
    ad_groups: list[AdGroupRow],
    keywords: list[KeywordRow],
) -> tuple[list[AdGroupRow], list[AdGroupRow], list[KeywordRow], list[KeywordRow]]:
    """Returns (ag_to_bump, ag_skipped_idempotent, kw_to_bump, kw_skipped)."""
    ag_to_bump = [ag for ag in ad_groups if ag.current_bid_micros != TARGET_BID_MICROS]
    ag_skipped = [ag for ag in ad_groups if ag.current_bid_micros == TARGET_BID_MICROS]

    # Only bump keywords that have an EXPLICIT override below target. cpc_bid=0
    # means "inherit ad_group default" — leave those alone so future ad_group
    # tuning still cascades.
    kw_to_bump = [
        k for k in keywords
        if 0 < k.current_bid_micros < TARGET_BID_MICROS
    ]
    kw_skipped = [
        k for k in keywords
        if k.current_bid_micros == 0 or k.current_bid_micros >= TARGET_BID_MICROS
    ]
    return ag_to_bump, ag_skipped, kw_to_bump, kw_skipped


def print_plan(
    campaigns: list[CampaignRow],
    eligible_campaigns: list[CampaignRow],
    skipped_campaigns: list[CampaignRow],
    ag_to_bump: list[AdGroupRow],
    ag_skipped: list[AdGroupRow],
    kw_to_bump: list[KeywordRow],
    kw_skipped: list[KeywordRow],
    total_daily_budget: int,
) -> None:
    print("=" * 78)
    print(f"PLAN — customer_id={CUSTOMER_ID}   target_bid=¥{TARGET_BID_MICROS // 1_000_000}")
    print("=" * 78)

    print("\n[campaigns] ENABLED summary")
    print(f"  total daily budget across ENABLED campaigns: ¥{total_daily_budget:,}/day"
          f"   (abort threshold: ¥{BUDGET_ABORT_DAILY_YEN:,}/day)")
    for c in campaigns:
        if c.status == "ENABLED":
            mark = "EDIT" if c in eligible_campaigns else "SKIP"
            print(f"  [{mark}] {c.name:35s} bidding={c.bidding:25s} budget=¥{c.daily_budget_yen:,}/day")
        else:
            print(f"  [---] {c.name:35s} status={c.status:8s} (not touched)")

    if skipped_campaigns:
        print("\n[WARNING] non-manual bidding campaigns skipped (cpc_bid_micros has no effect):")
        for c in skipped_campaigns:
            print(f"    - {c.name} ({c.bidding}) — to raise ceiling, edit the bidding "
                  f"strategy's cpc_bid_ceiling_micros, not ad_group.cpc_bid_micros")

    print(f"\n[ad_groups] to bump ({len(ag_to_bump)}) / already at ¥{TARGET_BID_MICROS // 1_000_000} ({len(ag_skipped)})")
    for ag in ag_to_bump:
        cur = ag.current_bid_micros // 1_000_000
        print(f"  [BUMP] {ag.campaign_name:30s} / {ag.name:30s}  ¥{cur} → ¥{TARGET_BID_MICROS // 1_000_000}")
    for ag in ag_skipped:
        print(f"  [SKIP] {ag.campaign_name:30s} / {ag.name:30s}  already ¥{TARGET_BID_MICROS // 1_000_000}")

    print(f"\n[keywords] explicit overrides to bump ({len(kw_to_bump)}) / inherit-or-already-target ({len(kw_skipped)})")
    for kw in kw_to_bump:
        cur = kw.current_bid_micros // 1_000_000
        print(f"  [BUMP] {kw.campaign_name:25s} / {kw.ad_group_name:25s} \"{kw.text}\" ({kw.match_type})  ¥{cur} → ¥{TARGET_BID_MICROS // 1_000_000}")
    if kw_skipped:
        inherit_n = sum(1 for k in kw_skipped if k.current_bid_micros == 0)
        already_n = len(kw_skipped) - inherit_n
        print(f"  [SKIP] {inherit_n} inherit ad_group default, {already_n} already ≥ ¥{TARGET_BID_MICROS // 1_000_000}")
    print("=" * 78)


# ─── apply ─────────────────────────────────────────────────────────────────

def apply_ad_group_bumps(
    client: GoogleAdsClient, ag_to_bump: list[AdGroupRow]
) -> tuple[int, int]:
    ok = err = 0
    for ag in ag_to_bump:
        try:
            bump_ad_group_bid(client, ag.resource_name, TARGET_BID_MICROS)
            cur = ag.current_bid_micros // 1_000_000
            print(f"[ag] {ag.campaign_name} / {ag.name}: ¥{cur} → ¥{TARGET_BID_MICROS // 1_000_000} ok")
            ok += 1
        except GoogleAdsException as e:
            print(f"[ag] {ag.campaign_name} / {ag.name}: ERROR {str(e).splitlines()[0]}")
            err += 1
    return ok, err


def apply_keyword_bumps(
    client: GoogleAdsClient, kw_to_bump: list[KeywordRow]
) -> tuple[int, int]:
    ok = err = 0
    for kw in kw_to_bump:
        try:
            bump_keyword_bid(client, kw.resource_name, TARGET_BID_MICROS)
            cur = kw.current_bid_micros // 1_000_000
            print(f"[kw] {kw.campaign_name} / {kw.ad_group_name} \"{kw.text}\": ¥{cur} → ¥{TARGET_BID_MICROS // 1_000_000} ok")
            ok += 1
        except GoogleAdsException as e:
            print(f"[kw] {kw.campaign_name} / {kw.ad_group_name} \"{kw.text}\": ERROR {str(e).splitlines()[0]}")
            err += 1
    return ok, err


# ─── verification ──────────────────────────────────────────────────────────

def verify(
    client: GoogleAdsClient, eligible_resource_names: list[str]
) -> tuple[list[AdGroupRow], list[KeywordRow]]:
    ag = list_ad_groups(client, eligible_resource_names)
    kw = list_keywords(client, eligible_resource_names)
    return ag, kw


# ─── main ──────────────────────────────────────────────────────────────────

def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--dry-run", action="store_true",
                   help="print the plan only, do not mutate")
    args = p.parse_args()

    client = get_client()

    print(f"→ customer_id={CUSTOMER_ID}")
    print(f"→ target bid: ¥{TARGET_BID_MICROS // 1_000_000} ({TARGET_BID_MICROS} micros)")
    print(f"→ collecting current state ...")

    campaigns = list_campaigns(client)
    enabled = [c for c in campaigns if c.status == "ENABLED"]
    eligible = [c for c in enabled if c.eligible]
    skipped_strategy = [c for c in enabled if not c.eligible]

    # Safety gate: total ENABLED daily budget must be under threshold.
    total_daily_budget = sum(c.daily_budget_yen for c in enabled)
    if total_daily_budget > BUDGET_ABORT_DAILY_YEN:
        print(
            f"\n[ABORT] total ENABLED daily budget ¥{total_daily_budget:,} > "
            f"¥{BUDGET_ABORT_DAILY_YEN:,} cap. Bid bump refused for safety.",
            file=sys.stderr,
        )
        print(
            f"        Review budgets first, or raise BUDGET_ABORT_DAILY_YEN in "
            f"the script after explicit re-approval.",
            file=sys.stderr,
        )
        sys.exit(1)

    eligible_rn = [c.resource_name for c in eligible]
    ad_groups = list_ad_groups(client, eligible_rn)
    keywords = list_keywords(client, eligible_rn)
    ag_to_bump, ag_skipped, kw_to_bump, kw_skipped = plan(eligible, ad_groups, keywords)

    print_plan(
        campaigns, eligible, skipped_strategy,
        ag_to_bump, ag_skipped, kw_to_bump, kw_skipped,
        total_daily_budget,
    )

    if args.dry_run:
        print("\n(dry-run mode — no mutations performed)")
        return

    if not ag_to_bump and not kw_to_bump:
        print("\n(nothing to do — already idempotent)")
        return

    print("\n--- applying ad_group bumps ---")
    ag_ok, ag_err = apply_ad_group_bumps(client, ag_to_bump)
    print(f"\nad_groups: ok={ag_ok} err={ag_err}")

    print("\n--- applying keyword bumps ---")
    kw_ok, kw_err = apply_keyword_bumps(client, kw_to_bump)
    print(f"\nkeywords: ok={kw_ok} err={kw_err}")

    print("\n--- verification (re-select) ---")
    v_ag, v_kw = verify(client, eligible_rn)
    ag_at_target = sum(1 for a in v_ag if a.current_bid_micros == TARGET_BID_MICROS)
    kw_at_or_above = sum(
        1 for k in v_kw
        if k.current_bid_micros == 0 or k.current_bid_micros >= TARGET_BID_MICROS
    )
    print(f"  ad_groups at ¥{TARGET_BID_MICROS // 1_000_000}: {ag_at_target}/{len(v_ag)}")
    print(f"  keywords inherit-or-≥target: {kw_at_or_above}/{len(v_kw)}")
    for a in v_ag:
        mark = "OK " if a.current_bid_micros == TARGET_BID_MICROS else "??"
        print(f"    [{mark}] {a.campaign_name} / {a.name}: ¥{a.current_bid_micros // 1_000_000}")


if __name__ == "__main__":
    main()
