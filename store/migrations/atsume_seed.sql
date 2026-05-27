-- ─────────────────────────────────────────────────────────────────────
--  MU × ATSUME — DEV TEAM EDITION (2026-05-27)
--
--  Collab with 株式会社アツメ (ATSUME inc., atsume.io) — "挑戦者の仲間を集める".
--  ATSUME Dev = the engineering team behind their sports apps
--  (TORASPO / ELEVEN / WeGoFast / BLANK_).
--
--  Lineup:
--    • ATSUME-TEE-DEV   — the ATSUME Dev mascot (engineer × athlete, the
--      "gathering dots" motif). LIVE — buyable now. MU-created art, so it is
--      fully fulfillable. White Bella+Canvas 3001 (dark mascot reads on white).
--    • ATSUME-TEE-{TORASPO,ELEVEN,WEGOFAST,BLANK} — one tee per ATSUME app.
--      status='review' until the partner's real logo files land; they only
--      surface on the LP / shop once flipped to 'live' (reads filter status).
--
--  Pricing: Tee ¥4,800 (Bella+Canvas 3001, DTG).
--
--  Stripe IDs NULL — built on the fly by /api/shop/checkout (price_data).
--  Printful IDs reuse roll's verified Bella+Canvas 3001 product 71
--  (variant 4012 White / 4017 Black) — no new unverified variant IDs.
--
--  revenue_share_pct = 0 → partner split TBD with ATSUME, not invented here.
--
--  UPSERT on the brand row so iterative hero-copy edits land on every boot;
--  INSERT OR IGNORE on products so manual status/asset edits are never
--  clobbered.
-- ─────────────────────────────────────────────────────────────────────

INSERT INTO catalog_brands
  (slug, name, emoji, color_primary, tagline, custom_domain, is_active, revenue_share_pct, config_json)
VALUES
  ('atsume', 'MU × ATSUME', '⊙', '#F2792B',
   '挑戦者の仲間を集める · DEV TEAM EDITION',
   NULL, 1, 0,
   '{"design_style":"ATSUME dev-studio collab. Scattered dots condensing into one mark (gathering motif), bold monoline + single warm amber #F2792B accent on black. Engineer-athlete energy.","lifestyle_scene":"startup studio meets sports court, developer at a standing desk with a ball nearby","ink_default":"black","approval_required":true,"partner":"株式会社アツメ (ATSUME inc.) — atsume.io","hero":{"title":"ATSUME ⊙ DEV","title_accent_letter":"⊙","subtitle":"BY MU × 株式会社アツメ","tagline_en":"GATHER THE CHALLENGERS","tagline_en_accent":"GATHER","tagline_jp":"挑戦者の仲間を、集める。","badge":"DEV TEAM COLLAB · 2026"},"product_blurbs":{"tee":"Bella+Canvas 3001 · 4.2oz リングスパンコットン · 在庫レス DTG プリント · 7–14日配送"}}')
ON CONFLICT(slug) DO UPDATE SET
  name              = excluded.name,
  emoji             = excluded.emoji,
  color_primary     = excluded.color_primary,
  tagline           = excluded.tagline,
  is_active         = excluded.is_active,
  revenue_share_pct = excluded.revenue_share_pct,
  config_json       = excluded.config_json;

-- ─── HERO: ATSUME Dev mascot tee — LIVE, buyable now ─────────────────
INSERT OR IGNORE INTO catalog_products
  (sku, brand, label, description_ja, retail_price_jpy,
   printful_product_id, printful_variant_id, printful_placement,
   printful_print_w, printful_print_h,
   printful_sync_product_id, printful_sync_variant_id,
   stripe_product_id, stripe_price_id,
   design_file, mockup_main_file, mockup_url_external,
   suzuri_url, is_active, sort_order, status, fulfillment_route)
VALUES
  ('ATSUME-TEE-DEV', 'atsume', 'MU × ATSUME DEV',
   '#01 · DEV MASCOT — エンジニア × アスリート、集合ドットが一つのマークへ · White',
   4800, 71, 4012, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/atsume/d/design_ATSUME-TEE-DEV.png',
   '/static/atsume/d/atsume_dev_white.png',
   'https://wearmu.com/static/atsume/d/atsume_dev_white.png',
   NULL, 1, 1, 'live', 'printful_dtg');

-- ─── ATSUME app tees — status='review' until real logos land ─────────
INSERT OR IGNORE INTO catalog_products
  (sku, brand, label, description_ja, retail_price_jpy,
   printful_product_id, printful_variant_id, printful_placement,
   printful_print_w, printful_print_h,
   printful_sync_product_id, printful_sync_variant_id,
   stripe_product_id, stripe_price_id,
   design_file, mockup_main_file, mockup_url_external,
   suzuri_url, is_active, sort_order, status, fulfillment_route)
VALUES
  ('ATSUME-TEE-TORASPO', 'atsume', 'MU × TORASPO',
   '#02 · TORASPO ロゴ — ATSUME sports app · Black',
   4800, 71, 4017, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/atsume/d/design_ATSUME-TEE-TORASPO.png', '', '',
   NULL, 0, 2, 'review', 'printful_dtg'),
  ('ATSUME-TEE-ELEVEN', 'atsume', 'MU × ELEVEN',
   '#03 · ELEVEN ロゴ — スポーツスクール管理アプリ · Black',
   4800, 71, 4017, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/atsume/d/design_ATSUME-TEE-ELEVEN.png', '', '',
   NULL, 0, 3, 'review', 'printful_dtg'),
  ('ATSUME-TEE-WEGOFAST', 'atsume', 'MU × WeGoFast',
   '#04 · WeGoFast ロゴ — スポーツ特化英会話 · Black',
   4800, 71, 4017, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/atsume/d/design_ATSUME-TEE-WEGOFAST.png', '', '',
   NULL, 0, 4, 'review', 'printful_dtg'),
  ('ATSUME-TEE-BLANK', 'atsume', 'MU × BLANK_',
   '#05 · BLANK_ ロゴ — ATSUME product · Black',
   4800, 71, 4017, 'front_large', 2250, 2700, NULL, NULL, NULL, NULL,
   '/static/atsume/d/design_ATSUME-TEE-BLANK.png', '', '',
   NULL, 0, 5, 'review', 'printful_dtg');
