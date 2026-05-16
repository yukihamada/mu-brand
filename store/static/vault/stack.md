# MUの裏側 — Rust + Gemini + Printful の全レイヤー

これは MU の本気のシステム解説です。Tシャツ所有者だけに公開しています。

## 全体の流れ

```
気象API (wttr.in/Teshikaga)
        ↓ 1時間ごと cron
   weather seed (temp + condition + dhash)
        ↓
   Gemini 3 Pro Image Preview
        ↓ generate_content(["IMAGE","TEXT"])
   PNG 2940×2940 透過
        ↓
   Cloudflare R2 upload
        ↓
   SQLite (Fly volume) に row INSERT
        ↓
   /api/products/item/:id で配信
        ↓ 購入時
   Stripe Payment Link
        ↓ webhook
   Printful EU で 1枚プリント (DTG)
        ↓ 2-3週間
   お客様のところへ
```

このうち**自動化されていないのは何もありません**。誰かが「今日のデザインどうしよう」と悩む工程はゼロです。気温が枚数を決め、月相が seed を変え、AIが絵を描き、注文1件で印刷工場が動きます。

## レイヤー別の詳細

### 1. バックエンド: Rust + axum + SQLite (libsql)

- **何故Rust**: 1コンテナ・1プロセスで全部を動かしたかった。Go や Node でもできるけど、コンパイル時の型安全がEC運用では効く (在庫/価格/権限の取り違えが本当に怖い)。
- **何故 axum**: tower-http のミドルウェアでヘルスチェック・トレーシング・静的配信が組み合わせやすい。
- **何故 SQLite**: 1機械で動く EC なら、Postgres は overkill。 月数千注文までは SQLite + Fly Volume (永続SSD) で十分。 バックアップは `litestream` で R2 にレプリケート。
- **デプロイ**: `git push` → GitHub Actions → `flyctl deploy --remote-only`。canaryなし、即時切替。ヘルスチェック3回失敗で自動ロールバック。

### 2. AI レイヤー: Gemini 3 Pro

主に2モデルを使い分け:

| Model | 用途 | 1コール単価 |
|---|---|---|
| `gemini-3-pro-image-preview` | Tシャツデザイン生成、商品モックアップ | $0.04 |
| `gemini-3-pro-preview` | テキスト生成 (商品名、prompt、ブログ) | $0.001-0.01 |

**ポイント**: 画像生成は `response_modalities=["IMAGE","TEXT"]` を必ず指定。これを忘れると text のみ返ってくる。

prompt 設計の詳細は別記事「プロンプト・クックブック」参照。

### 3. 印刷レイヤー: Printful DTG

- **製品**: Bella+Canvas 3001 Unisex Tee (PF product ID `71`)
- **印刷場所**: ヨーロッパ拠点 (品質安定、グローバル発送可)
- **コスト**: ¥1,800-2,200 / 枚 (size + ship dest 依存)
- **API**: 注文を `POST /v1/orders` で送るだけ。ファイルURLは R2 の公開URL を渡す。

### 4. 決済レイヤー: Stripe Payment Links

- 商品ごとに `payment_link_url` を持つ
- Webhook (`checkout.session.completed`) で `mu_purchases` テーブルに記録
- そこから Printful 発注 + サンクスメール (Resend) + 透明会計 (`donation_ledger` への accrual)

### 5. 監視 + 自己修復

`scripts/ads_monitor_loop.py` のような Python ループが Fly 上で並走している:

- 5分ごとに /healthz チェック
- 1時間ごとに Telegram bot に状況報告
- 商品ページの 4xx 検知で alert
- agent_journal テーブルに毎時 snapshot

これが今あなたが見ている「MUの今全部見せる」ダッシュボードの元ネタです。

## なぜ全部見せるか

普通のECは「在庫」「原価」「マージン」を企業秘密にします。MUは逆。

利益の50%は弟子屈町に寄付すると公約しているので、**利益を実態より高く見せる動機がない**。むしろ「実際にいくら寄付したか」をリアルタイムで証明できる方が、お客様 (= 寄付の最終受益者にとっても価値ある人) との信頼関係が強くなります。

これがあなたが今 vault に入っている意味です。Tシャツを買った = "MU の裏側のオーナーシップを少し買った" と捉えてください。

## あなたができること

- 上記コードベース全部は GitHub `yukihamada/mu-brand` で読めます (このリポジトリの README 末尾参照)
- 同じスタックで似たブランドを作りたい場合、相談ください: info@wearmu.com
- 「ここのアーキ間違ってる」「もっと良い方法ある」というツッコミ歓迎、X で `@yukihamada` まで

— 濱田優貴 (MU 創業者)
