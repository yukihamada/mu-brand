# MU Atelier — ミニマル・ラグジュアリー EC パターン

> 世界品質の「静かな完璧さ」。SSENSE / Aesop / Apple Store 級の編集的な佇まいで、
> AI ブランドであることを声高に言わず、購入転換に最も振り切った MU の別パターン。

- Bundle ID: `com.wearmu.mu.atelier` / 表示名「MU Atelier」 / iOS 17.0+ / portrait
- 構成: xcodegen (`project.yml`) + SwiftUI + Swift Concurrency。既存 `ios/` の作法 (MUAPI struct / @MainActor ObservableObject / Keychain セッション) を踏襲

## コンセプト

MU は毎時間、新しい一着を生む。このパターンはその事実を**数字やカウントダウンで叫ばず**、
「最新の一着」がいつ訪れても完璧な佇まいで置かれている——という編集的な静けさに翻訳する。

- **タイポグラフィ主導**: セリフ見出し (New York) × SF 本文。eyebrow は tracking 2.4 の大文字
- **墨と紙**: ライト=オフホワイト #FAF9F6 / ダーク=墨黒 #0B0B0A の両対応。金 #e6c449 は「売れた点」「保存済みハート」「タブ下線」の針の先ほどのアクセントのみ
- **影なし・角丸なし・罫線は髪の毛一本** (0.5pt / 12% opacity)
- **余白のリズム**: 24pt 基調の水平マージン、レール間 44pt、ステートメント上下 72pt
- **動きは控えめで上質**: ヒーローの stretchy パララックス、グリッド→PDP の matchedGeometryEffect、スケルトンの静かな呼吸 (opacity pulse)

## ターゲット

- 世界の編集的ファッション EC (SSENSE/END./Mr Porter) に慣れた購買者
- 「AI が作った」ではなく「良い物が静かに並んでいる」体験で買いたい層
- ja / en 完全対応 (String Catalog) — 英語が主、日本語も完全

## 画面一覧

| 画面 | 内容 |
|---|---|
| **Home** | フルブリードのヒーロー (最新作・パララックス・タップで PDP) / キュレーションレール「New Arrivals」「Essentials」「BJJ」(横スクロール) / ブランドステートメント一文 / 静かなフッター |
| **Collection** | 2カラムグリッド (影なし罫線なし・画像+一文+価格のみ) / kind テキストタブ (ALL〜HOUSE・金の下線) / 検索+最近の検索語 (ローカル) / 無限スクロール / スケルトン / empty・error も世界観を維持 |
| **PDP** | 大画像 (520pt) / matchedGeometryEffect でグリッドから浮上 / brand eyebrow + セリフタイトル + 説明 + SIZE/MADE 行 / 下部固定の購入バー (価格 + BUY + ウィッシュリスト) / 購入は SFSafariViewController で実 Stripe Checkout |
| **Wishlist** | 保存した商品の静かなグリッド。長押しで削除。空状態も丁寧に |
| **Account** | register/verify (6桁コード) ログイン / 売上 totals / プライバシー / サインアウト / アカウント削除 (App Store 5.1.1 対応・確認ダイアログ) |

## 実 API 配線 vs ローカル実装 (正直な一覧)

すべて 2026-06-13 に curl 実打で確認してから配線。

### 実 API 配線 (本番 wearmu.com)

| 機能 | エンドポイント | 確認内容 |
|---|---|---|
| フィード/グリッド/レール | `GET /api/shop/feed.json?page&kind&q` | page=60件/頁、kind=tee/hoodie/rashguard/sticker で絞り込み有効、q 検索有効を実打確認 |
| 購入 | `GET /api/shop/checkout?sku=` (feed の checkout_url) | SFSafariViewController で実 Stripe Checkout を開く |
| ログイン | `POST /api/agent/register` → `POST /api/agent/register/verify` | 既存 MU アプリと同一契約 (api_key は Keychain 保存) |
| 売上 | `GET /api/agent/sales` (Bearer) | totals (order_count / revenue_jpy) のみ表示 — **未確認**: 自分の api_key での実レスポンス (コード送信メールを伴うため実ログイン E2E は未実施)。契約は既存 ios/ 実装に準拠 |
| アカウント削除 | `POST /api/collab/account/delete` (Bearer) | 既存 MU アプリと同一契約。同上の理由で実打は**未確認** |
| Web PDP / プライバシー | `https://wearmu.com/shop/:sku` / `/privacy` | SFSafariViewController |

### ローカル実装 (サーバに存在しない機能)

| 機能 | 実装 | 理由 |
|---|---|---|
| **Wishlist** | UserDefaults に商品 JSON 丸ごと保存 (`atelier.wishlist.v1`) | サーバにお気に入り API が無い。端末ローカル・オフラインでも一覧表示可 |
| **最近の検索語** | UserDefaults 文字列配列・最大8件 (`atelier.recentSearches.v1`) | 同上 |
| 画像プリフェッチ | URLCache (64MB/512MB) を温める | クライアント側最適化 |

### 配線しなかった / できなかったもの

- `GET /api/brands` — 実打確認済みだが、ブランドチップは Atelier の「厳選レール」美学と競合するため意図的に不使用
- **PDP 複数画像 (extras)** — `GET /api/products/item/:id` は内部 i64 ID 専用 (SKU を渡すと `Cannot parse to i64` を実打確認)。公開 JSON に追加画像が無いため PDP は mockup 1枚。ギャラリーは複数化に備えた page TabView 実装済み
- 購入履歴の明細 — `/api/agent/sales` の公開契約は totals まで。明細は Web に委ねる旨を画面に明記

## 検証用起動引数 (UI 非干渉)

- `-atelier-tab <home|collection|wishlist|account>` 初期タブ
- `-atelier-open-first` Collection 読込後に先頭商品の PDP を開く
- `-atelier-seed-wishlist` Wishlist が空ならフィード先頭4点を保存

## 自己評価 (各20点)

| 軸 | 点 | 根拠 |
|---|---|---|
| 体験の新しさ | 14 | MU 既存アプリ/Web に無い編集的トップ+matched geometry 遷移+検索体験。ただし EC としての文法自体は王道 (意図的) |
| MUらしさ | 15 | 「毎時の一着」をヒーロー=LATEST ARRIVAL として静かに翻訳。MADE: Printed to order, one by one で POD の正直さを保持。金は MU の刻印として極小使用。AI を叫ばない方針はブランドの /transparency 文化と補完関係 |
| 完成度 | 17 | BUILD SUCCEEDED・全画面実データ・ja/en 完全 String Catalog・skeleton/empty/error/無限スクロール/プリフェッチ/ダーク・ライト両対応。実機未検証・実ログイン E2E 未実施が減点 |
| 購入転換力 | 17 | Home ヒーロー即 PDP・PDP 下部固定 BUY バー (常時視界)・チェックアウトは実 Stripe (Apple Pay は Checkout 内)・摩擦になる演出ゼロ。ネイティブ PaymentSheet 未実装が減点 |
| 拡張性 | 15 | レールは (title, kind/query) 定義で追加自由・ギャラリーは複数画像対応済み・Wishlist→サーバ同期への置換容易・String Catalog で多言語追加が機械的 |
| **合計** | **78 / 100** | |

## 未確認 (正直に)

- 実ログイン〜売上表示〜アカウント削除の E2E (メールコード送信を伴うため未実施。契約は既存 ios/ 準拠)
- 実機 (シミュレータのみ検証)
- `q` 検索の日本語クエリ encode はシミュレータ上の手動操作では網羅していない (URLComponents 任せ)
