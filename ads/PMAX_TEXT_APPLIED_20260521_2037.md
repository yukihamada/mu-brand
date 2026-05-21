# MU-PMax — Text Asset Upload Result

**実行日時**: 2026-05-21 20:37 JST
**Customer ID**: `9591303572`
**Campaign**: `MU-PMax` (id=`23858152693`)
**Asset Group**: `MU-PMax-group-1` (id=`6713500500`)
**Script**: `scripts/apply_pmax_text_assets.py`
**Source draft**: `ads/PMAX_ASSET_DRAFT_20260521.md` §3

---

## TL;DR

- HEADLINE **+11**、 LONG_HEADLINE **+3**、 DESCRIPTION **+1** = 計 **15 件** の text asset を upload。
- 全 field type が Google 推奨上限に到達: HEADLINE 4→**15/15**, LONG 2→**5/5**, DESC 4→**5/5**。
- ad_strength: POOR → PENDING (反映に数分。後で再確認)。
- 画像 (+24枚) / 動画 (+5本) / audience signal は別 work。

---

## 1. 結果サマリ

| Field Type | Before | After | Cap | Added | Skipped | Errors (recovered) |
|---|---:|---:|---:|---:|---:|---:|
| HEADLINE | 4 | **15** | 15 | 11 | 7 (dup) + 5 (trim_cap) | 2 (再投入で解決) |
| LONG_HEADLINE | 2 | **5** | 5 | 3 | 0 (dup) + 2 (trim_cap) | 0 |
| DESCRIPTION | 4 | **5** | 5 | 1 | 0 (dup) + 6 (trim_cap) | 0 |
| **合計** | 10 | **25** | 25 | **15** | — | — |

---

## 2. 追加された asset (resource_name id 付き)

### HEADLINE (11 件)

| # | Text | Asset ID | 字数 |
|---:|---|---|---:|
| 1 | AI が毎時間 Tシャツを描く | `362903751443` | 15 |
| 2 | 1 of 1、 二度と作られない | `362986368481` | 16 |
| 3 | 世界に 1 着だけのデザイン | `363066156000` | 14 |
| 4 | あなたの名前で生成する Tシャツ | `363066159843` | 16 |
| 5 | 利益の 50% を寄付する Tシャツ | `363066156243` | 18 |
| 6 | 弟子屈の気象を着る | `362903799377` | 9 |
| 7 | 北海道発、 AI 生成 アパレル | `362986379959` | 16 |
| 8 | ¥6,800、 海外発送込み | `362986401664` | 14 |
| 9 | 値引きしない、 透明原価設計 | `362903842241` | 14 |
| 10 | 1 サイクル終了で永久終売 | `363066264237` | 13 |
| 11 | 月相と気温から生成される | `362986546630` | 12 |

### LONG_HEADLINE (3 件)

| # | Text | Asset ID | 字数 |
|---:|---|---|---:|
| 1 | 1 時間に 1 着、 1 サイクルで永久終了。 AI が描く一点物の Tシャツ ブランド MU。 | `362903844419` | 48 |
| 2 | 弟子屈町の気温と月相を seed に AI が毎時間生成。 利益の 50% は地域に寄付。 | `363066138705` | 45 |
| 3 | あなたの名前を入れた世界に 1 着だけの Tシャツを、 ¥6,800 で海外配送。 | `363066172419` | 41 |

### DESCRIPTION (1 件)

| # | Text | Asset ID | 字数 |
|---:|---|---|---:|
| 1 | ¥4,900 から (国内発送) / ¥7,800 (海外発送込)。 7 日以内なら未着用に限り交換可能。 | `363066188562` | 53 |

---

## 3. 投入されなかった候補と理由

### Cap で trim (HEADLINE 5 件 / LONG 2 件 / DESC 6 件)

cap 上限 (HEADLINE 15 / LONG 5 / DESC 5) に到達したため、 残り候補は今回未投入。 必要なら既存の弱い asset を入れ替える形で別 work で投入可能。

**HEADLINE trim** (5 件):
- 在庫を持たない DTC ブランド (16c)
- 100 着限定、 14 日チャレンジ (18c)
- AI が運営する Tシャツ ブランド (18c)
- Made in Japan、 国内印刷 (19c)
- コミュニティに 10% 還元 (14c)

**LONG_HEADLINE trim** (2 件):
- 国内 ¥4,900 (SUZURI) / 海外 ¥7,800 (Printful EU)。 二重 fulfillment で世界へ。 (66c)
- AI が運営する自律ブランド MU。 14 日で 100 着完売を目指す build-in-public チャレンジ実施中。 (62c)

**DESCRIPTION trim** (6 件):
- AI が弟子屈町の気象から自動生成、 1 着 1 着が世界に 1 つ。 利益 50% を地域に寄付。 (50c)
- Stanley/Stella SATU001 (GOTS organic 認証、 リブ襟)。 国内 2-3 日 / 海外 7-10 日。 (68c)
- 値引きなし、 原価ベース透明設計。 §28 利益分配 (寄付 50% / コミュニティ 10%)。 (49c)
- 1 of 1。 デザイン重複なし、 同じものは二度と生まれない。 1 サイクル終了で永久終売。 (47c)
- 在庫を持たない受注生産。 注文後 3-5 日で印刷、 EU 工場から直接発送。 (39c)
- 14 日 100 枚チャレンジ 実施中 (5/18-5/31)。 build-in-public、 進捗は wearmu.com/100 で公開。 (73c)

### 初回 API reject (recovered) — Google Ads HEADLINE pixel-width 制約

最初の実行で 2 件が `FaultMessage: Too long.` で reject:
- "1 時間で生まれて、 永遠に終わる" (codepoint 17c だが pixel 幅 NG)
- "国内発送 ¥4,900 / Printful 海外" (codepoint 25c、 ASCII/digit が多く実 pixel 幅が広い)

→ 同 draft の trim list から短い 2 件 (12-13c の純 CJK) で置き換え、 第2 pass で投入成功。

**Lesson**: HEADLINE の Google Ads 上限は **codepoint 30** だが実際は **pixel 幅 (約 ½×ASCII + 1×CJK)** で測られる。 ASCII / 数字 / 半角記号が多い文字列は codepoint 25 でも reject される。 今後は pure-CJK 短文を優先。

---

## 4. ad_strength 再 read

```
asset_group_id=6713500500
  before: POOR
  after:  PENDING (= Google が再評価中)
```

ad_strength は asset 投入後すぐには反映されない (通常数分〜数十分)。 後続 work で再 query して AVERAGE / GOOD に上がっているか確認すること。

GAQL:
```sql
SELECT asset_group.id, asset_group.name, asset_group.ad_strength
FROM asset_group
WHERE asset_group.id = 6713500500
```

---

## 5. 残タスク (本 work では実施せず)

| 項目 | 件数 | 次の work |
|---|---:|---|
| MARKETING_IMAGE (1.91:1) | +19 | scripts/google_ads_pmax_upload_images.py を新設、 Gemini 3 Pro Image 生成 + Printful mockup 流用 |
| SQUARE_MARKETING_IMAGE (1:1) | +19 | 同上 |
| PORTRAIT_MARKETING_IMAGE (4:5) | +1〜5 | 同上 |
| LANDSCAPE_LOGO (4:1) | +1 | logo 別生成 |
| YOUTUBE_VIDEO | +5 | 動画 5 本制作 → YouTube 投稿 → YouTubeVideoAsset link |
| asset_group_signal (audience) | +3〜5 | scripts/google_ads_pmax_signals.py |
| HEADLINE / LONG / DESC の追加 trim 案 | 13 件 | 既存 asset の performance を見て弱い順から入れ替え |

---

## 6. 再現コマンド

```bash
# 状態確認のみ
python3 scripts/apply_pmax_text_assets.py --dry-run

# 適用 (idempotent: 既存 text とは dedup される)
python3 scripts/apply_pmax_text_assets.py
```

最終 GAQL チェック:
```sql
SELECT asset_group_asset.field_type,
       count(asset.id)
FROM asset_group_asset
WHERE asset_group_asset.asset_group = 'customers/9591303572/assetGroups/6713500500'
  AND asset_group_asset.status != 'REMOVED'
```

期待値: HEADLINE=15, LONG_HEADLINE=5, DESCRIPTION=5, BUSINESS_NAME=1, LOGO=1, MARKETING_IMAGE=1, SQUARE_MARKETING_IMAGE=1。
