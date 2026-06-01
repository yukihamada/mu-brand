-- ────────────────────────────────────────────────────────────
--  Shiopixel — 曲T (音を着る). 1着=1曲, ○のQR→mu.koe.live/oto.html?s=KEY
--  固定SKUカタログ商品 (MUGON型). is_active=1 で /shop 公開・購入可。
--  黒 Bella+Canvas 3001 (product 71) DTG, M=4017, ¥4,800。
-- ────────────────────────────────────────────────────────────
INSERT INTO catalog_brands
  (slug, name, emoji, color_primary, tagline, custom_domain, is_active, revenue_share_pct, config_json)
VALUES
  ('shiopixel', 'Shiopixel', '🎵', '#0b0b0b', '着ると、鳴る。', NULL, 1, 0,
   '{"lp_template":"standard","design_style":"Black tee, white type, one song per tee. Tap the circle to play on mu.koe.live.","hero":{"title":"Shiopixel","subtitle":"着ると、鳴る","badge":"♪"}}')
ON CONFLICT(slug) DO UPDATE SET
  name=excluded.name, emoji=excluded.emoji, tagline=excluded.tagline,
  is_active=excluded.is_active, config_json=excluded.config_json;

INSERT OR IGNORE INTO catalog_products
  (sku, brand, label, description_ja, retail_price_jpy,
   printful_product_id, printful_variant_id, printful_placement,
   printful_print_w, printful_print_h,
   design_file, mockup_main_file, mockup_url_external,
   is_active, sort_order, status, fulfillment_route)
VALUES
  ('SHIO-BJJ', 'shiopixel', 'Everybody say BJJ',
   'Everybody say BJJ · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=everybody-say-bjj',
   4800, 71, 4017, 'front', 2250, 2700,
   '/static/shiopixel/everybody-say-bjj_print.png',
   '/static/shiopixel/everybody-say-bjj.png',
   'https://wearmu.com/static/shiopixel/everybody-say-bjj.png',
   1, 1, 'live', 'printful_dtg');

-- 説明/価格は既存行も更新(冪等)
UPDATE catalog_products SET
  description_ja='Everybody say BJJ · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=everybody-say-bjj',
  is_active=1, status='live'
WHERE sku='SHIO-BJJ';
