# wearmu / MU Google Ads — ROAS Tune Applied

**適用日時**: 2026-05-21 19:18 JST
**Customer ID**: `9591303572` (MU / wearmu.com)
**Script**: `scripts/apply_negative_kw.py`
**Source draft**: `ads/ROAS_TUNE_DRAFT_20260521.md`
**API**: Google Ads API v22, auth via `~/.config/google-ads/google-ads.yaml`

## Scope (実際に触ったもの)

- 全 ENABLED campaign × 5 件の preventive negative kw を campaign-level で追加
- stray `Campaign #1` (PAUSED) の全 positive キーワードを REMOVE

## NOT touched (draft で言及されていたが今回は意図的に保留)

- max_cpc の引き上げ (¥80-120 → ¥150-250) — JP apparel auction floor 対策
- campaign budget 変更
- ad copy / ad / ad group 構造
- campaign status (Campaign #1 は PAUSED のまま、ARCHIVE もしていない)
- conversion tracking 確認 (`flyctl secrets list -a mu-store | grep GADS`)

これらは spend-gate 観点でユーザー承認の別 work に切り分け。

---

## 1. Negative keyword 追加結果

ハードコード negative リスト (5 件):

| kw | match |
|---|---|
| `カメラ` | BROAD |
| `中古 Tシャツ` | PHRASE |
| `古着` | BROAD |
| `テンプレート` | BROAD |
| `素材` | BROAD |

対象 campaign は `campaign.status = ENABLED` で filter (`PAUSED`/`REMOVED` 除外)。
結果 = **5 campaign × 5 kw = 25 操作期待 / 19 件 added / 6 件 skip (既存重複)** / 0 件 error

| Campaign | added | skipped (重複) |
|---|---|---|
| MU-AdsTees-Search | 3 | 2 (古着, 素材 既存) |
| MU-Brand | 3 | 2 (古着, 素材 既存) |
| MU-Discovery | 3 | 2 (古着, 素材 既存) |
| MU-PMax | 5 | 0 |
| MU-CRAFT-OneClick-2026-05 | 5 | 0 |
| **合計** | **19** | **6** |

skip 6 件は `scripts/google_ads_setup.py` の `NEGATIVE_KEYWORDS` 定数に
もともと「古着」「素材」が入っていて、setup 時に 3 campaign 分は反映済みだったため。
PMax と CRAFT-OneClick は別ルートで作られたので最初から全 5 件追加された。

検証 (適用直後の再 GAQL select):

```
MU-AdsTees-Search:        5/5 negatives present ✅
MU-Brand:                 5/5 negatives present ✅
MU-Discovery:             5/5 negatives present ✅
MU-PMax:                  5/5 negatives present ✅
MU-CRAFT-OneClick-2026-05: 5/5 negatives present ✅
```

---

## 2. Stray `Campaign #1` cleanup

- resource_name: `customers/9591303572/campaigns/23647601278`
- status: `PAUSED` (変更せず維持)
- ad_group: `広告グループ 1` (1 件のみ)
- 削除前 positive keyword 数: **25 件** (draft md は「12 件」と書いてあったが、
  実 API 結果は 25 件。draft 観測時から keyword recommendation 系の auto-apply
  で増えていた可能性。実数を正として 25 件全部削除)

REMOVE 内訳 (25 件、 全 BROAD、 ad_group `広告グループ 1`):

| # | kw |
|---|---|
| 1 | カメラ 小型 |
| 2 | カメラ wifi |
| 3 | カメラ 防犯 |
| 4 | カメラ セキュリティ |
| 5 | 店舗 カメラ |
| 6 | セキュリティ 防犯 |
| 7 | lan カメラ |
| 8 | 防犯 おすすめ |
| 9 | カメラ クラウド |
| 10 | カメラ 防犯 小型 |
| 11 | カメラ 解析 |
| 12 | 店舗 防犯 |
| 13 | カメラ 録画 |
| 14 | 防犯 店舗 |
| 15 | 店舗 カメラ 監視 |
| 16 | カメラ ai |
| 17 | dahua カメラ |
| 18 | ai 防犯 |
| 19 | 防犯 ai |
| 20 | 店 カメラ |
| 21 | hikvision ai |
| 22 | tplink カメラ |
| 23 | 受付 カメラ |
| 24 | 監視 カメラ 不審 者 |
| 25 | 個人 店 防犯 |

全 25 件で REMOVE 成功 (error 0)。

検証 (再 select):

```
stray Campaign #1 positive keywords remaining: 0 ✅
```

これで Campaign #1 が将来うっかり ENABLE されても、 「カメラ 防犯」系で
配信される事故は構造的に発生不能。 念のため上記 5 件 preventive negative の
うち `カメラ` BROAD も他 5 campaign に入っているので、 仮に Campaign #1 から
budget が他 campaign に共有された場合も blast radius 抑制済み。

---

## 3. 再現コマンド

```bash
cd /Users/yuki/workspace/mu-brand
python3 scripts/apply_negative_kw.py --dry-run   # 計画表示のみ
python3 scripts/apply_negative_kw.py             # 実適用 (idempotent)
```

スクリプトは idempotent:
- 同じ negative kw が既に存在する campaign では再実行時 `skip (already present)` になる
- Campaign #1 の positive KW を 既に全削除した状態で再実行すると 0 件 remove
- ENABLED → PAUSED に切り替わった campaign は次回 negative 投入対象から外れる

## 4. 次に手をつけるべき (今回は意図的に保留)

draft md §6 の残タスク:

- **Step 2 (Bid 引き上げ)**: JP apparel auction floor (~¥150-300) に対し
  現状 max_cpc が ¥80-120。 これが impression 0 の主因仮説 (H1)。
  ユーザー承認後に別 work で実行。
- **Step 3 (Conversion tracking 確認)**: `flyctl secrets list -a mu-store`
  で `GADS_CONVERSION_ID` / `GADS_PURCHASE_LABEL` の有無を確認。
- **Step 4 (配信開始後 24h 後の search_term_view 起点 negative 追加)**:
  今回入れた 5 件はあくまで予防。 実配信が始まったら `search_term_view`
  を見ながら本物の汚染クエリを切る。

---

## 5. 安全側の確認

- `campaign.status` は触っていない (Campaign #1 は PAUSED のまま、 他 5 件は ENABLED のまま)
- `campaign_budget` は触っていない
- `ad_group.cpc_bid_micros` は触っていない
- `ad_group_ad` (広告クリエイティブ) は触っていない
- スクリプト内で `campaign.status != 'PAUSED'` を ENABLED 判定に使ったが、
  実際は `campaign.status == 'ENABLED'` で厳密 filter (REMOVED 系も除外)
- stray cleanup は `campaign.status == 'PAUSED'` を確認してから REMOVE する
  二重 safety check 付き (ENABLED 状態の campaign から positive KW を
  抜く事故を構造的に防止)
