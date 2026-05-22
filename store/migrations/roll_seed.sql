-- ─────────────────────────────────────────────────────────────────────
--  ROLL ◐ MU — First Edition (2026-05-22)
--  10 Tees + 10 Rashguards, single brand `roll`.
--
--  Para-BJJ — but written so anyone who rolls through any obstacle can
--  wear it. Triple meaning: spar / wheelchair wheel / momentum.
--
--  Pricing: Tee ¥4,800 (Bella+Canvas 3001, DTG) ·
--           Rashguard ¥9,800 (AOP Men's Rash Guard, sublimation)
--
--  Stripe IDs intentionally NULL — populated post-deploy via
--  /admin/catalog/sync_roll once Printful sync product IDs exist.
--  The LP is buyable on merch.wearmu.com once merch-bridge mirrors
--  these SKUs (separate repo, see products.yaml).
--
--  config_json:
--    donation_pct=50  → §28 profit-split, see /profit-split
--    custom_lp=/roll/ → LP overrides /shop/:sku
--    lead_time_days=14
-- ─────────────────────────────────────────────────────────────────────

INSERT OR IGNORE INTO catalog_brands
  (slug, name, emoji, color_primary, tagline, custom_domain, is_active, revenue_share_pct, config_json)
VALUES
  ('roll', 'ROLL ◐ MU', '◐', '#e6c449',
   'SPIN THE WORLD · 日本のパラ柔術 最前線へ',
   NULL, 1, 50,
   '{"donation_pct":50,"lp_template":"/roll/","lead_time_days":14,"first_edition":true,"approval_required":false,"hero":{"title":"ROLL","title_accent_letter":"O","subtitle":"BY MU · 無 / 月","tagline_en":"SPIN THE WORLD","tagline_en_accent":"SPIN","tagline_jp":"回せ、世界を。","badge":"FIRST EDITION · 2026"},"product_blurbs":{"tee":"Bella+Canvas 3001 · 4.2oz リングスパンコットン · 在庫レス DTG プリント · 7–14日配送","rashguard":"All-Over Print Men''s Rashguard · 昇華プリント全面 · IBJJF対応 · 在庫レス · 7–14日配送"}}');

-- ─── 10 TEES (Bella+Canvas 3001) ─────────────────────────────────────

INSERT OR IGNORE INTO catalog_products
  (sku, brand, label, description_ja, retail_price_jpy,
   printful_product_id, printful_variant_id, printful_placement,
   printful_print_w, printful_print_h,
   printful_sync_product_id, printful_sync_variant_id,
   stripe_product_id, stripe_price_id,
   design_file, mockup_main_file, mockup_url_external,
   suzuri_url, is_active, sort_order, status, fulfillment_route)
VALUES
  ('ROLL-TEE-01', 'roll', 'ROLL ◐ MU',                '#01 · CORE LOGO — ROLL ワードマーク + ◐ 半月 · Black',                                4800, 71, 4017, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-TEE-01.png', NULL, NULL, NULL, 1,  1, 'live', 'printful_dtg'),
  ('ROLL-TEE-02', 'roll', 'SPIN THE WORLD',           '#02 · KINETIC TYPE — 世界を、回せ。 · Black',                                          4800, 71, 4017, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-TEE-02.png', NULL, NULL, NULL, 1,  2, 'live', 'printful_dtg'),
  ('ROLL-TEE-03', 'roll', '片月 HALF MOON',           '#03 · 漢字 + romaji vertical lockup · Black',                                            4800, 71, 4017, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-TEE-03.png', NULL, NULL, NULL, 1,  3, 'live', 'printful_dtg'),
  ('ROLL-TEE-04', 'roll', '回せ、世界を。',           '#04 · 墨絵 calligraphy ink-black on white · White',                                       4800, 71, 4012, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-TEE-04.png', NULL, NULL,  NULL, 1,  4, 'live', 'printful_dtg'),
  ('ROLL-TEE-05', 'roll', 'ROLL #001',                '#05 · LIMITED FIRST 100 — front chest + back number · Black',                            4800, 71, 4017, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-TEE-05.png', NULL, NULL, NULL, 1,  5, 'live', 'printful_dtg'),
  ('ROLL-TEE-06', 'roll', 'NO PITY. JUST ROLL.',      '#06 · MANIFESTO — 同情拒否 manifesto type · Black',                                       4800, 71, 4017, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-TEE-06.png', NULL, NULL, NULL, 1,  6, 'live', 'printful_dtg'),
  ('ROLL-TEE-07', 'roll', 'ONE TURNS ALL',            '#07 · MINIMAL — 片腕がすべてを回す · White',                                                4800, 71, 4012, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-TEE-07.png', NULL, NULL,  NULL, 1,  7, 'live', 'printful_dtg'),
  ('ROLL-TEE-08', 'roll', '◐ HALF MOON MARK',         '#08 · MARK ONLY — giant back print · Black',                                              4800, 71, 4017, 'back_large',  2250, 2700, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-TEE-08.png', NULL, NULL, NULL, 1,  8, 'live', 'printful_dtg'),
  ('ROLL-TEE-09', 'roll', 'MAT · WHEEL · WORLD',      '#09 · 三層 — three rolls stacked · Black',                                                4800, 71, 4017, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-TEE-09.png', NULL, NULL, NULL, 1,  9, 'live', 'printful_dtg'),
  ('ROLL-TEE-10', 'roll', '不可逆 IRREVERSIBLE',      '#10 · 哲学 — 魂の回転は止まらない · Black',                                                4800, 71, 4017, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-TEE-10.png', NULL, NULL, NULL, 1, 10, 'live', 'printful_dtg');

-- ─── 10 RASHGUARDS (AOP Men's Rash Guard, sublimation full-print) ───

INSERT OR IGNORE INTO catalog_products
  (sku, brand, label, description_ja, retail_price_jpy,
   printful_product_id, printful_variant_id, printful_placement,
   printful_print_w, printful_print_h,
   printful_sync_product_id, printful_sync_variant_id,
   stripe_product_id, stripe_price_id,
   design_file, mockup_main_file, mockup_url_external,
   suzuri_url, is_active, sort_order, status, fulfillment_route)
VALUES
  ('ROLL-RASH-01', 'roll', 'HALF MOON OVERLOAD',     '#01 · AOP — 巨大 ◐ がトルソーから袖まで bleed',                                          9800, 301, 9328, 'front', 4200, 5400, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-RASH-01.png', NULL, NULL, NULL, 1, 11, 'live', 'printful_aop'),
  ('ROLL-RASH-02', 'roll', 'SPIN GRID',              '#02 · AOP — 回転するタイポ・グリッド kinetic pattern',                                     9800, 301, 9328, 'front', 4200, 5400, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-RASH-02.png', NULL, NULL, NULL, 1, 12, 'live', 'printful_aop'),
  ('ROLL-RASH-03', 'roll', '回転 KAITEN',            '#03 · AOP — 巨大な 回 brush stroke wrapping body',                                          9800, 301, 9328, 'front', 4200, 5400, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-RASH-03.png', NULL, NULL, NULL, 1, 13, 'live', 'printful_aop'),
  ('ROLL-RASH-04', 'roll', 'MAT TOPOLOGY',           '#04 · AOP — grappling lines as topographic contour map',                                   9800, 301, 9328, 'front', 4200, 5400, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-RASH-04.png', NULL, NULL, NULL, 1, 14, 'live', 'printful_aop'),
  ('ROLL-RASH-05', 'roll', 'ROLL THUNDER',           '#05 · AOP — red lightning + ROLL wordmark, black base',                                    9800, 301, 9328, 'front', 4200, 5400, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-RASH-05.png', NULL, NULL, NULL, 1, 15, 'live', 'printful_aop'),
  ('ROLL-RASH-06', 'roll', 'THE WHEEL',              '#06 · AOP — wheelchair wheel motif, sacred geometry',                                       9800, 301, 9328, 'front', 4200, 5400, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-RASH-06.png', NULL, NULL, NULL, 1, 16, 'live', 'printful_aop'),
  ('ROLL-RASH-07', 'roll', 'PHASES',                 '#07 · AOP — moon phases sleeve to sleeve · 月相',                                          9800, 301, 9328, 'front', 4200, 5400, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-RASH-07.png', NULL, NULL, NULL, 1, 17, 'live', 'printful_aop'),
  ('ROLL-RASH-08', 'roll', '前線 FRONT LINE',        '#08 · AOP — JP/EN bilingual war-cry, red 国旗 accent',                                      9800, 301, 9328, 'front', 4200, 5400, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-RASH-08.png', NULL, NULL, NULL, 1, 18, 'live', 'printful_aop'),
  ('ROLL-RASH-09', 'roll', 'ASYMMETRY 片',           '#09 · AOP — 片袖のみ印刷, 非対称こそ美 · UNIQUE',                                            9800, 301, 9328, 'front', 4200, 5400, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-RASH-09.png', NULL, NULL, NULL, 1, 19, 'live', 'printful_aop'),
  ('ROLL-RASH-10', 'roll', '無限 MUGEN ROLL',        '#10 · AOP — ∞ infinity loop pattern · gold on black',                                       9800, 301, 9328, 'front', 4200, 5400, NULL, NULL, NULL, NULL, '/static/roll/d/design_ROLL-RASH-10.png', NULL, NULL, NULL, 1, 20, 'live', 'printful_aop');
