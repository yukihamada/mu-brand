# Crypto Payments Migration Roadmap (Readiness-Triggered)

**Owner**: Yuki Hamada (Enabler Inc. / 株式会社イネブラ)
**Last updated**: 2026-05-12
**Status**: PLAN ONLY — no execution yet.

## 思想

「日付で決めて移行」ではなく **「技術 / 経済 / 法制度が追いついた時点で移行」** とする。
各カテゴリに **Readiness Trigger (移行開始の条件)** を明示し、定期的にウォッチして条件を満たしたら着手。
fixed schedule にしないのは、crypto-native infra のうち多くは 2026 時点でまだ発展途上で、性急な移行は MU の運用品質を下げるリスクがあるため。

## Objective

MU の運営コスト (server / GPU / AI API / email / DNS) を、技術的・経済的に妥当な範囲で **クリプト決済 (Solana USDC/SOL 中心)** に移行する。
ゴールは「ENAI 売上 → Treasury → 運営費」のループ成立。原資 = ENAI Treasury (`DK29rBGCvP83LUNjUGVM6xt6qPy6rycBFopXbFkg9XvQ`)。

## 現状ベースライン (2026-05)

| カテゴリ | プロバイダ | 月額 (USD) | 払い |
|---------|-----------|-----------|------|
| GPU inference | RunPod | $50〜200 | card |
| LLM router | OpenAI Codex (OpenClaw fleet 4台) | $20〜80 | card |
| LLM API | Gemini | $10〜40 | card |
| LLM API | Anthropic Claude | (個別) | card |
| Web hosting | Fly.io アプリ x 20+ | $30〜80 | card |
| VPS | Hetzner x 5 | €30〜50 | card/SEPA |
| Email | Resend | $0〜20 | card |
| Solana RPC | Helius | $0〜40 | mixed (crypto-native) |
| DNS | Cloudflare | $0 | card |
| **合計** | | **$140〜510 / 月** | |

## カテゴリ別 Readiness Predictions

各カテゴリに対し、以下 4 軸で評価する:

- **Q (Quality)**: latency, SLA, error rate
- **C (Cost)**: 既存比 ±20% 以内
- **O (Ops)**: ボタン数、可観測性、debug 可能性
- **L (Legal)**: KYC 要件、税務、Enabler Inc. として支払可

「移行 Ready」= 4 軸すべてが既存に **同等以上**。Readiness は半年ごとに再評価。

---

### 1. LLM Router (OpenAI/Anthropic → crypto-pay router)

**現状**: OpenClaw fleet 4 台 が OpenAI Codex を直接呼出。$20〜80/月。

**移行先候補**: OpenRouter (USDC top-up 可) / Heurist (USDC native) / io.net Intelligence

| 軸 | OpenRouter (2026-05 時点) | 判定 |
|----|--------------------------|------|
| Q | OpenAI / Anthropic / Gemini を proxy 経由で同等品質、+100〜300ms latency | △ |
| C | OpenAI 直より +5〜10% margin | ✅ |
| O | OpenAI API 互換、SDK 差し替え 1 行 | ✅ |
| L | USDC top-up 可、KYC 不要 (個人) / 法人 KYC は account 設定 | ✅ |

**Readiness 判定**: ✅ **既に Ready**。技術待ちなし、決断待ち。

**Trigger**: yuki が「OpenAI Codex は手放してよい」と判断した時点で着手可。

**Predicted migration window**: いつでも (2026-Q2 〜)

---

### 2. GPU / LLM Inference (RunPod → crypto-native GPU)

**現状**: RunPod で Nemotron 9B + Qwen3-32B。$50〜200/月。Lambda がエンドポイントを呼ぶ。

**移行先候補**: io.net (Solana-native) / Hyperbolic / Akash GPU / Lilypad

| 軸 | io.net (2026-05 時点) | 判定 |
|----|----------------------|------|
| Q | A100/H100 pod 利用可、SLA 99.5% 程度 (RunPod 99.9%)、SSE streaming 安定性が時々スパイク | △〜○ |
| C | RunPod とほぼ同等、たまに RunPod 高い (GPU 需給次第) | ✅ |
| O | dashboard はまだ粗い、log は API 経由でしか取れない | △ |
| L | Solana ecosystem、USDC 払い OK、法人 KYC 別途要 | ✅ |

**Readiness 判定**: △ — **技術はギリ実用、Ops は数ヶ月待ち**

**Trigger (3 つ全て満たしたら着手)**:
1. io.net で連続 7 日間 99.5%+ uptime を 1 つの pod で実測
2. dashboard / log 取得 API が production-ready (currently 不安定)
3. 同等 GPU 価格が RunPod ベースライン ±10% に収まる週が 4 週連続

**Predicted ready**: **2026-Q4** (io.net の Series B 資金で infra ops が改善されると想定。ECDSA 検証だけして MUの利益確定的に動く)

**Watch list**: io.net status page weekly check, /admin/ai_decisions の RunPod レイテンシ trend

---

### 3. Solana RPC (Helius)

**現状**: 既に crypto-friendly。Treasury から自動引き落としに切替可能。

**Readiness 判定**: ✅ **Ready Now**

**Trigger**: 単なる billing 設定変更。

**Predicted migration window**: M4 (auto-settle 機構) と同タイミング

---

### 4. VPS (Hetzner → crypto-pay VPS)

**現状**: Hetzner x 5、€30〜50/月。openclaw fleet 4 台 + soluna-relay。

**移行先候補**: Bitlaunch (BTC/USDT, Linode/Vultr 再販) / NiceVPS / 1984hosting

| 軸 | Bitlaunch (2026-05 時点) | 判定 |
|----|--------------------------|------|
| Q | Linode/Vultr 系の SLA を継承 (99.9%) | ✅ |
| C | Hetzner より割高 (+30〜50%) | ✗ |
| O | API ありだが Hetzner Cloud API より機能制限あり | △ |
| L | BTC/USDT 払い、KYC 不要 | ✅ |

**Readiness 判定**: ✗ — **コストが追いついていない**

**Trigger (どちらか満たしたら着手)**:
1. Bitlaunch / 同等 reseller が Hetzner ±20% 以内の価格に下がる
2. Hetzner 自体が USDC 払いを受け入れる (希望薄、ドイツの税制とコンプライアンス上)

**Predicted ready**: **2027-Q2 以降** (crypto-native cloud の供給増 + 競合圧で価格下落想定)

**代替案**: Akash の GPU 以外の汎用 VPS layer が成熟したら同時検討 (こちらの方が早い可能性)

---

### 5. Web Hosting (Fly.io → Akash)

**現状**: Fly.io アプリ 20+ 台、$30〜80/月。コードベースは axum (Rust) / next.js / static。

**移行先候補**: Akash Network (Docker-first, AKT/USDC payment)

| 軸 | Akash (2026-05 時点) | 判定 |
|----|---------------------|------|
| Q | レイテンシ Fly より +50〜200ms、warm start 不安定 | △ |
| C | Fly より 30〜50% 安いケース多い (provider 競争原理) | ✅ |
| O | docker compose 移植要、persistent volume の癖、SSH なし | ✗ |
| L | USDC OK | ✅ |

**Readiness 判定**: ✗ — **Ops 面で Fly の方が圧倒的に楽**

**Trigger (3 つ全て満たしたら着手)**:
1. Akash で persistent volume + cold start < 5s が安定
2. Akash 公式 CLI が Fly CLI と同等の UX (logs / scale / rollback)
3. 信頼できる provider が常時 10 以上ある (現在 5〜8)

**Predicted ready**: **2027-Q3 〜 2028-Q1** (akashic foundation のロードマップ次第)

**Risk**: Fly.io 自体がクリプト払いを始めれば不要に (predicted ready: 2027-Q2 ?)。Fly は Hetzner 系 / 米ベンチャー系で、a16z crypto fund の press release 次第。**Watch**: fly.io blog の crypto pricing keyword をクォータごとに grep。

---

### 6. Foundational LLM API (Gemini / Anthropic)

**現状**: blog (Pro), vision_drift (Flash), self_evolve (Pro), x_brand (Flash), critic_check 全て Gemini。chatweb.ai 等で Anthropic も使用。

**移行先候補**:
- (A) OpenRouter 経由で USDC 払い化 (既存 router を utilize)
- (B) 自前 LLM を io.net で hosting し置換 (Llama 4 / Qwen 3 / DeepSeek-R2 等)

| 軸 | A: OpenRouter proxy | B: 自前 OSS LLM |
|----|--------------------|-----------------|
| Q | 同一モデル経由なので同等 | Gemini Pro 比 60〜80% の品質、blog は許容範囲 / critic は不安 |
| C | +5〜10% margin | 大幅安 ($10/月 程度) |
| O | API 互換、変更 1 行 | endpoint 変更 + prompt tuning 要 |
| L | USDC OK | USDC OK |

**Readiness 判定**: A は ✅ (Ready Now), B は △ (品質次第)

**Trigger**:
- **A**: 上記 #1 (LLM Router) と同タイミング着手
- **B**: OSS LLM の JP 品質が Gemini Pro ±10% に到達。**Watch**: HuggingFace Open LLM leaderboard JP 部門 + 自前で月次 critic_check ベンチ

**Predicted ready**:
- A: **Ready Now**
- B: **2027-Q1 以降** (Llama 5 / Qwen 4 等で JP 品質が一段上がるはず)

---

### 7. Email (Resend → 自前 SMTP)

**現状**: Resend、deliverability 高、$0〜20/月。

**Readiness 判定**: ✗ — **移行しない判断**

**Reason**: 自前 SMTP は IP reputation の維持コストが極めて高い。Resend を crypto pay にする方が筋がよく、Resend が USDC を始めれば 1 行変更で済む。

**Trigger**: Resend が公式に USDC / SOL 払いを受け入れた時点で billing 設定変更のみ

**Predicted ready**: **2028+ (Resend 自体は SaaS 企業、crypto 移行モチベーション低)**

---

### 8. DNS (Cloudflare → Njalla)

**現状**: Cloudflare free tier、無料、CDN + DDoS protection 込み。

**Readiness 判定**: ✗ — **移行しない判断**

**Reason**: 無料・高品質・SLA 圧勝。CDN + DDoS 全部入りで月 $0 を超える価値が crypto pay にない。

**Trigger**: なし (恒久的に Cloudflare で OK)

---

### 9. Stablecoin-acceptance by Mainstream

**Watch list (2026〜2028 で起こる可能性):**

| イベント | 確率予想 | 影響 |
|---------|---------|------|
| Stripe 自身が SaaS 課金に USDC 受け入れ | 中 (2027-Q1) | OpenAI/Anthropic 等が Stripe 経由で間接 USDC OK に |
| Google Cloud が USDC payout | 低 (2028+) | Gemini API 直接 USDC 払いに |
| OpenAI / Anthropic 公式 USDC | 低 (2028+) | 大半の移行を巻き戻し可能 |
| Fly.io / Hetzner USDC | 中 (2027-Q2 / 2028) | Web hosting 全体の移行不要に |

**この watch list を四半期ごとに更新する。**

## 統合ロードマップ (Readiness-Triggered)

```
                       2026                2027                2028
                Q2  Q3  Q4   Q1  Q2  Q3  Q4   Q1  Q2  Q3  Q4
LLM Router      ████████ READY NOW (whenever yuki OKs)
GPU/Inference   ░░░ ████ readiness predicted
Helius RPC      ████████ READY NOW
LLM (self-host)        ░░░░░░░░ █████ readiness predicted
VPS                       ░░░░░░░░░░ █████
Web (Akash)                        ░░░░░░ █████
Stripe USDC?                          ░░░░░░░░ █?
Fly.io USDC?                         ░░░░░░ █?
Email                                          (recommend: never)
DNS                                            (recommend: never)
```

凡例: `█` = 予測 Ready, `░` = 監視中, `?` = mainstream の crypto 受け入れ次第で前倒し可能性

## Foundation (即実装 OK な機構整備)

技術待ちと並行して、機構だけは整えておく。これらは M0〜M4 に相当:

### F0 — 月次コスト計測の自動化
- [ ] RunPod API / Stripe / Resend / Helius 各 dashboard の monthly_spend を毎日取り込み
- [ ] `ai_budget_usage` を実 spend と一致させる
- [ ] `/admin/cost_dashboard` の精度向上

### F1 — Treasury 残高 / 為替モニタ
- [ ] Helius で `DK29rB...` の USDC + SOL 残高を 1h ごとに snapshot
- [ ] JPY 換算 (Coinbase / Kraken API) と並列表示
- [ ] 残高 < 月予算 x 2 → Telegram alert

### F2 — Auto-Settle 機構 (技術が来た時の準備)
- [ ] `ai_budget_settle` cron (月初 JST 09:00 UTC) のスケルトン
- [ ] 各サプライヤーの top-up endpoint を `crypto_recipients` テーブルに登録
- [ ] 払い込み実行は **dry_run** で開始、Telegram 経由で人間 1-tap 承認 → 本実行
- [ ] `paid_by='crypto'` / `paid_by='card'` を usage row に正しく付ける

### F3 — Readiness Watch のオートメーション
- [ ] 四半期ごとに各サプライヤーの crypto 受け入れ status を Gemini で自動チェック
- [ ] watch list の更新 PR を自動 draft
- [ ] このドキュメントの「Predicted ready」を実データで update

F0〜F3 は **技術待ちなしで実装可能**。F2 が動いていれば、各カテゴリの Trigger が満たされた瞬間に「flag 1 つ flip するだけ」で移行できる。

## リスクレジスター (Readiness 判定で見落としやすいもの)

| リスク | 監視 | 対策 |
|--------|------|------|
| crypto migration 中に SOL/USDC が depeg | Helius 経由で USDC peg を 1h 監視 | depeg > 2% で全 settle 停止 |
| KYC 要件が法人化の範囲を超える | 各サプライヤー T&C を四半期 review | 個人 KYC で済む候補を優先 |
| 移行先の事業継続性 (rugpull) | TVL / GitHub commit / Twitter activity を月次 | Top 3 候補を常時並走可能に保つ |
| 為替変動による予算オーバー | snapshot 時点 rate を記録 | monthly_limit_jpy を超えたら AI 全停止 |
| 税務上の取り扱い (USDC 払い = 損金算入可能か) | 顧問税理士に四半期確認 | 全 settle に invoice 紐付け |

## yuki sign-off 待ち

実移行はゼロ。以下のうち承認 / 不承認を返してくれれば文書に反映:

1. **F0〜F3 (foundation) は技術待ちなしで実装着手 OK か?** (推奨: OK)
2. **「Ready Now」枠 (LLM Router + Helius) は yuki の go signal でいつでも着手して良いか?** (推奨: signal 出すまで触らない)
3. **四半期 review の cadence**: 3 ヶ月ごとに私から自動でこの文書の readiness を re-evaluate して PR を出す形で良いか?
4. **mainstream USDC watch (Stripe / Fly.io 等)** はリスト管理だけで良いか、それとも yuki に四半期 alert 送るか?

返信なしでも文書として残る。技術が追いついたら自然に動く設計。
