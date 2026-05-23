#!/usr/bin/env python3
"""Check Smart Bidding learning state and alert on transitions.

Compares current state with last-known (stored in /tmp/learning_state.json).
Sends Telegram alert when:
  - Campaign EXITS learning (transitions to ELIGIBLE) — optimization can resume
  - Campaign ENTERS learning — caution flag
  - Campaign STAYS in learning > 7 days — stuck (manual intervention)

Importable as `check_and_alert()` for embedding in other scripts.
"""
import sys, json
from pathlib import Path
from datetime import datetime

sys.path.insert(0, str(Path(__file__).parent))
from ads_lib import client_for, search_all, tg, ACCTS

STATE_FILE = Path("/tmp/learning_state.json")
ALERT_AFTER_DAYS_STUCK = 7


def check_and_alert(send_tg: bool = True, print_summary: bool = True):
    """Snapshot current learning state, diff with cache, alert on transitions.
    Returns (current_state_dict, alerts_list, learning_count, stable_count)."""
    last = {}
    if STATE_FILE.exists():
        try:
            last = json.loads(STATE_FILE.read_text())
        except Exception:
            last = {}

    now_iso = datetime.utcnow().isoformat()
    current = {}
    for cid, label in ACCTS:
        c = client_for(cid)
        for r in search_all(c, cid, """
        SELECT campaign.id, campaign.name, campaign.bidding_strategy_system_status, campaign.bidding_strategy_type
        FROM campaign WHERE campaign.status='ENABLED'"""):
            key = f"{cid}_{r.campaign.id}"
            st = r.campaign.bidding_strategy_system_status.name
            bid_type = r.campaign.bidding_strategy_type.name
            current[key] = {
                "acct": label, "campaign": r.campaign.name,
                "status": st, "bid_type": bid_type,
                "first_seen": last.get(key, {}).get("first_seen", now_iso) if last.get(key, {}).get("status") == st else now_iso,
            }

    alerts = []
    for key, c_state in current.items():
        l_state = last.get(key, {})
        was = l_state.get("status")
        now = c_state["status"]
        if was is None: continue  # don't alert on first-run "?" → state
        if was == now: continue
        if "LEARNING" in was and "LEARNING" not in now:
            alerts.append(f"✅ {c_state['acct']}/{c_state['campaign'][:25]}: EXITED learning ({was} → {now}) — optimization resumes")
        elif "LEARNING" not in was and "LEARNING" in now:
            alerts.append(f"⚠️ {c_state['acct']}/{c_state['campaign'][:25]}: ENTERED learning ({was} → {now}) — auto-loop will hold")
        elif "LEARNING" in now:
            alerts.append(f"🔄 {c_state['acct']}/{c_state['campaign'][:25]}: {was} → {now}")

    # Stuck-in-learning detection
    for key, c_state in current.items():
        if "LEARNING" not in c_state["status"]: continue
        first_seen = datetime.fromisoformat(c_state["first_seen"])
        days_stuck = (datetime.utcnow() - first_seen).total_seconds() / 86400
        if days_stuck > ALERT_AFTER_DAYS_STUCK:
            alerts.append(f"🚨 {c_state['acct']}/{c_state['campaign'][:25]}: STUCK in {c_state['status']} for {days_stuck:.1f} days")

    learning_count = sum(1 for c in current.values() if "LEARNING" in c["status"])
    stable_count = sum(1 for c in current.values() if "LEARNING" not in c["status"])

    if print_summary:
        print(f"  learning: {learning_count}, stable: {stable_count}")
        for c in current.values():
            flag = "🔴" if "LEARNING" in c["status"] else "🟢"
            print(f"  {flag} [{c['acct']}] {c['campaign'][:35]:<35} {c['status']}")

    STATE_FILE.write_text(json.dumps(current, indent=2))
    if alerts and send_tg:
        tg("📡 Learning state changes:\n" + "\n".join(alerts))

    return current, alerts, learning_count, stable_count


if __name__ == "__main__":
    print(f"=== Learning state @ {datetime.now().strftime('%Y-%m-%d %H:%M')} ===")
    _, alerts, _, _ = check_and_alert()
    if alerts:
        print("\n" + "\n".join(alerts))
