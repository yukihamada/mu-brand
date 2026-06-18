# オープン・オペレーション — 原価帳簿の見方とAIエージェントの動き

MU のオペレーションを「全部見せる」というのは具体的にどういうことか。実例ベースで説明します。

## 原価帳簿の構造

ある Tシャツが ¥5,000 で売れたとき、内部では何が起きるか:

```
gross_jpy       = 5,000   (お客様支払額・税込)
                ↓
stripe_fee_jpy  =   200   (3.6% + ¥30 程度)
printful_jpy    = 2,200   (DTG印刷 + EU→JP送料)
ad_cost_jpy    =   400   (Google Ads attribution = 月予算/月販売数)
                ↓
cost_jpy        = 2,800
profit_jpy      = 2,200   (gross - cost)
                ↓
donation_jpy    = 1,100   (profit ÷ 2、§27 condition)
retained_jpy    = 1,100   (再投資 + 給与)
```

これが `donation_ledger` テーブルに 1行ずつ accrued (発生主義) されます。

実際に銀行口座から弟子屈町に振込まれたタイミングで `status = 'sent'` に切り替わり、`sent_at` がセットされます。**お客様1人ごとに「あなたの支払いから¥X円が町に寄付された」が辿れる**設計です。

これは別に綺麗事ではなく、合理的な選択です:
- 寄付額を実態より高く見せる動機がない (=ブランド信頼の根本)
- 寄付額を実態より低く見せる動機もない (=町との関係を毀損したくない)
- 透明性で言質を取られるリスク < 不透明であることで疑われるコスト

## AI エージェントの動き (agent_journal の読み方)

サイトでバックグラウンドで動いているエージェント:

| Agent | 周期 | 役割 |
|---|---|---|
| `mugen_drop` | 1時間 | MUGEN新ドロップ生成 (Gemini 3 image → R2 → DB) |
| `muon_daily` | 1日 | 気温連動の MUON 在庫数を決定 |
| `business_health` | 1時間 | 売上・在庫・原価・寄付の異常検知 |
| `ads_optimizer` | 4時間 | Google Ads の CTR/CPC/CVR を見て KW 入札調整提案 |
| `customer_followup` | 1日 | 14日以上音沙汰なし購入者にサンクスフォローアップ |
| `printful_sync` | 30分 | Printful 注文 status (shipped/delivered) を mu_purchases に sync |
| `donation_accrual` | 1日 | donation_ledger の `accrued` を集計、月次送金候補リスト生成 |
| `chronicle_writer` | 1日 | その日の数字を1パラグラフの blog に変換、auto_blog_posts に下書き |

各エージェントは `agent_journal` テーブルに `observations` (見たもの) + `decisions` (考えたこと) + `actions` (やったこと) を JSON で残します。ダッシュボードの "Agent journal" セクションはこの最新8件を逆順表示しています。

### 失敗ログの例 (これも見せる)

```json
{
  "agent": "mugen_drop",
  "summary": "Failed to generate drop #197 — Gemini quota exceeded",
  "observations": {"hour": 14, "quota_used": "100%", "fallback_tried": "yes"},
  "decisions": ["retry in 1h", "alert telegram"],
  "actions": ["scheduled retry at 15:00", "sent telegram alert"]
}
```

失敗は隠さない。失敗の頻度・パターンが見えることがブランドの実態を伝えます。

## 自己修復メカニズム (4層)

1. **GH Actions hourly cron** — `/healthz` を1時間ごとに叩いて 5xx で fail → alert
2. **Fly platform health check** — 30秒ごと内部 /healthz、3連続失敗で VM 再起動
3. **`drop_filler` agent** — MUGEN ドロップが空になっていたら自動生成リトライ
4. **Telegram bot** — Hetzner サーバから 30分ごとに status reportを `@yukihamada_ai_bot` に投稿

何かが落ちる前提で全部組まれています。

## あなたが見ている数字の精度

ダッシュボード上の数字は:
- **今日の総売上**: `donation_ledger.gross_jpy` の合計 (today_start からの) → Stripe webhook 経由なので 30秒以内の遅延
- **累計購入数**: `mu_purchases` 件数 → 同上
- **累計利益**: `donation_ledger.profit_jpy` 合計 → 上記計算式に基づく見込み (確定は印刷費・送料の actual で adjust される)
- **弟子屈寄付累計**: `donation_ledger.donation_jpy` 合計 → accrued + sent 両方含む

つまり「これは accrued ベース、実際の銀行振込は月末締め」と理解してください。

## なぜTシャツ所有者だけに見せるのか

「公開ダッシュボードでいいのでは」という意見はもっとも。実際これらの数字の一部は `/about` や `/transparency` で 集計値として公開しています。

ただ、**個別の購入ログ・agent journal の生・原価の内訳まで全部** を見せると、競合に丸見えで、価格戦略の自由度がなくなります。Tシャツ所有者というのは「すでに買った人」なので、利害が一致している。

「お客様自身が監査役になる」という体験を作りたかった。これが vault の本質です。

## 関連リンク

- 寄付計画の根拠 (§27): `/constitution#27`
- 公開メトリクス (集計値): `/transparency`
- ブランド原則の全文: `/about`

— 濱田優貴 (MU 創業者)
