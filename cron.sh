#!/bin/bash
# MU Brand Cron Setup
# Run: bash cron.sh install
# Schedules:
#   MUGEN: every hour at :00
#   MUON:  daily at 09:00 JST
#   MA:    1st of month at 00:00 JST

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PYTHON="$(which python3)"
GENERATE="$SCRIPT_DIR/generate.py"
LOG_DIR="$SCRIPT_DIR/logs"
ENV_FILE="$HOME/.env"

mkdir -p "$LOG_DIR"

install_crons() {
    # Remove existing MU brand crons
    crontab -l 2>/dev/null | grep -v "mu-brand" > /tmp/mu_crontab_tmp

    NOUNS_GEN="$SCRIPT_DIR/generate_nouns.py"
    # /you daily backfill needs the admin token; read from env file or default.
    ADMIN_TOKEN="$(grep '^MU_ADMIN_TOKEN=' "$ENV_FILE" 2>/dev/null | cut -d= -f2)"
    : "${ADMIN_TOKEN:=mu-admin-2026}"
    cat >> /tmp/mu_crontab_tmp << EOF
# mu-brand MUGEN (every hour — random sleep 0-55min inside script makes actual time unpredictable)
0 * * * * set -a && source $ENV_FILE && set +a && $PYTHON $GENERATE mugen >> $LOG_DIR/mugen.log 2>&1
# mu-brand MUON (daily midnight UTC — random sleep 0-8h inside script, so appears at random time of day)
0 0 * * * set -a && source $ENV_FILE && set +a && $PYTHON $GENERATE muon >> $LOG_DIR/muon.log 2>&1
# mu-brand MA (monthly 1st)
0 0 1 * * set -a && source $ENV_FILE && set +a && $PYTHON $GENERATE ma >> $LOG_DIR/ma.log 2>&1
# mu-brand NOUNS × MUGEN (weekly Monday — random delay inside script)
0 0 * * 1 set -a && source $ENV_FILE && set +a && $PYTHON $GENERATE nouns_mugen >> $LOG_DIR/nouns_mugen.log 2>&1
# mu-brand NOUNS × MUON (daily — random delay inside script)
0 1 * * * set -a && source $ENV_FILE && set +a && $PYTHON $GENERATE nouns_muon >> $LOG_DIR/nouns_muon.log 2>&1
# mu-brand NOUNS × MA (monthly 15th)
0 0 15 * * set -a && source $ENV_FILE && set +a && $PYTHON $GENERATE nouns_ma >> $LOG_DIR/nouns_ma.log 2>&1
# mu-brand /you daily — triggers Gemini design + email at JST 9:00 (UTC 0:00)
0 0 * * * /usr/bin/curl -s -X POST -H 'Content-Type: application/json' -d '{"admin_token":"$ADMIN_TOKEN"}' https://wearmu.com/api/you/admin/backfill_today >> $LOG_DIR/you_daily.log 2>&1
# mu-brand sample personas grow — daily JST 9:05 (UTC 0:05) — adds 1 design/persona
5 0 * * * /usr/bin/curl -s -X POST -H 'Content-Type: application/json' -d '{"admin_token":"$ADMIN_TOKEN"}' https://wearmu.com/api/admin/sample_grow >> $LOG_DIR/sample_grow.log 2>&1
# mu-brand auto-blog — daily JST 9:10 (UTC 0:10) — Gemini writes today's Field log
10 0 * * * /usr/bin/curl -s -X POST -H 'Content-Type: application/json' -d '{"admin_token":"$ADMIN_TOKEN"}' https://wearmu.com/api/admin/blog_compose >> $LOG_DIR/auto_blog.log 2>&1
# mu-brand lifestyle photos — every 6 hours, generate up to 6 new ones
0 */6 * * * cd $SCRIPT_DIR && set -a && source $ENV_FILE && set +a && $PYTHON generate_lifestyle.py 6 >> $LOG_DIR/lifestyle.log 2>&1
# mu-brand exit-lottery weekly draw — Mondays JST 9:00 (UTC Sun 0:00)
0 0 * * 1 /usr/bin/curl -s -X POST -H 'Content-Type: application/json' -d '{"admin_token":"$ADMIN_TOKEN"}' https://wearmu.com/api/admin/lottery_draw >> $LOG_DIR/lottery_draw.log 2>&1
# mu-brand CV pulse — every 30 min, snapshots metrics, applies adjustments, posts Telegram digest
*/30 * * * * /usr/bin/curl -s -X POST -H 'Content-Type: application/json' -d '{"admin_token":"$ADMIN_TOKEN"}' https://wearmu.com/api/admin/cv_pulse >> $LOG_DIR/cv_pulse.log 2>&1
# mu-brand Google Ads CPC nudge — JST 10:00 daily (UTC 1:00)
0 1 * * * set -a && source $ENV_FILE && set +a && $PYTHON $SCRIPT_DIR/ads/cv_tune_ads.py >> $LOG_DIR/ads_tune.log 2>&1
EOF

    crontab /tmp/mu_crontab_tmp
    rm /tmp/mu_crontab_tmp
    echo "✅ Crons installed:"
    crontab -l | grep mu-brand
}

uninstall_crons() {
    crontab -l 2>/dev/null | grep -v "mu-brand" | crontab -
    echo "✅ MU brand crons removed"
}

case "$1" in
    install)   install_crons ;;
    uninstall) uninstall_crons ;;
    test)
        echo "Testing $2 generation..."
        set -a && source "$ENV_FILE" && set +a
        $PYTHON "$GENERATE" "${2:-mugen}"
        ;;
    *)
        echo "Usage: $0 [install|uninstall|test <brand>]"
        ;;
esac
