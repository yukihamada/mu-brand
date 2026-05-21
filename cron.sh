#!/bin/bash
# MU Brand Cron Setup (m5 Mac)
# Run: bash cron.sh install
#
# 注意: 2026-05-11 以降、curl-only / twitter / ads-tune は GitHub Actions に
# 移管した (see .github/workflows/cron-*.yml)。
# m5 cron として残るのは local 状態に依存する Python 生成系のみ:
#   - generate.py mugen/muon/ma   (local designs/, products.db)
#   - generate_nouns.py            (same)
#   - generate_lifestyle.py        (local designs/ から人着画合成)
#
# Fly app 内の self-heal watcher (1h tokio task) が m5 cron 死を検知して
# Telegram で警告する。両方落ちた場合の検知手段は X feed か wearmu.com の
# 404 になるので、最後の防衛線として GHA の cron-curl.yml も /api/health/cron
# を毎日叩く設計を将来追加検討。
#
# Schedules:
#   SELFIMPROVE:   every 10 minutes
#   MUGEN:         every hour at :00
#   MUON:          daily at 09:00 JST
#   MA:            1st of month at 00:00 JST
#   CART-ABANDON:  every 30 min (DRY_RUN unless MU_ABANDON_LIVE=1)
#   POSTPURCHASE:  every 60 min (DRY_RUN unless MU_POSTPURCHASE_LIVE=1)
#   SITEMAP-PING:  daily 03:30 JST (notify Google/Bing of new SKUs)
#   PRODUCT-CREATOR: every 2h (signal-driven, 3 designs/run)
#   X-POST-AGENT:  every 10 min — polls products.db for fresh drops,
#                  posts one tweet each (DRY_RUN unless MU_X_LIVE=1)
#   BURST-ADS-30K: hourly — monitors GA spend toward ¥30K/10d plan,
#                  Telegram alert on over-pace / cap-hit. Read-only.
#   SALES-100K:    hourly — SUMs sold×price_jpy toward ¥100K goal,
#                  alerts on every new order, one-shot 'GOAL HIT'.

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
# NOTE: STAPLE は GitHub Actions に配置 (.github/workflows/cron-staple.yml)。
# m5 cron からは外してある — 2026-05-16 user 要望で「m5 は使わない」方針。
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
# mu-brand auto-thank buyers — hourly, catches every new cs_live_* purchase
15 * * * * /usr/bin/curl -s -X POST -H 'Content-Type: application/json' -d '{"admin_token":"$ADMIN_TOKEN"}' https://wearmu.com/api/admin/thank_buyers >> $LOG_DIR/thank_buyers.log 2>&1
# mu-brand treasury snapshot — every 4h, logs Solana balance + AI budget suggestion
20 */4 * * * /usr/bin/curl -s https://wearmu.com/api/treasury >> $LOG_DIR/treasury.log 2>&1
# mu-brand X (Twitter) auto-post — hourly :25; posts up to 3 fresh drops if TWITTER_* env present
25 * * * * cd $SCRIPT_DIR && set -a && source $ENV_FILE && set +a && $PYTHON twitter_post.py >> $LOG_DIR/twitter_post.log 2>&1
# mu-brand MA Council weekly brief — Monday JST 12:00 (UTC Sun 3:00); Gemini が議題を生成
0 3 * * 1 /usr/bin/curl -s -X POST -H 'Content-Type: application/json' -d '{"admin_token":"$ADMIN_TOKEN"}' https://wearmu.com/api/admin/council_compose >> $LOG_DIR/council.log 2>&1
# mu-brand exit-lottery weekly draw — Mondays JST 9:00 (UTC Sun 0:00)
0 0 * * 1 /usr/bin/curl -s -X POST -H 'Content-Type: application/json' -d '{"admin_token":"$ADMIN_TOKEN"}' https://wearmu.com/api/admin/lottery_draw >> $LOG_DIR/lottery_draw.log 2>&1
# mu-brand CV pulse — every 30 min, snapshots metrics, applies adjustments, posts Telegram digest
*/30 * * * * /usr/bin/curl -s -X POST -H 'Content-Type: application/json' -d '{"admin_token":"$ADMIN_TOKEN"}' https://wearmu.com/api/admin/cv_pulse >> $LOG_DIR/cv_pulse.log 2>&1
# mu-brand Google Ads CPC nudge — JST 10:00 daily (UTC 1:00)
0 1 * * * set -a && source $ENV_FILE && set +a && $PYTHON $SCRIPT_DIR/ads/cv_tune_ads.py >> $LOG_DIR/ads_tune.log 2>&1
# mu-brand SELFIMPROVE (every 10 minutes — score recompute + log)
*/10 * * * * set -a && source $ENV_FILE && set +a && $PYTHON $SCRIPT_DIR/scripts/selfimprove_10min.py >> $LOG_DIR/selfimprove.log 2>&1
# mu-brand CART-ABANDON (every 30 min — DRY_RUN unless MU_ABANDON_LIVE=1)
*/30 * * * * set -a && source $ENV_FILE && set +a && $PYTHON $SCRIPT_DIR/scripts/cart_abandon_mail.py >> $LOG_DIR/cart_abandon.log 2>&1
# mu-brand POSTPURCHASE (every 60 min — DRY_RUN unless MU_POSTPURCHASE_LIVE=1)
17 * * * * set -a && source $ENV_FILE && set +a && $PYTHON $SCRIPT_DIR/scripts/post_purchase_mail.py >> $LOG_DIR/post_purchase.log 2>&1
# mu-brand SITEMAP-PING (daily 03:30 JST — notify Google/Bing of new SKUs)
30 3 * * * set -a && source $ENV_FILE && set +a && $PYTHON $SCRIPT_DIR/scripts/sitemap_ping.py >> $LOG_DIR/sitemap_ping.log 2>&1
# mu-brand PRODUCT-CREATOR (every 2h — signal-driven brand pick + 3 designs)
33 */2 * * * set -a && source $ENV_FILE && set +a && NO_DELAY=1 $PYTHON $SCRIPT_DIR/scripts/product_creator_agent.py >> $LOG_DIR/product_creator_agent.log 2>&1
# mu-brand X-POST-AGENT (every 10 min — polls products.db for new designs; DRY_RUN unless MU_X_LIVE=1)
*/10 * * * * set -a && source $ENV_FILE && set +a && MU_X_LIVE=\${MU_X_LIVE:-} $PYTHON $SCRIPT_DIR/scripts/x_post_agent.py >> $LOG_DIR/x_post.log 2>&1
# mu-brand BURST-ADS-30K (hourly — monitor spend toward ¥30K/10d, NO budget mutation)
0 * * * * set -a && source $ENV_FILE && set +a && $PYTHON $SCRIPT_DIR/scripts/burst_ads_30k.py >> $LOG_DIR/burst_ads.log 2>&1
# mu-brand SALES-100K (hourly — SUM sold×price toward ¥100K, alert on every new order)
0 * * * * set -a && source $ENV_FILE && set +a && $PYTHON $SCRIPT_DIR/scripts/sales_tracker_100k.py >> $LOG_DIR/sales_100k.log 2>&1
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
