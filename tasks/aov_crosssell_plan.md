# MU AOV クロスセル — 複数SKU発注 実装計画 (2026-05-30)

ゴール: 「あと¥800でステッカーも追加」等のクロスセルで AOV ¥1,914→向上。
**前提ブロッカー**: 現状 `fulfill_catalog_order` は単一SKUのみ発注。2品目を
Stripe に足すと課金されるが発注されず＝顧客被害。先に複数SKU発注を実装する。

## 関連コード (実読確認済み)
- `store/src/catalog.rs:2877` `shop_checkout` — Stripe session 作成、line_items[0] のみ、
  metadata に `catalog_sku`。success_url は `…&value={price}&sid=…`(本セッションで修正済)。
- `store/src/catalog.rs:3011` `fulfill_catalog_order` — `metadata.catalog_sku` を1つ読み、
  `catalog_products` から Printful id を引き、単一 `item` を組み `items:[item]` で発注 POST。
- `store/src/catalog.rs:93` `catalog_orders` スキーマ — sku 1列。
- 既存実績: ITTO checkout (`main.rs:4690〜`) が line_items[0]+[1] の複数 line_item を1
  セッションで実装済 → Stripe 側パターンはこれを踏襲。

## 実装ステップ (各段階で検証、最後に有料テスト発注)
1. **checkout (catalog.rs:2877)**: `?addon=<sku>` 受付。あれば line_items[1] に addon の
   price_data 追加 + metadata `catalog_addon_sku` 追加 + success_url value を合算に。
2. **fulfillment (catalog.rs:3011)**: `catalog_addon_sku` があれば addon の Printful id を引き
   2つ目 `item` を組み `items` に push (本体の (a)sync/(b)base+design 分岐を関数化し再利用)。
3. **記録**: `catalog_orders` に `addon_sku TEXT NULL` を ALTER 追加 (後方互換)。
4. **UI (顧客可視・最後)**: `/shop/:sku` PDP の buy リンクに `&addon=<sticker_sku>` トグル +
   送料無料 ¥7,000 への「あと¥X」訴求。
5. **検証 (必須・有料)**: addon 付きテストセッション→**実テスト発注**で Printful 注文に2品目が
   乗り両方 fulfill されることを確認してから UI ON。UI は flag で OFF のまま 1-3 を deploy。

## 安全策
- UI ON 前に必ずテスト発注で2品目 fulfill を実証 (顧客被害ゼロ)。
- 段階デプロイ: backend(1-3) → テスト発注 → UI(4) ON。
- addon SKU はハードコード前に DB で実在 verify ([[feedback_verify_sku_before_hardcode]])。
- 既存単一SKU注文は addon 無しで完全後方互換。
