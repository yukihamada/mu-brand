#!/usr/bin/env python3
"""Aggressive auto-tightener for zero-conv ad spend across all accounts.

Called from the 30-min cron loop. Rules tightened 2026-05-23:

  HARD PAUSE : 7d cost > ¥30,000 AND conv == 0
  DEEP CUT   : 7d cost > ¥5,000  AND conv == 0  → budget ¥300/d
  TIGHT      : 7d cost > ¥2,000  AND conv == 0  → budget ¥500/d
  SHRINK     : 7d cost > ¥500    AND conv == 0  → budget ¥300/d
  SCALE UP   : 7d ROAS > 1.5x AND conv >= 5     → budget +25% (cap ¥100K/d)
  OBSERVE    : conv > 0 AND ROAS < 1.0          → log only

Also auto-pauses individual KW under JiuFlow Search that:
  - 7d cost > ¥2K AND conv == 0 (per-keyword burn)
  - 7d cost > ¥10K AND ROAS < 0.4 (low-ROAS BROAD trap)
And raises bids +25% on KW with conv >= 2 AND ROAS >= 1.5.
"""
import os, sys, json
from pathlib import Path
from datetime import datetime

for ln in Path("/Users/yuki/.env").read_text().splitlines():
    if "=" in ln and not ln.startswith("#"):
        k, v = ln.split("=", 1)
        os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))

import yaml as _y
from google.ads.googleads.client import GoogleAdsClient
from google.ads.googleads.errors import GoogleAdsException

ACCTS = [("4070111170","JiuFlow"), ("5408218744","BANTO"), ("8516735301","UNNAMED"), ("9591303572","MU")]
LOG_DIR = Path("/Users/yuki/workspace/mu-brand/logs/loop_30m")
LOG_DIR.mkdir(parents=True, exist_ok=True)
TS = datetime.now().strftime("%Y%m%d_%H%M%S")

actions = []
TG_TOKEN = os.environ.get("TELEGRAM_BOT_TOKEN", "")
TG_CHAT = "1136442501"


def tg(msg: str):
    if not TG_TOKEN: return
    import urllib.request, urllib.parse
    try:
        urllib.request.urlopen(
            f"https://api.telegram.org/bot{TG_TOKEN}/sendMessage",
            data=urllib.parse.urlencode({"chat_id": TG_CHAT, "text": msg}).encode(),
            timeout=10,
        )
    except Exception:
        pass


def process_account(cid: str, label: str):
    base = _y.safe_load(open(str(Path.home()/".config/google-ads/google-ads.yaml")))
    base["login_customer_id"] = cid
    c = GoogleAdsClient.load_from_dict(base, version="v22")
    svc = c.get_service("GoogleAdsService")
    csvc = c.get_service("CampaignService")
    bsvc = c.get_service("CampaignBudgetService")

    rows = list(svc.search(customer_id=cid, query=(
        "SELECT campaign.id, campaign.name, campaign.status, campaign_budget.id, "
        "campaign_budget.amount_micros, metrics.impressions, metrics.clicks, "
        "metrics.cost_micros, metrics.conversions, metrics.conversions_value "
        "FROM campaign WHERE campaign.status='ENABLED' "
        "AND segments.date DURING LAST_7_DAYS ORDER BY metrics.cost_micros DESC"
    )))
    pause_ops = []; budget_ops = []
    for r in rows:
        cost = r.metrics.cost_micros / 1e6
        conv = r.metrics.conversions
        val = r.metrics.conversions_value
        cur = r.campaign_budget.amount_micros / 1e6
        roas = (val / cost) if cost > 0 else 0
        name = r.campaign.name

        # SCALE UP (cap ¥100K/d to prevent runaway; idempotent — only mutate if increase)
        if roas >= 1.5 and conv >= 5:
            BUDGET_CAP = 100000
            target = min(int(cur * 1.25), BUDGET_CAP)
            if target > cur:
                op = c.get_type("CampaignBudgetOperation")
                op.update.resource_name = bsvc.campaign_budget_path(cid, r.campaign_budget.id)
                op.update.amount_micros = int(target * 1_000_000)
                op.update_mask.paths.append("amount_micros")
                budget_ops.append((op, name, cur, target))
                actions.append({"acct":label,"cmp":name,"action":f"SCALE_UP ¥{cur:.0f}→¥{target}","roas":roas,"conv":conv,"cost7d":cost})
            continue
        if conv > 0:
            actions.append({"acct":label,"cmp":name,"action":"OBSERVE","roas":roas,"conv":conv,"cost7d":cost})
            continue

        # Zero-conv tighten
        target = None
        if cost > 30000:
            op = c.get_type("CampaignOperation")
            op.update.resource_name = csvc.campaign_path(cid, r.campaign.id)
            op.update.status = c.enums.CampaignStatusEnum.PAUSED
            op.update_mask.paths.append("status")
            pause_ops.append((op, name, cost))
            actions.append({"acct":label,"cmp":name,"action":"HARD_PAUSE","cost7d":cost})
            continue
        if cost > 5000: target = 300
        elif cost > 2000: target = 500
        elif cost > 500: target = 300
        else: continue
        if cur > target:
            op = c.get_type("CampaignBudgetOperation")
            op.update.resource_name = bsvc.campaign_budget_path(cid, r.campaign_budget.id)
            op.update.amount_micros = int(target * 1_000_000)
            op.update_mask.paths.append("amount_micros")
            budget_ops.append((op, name, cur, target))
            actions.append({"acct":label,"cmp":name,"action":f"BUDGET ¥{cur:.0f}→¥{target}","cost7d":cost})

    if pause_ops:
        try:
            csvc.mutate_campaigns(customer_id=cid, operations=[o[0] for o in pause_ops])
            for _, n, c_ in pause_ops:
                print(f"[PAUSE/{label}] {n[:40]:<40} 7d¥{c_:>7,.0f}")
        except GoogleAdsException as e:
            print(f"[PAUSE ERR/{label}] {str(e)[:200]}")
    if budget_ops:
        try:
            bsvc.mutate_campaign_budgets(customer_id=cid, operations=[o[0] for o in budget_ops])
            for _, n, cur, tgt in budget_ops:
                print(f"[BUDGET/{label}] {n[:40]:<40} ¥{cur:.0f}→¥{tgt}/d")
        except GoogleAdsException as e:
            print(f"[BUDGET ERR/{label}] {str(e)[:200]}")


def ad_group_bid_tune(cid: str, label: str):
    """Auto-tune ad-group max-CPC based on 7d perf.
    - High CPA (>¥3000) AND cost > ¥3K → lower bid 25% (floor ¥80)
    - High ROAS (>1.5x) AND conv >= 3 → raise bid 15% (cap ¥500)
    """
    base = _y.safe_load(open(str(Path.home()/".config/google-ads/google-ads.yaml")))
    base["login_customer_id"] = cid
    c = GoogleAdsClient.load_from_dict(base, version="v22")
    svc = c.get_service("GoogleAdsService")
    agsvc = c.get_service("AdGroupService")
    rows = list(svc.search(customer_id=cid, query=(
        "SELECT ad_group.id, ad_group.name, ad_group.cpc_bid_micros, "
        "campaign.name, metrics.cost_micros, metrics.clicks, "
        "metrics.conversions, metrics.conversions_value "
        "FROM ad_group WHERE ad_group.status='ENABLED' "
        "AND campaign.status='ENABLED' AND segments.date DURING LAST_7_DAYS "
        "AND metrics.cost_micros > 3000000"
    )))
    BID_FLOOR = 80; BID_CAP_AG = 500
    ops = []
    for r in rows:
        cur = r.ad_group.cpc_bid_micros / 1e6
        cost = r.metrics.cost_micros / 1e6
        conv = r.metrics.conversions
        val = r.metrics.conversions_value
        cpa = (cost / conv) if conv > 0 else 0
        roas = (val / cost) if cost > 0 else 0
        target = None
        # High CPA → lower bid 25%
        if cpa > 3000 and cost > 3000:
            target = max(int(cur * 0.75), BID_FLOOR)
        # High ROAS → raise bid 15%
        elif roas >= 1.5 and conv >= 3:
            target = min(int(cur * 1.15), BID_CAP_AG)
        if target and target != int(cur):
            op = c.get_type("AdGroupOperation")
            op.update.resource_name = agsvc.ad_group_path(cid, r.ad_group.id)
            op.update.cpc_bid_micros = int(target * 1_000_000)
            op.update_mask.paths.append("cpc_bid_micros")
            ops.append(op)
            kind = "LOWER" if cpa > 3000 else "RAISE"
            actions.append({"acct": label, "ag": r.ad_group.name, "campaign": r.campaign.name,
                            "action": f"AG_{kind} ¥{cur:.0f}→¥{target}", "cpa": cpa, "roas": roas, "cost7d": cost})
    if ops:
        try:
            agsvc.mutate_ad_groups(customer_id=cid, operations=ops)
            print(f"[AD-GROUP BID/{label}] mutated {len(ops)} ad-groups")
        except GoogleAdsException as e:
            print(f"[AD-GROUP BID ERR/{label}] {str(e)[:200]}")


def anomaly_detect(cid: str, label: str):
    """Compare today vs 7d-avg per converting campaign — alert Telegram on
    50%+ CPA worsening or 50%+ ROAS drop with cost > ¥5K spent today."""
    base = _y.safe_load(open(str(Path.home()/".config/google-ads/google-ads.yaml")))
    base["login_customer_id"] = cid
    c = GoogleAdsClient.load_from_dict(base, version="v22")
    svc = c.get_service("GoogleAdsService")
    # 7d baseline per campaign
    base_cpa = {}; base_roas = {}
    for r in svc.search(customer_id=cid, query=(
        "SELECT campaign.name, metrics.cost_micros, metrics.conversions, "
        "metrics.conversions_value FROM campaign WHERE campaign.status='ENABLED' "
        "AND segments.date DURING LAST_7_DAYS AND metrics.conversions > 0"
    )):
        cost = r.metrics.cost_micros/1e6
        conv = r.metrics.conversions
        val = r.metrics.conversions_value
        base_cpa[r.campaign.name] = cost/conv if conv > 0 else 0
        base_roas[r.campaign.name] = val/cost if cost > 0 else 0
    # Today (cost > ¥5,000 = 5_000_000_000 micros)
    for r in svc.search(customer_id=cid, query=(
        "SELECT campaign.name, metrics.cost_micros, metrics.conversions, "
        "metrics.conversions_value FROM campaign WHERE campaign.status='ENABLED' "
        "AND segments.date DURING TODAY AND metrics.cost_micros > 5000000000"
    )):
        name = r.campaign.name
        cost = r.metrics.cost_micros/1e6
        conv = r.metrics.conversions
        val = r.metrics.conversions_value
        today_cpa = cost/conv if conv > 0 else float("inf")
        today_roas = val/cost if cost > 0 else 0
        if name in base_cpa and base_cpa[name] > 0 and today_cpa > base_cpa[name] * 2:
            tg_msg = f"⚠️ MU Ads: {label}/{name[:25]} CPA {base_cpa[name]:.0f}→{today_cpa:.0f} (worse 2x). today cost ¥{cost:,.0f}"
            actions.append({"acct":label,"cmp":name,"action":"ANOMALY_CPA","cpa_was":base_cpa[name],"cpa_now":today_cpa})
            tg(tg_msg)
        if name in base_roas and base_roas[name] > 0 and today_roas < base_roas[name] * 0.5:
            tg_msg = f"⚠️ MU Ads: {label}/{name[:25]} ROAS {base_roas[name]:.2f}x→{today_roas:.2f}x (drop 50%+). cost ¥{cost:,.0f} conv {conv}"
            actions.append({"acct":label,"cmp":name,"action":"ANOMALY_ROAS","roas_was":base_roas[name],"roas_now":today_roas})
            tg(tg_msg)


def disable_display_on_search(cid: str, label: str):
    """SEARCH campaigns shouldn't include Display Network (junk traffic, low ROAS).
    Auto-fix any SEARCH campaign with content_network=True.
    """
    base = _y.safe_load(open(str(Path.home()/".config/google-ads/google-ads.yaml")))
    base["login_customer_id"] = cid
    c = GoogleAdsClient.load_from_dict(base, version="v22")
    svc = c.get_service("GoogleAdsService")
    csvc = c.get_service("CampaignService")
    for r in svc.search(customer_id=cid, query=(
        "SELECT campaign.id, campaign.name, campaign.advertising_channel_type, "
        "campaign.network_settings.target_search_network, "
        "campaign.network_settings.target_content_network "
        "FROM campaign WHERE campaign.status='ENABLED' "
        "AND campaign.advertising_channel_type='SEARCH'"
    )):
        if r.campaign.network_settings.target_content_network:
            op = c.get_type("CampaignOperation")
            op.update.resource_name = csvc.campaign_path(cid, r.campaign.id)
            op.update.network_settings.target_google_search = True
            op.update.network_settings.target_search_network = r.campaign.network_settings.target_search_network
            op.update.network_settings.target_content_network = False
            op.update.network_settings.target_partner_search_network = False
            op.update_mask.paths.append("network_settings.target_content_network")
            try:
                csvc.mutate_campaigns(customer_id=cid, operations=[op])
                actions.append({"acct":label,"cmp":r.campaign.name,"action":"DISABLE_DISPLAY_ON_SEARCH"})
                print(f"[DISPLAY-OFF/{label}] {r.campaign.name}")
            except GoogleAdsException:
                pass


def auto_negative_keywords(cid: str, label: str):
    """Scan search-term reports across all ENABLED campaigns for
    high-cost queries that didn't convert and add them as campaign-level
    negatives so they stop bleeding budget.

    Rule: query with cost > ¥500 AND clk > 5 AND conv == 0 over LAST_7_DAYS.

    Dedup: queries existing negatives (any match type) per campaign before
    adding. Uses BROAD to match manual pattern + maximize block surface.
    """
    base = _y.safe_load(open(str(Path.home()/".config/google-ads/google-ads.yaml")))
    base["login_customer_id"] = cid
    c = GoogleAdsClient.load_from_dict(base, version="v22")
    svc = c.get_service("GoogleAdsService")
    ccs = c.get_service("CampaignCriterionService")
    mt = c.enums.KeywordMatchTypeEnum
    # Need 14d view to avoid blocking long-tail converters
    # (lesson learned 2026-05-23: 'karatê' had 0 conv 7d but 2 conv 14d → 35clk wasted block)
    rows = list(svc.search(customer_id=cid, query=(
        "SELECT search_term_view.search_term, campaign.id, campaign.name, "
        "metrics.cost_micros, metrics.clicks, metrics.conversions "
        "FROM search_term_view WHERE campaign.status='ENABLED' "
        "AND segments.date DURING LAST_14_DAYS "
        "AND metrics.cost_micros > 1000000 AND metrics.conversions = 0 "
        "AND metrics.clicks > 8"
    )))
    # Group by campaign
    by_camp = {}
    for r in rows:
        cmp_id = r.campaign.id
        term = r.search_term_view.search_term.strip().lower()
        if len(term) < 3 or len(term) > 80: continue
        by_camp.setdefault((cmp_id, r.campaign.name), set()).add(term)

    for (cmp_id, cmp_name), terms in by_camp.items():
        # Pull existing negative keyword texts (any match type)
        existing_negs = set()
        for nr in svc.search(customer_id=cid, query=(
            f"SELECT campaign_criterion.keyword.text FROM campaign_criterion "
            f"WHERE campaign.id={cmp_id} AND campaign_criterion.type='KEYWORD' "
            f"AND campaign_criterion.negative=TRUE"
        )):
            existing_negs.add(nr.campaign_criterion.keyword.text.strip().lower())

        ok = 0
        skipped = 0
        for term in list(terms)[:20]:
            if term in existing_negs:
                skipped += 1
                continue
            op = c.get_type("CampaignCriterionOperation")
            op.create.campaign = svc.campaign_path(cid, cmp_id)
            op.create.negative = True
            op.create.keyword.text = term
            op.create.keyword.match_type = mt.BROAD  # BROAD = blocks any query containing all words
            try:
                ccs.mutate_campaign_criteria(customer_id=cid, operations=[op])
                ok += 1
                actions.append({"acct": label, "campaign": cmp_name, "action": "AUTO_NEG", "term": term})
            except GoogleAdsException:
                pass  # rare race / API error
        if ok or skipped:
            print(f"[AUTO-NEG/{label}] {cmp_name}: +{ok} new ({skipped} dedup-skip)")


def device_modifier_tune(cid: str, label: str):
    """Auto-tune device bid modifiers per SEARCH campaign based on 14d perf.

    Thresholds:
      - DESKTOP/TABLET/MOBILE cost > ¥3K AND conv == 0 → modifier 0.5 (-50%)
      - ROAS >= 3.0x AND conv >= 2 → modifier 1.3 (+30%)
      - Skip if already within ±10% of target (avoid churn)
      - Never modify MOBILE if it's the workhorse (>50% of campaign conv)
    """
    base = _y.safe_load(open(str(Path.home()/".config/google-ads/google-ads.yaml")))
    base["login_customer_id"] = cid
    c = GoogleAdsClient.load_from_dict(base, version="v22")
    svc = c.get_service("GoogleAdsService")
    ccs = c.get_service("CampaignCriterionService")

    # 14d device perf per campaign
    rows = list(svc.search(customer_id=cid, query=(
        "SELECT campaign.id, campaign.name, segments.device, "
        "metrics.cost_micros, metrics.conversions, metrics.conversions_value "
        "FROM campaign WHERE campaign.status='ENABLED' "
        "AND campaign.advertising_channel_type='SEARCH' "
        "AND segments.date DURING LAST_14_DAYS"
    )))
    by_camp = {}
    for r in rows:
        by_camp.setdefault((r.campaign.id, r.campaign.name), {})[r.segments.device.name] = {
            "cost": r.metrics.cost_micros/1e6,
            "conv": r.metrics.conversions,
            "val": r.metrics.conversions_value,
        }

    for (camp_id, camp_name), devices in by_camp.items():
        total_conv = sum(d["conv"] for d in devices.values())
        if total_conv < 2: continue  # need at least some signal

        # Existing device criteria
        existing = {}
        for r in svc.search(customer_id=cid, query=(
            "SELECT campaign_criterion.resource_name, campaign_criterion.device.type, "
            "campaign_criterion.bid_modifier "
            f"FROM campaign_criterion WHERE campaign.id={camp_id} AND campaign_criterion.type=DEVICE"
        )):
            existing[r.campaign_criterion.device.type_.name] = (
                r.campaign_criterion.resource_name,
                r.campaign_criterion.bid_modifier
            )

        for dev_name, perf in devices.items():
            if dev_name == "CONNECTED_TV": continue  # negligible
            mobile_share = devices.get("MOBILE", {}).get("conv", 0) / max(total_conv, 1)
            cost = perf["cost"]; conv = perf["conv"]; val = perf["val"]
            roas = (val/cost) if cost > 0 else 0
            target = None
            reason = ""
            if cost > 3000 and conv == 0:
                target = 0.5  # -50%
                reason = f"14d ¥{cost:.0f}/0conv"
            elif roas >= 3.0 and conv >= 2:
                # Skip MOBILE if it's the workhorse (don't over-amplify)
                if dev_name == "MOBILE" and mobile_share > 0.7: continue
                target = 1.3  # +30%
                reason = f"14d ROAS {roas:.1f}x"
            if target is None: continue

            # Get current modifier (treat 0.0 as 1.0 / unset)
            cur_mod = existing.get(dev_name, (None, 0.0))[1]
            effective_cur = cur_mod if cur_mod > 0 else 1.0
            # Skip if already within 10% of target
            if abs(effective_cur - target) / target < 0.1: continue
            # Don't reverse human fine-tuning (more aggressive existing wins)
            if target < 1.0 and effective_cur < target: continue
            if target > 1.0 and effective_cur > target: continue

            if dev_name in existing:
                rn = existing[dev_name][0]
                op = c.get_type("CampaignCriterionOperation")
                op.update.resource_name = rn
                op.update.bid_modifier = target
                op.update_mask.paths.append("bid_modifier")
            else:
                op = c.get_type("CampaignCriterionOperation")
                cr = op.create
                cr.campaign = svc.campaign_path(cid, camp_id)
                cr.device.type_ = getattr(c.enums.DeviceEnum, dev_name)
                cr.bid_modifier = target
            try:
                ccs.mutate_campaign_criteria(customer_id=cid, operations=[op])
                actions.append({"acct": label, "campaign": camp_name, "device": dev_name,
                                "action": f"DEVICE_MOD {effective_cur:.2f}x→{target:.2f}x",
                                "reason": reason})
                print(f"[DEVICE/{label}] {camp_name[:25]} {dev_name}: {effective_cur:.2f}x→{target:.2f}x ({reason})")
            except GoogleAdsException as e:
                print(f"[DEVICE ERR/{label}] {dev_name}: {str(e)[:120]}")


def state_region_tune(cid: str, label: str):
    """Auto-tune SUB-COUNTRY (state/province/region) bid modifiers based on 14d perf.

    Higher thresholds than country-level (state data is sparser → less statistical power).

    Thresholds:
      - State cost > ¥5K AND 0 conv → modifier 0.5x (-50%)
      - State ROAS >= 4.0x AND conv >= 3 → modifier 1.4x (+40%)
      - State ROAS >= 2.5x AND conv >= 4 → modifier 1.2x (+20%)

    Only operates on campaigns whose 14d cost > ¥30K (significant scale).
    Adds new state criteria as needed (states don't need pre-existing targeting).
    """
    base = _y.safe_load(open(str(Path.home()/".config/google-ads/google-ads.yaml")))
    base["login_customer_id"] = cid
    c = GoogleAdsClient.load_from_dict(base, version="v22")
    svc = c.get_service("GoogleAdsService")
    ccs = c.get_service("CampaignCriterionService")

    # Find significant campaigns first
    big = []
    for r in svc.search(customer_id=cid, query=(
        "SELECT campaign.id, campaign.name, campaign.status, campaign.advertising_channel_type, "
        "metrics.cost_micros FROM campaign WHERE campaign.status='ENABLED' "
        "AND campaign.advertising_channel_type='SEARCH' AND segments.date DURING LAST_14_DAYS "
        "AND metrics.cost_micros > 30000000000"  # ¥30K
    )):
        big.append((r.campaign.id, r.campaign.name))
    if not big: return

    for camp_id, camp_name in big:
        # Aggregate state perf
        perf = {}  # region_id_str -> {cost, conv, val, clk}
        for r in svc.search(customer_id=cid, query=(
            f"SELECT campaign.id, segments.geo_target_region, geographic_view.location_type, "
            f"metrics.cost_micros, metrics.conversions, metrics.conversions_value, metrics.clicks "
            f"FROM geographic_view WHERE campaign.id={camp_id} "
            f"AND segments.date DURING LAST_14_DAYS "
            f"AND geographic_view.location_type='LOCATION_OF_PRESENCE'"
        )):
            if not r.segments.geo_target_region: continue
            rid = r.segments.geo_target_region.split('/')[-1]
            p = perf.get(rid, {"cost": 0, "conv": 0, "val": 0, "clk": 0})
            p["cost"] += r.metrics.cost_micros / 1e6
            p["conv"] += r.metrics.conversions
            p["val"] += r.metrics.conversions_value
            p["clk"] += r.metrics.clicks
            perf[rid] = p

        # Existing state criteria for this campaign
        existing = {}
        for r in svc.search(customer_id=cid, query=(
            f"SELECT campaign_criterion.resource_name, campaign_criterion.location.geo_target_constant, "
            f"campaign_criterion.bid_modifier "
            f"FROM campaign_criterion WHERE campaign.id={camp_id} "
            f"AND campaign_criterion.type='LOCATION' AND campaign_criterion.negative=FALSE"
        )):
            gid = r.campaign_criterion.location.geo_target_constant.split('/')[-1]
            existing[gid] = (r.campaign_criterion.resource_name, r.campaign_criterion.bid_modifier)

        for rid, p in perf.items():
            cost = p["cost"]; conv = p["conv"]; val = p["val"]
            roas = (val / cost) if cost > 0 else 0
            target_mod = None; reason = ""
            if cost > 5000 and conv == 0:
                target_mod = 0.5
                reason = f"14d ¥{cost:.0f}/0c"
            elif roas >= 4.0 and conv >= 3:
                target_mod = 1.4
                reason = f"14d ROAS {roas:.1f}x ({conv:.0f}c)"
            elif roas >= 2.5 and conv >= 4:
                target_mod = 1.2
                reason = f"14d ROAS {roas:.1f}x ({conv:.0f}c)"
            if target_mod is None: continue

            cur = existing.get(rid, (None, 0.0))[1]
            effective_cur = cur if cur > 0 else 1.0
            if abs(effective_cur - target_mod) / target_mod < 0.1: continue
            # Don't reverse human fine-tuning:
            #  - For throttle (target < 1): skip if existing is already more strict (lower)
            #  - For boost (target > 1): skip if existing is already more aggressive (higher)
            if target_mod < 1.0 and effective_cur < target_mod: continue
            if target_mod > 1.0 and effective_cur > target_mod: continue

            if rid in existing:
                op = c.get_type("CampaignCriterionOperation")
                op.update.resource_name = existing[rid][0]
                op.update.bid_modifier = target_mod
                op.update_mask.paths.append("bid_modifier")
            else:
                op = c.get_type("CampaignCriterionOperation")
                cr = op.create
                cr.campaign = svc.campaign_path(cid, camp_id)
                cr.location.geo_target_constant = f"geoTargetConstants/{rid}"
                cr.bid_modifier = target_mod
            try:
                ccs.mutate_campaign_criteria(customer_id=cid, operations=[op])
                actions.append({
                    "acct": label, "campaign": camp_name, "region_id": rid,
                    "action": f"STATE_MOD {effective_cur:.2f}x→{target_mod:.2f}x",
                    "reason": reason,
                })
                print(f"[STATE/{label}] {camp_name[:25]} region#{rid}: {effective_cur:.2f}x→{target_mod:.2f}x ({reason})")
            except GoogleAdsException as e:
                print(f"[STATE ERR/{label}] {rid}: {str(e)[:120]}")


def geo_modifier_tune(cid: str, label: str):
    """Auto-tune geographic bid modifiers per SEARCH campaign based on 14d perf.

    Country-level only (sub-region tuning requires explicit setup).

    Thresholds:
      - Country cost > ¥3K AND conv == 0 → modifier 0.5x (-50%)
      - Country ROAS >= 3.0x AND conv >= 2 → modifier 1.3x (+30%)
      - Country ROAS >= 2.0x AND conv >= 3 → modifier 1.2x (+20%)
      - Skip if already within ±10% of target (idempotent)
      - Only operates on countries already in positive targeting
    """
    base = _y.safe_load(open(str(Path.home()/".config/google-ads/google-ads.yaml")))
    base["login_customer_id"] = cid
    c = GoogleAdsClient.load_from_dict(base, version="v22")
    svc = c.get_service("GoogleAdsService")
    ccs = c.get_service("CampaignCriterionService")

    # Get 14d perf per (campaign, country)
    perf = {}  # (camp_id, country_id) -> {cost, conv, val}
    camp_names = {}
    for r in svc.search(customer_id=cid, query=(
        "SELECT campaign.id, campaign.name, campaign.status, campaign.advertising_channel_type, "
        "geographic_view.country_criterion_id, geographic_view.location_type, "
        "metrics.cost_micros, metrics.conversions, metrics.conversions_value "
        "FROM geographic_view WHERE campaign.status='ENABLED' "
        "AND campaign.advertising_channel_type='SEARCH' "
        "AND segments.date DURING LAST_14_DAYS "
        "AND geographic_view.location_type='LOCATION_OF_PRESENCE'"
    )):
        key = (r.campaign.id, r.geographic_view.country_criterion_id)
        cur = perf.get(key, {"cost": 0, "conv": 0, "val": 0})
        cur["cost"] += r.metrics.cost_micros / 1e6
        cur["conv"] += r.metrics.conversions
        cur["val"] += r.metrics.conversions_value
        perf[key] = cur
        camp_names[r.campaign.id] = r.campaign.name

    # Existing positive location criteria per campaign
    targets = {}  # camp_id -> {country_id_str: (resource_name, bid_modifier)}
    for r in svc.search(customer_id=cid, query=(
        "SELECT campaign.id, campaign.status, campaign_criterion.resource_name, "
        "campaign_criterion.location.geo_target_constant, campaign_criterion.bid_modifier, "
        "campaign_criterion.negative, campaign_criterion.type "
        "FROM campaign_criterion WHERE campaign.status='ENABLED' "
        "AND campaign_criterion.type='LOCATION' AND campaign_criterion.negative=FALSE"
    )):
        gid = r.campaign_criterion.location.geo_target_constant.split('/')[-1]
        if gid.isdigit():
            targets.setdefault(r.campaign.id, {})[gid] = (
                r.campaign_criterion.resource_name,
                r.campaign_criterion.bid_modifier,
            )

    for (camp_id, country_id), p in perf.items():
        if camp_id not in targets: continue
        country_str = str(country_id)
        # Country might be a state — we only act on country-level entries in targets
        if country_str not in targets[camp_id]: continue
        # Single-country campaigns: skip auto-modify (budget tighten already handles them;
        # throttling a single-country campaign's only country is redundant)
        if len(targets[camp_id]) < 2: continue
        cost = p["cost"]; conv = p["conv"]; val = p["val"]
        roas = (val / cost) if cost > 0 else 0

        target_mod = None; reason = ""
        if cost > 3000 and conv == 0:
            target_mod = 0.5
            reason = f"14d ¥{cost:.0f}/0conv"
        elif roas >= 3.0 and conv >= 2:
            target_mod = 1.3
            reason = f"14d ROAS {roas:.1f}x ({conv:.0f}c)"
        elif roas >= 2.0 and conv >= 3:
            target_mod = 1.2
            reason = f"14d ROAS {roas:.1f}x ({conv:.0f}c)"
        if target_mod is None: continue

        rn, cur_mod = targets[camp_id][country_str]
        effective_cur = cur_mod if cur_mod > 0 else 1.0
        if abs(effective_cur - target_mod) / target_mod < 0.1: continue
        # Don't reverse human fine-tuning (more aggressive existing wins)
        if target_mod < 1.0 and effective_cur < target_mod: continue
        if target_mod > 1.0 and effective_cur > target_mod: continue

        op = c.get_type("CampaignCriterionOperation")
        op.update.resource_name = rn
        op.update.bid_modifier = target_mod
        op.update_mask.paths.append("bid_modifier")
        try:
            ccs.mutate_campaign_criteria(customer_id=cid, operations=[op])
            actions.append({
                "acct": label, "campaign": camp_names[camp_id],
                "country_id": country_id, "action": f"GEO_MOD {effective_cur:.2f}x→{target_mod:.2f}x",
                "reason": reason,
            })
            print(f"[GEO/{label}] {camp_names[camp_id][:25]} country#{country_id}: {effective_cur:.2f}x→{target_mod:.2f}x ({reason})")
        except GoogleAdsException as e:
            print(f"[GEO ERR/{label}] {country_id}: {str(e)[:120]}")


def jiuflow_kw_tune():
    """Per-keyword tune inside JiuFlow Search (only campaign that converts)."""
    base = _y.safe_load(open(str(Path.home()/".config/google-ads/google-ads.yaml")))
    base["login_customer_id"] = "4070111170"
    c = GoogleAdsClient.load_from_dict(base, version="v22")
    svc = c.get_service("GoogleAdsService")
    agc = c.get_service("AdGroupCriterionService")
    cid = "4070111170"

    rows = list(svc.search(customer_id=cid, query=(
        "SELECT ad_group_criterion.criterion_id, ad_group.id, ad_group.name, "
        "ad_group_criterion.keyword.text, ad_group_criterion.cpc_bid_micros, "
        "ad_group_criterion.effective_cpc_bid_micros, "
        "metrics.cost_micros, metrics.clicks, metrics.conversions, metrics.conversions_value "
        "FROM keyword_view WHERE campaign.name = 'JiuFlow Search JP/EN 2026-05-07' "
        "AND ad_group_criterion.status='ENABLED' AND segments.date DURING LAST_7_DAYS "
        "AND metrics.cost_micros > 100000"
    )))
    ops = []
    for r in rows:
        cost = r.metrics.cost_micros / 1e6
        conv = r.metrics.conversions
        val = r.metrics.conversions_value
        roas = (val / cost) if cost > 0 else 0
        kw = r.ad_group_criterion.keyword.text
        crit = str(r.ad_group_criterion.criterion_id)
        ag = str(r.ad_group.id)
        # Pause: cost > ¥1K AND conv = 0  (tightened 14:14 — was ¥2K)
        # OR clk > 25 AND conv = 0 (volume signal even at lower cost)
        if (cost > 1000 and conv == 0) or (r.metrics.clicks > 25 and conv == 0):
            op = c.get_type("AdGroupCriterionOperation")
            op.update.resource_name = agc.ad_group_criterion_path(cid, ag, crit)
            op.update.status = c.enums.AdGroupCriterionStatusEnum.PAUSED
            op.update_mask.paths.append("status")
            ops.append(op)
            actions.append({"acct":"JiuFlow","kw":kw,"action":"PAUSE_KW","cost":cost,"clk":r.metrics.clicks})
        # Pause: cost > ¥10K AND ROAS < 0.4
        elif cost > 10000 and roas < 0.4:
            op = c.get_type("AdGroupCriterionOperation")
            op.update.resource_name = agc.ad_group_criterion_path(cid, ag, crit)
            op.update.status = c.enums.AdGroupCriterionStatusEnum.PAUSED
            op.update_mask.paths.append("status")
            ops.append(op)
            actions.append({"acct":"JiuFlow","kw":kw,"action":"PAUSE_LOW_ROAS_KW","cost":cost,"roas":roas})
        # Raise +25%: conv>=2 AND ROAS>=1.5 (cap ¥600 to prevent runaway)
        elif conv >= 2 and roas >= 1.5:
            BID_CAP = 600
            eff = r.ad_group_criterion.effective_cpc_bid_micros / 1e6
            target = min(int(max(eff, 50) * 1.25), BID_CAP)
            if target > eff:
                op = c.get_type("AdGroupCriterionOperation")
                op.update.resource_name = agc.ad_group_criterion_path(cid, ag, crit)
                op.update.cpc_bid_micros = int(target * 1_000_000)
                op.update_mask.paths.append("cpc_bid_micros")
                ops.append(op)
                actions.append({"acct":"JiuFlow","kw":kw,"action":f"RAISE_KW ¥{eff:.0f}→¥{target}","roas":roas,"conv":conv})

    if ops:
        try:
            agc.mutate_ad_group_criteria(customer_id=cid, operations=ops)
            print(f"[JiuFlow KW] mutated {len(ops)} keywords")
        except GoogleAdsException as e:
            print(f"[JiuFlow KW ERR] {str(e)[:300]}")


def main():
    print(f"━━━━━━━━━━━━━━━━━━━━ TIGHTEN @ {TS} ━━━━━━━━━━━━━━━━━━━━")
    for cid, label in ACCTS:
        try:
            process_account(cid, label)
        except Exception as e:
            print(f"[ERR {label}] {type(e).__name__}: {str(e)[:160]}")
    for cid, label in ACCTS:
        try:
            ad_group_bid_tune(cid, label)
        except Exception as e:
            print(f"[ERR AG tune {label}] {str(e)[:160]}")
    for cid, label in ACCTS:
        try:
            disable_display_on_search(cid, label)
        except Exception as e:
            print(f"[ERR display-off {label}] {str(e)[:160]}")
    for cid, label in ACCTS:
        try:
            anomaly_detect(cid, label)
        except Exception as e:
            print(f"[ERR anomaly {label}] {str(e)[:160]}")
    for cid, label in ACCTS:
        try:
            auto_negative_keywords(cid, label)
        except Exception as e:
            print(f"[ERR AUTO-NEG {label}] {str(e)[:160]}")
    for cid, label in ACCTS:
        try:
            device_modifier_tune(cid, label)
        except Exception as e:
            print(f"[ERR DEVICE {label}] {str(e)[:160]}")
    for cid, label in ACCTS:
        try:
            geo_modifier_tune(cid, label)
        except Exception as e:
            print(f"[ERR GEO {label}] {str(e)[:160]}")
    for cid, label in ACCTS:
        try:
            state_region_tune(cid, label)
        except Exception as e:
            print(f"[ERR STATE {label}] {str(e)[:160]}")
    try:
        jiuflow_kw_tune()
    except Exception as e:
        print(f"[ERR KW tune] {str(e)[:160]}")

    log_path = LOG_DIR / f"iter_{TS}.json"
    with log_path.open("w") as f:
        json.dump({"ts": TS, "actions": actions}, f, ensure_ascii=False, indent=2)
    print(f"\n=== Total actions: {len(actions)}  Log: {log_path}")

    # Telegram summary if any actions
    if actions:
        n_pause = sum(1 for a in actions if "PAUSE" in a.get("action","") or a.get("action")=="HARD_PAUSE")
        n_budget = sum(1 for a in actions if a.get("action","").startswith("BUDGET"))
        n_raise = sum(1 for a in actions if "RAISE" in a.get("action","") or "SCALE_UP" in a.get("action",""))
        tg(f"🔧 MU/JF Ads loop @ {TS}\n  pause: {n_pause}\n  budget cut: {n_budget}\n  scale/raise: {n_raise}")


if __name__ == "__main__":
    main()
