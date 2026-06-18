# FUNNEL OBSERVATION — wearmu.com

**取得日時**: 2026-05-21 20:25 JST
**ソース**: `GET https://wearmu.com/admin/funnel` (admin_funnel_page, main.rs L1926)
**補助**: `/api/admin/funnel/clicks`, `/api/admin/funnel/top_paths`
**期間**: 直近 1d / 7d / 30d
**取得モード**: read-only, prod (live SQLite `funnel_events` テーブル集計)

---

## 1. 全体集計 (event count / uniq visitor)

| step | 1d (events / uniq) | 7d (events / uniq) | 30d (events / uniq) |
|---|---:|---:|---:|
| 1. PAGEVIEW       | 28 / 20 | 472 / 208 | 745 / 363 |
| 2. CTA_CLICK      | 1 / 1   | 45 / 17   | 89 / 22   |
| 3. CHECKOUT_START | 0 / 0   | 5 / 5     | 7 / 7     |
| 4. CHECKOUT_PAID  | 10 / 10 | 58 / 58   | 73 / 73   |
| **TOTAL CVR (paid/pv)** | 35.7% | 12.3% | 9.8% |

> ※ `checkout_paid` > `checkout_start` の逆転は server-side webhook 計測 (main.rs L16540) が funnel cookie 無視で直接書き込むため。 ボットや旧クライアント由来の paid 計上が混じる可能性が高い。 **「step3 → step4 の上昇」 は計測不整合であって、 リアルな conversion path ではない。**

---

## 2. step-over-step 変換率 (visitor-based, 7d primary)

| transition | 7d uniq | conv | drop |
|---|---|---:|---:|
| pageview → cta_click       | 208 → 17 | **8.2%** | **-91.8%** |
| cta_click → checkout_start | 17 → 5   | **29.4%** | -70.6% |
| checkout_start → checkout_paid | 5 → 58 | (計測ずれ) | n/a |

| transition (30d) | uniq | conv | drop |
|---|---|---:|---:|
| pageview → cta_click       | 363 → 22 | **6.1%** | **-93.9%** |
| cta_click → checkout_start | 22 → 7   | **31.8%** | -68.2% |

**event ベース** で見ると 7d は pv 472 → cta 45 = 9.5%、cta 45 → start 5 = **11.1%**。 ここが本当の崖。

---

## 3. 一番大きい drop step

### 🔴 #1: pageview → cta_click (90–94% drop)

- 7d: 208 uniq pv → 17 uniq cta = **8.2% click-through**
- 30d: 363 → 22 = **6.1%**
- 目安 (健康 5–15%) のギリ下限。 **「ぎり健康」 だが top_paths を見ると問題が見える**。

#### top_paths (7d pageview) のシグナル
| path | pv | uv |
|---|---:|---:|
| /you | 180 | 145 |
| / | 124 | 54 |
| /ma | 42 | 10 |
| /buy | 27 | 9 |
| /products/ma/2 | 17 | 5 |
| /kokon/proposal | 7 | 7 |

- **/you が 145 uniq で最大流入**。 referrer は `(direct) 186`, `syndicatedsearch.goog 70`, `Google 62` — つまり **Google Ads トラフィックの 90% は /you に着地**。
- 一方 cta_click 内訳 (`hero_buy_now 11uv, modal_size_M 5uv, buy_page_card 1uv …`) は **buy / product ページ起点**。 /you には CTA イベントが ほぼ計測されていない。
- → **/you LP に Buy CTA が無い、 もしくは funnel_track イベントが発火していない**。 145 人が来て CTA を 1 回も押していないなら、 「LP が cul-de-sac」 になっている可能性が極めて高い。

### 🟡 #2: cta_click → checkout_start (68–88% drop, event-base 88.9%)

- 17 uniq が CTA を押したのに、 5 uniq しか checkout session 作成まで到達していない。
- click_by_cta を見ると `hero_buy_now 16 clicks / 11 uniq` が dominant。 ここ押した 11 人のうち何人が start に行ったか不明だが、 上記 5 を考えると **half 以上脱落**。
- 仮説: hero_buy_now → modal_size 選択フローで「サイズ」「種類 (tee/longsleeve/hoodie)」 が必須 → 選択せず諦め (`modal_size_M 5`, `modal_size_L 1`, `modal_size_XL 1` = 7 件のみ)。

### ⚪ #3: checkout_start → checkout_paid

**計測不整合** のため判定不能。 paid が start を超えている。 server-only webhook 計上が funnel_events に重複混入している。 修正候補: `funnel_track_server("checkout_paid", …)` 側で `visitor_id` が空 / null の event を別 channel に分ける。

---

## 4. 修正案候補 (3–5)

優先順位は **#1 (pageview → cta_click)** が圧倒的に大きい。

| # | 仮説 | 推奨修正 | 期待効果 |
|---|---|---|---|
| A | /you LP に Buy CTA が視認できない、 または funnel_track 未発火 | /you に hero_buy_now ボタン (現状 / と同じ) を明示配置 + DevTools で `funnel_track('cta_click', 'you_hero_buy')` 発火確認 | pv→cta 8% → 15%+ (=2x) |
| B | /you 流入の意図と LP コピーがミスマッチ (Google Ads keyword と LP テキストの一貫性欠落) | /you の hero 1st-view コピーを Ads 広告文と完全一致させる (`wearmu_you_search_2026-05.md` 参照) | pv→cta +30-50% |
| C | hero_buy_now → modal の摩擦 (サイズ/タイプ選択強制) | modal 初期値を `size=M, type=tee` に prefill、 「そのまま購入」ボタンを最上段 | cta→start 30% → 50%+ |
| D | top_paths に proposal 系 (kokon/jiuflow) や個別 product が散らばる → 分母希釈 | /products/* に共通の Sticky CTA bar を表示 (現状あるか要確認) | pv→cta +20% |
| E | checkout_paid 計測不整合 | `funnel_track_server` の paid 側に visitor_id 必須化、 webhook event は別 table (`checkout_paid_webhook`) に隔離 | 数字が信用できるようになる (修正必要だが今回は read-only タスク外) |

---

## 5. アクションサマリ

**最優先**: **/you LP の Buy CTA 監査** (修正案 A + B)。
理由: 7d で /you 流入 145 uniq、 全 cta_click 17 uniq のうち /you 起点と特定できる click が CTA 内訳から見えない。 もし /you に CTA が無い・壊れている なら、 Google Ads ¥42K の loss は **「クリックは来てるが LP の出口が無い」 構造問題**。

次点: **modal サイズ選択フロー** (修正案 C)。 cta→start 11.1% は健康下限 (30-60%) の 1/3。

最終確認: **paid 計測不整合** は数値の信頼性を毀損している。 read-only タスクでは指摘のみに留める。

---

## 6. 一次データ補足

- 7d referrer 上位: `(direct) 186`, `syndicatedsearch.goog 70` (= Google Ads), `Google 62`, `wearmu.com 22`, `checkout.stripe.com 20`
- 7d top CTA: hero_buy_now (16 click / 11 uv), modal_size_M (5/5), buy_page_card (5/1)
- `clicks_by_product` は空 = 商品レベルの CTA 計測は機能していない可能性 (要追跡)
