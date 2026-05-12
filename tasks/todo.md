# MU 完全自律組織 — 実装 todo

> Updated 2026-05-12 by Claude.
> Source-of-truth は `static/constitution.md` (人間しか書き換えない)。
> このファイルはClaude / yuki の作業進捗トラッカー。完了したらチェック。

## 設計フレーム

4 層モデル (海外自律組織事例から共通抽出):

| 層 | 速度 | 主体 |
|---|---|---|
| 0. Constitution | 不変 (人間更新) | yuki |
| 1. Sensing | 継続観測 | (受動) |
| 2. Execution | 高速・可逆 (T2) | agents 自動 |
| 3. Strategy | 低速・監査付 (T1) | agents 提案 → human approve |
| 4. Governance | 週次 30 分 | yuki |

参照: Botto (curation pattern), Truth Terminal (bounded autonomy),
MakerDAO (governance/executive separation), Bezos T1/T2 doors,
Bridgewater principles, Olas Mech reputation, The DAO failure (kill switch).

---

## P0 — Foundation (半日, コスト 0, approval 不要)

- [ ] `static/constitution.md` を起草・配置
- [ ] `store/src/main.rs` に `include_str!("../../static/constitution.md")` で取り込み
- [ ] `MU_VISION` 定数を `mu_vision()` (constitution Vision section parser) に置換
- [ ] `kill_switch_active(name)` / `dry_run_active(name)` helper を追加
- [ ] `agent_scheduler` に kill_switch check を挿入
- [ ] `agent_auto_refund` に DRY_RUN を組み込み (refund 直前で skip + log)
- [ ] `self_evolve` を `AGENT_REGISTRY` に追加 (interval_secs: 86_400)
- [ ] 新 schema 7 テーブル追加:
  - `autonomy_kill_log`
  - `autonomy_decision_log`
  - `autonomy_governance_queue`
  - `sns_post_metrics`
  - `funnel_events`
  - `journal_embeddings`
  - `agent_scorecard`
- [ ] `startup_validate_constitution()` で Vision 空ならパニック
- [ ] `cargo check` (m5 SSH or local)
- [ ] commit + push → Fly Actions deploy
- [ ] 本番 `/admin/agents?token=…` で self_evolve が列に出ることを確認

## P1 — Sensing 拡張 (2 日, +¥15K/月, X API plan 必要)

- [ ] X API basic plan 申請 (mail@yukihamada.jp)
- [ ] `sns_metrics_collector` agent: 1h, 過去 72h の post を `tweets/:id` で fetch、`sns_post_metrics` に upsert
- [ ] `/api/v1/event` endpoint: visitor_id (cookie) + session_id + event + path → `funnel_events`
- [ ] `enabler-analytics.fly.dev` の `t.js` を改修 — wearmu 経由でも `/api/v1/event` に POST
- [ ] `journal_embedder` agent: 6h, embedding 未生成の journal を Gemini `text-embedding-004` で埋め込み → `journal_embeddings`
- [ ] `find_similar_journal(query, k)` helper: cosine similarity で過去事例 k 件返す
- [ ] `/admin/insights?token=…` page: X post → impressions/clicks、funnel 7 日漏斗 (PV→CTA→checkout→paid)、journal 検索 box

## P2 — Auto-merge harness (1 日, 0 コスト, **PR merge自動化承認**済)

- [ ] `.github/workflows/self-evolve-merge.yml` 作成
- [ ] `scripts/check_auto_merge_allowlist.sh`: 
  - diff size < 50 行
  - 変更 file が `store/src/main.rs` または `static/templates/messages/*.txt` のみ
  - 禁止 token: STRIPE / PRINTFUL_API_KEY / GEMINI_API_KEY / SECRET / password / `DROP ` / `ALTER ` / `DELETE FROM`
  - 行ベース: main.rs の変更行が「文字列リテラル内」「`interval_secs: N` の N」「`pub const *_THRESHOLD*: i64 = N;` の N」のいずれかにマッチ
- [ ] テスト追加: `cargo check` + 既存 unit tests
- [ ] self_evolve PR body に `auto-merge-eligible: true` を含めるよう agent 側を改修
- [ ] kill switch: repo secret `SELF_EVOLVE_AUTO_MERGE=0` で停止

## P3 — 新エージェント 7 本 (3 日, +¥3K/月)

- [ ] `inventory_rebalance` (6h, T2): Printful 在庫薄い size を auto 補充 (¥150K/月 cap)
- [ ] `price_micro` (24h, T2): 売れ筋 +¥200 / 不人気 -¥200 (±5% cap、絶対値 ¥500 超は T1 escalate)
- [ ] `support_reply_sender` (30m, T2): customer_support 草案を 24h 経過 + severity ∈ {low, medium} なら自動送信
- [ ] `strategist` (週次月曜 09:00, T1, Gemini Pro): 次 drop テーマ / 価格帯 / 広告予算を 1 案 → governance_queue
- [ ] `market_sense` (24h, Gemini Pro + web): X trends / Google Trends でブランド外圧検知
- [ ] `performance_audit` (週次, T2): 直近 7 日の `autonomy_decision_log` をスコアリング → `agent_scorecard`
- [ ] `council` (escalation 時): 高 stakes 決定を strategist + vision_drift + self_evolve の 3 agent 合議 → 過半数のみ governance_queue
- [ ] Constitution の budget caps に従って自動停止 (既存 `budget_check` を inventory/price にも適用)

## P4 — Governance UI (1 日, 0 コスト)

- [ ] `/admin/governance?token=…` page (Askama テンプレ): pending T1 一覧 + 承認 / 却下 ボタン
- [ ] `POST /admin/governance/:id/approve` `POST /admin/governance/:id/reject` (CSRF / token guard)
- [ ] 承認時に decision を実行する dispatcher (decision.kind に応じて)
- [ ] 週次月曜 10:00 JST: Telegram weekly digest cron
  - 過去 7 日 notable / governance pending / agent_scorecard 平均
- [ ] 7 日 expire: pending を `status='expired'` に遷移する 1h tick

## P5 — Self-audit loop (1.5 日, +¥1K/月)

- [ ] 全 spending/strategy agent が `autonomy_decision_log` に書く (reversibility tag 必須)
- [ ] `score_past_decisions` agent (週次 T2, Gemini Pro): 30 日経過の決定を outcome score (0–1) + notes で採点
- [ ] `inject_similar_history` helper: 新 decision 提案前に過去類似 decision (低スコアの) を prompt に injection
- [ ] self_evolve / vision_drift / strategist の prompt 改造: `{similar_past_decisions}` block を追加
- [ ] `/admin/audit?token=…` page: agent_scorecard 推移グラフ (Chart.js)

---

## Approval ゲート (この commit までに完了)

- [x] cost 増加 (累計 +¥4.5K/月): user 承認済 (このセッション 2026-05-12)
- [x] auto-merge PR (visible to others): user 承認済
- [x] X API paid plan: user 承認済
- [x] Constitution の constitution.md 配置 + include_str!: user 承認済

## 監査トレイル

- 全 agent 決定 → `autonomy_decision_log` (reversibility tag 付き)
- kill switch 発火 → `autonomy_kill_log`
- governance approve/reject → `autonomy_governance_queue.decided_by / decided_at`
- Constitution 変更 → git diff (yuki commit のみ)
- self_evolve PR → GitHub PR (auto-merge は allowlist 内のみ)
