# wearmu / MU Google Ads — ROAS Tune Draft

**作成日**: 2026-05-21
**Customer ID**: `9591303572` (MU / wearmu.com)
**期間**: LAST_30_DAYS (read-only via Google Ads API v22)
**ステータス**: 提案のみ。Ads API への mutate は一切なし。**ユーザー承認後に別 work で反映**する。

> 関連 memory:
> - `google_ads_jiuflow.md` — JiuFlow MCC 1532515844 (別 customer)
> - `jiuflow_ads_cvr_findings.md` — JiuFlow ¥42K / 0 conv 事件 → 同じ轍は踏まない

---

## 0. TL;DR (3 行)

1. **全 6 キャンペーンが LAST_30_DAYS で impr=0 / clk=0 / cost=¥0 / conv=0**。費用ゼロなので「高 spend / 低 conv な query」は **存在しない**。
2. 真の問題は「**配信が走っていない**」こと。最大 CPC が ¥80-120 と JP apparel 検索オークション底値 (~¥150-300) を割っており、入札落札不能 + キーワード個別 bid が ¥0 のため keyword override も効いていない。
3. **stray キャンペーン**: `Campaign #1` (PAUSED だが ¥7,257/日 budget が紐付き、「カメラ 防犯」など MU と無関係な BROAD KW 12 件)。完全削除 or budget detach 推奨。

---

## 1. 集計サマリ (LAST_30_DAYS, LAST_7_DAYS 同値)

| Campaign | Status | Serving | Bid Strategy | 1日予算 | impr | clk | cost | conv | CTR | CPC |
|---|---|---|---|---|---|---|---|---|---|---|
| Campaign #1 (camera) | PAUSED | SERVING | — | ¥7,257 | 0 | 0 | ¥0 | 0 | 0% | ¥0 |
| MU-AdsTees-Search | ENABLED | SERVING | TARGET_SPEND | ¥1,500 | 0 | 0 | ¥0 | 0 | 0% | ¥0 |
| MU-Brand | ENABLED | SERVING | TARGET_SPEND | ¥300 | 0 | 0 | ¥0 | 0 | 0% | ¥0 |
| MU-Discovery | ENABLED | SERVING | TARGET_SPEND | ¥1,500 | 0 | 0 | ¥0 | 0 | 0% | ¥0 |
| MU-PMax | ENABLED | SERVING | MAXIMIZE_CONV | ¥1,700 | 0 | 0 | ¥0 | 0 | 0% | ¥0 |
| MU-CRAFT-OneClick-2026-05 | ENABLED | SERVING | MANUAL_CPC | ¥1,000 | 0 | 0 | ¥0 | 0 | 0% | ¥0 |
| **合計** | — | — | — | **¥13,257** | **0** | **0** | **¥0** | **0** | — | — |

- 広告承認: ほぼ全件 `APPROVED/REVIEWED` (一部 `APPROVED_LIMITED` あり — `MU-AdsTees-Search/profession`, `MU-CRAFT-OneClick/AI Tシャツ ワンクリック`)
- Ad strength: `event` / `profession` が **POOR**。他は AVERAGE。GOOD が一つもない。
- `search_term_view` LAST_30_DAYS = **0 行** (配信されていないので search term データが存在しない)

---

## 2. 推奨 negative keyword list

**現時点で実トラフィックがゼロのため、検索クエリ起点の negative kw 推奨は出せない**。これは「現状ベースの保留」であり、配信が走り出してから初めて意味を持つ。

ただし、既存の手書き negative リスト (`ads/wearmu_you_search_2026-05.md` + `scripts/google_ads_setup_ads_tees.py` の 21 件) には 配信開始前に **以下を追加投入** することを推奨:

| 追加 negative kw | match | 理由 |
|---|---|---|
| `カメラ` | BROAD | `Campaign #1` の汚染防止 (別ジャンル混在) |
| `防犯 カメラ` | PHRASE | 同上 |
| `セキュリティ` | BROAD | 同上 |
| `中古 Tシャツ` | PHRASE | 古着ユーザー除外 (apparel 業界共通) |
| `古着` | BROAD | 同上 |
| `無料` | BROAD | LP 比較サイト・素材サイト除外 |
| `素材` | BROAD | 「フリー素材」「Tシャツ素材」検索者除外 |
| `卸` / `卸売` | BROAD | B2B 仕入れ業者除外 (重複なら skip OK) |
| `テンプレート` | BROAD | Tシャツデザインテンプレ探し除外 |
| `アプリ` | BROAD | 「Tシャツ デザイン アプリ」 (制作ツール) 除外 |
| `自作` | BROAD | DIY 系除外 |

これらは **list として持っておき、配信が回り出してから search_term_view で確認しながら追加**するのが正しい順序。

---

## 3. 停止 / 低 bid 推奨 (現状は逆)

費用ゼロ → 停止しても何も節約できない。**逆に「配信開始のための bid 引き上げ」が必要**。

### 3-A. 完全 stop 候補

| Campaign / Ad group | 理由 | 推奨アクション |
|---|---|---|
| `Campaign #1` / `広告グループ 1` | MU と無関係 (カメラ 防犯系 12 BROAD kw)、PAUSED だが budget ¥7,257/日 が紐付き | **REMOVE** (campaign + budget detach)。budget だけ別 campaign に再利用 OK |
| `MU-AdsTees-Search` / `event` | ad strength=POOR、 KW 1 件のみ (`父の日 プレゼント Tシャツ`、季節終了) | 父の日 (6/15) 後に削除 or 同 ad group に Father's Day 後の event KW 追加 |

### 3-B. Bid 引き上げ (= 配信を始めるため)

| Ad group | 現在 max_cpc | 推奨 max_cpc | 根拠 |
|---|---|---|---|
| `MU-Brand/brand_defense` | ¥80 | ¥200 | ブランド名 (`wearmu`, `MUGEN apparel`) は competitor 入札もあり 80 円では負ける |
| `MU-Discovery/*` (全 7) | ¥120 | ¥250 | JP apparel 検索の floor は ¥150-300。120 円では auction floor 割れ |
| `MU-AdsTees-Search/regional` | ¥80 | ¥200 | 「弟子屈 Tシャツ」「三田 Tシャツ」のロングテール狙いでも 80 は薄い |
| `MU-AdsTees-Search/jujitsu` | ¥100 | ¥200 | 「柔術 Tシャツ」は JiuFlow も入札している競合枠 |
| `MU-AdsTees-Search/profession` | ¥100 | ¥180 | エンジニア・ナース系は CPC 高め |
| `MU-AdsTees-Search/kokon` | ¥100 | ¥150 | ニッチなので 150 で十分 |
| `MU-CRAFT-OneClick/AI Tシャツ ワンクリック` | ¥100 | ¥200 | EXACT 中心だが「AI Tシャツ」は new entrant 多く競争激化 |

### 3-C. 個別キーワードの ¥0 個別 bid 問題

`keyword_view` で確認したところ、ほぼ全 KW の `ad_group_criterion.cpc_bid_micros = 0` (= ad_group default を使う) になっている。これ自体は問題なし。ただし `MU-Brand/brand_defense` の `wearmu`, `MUGEN apparel` だけ ¥50 で明示 override されており、これは ad_group default (¥80) より低いため **無効化** すべき (¥0 にして default に従わせる、または ¥250 に引き上げ)。

---

## 4. 強化 keyword (conv > 0 のもの)

**該当なし**。conv が 0 件 (配信ゼロ) なので「強化すべき winner KW」が存在しない。

ただし提案として、配信開始後 7-14 日で見るべき keyword 優先順位:

1. `MU-Brand/brand_defense` の `wearmu` / `MUGEN apparel` (ブランド指名検索 = 最高 CVR)
2. `MU-Discovery/ai_tshirt` の `AI Tシャツ 生成` (new market、 SEO competitor 少ない)
3. `MU-AdsTees-Search/jujitsu` の `柔術 Tシャツ` / `黒帯 Tシャツ` (JiuFlow ユーザーの cross-sell 見込み)

---

## 5. 仮説 — なぜ全キャンペーンが impression 0 か

3 つの可能性 (重なってる可能性大):

| # | 仮説 | 検証方法 | 対処 |
|---|---|---|---|
| H1 | **入札が低すぎてオークション勝てない** | ad_group bid ¥80-120 vs 日本 apparel 検索 CPC 平均 ¥150-300 | 全 ad_group max_cpc を ¥200-250 に引き上げ (上表 3-B) |
| H2 | **TARGET_SPEND が学習中** で吐き出さない | TARGET_SPEND は通常 24-48h で立ち上がる。 既に 5 日経過なので学習問題ではない | MANUAL_CPC に switch、明示的に CPC を出させる |
| H3 | **ad strength POOR / 広告制限** | `event` / `profession` で POOR、 2 件 `APPROVED_LIMITED` | headlines/descriptions を 15/4 まで埋める、 LP の content 改善 |

**最有力**: H1 + H2 の combo。`MU-CRAFT-OneClick` だけ MANUAL_CPC ¥100 にしてあるが ¥100 でも floor 割れしている → JP apparel auction の現実を再認識した上で予算 ¥1000 のまま max_cpc を引き上げる。

副次仮説 (H4): **コンバージョン計測タグが未設定**な可能性 → MAXIMIZE_CONVERSIONS (PMax) や TARGET_SPEND が「コンバージョンデータが無いから機械学習が出稿を絞っている」可能性。`/api/tracking/config` の GADS_CONVERSION_ID / GADS_PURCHASE_LABEL が Fly secrets に入っているか別途確認すべき。

---

## 6. 適用方法 (ユーザー承認後の手順)

mutate は **明示承認後の別 work** で実行。以下は提案手順:

### Step 1 — Camera campaign 切り離し
```python
# scripts/google_ads_cleanup_stale.py (新規) — Campaign #1 を REMOVED 化
# (campaign id を確認してから campaign_service.mutate で status=REMOVED)
```

### Step 2 — Bid 引き上げ
```python
# scripts/google_ads_bid_raise.py (新規) — 上表 3-B の max_cpc を反映
# ad_group_service.mutate, operations[].update_mask = ['cpc_bid_micros']
# 既存 cv_tune_ads.py の hysteresis ロジックを流用可
```

### Step 3 — Conversion tracking 確認
```bash
flyctl secrets list -a mu-store | grep -E "GADS|GA4"
# 値が空なら ADS_TEES_RUNBOOK.md §5 手順で再投入
```

### Step 4 — Negative kw 追加投入 (配信開始 24h 後)
- 配信が動き出して **search_term_view に行が出てから** §2 の追加 negative kw を流し込む
- 既存 21 件 + 新規 11 件 = 32 件
- `scripts/google_ads_setup_ads_tees.py` の `NEGATIVE_KEYWORDS` 定数を編集 + 再実行 (idempotent なので duplicate skip される)

### Step 5 — 7 日後の振り返り
- `scripts/ads_monitor_loop.py` のループは既に動いている (logs/ads_launch_*/) — そのまま継続
- 7 日経って **impr > 0 になっても conv = 0** ならば **JiuFlow と同じ事象** → 即 pause + LP 改善優先

---

## 7. 数字の出典 (再現コマンド)

read-only GAQL (今回叩いたもの):

```python
# scripts/ads_monitor_loop.py と同じ認証 (~/.config/google-ads/google-ads.yaml)
from google.ads.googleads.client import GoogleAdsClient
client = GoogleAdsClient.load_from_storage(YAML, version="v22")
svc = client.get_service("GoogleAdsService")

# 1. campaign LAST_30_DAYS
svc.search(customer_id="9591303572", query="""
  SELECT campaign.name, campaign.status, campaign.bidding_strategy_type,
         campaign_budget.amount_micros, metrics.impressions, metrics.clicks,
         metrics.cost_micros, metrics.conversions
  FROM campaign WHERE segments.date DURING LAST_30_DAYS
""")

# 2. ad_group max_cpc
svc.search(customer_id="9591303572", query="""
  SELECT campaign.name, ad_group.name, ad_group.cpc_bid_micros
  FROM ad_group WHERE ad_group.status = 'ENABLED'
""")

# 3. search_term_view (empty for now)
svc.search(customer_id="9591303572", query="""
  SELECT search_term_view.search_term, metrics.impressions, metrics.clicks, metrics.cost_micros
  FROM search_term_view WHERE segments.date DURING LAST_30_DAYS
""")
```

---

## 8. 結論

- ✅ **mutate 系コール一切なし** (このドラフト作成中も含めて GAQL search のみ)
- ⚠ **「高 spend / 低 conv な query」 ⇒ 該当データなし** (全 ¥0)
- 🔥 **真の問題**: max_cpc が JP apparel auction floor を下回っており、 配信が走らない
- 🔥 **stray camera campaign が budget ¥7,257/日 を握ったまま** — PAUSED でも将来再 enable のリスクあり
- 📋 **次のアクション**: ユーザー承認後 → §6 Step 1-3 を順に別 work で実行

> **JiuFlow 失敗事例との対比**: JiuFlow は ¥42K 消化して 0 conv だった (= 配信は走ったが LP UX 問題)。wearmu は **¥0 消化して 0 conv** で、 まだ配信前段階で詰まっている。 解くべき問題のレイヤーが違う。

