# CHANGELOG — RASH 2 SKU print-files 修理 (2026-06-11)

## 事象

`MU-ZEN-03-RASH` / `MU-MU-01-RASH` の購入が Printful 投稿で
`Item 0: Item can't be submitted without any print files` (400) になり、
全件自動返金されていた (catalog_orders #16 = 2026-06-04, #24 = 2026-06-05,
いずれも status='refunded'・購入者は本人)。

## 根本原因

シード行が `printful_sync_product_id` だけ持ち、
`printful_sync_variant_id` と `design_file` が両方 NULL だったため、
`build_printful_item()` が shape (c)(variant_id のみ・files なし)に落ちていた。
Printful 側の sync product 自体は無傷で、print file は `status: ok` で存在する
(2026-06-11 に `GET /store/products/{434211687,434212898}` で実 API 確認):

| SKU | sync_product_id | sync_variant_id | variant |
|---|---|---|---|
| MU-ZEN-03-RASH | 434211687 | 5317891114 | White / M (9328) |
| MU-MU-01-RASH | 434212898 | 5317914251 | White / M (9328) |

## 対応 (本番データ変更)

起動時 one-shot migration `migrate_rash_sync_variants()`
(`store/src/catalog.rs`、呼び出しは `main.rs` の migration 列) が上記 2 行の
`printful_sync_variant_id` を埋める。以後 fulfillment は shape (a)
(sync_variant_id) になり、Printful 保管済み print file で発注される。
冪等 (NULL/0 の行のみ更新)・他 SKU 非接触。

サイズ補足: nouns 以外の checkout は Stripe size custom-field を付けないため、
この 2 SKU は従来から M 単一サイズ販売。sync variant (White/M) と完全一致し
挙動変化なし。
