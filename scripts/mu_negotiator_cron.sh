#!/usr/bin/env bash
# launchd から呼ばれる MU 自走ネゴシエーターの日次tick。
# キル: launchctl unload ~/Library/LaunchAgents/com.yukihamada.mu-negotiator.plist
#       or  この行に MU_NEGOTIATE_ENABLED=0 を立てる(受信/解析だけ動き送信停止)
set -a
[ -f "$HOME/.cron_secrets" ] && source "$HOME/.cron_secrets" 2>/dev/null
set +a
export PATH="/opt/homebrew/bin:/usr/bin:/bin:$PATH"
export MU_NEGOTIATE_ENABLED="${MU_NEGOTIATE_ENABLED:-1}"
LOG="$HOME/.config/mu-negotiator/cron.log"
echo "==== $(date) ====" >> "$LOG"
cd "$HOME/workspace/mu-mfg-impl" && /usr/bin/python3 scripts/mu_negotiator.py tick >> "$LOG" 2>&1
/usr/bin/python3 scripts/rfq_dashboard.py >> "$LOG" 2>&1
