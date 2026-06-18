---
name: wearmu-cross-heal
description: wearmu.com の自己修復パイプライン (生成 cron + watchdog + 多層検知)
metadata: 
  node_type: memory
  type: project
  originSessionId: 01e1b5cd-b22d-42b3-bfcb-1ac5e9e996d4
---

wearmu.com (MU) は「誰かが止まっても誰かが動いたら治る」前提で組まれている。

**Why:** 0-human apparel ブランドが標榜するからには、単一 agent の停止で
ブランドが沈黙してはダメ。MUGEN 生成が止まる、X 投稿が止まる、メールが
止まる…どれも他の agent が検知して force-run or alert する。

**How to apply:** 修復対象を追加する時は必ず 2 つ用意:
- 主役: 動かす agent (e.g., GH Actions cron, Fly tokio task)
- 監視: 主役の last_seen を見て力業で再起動する agent

現状の冗長 layer:

1. **生成 (MUGEN)**: GH Actions hourly `*/25 * * * *` で
   `python3 generate.py mugen` 実行。`/api/admin/next_drop?brand=mugen`
   で衝突回避。
2. **watchdog (Fly tokio, 5min)**: AGENT_REGISTRY 全 agent の
   `agent_journal.cycle_at` を読み、`now - last > 2*interval + 600s`
   なら force-run。max 3/cycle (stampede 回避)。
3. **gen_stalled detect**: watchdog 内で
   `MAX(products.created_at WHERE brand='mugen') > 6h ago` を見て
   Telegram alert。GH Actions が死んでも気づく。
4. **drop_filler (6h)**: drop_num gap を検出して python コマンドを
   Telegram で yuki に提示 (Fly から Python 実行不可なので nudge 型)。
5. **Telegram 最終層**: watchdog + drop_filler + GH Actions failure
   notify が全部 chat_id 1136442501 に流れる。

エンドポイント:
- `GET /api/admin/next_drop?brand=<x>&token=…` → `{brand, current_max, next}`
- `POST /admin/x/delete?token=…&tweet_id=…` → 誤投稿削除

cron 設定: `.github/workflows/cron-curl.yml` の `mugen_generate` ジョブ。
失敗時に `secrets.TELEGRAM_BOT_TOKEN` で notify。