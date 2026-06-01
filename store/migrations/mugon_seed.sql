-- ────────────────────────────────────────────────────────────────────
--  MUGON 無言 — MU メッセージT 公開コレクション (2026-06-01)
--
--  墨黒×月白・明朝・文字だけ。普遍的なMUのことば(引き算/静/月/禅)。
--  ⚠ TAKA/家の内輪ネタ(家賃はライブ1本 等)は含めない=非売品。
--  is_active=0/status='draft' → 公開前。確認後 1 に上げて /shop 露出。
--  黒 Bella+Canvas 3001 (product 71) DTG. size→variant: S=4016/M=4017/L=4018
--  価格 ¥4,800。生成りカラー/XLは variant 未検証→別seedで追加。
-- ────────────────────────────────────────────────────────────────────

INSERT INTO catalog_brands
  (slug, name, emoji, color_primary, tagline, custom_domain, is_active, revenue_share_pct, config_json)
VALUES
  ('mugon', 'MUGON 無言', '無', '#0b0b0b',
   '無言。引き算のことば。',
   NULL, 0, 0,
   '{"lp_template":"standard","design_style":"Sumi-black tee, moon-white Mincho W6, text only, deadpan. Pure MU philosophy.","ink_default":"white","hero":{"title":"無言","subtitle":"引き算のことば","badge":"MU"}}')
ON CONFLICT(slug) DO UPDATE SET
  name=excluded.name, emoji=excluded.emoji, color_primary=excluded.color_primary,
  tagline=excluded.tagline, is_active=excluded.is_active, config_json=excluded.config_json;

-- 旧 per-size SKU(MUGON-*-S 等=2ダッシュ)を掃除。基本7SKU(MUGON-*=1ダッシュ)は残す。未公開なので安全・冪等。
DELETE FROM catalog_products WHERE brand='mugon' AND sku GLOB 'MUGON-*-*';

INSERT OR IGNORE INTO catalog_products
  (sku, brand, label, description_ja, retail_price_jpy,
   printful_product_id, printful_variant_id, printful_placement,
   printful_print_w, printful_print_h,
   printful_sync_product_id, printful_sync_variant_id,
   stripe_product_id, stripe_price_id,
   design_file, mockup_main_file, mockup_url_external,
   suzuri_url, is_active, sort_order, status, fulfillment_route)
VALUES
  ('MUGON-MU', 'mugon', '無',
   '無 · 黒T Bella+Canvas 3001 (M) · DTG ·  · ♪ mu.koe.live/?s=mu',
   4800, 71, 4017, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/mugon/d/design_MUGON-MU.png',
   '/static/mugon/preview/preview_MUGON-MU.png',
   'https://wearmu.com/static/mugon/preview/preview_MUGON-MU.png',
   NULL, 0, 1, 'draft', 'printful_dtg'),
  ('MUGON-TASANAI', 'mugon', '足さない。',
   '足さない。 · 黒T Bella+Canvas 3001 (M) · DTG · Add nothing.',
   4800, 71, 4017, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/mugon/d/design_MUGON-TASANAI.png',
   '/static/mugon/preview/preview_MUGON-TASANAI.png',
   'https://wearmu.com/static/mugon/preview/preview_MUGON-TASANAI.png',
   NULL, 0, 2, 'draft', 'printful_dtg'),
  ('MUGON-NOISE', 'mugon', 'ノイズを、抜く。',
   'ノイズを、抜く。 · 黒T Bella+Canvas 3001 (M) · DTG · Cut the noise. · ♪ mu.koe.live/?s=noise',
   4800, 71, 4017, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/mugon/d/design_MUGON-NOISE.png',
   '/static/mugon/preview/preview_MUGON-NOISE.png',
   'https://wearmu.com/static/mugon/preview/preview_MUGON-NOISE.png',
   NULL, 0, 3, 'draft', 'printful_dtg'),
  ('MUGON-SEIJAKU', 'mugon', '静けさも、強さ。',
   '静けさも、強さ。 · 黒T Bella+Canvas 3001 (M) · DTG · Stillness is strength. · ♪ mu.koe.live/?s=seijaku',
   4800, 71, 4017, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/mugon/d/design_MUGON-SEIJAKU.png',
   '/static/mugon/preview/preview_MUGON-SEIJAKU.png',
   'https://wearmu.com/static/mugon/preview/preview_MUGON-SEIJAKU.png',
   NULL, 0, 4, 'draft', 'printful_dtg'),
  ('MUGON-TSUKI', 'mugon', '月は、どこでも同じ。',
   '月は、どこでも同じ。 · 黒T Bella+Canvas 3001 (M) · DTG · Same moon, everywhere.',
   4800, 71, 4017, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/mugon/d/design_MUGON-TSUKI.png',
   '/static/mugon/preview/preview_MUGON-TSUKI.png',
   'https://wearmu.com/static/mugon/preview/preview_MUGON-TSUKI.png',
   NULL, 0, 5, 'draft', 'printful_dtg'),
  ('MUGON-MUSHIN', 'mugon', '無心。',
   '無心。 · 黒T Bella+Canvas 3001 (M) · DTG · No-mind.',
   4800, 71, 4017, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/mugon/d/design_MUGON-MUSHIN.png',
   '/static/mugon/preview/preview_MUGON-MUSHIN.png',
   'https://wearmu.com/static/mugon/preview/preview_MUGON-MUSHIN.png',
   NULL, 0, 6, 'draft', 'printful_dtg'),
  ('MUGON-CAGE', 'mugon', '四分三十三秒',
   '四分三十三秒 · 黒T Bella+Canvas 3001 (M) · DTG · 4''33″ · ♪ mu.koe.live/?s=cage',
   4800, 71, 4017, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/mugon/d/design_MUGON-CAGE.png',
   '/static/mugon/preview/preview_MUGON-CAGE.png',
   'https://wearmu.com/static/mugon/preview/preview_MUGON-CAGE.png',
   NULL, 0, 7, 'draft', 'printful_dtg');
