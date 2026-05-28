-- ─────────────────────────────────────────────────────────────────────
--  MU × YUMA — 碧 (AO) LINE (2026-05-28)
--
--  Collab with 税理士 YUMA. Theme: お金・数字の話を、爽やかに。
--  「碧」= 澄んだ水と空の色 = 透明でクリーンな会計。水色 (Baby Blue) tee +
--  deep teal/navy 1色プリント。コピーは "税理士あるある"。
--
--  All 4 designs are MU-original art (generic accountant phrases + 碧 kanji,
--  no partner logo/IP) so they are fully fulfillable → status='live'.
--
--  Garment: Bella+Canvas 3001, Baby Blue, product 71 / variant 4037 (M)
--  — verified against the live Printful /products/71 API before seeding.
--  Pricing: ¥4,800 (DTG). Stripe IDs NULL → built by /api/shop/checkout.
--  revenue_share_pct = 0 → partner split TBD with YUMA, not invented here.
--
--  UPSERT brand row (hero-copy edits land on boot); INSERT OR IGNORE products.
-- ─────────────────────────────────────────────────────────────────────

INSERT INTO catalog_brands
  (slug, name, emoji, color_primary, tagline, custom_domain, is_active, revenue_share_pct, config_json)
VALUES
  ('yuma', 'MU × YUMA', '碧', '#0E7C9B',
   '数字に、誠実を。 · 税理士 YUMA 碧 (AO) line',
   NULL, 1, 0,
   '{"design_style":"Tax-accountant collab. Fresh aqua (水色) tee + deep teal/navy single-color print. 碧 kanji signature + witty 税理士 phrases. Clean, minimal, 爽やか.","lifestyle_scene":"bright minimal accounting office, clear blue light, plants, tidy desk","ink_default":"teal","partner":"税理士 YUMA","hero":{"title":"碧","subtitle":"MU × YUMA · 税理士 collab","tagline_en":"HONESTY IN NUMBERS","tagline_en_accent":"HONESTY","tagline_jp":"数字に、誠実を。","badge":"碧 (AO) LINE · 2026"},"product_blurbs":{"tee":"Bella+Canvas 3001 · 水色 (Baby Blue) · DTG プリント · 在庫レス · 7–14日配送"}}')
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
  ('YUMA-TEE-AO', 'yuma', 'MU × YUMA 碧',
   '#01 · HERO — 碧 + 青色申告は、正義。 · 水色',
   4800, 71, 4037, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/yuma/d/design_YUMA-TEE-AO.png',
   '/static/yuma/preview/hero_ao.png',
   'https://wearmu.com/static/yuma/preview/hero_ao.png',
   NULL, 1, 1, 'live', 'printful_dtg'),
  ('YUMA-TEE-KEIHI', 'yuma', 'MU × YUMA 経費',
   '#02 · それ、経費で落ちません。 · 水色',
   4800, 71, 4037, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/yuma/d/design_YUMA-TEE-KEIHI.png',
   '/static/yuma/preview/keihi.png',
   'https://wearmu.com/static/yuma/preview/keihi.png',
   NULL, 1, 2, 'live', 'printful_dtg'),
  ('YUMA-TEE-RYOSHU', 'yuma', 'MU × YUMA 領収書',
   '#03 · 領収書は、愛。 · 水色',
   4800, 71, 4037, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/yuma/d/design_YUMA-TEE-RYOSHU.png',
   '/static/yuma/preview/ryoshusho.png',
   'https://wearmu.com/static/yuma/preview/ryoshusho.png',
   NULL, 1, 3, 'live', 'printful_dtg'),
  ('YUMA-TEE-KAIHI', 'yuma', 'MU × YUMA 会費',
   '#04 · 税金は、未来への会費。 · 水色',
   4800, 71, 4037, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/yuma/d/design_YUMA-TEE-KAIHI.png',
   '/static/yuma/preview/kaihi.png',
   'https://wearmu.com/static/yuma/preview/kaihi.png',
   NULL, 1, 4, 'live', 'printful_dtg');

-- ─── 2026-05-28: 6 more 税理士 phrases (碧 line expansion) ─────────────
INSERT OR IGNORE INTO catalog_products
  (sku, brand, label, description_ja, retail_price_jpy,
   printful_product_id, printful_variant_id, printful_placement,
   printful_print_w, printful_print_h,
   printful_sync_product_id, printful_sync_variant_id,
   stripe_product_id, stripe_price_id,
   design_file, mockup_main_file, mockup_url_external,
   suzuri_url, is_active, sort_order, status, fulfillment_route)
VALUES
  ('YUMA-TEE-DATSUZEI', 'yuma', 'MU × YUMA 脱税',
   '#05 · 節税と脱税は、ちがう。 · 水色',
   4800, 71, 4037, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/yuma/d/design_YUMA-TEE-DATSUZEI.png',
   '/static/yuma/preview/datsuzei.png',
   'https://wearmu.com/static/yuma/preview/datsuzei.png',
   NULL, 1, 5, 'live', 'printful_dtg'),
  ('YUMA-TEE-DONBURI', 'yuma', 'MU × YUMA どんぶり',
   '#06 · どんぶり勘定、卒業。 · 水色',
   4800, 71, 4037, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/yuma/d/design_YUMA-TEE-DONBURI.png',
   '/static/yuma/preview/donburi.png',
   'https://wearmu.com/static/yuma/preview/donburi.png',
   NULL, 1, 6, 'live', 'printful_dtg'),
  ('YUMA-TEE-INVOICE', 'yuma', 'MU × YUMA インボイス',
   '#07 · インボイス、登録した? · 水色',
   4800, 71, 4037, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/yuma/d/design_YUMA-TEE-INVOICE.png',
   '/static/yuma/preview/invoice.png',
   'https://wearmu.com/static/yuma/preview/invoice.png',
   NULL, 1, 7, 'live', 'printful_dtg'),
  ('YUMA-TEE-GENKA', 'yuma', 'MU × YUMA 減価償却',
   '#08 · 減価償却は、人生。 · 水色',
   4800, 71, 4037, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/yuma/d/design_YUMA-TEE-GENKA.png',
   '/static/yuma/preview/genka.png',
   'https://wearmu.com/static/yuma/preview/genka.png',
   NULL, 1, 8, 'live', 'printful_dtg'),
  ('YUMA-TEE-CASH', 'yuma', 'MU × YUMA 現金',
   '#09 · 黒字より、現金。 · 水色',
   4800, 71, 4037, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/yuma/d/design_YUMA-TEE-CASH.png',
   '/static/yuma/preview/cash.png',
   'https://wearmu.com/static/yuma/preview/cash.png',
   NULL, 1, 9, 'live', 'printful_dtg'),
  ('YUMA-TEE-KIGEN', 'yuma', 'MU × YUMA 期限',
   '#10 · 期限は、待ってくれない。 · 水色',
   4800, 71, 4037, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/yuma/d/design_YUMA-TEE-KIGEN.png',
   '/static/yuma/preview/kigen.png',
   'https://wearmu.com/static/yuma/preview/kigen.png',
   NULL, 1, 10, 'live', 'printful_dtg');

-- ─── 2026-05-28: extend 碧 line beyond tees (hoodie / crewneck / mug /
-- tote). All reuse the hero design_YUMA-TEE-AO.png as the print. Printful
-- product/variant/placement IDs verified against the live API:
--   146/10842 Light Blue M (Gildan 18500 hoodie), placement 'front'
--   145/7861  Light Blue M (Gildan 18000 crewneck), placement 'front_large'
--   403/11050 White ext / Blue interior 11oz (color-inside mug), 'default'
--   641/16289 White One-size (AS Colour 1001 cotton tote), 'front'
-- ─────────────────────────────────────────────────────────────────────
INSERT OR IGNORE INTO catalog_products
  (sku, brand, label, description_ja, retail_price_jpy,
   printful_product_id, printful_variant_id, printful_placement,
   printful_print_w, printful_print_h,
   printful_sync_product_id, printful_sync_variant_id,
   stripe_product_id, stripe_price_id,
   design_file, mockup_main_file, mockup_url_external,
   suzuri_url, is_active, sort_order, status, fulfillment_route)
VALUES
  ('YUMA-HOODIE-AO', 'yuma', 'MU × YUMA 碧 パーカー',
   '#11 · HOODIE — 碧 + 青色申告は、正義。 · 水色 (Light Blue)',
   9800, 146, 10842, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/yuma/d/design_YUMA-TEE-AO.png',
   '/static/yuma/preview/hoodie_ao.png',
   'https://wearmu.com/static/yuma/preview/hoodie_ao.png',
   NULL, 1, 11, 'live', 'printful_dtg'),
  ('YUMA-CREWNECK-AO', 'yuma', 'MU × YUMA 碧 クルーネック',
   '#12 · CREWNECK — 碧 + 青色申告は、正義。 · 水色 (Light Blue)',
   7800, 145, 7861, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/yuma/d/design_YUMA-TEE-AO.png',
   '/static/yuma/preview/crewneck_ao.png',
   'https://wearmu.com/static/yuma/preview/crewneck_ao.png',
   NULL, 1, 12, 'live', 'printful_dtg'),
  ('YUMA-MUG-AO', 'yuma', 'MU × YUMA 碧 マグ',
   '#13 · MUG — 碧 + 青色申告は、正義。 · 内側ブルー (11oz)',
   3800, 403, 11050, 'default', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/yuma/d/design_YUMA-TEE-AO.png',
   '/static/yuma/preview/mug_ao.png',
   'https://wearmu.com/static/yuma/preview/mug_ao.png',
   NULL, 1, 13, 'live', 'printful_dtg'),
  ('YUMA-TOTE-AO', 'yuma', 'MU × YUMA 碧 トート',
   '#14 · TOTE — 碧 + 青色申告は、正義。 · ホワイト',
   3800, 641, 16289, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/yuma/d/design_YUMA-TEE-AO.png',
   '/static/yuma/preview/tote_ao.png',
   'https://wearmu.com/static/yuma/preview/tote_ao.png',
   NULL, 1, 14, 'live', 'printful_dtg');
