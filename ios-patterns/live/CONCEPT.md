# MU Live — 買い物が、エンタメになる

TikTok 型・縦スワイプ全画面フィードの没入型ショッピング iOS アプリ (MU iPhone アプリの別パターン試作)。
AI が毎時1着生む wearmu.com の「生きている感」を、スクロールではなく**鼓動**として体験させる。

- Bundle ID: `com.wearmu.mu.live` / 表示名「MU Live」/ iOS 17.0+ / portrait 固定 / xcodegen
- 1スワイプ=1商品。ダブルタップで「欲しい」(金のハート炸裂 + heavy haptic)。下から商品情報がせり上がる。

## ターゲット

- 「服を探す」のではなく「眺めて拾う」Z世代〜のモバイルネイティブ。MU の既存導線 (Web グリッド) が
  届かない、目的なしブラウジング層。
- 毎時ドロップという MU 固有の供給リズムを娯楽 (Pulse) として消費する MU ファン。

## 画面一覧

| 画面 | 内容 |
|---|---|
| 🔥 Feed | 全画面縦ページング (`.scrollTargetBehavior(.paging)` + `containerRelativeFrame`)。フルブリード画像 + Ken Burns ズーム (14s 往復) + 薄い黒グラデ。左下に brand/タイトル/価格/「⚡︎ AIが◯時間前に生成」。右側に縦アクションバー (欲しい/共有/詳細)。上部に kind チャンネル (すべて/TEE/パーカー/ラッシュガード/ステッカー)。無限スクロール (page=2,3…) + 次4枚の画像プリフェッチ。loading/empty/error の3状態。 |
| 詳細シート | `.presentationDetents([.medium, .large])` のハーフモーダル。説明全文・価格・生成時刻・sold。購入ボタン → SFSafariViewController で Stripe Checkout。Web リンクで実商品ページ。 |
| ❤️ Wants | ダブルタップした商品のグリッド (2列)。左スワイプで削除ボタンがせり出す + 長押しメニュー削除。タップで詳細シート。空状態あり。 |
| ⚡️ Pulse | ブランドの鼓動。波紋アニメの金ドット +「この24時間で◯着誕生」「最新ドロップ◯分前」+ 新着タイムライン (時刻つき・タップで詳細)。60秒ごと自動再取得。 |
| 共有 | ShareLink で実商品 URL (`pdp_url`) を共有。 |

## 実 API 配線 vs ローカル実装 (正直な一覧)

**実配線 (2026-06-13 に curl 実打で確認した実在 API のみ)**

| 機能 | エンドポイント | 確認内容 |
|---|---|---|
| フィード/Pulse/チャンネル | `GET https://wearmu.com/api/shop/feed.json?page=N&kind=K` | page=1,2 で各60件・created_at 降順。kind=tee/hoodie/rashguard/sticker が正しく絞れることを実打確認。フィールド: sku/brand/description/price_jpy/mockup_url/sold/created_at/pdp_url/checkout_url |
| 購入 | `GET /api/shop/checkout?sku=` (feed の `checkout_url`) | Stripe Checkout を SFSafariViewController で表示 (既存 MU アプリと同方式)。決済完走の E2E は **unverified** (実カードを切らないため) |
| 商品ページ/共有 | `pdp_url` (`https://wearmu.com/shop/:sku`) | ShareLink / Link で使用 |

**ローカル実装 (サーバに API が存在しないもの)**

| 機能 | 実装 | 理由 |
|---|---|---|
| 「欲しい」(ダブルタップ/ハート) | `WantsStore` — UserDefaults JSON・端末内完結 | wants/likes API はサーバに存在しない。PII なし |
| 商品名 | `description` の第一文を表示タイトルに加工 | feed.json に name フィールドが無い |
| サイズ選択 | 非実装 — シートに「購入ページで選択」と正直に表示 | feed.json にサイズ情報が無い (checkout 側で選択) |
| kind=song / house | チャンネル非表示 | 実打で song は TEE が返る (サーバ側の挙動)、house は1件のみ |

**意図的にやらないこと**: ログイン (閲覧+購入に不要)、独自カート (単品即購入)、デジタル商品の販売 UI (IAP リスク — docs/IOS_APP_DESIGN.md §4 準拠)。

## 既存 MU アプリとの関係

- `MUAPI` struct / `FeedProduct` / `@MainActor ObservableObject` / String Catalog (ja+en・ハードコードなし) など作法は `ios/` を踏襲。
- `ios/` には一切手を入れていない (本パターンは `ios-patterns/live/` で独立ビルド)。
- スクショ自動化用の起動引数: `-initialTab feed|wants|pulse` / `-seedWants` (実フィード先頭6点を Wants に投入) / `-autoDetail` (起動後に詳細シートを自動表示)。

## 検証

- `xcodegen generate` → `xcodebuild -destination 'platform=iOS Simulator,name=MU-PAT-LIVE'` → **BUILD SUCCEEDED**
- iPhone 16 / iOS 18.3 シミュレータで Feed/詳細シート/Wants/Pulse を実起動・スクショ (`screenshots/`)
- 実機・実決済・haptics の体感は **unverified** (シミュレータのみ)

## 自己評価 (5軸・各20点)

| 軸 | 点 | 根拠 |
|---|---|---|
| 体験の新しさ | 16 | 縦スワイプ全画面×毎時生成ブランドの「鼓動」可視化は MU 既存導線に無い。TikTok 文法自体は既知 |
| MUらしさ | 17 | 黒地に金 #e6c449・フルブリード+薄黒グラデ・「⚡︎ AIが◯時間前に生成」で自律ブランドの生体感を前面に。Pulse はブランドの正直さ (実データのみ・予告ダミーなし) を踏襲 |
| 完成度 | 15 | 3状態・無限スクロール・プリフェッチ・ja/en・haptics・Ken Burns まで実装しビルド/実起動済み。実機未確認・決済 E2E 未走 |
| 購入転換力 | 13 | 各カードから2タップで Stripe Checkout 到達・「欲しい」が再訪問の種。ただし Apple Pay ネイティブ (PaymentSheet) 不在・Push 不在で衝動の回収力は Web+α 止まり |
| 拡張性 | 15 | MUAPI/Models は既存アプリと同契約 — Push/Widget/PaymentSheet (設計書 P0/P1) をそのまま载せられる。Wants はサーバ API ができたら同期に昇格可能な形 |
| **計** | **76/100** | |
