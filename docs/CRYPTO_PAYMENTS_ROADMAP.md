# Crypto Payments Migration Roadmap

**Owner**: Yuki Hamada (Enabler Inc. / 株式会社イネブラ)
**Last updated**: 2026-05-12
**Status**: PLAN ONLY — no execution yet, requires sign-off per milestone.

## Objective

MU の運営コスト (server / GPU / AI API / email / DNS など) を、技術的に可能な範囲で **クリプト決済 (Solana USDC / SOL 中心)** に移行する。法定代理人 = 株式会社イネブラ。原資は ENAI Treasury (`DK29rBGCvP83LUNjUGVM6xt6qPy6rycBFopXbFkg9XvQ`)。

完全な crypto-only を目指すのではなく、**「ENAI 売上 → Treasury → 運営費」のループを成立** させることがゴール。card 必須サプライヤー (Google / OpenAI / Anthropic / Resend / Fly / Hetzner) は当面残す。

## 現状の月次コスト構造 (2026-05 推定)

| カテゴリ | プロバイダ | 月額 (USD) | 払い | 移行優先度 |
|---------|-----------|-----------|------|----------|
| GPU inference | RunPod (Nemotron 9B + Qwen3-32B) | $50〜200 | card | ★★★ |
| LLM router | OpenAI Codex (OpenClaw fleet) | $20〜80 | card | ★★★ |
| LLM API | Gemini (blog, vision_drift, X 投稿) | $10〜40 | card | ★ |
| LLM API | Anthropic (Claude Code SDK) | (個別) | card | — |
| Web hosting | Fly.io アプリ x 20+ | $30〜80 | card | ★ |
| VPS | Hetzner x 5 (openclaw fleet 4 + soluna-relay) | €30〜50 | card/SEPA | ★★ |
| Email | Resend (digest / 発注 / 当選通知) | $0〜20 | card | — |
| Solana RPC | Helius | $0〜40 | mixed | ✓ already crypto-native |
| DNS | Cloudflare | $0 (free tier) | card | ★ |
| **合計** | | **$140〜510 / 月** | | |

## クリプト決済可能なサプライヤー (2026-05 時点)

| 用途 | 候補 | 通貨 | 信頼度 | 備考 |
|------|------|------|-------|------|
| GPU + LLM inference | **io.net** | USDC / SOL | 高 | Solana ecosystem, KYC 要 |
| GPU inference | Hyperbolic | USDC | 中 | Base / Arbitrum |
| GPU inference | Lilypad / Akash | LP / AKT / USDC | 中 | docker 移植要 |
| LLM router | OpenRouter (top-up USDC) | USDC | 高 | API 互換性高 |
| Decentralized cloud | Akash Network | AKT / USDC | 中 | Fly 代替候補 |
| BTC-accept VPS | Bitlaunch | BTC / ETH / USDT | 中 | Linode/Vultr 再販 |
| BTC-accept VPS | NiceVPS / 1984hosting | BTC / XMR | 中 | 個人運営多い |
| Solana RPC | Helius | SOL | 高 | 既に crypto |
| DNS | Njalla | BTC | 中 | プライバシー寄り |

## マイルストーン

### M0 — Foundation (現状ベースライン確定)
- [ ] 月次 AI コスト記録の自動化 (`ai_budget_usage` 拡張 — 実 spend を Stripe/RunPod/Helius API から取り込む)
- [ ] `ai_budget_config.payment_methods_json = ["card","crypto"]` の意味確定
- [ ] Treasury 残高ダッシュボード (`/admin/budget` を拡張、USDC + SOL 残高表示)
- **完了条件**: 1 ヶ月分の実 spend が JPY / USDC 両建てで集計できる

### M1 — GPU/LLM Inference の crypto 化 (Phase 1A)
- [ ] io.net で Nemotron 9B 同等 pod を立ち上げ、Lambda 側の endpoint を差し替える
- [ ] 既存 RunPod は **3 週間並走** (rollback path 維持)
- [ ] 並走中の cost diff を週次比較
- [ ] OK なら RunPod 解約、io.net 一本化
- **完了条件**: chatweb.ai SSE streaming レイテンシ < 4s, error rate < 1%, 月コスト ≦ RunPod ベースライン
- **見積**: 2-3 週間 (technical) + 1 週間 (account + Treasury 払い込み)

### M2 — LLM Router の crypto 化 (Phase 1B)
- [ ] OpenRouter アカウント開設 (USDC top-up 確認)
- [ ] OpenClaw fleet 4 台 (Hachi / Kuro / Ichi / Ni) の OpenAI Codex 接続を OpenRouter へ swap
- [ ] 既存 OpenAI key は **1 週間 fallback として残置**
- [ ] cost / 品質を `ai_decisions` 経由で比較
- [ ] OK なら OpenAI key 解約
- **完了条件**: 4 台全 fleet が OpenRouter で正常動作、heartbeat 連続 7 日
- **見積**: 1 週間

### M3 — Helius を Treasury 直支払いに切替
- [ ] Helius dashboard で billing method を SOL 払いに変更
- [ ] Treasury から月初に自動引き落とし
- **完了条件**: 1 ヶ月分の RPC コストが Treasury から引き落とし
- **見積**: 1 日

### M4 — Crypto Budget Auto-Settle (機構整備)
- [ ] `ai_budget_settle` cron 追加 (月初 JST 09:00 UTC)
- [ ] 前月の `ai_budget_usage` 合計を見て、Treasury から OpenRouter / io.net / Helius へ自動 USDC top-up
- [ ] 各サプライヤー残高 < 7日分なら Telegram alert
- [ ] `/admin/budget/settle` 手動 trigger 用 endpoint
- **完了条件**: M1-M3 が動いていて、月初 cron で残高補充が自動成立
- **見積**: 1-2 週間

### M5 — VPS の crypto 化パイロット (Phase 2)
- [ ] Bitlaunch で 1 台立ち上げ、openclaw fleet の Ni (178.104.60.154) を rsync 移行
- [ ] DNS 切替 (Cloudflare 経由のまま IP 差し替え)
- [ ] BTC 払いで 1 ヶ月運用、可用性と費用を Hetzner と比較
- [ ] OK なら残り 3 台 (Hachi / Kuro / Ichi) も順次移行
- **完了条件**: 月 €40 程度の Hetzner spend が BTC 払いに切替
- **見積**: 3 週間

### M6 — Decentralized Hosting 検証 (Phase 3, optional)
- [ ] Akash Network に mu-store の docker image を deploy (staging)
- [ ] Fly.io と並走、レイテンシ / 安定性 / 価格を 4 週間ベンチマーク
- [ ] **見送り条件**: Akash > Fly でない (price / latency / ops のいずれかで明確に劣る場合 → 移行しない)
- **完了条件**: 客観データに基づいた採否判断
- **見積**: 4 週間

### M7 — DNS / Email の検討 (Phase 3+)
- [ ] **DNS**: Cloudflare の free tier で運用継続 (移行しない理由: 速度・信頼性・無料)
- [ ] **Email**: Resend 継続 (移行しない理由: deliverability 確保の難易度)
- [ ] 毎四半期、上記判断を再検証
- **完了条件**: 明示的に「移行しない」を文書化

## 依存関係グラフ

```
M0 ─────┬─────► M1 ────► M4
        ├─────► M2 ────►/
        ├─────► M3 ───►/
        └─────► M5
                M6 (independent)
                M7 (independent, eval only)
```

## 予算ガード (M4 で実装する自動化)

```
1. 月初 JST 09:00 UTC:
   - 前月 ai_budget_usage SUM by paid_by='card' → 法人カード残高アラート
   - 前月 ai_budget_usage SUM by paid_by='crypto' → Treasury USDC で各サプライヤー top-up
2. Treasury USDC 残高 < 月予算 * 2 → Telegram alert
3. ai_budget_config.monthly_limit_jpy ≧ Σ(card + crypto) の超過チェック → AI 全停止
```

## リスクレジスター

| リスク | 影響 | 対策 |
|--------|------|------|
| io.net の SLA 不明 | inference downtime → chatweb.ai 障害 | M1 で 3 週間並走、rollback 即可 |
| OpenRouter の rate-limit | OpenClaw fleet が止まる | fallback として OpenAI key を 1 週間維持 |
| Bitlaunch の事業継続性 | VPS 消失 | 月次 backup + 既存 Hetzner は M5 後も 1 ヶ月保持 |
| Treasury 残高不足 | crypto top-up 失敗 → AI 停止 | M4 で 2 週間前 alert + card auto-fallback |
| KYC 要求 | Enabler Inc. として法人 KYC を求められる | 法人書類は事前用意 (登記簿・代表者証明) |
| 為替変動 | USDC → JPY 換算のブレ | ai_budget_usage に snapshot 時点 rate を記録 |

## 各 M の Rollback Strategy

| M | rollback コスト | rollback 期限 |
|---|---------------|--------------|
| M1 | endpoint 1 行差し替え | 並走 3 週間中はいつでも |
| M2 | OpenRouter key を OpenAI key に戻す | 1 週間 |
| M3 | Helius billing 設定戻し | 即時 |
| M4 | cron 無効化 | 即時 |
| M5 | DNS 戻し + rsync 逆方向 | 数時間 |
| M6 | Fly のまま, Akash dry-up | 即時 |

## 想定タイムライン (gantt-style)

```
2026-05 ─ M0 (foundation)
2026-06 ─ M1 + M2 (LLM/GPU を crypto へ)
2026-07 ─ M3 + M4 (Helius + auto-settle)
2026-08 ─ M5 (VPS パイロット)
2026-09 ─ M6 (Akash 検証, 結果次第で M5 残り完了)
2026-10 ─ M7 (DNS/Email 見送り判断), 全体 review
```

## 移行しないもの (明示)

- **Fly.io アプリ群**: 価格・SLA・運用面で代替なし。Akash 検証 (M6) で覆らない限り継続
- **Stripe**: 収益側 — 当然継続、対象外
- **Resend**: deliverability 確保コストが高く、自前 SMTP の障害コスト > メリット
- **Cloudflare DNS**: 無料 + 高品質 — 移行する理由がない
- **Anthropic (Claude Code SDK)**: 個人ユースケース、card で支払い続行
- **Gemini**: Google のみ card 必須。M1 後に inference 部を io.net 自前 LLM へ寄せれば自然に減る

## 質問 (yuki sign-off 要)

各マイルストーン開始前に以下を確認:

1. M1 — io.net account 開設 (Enabler Inc. 法人 KYC) いつ着手？
2. M2 — OpenRouter account 復活させる？ (既存があれば top-up のみ)
3. M4 — Treasury からの自動 top-up は **承認 1 回 / 月次** で OK? それとも閾値 alert + 手動承認毎回？
4. M5 — Bitlaunch / NiceVPS どっちで始める？ 推し: Bitlaunch (Linode 再販なので品質保証あり)
5. M6 — Akash 検証はやる/見送り？ (見送りなら docs 上で明示)

## 関連リソース

- ENAI Treasury: `DK29rBGCvP83LUNjUGVM6xt6qPy6rycBFopXbFkg9XvQ`
- 法定代理人: 株式会社イネブラ (Enabler Inc.) / 〒102-0074 東京都千代田区九段南1丁目5番6号
- 既存実装:
  - `ai_budget_config` / `ai_budget_usage` テーブル (`store/src/main.rs`)
  - `/admin/budget` endpoint
  - `BLOG_GEMINI_MODEL`, `SELF_EVOLVE_GEMINI_MODEL` 定数
- 関連 PR: [Gemini Pro + budget 切替 commit `caf08b9`](https://github.com/yukihamada/mu-brand/commit/caf08b9)
