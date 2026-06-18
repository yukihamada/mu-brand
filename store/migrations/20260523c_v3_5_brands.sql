-- wearmu 2026-05-23 v3 migration: 5 research-driven brands
-- Idempotent. Apply on Fly DB:
-- fly ssh sftp shell -a mu-store put store/migrations/20260523c_v3_5_brands.sql /tmp/migration3.sql
-- fly ssh console -a mu-store -C 'bash -c "sqlite3 /data/products.db < /tmp/migration3.sql"'

INSERT OR REPLACE INTO catalog_brands
  (slug, name, emoji, color_primary, tagline, is_active, revenue_share_pct, config_json)
VALUES
  ('anime',  'MU × ANIME',  '🌀', '#a855f7', 'Mono no aware · sophisticated otaku', 1, 0,
    '{"design_style":"Anime tribute, sophisticated older-fan aesthetic. Single character silhouette + episode metadata typography. NOT loud anime art — mono no aware feel.","lifestyle_scene":"Tokyo Nakano Broadway hallway evening, person in tee browsing vinyl figurines under fluorescent light","ink_default":"white"}'),
  ('wagyu',  'MU × WAGYU',  '🥩', '#b91c1c', '霜降り · 脂 = 静寂',                    1, 0,
    '{"design_style":"Japan-premium wagyu food culture. Marbled texture pattern + A5 grade typography + binchotan charcoal accent. Refined and meaty.","lifestyle_scene":"Tokyo yakiniku restaurant private room, chef holding tongs over glowing binchotan, soft red ember light, marbled beef on tray","ink_default":"gold"}'),
  ('analog', 'MU × ANALOG', '📷', '#525252', 'ISO 800 · Still developing',          1, 0,
    '{"design_style":"Film photography / soft-stitch era. Silver-halide grain texture, frame counter glyph, exposure metadata stamp. Hand-developed feel.","lifestyle_scene":"Tokyo darkroom amber safelight, person holding wet print over developing tray, hanging negatives behind, contact sheets on wall","ink_default":"white"}'),
  ('quiet',  'MU × QUIET',  '🤫', '#374151', 'Do not disturb · Deep work',          1, 0,
    '{"design_style":"Introvert / deep-work culture. Minimal sans-serif + music rest notation + ''absence of sound'' kanji 静. Library calm aesthetic.","lifestyle_scene":"Tokyo library reading room early morning, single person at long wooden desk with closed laptop and a single notebook, soft natural light","ink_default":"white"}'),
  ('roam',   'MU × ROAM',   '🚶', '#0f766e', 'VISA RUN · 路 · GMT+0',                1, 0,
    '{"design_style":"Traveler / nomad print. Border-stamp typography + visa-page motifs + path 路 calligraphy. Earthy palette.","lifestyle_scene":"Narita Airport pre-dawn departures hall, person with duffel bag at empty check-in counter, soft blue cold light, suitcase tag visible","ink_default":"white"}');

INSERT OR REPLACE INTO catalog_products (sku, brand, label, description_ja, retail_price_jpy, printful_product_id, printful_variant_id, printful_placement, printful_print_w, printful_print_h, is_active, sort_order, status, fulfillment_route) VALUES
  ('ANIME-TEE-01',   'anime',  'FINAL ARC',    'MU × ANIME · FINAL ARC',    3900,   71,  4017, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('ANIME-HOOD-01',  'anime',  'EP. 1024',     'MU × ANIME · EP. 1024',     9800, 1543, 48770, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('ANIME-POST-01',  'anime',  'RE-WATCH',     'MU × ANIME · RE-WATCH',     2900,  171,  4530, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('ANIME-STICK-01', 'anime',  '戸惑い',         'MU × ANIME · 戸惑い',        800,  358, 10164, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('WAGYU-TEE-01',   'wagyu',  'A5',           'MU × WAGYU · A5',           3900,   71,  4017, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('WAGYU-APRON-01', 'wagyu',  '霜降り',         'MU × WAGYU · 霜降り',       4900,  297,  9287, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('WAGYU-MUG-01',   'wagyu',  '炭火',           'MU × WAGYU · 炭火',         2200,   19,  1320, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('WAGYU-TOTE-01',  'wagyu',  '脂 = 静寂',      'MU × WAGYU · 脂 = 静寂',     2900,   19,  1320, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('ANALOG-TEE-01',  'analog', 'ISO 800',      'MU × ANALOG · ISO 800',     3900,   71,  4017, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('ANALOG-TOTE-01', 'analog', '1/125s',       'MU × ANALOG · 1/125s',      2900,   19,  1320, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('ANALOG-JOUR-01', 'analog', '現像中',         'MU × ANALOG · 現像中',       3500,   19,  1320, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('ANALOG-STICK-01','analog', '35mm forever', 'MU × ANALOG · 35mm forever',  800,  358, 10164, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('QUIET-HOOD-01',  'quiet',  'DO NOT DISTURB','MU × QUIET · DO NOT DISTURB',9800, 1543, 48770, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('QUIET-TEE-01',   'quiet',  'DEEP WORK',    'MU × QUIET · DEEP WORK',    3900,   71,  4017, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('QUIET-MUG-01',   'quiet',  '音の不在',       'MU × QUIET · 音の不在',     2200,   19,  1320, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('QUIET-JOUR-01',  'quiet',  'off the grid', 'MU × QUIET · off the grid', 3500,   19,  1320, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('ROAM-TEE-01',    'roam',   'VISA RUN',     'MU × ROAM · VISA RUN',      3900,   71,  4017, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('ROAM-HOOD-01',   'roam',   '路',            'MU × ROAM · 路',             9800, 1543, 48770, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('ROAM-CAP-01',    'roam',   'GMT+0',        'MU × ROAM · GMT+0',         3500,  438, 12736, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg'),
  ('ROAM-TOTE-01',   'roam',   '間',            'MU × ROAM · 間',             2900,   19,  1320, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg');
