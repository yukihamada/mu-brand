-- ────────────────────────────────────────────────────────────────────
--  MUON 無音 — MU メッセージT 公開コレクション (2026-06-01)
--  旧 MUGON(無言) からのリネーム。墨黒×明朝・文字だけ・引き算。
--  is_active=1/live(公開)。黒 Bella+Canvas 3001 (product 71, M=4017) ¥4,800。
-- ────────────────────────────────────────────────────────────────────

-- 旧 mugon ブランドを除去(リネーム元)。muonの行は触らない=トグル/公開状態を保持。
DELETE FROM catalog_products WHERE brand='mugon';
DELETE FROM catalog_brands   WHERE slug='mugon';

INSERT INTO catalog_brands
  (slug, name, emoji, color_primary, tagline, custom_domain, is_active, revenue_share_pct, config_json)
VALUES
  ('muon', 'MUON 無音', '無', '#0b0b0b',
   '無音。引き算のことば。',
   NULL, 1, 0,
   '{"lp_template":"standard","design_style":"Sumi-black tee, moon-white Mincho W6, text only, deadpan. Pure MU philosophy.","ink_default":"white","hero":{"title":"無音","subtitle":"引き算のことば","badge":"MU"}}')
ON CONFLICT(slug) DO UPDATE SET
  name=excluded.name, emoji=excluded.emoji, color_primary=excluded.color_primary,
  tagline=excluded.tagline, config_json=excluded.config_json;

INSERT OR IGNORE INTO catalog_products
  (sku, brand, label, description_ja, retail_price_jpy,
   printful_product_id, printful_variant_id, printful_placement,
   printful_print_w, printful_print_h,
   printful_sync_product_id, printful_sync_variant_id,
   stripe_product_id, stripe_price_id,
   design_file, mockup_main_file, mockup_url_external,
   suzuri_url, is_active, sort_order, status, fulfillment_route)
VALUES
  ('MUON-MU', 'muon', '無',
   '無 · 黒T Bella+Canvas 3001 (M) · DTG ·  · ♪ mu.koe.live/?s=mu',
   4800, 71, 4017, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/muon/d/design_MUON-MU.png',
   '/static/muon/preview/preview_MUON-MU.png',
   'https://wearmu.com/static/muon/preview/preview_MUON-MU.png',
   NULL, 1, 1, 'live', 'printful_dtg'),
  ('MUON-TASANAI', 'muon', '足さない。',
   '足さない。 · 黒T Bella+Canvas 3001 (M) · DTG · Add nothing.',
   4800, 71, 4017, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/muon/d/design_MUON-TASANAI.png',
   '/static/muon/preview/preview_MUON-TASANAI.png',
   'https://wearmu.com/static/muon/preview/preview_MUON-TASANAI.png',
   NULL, 1, 2, 'live', 'printful_dtg'),
  ('MUON-NOISE', 'muon', 'ノイズを、抜く。',
   'ノイズを、抜く。 · 黒T Bella+Canvas 3001 (M) · DTG · Cut the noise. · ♪ mu.koe.live/?s=noise',
   4800, 71, 4017, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/muon/d/design_MUON-NOISE.png',
   '/static/muon/preview/preview_MUON-NOISE.png',
   'https://wearmu.com/static/muon/preview/preview_MUON-NOISE.png',
   NULL, 1, 3, 'live', 'printful_dtg'),
  ('MUON-SEIJAKU', 'muon', '静けさも、強さ。',
   '静けさも、強さ。 · 黒T Bella+Canvas 3001 (M) · DTG · Stillness is strength. · ♪ mu.koe.live/?s=seijaku',
   4800, 71, 4017, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/muon/d/design_MUON-SEIJAKU.png',
   '/static/muon/preview/preview_MUON-SEIJAKU.png',
   'https://wearmu.com/static/muon/preview/preview_MUON-SEIJAKU.png',
   NULL, 1, 4, 'live', 'printful_dtg'),
  ('MUON-TSUKI', 'muon', '月は、どこでも同じ。',
   '月は、どこでも同じ。 · 黒T Bella+Canvas 3001 (M) · DTG · Same moon, everywhere.',
   4800, 71, 4017, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/muon/d/design_MUON-TSUKI.png',
   '/static/muon/preview/preview_MUON-TSUKI.png',
   'https://wearmu.com/static/muon/preview/preview_MUON-TSUKI.png',
   NULL, 1, 5, 'live', 'printful_dtg'),
  ('MUON-MUSHIN', 'muon', '無心。',
   '無心。 · 黒T Bella+Canvas 3001 (M) · DTG · No-mind.',
   4800, 71, 4017, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/muon/d/design_MUON-MUSHIN.png',
   '/static/muon/preview/preview_MUON-MUSHIN.png',
   'https://wearmu.com/static/muon/preview/preview_MUON-MUSHIN.png',
   NULL, 1, 6, 'live', 'printful_dtg'),
  ('MUON-CAGE', 'muon', '四分三十三秒',
   '四分三十三秒 · 黒T Bella+Canvas 3001 (M) · DTG · 4''33″ · ♪ mu.koe.live/?s=cage',
   4800, 71, 4017, 'front', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/muon/d/design_MUON-CAGE.png',
   '/static/muon/preview/preview_MUON-CAGE.png',
   'https://wearmu.com/static/muon/preview/preview_MUON-CAGE.png',
   NULL, 1, 7, 'live', 'printful_dtg');
