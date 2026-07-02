# JiuFlow → MU(BJJ) ブリッジ お知らせ 下書き

**目的**: JiuFlow の実 BJJ トラフィック(2,733PV/7d)を MU の BJJ コレクションへ流す。
**投稿先**: JiuFlow のお知らせ / ニュース（`mcp__jiuflow__create_news` or `post_announcement`）。
**リンク先**: `https://wearmu.com/bjj?ref=jiuflow`（`/bjj` は本番 200・TAP/NAP 等 BJJ商品在庫あり・`?ref=` 帰属は MU 側実装済）。
**🔴 投稿は人間ゲート**（JiuFlow ユーザーに可視 = 他者向け）。優貴さんの GO 後に MCP で投稿。

---

## 多言語（JiuFlow は 日/英/葡）

### 日本語
**タイトル**: 🥋 MU × BJJ — 練習の相棒に、一着。

道場でも、外でも。JiuFlow と同じ「一本」の美意識でつくった BJJ アパレル
コレクションができました。TAP / NAP の柔術デザイン、AIが一点ずつ生成。
在庫リスクゼロのオンデマンド製造なので、売り切れる前の今が一番安い。

👉 見てみる: https://wearmu.com/bjj?ref=jiuflow

### English
**Title**: 🥋 MU × BJJ — one shirt for the mat and the street.

Same "one clean finish" aesthetic as JiuFlow, now in apparel. A BJJ
collection (TAP / NAP designs) where each piece is AI-generated,
made on demand — zero inventory, cheapest before it sells out.

👉 See it: https://wearmu.com/bjj?ref=jiuflow

### Português
**Título**: 🥋 MU × BJJ — uma camisa para o tatame e para a rua.

A mesma estética "um acabamento limpo" do JiuFlow, agora em roupas.
Uma coleção BJJ (designs TAP / NAP), cada peça gerada por IA e feita
sob demanda — estoque zero, mais barata antes de esgotar.

👉 Veja: https://wearmu.com/bjj?ref=jiuflow

---

## 投稿後の計測
- `curl ".../api/admin/funnel/ab?token=$MU_ADMIN_TOKEN&path=/bjj&days=7"` で /bjj への流入回復を確認。
- MU 側 `?ref=jiuflow` 帰属で JiuFlow 由来の buy CTA / Stripe CV を追跡。
- 反応が出たら レバー2（YouTube 短尺）へ展開。
