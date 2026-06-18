-- ─────────────────────────────────────────────────────────────────────
--  MU × ELE × POTE — 2匹のグッズ (2026-05-28)
--
--  Yuki の personal pets — Ele (ビションプー / Bichon-Poodle mix) と
--  Pote (フレンチブルドッグ・ブルー&タン). Vibe: ふわふわ × ふがふが、
--  2匹の毎日。 Friendly modern editorial illustration mascots, generated
--  via gemini-3-pro-image-preview with the actual dog photos as references.
--
--  All designs are MU-original art (Yuki's own pets) → status=live, fully
--  fulfillable. Pricing matches /yuma /atsume tee+hoodie+mug+tote+sticker.
--
--  Printful product/variant/placement (all WHITE base, verified vs live API):
--    71/4012  front_large  Bella+Canvas 3001 White M    (tee     ¥4,800)
--    146/5523 front        Gildan 18500 White M         (hoodie  ¥9,800)
--    19/1320  default      White Glossy Mug 11oz        (mug     ¥3,800)
--    641/16289 front       AS Colour 1001 White         (tote    ¥3,800)
--    358/10164 default     Kiss-Cut Sticker 4×4 White   (sticker ¥800)
-- ─────────────────────────────────────────────────────────────────────

INSERT INTO catalog_brands
  (slug, name, emoji, color_primary, tagline, custom_domain, is_active, revenue_share_pct, config_json)
VALUES
  ('elepote', 'MU × ELE × POTE', '🤍', '#C97D6B',
   'ふわふわ × ふがふが · 2匹の毎日',
   NULL, 1, 0,
   '{"lp_template":"/elepote","design_style":"Friendly modern editorial illustration of two best-friend puppy mascots. Confident monoline + soft flat fills + one warm accent. Cozy, premium.","lifestyle_scene":"warm sunlit living room, cream sofa, two dogs napping","ink_default":"navy","partner":"personal · Yuki''s dogs","hero":{"title":"ELE × POTE","subtitle":"BY MU · 2匹の毎日","tagline_en":"FLUFF MEETS SQUISH","tagline_en_accent":"FLUFF","tagline_jp":"ふわふわ × ふがふが。","badge":"ELE × POTE · 2026"},"product_blurbs":{"tee":"Bella+Canvas 3001 · 4.2oz リングスパンコットン・ホワイト · 在庫レス DTG · 7–14日配送"}}')
ON CONFLICT(slug) DO UPDATE SET
  name              = excluded.name,
  emoji             = excluded.emoji,
  color_primary     = excluded.color_primary,
  tagline           = excluded.tagline,
  is_active         = excluded.is_active,
  revenue_share_pct = excluded.revenue_share_pct,
  config_json       = excluded.config_json;

INSERT OR IGNORE INTO catalog_products
  (sku, brand, label, description_ja, retail_price_jpy,
   printful_product_id, printful_variant_id, printful_placement,
   printful_print_w, printful_print_h,
   printful_sync_product_id, printful_sync_variant_id,
   stripe_product_id, stripe_price_id,
   design_file, mockup_main_file, mockup_url_external,
   suzuri_url, is_active, sort_order, status, fulfillment_route)
VALUES
  ('ELEPOTE-TEE-DUO', 'elepote', 'MU × ELE × POTE Tee',
   '#01 · DUO HERO — ふわふわ × ふがふが · White',
   4800, 71, 4012, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/elepote/d/design_ELEPOTE-DUO.png',
   '/static/elepote/preview/duo_tee.png',
   'https://wearmu.com/static/elepote/preview/duo_tee.png',
   NULL, 1, 1, 'live', 'printful_dtg'),
  ('ELEPOTE-TEE-ELE', 'elepote', 'MU × ELE Tee',
   '#02 · ELE (ビションプー) · White',
   4800, 71, 4012, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/elepote/d/design_ELEPOTE-ELE.png',
   '/static/elepote/preview/ele_tee.png',
   'https://wearmu.com/static/elepote/preview/ele_tee.png',
   NULL, 1, 2, 'live', 'printful_dtg'),
  ('ELEPOTE-TEE-POTE', 'elepote', 'MU × POTE Tee',
   '#03 · POTE (フレンチブルドッグ) · White',
   4800, 71, 4012, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/elepote/d/design_ELEPOTE-POTE.png',
   '/static/elepote/preview/pote_tee.png',
   'https://wearmu.com/static/elepote/preview/pote_tee.png',
   NULL, 1, 3, 'live', 'printful_dtg'),
  ('ELEPOTE-HOODIE-DUO', 'elepote', 'MU × ELE × POTE Hoodie',
   '#04 · DUO HOODIE — Gildan 18500 White',
   9800, 146, 5523, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/elepote/d/design_ELEPOTE-DUO.png',
   '/static/elepote/preview/duo_hoodie.png',
   'https://wearmu.com/static/elepote/preview/duo_hoodie.png',
   NULL, 1, 4, 'live', 'printful_dtg'),
  ('ELEPOTE-MUG-DUO', 'elepote', 'MU × ELE × POTE Mug',
   '#05 · DUO MUG — 白マグ 11oz',
   3800, 19, 1320, 'default', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/elepote/d/design_ELEPOTE-DUO.png',
   '/static/elepote/preview/duo_char.png',
   'https://wearmu.com/static/elepote/preview/duo_char.png',
   NULL, 1, 5, 'live', 'printful_dtg'),
  ('ELEPOTE-TOTE-DUO', 'elepote', 'MU × ELE × POTE Tote',
   '#06 · DUO TOTE — AS Colour 1001 White',
   3800, 641, 16289, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/elepote/d/design_ELEPOTE-DUO.png',
   '/static/elepote/preview/duo_char.png',
   'https://wearmu.com/static/elepote/preview/duo_char.png',
   NULL, 1, 6, 'live', 'printful_dtg'),
  ('ELEPOTE-STICKER-DUO', 'elepote', 'ELE × POTE Sticker',
   '#07 · DUO STICKER — 4×4 キスカット',
   800, 358, 10164, 'default', 0, 0, NULL, NULL, NULL, NULL,
   '/static/elepote/d/design_ELEPOTE-DUO.png',
   '/static/elepote/preview/duo_char.png',
   'https://wearmu.com/static/elepote/preview/duo_char.png',
   NULL, 1, 7, 'live', 'printful_dtg'),
  ('ELEPOTE-STICKER-ELE', 'elepote', 'ELE Sticker',
   '#08 · ELE STICKER — 4×4 キスカット',
   800, 358, 10164, 'default', 0, 0, NULL, NULL, NULL, NULL,
   '/static/elepote/d/design_ELEPOTE-ELE.png',
   '/static/elepote/preview/ele_char.png',
   'https://wearmu.com/static/elepote/preview/ele_char.png',
   NULL, 1, 8, 'live', 'printful_dtg'),
  ('ELEPOTE-STICKER-POTE', 'elepote', 'POTE Sticker',
   '#09 · POTE STICKER — 4×4 キスカット',
   800, 358, 10164, 'default', 0, 0, NULL, NULL, NULL, NULL,
   '/static/elepote/d/design_ELEPOTE-POTE.png',
   '/static/elepote/preview/pote_char.png',
   'https://wearmu.com/static/elepote/preview/pote_char.png',
   NULL, 1, 9, 'live', 'printful_dtg');

-- ─── 2026-05-28 round 2: long-sleeve + crewneck + sleep design + solo mugs
-- 57/3449  front_large  Gildan 2400 White M       (long-sleeve)
-- 145/5427 front_large  Gildan 18000 White M      (crewneck)
-- New design "2匹が寝てる" applied to tee + mug.
-- Solo mugs: Ele only / Pote only.
INSERT OR IGNORE INTO catalog_products
  (sku, brand, label, description_ja, retail_price_jpy,
   printful_product_id, printful_variant_id, printful_placement,
   printful_print_w, printful_print_h,
   printful_sync_product_id, printful_sync_variant_id,
   stripe_product_id, stripe_price_id,
   design_file, mockup_main_file, mockup_url_external,
   suzuri_url, is_active, sort_order, status, fulfillment_route)
VALUES
  ('ELEPOTE-LONGSLEEVE-DUO', 'elepote', 'MU × ELE × POTE Long Sleeve',
   '#10 · DUO LONG SLEEVE — Gildan 2400 White',
   6800, 57, 3449, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/elepote/d/design_ELEPOTE-DUO.png',
   '/static/elepote/preview/duo_tee.png',
   'https://wearmu.com/static/elepote/preview/duo_tee.png',
   NULL, 1, 10, 'live', 'printful_dtg'),
  ('ELEPOTE-CREWNECK-DUO', 'elepote', 'MU × ELE × POTE Crewneck',
   '#11 · DUO CREWNECK — Gildan 18000 White',
   7800, 145, 5427, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/elepote/d/design_ELEPOTE-DUO.png',
   '/static/elepote/preview/duo_tee.png',
   'https://wearmu.com/static/elepote/preview/duo_tee.png',
   NULL, 1, 11, 'live', 'printful_dtg'),
  ('ELEPOTE-TEE-SLEEP', 'elepote', 'MU × ELE × POTE 寝てる Tee',
   '#12 · SLEEPING DUO TEE — 2匹で寝てる · White',
   4800, 71, 4012, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/elepote/d/design_ELEPOTE-SLEEP.png',
   '/static/elepote/preview/sleep_tee.png',
   'https://wearmu.com/static/elepote/preview/sleep_tee.png',
   NULL, 1, 12, 'live', 'printful_dtg'),
  ('ELEPOTE-MUG-SLEEP', 'elepote', 'MU × ELE × POTE 寝てる Mug',
   '#13 · SLEEPING DUO MUG — 朝のコーヒーに寝顔',
   3800, 19, 1320, 'default', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/elepote/d/design_ELEPOTE-SLEEP.png',
   '/static/elepote/preview/sleep_char.png',
   'https://wearmu.com/static/elepote/preview/sleep_char.png',
   NULL, 1, 13, 'live', 'printful_dtg'),
  ('ELEPOTE-MUG-ELE', 'elepote', 'ELE Mug',
   '#14 · ELE SOLO MUG — 白マグ 11oz',
   3800, 19, 1320, 'default', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/elepote/d/design_ELEPOTE-ELE.png',
   '/static/elepote/preview/ele_char.png',
   'https://wearmu.com/static/elepote/preview/ele_char.png',
   NULL, 1, 14, 'live', 'printful_dtg'),
  ('ELEPOTE-MUG-POTE', 'elepote', 'POTE Mug',
   '#15 · POTE SOLO MUG — 白マグ 11oz',
   3800, 19, 1320, 'default', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/elepote/d/design_ELEPOTE-POTE.png',
   '/static/elepote/preview/pote_char.png',
   'https://wearmu.com/static/elepote/preview/pote_char.png',
   NULL, 1, 15, 'live', 'printful_dtg');
