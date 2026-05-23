-- wearmu 2026-05-23 migration: 5 new brands + brand config_json backfill
-- Idempotent (uses INSERT OR REPLACE / UPDATE). Safe to re-run.
-- Apply on Fly live DB: fly ssh console -a mu-store -C "sqlite3 /data/products.db < /app/store/migrations/20260523_5_brands_and_config.sql"

-- ── 5 new brands ──────────────────────────────────────────────
INSERT OR REPLACE INTO catalog_brands
  (slug, name, emoji, color_primary, tagline, is_active, revenue_share_pct, config_json)
VALUES
  ('voice',  'MU × VOICE',   '🎤', '#9333ea', 'First the word · Koe-first apparel',         1, 0,
    '{"design_style":"Voice / Koe brand. Audio waveform glyph + katakana, technological yet calm. Grayscale + neon-violet accent.","lifestyle_scene":"Person speaking into a small microphone in a sunlit Tokyo studio, soft acoustic foam wall behind, late morning","ink_default":"white"}'),
  ('ocean',  'MU × OCEAN',   '🌊', '#0ea5e9', 'Aloha ◐ MU · Salt year',                     1, 0,
    '{"design_style":"Pacific Ocean / Hawaii beach. Sun-bleached palette, salt texture, ALOHA katakana, single wave line.","lifestyle_scene":"Hawaii beach late afternoon, person standing in shallow waves holding a surfboard under arm, golden hour light","ink_default":"white"}'),
  ('lodge',  'MU × LODGE',   '🏔️', '#92400e', '弟子屈 hut life · 杉 forever',                1, 0,
    '{"design_style":"Hokkaido lodge life. Deep brown + linen + navy. Cabin, firewood, falling snow motif. Crafted wood-block stamp feel.","lifestyle_scene":"snowy Hokkaido cabin doorway at dusk, person with chopped firewood in arms, breath visible, wooden porch lit by lantern","ink_default":"white"}'),
  ('octagon','MU × OCTAGON', '🥊', '#dc2626', 'Walk-out · 朱と群青',                         1, 0,
    '{"design_style":"Combat sport / UFC walk-out. Crimson 朱 #DC2626 + ultramarine 群青 #1E40AF two-color, bold athletic type, no fluff.","lifestyle_scene":"MMA octagon walk-out tunnel, athlete entering ring, harsh side spotlight, hand-wrapped fists, intense composure","ink_default":"white"}'),
  ('founder','MU × FOUNDER', '🚀', '#1f2937', '20 years shipping · Still early',             1, 0,
    '{"design_style":"Startup founder culture. Jet-black + clean white. Bureaucratic document fonts, archival stamp style, dry humor.","lifestyle_scene":"Tokyo startup office at night, person at standing desk with single laptop and a cardboard moving box, soft lamp","ink_default":"white"}');

-- ── 20 starter SKUs ──────────────────────────────────────────
-- VOICE
INSERT OR REPLACE INTO catalog_products (sku, brand, label, description_ja, retail_price_jpy, printful_product_id, printful_variant_id, printful_placement, printful_print_w, printful_print_h, is_active, sort_order, status, fulfillment_route) VALUES
  ('VOICE-TEE-01',    'voice', 'FIRST WORD', 'MU × VOICE #01 · TEE-BLACK · FIRST WORD',    3900,  71, 4017,  'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('VOICE-HOOD-01',   'voice', 'WAV.MU',     'MU × VOICE #01 · HOODIE-BLACK · WAV.MU',     9800, 1543, 48770, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('VOICE-MUG-01',    'voice', '聞こえる',    'MU × VOICE #01 · MUG · 聞こえる',             2200,  19, 1320,  'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('VOICE-STICK-01',  'voice', 'NO TYPE',    'MU × VOICE #01 · STICKER · NO TYPE',          800,  358, 10164, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg');
-- OCEAN
INSERT OR REPLACE INTO catalog_products (sku, brand, label, description_ja, retail_price_jpy, printful_product_id, printful_variant_id, printful_placement, printful_print_w, printful_print_h, is_active, sort_order, status, fulfillment_route) VALUES
  ('OCEAN-TEE-01',      'ocean', 'ALOHA・MU',     'MU × OCEAN #01 · TEE-BLACK · ALOHA·MU',           3900, 71, 4017, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('OCEAN-TANK-01',     'ocean', 'SALT YEAR',     'MU × OCEAN #01 · TEE WHITE · SALT YEAR',          3900, 71, 4012, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('OCEAN-TOTE-01',     'ocean', 'PACIFIC TIME',  'MU × OCEAN #01 · TOTE · PACIFIC TIME',            2900, 19, 1320, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('OCEAN-TEE-WHITE-01','ocean', '波 ◐ MOON',     'MU × OCEAN #01 · TEE-WHITE · 波 ◐ MOON',          3900, 71, 4012, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg');
-- LODGE
INSERT OR REPLACE INTO catalog_products (sku, brand, label, description_ja, retail_price_jpy, printful_product_id, printful_variant_id, printful_placement, printful_print_w, printful_print_h, is_active, sort_order, status, fulfillment_route) VALUES
  ('LODGE-HOOD-01',   'lodge', 'WINTER STAY',  'MU × LODGE #01 · HOODIE-BLACK · WINTER STAY',           9800, 1543, 48770, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('LODGE-LST-01',    'lodge', '杉 = 永遠',     'MU × LODGE #01 · LONG-SLEEVE-BLACK · 杉 = 永遠',         5800,  356, 10096, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('LODGE-BEAN-01',   'lodge', 'FIRE BUILT',   'MU × LODGE #01 · TEE WHITE · FIRE BUILT',               3200,   71,  4012, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('LODGE-CANVAS-01', 'lodge', '1100 KM SOUTH','MU × LODGE #01 · CANVAS 10x20 · 1100 KM SOUTH',         7800,    3, 19297, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg');
-- OCTAGON
INSERT OR REPLACE INTO catalog_products (sku, brand, label, description_ja, retail_price_jpy, printful_product_id, printful_variant_id, printful_placement, printful_print_w, printful_print_h, is_active, sort_order, status, fulfillment_route) VALUES
  ('OCT-TEE-01',     'octagon', 'WALK OUT',  'MU × OCTAGON #01 · TEE-BLACK · WALK OUT',             3900,  71,  4017, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('OCT-RASH-01',    'octagon', '5 ROUNDS',  'MU × OCTAGON #01 · RASH · 5 ROUNDS',                  6800, 301,  9328, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('OCT-TEE-RED-01', 'octagon', '朱と群青',   'MU × OCTAGON #01 · TEE-RED · 朱と群青',                3900,  71,  4014, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('OCT-CAP-01',     'octagon', 'OCTAGON ◯', 'MU × OCTAGON #01 · CAP · OCTAGON ◯',                  3500, 438, 12736, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg');
-- FOUNDER
INSERT OR REPLACE INTO catalog_products (sku, brand, label, description_ja, retail_price_jpy, printful_product_id, printful_variant_id, printful_placement, printful_print_w, printful_print_h, is_active, sort_order, status, fulfillment_route) VALUES
  ('FOUND-TEE-01',  'founder', '20 YEARS SHIPPING','MU × FOUNDER #01 · TEE-BLACK · 20 YEARS SHIPPING', 3900,   71,  4017, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('FOUND-HOOD-01', 'founder', 'STILL EARLY',      'MU × FOUNDER #01 · HOODIE-BLACK · STILL EARLY',    9800, 1543, 48770, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('FOUND-CAP-01',  'founder', 'CEO・MU',          'MU × FOUNDER #01 · CAP · CEO・MU',                3500,  438, 12736, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('FOUND-MUG-01',  'founder', 'DAY 1 EVERY DAY',  'MU × FOUNDER #01 · MUG · DAY 1 EVERY DAY',        2200,   19,  1320, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg');

-- ── existing brands: backfill design_style / lifestyle_scene / ink_default ──
-- For each brand, merge the keys into existing config_json without losing other keys.
-- SQLite ≥3.38 supports json_patch; we use json_set per key for compatibility.
UPDATE catalog_brands SET config_json = json_set(
  COALESCE(config_json, '{}'),
  '$.design_style', 'BJJ humor/quote print. Bold editorial sumi-ink brush type. Mostly type, single optional line illustration.',
  '$.lifestyle_scene', 'BJJ academy lobby late afternoon, Japanese athlete with folded gi over arm',
  '$.ink_default', 'white'
) WHERE slug = 'bjj';

UPDATE catalog_brands SET config_json = json_set(
  COALESCE(config_json, '{}'),
  '$.design_style', 'Developer terminal aesthetic. Monospace pixel-font type, ASCII glyph. Single color.',
  '$.lifestyle_scene', 'Tokyo developer cafe, person at MacBook, soft window light',
  '$.ink_default', 'white'
) WHERE slug = 'code';

UPDATE catalog_brands SET config_json = json_set(
  COALESCE(config_json, '{}'),
  '$.design_style', 'Coffee culture print. Hand-drawn line work, warm earthy serif. Espresso brown.',
  '$.lifestyle_scene', 'specialty coffee bar interior, barista or customer at counter',
  '$.ink_default', 'brown'
) WHERE slug = 'coffee';

UPDATE catalog_brands SET config_json = json_set(
  COALESCE(config_json, '{}'),
  '$.design_style', 'Zen sumi-e single-stroke kanji calligraphy. Black ink.',
  '$.lifestyle_scene', 'minimalist tatami room at dawn, quiet posture, single ceramic cup',
  '$.ink_default', 'white'
) WHERE slug = 'zen';

UPDATE catalog_brands SET config_json = json_set(
  COALESCE(config_json, '{}'),
  '$.design_style', 'Lunar crescent + dotted constellation, minimal type. Pale gold.',
  '$.lifestyle_scene', 'rooftop at twilight, lone figure, deep blue gradient sky, no harsh light',
  '$.ink_default', 'pale_gold'
) WHERE slug = 'moon';

UPDATE catalog_brands SET config_json = json_set(
  COALESCE(config_json, '{}'),
  '$.design_style', 'MU void — empty circle, single brush stroke, 無 calligraphy. Gold.',
  '$.lifestyle_scene', 'minimalist white gallery, single figure centered, soft shadow, gold leaf accent',
  '$.ink_default', 'gold'
) WHERE slug = 'mu';

UPDATE catalog_brands SET config_json = json_set(
  COALESCE(config_json, '{}'),
  '$.design_style', 'Tokyo mid-century travel-poster mix katakana + roman, 2-color flat palette.',
  '$.lifestyle_scene', 'Shibuya crossing dusk, person mid-stride, blurred neon',
  '$.ink_default', 'white'
) WHERE slug = 'tokyo';

UPDATE catalog_brands SET config_json = json_set(
  COALESCE(config_json, '{}'),
  '$.design_style', 'BJJ athlete brand. Bold sport typography, stopwatch / mat / belt motif. JF mark.',
  '$.lifestyle_scene', 'BJJ tournament side area, athlete on bench preparing',
  '$.ink_default', 'white'
) WHERE slug = 'jiuflow';

UPDATE catalog_brands SET config_json = json_set(
  COALESCE(config_json, '{}'),
  '$.design_style', 'Premium yakiniku. Refined brass serif, charcoal/binchotan motif.',
  '$.lifestyle_scene', 'yakiniku restaurant interior, server behind counter, charcoal grill smoke',
  '$.ink_default', 'gold'
) WHERE slug = 'kokon';

UPDATE catalog_brands SET config_json = json_set(
  COALESCE(config_json, '{}'),
  '$.design_style', 'BJJ rolling action. Dynamic kanji + energetic ink line.',
  '$.lifestyle_scene', 'BJJ academy after roll, towel over shoulder, mat in background',
  '$.ink_default', 'white'
) WHERE slug = 'roll';
