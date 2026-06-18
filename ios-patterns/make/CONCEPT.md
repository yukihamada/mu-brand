# MU Make — 「何が欲しい？」が画面のすべて

iOS アプリ別パターン検証 `ios-patterns/make/`。
**コンセプト: 創作が入口。** Amazon が「買う」を、メルカリが「売る」を空気にしたように、
MU は「作る」を空気にする — このアプリはその一文を画面そのものにする。
ホーム = 巨大なプロンプト入力。ショップ閲覧 (Gallery) は脇役。

- Bundle ID: `com.wearmu.mu.make` / 表示名「MU Make」/ iOS 17.0+ / portrait / xcodegen
- トーン: 黒地に金 #e6c449 (ダークモード固定) + 創作の高揚感 (粒子・シマー・スプリング)
- i18n: 全 UI 文字列 String Catalog (`Localizable.xcstrings`) ja + en。ハードコードなし
- 既存 `ios/` の作法踏襲: `MUAPI` struct / `@MainActor ObservableObject` / Keychain セッション

## ターゲット

- 「自分のTシャツが欲しい」と一度でも思ったことのある人 (道場・チーム・家族・推し)
- ECアプリは開かないが、生成AIで「作る遊び」はする層
- 既存 MU アプリ (観測型: Live/Shop/Closet) と対になる**創作型**の入口

## 画面一覧

| 画面 | 役割 |
|---|---|
| **Make (ホーム)** | フルスクリーン創作入口。巨大見出し「何が欲しい？」+ 複数行プロンプト + kind チップ (おまかせ/TEE/パーカー/ステッカー/マグ/トート/ポスター) + サジェストチップ6種 + 「みんなが作ったばかり」レール |
| **生成中オーバーレイ** | 金の粒子が立ちのぼる Canvas + 月のパルス + 工程テキストの移ろい (実APIは同期20〜90秒 — 待ち時間を体験化)。キャンセル可 |
| **結果 (fullScreenCover)** | デザイン即表示 → 着用イメージ完成をポーリングしてクロスフェード + バッジ。購入 (Stripe Checkout) / 商品ページ / 編集 / 共有 / もうひとつ作る。review 落ちの場合は正直に「確認中」表示・購入ボタン非表示 |
| **Gallery** | 実商品フィードのマソナリー2カラム (「他の人が作ったもの」として見せる)。無限スクロール + pull-to-refresh + 簡易PDPシート (購入可) |
| **Mine** | この端末で作ったもの (ローカル台帳・編集リンク保持) + ログイン (メール→6桁コード→api_key) + 売上/自ストア商品 + アカウント削除 (App Store 5.1.1) |

全画面 loading / empty / error の3状態 (`StateBanner`) + haptics + スプリングアニメーション実装。

## 実API配線 vs スタブ (正直な一覧)

**実配線 (全て store/src/{catalog,agent_api,main}.rs でルート実在確認 + curl 実打済み 2026-06-13):**

| 機能 | エンドポイント | 認証 | 備考 |
|---|---|---|---|
| 生成 | `GET /api/make?prompt=&kind=` | 不要 (匿名可) | 同期で sku/design_url/checkout_url/edit_token が返る。ログイン時は Bearer 添付 → maker_email 帰属 (売上10%)。全体40件/時キャップ・flagged は status=review |
| 着用イメージ | `GET /api/make/peek?sku=` | 不要 | 6秒間隔ポーリング (サーバ側 max-age=5 設計に一致)・約4分で打ち切り |
| みんなの新作 | `GET /api/make/recent` | 不要 | 直近8件 |
| Gallery | `GET /api/shop/feed.json?page=` | 不要 | 既存 MU アプリと同一契約 |
| 購入 | `checkout_url` (Stripe Checkout) | — | SFSafariViewController (Apple Pay は Stripe 側で出る)。既存アプリと同方式 |
| 認証 | `POST /api/agent/register` → `/verify` | — | メール→6桁→api_key (Keychain 保存) |
| 売上 | `GET /api/agent/sales` | Bearer | |
| 自ストア商品 | `GET /api/agent/products` | Bearer | 自ブランド(owner_email)の商品のみ。/make 産 minna は含まれない (サーバ仕様) |
| アカウント削除 | `POST /api/collab/account/delete` | Bearer | App Store 5.1.1(v) |
| 編集 | `edit_url` (`/make/edit/:sku?t=`) | edit_token | Web をアプリ内 Safari で開く (位置/価格/タイトル編集はWebが完成形のため) |

**ローカル実装 (API でなく端末):**
- 「作ったもの」台帳 = `MakeHistory` (Application Support の JSON)。/make は匿名設計でサーバ側に「自分の make 一覧」APIが存在しないため、端末を一次台帳にし edit_url (編集権) も保持する。これはスタブではなく設計判断

**スタブ/未配線 (動かない想像APIは呼ばない方針で除外):**
- 写真添付 (`/api/make/upload`) — API実在確認済みだが multipart 配線とUIは今回スコープ外 (P1)
- Push通知 / Widget / Scan — サーバ側未実装 or 別パターンの領分
- `engine=local` (m5 無料生成) — secrets 依存で不可視のため未使用 (常に Gemini 経路)
- 購入履歴 (買ったもの) — 匿名購入の照会APIが存在しないため Mine には置かず、誇張表示もしない

**検証用の仕掛け:** 起動引数 `-tab gallery|mine` で初期タブ切替、`-autoprompt "<text>"` で入力→生成まで自走 (simctl からの E2E / スクショ自動化用。通常起動には無影響)。

## 自己評価 (5軸・各20点)

| 軸 | 点 | 根拠 |
|---|---|---|
| 体験の新しさ | 17 | 「ECアプリのホーム=入力欄」という逆転。生成待ち時間を粒子演出で体験化、着用イメージ完成のクロスフェードまで一筆書き。既視感のある要素 (チップ/レール) も残る |
| MUらしさ | 18 | 黒×金トーン固定・「作ることを空気に」をホーム一画面で体現・review/匿名帰属/10%還元などサーバの思想 (note 文言含む) をそのまま見せる正直設計 |
| 完成度 | 16 | BUILD SUCCEEDED・実API E2E・3状態/haptics/i18n(ja+en)完備。一方で写真添付なし・編集はWeb依存・実機/審査未検証 |
| 購入転換力 | 15 | 生成直後 (購入意欲のピーク) に checkout 直行ボタン、Gallery からも2タップで Stripe。ただし決済は Safari 経由でネイティブ PaymentSheet 未実装・サイズ選択もStripe任せ |
| 拡張性 | 16 | MUAPI/Models が既存アプリと同契約で P1 (upload/push/widget) の置き場が明確。MakeFlow 状態機械は engine=local 切替も1パラメータ。ただし ios/ 本体との共通コード化は未着手 |
| **合計** | **82/100** | |

## 既知の制約・正直メモ

- 生成は本番APIに対する実行為 (¥12/SKU・公開棚に並ぶ)。E2E検証では1件のみ生成
- `/api/make` は全体40件/時の共有キャップ — 混雑時は 429 を正直にエラー表示
- アプリ内から song 等デジタル商品の購入導線は出ない構成 (kind チップは物販のみ) — IAP 3.1.1 リスク回避は設計書 v1 と同方針
- 未検証: 実機・TestFlight・App Store 審査・Apple Pay 実決済
