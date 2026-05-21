#!/usr/bin/env python3
"""
product_creator_agent — autonomous "what should we make next?" loop for MU.

Reads signals from `products.db` (active count per brand, 7d sold, winner
score via `winner_picker.pick_winners`) and optionally from
`ads/*.csv` (any `conversions` column > 0), then rule-based picks the
single highest-priority brand and asks `generate.py` to produce 3 fresh
designs sequentially with `NO_DELAY=1` and a 10s gap between calls.

Priority score per brand:
    priority = 0
        + 10 if active < TARGET (gap fill)
        + 20 * recent_sold_7d   (proven demand)
        + 5  if winner score > 0
        + 3  if any ads campaign has > 0 impressions for this brand
Tie-breaker: mugen wins.

CLI:
    python product_creator_agent.py                    # one-shot
    python product_creator_agent.py --dry-run          # decision only
    python product_creator_agent.py --daemon           # 2h sleep loop
    python product_creator_agent.py --brand mugen --n 5  # manual override

Side effects:
    - Appends one JSON line per execution to logs/product_creator_agent.jsonl
    - Prints the same JSON line to stdout
    - Calls `python generate.py <brand>` via subprocess (sequential, 120s
      timeout each, NO_DELAY=1). Never parallel.
    - On exception: prints error JSON and exits 0 (cron friendliness).

NOT in scope (do not edit/touch from this script):
    - generate.py itself
    - products.db write paths (read-only here)
    - Stripe / Resend / Telegram production sends (alert is optional & best-effort)
"""
from __future__ import annotations

import argparse
import csv
import json
import os
import sqlite3
import subprocess
import sys
import time
import traceback
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
DB_PATH = Path(os.environ.get("MU_DB", str(ROOT / "products.db")))
GENERATE_PY = ROOT / "generate.py"
ADS_DIR = ROOT / "ads"
LOG_DIR = ROOT / "logs"
LOG_FILE = LOG_DIR / "product_creator_agent.jsonl"

JST = timezone(timedelta(hours=9))

CORE_BRANDS = ("mugen", "muon", "ma", "nouns")
TARGET_FLOOR = 36
DESIGNS_PER_RUN = 3
GENERATE_TIMEOUT_S = 120
INTER_CALL_SLEEP_S = 10
DAEMON_SLEEP_S = 2 * 60 * 60  # 2h
MAX_CONSECUTIVE_FAILS = 3

# Lazy import so dry-run works even if winner_picker import path differs.
sys.path.insert(0, str(Path(__file__).resolve().parent))


def _now_jst_iso() -> str:
    ts = datetime.now(JST).strftime("%Y-%m-%dT%H:%M%z")
    return ts[:-2] + ":" + ts[-2:]


def _connect_ro(db_path: Path) -> sqlite3.Connection:
    return sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)


def gather_signals(db_path: Path = DB_PATH) -> dict[str, dict[str, Any]]:
    """Return {brand: {active, recent_sold_7d, winner_score, winner_name}} for CORE_BRANDS."""
    out: dict[str, dict[str, Any]] = {b: {
        "active": 0,
        "total": 0,
        "recent_sold_7d": 0,
        "winner_score": 0,
        "winner_name": None,
    } for b in CORE_BRANDS}

    if not db_path.exists():
        return out

    cutoff_iso = (datetime.now(timezone.utc) - timedelta(days=7)).isoformat()
    try:
        con = _connect_ro(db_path)
        try:
            for brand in CORE_BRANDS:
                row = con.execute(
                    "SELECT COUNT(*), COALESCE(SUM(CASE WHEN active=1 THEN 1 ELSE 0 END),0) "
                    "FROM products WHERE brand = ?",
                    (brand,),
                ).fetchone()
                total, active = int(row[0] or 0), int(row[1] or 0)
                out[brand]["total"] = total
                out[brand]["active"] = active

                # Recent 7d sold: sold_out_at OR created_at within window with sold>0
                row = con.execute(
                    """
                    SELECT COALESCE(SUM(sold), 0)
                    FROM products
                    WHERE brand = ? AND sold > 0
                      AND (sold_out_at >= ? OR created_at >= ?)
                    """,
                    (brand, cutoff_iso, cutoff_iso),
                ).fetchone()
                out[brand]["recent_sold_7d"] = int(row[0] or 0)
        finally:
            con.close()
    except sqlite3.Error as exc:
        out["_db_error"] = f"{type(exc).__name__}: {exc}"  # type: ignore[assignment]

    # winner_picker (best-effort import)
    try:
        from winner_picker import pick_winners  # type: ignore
        for brand in CORE_BRANDS:
            winners = pick_winners(brand, top_n=1)
            if winners:
                w = winners[0]
                score = int(w.get("sold", 0) or 0) * 10 \
                    + int(w.get("bid_count", 0) or 0) * 3
                out[brand]["winner_score"] = score
                out[brand]["winner_name"] = w.get("name")
    except Exception:
        # silent — cold-start fallback
        pass

    return out


def gather_ads_signal(ads_dir: Path = ADS_DIR) -> dict[str, int]:
    """Return {brand: total_conversions} from any ads/*.csv with a `conversions` column.

    Brand mapping is best-effort: we look for the brand string in the Campaign
    or Ad group cell. If no CSV carries a conversions column or no row > 0,
    returns empty dict (skipped per spec).
    """
    out: dict[str, int] = {}
    if not ads_dir.exists():
        return out

    for csv_path in sorted(ads_dir.glob("*.csv")):
        try:
            with csv_path.open("r", encoding="utf-8", newline="") as f:
                reader = csv.DictReader(f)
                if not reader.fieldnames:
                    continue
                lower_fields = {fn.lower(): fn for fn in reader.fieldnames}
                conv_field = lower_fields.get("conversions")
                impr_field = lower_fields.get("impressions") or lower_fields.get("impr.")
                campaign_field = lower_fields.get("campaign") or lower_fields.get("campaign name")
                adgroup_field = lower_fields.get("ad group") or lower_fields.get("ad group name")
                if not conv_field and not impr_field:
                    continue
                for row in reader:
                    haystack = " ".join(
                        str(row.get(f, "")) for f in (campaign_field, adgroup_field) if f
                    ).lower()
                    matched_brand = next((b for b in CORE_BRANDS if b in haystack), None)
                    if not matched_brand:
                        continue
                    try:
                        conv = float(row.get(conv_field, 0) or 0) if conv_field else 0.0
                    except (TypeError, ValueError):
                        conv = 0.0
                    try:
                        impr = float(row.get(impr_field, 0) or 0) if impr_field else 0.0
                    except (TypeError, ValueError):
                        impr = 0.0
                    # Track conversions if present, else impressions as a weak signal.
                    weight = int(conv) if conv > 0 else (1 if impr > 0 else 0)
                    if weight:
                        out[matched_brand] = out.get(matched_brand, 0) + weight
        except Exception:
            continue
    return out


def decide(signals: dict[str, dict[str, Any]], ads: dict[str, int]) -> dict[str, Any]:
    """Apply the priority rule and return the winning decision dict."""
    scored: list[dict[str, Any]] = []
    for brand in CORE_BRANDS:
        s = signals.get(brand, {})
        reasons: list[str] = []
        score = 0
        active = int(s.get("active", 0) or 0)
        if active < TARGET_FLOOR:
            score += 10
            reasons.append(f"gap(active={active}<{TARGET_FLOOR})")
        recent = int(s.get("recent_sold_7d", 0) or 0)
        if recent > 0:
            score += 20 * recent
            reasons.append(f"recent_sold_7d={recent}")
        wscore = int(s.get("winner_score", 0) or 0)
        if wscore > 0:
            score += 5
            reasons.append(f"winner_score={wscore}")
        if ads.get(brand, 0) > 0:
            score += 3
            reasons.append(f"ads_signal={ads[brand]}")
        scored.append({
            "brand": brand,
            "score": score,
            "reason": ", ".join(reasons) if reasons else "no_signal",
        })

    # Highest score first; tie-break: mugen > muon > ma > nouns (CORE_BRANDS order)
    scored.sort(key=lambda d: (-d["score"], CORE_BRANDS.index(d["brand"])))
    winner = scored[0]
    return {
        "brand": winner["brand"],
        "score": winner["score"],
        "reason": winner["reason"],
        "all_scores": scored,
    }


def run_generate(brand: str) -> dict[str, Any]:
    """Invoke `python generate.py <brand>` once. Returns a result dict."""
    env = os.environ.copy()
    env["NO_DELAY"] = "1"
    started = time.time()
    try:
        proc = subprocess.run(
            [sys.executable, str(GENERATE_PY), brand],
            cwd=str(ROOT),
            env=env,
            capture_output=True,
            text=True,
            timeout=GENERATE_TIMEOUT_S,
        )
        ok = proc.returncode == 0
        return {
            "brand": brand,
            "ok": ok,
            "rc": proc.returncode,
            "duration_s": round(time.time() - started, 1),
            "stderr_tail": (proc.stderr or "")[-280:],
        }
    except subprocess.TimeoutExpired:
        return {
            "brand": brand,
            "ok": False,
            "rc": None,
            "duration_s": round(time.time() - started, 1),
            "stderr_tail": f"timeout after {GENERATE_TIMEOUT_S}s",
        }
    except Exception as exc:
        return {
            "brand": brand,
            "ok": False,
            "rc": None,
            "duration_s": round(time.time() - started, 1),
            "stderr_tail": f"{type(exc).__name__}: {exc}",
        }


def _telegram_notify(text: str) -> None:
    """Best-effort one-line Telegram alert. Silent if env not set or send fails."""
    token = os.environ.get("TELEGRAM_BOT_TOKEN")
    chat_id = os.environ.get("TELEGRAM_CHAT_ID") or os.environ.get("CHAT_ID")
    if not token or not chat_id:
        return
    try:
        import urllib.request
        import urllib.parse
        data = urllib.parse.urlencode({
            "chat_id": chat_id,
            "text": text[:3500],
            "disable_web_page_preview": "true",
        }).encode("utf-8")
        req = urllib.request.Request(
            f"https://api.telegram.org/bot{token}/sendMessage",
            data=data,
            method="POST",
        )
        urllib.request.urlopen(req, timeout=5).read()
    except Exception:
        pass


def write_log(record: dict[str, Any]) -> None:
    try:
        LOG_DIR.mkdir(parents=True, exist_ok=True)
        with LOG_FILE.open("a", encoding="utf-8") as f:
            f.write(json.dumps(record, ensure_ascii=False) + "\n")
    except Exception as exc:
        sys.stderr.write(f"[product_creator_agent] log write failed: {exc}\n")


def run_once(*, dry_run: bool, brand_override: str | None, n_designs: int) -> dict[str, Any]:
    ts = _now_jst_iso()
    try:
        signals = gather_signals()
        ads = gather_ads_signal()
        decision = decide(signals, ads)
        if brand_override:
            decision = {
                "brand": brand_override,
                "score": -1,
                "reason": f"manual_override (--brand {brand_override})",
                "all_scores": decision["all_scores"],
            }
    except Exception as exc:
        record = {
            "ts": ts,
            "decision": None,
            "results": [],
            "summary": "decision_failed",
            "error": f"{type(exc).__name__}: {exc}",
            "trace": traceback.format_exc(limit=3),
        }
        write_log(record)
        print(json.dumps({k: v for k, v in record.items() if k != "trace"}, ensure_ascii=False))
        return record

    record: dict[str, Any] = {
        "ts": ts,
        "decision": decision,
        "signals": signals,
        "ads": ads,
        "results": [],
        "summary": "",
    }

    if dry_run:
        record["summary"] = f"dry_run brand={decision['brand']} score={decision['score']}"
        write_log(record)
        print(json.dumps(record, ensure_ascii=False))
        return record

    consecutive_fails = 0
    ok_count = 0
    fail_count = 0
    for i in range(n_designs):
        if i > 0:
            time.sleep(INTER_CALL_SLEEP_S)
        result = run_generate(decision["brand"])
        record["results"].append(result)
        if result["ok"]:
            ok_count += 1
            consecutive_fails = 0
        else:
            fail_count += 1
            consecutive_fails += 1
            if consecutive_fails >= MAX_CONSECUTIVE_FAILS:
                record["aborted"] = f"consecutive_fails>={MAX_CONSECUTIVE_FAILS}"
                break

    record["summary"] = f"ok={ok_count} fail={fail_count}"

    write_log(record)
    print(json.dumps(record, ensure_ascii=False))

    if fail_count > 0:
        _telegram_notify(
            f"[mu product_creator_agent] {ts} brand={decision['brand']} "
            f"{record['summary']}"
        )
    return record


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    parser.add_argument("--dry-run", action="store_true",
                        help="Compute decision but do not call generate.py.")
    parser.add_argument("--daemon", action="store_true",
                        help="Loop forever, sleeping 2h between runs.")
    parser.add_argument("--brand", default=None,
                        help="Manual brand override (skip rule-based pick).")
    parser.add_argument("--n", type=int, default=DESIGNS_PER_RUN,
                        help=f"Designs per run (default {DESIGNS_PER_RUN}).")
    args = parser.parse_args(argv)

    if args.daemon:
        while True:
            try:
                run_once(dry_run=args.dry_run, brand_override=args.brand, n_designs=args.n)
            except Exception as exc:
                sys.stderr.write(f"[product_creator_agent] loop error: {exc}\n")
            time.sleep(DAEMON_SLEEP_S)

    try:
        run_once(dry_run=args.dry_run, brand_override=args.brand, n_designs=args.n)
    except Exception as exc:
        sys.stderr.write(f"[product_creator_agent] fatal: {exc}\n")
    return 0  # always green for cron


if __name__ == "__main__":
    sys.exit(main())
