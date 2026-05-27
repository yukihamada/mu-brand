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
