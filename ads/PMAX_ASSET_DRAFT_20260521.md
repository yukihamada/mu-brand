# MU-PMax — Asset Strength Analysis & Reinforcement Draft

**作成日**: 2026-05-21
**Customer ID**: `9591303572` (MU / wearmu.com)
**Campaign**: `MU-PMax` (id=`23858152693`, ENABLED, MAXIMIZE_CONVERSIONS, budget ¥1,700/日)
**Asset Group**: `MU-PMax-group-1` (id=`6713500500`, ENABLED, final_url=`https://wearmu.com/buy`)
**Ad Strength**: **POOR**
**ステータス**: 提案のみ。Ads API への mutate は一切なし。**承認後に別 work で反映**する。
**前 draft**: `ads/ROAS_TUNE_DRAFT_20260521.md` (event/profession=POOR、PMax は未深掘り)

> 関連 memory:
> - `mu_protocol_v2.md` — universal autonomous brand protocol、5/18 ship
> - `mu_profit_split_28.md` — 利益の 50% を弟子屈町に寄付
> - `wearmu_100_challenge.md` — 14日100枚チャレンジ (5/18-5/31)、deadline narrative
> - `wearmu_suzuri_mirror.md` — JP ¥4,900 / 海外 ¥7,800 dual channel
> - `product_philosophy.md` — 「速く、ノイズなく」

---

## 0. TL;DR (3 行)

1. **MU-PMax-group-1 の ad_strength = POOR**。原因は asset 数の絶対不足。全 field type で Google 推奨数を大幅に下回っている (HEADLINE 4/15, DESCRIPTION 4/5, IMAGE 2/20, VIDEO 0/5)。
2. policy reject は **ゼロ**。承認待ち / 制限付き承認なし。純粋に「タマが足りない」だけ。
3. 即対応: text asset (HEADLINE+11 / LONG_HEADLINE+3 / DESCRIPTION+1) と画像 (+18) を追加投入すれば AVERAGE 以上に上がる見込み。**動画 5 本は別 work で生成**。

---

## 1. 現状 inventory (asset_group_asset GAQL 結果)

GAQL: `SELECT asset_group_asset.field_type, asset.text_asset.text, asset.policy_summary.approval_status FROM asset_group_asset WHERE campaign.id = 23858152693 AND asset_group_asset.status != 'REMOVED'`

| Field Type | 実数 | Google 推奨 (max) | 過不足 | 状態 |
|---|---:|---:|---:|---|
| HEADLINE (≤30 字) | 4 | 15 | **−11** | 全件 UNSPECIFIED (= APPROVED に未昇格、ただし reject ではない) |
| LONG_HEADLINE (≤90 字) | 2 | 5 | **−3** | 同上 |
| DESCRIPTION (≤90 字) | 4 | 5 (1 件は ≤60 字必須) | **−1** | 同上 |
| BUSINESS_NAME | 1 | 1 | 0 | "MU" |
| MARKETING_IMAGE (1.91:1) | 1 | 20 | **−19** | host=tpc.googlesyndication.com (Google CDN にアップ済) |
| SQUARE_MARKETING_IMAGE (1:1) | 1 | 20 | **−19** | 同上 |
| PORTRAIT_MARKETING_IMAGE (4:5) | **0** | 任意 (推奨 ≥1) | **−1** | **欠落** |
| LOGO (1:1) | 1 | 5 | −4 | 1:1 ロゴあり |
| LANDSCAPE_LOGO (4:1) | **0** | 任意 (推奨 ≥1) | **−1** | **欠落** |
| YOUTUBE_VIDEO | **0** | 5 | **−5** | **完全欠落 → POOR の最大要因** |
| CALLOUT / SITELINK / PRICE / PROMO (extensions) | 0 | 任意 | — | 未投入 |

**asset_group_signal** (audience signals): **0 行** = 未投入。MU は new brand なので custom audience signal (柔術 / アート / DTC tee / Made-in-Japan apparel 等) を 3-5 件投入すべき。

### 現状 HEADLINE / LONG_HEADLINE / DESCRIPTION 全文

| Field | Text (生データ) |
|---|---|
| HEADLINE #1 | "MU — AI が描く Tシャツ" (15 字) |
| HEADLINE #2 | "利益の 50% を寄付" (11 字) |
| HEADLINE #3 | "世界に 1 着のデザイン" (12 字) |
| HEADLINE #4 | "弟子屈の気象から生成" (11 字) |
| LONG_HEADLINE #1 | "あなたの名前で世界に 1 着を作る、 AI 生成 Tシャツ" (28 字) |
| LONG_HEADLINE #2 | "AI が毎時間 1 着 Tシャツを描く DTC ブランド MU" (28 字) |
| DESCRIPTION #1 | "弟子屈の気温・月相を seed に AI 生成。 1 時間に 1 着、 1 サイクルで永久終了。" |
| DESCRIPTION #2 | "AI が北海道弟子屈の気象を読み毎時間 Tシャツを生成。 ¥6,800。" |
| DESCRIPTION #3 | "利益の 50% を弟子屈町に寄付。 原価ベース透明設計、 値引き無し。" |
| DESCRIPTION #4 | "1 of 1。 同じデザインは二度と作られません。 Printful EU 発送。" |

policy: 全件 reject なし。`UNSPECIFIED` は新規 asset が learning 中の通常状態。

---

## 2. 不足 asset サマリ (足りない順)

| 優先度 | 不足 | 件数 | 影響 |
|---|---|---:|---|
| ★★★ | YOUTUBE_VIDEO | +5 | PMax は動画 0 件だと自動生成動画が低品質で配信される (ad_strength=POOR の最大要因)。 |
| ★★★ | MARKETING_IMAGE (1.91:1) | +19 | PMax は YouTube/Discover/Gmail で 1.91:1 を多用。1 枚だと枯渇 → 配信抑制。 |
| ★★★ | SQUARE_MARKETING_IMAGE (1:1) | +19 | Discover/YouTube/Display で支配的なフォーマット。 |
| ★★ | HEADLINE | +11 | 4 件では機械学習が組合せ探索できない (15 推奨)。 |
| ★★ | asset_group_signal (audience) | +3-5 | new brand は signal なしだと learning が遅い。 |
| ★ | LONG_HEADLINE | +3 | 5 が推奨。3 件追加で満タン。 |
| ★ | PORTRAIT_MARKETING_IMAGE (4:5) | +1+ | Discover feed で表示優位。 |
| ★ | LANDSCAPE_LOGO (4:1) | +1 | 横長 logo は header banner で使われる。 |
| ★ | DESCRIPTION (≤60 字 短い枠) | +1 | 5 件目を埋める。 |
| △ | sitelink / callout / promotion extensions | 任意 | conv 計測あれば CTR boost。 |

---

## 3. 新規 HEADLINE 案 18 件 (各 ≤30 字、組合せ重視で重複語を抑制)

> ルール: HEADLINE は最大 30 字 (全角混在で正確には ピクセル数判定だが目安 30 字)。MU narrative を「AI / 時間 / 弟子屈 / 寄付 50% / 1 of 1 / ¥6,800 / 国内 ¥4,900 / Printful EU / 14日100枚」 の 9 軸に分散させる。

| # | Headline 案 | 軸 | 字数 |
|---:|---|---|---:|
| 1 | AI が毎時間 Tシャツを描く | AI / 時間 | 14 |
| 2 | 1 of 1、 二度と作られない | 希少性 | 13 |
| 3 | 世界に 1 着だけのデザイン | 希少性 | 13 |
| 4 | あなたの名前で生成する Tシャツ | パーソナライズ | 16 |
| 5 | 利益の 50% を寄付する Tシャツ | 寄付 | 16 |
| 6 | 弟子屈の気象を着る | narrative | 9 |
| 7 | 北海道発、 AI 生成 アパレル | 産地 | 14 |
| 8 | 1 時間で生まれて、 永遠に終わる | narrative | 16 |
| 9 | 国内発送 ¥4,900 / Printful 海外 | 価格 / 物流 | 19 |
| 10 | ¥6,800、 海外発送込み | 価格 | 11 |
| 11 | 値引きしない、 透明原価設計 | 価格哲学 | 14 |
| 12 | 在庫を持たない DTC ブランド | 業態 | 14 |
| 13 | 1 サイクル終了で永久終売 | 希少性 | 13 |
| 14 | 月相と気温から生成される | narrative | 12 |
| 15 | 100 着限定、 14 日チャレンジ | 100枚 | 14 |
| 16 | AI が運営する Tシャツ ブランド | 業態 | 16 |
| 17 | Made in Japan、 国内印刷 | 産地 | 13 |
| 18 | コミュニティに 10% 還元 | 分配 | 12 |

採用基準: HEADLINE #1-4 の既存と重複しない / 「無料」「公式」「最安」 など policy-risk な語は使わない / 数字 (50%, 1 of 1, ¥6,800, 14, 100) を冒頭に置いて CTR を稼ぐ。

### 新規 LONG_HEADLINE 案 5 件 (各 ≤90 字)

| # | Long Headline 案 | 字数 |
|---:|---|---:|
| 1 | 1 時間に 1 着、 1 サイクルで永久終了。 AI が描く一点物の Tシャツ ブランド MU。 | 41 |
| 2 | 弟子屈町の気温と月相を seed に AI が毎時間生成。 利益の 50% は地域に寄付。 | 39 |
| 3 | あなたの名前を入れた世界に 1 着だけの Tシャツを、 ¥6,800 で海外配送。 | 36 |
| 4 | 国内 ¥4,900 (SUZURI) / 海外 ¥7,800 (Printful EU)。 二重 fulfillment で世界へ。 | 47 |
| 5 | AI が運営する自律ブランド MU。 14 日で 100 着完売を目指す build-in-public チャレンジ実施中。 | 50 |

### 新規 DESCRIPTION 案 7 件 (各 ≤90 字、価格 / 製造 / 返品 / 送料 を盛り込み)

> Description は CTA トリガーになりやすい欄。**価格・送料・返品ポリシー** を必ず 1 件以上含める (PMax の信用シグナル)。

| # | Description 案 | 字数 |
|---:|---|---:|
| 1 | ¥4,900 から (国内発送) / ¥7,800 (海外発送込)。 7 日以内なら未着用に限り交換可能。 | 47 |
| 2 | AI が弟子屈町の気象から自動生成、 1 着 1 着が世界に 1 つ。 利益 50% を地域に寄付。 | 45 |
| 3 | Stanley/Stella SATU001 (GOTS organic 認証、 リブ襟)。 国内 2-3 日 / 海外 7-10 日。 | 47 |
| 4 | 値引きなし、 原価ベース透明設計。 §28 利益分配 (寄付 50% / コミュニティ 10%)。 | 43 |
| 5 | 1 of 1。 デザイン重複なし、 同じものは二度と生まれない。 1 サイクル終了で永久終売。 | 45 |
| 6 | 在庫を持たない受注生産。 注文後 3-5 日で印刷、 EU 工場から直接発送。 | 38 |
| 7 | 14 日 100 枚チャレンジ 実施中 (5/18-5/31)。 build-in-public、 進捗は wearmu.com/100 で公開。 | 53 |

採用基準: 1 件は **必ず価格 + 送料**、1 件は **製造・素材**、1 件は **返品ポリシー**、1 件は **narrative** (寄付 50%) を含む。

---

## 4. 画像案 (20 枚の内訳設計)

> 現状: 1.91:1 が 1 枚 / 1:1 が 1 枚 / portrait (4:5) が 0 枚。**1.91:1 を +19 / 1:1 を +19 / portrait を +5** が PMax 推奨上限。今回は **計 24 枚の制作リスト** を提案する。生成は別 work (Gemini 3 Pro Image)、Printful の既存 mockup と組み合わせて運用。

### 内訳

| カテゴリ | 枚数 | 1.91:1 | 1:1 | 4:5 | 内容 |
|---|---:|---:|---:|---:|---|
| **product / mockup** (Printful flat-lay + close-up) | 8 | 3 | 3 | 2 | MUGEN / SWEEP / kokon / JIU FIGHT / nakamura collab の Tシャツ展開写真。襟・タグ・縫製のクローズアップ込み。 |
| **lifestyle** (着用シーン) | 6 | 2 | 2 | 2 | 道場・カフェ・夜の街・自然 (弟子屈) の 4 シーン。モデル顔は控えめ (PII protection)。 |
| **brand narrative** (text+graphic 合成) | 4 | 2 | 2 | 0 | 「1 of 1」「Made in Japan」「利益 50% を寄付」「100 着チャレンジ」 の text overlay 4 種。 |
| **process** (生成過程) | 3 | 1 | 1 | 1 | 弟子屈の気象データ → AI seed → デザイン → 印刷 の 3 ステップ visual。 |
| **logo / mark** | 3 | 1 (4:1 landscape) | 1 (1:1) | 0 | MU 円形ロゴ / 漢字「無」 / landscape header banner。**LANDSCAPE_LOGO 不足の補填**。 |

合計 **24 枚** (内 portrait 4:5 が 5 枚 = 推奨 ≥1 を満たす)。

### 制作仕様

- 解像度: 1.91:1 = **1200×628 以上** / 1:1 = **1200×1200 以上** / 4:5 = **960×1200 以上** / 4:1 landscape = **1200×300 以上**
- ファイルサイズ: ≤5MB / 形式 JPG or PNG (透過不要)
- text overlay は **画像面積の 20% 以下** (Google PMax は text-heavy 画像を低評価)
- 既存 mockup は `mu-brand/store/static/designs/` + `mu-brand/static_craft/` に存在 → 流用候補を別 work で監査

### 動画案 (YOUTUBE_VIDEO +5、別 work 必須)

PMax は動画 0 件 → Google が prompt-generated な低品質動画を自動作る = ad_strength POOR の最大要因。 5 本案:

| # | 内容 | 尺 | 制作元 |
|---:|---|---:|---|
| 1 | AI が Tシャツを描く 1 時間のタイムラプス | 30s | `mu-brand/scripts/gen_*.py` の出力を録画 |
| 2 | 弟子屈の風景 → MU Tシャツ着用 transition | 15s | 既存 SOLUNA 撮影素材 + Printful flat-lay |
| 3 | 「利益 50% を寄付」 narrative + テキスト | 15s | After Effects 或いは Veo3 で生成 |
| 4 | 100 着チャレンジ progress bar live | 6s | wearmu.com/100 のスクリーンキャスト |
| 5 | Founder Yuki の voice over (15s)「無の哲学」 | 15s | iPhone 撮影 + JP/EN 字幕 |

最低 1 本 (Vertical 9:16 or Square 1:1) は **無音 + 字幕対応** で自動再生環境に最適化する。

---

## 5. asset_group_signal (audience) 投入案 3-5 件

new brand は signal 0 だと learning が遅延 → 以下を **CUSTOM_AUDIENCE / INTEREST** で追加:

1. **柔術 / BJJ / Tシャツ** (custom audience: 検索クエリ + URL signal、 jiuflow.com / sjjjf.com を input)
2. **DTC apparel** (interest: "Designer apparel", "Direct-to-consumer fashion")
3. **AI 生成アート** (custom audience: 検索 "Midjourney", "AI art", "generative design")
4. **Made in Japan / 工芸品** (interest: "Japanese craftsmanship", "Slow fashion")
5. **寄付 / エシカル消費** (interest: "Ethical fashion", "Sustainable apparel")

signal は **完璧でなくていい** (Google は signal を出発点として広げる)。3 件以上あれば learning は加速する。

---

## 6. 適用方法 (承認後の別 work)

### Step 1 — text asset 一括 add (HEADLINE 11 + LONG_HEADLINE 3 + DESCRIPTION 3)

```python
# scripts/google_ads_pmax_add_text_assets.py (新規)
# AssetService.mutate_assets で TEXT asset を create
# → AssetGroupAssetService.mutate_asset_group_assets で field_type=HEADLINE
#    / LONG_HEADLINE / DESCRIPTION 指定で asset_group=6713500500 に link
# 既存 4 件と重複しないよう text を normalize して dedupe
```

API パターン (擬似コード):

```python
# 1) asset 本体を create
op = client.get_type("AssetOperation")
op.create.text_asset.text = "AI が毎時間 Tシャツを描く"
asset_resp = asset_service.mutate_assets(customer_id=CUSTOMER_ID, operations=[op])
asset_rn = asset_resp.results[0].resource_name

# 2) asset_group に link
op2 = client.get_type("AssetGroupAssetOperation")
op2.create.asset_group = "customers/9591303572/assetGroups/6713500500"
op2.create.asset = asset_rn
op2.create.field_type = client.enums.AssetFieldTypeEnum.HEADLINE
asset_group_asset_service.mutate_asset_group_assets(
    customer_id=CUSTOMER_ID, operations=[op2]
)
```

### Step 2 — 画像 24 枚 upload (別 work)

```python
# scripts/google_ads_pmax_upload_images.py (新規)
# AssetOperation で ImageAsset (data=base64) を create
# → AssetGroupAsset で field_type ∈ {MARKETING_IMAGE, SQUARE_MARKETING_IMAGE,
#    PORTRAIT_MARKETING_IMAGE, LANDSCAPE_LOGO} を指定
# 画像は mu-brand/store/static/designs/ + mu-brand/static_craft/ から選択
# ファイルサイズ / 寸法 / アスペクト比 を upload 前に PIL で検証
```

> **画像生成は別 work**。本 draft は upload 対象の選定基準とリストのみ。Gemini 3 Pro Image での生成は別 task で実行。既存 Printful mockup の流用 + 既存 SOLUNA 撮影素材の活用を優先する (生成コスト ¥0)。

### Step 3 — 動画 5 本生成 (別 work)

- YouTube に upload (unlisted で可) → YouTubeVideoAsset で link
- 1 本目だけ先に投入して残り 4 本は順次 (PMax の learning は 1 本でも改善する)

### Step 4 — asset_group_signal 投入

```python
# scripts/google_ads_pmax_signals.py (新規)
# AssetGroupSignalService.mutate_asset_group_signals で
# audience= ... を attach。Custom audience は事前に CustomAudienceService で create 必要
```

### Step 5 — ad_strength 再確認

```python
svc.search(customer_id="9591303572", query="""
  SELECT asset_group.id, asset_group.name, asset_group.ad_strength
  FROM asset_group WHERE campaign.id = 23858152693
""")
# ad_strength が POOR → AVERAGE → GOOD に上がっていることを確認
```

---

## 7. 注意 / risk

- **policy reject は現時点でゼロ**。新規 asset 投入で reject が出る可能性は低いが、 「最安」「公式」「保証」 など強い断定語は避ける。 上記 18 件の HEADLINE 案 はすべて policy-safe な文言で構成済み。
- **既存 4 HEADLINE / 4 DESCRIPTION は触らない** (state UNSPECIFIED = まだ learning 中 + パフォーマンスデータが付くのを待つ段階)。 「上書き / 削除」 は別判断。 今回は **追加のみ**。
- **動画 0 件の状態で PMax を回し続けると** Google が自動生成動画を作って配信する → ブランド毀損リスク。 動画 1 本だけでも先に投入する。
- **conversion tracking 未確認** (前 draft で指摘済)。 PMax は conversion-based learning なので tracking が無いと asset 追加効果が出にくい。 別 work で確認必須。
- **MU-CRAFT-OneClick (MANUAL_CPC) と TARGET_SPEND 系 (MU-AdsTees / MU-Brand / MU-Discovery) には今回触らない**。 別 agent 担当。
- **mutate コール一切なし**。本 draft の作成中は GAQL `search` のみ。

---

## 8. 数字の出典 (再現コマンド)

```python
# scripts/apply_negative_kw.py と同じ auth pattern
from google.ads.googleads.client import GoogleAdsClient
client = GoogleAdsClient.load_from_storage(YAML, version="v22")
svc = client.get_service("GoogleAdsService")

# 1) PMax campaign
svc.search(customer_id="9591303572", query="""
  SELECT campaign.id, campaign.name, campaign.status,
         campaign.advertising_channel_type, campaign.bidding_strategy_type
  FROM campaign WHERE campaign.advertising_channel_type = 'PERFORMANCE_MAX'
""")

# 2) asset_group + ad_strength
svc.search(customer_id="9591303572", query="""
  SELECT asset_group.id, asset_group.name, asset_group.ad_strength,
         asset_group.final_urls FROM asset_group
  WHERE campaign.id = 23858152693
""")

# 3) asset_group_asset (全 asset 列挙)
svc.search(customer_id="9591303572", query="""
  SELECT asset_group_asset.field_type, asset_group_asset.status,
         asset.id, asset.type, asset.text_asset.text,
         asset.image_asset.full_size.url,
         asset.youtube_video_asset.youtube_video_id,
         asset.policy_summary.approval_status
  FROM asset_group_asset
  WHERE campaign.id = 23858152693 AND asset_group_asset.status != 'REMOVED'
""")

# 4) asset_group_signal
svc.search(customer_id="9591303572", query="""
  SELECT asset_group_signal.asset_group, asset_group_signal.audience.audience
  FROM asset_group_signal WHERE campaign.id = 23858152693
""")
```

---

## 9. 結論

- ✅ **mutate コール 0 件**。GAQL `search` のみ。
- 🔥 **ad_strength=POOR の主因**: text/image/video の絶対数不足 (HEADLINE 4/15, IMAGE 2/40+, VIDEO 0/5)。
- 📋 **次のアクション**: ユーザー承認後 → §6 Step 1 (text asset 17 件 add) → Step 2 (画像 24 枚 upload) → Step 3 (動画 5 本) → Step 4 (audience signal 3-5 件) → Step 5 (ad_strength 再確認)。
- ⚠ **動画は CAPEX**。 既存素材 (SOLUNA 撮影 + Printful mockup + scripts/gen_*.py のタイムラプス) を最大流用すれば外注ゼロで 5 本確保可能。

> **JiuFlow ¥42K/0 conv 教訓**: 配信開始だけで満足せず、 ad_strength が AVERAGE 以上に上がってから budget を増やす。 PMax は asset 数 × 多様性 × 動画 の 3 軸で品質が決まる。 「配信させる → asset を増やす → conv を計測する」 の順番を守る。
