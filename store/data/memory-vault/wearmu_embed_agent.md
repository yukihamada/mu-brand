---
name: wearmu.com embed API + 自律エージェント
description: MU 商品の外部 EC 埋め込み 3 経路 + Fly machine 内 1h 自律 agent
type: reference
originSessionId: 1ce5a54b-0fdf-4f54-9bf4-90a31df46c16
---
## 外部 EC 埋め込み (CORS 公開、無認証)

3 つの経路、用途で使い分け:

**1. JS ウィジェット (推奨)**: `<script src="https://wearmu.com/embed.js" data-brand="mugen" data-count="6" data-container="#mu-mount" defer></script>`
   - 8KB、theme=dark/light、auto-mount または `MU.mount({...})` API
   - 在庫切れは灰色化、画像 lazy-load、"powered by MU" フッター必須

**2. iframe**: `<iframe src="https://wearmu.com/embed/products?brand=mugen&count=6" width="100%" height="720"></iframe>`
   - JS 不可サイト用 (Notion, Wix)
   - `Content-Security-Policy: frame-ancestors *` で全オリジン許可
   - `security_headers` middleware が `/embed/` パスを exempt

**3. JSON API**: `GET https://wearmu.com/api/v1/embed/products?brand=mugen&limit=12&available=1`
   - SSR 用 (Next.js, Rails, Python)
   - 返却: `{products: [{id, brand, drop_num, name, image_url(absolute), price_jpy, available, is_auction, checkout_url, share_url}], count, source, docs}`
   - **image_url は絶対 URL** (https://wearmu.com/mockups/... or imgur)

**CORS**: `tower-http cors` feature 追加。`CorsLayer::new().allow_origin(Any).allow_methods([GET,OPTIONS])`。書き込み系 (POST /api/checkout) は同一オリジン縛り維持。

**ドキュメント**: https://wearmu.com/developers (live preview + copy snippet)

## MU マルチエージェント (in-process, 16 agents 並行運転、2026-05-12 自律組織化)

`AGENT_REGISTRY` に登録された 16 agent を独立 interval で並列運転。`agent_scheduler` が 1 分 tick で interval 超えを順次実行。Constitution (static/constitution.md) が source of truth。kill switch: `AGENT_KILL_<NAME>=1`、dry-run: `DRY_RUN_<NAME>=1`、master `AGENT_KILL_ALL=1` / `DRY_RUN_ALL=1`。

| agent | interval | 役割 | Type |
|---|---|---|---|
| `business_health` | 1h | 在庫・SWEEP 👎・FB backlog・blog 不在 | sense |
| `treasury` | 6h | Stripe 売上 + Printful 仕入 → 推定純利益 | sense |
| `customer_support` | 30m | Gemini で FB classify (severity/category/refund/草案) | sense |
| `auto_refund` | 1h | ¥10K 以下の苦情 → Stripe API で自動返金 (DRY_RUN対応) | T2 + T1 escalate |
| `compliance_watch` | 24h | 特商法/PP/Terms の更新日 + GDPR 削除滞留 | sense |
| `self_improvement` | 24h | agent_journal を走査して repeat error 検知 | sense |
| `vision_drift` | 24h | Gemini Pro でブランド drift 検知 | sense |
| `self_evolve` | 24h | Gemini Pro が小改善提案 → ai_decisions (PR は別 workflow) | T1 propose |
| `sns_metrics` | 1h | X tweets/:id/public_metrics → sns_post_metrics | sense |
| `journal_embedder` | 6h | text-embedding-004 で journal を埋め込み → journal_embeddings | sense |
| `support_reply_sender` | 30m | praise/request/shipping/other 草案を 24h 後に Resend 送信 | T2 |
| `catalog_health` | 24h | active collab_products の image到達性 + 利益>0 | T1 escalate |
| `price_micro` | 24h | 7d 売上で ±¥200 微調整 (T2 ±5%/¥500 cap、超 T1) | T2 + T1 escalate |
| `strategist` | 週次 | Gemini Pro が次 7d の動き提案 → governance | T1 propose |
| `weekly_digest` | 週次 | Telegram digest + 7d pending を expire | governance |
| `decision_audit` | 週次 | 30d 経過 decision を heuristic 採点 → agent_scorecard | meta |

**共通 `AgentReport`**: observations / decisions / actions / summary / notable
**`journal_agent_report()`**: 全 agent の結果を agent_journal に INSERT + 必要なら Telegram (per-agent 6h dedup)

**コード位置**: `store/src/main.rs`:
- `AGENT_REGISTRY` / `AgentDef` / `AgentReport` / `agent_scheduler`
- `agent_business_health` / `agent_treasury` / `agent_customer_support` / `agent_auto_refund` / `agent_compliance_watch` / `agent_self_improvement`

**管理画面**:
- `/admin/agents?token=…` — HTML 一覧 (各 agent の状態 / interval / 最終実行 / 24h notable / journal リンク)
- `/admin/agent?token=…&name=<agent>` — 個別 agent の journal JSON

**スキーマ**: `agent_journal(cycle_at, agent_name, observations, decisions, actions, summary, notable, created_at)` + `customer_feedback.ai_action_taken` (auto_refund dedup)

**Env vars**:
- `PRINTFUL_AUTO_CONFIRM`: default `true`
- `AUTO_REFUND_THRESHOLD_JPY`: default `10000`

## セキュリティ (2026-05-11 監査済)

**Admin 認証**: 3 経路の優先順位
1. `Authorization: Bearer <token>` (推奨、URL ログ漏れなし)
2. `X-Admin-Token: <token>` (代替)
3. `?token=<token>` (レガシー、後方互換)

**監査**: 全 admin attempt が `admin_auth_log` テーブルに記録 (ip / path / ok / token_prefix 4ch / via)
- `/admin/auth_log?token=…&failed=1` でブラックリスト IP 確認
- 同一 IP の失敗が 1h で 30 回超 → 429 lockout

**監査履歴 (rotated)**:
- 2026-05-11: 旧 `ADMIN_TOKEN=mu-admin-2026` (git history に流出) → 64-char hex
> [line redacted]
- 新トークンは `/Users/yuki/.config/mu-brand/secrets.env` (chmod 600)

**安全確認済**:
- Stripe / Gemini / Resend / Helius secrets は git history に無し
- 全 SQL クエリは `params![]` (no injection)
- CORS は GET/OPTIONS のみクロスオリジン許可 (POST 不可)
- Stripe webhook: HMAC-SHA256 + 5min replay protection + fail-closed
- `/api/feedback` レート制限: 1/30s + 20/24h per email
- `require_admin_token` は constant-time 比較

## 管理画面 (admin token 必要)

- `/admin/agents?token=…` — 16 agent dashboard (Constitution byte size + governance/insights/audit リンク)
- `/admin/sweep?token=…` — SWEEP 全商品の原価/利益率/Printful link/FB 集計
- `/admin/agent?token=…&name=<agent>` — 個別 agent の journal 直近 50 cycle
- `/admin/insights?token=…` — 7d X反応 + funnel events + journal embedding 数
- `/admin/governance?token=…` — pending T1 escalation 一覧 + approve/reject ボタン
- `/admin/audit?token=…` — agent_scorecard (decision_audit が roll-up)
- `/api/admin/sweep_signals?token=…` — 👍/👎/💬 全件 raw

admin token は `/Users/yuki/.config/mu-brand/secrets.env` (旧 `mu-admin-2026` は git history に流出のため revoke 済 2026-05-11)

## Constitution v1 (2026-05-12)

- `static/constitution.md` が機械可読 single source of truth (vision/principles/T1リスト/budget caps/kill switches)
- Rust 側で `include_str!("../../static/constitution.md")` + `mu_vision()` parser
- `validate_constitution()` で起動時 fail-fast (Vision/5 required sections 必須)
- 変更権限: yuki のみ。git log が audit log。
- 参照事例: Bezos T1/T2 / MakerDAO governance separation / Botto curation / Truth Terminal bounded autonomy / Bridgewater principles / The DAO 失敗からの kill switch 必須化
- 累計 commit: P0 → P5 (fd04303 → c55f086、6 commits、~2400 行)

## 新しい autonomy schema (P0 で追加)

- `autonomy_kill_log`: kill switch 発火の audit
- `autonomy_decision_log`: 全 agent 決定 (reversibility T1/T2, dry_run, executed, escalated, outcome_score)
- `autonomy_governance_queue`: T1 escalation 待ち、approve/reject/expired
- `sns_post_metrics`: X tweet 反応 (P1, sns_metrics agent が書く)
- `funnel_events`: /api/v1/event 経由 (P1, mu-funnel.js が POST)
- `journal_embeddings`: f32[768] LE packed BLOB (P1, journal_embedder が書く)
- `agent_scorecard`: 30d window roll-up (P5, decision_audit が書く)

## Auto-merge harness (self_evolve PR, P2)

- `.github/workflows/self-evolve-merge.yml`: `self-evolve` label の PR にだけ動く
- `scripts/check_auto_merge_allowlist.sh`: file allowlist / diff <50行 / 禁止token / main.rs は string literal + interval_secs + *_THRESHOLD/_CAP 定数のみ
- PR body に `auto-merge-eligible: true` 必須
- Kill switch: repo secret `SELF_EVOLVE_AUTO_MERGE=0`
- cargo check が最終ゲート

## Funnel collector (P1)

- `static/mu-funnel.js` を `<script defer src="/mu-funnel.js"></script>` で読み込むと auto pageview + `[data-funnel="<event>"]` click 送信
- 許可 event: pageview / cta_click / checkout_start / checkout_paid / you_register / you_skip / you_like / share
- 30 分 idle で新 session、`window.MU_FUNNEL.send(event, extra)` で手動送信も可