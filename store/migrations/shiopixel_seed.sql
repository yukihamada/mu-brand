-- Shiopixel 曲T (音を着る) 固定SKU. 1着=1曲, ○のQR→mu.koe.live/oto.html?s=KEY

-- 黒 product71/4017 DTG ¥4,800. is_active=1 で /shop 購入可。

INSERT INTO catalog_brands
  (slug, name, emoji, color_primary, tagline, custom_domain, is_active, revenue_share_pct, config_json)
VALUES
  ('shiopixel','Shiopixel','🎵','#0b0b0b','着ると、鳴る。',NULL,1,0,
   '{"lp_template":"standard","hero":{"title":"Shiopixel","subtitle":"着ると、鳴る","badge":"♪"}}')
ON CONFLICT(slug) DO UPDATE SET name=excluded.name, emoji=excluded.emoji,
  tagline=excluded.tagline, is_active=excluded.is_active, config_json=excluded.config_json;

INSERT OR IGNORE INTO catalog_products
  (sku,brand,label,description_ja,retail_price_jpy,printful_product_id,printful_variant_id,printful_placement,
   printful_print_w,printful_print_h,design_file,mockup_main_file,mockup_url_external,is_active,sort_order,status,fulfillment_route)
VALUES ('SHIO-BJJ','shiopixel','Everybody say BJJ','Everybody say BJJ · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=everybody-say-bjj',4800,71,4017,'front',2250,2700,
   '/static/shiopixel/everybody-say-bjj_print.png','/static/shiopixel/everybody-say-bjj_mockup.png',
   'https://wearmu.com/static/shiopixel/everybody-say-bjj_mockup.png',1,1,'live','printful_dtg');

UPDATE catalog_products SET description_ja='Everybody say BJJ · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=everybody-say-bjj',
   design_file='/static/shiopixel/everybody-say-bjj_print.png',
   mockup_main_file='/static/shiopixel/everybody-say-bjj_mockup.png',
   mockup_url_external='https://wearmu.com/static/shiopixel/everybody-say-bjj_mockup.png',
   is_active=1,status='live',sort_order=1 WHERE sku='SHIO-BJJ';

INSERT OR IGNORE INTO catalog_products
  (sku,brand,label,description_ja,retail_price_jpy,printful_product_id,printful_variant_id,printful_placement,
   printful_print_w,printful_print_h,design_file,mockup_main_file,mockup_url_external,is_active,sort_order,status,fulfillment_route)
VALUES ('SHIO-SHIO','shiopixel','塩とピクセル','塩とピクセル · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=shio-to-pixel',4800,71,4017,'front',2250,2700,
   '/static/shiopixel/shio-to-pixel_print.png','/static/shiopixel/shio-to-pixel_mockup.png',
   'https://wearmu.com/static/shiopixel/shio-to-pixel_mockup.png',1,2,'live','printful_dtg');

UPDATE catalog_products SET description_ja='塩とピクセル · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=shio-to-pixel',
   design_file='/static/shiopixel/shio-to-pixel_print.png',
   mockup_main_file='/static/shiopixel/shio-to-pixel_mockup.png',
   mockup_url_external='https://wearmu.com/static/shiopixel/shio-to-pixel_mockup.png',
   is_active=1,status='live',sort_order=2 WHERE sku='SHIO-SHIO';

INSERT OR IGNORE INTO catalog_products
  (sku,brand,label,description_ja,retail_price_jpy,printful_product_id,printful_variant_id,printful_placement,
   printful_print_w,printful_print_h,design_file,mockup_main_file,mockup_url_external,is_active,sort_order,status,fulfillment_route)
VALUES ('SHIO-MUSUBI','shiopixel','結び直す朝','結び直す朝 · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=musubinaosu-asa',4800,71,4017,'front',2250,2700,
   '/static/shiopixel/musubinaosu-asa_print.png','/static/shiopixel/musubinaosu-asa_mockup.png',
   'https://wearmu.com/static/shiopixel/musubinaosu-asa_mockup.png',1,3,'live','printful_dtg');

UPDATE catalog_products SET description_ja='結び直す朝 · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=musubinaosu-asa',
   design_file='/static/shiopixel/musubinaosu-asa_print.png',
   mockup_main_file='/static/shiopixel/musubinaosu-asa_mockup.png',
   mockup_url_external='https://wearmu.com/static/shiopixel/musubinaosu-asa_mockup.png',
   is_active=1,status='live',sort_order=3 WHERE sku='SHIO-MUSUBI';

INSERT OR IGNORE INTO catalog_products
  (sku,brand,label,description_ja,retail_price_jpy,printful_product_id,printful_variant_id,printful_placement,
   printful_print_w,printful_print_h,design_file,mockup_main_file,mockup_url_external,is_active,sort_order,status,fulfillment_route)
VALUES ('SHIO-HELLO','shiopixel','HELLO 2150','HELLO 2150 · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=hello-2150',4800,71,4017,'front',2250,2700,
   '/static/shiopixel/hello-2150_print.png','/static/shiopixel/hello-2150_mockup.png',
   'https://wearmu.com/static/shiopixel/hello-2150_mockup.png',1,4,'live','printful_dtg');

UPDATE catalog_products SET description_ja='HELLO 2150 · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=hello-2150',
   design_file='/static/shiopixel/hello-2150_print.png',
   mockup_main_file='/static/shiopixel/hello-2150_mockup.png',
   mockup_url_external='https://wearmu.com/static/shiopixel/hello-2150_mockup.png',
   is_active=1,status='live',sort_order=4 WHERE sku='SHIO-HELLO';

INSERT OR IGNORE INTO catalog_products
  (sku,brand,label,description_ja,retail_price_jpy,printful_product_id,printful_variant_id,printful_placement,
   printful_print_w,printful_print_h,design_file,mockup_main_file,mockup_url_external,is_active,sort_order,status,fulfillment_route)
VALUES ('SHIO-LOVE','shiopixel','I love you','I love you · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=i-love-you',4800,71,4017,'front',2250,2700,
   '/static/shiopixel/i-love-you_print.png','/static/shiopixel/i-love-you_mockup.png',
   'https://wearmu.com/static/shiopixel/i-love-you_mockup.png',1,5,'live','printful_dtg');

UPDATE catalog_products SET description_ja='I love you · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=i-love-you',
   design_file='/static/shiopixel/i-love-you_print.png',
   mockup_main_file='/static/shiopixel/i-love-you_mockup.png',
   mockup_url_external='https://wearmu.com/static/shiopixel/i-love-you_mockup.png',
   is_active=1,status='live',sort_order=5 WHERE sku='SHIO-LOVE';

INSERT OR IGNORE INTO catalog_products
  (sku,brand,label,description_ja,retail_price_jpy,printful_product_id,printful_variant_id,printful_placement,
   printful_print_w,printful_print_h,design_file,mockup_main_file,mockup_url_external,is_active,sort_order,status,fulfillment_route)
VALUES ('SHIO-NEED','shiopixel','I need your attention','I need your attention · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=i-need-your-attention',4800,71,4017,'front',2250,2700,
   '/static/shiopixel/i-need-your-attention_print.png','/static/shiopixel/i-need-your-attention_mockup.png',
   'https://wearmu.com/static/shiopixel/i-need-your-attention_mockup.png',1,6,'live','printful_dtg');

UPDATE catalog_products SET description_ja='I need your attention · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=i-need-your-attention',
   design_file='/static/shiopixel/i-need-your-attention_print.png',
   mockup_main_file='/static/shiopixel/i-need-your-attention_mockup.png',
   mockup_url_external='https://wearmu.com/static/shiopixel/i-need-your-attention_mockup.png',
   is_active=1,status='live',sort_order=6 WHERE sku='SHIO-NEED';

INSERT OR IGNORE INTO catalog_products
  (sku,brand,label,description_ja,retail_price_jpy,printful_product_id,printful_variant_id,printful_placement,
   printful_print_w,printful_print_h,design_file,mockup_main_file,mockup_url_external,is_active,sort_order,status,fulfillment_route)
VALUES ('SHIO-FREE','shiopixel','Free to Change','Free to Change · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=free-to-change',4800,71,4017,'front',2250,2700,
   '/static/shiopixel/free-to-change_print.png','/static/shiopixel/free-to-change_mockup.png',
   'https://wearmu.com/static/shiopixel/free-to-change_mockup.png',1,7,'live','printful_dtg');

UPDATE catalog_products SET description_ja='Free to Change · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=free-to-change',
   design_file='/static/shiopixel/free-to-change_print.png',
   mockup_main_file='/static/shiopixel/free-to-change_mockup.png',
   mockup_url_external='https://wearmu.com/static/shiopixel/free-to-change_mockup.png',
   is_active=1,status='live',sort_order=7 WHERE sku='SHIO-FREE';

INSERT OR IGNORE INTO catalog_products
  (sku,brand,label,description_ja,retail_price_jpy,printful_product_id,printful_variant_id,printful_placement,
   printful_print_w,printful_print_h,design_file,mockup_main_file,mockup_url_external,is_active,sort_order,status,fulfillment_route)
VALUES ('SHIO-ATTN','shiopixel','アテンションください','アテンションください · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=attention-kudasai',4800,71,4017,'front',2250,2700,
   '/static/shiopixel/attention-kudasai_print.png','/static/shiopixel/attention-kudasai_mockup.png',
   'https://wearmu.com/static/shiopixel/attention-kudasai_mockup.png',1,8,'live','printful_dtg');

UPDATE catalog_products SET description_ja='アテンションください · 黒T Bella+Canvas 3001 (M) · DTG · 🎵 着ると鳴る mu.koe.live/oto.html?s=attention-kudasai',
   design_file='/static/shiopixel/attention-kudasai_print.png',
   mockup_main_file='/static/shiopixel/attention-kudasai_mockup.png',
   mockup_url_external='https://wearmu.com/static/shiopixel/attention-kudasai_mockup.png',
   is_active=1,status='live',sort_order=8 WHERE sku='SHIO-ATTN';
