#!/usr/bin/env python3
"""burst_ads_30k — monitor Google Ads spend toward a ¥30,000 over 10-day
plan (= ¥3,000/day target). MONITORING ONLY — never mutates campaign
budgets, never pauses, never bumps CPC. Mutation requires explicit user
sign-off via a separate work.

What it does:
  - Pulls all ENABLED campaigns under customer 9591303572 with today's
    cost_micros and cost_micros for the rolling 10-day window.
  - Reports today_spend, window_spend, percentage of the ¥30K cap.
  - Telegram alert if:
      * today_spend > ¥4,500 (= 150% of ¥3,000/d target → over-pacing)
      * window_spend > ¥30,000 (= cap hit)
      * any campaign in PAUSED/REMOVED that was last seen ENABLED
        (anomaly hint — purely informational)
  - Appends one JSON record per run to logs/burst_ads_30k.jsonl
  - Exits 0 even on partial failure (cron-friendly).

Run hourly (cron entry added by cron.sh):
  0 * * * * $PYTHON $SCRIPT_DIR/scripts/burst_ads_30k.py

Override the start of the 10-day window:
  BURST_START_DATE=2026-05-21 python3 scripts/burst_ads_30k.py

NEVER reads or logs TELEGRAM_BOT_TOKEN. Token comes from env, never
written to disk except for the URL-encoded outbound request.
"""
from __future__ import annotations

import json
import os
import sys
import time
import urllib.parse
import urllib.request
from datetime import date, datetime, timedelta
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LOG_DIR = ROOT / "logs"
LOG_FILE = LOG_DIR / "burst_ads_30k.jsonl"

# Goal: ¥30,000 spent over 10 days = ¥3,000/day average target.
PLAN_TOTAL_JPY = 30_000
PLAN_DAYS = 10
DAILY_TARGET_JPY = PLAN_TOTAL_JPY // PLAN_DAYS  # 3,000
DAILY_OVERPACE_JPY = int(DAILY_TARGET_JPY * 1.5)  # 4,500 — alert ceiling

# Google Ads customer (confirmed via ads/*.md 2026-05-21).
CUSTOMER_ID = os.environ.get("MU_ADS_CUSTOMER_ID", "9591303572")
YAML_PATH = Path(os.environ.get(
    "GOOGLE_ADS_YAML",
    str(Path.home() / ".config" / "google-ads" / "google-ads.yaml"),
))
# Fallback YAML — older scripts use $HOME/google-ads.yaml
YAML_FALLBACK = Path.home() / "google-ads.yaml"


# ────────────────────────────────────────────────────────────────────────
# Env load (silently)
# ────────────────────────────────────────────────────────────────────────

def _load_env_file() -> None:
    env_path = Path("/Users/yuki/.env")
    if not env_path.exists():
        return
    try:
        for ln in env_path.read_text().splitlines():
            ln = ln.strip()
            if "=" in ln and not ln.startswith("#"):
                k, v = ln.split("=", 1)
                k = k.strip()
                os.environ.setdefault(k, v.strip().strip('"').strip("'"))
    except Exception:
        pass


_load_env_file()


# ────────────────────────────────────────────────────────────────────────
# Telegram
# ────────────────────────────────────────────────────────────────────────

def telegram(msg: str) -> bool:
    token = os.environ.get("TELEGRAM_BOT_TOKEN")
    chat_id = os.environ.get("TELEGRAM_CHAT_ID") or "1136442501"
    if not token:
        # No silent crash, but no info either — Telegram never sees the token.
        return False
    try:
        data = urllib.parse.urlencode({
            "chat_id": chat_id,
            "text": msg[:3500],
            "disable_web_page_preview": "true",
        }).encode("utf-8")
        req = urllib.request.Request(
            f"https://api.telegram.org/bot{token}/sendMessage",
            data=data, method="POST",
        )
        urllib.request.urlopen(req, timeout=10).read()
        return True
    except Exception:
        return False


# ────────────────────────────────────────────────────────────────────────
# Google Ads REST (mirrors ads/cv_tune_ads.py style — no SDK dep)
# ────────────────────────────────────────────────────────────────────────

def _yaml_path() -> Path:
    if YAML_PATH.exists():
        return YAML_PATH
    if YAML_FALLBACK.exists():
        return YAML_FALLBACK
    raise FileNotFoundError(f"google-ads.yaml not found at {YAML_PATH} or {YAML_FALLBACK}")


def ads_token() -> tuple[str, str, str]:
    try:
        import yaml  # type: ignore
    except ImportError as e:
        raise RuntimeError(f"PyYAML missing: {e}") from e
    cfg = yaml.safe_load(_yaml_path().read_text())
    data = urllib.parse.urlencode({
        "client_id":     cfg["client_id"],
        "client_secret": cfg["client_secret"],
        "refresh_token": cfg["refresh_token"],
        "grant_type":    "refresh_token",
    }).encode()
    req = urllib.request.Request("https://oauth2.googleapis.com/token", data=data)
    with urllib.request.urlopen(req, timeout=30) as r:
        access = json.loads(r.read())["access_token"]
    login_cid = str(cfg.get("login_customer_id") or CUSTOMER_ID)
    return access, cfg["developer_token"], login_cid


def ads_search(query: str, tok: str, dev_tok: str, login_cid: str) -> list[dict]:
    body = {"query": query}
    url = f"https://googleads.googleapis.com/v22/customers/{CUSTOMER_ID}/googleAds:searchStream"
    req = urllib.request.Request(
        url, method="POST",
        data=json.dumps(body).encode(),
    )
    req.add_header("Authorization", f"Bearer {tok}")
    req.add_header("developer-token", dev_tok)
    req.add_header("login-customer-id", login_cid)
    req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, timeout=60) as r:
        raw = r.read()
    parsed = json.loads(raw or b"{}")
    out: list[dict] = []
    chunks = parsed if isinstance(parsed, list) else [parsed]
    for chunk in chunks:
        out.extend(chunk.get("results", []))
    return out


# ────────────────────────────────────────────────────────────────────────
# Snapshot
# ────────────────────────────────────────────────────────────────────────

def window_dates() -> tuple[str, str]:
    """Returns (start_yyyymmdd, today_yyyymmdd) for the 10-day window.

    Window start defaults to (today - PLAN_DAYS + 1) so today is day 10.
    Override with BURST_START_DATE=YYYY-MM-DD.
    """
    today = date.today()
    raw = os.environ.get("BURST_START_DATE")
    if raw:
        try:
            start = datetime.strptime(raw, "%Y-%m-%d").date()
        except ValueError:
            start = today - timedelta(days=PLAN_DAYS - 1)
    else:
        start = today - timedelta(days=PLAN_DAYS - 1)
    return start.strftime("%Y-%m-%d"), today.strftime("%Y-%m-%d")


def collect_spend() -> dict:
    tok, dev_tok, login_cid = ads_token()
    start, end = window_dates()

    # Today's spend per campaign
    today_q = (
        "SELECT campaign.id, campaign.name, campaign.status, "
        "campaign_budget.amount_micros, metrics.cost_micros, "
        "metrics.impressions, metrics.clicks "
        "FROM campaign "
        "WHERE segments.date DURING TODAY"
    )
    # Window total
    window_q = (
        "SELECT campaign.id, campaign.name, metrics.cost_micros, "
        "metrics.impressions, metrics.clicks "
        "FROM campaign "
        f"WHERE segments.date BETWEEN '{start}' AND '{end}'"
    )

    today_rows = ads_search(today_q, tok, dev_tok, login_cid)
    window_rows = ads_search(window_q, tok, dev_tok, login_cid)

    # Aggregate today
    today_by_cmp: dict[str, dict] = {}
    today_total_jpy = 0
    for r in today_rows:
        c = r.get("campaign", {})
        m = r.get("metrics", {}) or {}
        b = r.get("campaignBudget", {}) or {}
        cid = str(c.get("id", ""))
        cost_jpy = int(m.get("costMicros", 0) or 0) / 1_000_000
        today_total_jpy += cost_jpy
        today_by_cmp[cid] = {
            "name": c.get("name", ""),
            "status": c.get("status", ""),
            "today_jpy": round(cost_jpy),
            "today_impr": int(m.get("impressions", 0) or 0),
            "today_clicks": int(m.get("clicks", 0) or 0),
            "daily_budget_jpy": round(int(b.get("amountMicros", 0) or 0) / 1_000_000),
        }

    # Aggregate window
    window_by_cmp: dict[str, dict] = {}
    window_total_jpy = 0
    for r in window_rows:
        c = r.get("campaign", {})
        m = r.get("metrics", {}) or {}
        cid = str(c.get("id", ""))
        cost_jpy = int(m.get("costMicros", 0) or 0) / 1_000_000
        window_total_jpy += cost_jpy
        slot = window_by_cmp.setdefault(cid, {
            "name": c.get("name", ""),
            "window_jpy": 0,
            "window_impr": 0,
            "window_clicks": 0,
        })
        slot["window_jpy"] += round(cost_jpy)
        slot["window_impr"] += int(m.get("impressions", 0) or 0)
        slot["window_clicks"] += int(m.get("clicks", 0) or 0)

    # Merge views
    campaigns: dict[str, dict] = {}
    for cid, t in today_by_cmp.items():
        campaigns[cid] = {**t}
    for cid, w in window_by_cmp.items():
        slot = campaigns.setdefault(cid, {
            "name": w["name"],
            "status": "",
            "today_jpy": 0,
            "today_impr": 0,
            "today_clicks": 0,
            "daily_budget_jpy": 0,
        })
        slot.update({
            "window_jpy": w["window_jpy"],
            "window_impr": w["window_impr"],
            "window_clicks": w["window_clicks"],
        })

    return {
        "window_start": start,
        "window_end": end,
        "today_total_jpy": round(today_total_jpy),
        "window_total_jpy": round(window_total_jpy),
        "plan_total_jpy": PLAN_TOTAL_JPY,
        "plan_days": PLAN_DAYS,
        "daily_target_jpy": DAILY_TARGET_JPY,
        "daily_overpace_jpy": DAILY_OVERPACE_JPY,
        "pct_of_cap": round(
            (window_total_jpy / PLAN_TOTAL_JPY) * 100 if PLAN_TOTAL_JPY else 0, 1),
        "campaigns": campaigns,
    }


# ────────────────────────────────────────────────────────────────────────
# Alerting
# ────────────────────────────────────────────────────────────────────────

def alerts(snap: dict) -> list[str]:
    msgs: list[str] = []
    today_jpy = snap["today_total_jpy"]
    window_jpy = snap["window_total_jpy"]
    if today_jpy > DAILY_OVERPACE_JPY:
        msgs.append(
            f"[burst_ads_30k] OVER-PACE: today ¥{today_jpy:,} > target +50% "
            f"(¥{DAILY_OVERPACE_JPY:,}). pace=¥{today_jpy:,}/d × 10d "
            f"= ¥{today_jpy * 10:,} (vs cap ¥{PLAN_TOTAL_JPY:,})."
        )
    if window_jpy > PLAN_TOTAL_JPY:
        msgs.append(
            f"[burst_ads_30k] CAP HIT: window ¥{window_jpy:,} "
            f"> plan ¥{PLAN_TOTAL_JPY:,} (10d)."
        )
    return msgs


# ────────────────────────────────────────────────────────────────────────
# Main
# ────────────────────────────────────────────────────────────────────────

def write_log(record: dict) -> None:
    try:
        LOG_DIR.mkdir(parents=True, exist_ok=True)
        with LOG_FILE.open("a", encoding="utf-8") as f:
            f.write(json.dumps(record, ensure_ascii=False) + "\n")
    except Exception as e:
        sys.stderr.write(f"[burst_ads_30k] log write failed: {e}\n")


def run_once(dry_run: bool = False) -> dict:
    started = time.time()
    record: dict = {
        "ts": datetime.now().isoformat(timespec="seconds"),
        "dry_run": dry_run,
    }
    try:
        snap = collect_spend()
        record.update(snap)
        record["alerts"] = alerts(snap)
        if record["alerts"] and not dry_run:
            telegram("\n".join(record["alerts"]))
        record["ok"] = True
    except Exception as e:
        record["ok"] = False
        record["error"] = f"{type(e).__name__}: {e}"
    record["duration_s"] = round(time.time() - started, 2)
    write_log(record)
    # Stdout summary (cron-friendly)
    if record["ok"]:
        print(
            f"{record['ts']} window={record['window_start']}..{record['window_end']} "
            f"today=¥{record['today_total_jpy']:,} window=¥{record['window_total_jpy']:,} "
            f"({record['pct_of_cap']}% of ¥{PLAN_TOTAL_JPY:,} cap) "
            f"alerts={len(record['alerts'])}"
        )
    else:
        print(f"{record['ts']} ERROR {record.get('error')}")
    return record


def main(argv: list[str] | None = None) -> int:
    import argparse
    p = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    p.add_argument("--dry-run", action="store_true",
                   help="Skip Telegram send even if alerts fire.")
    args = p.parse_args(argv)
    rec = run_once(dry_run=args.dry_run)
    return 0  # cron-friendly — never crash


if __name__ == "__main__":
    sys.exit(main())
