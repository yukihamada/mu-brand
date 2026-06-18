#!/usr/bin/env python3
"""
MU Google Ads — set `bidding_strategy.target_spend.cpc_bid_ceiling_micros`
to ¥250 (= 250_000_000 micros) on every ENABLED Standard TARGET_SPEND
campaign. This is the follow-up to bump_max_cpc.py (commit 23f9553), which
only worked on the lone MANUAL_CPC campaign (MU-CRAFT). The other 3
ENABLED campaigns run TARGET_SPEND (= Maximize Clicks), so ad_group
cpc_bid_micros is ignored — only the campaign-level CPC ceiling matters.

Background:
  - ads/MAX_CPC_BUMP_20260521_2015.md §5 identifies MU-AdsTees-Search,
    MU-Brand, MU-Discovery (TARGET_SPEND, ENABLED) as the campaigns
    where the ¥80-120 implicit ceiling was likely below the JP apparel
    auction floor → impr=0.
  - Setting cpc_bid_ceiling_micros = 250_000_000 gives Google's bidder
    permission to clear the floor while keeping the daily-budget cost cap
    intact (this script never touches budgets).

Scope (strictly bounded):
  YES  Standard TARGET_SPEND campaign-level target_spend.cpc_bid_ceiling_micros
       → 250_000_000 (¥250)
  NO   MAXIMIZE_CONVERSIONS / PMax — different agent, different work
  NO   MANUAL_CPC (MU-CRAFT) — done in 23f9553
  NO   Portfolio bidding_strategy.resource_name campaigns — those live on
       BiddingStrategy, not Campaign.bidding_strategy. Reported as SKIP.
  NO   bidding strategy switch — TARGET_SPEND stays TARGET_SPEND
  NO   campaign budget changes
  NO   campaign status changes
  NO   stray "Campaign #1" (PAUSED) — not touched
  NO   ad copy / ad group structure / keywords / negative kw

Safety gate:
  - If total daily budget across ENABLED campaigns exceeds ¥10,000, abort
    immediately with exit code 1 (no prompts).
  - Idempotent: campaigns already at ceiling=¥250 are skipped.

Usage:
  python3 scripts/apply_bid_ceiling.py --dry-run    # print plan, no mutate
  python3 scripts/apply_bid_ceiling.py              # actually apply

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

TARGET_CEILING_MICROS = 250_000_000  # ¥250
BUDGET_ABORT_DAILY_YEN = 10_000  # total ENABLED daily budget cap

# Only Standard TARGET_SPEND is in scope. (Portfolio TARGET_SPEND would be a
# separate mutation against BiddingStrategy — flagged but not mutated here.)
ELIGIBLE_BIDDING = {"TARGET_SPEND"}


# ─── data ──────────────────────────────────────────────────────────────────

@dataclass
class CampaignRow:
    resource_name: str
    name: str
    status: str  # ENABLED / PAUSED / ...
    bidding: str  # TARGET_SPEND / MANUAL_CPC / MAXIMIZE_CONVERSIONS / ...
    daily_budget_yen: int
    portfolio_strategy_rn: str  # non-empty → portfolio (skip)
    current_ceiling_micros: int  # 0 when unset

    @property
    def eligible(self) -> bool:
        return (
            self.status == "ENABLED"
            and self.bidding in ELIGIBLE_BIDDING
            and not self.portfolio_strategy_rn  # only Standard TARGET_SPEND
        )


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
        "campaign.bidding_strategy_type, "
        "campaign.bidding_strategy, "
        "campaign.target_spend.cpc_bid_ceiling_micros, "
        "campaign_budget.amount_micros "
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
            portfolio_strategy_rn=r.campaign.bidding_strategy or "",
            current_ceiling_micros=int(r.campaign.target_spend.cpc_bid_ceiling_micros),
        ))
    return rows


# ─── writes ────────────────────────────────────────────────────────────────

def set_target_spend_ceiling(
    client: GoogleAdsClient, resource_name: str, ceiling_micros: int
) -> None:
    """Update only target_spend.cpc_bid_ceiling_micros. update_mask is
    constructed via protobuf_helpers so only that one field is sent — this
    avoids any accidental overwrite of status / budget / other fields."""
    op = client.get_type("CampaignOperation")
    c = op.update
    c.resource_name = resource_name
    c.target_spend.cpc_bid_ceiling_micros = ceiling_micros
    client.copy_from(op.update_mask, protobuf_helpers.field_mask(None, c._pb))
    client.get_service("CampaignService").mutate_campaigns(
        customer_id=CUSTOMER_ID, operations=[op]
    )


# ─── plan ──────────────────────────────────────────────────────────────────

def plan(
    campaigns: list[CampaignRow],
) -> tuple[list[CampaignRow], list[CampaignRow], list[CampaignRow]]:
    """Returns (to_apply, idempotent_skip, out_of_scope_skip)."""
    eligible = [c for c in campaigns if c.eligible]
    to_apply = [c for c in eligible if c.current_ceiling_micros != TARGET_CEILING_MICROS]
    idempotent = [c for c in eligible if c.current_ceiling_micros == TARGET_CEILING_MICROS]
    out_of_scope = [
        c for c in campaigns
        if c.status == "ENABLED" and not c.eligible
    ]
    return to_apply, idempotent, out_of_scope


def print_plan(
    all_campaigns: list[CampaignRow],
    to_apply: list[CampaignRow],
    idempotent: list[CampaignRow],
    out_of_scope: list[CampaignRow],
    total_daily_budget: int,
) -> None:
    print("=" * 78)
    print(f"PLAN — customer_id={CUSTOMER_ID}   target_ceiling=¥{TARGET_CEILING_MICROS // 1_000_000}")
    print("=" * 78)

    print("\n[campaigns] ENABLED summary")
    print(f"  total daily budget across ENABLED campaigns: ¥{total_daily_budget:,}/day"
          f"   (abort threshold: ¥{BUDGET_ABORT_DAILY_YEN:,}/day)")
    for c in all_campaigns:
        if c.status == "ENABLED":
            if c in to_apply:
                mark = "EDIT"
            elif c in idempotent:
                mark = "IDEM"
            else:
                mark = "SKIP"
            cur = c.current_ceiling_micros // 1_000_000
            cur_s = f"¥{cur}" if c.current_ceiling_micros else "unset"
            portfolio = " [portfolio]" if c.portfolio_strategy_rn else ""
            print(f"  [{mark}] {c.name:35s} bidding={c.bidding:22s}{portfolio} "
                  f"budget=¥{c.daily_budget_yen:,}/day ceiling={cur_s}")
        else:
            print(f"  [---] {c.name:35s} status={c.status:8s} (not touched)")

    if out_of_scope:
        print("\n[NOTE] ENABLED but out-of-scope (intentionally skipped):")
        for c in out_of_scope:
            reason = (
                "portfolio strategy (mutate BiddingStrategy, not campaign)"
                if c.portfolio_strategy_rn
                else f"bidding={c.bidding} (different agent / strategy)"
            )
            print(f"    - {c.name}: {reason}")

    print(f"\n[to apply] ({len(to_apply)}) / [idempotent skip] ({len(idempotent)})")
    for c in to_apply:
        cur = c.current_ceiling_micros // 1_000_000
        cur_s = f"¥{cur}" if c.current_ceiling_micros else "unset"
        print(f"  [APPLY] {c.name:35s}  ceiling: {cur_s} → ¥{TARGET_CEILING_MICROS // 1_000_000}")
    for c in idempotent:
        print(f"  [IDEM ] {c.name:35s}  ceiling already ¥{TARGET_CEILING_MICROS // 1_000_000}")
    print("=" * 78)


# ─── apply ─────────────────────────────────────────────────────────────────

def apply_ceilings(
    client: GoogleAdsClient, to_apply: list[CampaignRow]
) -> tuple[int, int]:
    ok = err = 0
    for c in to_apply:
        try:
            set_target_spend_ceiling(client, c.resource_name, TARGET_CEILING_MICROS)
            cur = c.current_ceiling_micros // 1_000_000
            cur_s = f"¥{cur}" if c.current_ceiling_micros else "unset"
            print(f"[ceiling] {c.name}: {cur_s} → ¥{TARGET_CEILING_MICROS // 1_000_000} ok")
            ok += 1
        except GoogleAdsException as e:
            print(f"[ceiling] {c.name}: ERROR {str(e).splitlines()[0]}")
            err += 1
    return ok, err


# ─── main ──────────────────────────────────────────────────────────────────

def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--dry-run", action="store_true",
                   help="print the plan only, do not mutate")
    args = p.parse_args()

    client = get_client()

    print(f"→ customer_id={CUSTOMER_ID}")
    print(f"→ target ceiling: ¥{TARGET_CEILING_MICROS // 1_000_000} ({TARGET_CEILING_MICROS} micros)")
    print(f"→ collecting current state ...")

    campaigns = list_campaigns(client)
    enabled = [c for c in campaigns if c.status == "ENABLED"]

    # Safety gate: total ENABLED daily budget must be under threshold.
    total_daily_budget = sum(c.daily_budget_yen for c in enabled)
    if total_daily_budget > BUDGET_ABORT_DAILY_YEN:
        print(
            f"\n[ABORT] total ENABLED daily budget ¥{total_daily_budget:,} > "
            f"¥{BUDGET_ABORT_DAILY_YEN:,} cap. Ceiling change refused for safety.",
            file=sys.stderr,
        )
        print(
            f"        Review budgets first, or raise BUDGET_ABORT_DAILY_YEN in "
            f"the script after explicit re-approval.",
            file=sys.stderr,
        )
        sys.exit(1)

    to_apply, idempotent, out_of_scope = plan(campaigns)
    print_plan(campaigns, to_apply, idempotent, out_of_scope, total_daily_budget)

    if args.dry_run:
        print("\n(dry-run mode — no mutations performed)")
        return

    if not to_apply:
        print("\n(nothing to do — already idempotent)")
        return

    print("\n--- applying target_spend ceiling updates ---")
    ok, err = apply_ceilings(client, to_apply)
    print(f"\ncampaigns: ok={ok} err={err}")

    print("\n--- verification (re-select) ---")
    after = list_campaigns(client)
    # Re-compute total budget to confirm we didn't drift.
    after_enabled = [c for c in after if c.status == "ENABLED"]
    after_total_budget = sum(c.daily_budget_yen for c in after_enabled)
    print(f"  total daily budget (ENABLED) after: ¥{after_total_budget:,}/day "
          f"(was ¥{total_daily_budget:,}/day) "
          f"{'OK' if after_total_budget == total_daily_budget else 'DRIFT'}")

    target_names = {c.name for c in to_apply}
    for c in after:
        if c.name not in target_names:
            continue
        cur = c.current_ceiling_micros // 1_000_000
        mark = "OK " if c.current_ceiling_micros == TARGET_CEILING_MICROS else "??"
        print(f"    [{mark}] {c.name}: ceiling=¥{cur}")


if __name__ == "__main__":
    main()
