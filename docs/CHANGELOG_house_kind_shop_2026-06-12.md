# CHANGELOG — house kind を /shop で発見可能に + 熊牛SOLUNA 3商品投入 (2026-06-12)

## 事象

kind=`house` はバックエンド実装済 (PRODUCT_SPECS / fulfillment_route=`manual` /
PDP の is_house 分岐 / agent API whitelist) だったが、実質未公開だった:

1. `/shop` の種類チップ (`kind_defs`) に house が無く、ユーザーが発見できない。
2. `/shop?kind=house` を開いても絞り込み・チップ選択・検索フォームの
   hidden `name="kind"` のどれにも反映されない。
3. house kind の商品が 0 件。

## 根本原因 (2 の正体)

`shop_index()` (store/src/catalog.rs) の kind ホワイトリスト
`("tee" | "rashguard" | "hoodie" | "sticker" | "song")` に `house` が無く、
`?kind=house` が空文字に潰されていた。hidden input への「伝搬バグ」ではなく
入口の whitelist 漏れ (hidden input は whitelist 通過後の `kind` をそのまま
出すので、whitelist 修正だけで直る)。あわせて `shop_kind_sql()` にも house の
SQL アームが無かった。

## 対応 (コード)

`store/src/catalog.rs`:

- shop_index の kind whitelist に `"house"` を追加。
- `shop_kind_sql()` に `"house" => "(UPPER(sku) LIKE '%-HOUSE-%' OR UPPER(sku)
  LIKE '%-HOUSE')"` を追加 (`kind_from_sku` と同じダッシュ込みトークンで
  LIGHTHOUSE 等の誤マッチを防ぐ)。
- `kind_defs` を 5→6 要素にし `("house", "🏠 家")` / EN `("house", "🏠 Houses")`
  のチップを追加。

## 対応 (本番データ変更 — catalog_products 3 行 INSERT)

bim.house の熊牛SOLUNA製品ライン (本番 LIVE の実物件) をミラーする house 商品
3 点を、正規の agent API (`POST /api/agent/products` 経由 = MU MCP
`mu_create_product`) で store `bim-house` に投入。

**全件 status=`review` で着地** (即公開なし)。design_url が bim.house
(信頼ホスト外) のため `assess_product_risk()` の risk gate
"external image domain (untrusted host)" が発動し、AUTO_PUBLISH_OWNERS /
MA council のいかんに関わらず review 行きであることをコードで事前確認済み
(`agent_api.rs` の `if trusted && risk.is_none()` 分岐)。公開には MA council
承認が必要。

| SKU | label | price (設計相談デポジット) | bim.house 物件 | 建物概算 (実ページ 2026-06-12 取得) |
|---|---|---|---|---|
| BIMHOUSE-AGENT-HOUSE-6fb1bd43 | SOLUNA 熊牛 S｜ひとりの小屋 64㎡ | ¥50,000 | u-solunas64-ul3gy0b1 | ¥18,421,491 |
| BIMHOUSE-AGENT-HOUSE-18c4cd7b | SOLUNA 熊牛 M｜平屋リトリート 110㎡ | ¥50,000 | u-house-a9v04rgo | ¥50,953,980 |
| BIMHOUSE-AGENT-HOUSE-a910bc2f | SOLUNA 熊牛 L｜母屋＋サウナ離れ 156㎡ | ¥50,000 | u-solunal156-xzk0vcbq | ¥71,601,816 |

価格設計: checkout は商品の retail_price_jpy を全額課金するため、コード内の
法規ガード (MU が売るのは設計相談デポジットのみ・建物売買/仲介はしない —
PRODUCT_SPECS house の注記) に従い retail はデポジット ¥50,000 (= kind の
価格フロア)。建物概算は商品説明に「bim.house 見積・2026-06-12 時点」と明記。
M/L の概算はメモリ上の旧値 (¥25.8M/¥33.2M) から bim.house 側で再見積りされて
いたため、**実ページの価格バッジ「建物」行から取得した現行値**を採用。

画像: `design_url = https://bim.house/api/showcase/<slug>/image`
(3 件とも HTTP 200 確認済・物件 slug が URL から復元可能)。

作成者 (audit): agent `yuki@hamada.tokyo` / 経路 = MU MCP mu_create_product /
2026-06-12。取り消す場合は owner の `mu_retire_product` または MA review で
reject。
