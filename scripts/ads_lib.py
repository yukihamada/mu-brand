"""Google Ads ops helpers — shared boilerplate for ads optimization scripts.

Usage:
    from ads_lib import client_for, ACCTS, top_wasters, top_winners, add_negative, set_budget, set_bid_ceiling, tg, log_iter

All functions raise on error — fail loud.
"""
import os, json, urllib.request, urllib.parse
from pathlib import Path
from datetime import datetime

# Auto-load .env on import
_ENV = Path("/Users/yuki/.env")
if _ENV.exists():
    for ln in _ENV.read_text().splitlines():
        if "=" in ln and not ln.startswith("#"):
            k, v = ln.split("=", 1)
            os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))

import yaml as _y
from google.ads.googleads.client import GoogleAdsClient
from google.protobuf import field_mask_pb2

ACCTS = [
    ("4070111170", "JiuFlow"),
    ("5408218744", "BANTO"),
    ("8516735301", "misebanai"),
    ("9591303572", "MU"),
]
ACCT_BY_LABEL = {label: cid for cid, label in ACCTS}

_GADS_YAML = str(Path.home() / ".config/google-ads/google-ads.yaml")
_BASE_CFG = _y.safe_load(open(_GADS_YAML))

TG_TOKEN = os.environ.get("TELEGRAM_BOT_TOKEN", "")
TG_CHAT = "1136442501"
LOG_DIR = Path("/Users/yuki/workspace/mu-brand/logs/loop_30m")


def client_for(cid_or_label: str) -> GoogleAdsClient:
    """Get a GoogleAdsClient logged into the given customer (by id or label)."""
    cid = ACCT_BY_LABEL.get(cid_or_label, cid_or_label)
    cfg = dict(_BASE_CFG)
    cfg["login_customer_id"] = cid
    return GoogleAdsClient.load_from_dict(cfg, version="v22")


def search_all(c: GoogleAdsClient, cid: str, query: str):
    """Iterator over search results — wraps the common service call."""
    return c.get_service("GoogleAdsService").search(customer_id=cid, query=query)


def top_wasters(c: GoogleAdsClient, cid: str, campaign_name: str, days: int = 14,
                min_clicks: int = 3, min_cost_yen: int = 500):
    """Search terms with cost ≥ min_cost_yen and 0 conversions."""
    micros = min_cost_yen * 1_000_000
    q = f"""
    SELECT search_term_view.search_term, metrics.clicks, metrics.cost_micros, metrics.conversions
    FROM search_term_view
    WHERE campaign.name='{campaign_name}' AND segments.date DURING LAST_{days}_DAYS
      AND metrics.clicks > {min_clicks} AND metrics.cost_micros > {micros}
      AND metrics.conversions = 0
    """
    out = []
    for r in search_all(c, cid, q):
        out.append({
            "term": r.search_term_view.search_term,
            "clicks": r.metrics.clicks,
            "cost": r.metrics.cost_micros / 1e6,
        })
    out.sort(key=lambda x: -x["cost"])
    return out


def top_winners(c: GoogleAdsClient, cid: str, campaign_name: str, days: int = 14,
                min_conv: int = 1, min_roas: float = 1.5):
    """Search terms with conversions and ROAS ≥ min_roas."""
    q = f"""
    SELECT search_term_view.search_term, ad_group.name, ad_group.id,
           metrics.clicks, metrics.cost_micros, metrics.conversions, metrics.conversions_value
    FROM search_term_view
    WHERE campaign.name='{campaign_name}' AND segments.date DURING LAST_{days}_DAYS
      AND metrics.conversions > 0
    """
    out = []
    for r in search_all(c, cid, q):
        cost = r.metrics.cost_micros / 1e6
        conv = r.metrics.conversions
        val = r.metrics.conversions_value
        roas = (val / cost) if cost > 0 else 99
        if conv >= min_conv and roas >= min_roas:
            out.append({
                "term": r.search_term_view.search_term,
                "ag": r.ad_group.name,
                "ag_id": r.ad_group.id,
                "clicks": r.metrics.clicks,
                "cost": cost,
                "conv": conv,
                "val": val,
                "roas": roas,
            })
    out.sort(key=lambda x: -x["val"])
    return out


def existing_negatives(c: GoogleAdsClient, cid: str, campaign_id: int) -> set:
    """Set of lower-cased negative keyword texts already on the campaign."""
    out = set()
    q = f"""
    SELECT campaign_criterion.keyword.text FROM campaign_criterion
    WHERE campaign.id={campaign_id} AND campaign_criterion.type=KEYWORD AND campaign_criterion.negative=TRUE
    """
    for r in search_all(c, cid, q):
        out.add(r.campaign_criterion.keyword.text.lower().strip())
    return out


def add_negative(c: GoogleAdsClient, cid: str, campaign_id: int, keyword: str, match_type: str = "PHRASE") -> bool:
    """Add a campaign-level negative keyword. Returns True on success."""
    svc = c.get_service("CampaignCriterionService")
    op = c.get_type("CampaignCriterionOperation")
    cr = op.create
    cr.campaign = f"customers/{cid}/campaigns/{campaign_id}"
    cr.negative = True
    cr.keyword.text = keyword
    cr.keyword.match_type = getattr(c.enums.KeywordMatchTypeEnum, match_type)
    try:
        svc.mutate_campaign_criteria(customer_id=cid, operations=[op])
        return True
    except Exception as e:
        print(f"  ❌ neg '{keyword}': {str(e)[:120]}")
        return False


def add_negatives_bulk(c: GoogleAdsClient, cid: str, campaign_id: int, kws: list) -> dict:
    """Add many negatives, dedupe against existing.
    kws: list of (text, match_type) tuples.
    Returns {added: [...], skipped_dup: [...], failed: [...]}."""
    existing = existing_negatives(c, cid, campaign_id)
    res = {"added": [], "skipped_dup": [], "failed": []}
    for kw, mt in kws:
        if kw.lower() in existing:
            res["skipped_dup"].append(kw); continue
        if add_negative(c, cid, campaign_id, kw, mt):
            res["added"].append(f"{mt}:{kw}")
        else:
            res["failed"].append(kw)
    return res


def set_budget(c: GoogleAdsClient, cid: str, budget_resource_name: str, amount_yen: int):
    """Update campaign budget to amount_yen per day."""
    svc = c.get_service("CampaignBudgetService")
    op = c.get_type("CampaignBudgetOperation")
    op.update.resource_name = budget_resource_name
    op.update.amount_micros = amount_yen * 1_000_000
    op.update_mask.CopyFrom(field_mask_pb2.FieldMask(paths=["amount_micros"]))
    svc.mutate_campaign_budgets(customer_id=cid, operations=[op])


def set_bid_ceiling(c: GoogleAdsClient, cid: str, campaign_resource_name: str, amount_yen: int):
    """Update TARGET_SPEND cpc_bid_ceiling on a campaign."""
    svc = c.get_service("CampaignService")
    op = c.get_type("CampaignOperation")
    op.update.resource_name = campaign_resource_name
    op.update.target_spend.cpc_bid_ceiling_micros = amount_yen * 1_000_000
    op.update_mask.CopyFrom(field_mask_pb2.FieldMask(paths=["target_spend.cpc_bid_ceiling_micros"]))
    svc.mutate_campaigns(customer_id=cid, operations=[op])


def get_campaign_info(c: GoogleAdsClient, cid: str, campaign_name: str):
    """Return (campaign_id, campaign_resource_name, budget_resource_name, budget_yen) or None."""
    for r in search_all(c, cid, f"""
    SELECT campaign.id, campaign.resource_name, campaign_budget.resource_name, campaign_budget.amount_micros
    FROM campaign WHERE campaign.name='{campaign_name}' LIMIT 1
    """):
        return (r.campaign.id, r.campaign.resource_name,
                r.campaign_budget.resource_name, r.campaign_budget.amount_micros // 1_000_000)
    return None


def today_snapshot():
    """Return today snapshot: list of dicts per enabled campaign with cost+conv."""
    out = []
    for cid, label in ACCTS:
        c = client_for(cid)
        for r in search_all(c, cid, """
        SELECT campaign.name, metrics.impressions, metrics.clicks, metrics.cost_micros,
               metrics.conversions, metrics.conversions_value
        FROM campaign WHERE campaign.status='ENABLED' AND segments.date DURING TODAY"""):
            cost = r.metrics.cost_micros / 1e6
            conv = r.metrics.conversions
            if cost == 0 and conv == 0: continue
            out.append({
                "acct": label, "cid": cid, "campaign": r.campaign.name,
                "imp": r.metrics.impressions, "clk": r.metrics.clicks,
                "cost": cost, "conv": conv, "val": r.metrics.conversions_value,
                "roas": (r.metrics.conversions_value / cost) if cost else 0,
            })
    return sorted(out, key=lambda x: -x["cost"])


def tg(msg: str):
    """Send Telegram alert. Silent fail."""
    if not TG_TOKEN: return
    try:
        urllib.request.urlopen(
            f"https://api.telegram.org/bot{TG_TOKEN}/sendMessage",
            data=urllib.parse.urlencode({"chat_id": TG_CHAT, "text": msg}).encode(),
            timeout=15,
        )
    except Exception as e:
        print(f"[tg-err] {e}")


def log_iter(ts: str = None, trigger: str = "", actions: list = None,
             observations: list = None, **extra):
    """Write iteration log to logs/loop_30m/iter_<ts>.json."""
    ts = ts or datetime.now().strftime("%Y%m%d_%H%M%S")
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    payload = {"ts": ts, "trigger": trigger, "actions": actions or [], "observations": observations or [], **extra}
    p = LOG_DIR / f"iter_{ts}.json"
    p.write_text(json.dumps(payload, ensure_ascii=False, indent=2))
    return p
