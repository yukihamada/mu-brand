# MU — GitHub Actions cron

m5 Mac の cron.sh は脆かった (MUON 9 日抜けの前歴あり)。
信頼性の高い GHA scheduler に「人手不要」系を移管した。

## Workflow 一覧

| File | 起動 | やる事 |
|---|---|---|
| `cron-curl.yml` | 8 種類 (30分/1h/4h/daily/weekly) | wearmu.com の admin endpoint を curl で叩くだけ |
| `cron-twitter.yml` | 毎時 :25 | `twitter_post.py` (tweepy で X 自動投稿) |
| `cron-ads-tune.yml` | daily 01:00 UTC | `ads/cv_tune_ads.py` (Google Ads CPC 自動調整) |
| `deploy.yml` | push to main | Fly deploy (既存) |

GHA cron は **延長 1 時間** 程度遅れる事がある (GHA infra の都合)。
Fly app 内の self-heal watcher (1h tokio task) が遅延を検知して Telegram で
警告する設計になっているので、致命的な抜けには気づける。

## 必須 GitHub Secrets

cron-curl.yml:
- `MU_ADMIN_TOKEN` — server admin token (e.g. `mu-admin-2026`)

cron-twitter.yml:
- `MU_ADMIN_TOKEN`
- `TWITTER_API_KEY` / `TWITTER_API_SECRET`
- `TWITTER_ACCESS_TOKEN` / `TWITTER_ACCESS_TOKEN_SECRET`

cron-ads-tune.yml:
- `MU_ADMIN_TOKEN`
- `GOOGLE_ADS_YAML` — `~/google-ads.yaml` の中身まるごと貼る
- `TELEGRAM_BOT_TOKEN` / `TELEGRAM_CHAT_ID` (optional)

## まだ m5 で動いてるもの (要 future refactor)

- `generate.py mugen/muon/ma` — Gemini 3 Pro Image でデザイン生成 → 局所
  `designs/` フォルダ + Printful + R2 upload + 局所 products.db。GHA に
  移すには「局所 DB 書き込みを admin endpoint 呼び出しに置き換え」+
  「designs フォルダを R2 に常駐」が必要。
- `generate_nouns.py` — Nouns CC0 SVG コラボ生成。同上。
- `generate_lifestyle.py` — 既存 design PNG を読んで人着画生成。**designs/
  をローカルに持つマシンでしか動かない**。R2 から fetch するように直せば
  GHA でも動く。

これらが死んだ場合 Fly self-heal が 1h 以内に Telegram で警告する。

## 手動実行

すべての workflow に `workflow_dispatch` を付けてある。
GitHub UI から: Actions → 該当 workflow → Run workflow。

`cron-curl.yml` には `target` input で個別ジョブだけ走らせられる:
- cv_pulse / thank_buyers / treasury / you_backfill
- sample_grow / sample_grow_force / blog_compose / lottery_draw / council_compose
