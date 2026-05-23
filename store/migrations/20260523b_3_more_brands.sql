-- wearmu 2026-05-23 v2 migration: 3 more brands (NEWS / KAGI / CHIP)
-- Idempotent. Apply on Fly DB:
-- fly ssh sftp shell -a mu-store put store/migrations/20260523b_3_more_brands.sql /tmp/migration2.sql
-- fly ssh console -a mu-store -C "sqlite3 /data/products.db < /tmp/migration2.sql"

INSERT OR REPLACE INTO catalog_brands
  (slug, name, emoji, color_primary, tagline, is_active, revenue_share_pct, config_json)
VALUES
  ('news', 'MU × NEWS', '📡', '#06b6d4', 'T-minus 0 · No comment', 1, 0,
    '{"design_style":"News / journalism aesthetic. Telegraph monospace + cyan accent + date-stamped panels. Single-color screen-print.","lifestyle_scene":"Tokyo press room late at night, person typing at desk, multiple monitors with news feed glow, soft cyan light","ink_default":"white"}'),
  ('kagi', 'MU × KAGI', '🔑', '#e6c449', '鍵あり · presence', 1, 0,
    '{"design_style":"KAGI smart-home brand. Matte black + brass key cylinder geometry. Single line keychain illustration.","lifestyle_scene":"Tokyo apartment doorway dusk, person turning the doorknob to leave, soft hallway light, gold key in hand","ink_default":"gold"}'),
  ('chip', 'MU × CHIP', '⚡', '#22c55e', 'ESP32 + ❤ · Solder on', 1, 0,
    '{"design_style":"Hardware / maker print. PCB green + silver solder + IC pin grid pattern. Pixel-perfect technical diagram.","lifestyle_scene":"Maker workspace garage, person at soldering bench with magnifier and breadboard, warm tungsten lamp","ink_default":"white"}');

-- 12 starter SKUs
INSERT OR REPLACE INTO catalog_products (sku, brand, label, description_ja, retail_price_jpy, printful_product_id, printful_variant_id, printful_placement, printful_print_w, printful_print_h, is_active, sort_order, status, fulfillment_route) VALUES
  ('NEWS-TEE-01',   'news',  'BREAKING ▮',    'MU × NEWS · BREAKING ▮',     3900,   71,  4017, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('NEWS-HOOD-01',  'news',  'T-MINUS 0',     'MU × NEWS · T-MINUS 0',      9800, 1543, 48770, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('NEWS-JOUR-01',  'news',  'EMBARGOED ✕',   'MU × NEWS · EMBARGOED ✕',    3500,   19,  1320, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('NEWS-STICK-01', 'news',  'NO COMMENT',    'MU × NEWS · NO COMMENT',      800,  358, 10164, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('KAGI-TEE-01',   'kagi',  '鍵あり ◯',       'MU × KAGI · 鍵あり ◯',         3900,   71,  4017, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('KAGI-CAP-01',   'kagi',  'PRESENCE',      'MU × KAGI · PRESENCE',       3500,  438, 12736, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('KAGI-MUG-01',   'kagi',  'LOCK · UNLOCK', 'MU × KAGI · LOCK · UNLOCK',  2200,   19,  1320, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('KAGI-TOTE-01',  'kagi',  '鍵束 12',        'MU × KAGI · 鍵束 12',          2900,   19,  1320, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('CHIP-TEE-01',   'chip',  'ESP32 + ❤',     'MU × CHIP · ESP32 + ❤',      3900,   71,  4017, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('CHIP-HOOD-01',  'chip',  'SOLDER ON',     'MU × CHIP · SOLDER ON',      9800, 1543, 48770, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('CHIP-MUG-01',   'chip',  'PCB ART',       'MU × CHIP · PCB ART',        2200,   19,  1320, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('CHIP-STICK-01', 'chip',  'FW v0.1',       'MU × CHIP · FW v0.1',         800,  358, 10164, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg');
