# MU iPhone アプリ 設計書 v1 (2026-06-12)

「ポケットの中の、生きてるブランド」— MU は毎時間、新しい一着を生む。
アプリはそれを**観測して・言葉で頼んで・贈って・着た後の物語まで持ち歩く**場所。
Web (wearmu.com) の移植ではなく、**Web にできないこと**だけをアプリの背骨にする。

## 0. なぜアプリか (Web で足りない4つ)

| # | Web の限界 | アプリの答え |
|---|---|---|
| 1 | 毎時ドロップに気づけない | **Push 通知 + ホーム画面 Widget**(最新の一着が常に見える) |
| 2 | 住所入力で買うのが面倒 (CVR ボトルネック) | **Apple Pay 2タップ購入**(Stripe PaymentSheet) |
| 3 | served な服の QR/ムーンマーカーが「カメラアプリ→ブラウザ」と遠い | **内蔵 Scan**(/scan・shirt life・イベントチェックイン) |
| 4 | 「言えば届く」(ホシイ) の会話入口が Web フォーム | **会話型 Make**(写真添付=カメラ直結・編集リンクの続きも開ける) |

## 1. 画面構成 (4タブ + Scan)

```
┌──────────────────────────────────┐
│  🔥 Live   🛍 Shop   ✨ Make   👤 Closet │ ← タブバー
│            ( 📷 Scan = Live 右上 +PDP内 )  │
└──────────────────────────────────┘
```

- **🔥 Live** — 毎時生成のドロップフィード (時系列・kind フィルタ)。1枚カード=モック画像+物語の一節。引っ張って更新。「次の一着まで ◯◯分」カウントダウンが最上部で呼吸する。
- **🛍 Shop** — カタログ + PDP。PDP は Web と同等 (物語/chronicle/「他のかたち」横展開) + **Apple Pay ボタン**。ギフト (@handle 宛・住所非開示) もここから。
- **✨ Make** — 会話 UI。テキスト+写真 (カメラ/ライブラリ) → 提案 → 直接購入 or もう1案。途中保存は編集リンク (メール) と同期。
- **👤 Closet** — @handle プロフィール (買った/持ってる/修理=本人のみ・PII ゲートは PR#193 準拠)。ロイヤルティ/アフィリエイト残高。ギフト受取箱。焚き火券などの「券」。
- **📷 Scan** — ムーンマーカー/QR → shirt life (`/shirt/:pid/life`)・イベントチェックイン (`/api/scan/checkin`)・焚き火券。

## 2. 既存 API との接続 (新規サーバ実装を最小化)

すべて wearmu.com の実在エンドポイント (store/src/main.rs 確認済 2026-06-12):

| 機能 | エンドポイント |
|---|---|
| カタログ一覧 | `GET /api/products` (brands) / `GET /api/products/:brand` |
| 商品詳細 | `GET /api/products/item/:id` + `/chronicle` + `/upsell` |
| 購入 | `POST /api/checkout/v2` / `POST /api/shop/checkout` (Stripe) |
| crypto 決済 | `POST /api/checkout/crypto` + `status/:reference` |
| 認証 (magic link) | `POST /api/collab/auth/start` → `GET /api/collab/auth/magic` → `verify` / mypage 系 `GET /mypage/auth/:token` |
| handle 設定 | `POST /api/mypage/handle` |
| エージェント/クリエイター | `POST /api/agent/register` → `verify` / `GET /api/agent/sales` / `GET /api/agent/affiliate` |
| ギフト claim | `GET/POST /api/claim/ma/:token` |
| 透明性 | `GET /api/transparency` (JSON) |
| 更新フィード | `GET /api/updates` |
| スキャン | `GET /api/mark/decode/:value` / `POST /api/scan/checkin` / `POST /api/scan/phash` |
| 天気連動 | `GET /api/weather` |

**新規に必要なサーバ実装 (これだけ)**:
1. `POST /api/app/push/register` — APNs device token 登録 (member 紐付け・セグメント: 全ドロップ/kind 別/ギフト・発送のみ)
2. ドロップ/ギフト/発送イベント → APNs 送信 worker (毎時 gen 完了フックに1行、Stripe webhook に1行)
3. (任意) `GET /api/drops?since=` — Live フィード用の軽量 JSON (現状は /api/products で代替可)

## 3. 技術スタック

- **SwiftUI + Swift Concurrency / iOS 17+**。プロジェクト生成 = **xcodegen** (KAGI と同じ運用: project.yml が真実源。plist キーの欠落ドリフトに注意 — KAGI の罠の再発防止)
- **認証**: magic link → `api_key` を **Keychain** 保存 (UserDefaults 平文は禁止 — パシャの課金バイパス監査の教訓)
- **決済**: 物販 = **Stripe PaymentSheet + Apple Pay**。物理商品は IAP 対象外 (ガイドライン 3.1.3(e)) なので手数料 30% は掛からない。**song などデジタル単品は v1 ではアプリ内に「フル再生・購入」を置かない**(試聴のみ・購入導線非表示) — IAP 強制リスクを構造的に回避
- **Push**: APNs (token ベース)。サーバは fly 上の store に登録 API + 送信 worker
- **Widget**: WidgetKit Small/Medium「最新の一着」(1時間ごと timeline = MU の生成周期と一致)。
- **i18n**: String Catalog (ja/en)。API 側は `?lang=en` 対応済。**UI 文言ハードコード禁止** (workspace MISSION)
- **計測**: enabler-analytics に `app_open` / `drop_view` / `pdp_view` / `pay_begin` / `pay_done` / `make_start` / `scan` (kind ホワイトリストに追加要 — analytics の2大罠に注意)
- **MA 決済の注意**: MA ブランド商品は JP 住所収集必須 (feedback_stripe_ma_shipping) — Apple Pay の shipping contact 必須フラグで担保

## 4. App Store 審査対策 (JiuFlow 10連続リジェクトの教訓を先回り)

1. **4.2 (minimal functionality) 対策が最重要**: WebView ラッパーにしない。全画面 SwiftUI。Web にしかない長尺ページ (透明性レポート等) のみ SFSafariViewController
2. **デモアカウント**: 審査ノートに magic link 不要のレビュー用トークンを用意 (サーバに review-only api_key)
3. **App Privacy**: 収集=メール・購入履歴・(任意)写真。Tracking なし (ATT 不要構成にする)
4. **IAP 指摘の芽を摘む**: デジタルコンテンツ販売 UI をアプリから完全に消す (上記)。「外部で買える」リンクも置かない
5. **提出は fastlane deliver + Apple ID 認証** (API キー認証は審査提出に使えない — workspace 既知の罠)。スクショは実機撮影 (alpha 版スクショ不可)
6. 拒否されたら**文言は spaceship でしか取れない** — 取得スクリプトを最初から仕込む

## 5. MVP スコープ (フェーズ計画)

**P0 — 2週間 (審査に出せる最小)**
- Live フィード / PDP / Apple Pay 購入 / magic link ログイン / Closet (閲覧) / Push (新ドロップ+ギフト) / ja+en
**P1 — +2週間**
- Make 会話 (写真添付) / Scan (マーカー+チェックイン) / Widget / ギフト送り (@handle) / ロイヤルティ表示
**P2 — 検証後**
- Live Activity (発送追跡) / NOUNS / DAO 投票 / 共同編集 / song の扱い再判断 (IAP 入れるか Web 誘導か)

**やらないこと (v1)**: 独自カート (単品即購入のみ・Web と同じ)、Android、iPad 最適化、オフラインモード

## 6. KPI (App が勝ったと言える基準)

- Push 許諾率 > 50% / Push 経由ドロップ閲覧 → 購入 CVR が Web 流入比 2倍
- Apple Pay 利用率 > 60% (住所入力レス効果の実証)
- 7日リテンション > 20% (毎時ドロップ × Widget の習慣化)
- 計測は enabler-analytics + Stripe 実売 (「未確認数字を公開面に載せない」 — /transparency へは実数のみ)

## 7. 人間ゲート / 未確認

- [ ] Bundle ID (`com.wearmu.mu` 案) と App 名 (「MU」単独は商標的に通るか → 商標まるで事前診断)
- [ ] Apple Pay merchant ID 作成 + Stripe ダッシュボードでの Apple Pay ドメイン/証明書設定 (人間)
- [ ] App Store 提出の最終ボタン (Apple ID = 人間のみ)
- [ ] song / デジタル単品の v2 方針 (IAP 30% を飲むか)
- [ ] 未確認: 既存 magic link フローのアプリ用 deep link (`mu://auth/:token` or Universal Links) はサーバ側メールテンプレ変更が必要 — 実装時に要検証

---
*関連: docs/CATALOG_CONTRACT.md / PR#193 (handle profile) / PR#156・#191 (gift) / JiuFlow iOS 拒否チェーン (auto-memory jiuflow_ios_rejection_chain)*
