---
name: wearmu-suzuri-mirror
description: "wearmu.com の SUZURI marketplace 二重 fulfillment 経路 (Constitution §24-v2)。JP→SUZURI ¥4,900、海外→Printful ¥7,800"
metadata: 
  node_type: memory
  type: project
  originSessionId: 705e7c8b-754e-4a04-8d13-32fe62d8e27a
---

MU は dual-channel fulfillment を採用 (Constitution §24-v2):

- **国内 (JP)**: suzuri.jp marketplace ミラー、¥4,900 開始、2-3 日 国内発送
- **海外 / collectors**: wearmu.com Stripe → Printful EU、¥7,800、リブ襟 Stanley/Stella SATU001

**Why:** Toru persona (港区 30 歳 Visvim 層) の指摘で「Visvim Jumbo の 5 年勝負に負ける」→ Stanley/Stella SATU001 (GOTS organic + リブ襟) に切替。しかし Printful EU 経由は ¥5,700 原価 → JP 客には送料が重く UX 劣化。SUZURI なら国内印刷で速く、 wearmu.com の narrative + SUZURI の物流を分離できる。

**How to apply:**
- 新 MUGEN drop が `products` に挿入されたら `POST /api/admin/suzuri/publish/:pid?token=…` で suzuri.jp にミラー
- 商品ページ (`/products/...`) の PDP に `🇯🇵 SUZURI で買う` CTA がレンダー (`p.suzuri_url` が set されたとき)
- SUZURI は **fulfillment-only API は提供しない** (`/api/v1/orders`, `/checkout`, `/trippo` すべて 404)。creator marketplace モデルのみ
- Direct fulfillment API が欲しい場合は オリジナルプリント.jp に B2B 問合せ済 ([email redacted] + [email redacted]、2026-05-13 送信、返事待ち)

**SUZURI API 落とし穴:**
- `POST /api/v1/materials` は **JSON only** (multipart は `Content-Type must be application/json` で 400)
- design は base64 で `texture: "data:image/png;base64,..."` フィールドに埋め込む
- `itemId 148` = ヘビーウェイトTシャツ (5.6oz Printstar)、creator margin は `price` に JPY で指定
- レスポンスは `{ material: {id}, products: [{id, url}] }`。`url` には `{size}` と `{color}` placeholder が含まれる
- Token は `SUZURI_ACCESS_TOKEN` env (Fly secret に staged 済)

**Migration columns:** `products.suzuri_material_id`, `products.suzuri_product_id`, `products.suzuri_url`

[[mu_collab_b2b]] / [[sweep_collab]] と同じく partner-channel の例