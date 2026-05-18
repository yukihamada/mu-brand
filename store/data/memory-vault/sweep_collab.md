---
name: MU × SIIIEEP (SWEEP) collab status
description: 北参道 BJJ アパレル SIIIEEP との MU collab。13 商品実販売 + 4 商品 SIIIEEP社 契約待ち
type: project
originSessionId: 1ce5a54b-0fdf-4f54-9bf4-90a31df46c16
---
**ブランド**: SIIIEEP™ (公式表記、別名 SWEEP)
**所在**: 北参道 (Shibuya, Tokyo)
**公式サイト**: shop.sweep.love
**ロゴ R2 配置**: `lifestyle.wearmu.com/sweep/_logo.png` (3000×474 PNG transparent)
> [line redacted]

**23 商品が今日から実際に Stripe Live で買える** (Printful 全自動 fulfill, 7-14日発送):

第一弾 13 (検証済 2026-05-11):
- BJJ 3品 (all-over print): rashguard-ls (pid 301), fight-shorts (pid 332), spats (pid 189)
- ライフスタイル 10品: hoodie 146, tee 71, tee-classic 71 (別仕様), longsleeve 356,
  sweatpants 898, cap 206, beanie 519, tote 641, socks-3pack 502, windbreaker 661

第二弾 10 (追加 2026-05-11, 検証済):
- DTG 3: tank-top 248, zip-hoodie 692, crewneck 318 (Champion)
- 刺繍 1: snapback 99 (Yupoong 6089M)
- サブリメーション/雑貨 3: mug 300 (¥2,800), bottle 382 (¥7,800), stickers 505 (¥2,400)
- 全面プリント 3: duffle 465 (¥22,800), gym-bag 594 (¥18,800), cotton-shorts 1481

**4 商品は SIIIEEP社 と本契約完了まで非表示** (active=0, Printful カタログ外):
gi-classic, belt-promo, bjj-tape, mouthguard

**自動承認**: 環境変数 `PRINTFUL_AUTO_CONFIRM` で制御。
- 未設定 / "true" / "1" → confirm=true (即生産・配送)
- "false" / "0" / "kill" → draft (手動承認)
- 現在: 検証中 = "false" stage. 本番運用時に unset または "true" に。

**マージン (全 23 商品)**: 28-76% (hoodie 76% 最大、socks/stickers/cap 28-35% 最小)
全面プリントバッグ系 (duffle/gym-bag) は印刷コスト高なので価格高め設定 (¥18-23K)。

**Why**: 濱田は柔術青帯で SIIIEEP の道場 (北参道) に通っており、コラボ提案中。
SIIIEEP社サインオフ前のため password gate。承認後に gate 解除予定。

**How to apply**:
- 価格・コピー・variant_id を変える時は `store/src/main.rs` の `sweep_items` 配列を編集
- DB の `collab_products` テーブル + 4 列で構成:
    - `printful_variant_map` (JSON: size → variant_id)
    - `printful_files` (JSON: `[{type,url}]`)
    - `printful_options` (JSON: `[{id,value}]`)
    - `printful_variant_id` (M default fallback)
- webhook handler `handle_collab_sweep_order` (store/src/main.rs):
    - size を `printful_variant_map` から引く
    - `printful_files` と `printful_options` を Printful order に直接渡す
    - `external_id` は **必ず 32 char 以内 truncate** (Stripe Live は 86 char で Printful 上限超過)
    - `jp_prefecture_to_iso()` で "Tokyo"→"JP-13" 等の都道府県変換
- E2E webhook テスト (2026-05-11 検証済):
    - DTG (tee), 全面プリント (rashguard), 刺繍 (cap/windbreaker) の 3 種で実 Printful draft 作成成功
    - 合成 Stripe webhook (HMAC-SHA256 with STRIPE_WEBHOOK_SECRET) で全 13 商品 fulfill OK
- 印刷ファイル: 刺繍/DTG は SIIIEEP ロゴ (`lifestyle.wearmu.com/sweep/_logo.png`、3000×474 PNG transparent)、全面プリントはライフスタイル写真
- 画像生成スクリプト: `mu-brand/sweep_images.py` (要 .env GEMINI_API_KEY)
- 信号 / FB ループ: `/api/sweep/signal`, `/api/sweep/signals`, `/api/admin/sweep_signals`
- マージン: 全 13 商品で 28-76% (hoodie 最高 76%, socks 最低 28%)