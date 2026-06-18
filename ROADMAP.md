# wearmu.com / MU × YOU — Feature Roadmap

> 作成 2026-05-10。これまでに実装した「30日トライアル / 一生無料 / R2 移行 / bio 反映 / 9 時整合 / Google Ads アセット」をベースに、**自走できる機能を追加していく順番**。

優先順位は「(A) 売上 / 継続率に直接効く × (B) 実装重さ × (C) コーポレート哲学（速く、ノイズなく）に合うか」の三軸で並べた。

---

## P0 — 今週中に出すべき（売上ループの完成）

### 1. 「仕立てる」一発購入フロー（/you 内 Stripe）
今は `/you` で出てきたデザインを「仕立てる」と書きつつ、実際には注文が走らない。Stripe Checkout を `/you` の design ごとに発行できるように：
- DB: `you_designs` に `stripe_session_id`, `claimed_at`, `printful_order_id`
- API: `POST /api/you/claim` を Checkout 起動 → success → Printful 自動発注
- success ページで `lifetime_free=1` がトリガー（既存の webhook ルートに乗る）
- これで **「30日間、お試し → 1着仕立て → 一生無料」が一画面で完結**

> 効果: 売上 + リテンション両取り。`/you` 単体で finish のある体験になる。

### 2. 共有しやすい OGP
`/<slug>` ページの OGP は static `og.jpg` のまま。ユーザーごとの最新デザインを動的 OGP に：
- `GET /og/u/<slug>.png` → 1200×630 の合成画像（デザイン + 名前 + Day N）を on-the-fly 生成 / R2 cache
- `<meta property="og:image">` を `/og/u/<slug>.png` に
- これで Twitter / LINE 共有時にちゃんと「自分の Tee」が見える

> 効果: 0 → SNS バイラル経路。CAC 下がる。

### 3. 友達紹介で +30 日トライアル延長
- `you_users` に `referrer_id`, `referral_count`
- /you 登録 URL に `?ref=<slug>` → 紹介者の `referral_count++`、自分の trial を +30d
- 紹介者は累計 3 人で `lifetime_free=1` 付与（無料 MU を取る代わりの導線）

> 効果: バイラル係数 K > 0.4 が見えれば事業として化ける。

---

## P1 — 今月中（リテンション + 客単価）

### 4. Skip 学習がデザインに効く
今は Skip カウントを使ってないので「Skip するほど寄っていく」は実質ハッタリ。
- `you_feedback` の skip / like を集計し、好まれたデザインの mood/palette/scene/seed-noun を加重
- Bio + 集計結果を Gemini プロンプトの「最近この人が選んだ系」セクションに

> 効果: 「明日のほうが自分っぽい」という体験 → 30 日完走率 ↑

### 5. ¥980 / 年 の Lifetime プラン（ライト）
1 着買えるほどではないが続けたい層のためのライトプラン。
- Stripe サブスク連携
- `you_users.subscription_until` を埋めて active 判定に組み込む
- 既存 helper `you_user_active` は trial / lifetime / subscription_until のどれかが OK なら true

### 6. デザイン本人による「MU Atelier」入札
気に入った自分のデザインを **公開オークション** に投げる。落札があったら本人 + MU で売上シェア。
- `/you` の design に「公開」スイッチ
- `/atelier` ページで全公開デザインの常時オークション
- 既存 MA エンジン（`/api/bid`）を reuse、収益分配ロジックを上に追加

### 7. 物理 NFC タグ → スマホで Day N が見える
Tee の襟裏に NFC sticker。タップすると `wearmu.com/u/<slug>` のシェアページに飛ぶ。
- 既に share page と slug があるので URL 設計は変えない、Printful 側の同梱だけ

---

## P2 — 来月（ブランド体験の深掘り）

### 8. 月次 Atelier レポート（紙 / PDF）
- その月のあなたの 30 案 + skip / like 履歴 + 当月の弟子屈気象 + Printful 注文履歴 を 1 枚 PDF
- メールで配信。年末に冊子化オプション ¥3,000

### 9. クローズドな「同じ Tee 仲間」マップ
同じ MUGEN/MUON ドロップを所有している人を **匿名で** 表示する地図。
- DB: `you_users.region_hint`（任意入力 or IP→都道府県）
- /community ページで雲のような分布図

### 10. 「服を返す」（リサイクル / 第二オーナー）
- 着なくなった MU Tee を MU に返送 → クリーニング → 中古オークションに出す → 売上は元オーナー 70% / MU 30%
- Soulbound NFT 移転トランザクションで所有権を二次オーナーへ

### 11. AI ボイスで Bio を更新
スマホで `/you` ページから録音 → Whisper で文字起こし → `bio` を毎月勝手に更新
- 既存の音声哲学（「速く、ノイズなく」）と整合する
- 月初に「今月のあなた」を 30 秒で吹き込む UX

### 12. Apple Wallet パス
所有 NFT の証明書を Apple Wallet に入れる。
- `passkit-builder` で動的に発行、`/api/you/wallet/:design_id` で .pkpass を返す

---

## P3 — 半年〜1 年（スケール / 別事業化）

### 13. 法人プラン（チーム T シャツ AI）
- 会社 Slack に bot → 毎週月曜「今週のチーム T」を提案 → 全員で投票 → 当選者のサイズで一斉発送
- 単価 ¥1,500 / 人 / 月。MU の B2B 入口

### 14. オフライン弟子屈ストア
- 弟子屈町 SOLUNA の物件と兼用で「MU の物理ショールーム」
- 来訪者は店内で `/you` 登録 → その場で生成 → DTG プリンター（Brother GTX） で 30 分仕立て

### 15. データ販売（弟子屈気象 → ファッション統計）
- 「気温 N 度のとき、よく Skip されたカラー」みたいな匿名集計データを Vogue Business / WGSN に売る
- 1 レポート ¥500K

### 16. 多言語化（英語 / 中国語）
- ターゲット: 海外の AI ファッション層（NYC / Tokyo / Seoul）
- /you の Gemini プロンプトは英語の方が安定するので、UI 多言語 + プロンプト語切替

---

## 自動アップ運用ルール（このリポジトリで回す）

毎週月曜日に Claude へこのファイルを渡して：
- どれが完了済みか（git log と突き合わせ）
- どの P0 / P1 がまだ未着手か
- 「次の 1 機能を実装してデプロイして」と頼むだけで進む

GitHub Actions の deploy.yml は既に push → Fly auto deploy になっているので、merge した瞬間に本番反映。

---

## 計測指標（毎週見る数字）

| メトリクス | 取得元 | 目標 |
|---|---|---|
| /you 登録数 | `SELECT COUNT(*) FROM you_users` | +20/week |
| 7日継続率 | `you_designs.day_num >= 7` | 50% |
| トライアル → MUGEN 転換 | `mu_purchases JOIN you_users` | 5% |
| Lifetime free 比率 | `WHERE lifetime_free=1 / total` | 15%+ |
| 平均 Skip 率 | `you_feedback action='skip' / total` | <30%（好み合致の代理指標） |
| Atelier 公開率（P1 #6 後） | 公開済 design / total | 8% |
