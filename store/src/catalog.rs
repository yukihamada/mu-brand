// catalog.rs — unified POD catalog (absorbs merch-bridge / merch.wearmu.com).
//
// Why this module exists:
//   Until 2026-05-22 the POD catalog (1,500+ SKUs across MU × bjj / kokon /
//   jiuflow / etc.) ran as a separate Python Flask app at merch.wearmu.com.
//   Two apps = two admins / two webhooks / two ways for customers to land.
//   This module pulls that whole surface into wearmu Rust:
//
//     - catalog_brands / catalog_products / catalog_product_extras / catalog_orders
//       tables (idempotent CREATE on startup)
//     - Bundled seed SQL (migrations/catalog_seed.sql) replays the merch-bridge
//       data — INSERT OR IGNORE so the wearmu DB becomes the source of truth
//       after first boot; further updates land directly here, not in Python.
//     - GET /shop and /shop/:sku — public storefront
//     - GET /api/shop/checkout?sku=… — Stripe Session via the pre-created
//       stripe_price_id (matches merch-bridge URL contract so existing ads
//       and emails keep working).
//     - fulfill_catalog_order() — called from the central stripe_webhook
//       when checkout.session.completed metadata.kind = "catalog". Posts to
//       Printful /orders?confirm=true with the JP→ISO state normalization
//       and the customer-selected size variant override.
//
// merch-bridge stays running as a hot standby during cutover; once /shop
// has taken real orders cleanly we DNS-flip merch.wearmu.com → wearmu.com/shop
// and the Python repo can be archived.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use std::env;

use crate::Db;

// ─── Schema + seed ────────────────────────────────────────────────────

const SEED_SQL: &str = include_str!("../migrations/catalog_seed.sql");
const ROLL_SEED_SQL: &str = include_str!("../migrations/roll_seed.sql");
const ATSUME_SEED_SQL: &str = include_str!("../migrations/atsume_seed.sql");
const YUMA_SEED_SQL: &str = include_str!("../migrations/yuma_seed.sql");
const ELEPOTE_SEED_SQL: &str = include_str!("../migrations/elepote_seed.sql");
const HALO_SEED_SQL: &str = include_str!("../migrations/halo_seed.sql");
const MUON_SEED_SQL: &str = include_str!("../migrations/muon_seed.sql");
const SHIOPIXEL_SEED_SQL: &str = include_str!("../migrations/shiopixel_seed.sql");

pub fn ensure_schema(conn: &rusqlite::Connection) {
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS catalog_brands (
            slug              TEXT PRIMARY KEY,
            name              TEXT NOT NULL,
            emoji             TEXT,
            color_primary     TEXT NOT NULL DEFAULT '#888',
            tagline           TEXT,
            custom_domain     TEXT,
            is_active         INTEGER NOT NULL DEFAULT 1,
            revenue_share_pct INTEGER NOT NULL DEFAULT 0,
            config_json       TEXT
         );
         CREATE TABLE IF NOT EXISTS catalog_products (
            sku                       TEXT PRIMARY KEY,
            brand                     TEXT NOT NULL,
            label                     TEXT NOT NULL,
            description_ja            TEXT NOT NULL,
            retail_price_jpy          INTEGER NOT NULL,
            printful_product_id       INTEGER NOT NULL,
            printful_variant_id       INTEGER NOT NULL,
            printful_placement        TEXT NOT NULL DEFAULT 'front',
            printful_print_w          INTEGER NOT NULL DEFAULT 0,
            printful_print_h          INTEGER NOT NULL DEFAULT 0,
            printful_sync_product_id  INTEGER,
            printful_sync_variant_id  INTEGER,
            stripe_product_id         TEXT,
            stripe_price_id           TEXT,
            design_file               TEXT,
            mockup_main_file          TEXT,
            mockup_url_external       TEXT,
            suzuri_url                TEXT,
            is_active                 INTEGER NOT NULL DEFAULT 1,
            sort_order                INTEGER NOT NULL DEFAULT 100,
            created_at                TEXT DEFAULT (datetime('now')),
            updated_at                TEXT DEFAULT (datetime('now'))
         );
         CREATE INDEX IF NOT EXISTS idx_catprod_brand_active
             ON catalog_products(brand, is_active, sort_order);
         CREATE TABLE IF NOT EXISTS catalog_product_extras (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            sku         TEXT NOT NULL,
            label       TEXT,
            image_url   TEXT NOT NULL,
            sort_order  INTEGER DEFAULT 100
         );
         CREATE INDEX IF NOT EXISTS idx_catextras_sku
             ON catalog_product_extras(sku);
         CREATE TABLE IF NOT EXISTS catalog_orders (
            id                     INTEGER PRIMARY KEY AUTOINCREMENT,
            stripe_session_id      TEXT UNIQUE NOT NULL,
            sku                    TEXT,
            amount_jpy             INTEGER,
            customer_email         TEXT,
            customer_name          TEXT,
            shipping_address_json  TEXT,
            printful_order_id      TEXT,
            printful_response_json TEXT,
            status                 TEXT,
            created_at             TEXT DEFAULT (datetime('now'))
         );
         CREATE INDEX IF NOT EXISTS idx_catorders_session
             ON catalog_orders(stripe_session_id);
         CREATE TABLE IF NOT EXISTS catalog_founder_cards (
            number              INTEGER PRIMARY KEY,  -- 1..100
            stripe_session_id   TEXT UNIQUE NOT NULL,
            sku                 TEXT,
            customer_email      TEXT NOT NULL,
            customer_name       TEXT,
            ship_address_json   TEXT,
            assigned_at         TEXT NOT NULL DEFAULT (datetime('now')),
            mailed_at           TEXT  -- set by Yuki when he posts the signed card
         );
         CREATE INDEX IF NOT EXISTS idx_founder_cards_email
             ON catalog_founder_cards(customer_email);
         CREATE TABLE IF NOT EXISTS catalog_return_requests (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            order_ref       TEXT NOT NULL,        -- order number / stripe session the customer typed
            customer_email  TEXT NOT NULL,
            reason          TEXT NOT NULL,
            photo_url       TEXT,                 -- optional evidence link
            client_ip       TEXT NOT NULL,        -- fly-validated client IP
            -- 'received'    = first-time IP, auto-accepted (refund still manual)
            -- 'needs_review'= a prior request exists from this IP → Yuki confirms
            status          TEXT NOT NULL DEFAULT 'received',
            created_at      TEXT DEFAULT (datetime('now'))
         );
         CREATE INDEX IF NOT EXISTS idx_return_requests_ip
             ON catalog_return_requests(client_ip);
         "
    );

    // Idempotent ALTER for the V1 catalog contract additions (see
    // docs/CATALOG_CONTRACT.md). Runs AFTER the CREATE TABLEs so a fresh
    // DB picks up the new columns (the ALTER is a no-op on a missing
    // table, so order matters). SQLite has no IF NOT EXISTS on ALTER —
    // each call's duplicate-column error is silently swallowed on re-run.
    let _ = conn.execute("ALTER TABLE catalog_brands   ADD COLUMN config_json TEXT", []);
    let _ = conn.execute("ALTER TABLE catalog_products ADD COLUMN status TEXT NOT NULL DEFAULT 'live'", []);
    let _ = conn.execute("ALTER TABLE catalog_products ADD COLUMN fulfillment_route TEXT NOT NULL DEFAULT 'printful_dtg'", []);
    let _ = conn.execute("ALTER TABLE catalog_products ADD COLUMN legacy_source TEXT", []);
    // Cross-sell add-on: the optional 2nd SKU fulfilled alongside the main
    // SKU in a single Printful order. NULL for every existing single-SKU
    // order (full backward compat). Single column per the catalog contract
    // (no new per-type table).
    let _ = conn.execute("ALTER TABLE catalog_orders ADD COLUMN addon_sku TEXT", []);
    // Event tickets (fulfillment_route='digital'): per-product capacity +
    // event metadata live in ONE general JSON column, not per-attribute
    // columns (catalog contract: extend, don't add a column per concept).
    // `{"capacity": 50}` etc. NULL for every non-ticket SKU.
    let _ = conn.execute("ALTER TABLE catalog_products ADD COLUMN meta_json TEXT", []);
    // The unique ticket code issued per paid seat — encoded in the QR and
    // reverse-looked-up by the public /t/:code gate. NULL for physical orders.
    let _ = conn.execute("ALTER TABLE catalog_orders ADD COLUMN ticket_code TEXT", []);
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_catorders_ticket ON catalog_orders(ticket_code)", []);
    // Affiliate attribution: which referral code drove this sale + the
    // commission credited to the referrer (also written to mu_credit_ledger,
    // the payout source of truth). NULL/0 for unattributed orders.
    let _ = conn.execute("ALTER TABLE catalog_orders ADD COLUMN referrer_code TEXT", []);
    let _ = conn.execute("ALTER TABLE catalog_orders ADD COLUMN commission_jpy INTEGER NOT NULL DEFAULT 0", []);
}

/// How many founder cards are still available (0..100).
pub fn founder_cards_remaining(conn: &rusqlite::Connection) -> i64 {
    let used: i64 = conn
        .query_row("SELECT COUNT(*) FROM catalog_founder_cards", [], |r| r.get(0))
        .unwrap_or(0);
    (100 - used).max(0)
}

/// Idempotent seeder for the ROLL ◐ MU brand (1 brand + 20 products).
/// Runs the full SQL on every boot — the brand row uses ON CONFLICT
/// DO UPDATE so config_json edits land each release, and product inserts
/// use INSERT OR IGNORE so existing rows stay intact.
///
/// Para-BJJ first edition. See /static/roll/index.html and
/// /static/roll/designs.json for the curated design briefs.
pub fn seed_roll_brand(conn: &rusqlite::Connection) {
    match conn.execute_batch(ROLL_SEED_SQL) {
        Ok(()) => {
            let n: i64 = conn
                .query_row("SELECT COUNT(*) FROM catalog_products WHERE brand='roll'", [], |r| r.get(0))
                .unwrap_or(0);
            tracing::info!("[catalog] ROLL brand upserted · {} products live", n);
        }
        Err(e) => tracing::error!("[catalog] roll seed failed: {}", e),
    }
}

/// MU × ATSUME dev-team collab. UPSERTs the `atsume` brand row + INSERT OR
/// IGNORE its products on every boot (mirrors seed_roll_brand). The DEV
/// mascot tee ships `live`; the four ATSUME-app tees stay `review` until the
/// partner's real logo files land and they're flipped to `live`.
pub fn seed_atsume_brand(conn: &rusqlite::Connection) {
    match conn.execute_batch(ATSUME_SEED_SQL) {
        Ok(()) => {
            let n: i64 = conn
                .query_row("SELECT COUNT(*) FROM catalog_products WHERE brand='atsume' AND status='live'", [], |r| r.get(0))
                .unwrap_or(0);
            tracing::info!("[catalog] ATSUME brand upserted · {} products live", n);
        }
        Err(e) => tracing::error!("[catalog] atsume seed failed: {}", e),
    }
}

/// MU × YUMA — 碧 (AO) tax-accountant line. UPSERTs the `yuma` brand + INSERT
/// OR IGNORE its 4 products on boot (mirrors seed_roll_brand). All 4 are
/// MU-original designs (碧 + 税理士 phrases) so they ship `live` & buyable.
pub fn seed_yuma_brand(conn: &rusqlite::Connection) {
    match conn.execute_batch(YUMA_SEED_SQL) {
        Ok(()) => {
            let n: i64 = conn
                .query_row("SELECT COUNT(*) FROM catalog_products WHERE brand='yuma' AND status='live'", [], |r| r.get(0))
                .unwrap_or(0);
            tracing::info!("[catalog] YUMA brand upserted · {} products live", n);
        }
        Err(e) => tracing::error!("[catalog] yuma seed failed: {}", e),
    }
}

/// MU × ELE × POTE — personal pets (Ele = Bichon-Poo, Pote = Frenchie).
/// 9 buyable SKUs across tee/hoodie/mug/tote/sticker, all MU-original art
/// generated from the actual dog photos.
pub fn seed_elepote_brand(conn: &rusqlite::Connection) {
    match conn.execute_batch(ELEPOTE_SEED_SQL) {
        Ok(()) => {
            let n: i64 = conn
                .query_row("SELECT COUNT(*) FROM catalog_products WHERE brand='elepote' AND status='live'", [], |r| r.get(0))
                .unwrap_or(0);
            tracing::info!("[catalog] ELEPOTE brand upserted · {} products live", n);
        }
        Err(e) => tracing::error!("[catalog] elepote seed failed: {}", e),
    }
}

/// HALO — private message tees (無 / 引き算 / 月 / 島). Pure MU-original
/// typography, no partner logo/IP. All 13 designs × S/M/L seed as
/// `is_active=0` / `status='draft'` so they NEVER surface on /shop,
/// /sitemap, or new-arrivals. They are viewable + buyable ONLY through
/// the gift gallery at `/gift/:key` (gated by env `MU_GIFT_KEY`), which
/// passes the same key to `/api/shop/checkout?...&key=` to unlock the
/// otherwise-hidden SKU. Fulfillment is the standard Printful DTG path.
pub fn seed_halo_brand(conn: &rusqlite::Connection) {
    match conn.execute_batch(HALO_SEED_SQL) {
        Ok(()) => {
            let n: i64 = conn
                .query_row("SELECT COUNT(*) FROM catalog_products WHERE brand='halo'", [], |r| r.get(0))
                .unwrap_or(0);
            tracing::info!("[catalog] HALO private tees upserted · {} hidden SKUs", n);
        }
        Err(e) => tracing::error!("[catalog] halo seed failed: {}", e),
    }
}

/// MUON 無音 — public message-tee collection (墨黒×明朝, deadpan).
/// Seeded as status='draft'/is_active=0 → hidden from /shop until go-live.
/// Brand row + N catalog_products in one upsert (catalog contract).
pub fn seed_muon_brand(conn: &rusqlite::Connection) {
    match conn.execute_batch(MUON_SEED_SQL) {
        Ok(()) => {
            let n: i64 = conn
                .query_row("SELECT COUNT(*) FROM catalog_products WHERE brand='muon'", [], |r| r.get(0))
                .unwrap_or(0);
            tracing::info!("[catalog] MUON tees upserted · {} SKUs (live)", n);
        }
        Err(e) => tracing::error!("[catalog] muon seed failed: {}", e),
    }
}

pub fn seed_shiopixel_brand(conn: &rusqlite::Connection) {
    match conn.execute_batch(SHIOPIXEL_SEED_SQL) {
        Ok(()) => {
            let n: i64 = conn
                .query_row("SELECT COUNT(*) FROM catalog_products WHERE brand='shiopixel'", [], |r| r.get(0))
                .unwrap_or(0);
            tracing::info!("[catalog] Shiopixel song-tees upserted · {} SKUs (live)", n);
        }
        Err(e) => tracing::error!("[catalog] shiopixel seed failed: {}", e),
    }
}

/// Gift-link gate. True only when env `MU_GIFT_KEY` is set (non-empty)
/// AND the supplied key matches it exactly. Closed-by-default: if the
/// secret is unset, no key is ever valid. Used to expose the hidden
/// 'halo' tees for view (/gift/:key) + purchase (checkout ?key=).
fn gift_key_valid(key: Option<&str>) -> bool {
    let secret = std::env::var("MU_GIFT_KEY").unwrap_or_default();
    !secret.is_empty() && key.map(|k| k == secret).unwrap_or(false)
}

/// Seed the universal MU mark (━◯━) kiss-cut sticker. This is the
/// fallback cross-sell add-on (shop_pdp) for every brand that lacks its
/// own ¥800 sticker — i.e. almost all of them (bjj/coffee/moon/code/…),
/// so the in-order AOV cross-sell fires across the whole catalog instead
/// of only the 3 collab brands that happen to ship a sticker.
/// Printful 358/10164 (Kiss-Cut 4×4) is the same SKU the elepote stickers
/// use and is fulfillment-validated. Design is a flat gold MU mark on
/// transparent at /static/mu/d/mu-mark-sticker.png (git-deployed, so
/// Printful can fetch it). INSERT OR IGNORE → idempotent on every boot.
pub fn seed_mu_sticker(conn: &rusqlite::Connection) {
    let r = conn.execute(
        "INSERT OR IGNORE INTO catalog_products
           (sku, brand, label, description_ja, retail_price_jpy,
            printful_product_id, printful_variant_id, printful_placement,
            printful_print_w, printful_print_h,
            printful_sync_product_id, printful_sync_variant_id,
            stripe_product_id, stripe_price_id,
            design_file, mockup_main_file, mockup_url_external,
            suzuri_url, is_active, sort_order, status, fulfillment_route)
         VALUES
           ('MU-STICKER-MARK', 'mu', 'MU Sticker',
            'MU ━◯━ キスカットステッカー 4×4',
            800, 358, 10164, 'default', 0, 0, NULL, NULL, NULL, NULL,
            '/static/mu/d/mu-mark-sticker.png',
            '/static/mu/d/mu-mark-sticker.png',
            'https://wearmu.com/static/mu/d/mu-mark-sticker.png',
            NULL, 1, 50, 'live', 'printful_dtg')",
        [],
    );
    match r {
        Ok(_) => tracing::info!("[catalog] MU mark sticker seeded (cross-sell fallback)"),
        Err(e) => tracing::error!("[catalog] mu sticker seed failed: {}", e),
    }
}

/// One-shot async backfill: for every ROLL SKU whose mockup is still the
/// typography preview PNG (not a real on-body Printful render), call the
/// Printful Mockup Generator with the design PNG and update
/// `mockup_url_external` to the resulting model-wearing-shirt photo.
///
/// Spawned in the background by main() after seed_roll_brand so boot is
/// non-blocking. Generator basic single-front is free; sleep 3s between
/// SKUs to be polite to the queue.
///
/// Detection: a "real" Printful mockup URL contains either
/// `files.cdn.printful.com` (direct Printful CDN) or our R2 mirror.
/// Typography previews live under `/roll/mockups/preview_…` or our
/// `wearmu.com/roll/mockups/preview_…` mirror — those trigger backfill.
pub fn spawn_roll_mockup_backfill(db: Db) {
    tokio::spawn(async move {
        // Wait a bit so the boot logs are clean and the LP is already serving.
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;

        if std::env::var("PRINTFUL_API_KEY").unwrap_or_default().is_empty() {
            tracing::warn!("[catalog/roll-mockups] PRINTFUL_API_KEY unset — skipping");
            return;
        }

        let pending: Vec<(String, i64, i64)> = {
            let conn = db.lock().unwrap();
            let mut stmt = match conn.prepare(
                "SELECT sku, printful_product_id, printful_variant_id
                 FROM catalog_products
                 WHERE brand='roll'
                   AND (mockup_url_external IS NULL
                        OR (mockup_url_external NOT LIKE '%files.cdn.printful.com%'
                            AND mockup_url_external NOT LIKE '%r2.dev%'
                            AND mockup_url_external NOT LIKE '%r2.cloudflarestorage%'))
                 ORDER BY sort_order, sku"
            ) { Ok(s) => s, Err(e) => { tracing::error!("[catalog/roll-mockups] prepare: {}", e); return; } };
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .and_then(|it| it.collect::<Result<Vec<_>, _>>())
                .unwrap_or_default()
        };

        if pending.is_empty() {
            tracing::info!("[catalog/roll-mockups] all 20 SKUs already have real mockups");
            return;
        }
        tracing::info!("[catalog/roll-mockups] backfilling {} SKUs…", pending.len());

        let base = std::env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
        let mut ok = 0;
        let mut err = 0;
        for (sku, prod, var) in &pending {
            let design_url = format!("{}/static/roll/d/design_{}.png", base, sku);
            match generate_onbody_mockup(db.clone(), sku.clone(), *prod, *var, design_url).await {
                Ok(()) => { ok += 1; tracing::info!("[catalog/roll-mockups] {} OK", sku); }
                Err(e) => { err += 1; tracing::warn!("[catalog/roll-mockups] {} FAIL: {}", sku, e); }
            }
            // Be polite to Printful's mockup queue.
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
        tracing::info!("[catalog/roll-mockups] done · ok={} err={}", ok, err);
    });
}

/// One-shot migration: fix the wrong printful_product_id (162 →
/// Bella+Canvas longsleeve) stamped on existing rashguard rows. The
/// real product for variant 9328 is 301 (Men's AOP Rash Guard). Stale
/// 162 rows were causing both the mockup-generator backfill AND any
/// future fulfill_catalog_order POST to 4xx with "No variants to
/// generate". Idempotent (UPDATE only matches the broken rows).
pub fn migrate_rashguard_product_id(conn: &rusqlite::Connection) {
    let n = conn.execute(
        "UPDATE catalog_products
         SET printful_product_id = 301
         WHERE brand='auto'
           AND sku LIKE '%RASHGUARD%'
           AND printful_product_id = 162",
        [],
    ).unwrap_or(0);
    if n > 0 {
        tracing::info!("[catalog] migrate_rashguard_product_id: fixed {} rows", n);
    }
}

/// One-shot migration: retire legacy `MU-<BRAND>-NN-*` seed SKUs whose
/// `mockup_main_file` points at `/static/collections/<brand>/mockup_*.jpg`
/// — files that no longer exist on disk (verified 404 on both wearmu.com
/// and merch.wearmu.com on 2026-05-23). Their PDPs already 404, so they
/// are unreachable and unbuyable, but they were never flipped to
/// `status='retired'`, which left them polluting `/admin/products` with
/// broken-image rows. First pass on 2026-05-23 retired 989 `brand=bjj`
/// rows; second pass extends the same logic to code/coffee/zen (203 more,
/// all 404-verified) and any future cousins via the brand-agnostic LIKE.
/// Idempotent: only matches rows still flagged `is_active=1` with the
/// broken static path AND no working external mockup URL.
pub fn retire_dead_static_collection_mockups(conn: &rusqlite::Connection) {
    let n = conn.execute(
        "UPDATE catalog_products
         SET status='retired', is_active=0
         WHERE is_active = 1
           AND (mockup_url_external IS NULL OR mockup_url_external = '')
           AND mockup_main_file LIKE '/static/collections/%'",
        [],
    ).unwrap_or(0);
    if n > 0 {
        tracing::info!("[catalog] retire_dead_static_collection_mockups: retired {} rows", n);
    }
}

/// One-shot migration: retire SKUs that have ZERO usable images — empty
/// `mockup_url_external`, empty `mockup_main_file`, and no
/// `catalog_product_extras` row with an http-URL image. PDP-404 verified
/// on 2026-05-24 for 52 such rows across 13 brands (analog/anime/chip/
/// founder/kagi/lodge/news/ocean/octagon/quiet/roam/voice/wagyu). These
/// were seed rows for brands whose creative work was never completed,
/// so the products are functionally unsellable and just inflate the
/// admin score board. Idempotent (is_active=1 filter); brand-agnostic
/// so it also catches any future imageless seeds that slip through.
pub fn retire_imageless_products(conn: &rusqlite::Connection) {
    let n = conn.execute(
        "UPDATE catalog_products
         SET status='retired', is_active=0
         WHERE is_active = 1
           AND (mockup_url_external IS NULL OR mockup_url_external = '')
           AND (mockup_main_file IS NULL OR mockup_main_file = '')
           AND NOT EXISTS (
             SELECT 1 FROM catalog_product_extras ex
             WHERE ex.sku = catalog_products.sku
               AND ex.image_url IS NOT NULL
               AND ex.image_url LIKE 'http%'
           )",
        [],
    ).unwrap_or(0);
    if n > 0 {
        tracing::info!("[catalog] retire_imageless_products: retired {} rows", n);
    }
}

/// One-shot migration: fix hoodie + crewneck variant IDs that were wrong
/// in the original PRODUCT_SPECS seed:
///   - Hoodie product 146 used variant 5530, which is actually Black/S
///     (correct Black/M is 5531). Symptom: customers received the wrong
///     size; mockups still rendered though, masking the bug.
///   - Crewneck product 145 used variant 5403, which does not exist in
///     product 145 at all. Symptom: 100% mockup-generation failure
///     ("No variants to generate" from Printful), so all 11 crewneck
///     SKUs landed without on-body photos.
/// Both verified against the Printful API on 2026-05-24. Idempotent.
pub fn migrate_hoodie_crewneck_variants(conn: &rusqlite::Connection) {
    let h = conn.execute(
        "UPDATE catalog_products SET printful_variant_id = 5531
         WHERE printful_product_id = 146 AND printful_variant_id = 5530",
        [],
    ).unwrap_or(0);
    // For crewnecks the wrong variant_id (5403) caused every mockup gen
    // attempt to fail, which the stale_sku_killer then auto-retired after
    // 5 failures. So in addition to fixing the variant, un-retire the rows
    // and reset mockup_url_external to design_file so mockup_backfill_step
    // picks them up next cron tick. (The backfill cron uses
    // mockup_url_external == design_file as its "needs work" heuristic.)
    let c = conn.execute(
        "UPDATE catalog_products
         SET printful_variant_id = 5435,
             status = 'live',
             is_active = 1,
             mockup_url_external = design_file
         WHERE printful_product_id = 145 AND printful_variant_id = 5403",
        [],
    ).unwrap_or(0);
    // The fix above only touched printful_variant_id (the base/default
    // variant). But fulfillment resolves size→variant from
    // printful_variant_map FIRST (main.rs ~19422), and only falls back to
    // the base column when the size key is ABSENT. Since the map carries
    // every size, the base-column fix was bypassed for any sized order:
    //   - Crewneck (145) maps held 5384–5388, none of which exist in
    //     Printful (404) → the order is rejected at fulfillment.
    //   - Hoodie (146) map "3XL":5534 is actually 2XL (real 3XL = 5535) →
    //     a 3XL order ships a 2XL.
    // Rewrite the maps to the API-verified IDs (GET /products/145,/146 on
    // 2026-05-30). Targeted + idempotent: only rows still carrying the bad
    // IDs are touched.
    let cm = conn.execute(
        r#"UPDATE catalog_products
           SET printful_variant_map =
               '{"S":5434,"M":5435,"L":5436,"XL":5437,"2XL":5438,"OS":5435,"ONE SIZE":5435,"XS":5434,"3XL":5439}'
           WHERE printful_product_id = 145
             AND printful_variant_map LIKE '%5384%'"#,
        [],
    ).unwrap_or(0);
    // Surgical substring swap so "2XL":5534 (correct) is left untouched.
    let hm = conn.execute(
        r#"UPDATE catalog_products
           SET printful_variant_map =
               replace(printful_variant_map, '"3XL":5534', '"3XL":5535')
           WHERE printful_product_id = 146
             AND printful_variant_map LIKE '%"3XL":5534%'"#,
        [],
    ).unwrap_or(0);
    if h > 0 || c > 0 || cm > 0 || hm > 0 {
        tracing::info!(
            "[catalog] migrate_hoodie_crewneck_variants: fixed {} hoodie + {} crewneck base, {} crewneck + {} hoodie maps",
            h, c, cm, hm
        );
    }
}

/// One-shot migration: retire 7 belt-rashguard SKUs that were superseded
/// by the Phase B full-canvas regeneration. Five V1 chest-graphic SKUs and
/// two V2 SKUs where Gemini drifted off the target color (brown→near-black,
/// black→navy) — all replaced by cleaner V3 renders that ship as the
/// canonical 5-belt line. Idempotent: only flips rows still active.
pub fn retire_superseded_belt_rashguards(conn: &rusqlite::Connection) {
    const DEAD_SKUS: &[&str] = &[
        // V1 — chest-graphic on white (placement=front-only before Phase B)
        "AUTO-NL-NL-RASHGUARD-LS-nladd35715",
        "AUTO-NL-BLUEBELT-RASHGUARD-LS-nl6b349690",
        "AUTO-NL-PURPLEBELT-RASHGUARD-LS-nl1e0647f1",
        "AUTO-NL-BROWNBELT-RASHGUARD-LS-nlc9f0eaac",
        "AUTO-NL-BLACKBELT-RASHGUARD-BLACK-nl777c35ec",
        // V2 — full-canvas, but brown/black drifted off color in Gemini
        "AUTO-NL-BROWNBELT-RASHGUARD-LS-nldbb4f30a",
        "AUTO-NL-BLACKBELT-RASHGUARD-BLACK-nlc695e54b",
    ];
    let placeholders = vec!["?"; DEAD_SKUS.len()].join(",");
    let sql = format!(
        "UPDATE catalog_products SET status='retired', is_active=0 \
         WHERE is_active = 1 AND sku IN ({})",
        placeholders
    );
    let params: Vec<&dyn rusqlite::ToSql> =
        DEAD_SKUS.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    let n = conn.execute(&sql, params.as_slice()).unwrap_or(0);
    if n > 0 {
        tracing::info!("[catalog] retire_superseded_belt_rashguards: retired {} rows", n);
    }
}

/// One-shot migration: rewrite the mechanical "BJJ 黒帯 · T シャツ"
/// descriptions on existing AUTO SKUs to use the theme hook copy
/// ("BJJ 黒帯 — 黒帯への 10 年を …"). Safe to re-run; each row matches
/// at most one theme.
pub fn migrate_auto_labels(conn: &rusqlite::Connection) {
    for t in SEED_THEMES {
        let prefix = format!("AUTO-{}-", t.slug.to_uppercase().replace('_', "-"));
        let new_desc = format!("{} — {}", t.display, t.hook);
        let _ = conn.execute(
            "UPDATE catalog_products
             SET label=?, description_ja=?
             WHERE brand='auto' AND sku LIKE ?
               AND description_ja LIKE '% · %'",
            rusqlite::params![&new_desc, &new_desc, format!("{}%", prefix)],
        );
    }
}

/// Phase A of the contract migration (docs/CATALOG_CONTRACT.md).
/// Shadow-write legacy product surfaces into catalog_products so the
/// rest of wearmu can read from one place going forward.
///
/// Strictly additive — reads on proposal_skus / collab_products still
/// work; we just mirror their rows into catalog_products with
/// brand="proposal:<slug>" or brand=<partner>, status='live' or 'draft'
/// based on the legacy approval flag, and legacy_source set so a future
/// reconciliation pass knows where each row came from.
///
/// Idempotent via INSERT OR IGNORE on the catalog_products.sku PK.
pub fn migrate_legacy_to_catalog(conn: &rusqlite::Connection) {
    // proposal_skus → catalog_products. The legacy PK is (slug, letter);
    // we synthesize a deterministic sku "PROPOSAL-<SLUG>-<LETTER>".
    let n_proposal: i64 = conn
        .execute(
            "INSERT OR IGNORE INTO catalog_products
                (sku, brand, label, description_ja, retail_price_jpy,
                 printful_product_id, printful_variant_id, printful_placement,
                 printful_print_w, printful_print_h,
                 design_file, mockup_main_file, mockup_url_external,
                 is_active, sort_order, status, fulfillment_route, legacy_source)
             SELECT 'PROPOSAL-' || UPPER(slug) || '-' || UPPER(letter),
                    'proposal:' || slug,
                    label, label, price_jpy,
                    71, 4017, 'front', 0, 0,
                    design_url, design_url, design_url,
                    CASE WHEN published=1 THEN 1 ELSE 0 END,
                    100,
                    CASE WHEN published=1 THEN 'live' ELSE 'draft' END,
                    'printful_dtg',
                    'proposal_skus'
             FROM proposal_skus
             WHERE design_url IS NOT NULL AND design_url != ''",
            [],
        )
        .unwrap_or(0) as i64;

    // collab_products → catalog_products. Legacy PK is (slug UNIQUE);
    // synthesize "COLLAB-<PARTNER>-<SLUG>".
    let n_collab: i64 = conn
        .execute(
            "INSERT OR IGNORE INTO catalog_products
                (sku, brand, label, description_ja, retail_price_jpy,
                 printful_product_id, printful_variant_id, printful_placement,
                 printful_print_w, printful_print_h,
                 design_file, mockup_main_file, mockup_url_external,
                 is_active, sort_order, status, fulfillment_route, legacy_source)
             SELECT 'COLLAB-' || UPPER(partner) || '-' || UPPER(slug),
                    partner,
                    name, COALESCE(description, name), price_jpy,
                    COALESCE(printful_product_id, 71),
                    COALESCE(printful_variant_id, 4017),
                    'front', 0, 0,
                    image_url, image_url, image_url,
                    CASE WHEN active=1 AND draft=0 THEN 1 ELSE 0 END,
                    100,
                    CASE
                      WHEN active=1 AND draft=0 AND partner_approved=1 THEN 'live'
                      WHEN partner_approved=0 THEN 'review'
                      WHEN draft=1 THEN 'draft'
                      ELSE 'approved'
                    END,
                    CASE production_route
                      WHEN 'printful' THEN 'printful_dtg'
                      WHEN 'sweep_manual' THEN 'manual'
                      ELSE 'manual'
                    END,
                    'collab_products'
             FROM collab_products
             WHERE image_url IS NOT NULL AND image_url != ''",
            [],
        )
        .unwrap_or(0) as i64;

    if n_proposal + n_collab > 0 {
        tracing::info!(
            "[catalog/migrate] phase A: proposal_skus={} collab_products={} mirrored into catalog_products",
            n_proposal, n_collab
        );
    }
}

/// Phase C of the migration: rename the legacy tables to
/// `_legacy_<name>` so reads start failing loudly (vs silently serving
/// stale data) and we can drop them after a 30-day soak. NEVER drops
/// — that's a separate manual step once we've watched logs for missed
/// reads.
///
/// Token-gated via /admin/catalog/legacy_rename so a stray crash-restart
/// can't trigger it accidentally.
pub fn rename_legacy_tables(conn: &rusqlite::Connection) -> Vec<(String, bool)> {
    let legacy = [
        // Per-partner approval queues — all empty (verified 2026-05-22)
        "kichinan_approval",
        "asoview_approval",
        "elsoul_approval",
        "ele_approval",
        "nojimahal_approval",
        "ryozo_approval",
        // Collab tables superseded by catalog_products (mirrored in Phase A)
        "collab_products",
        // Proposal table superseded by catalog_products
        "proposal_skus",
        // collab_account_deletions / collab_applications / collab_signups /
        // collab_users / collab_orders stay — they're orthogonal to product
        // data (auth + orders). collab_orders is a candidate for
        // backfilling into catalog_orders but Phase B owns that.
    ];
    let mut out = Vec::new();
    for t in legacy {
        let new = format!("_legacy_{}", t);
        let r = conn.execute(&format!("ALTER TABLE {} RENAME TO {}", t, new), []);
        out.push((t.to_string(), r.is_ok()));
    }
    out
}

/// Seed the catalog from the bundled SQL dump. Runs once if the catalog
/// is empty (and on every boot — the INSERT OR IGNORE makes it cheap to
/// re-run; we still gate on row count to avoid the file parse cost).
pub fn seed_if_empty(conn: &rusqlite::Connection) {
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM catalog_products", [], |r| r.get(0))
        .unwrap_or(0);
    if n > 0 {
        return;
    }
    match conn.execute_batch(SEED_SQL) {
        Ok(()) => {
            let n2: i64 = conn
                .query_row("SELECT COUNT(*) FROM catalog_products", [], |r| r.get(0))
                .unwrap_or(0);
            tracing::info!("[catalog] seeded {} products from migrations/catalog_seed.sql", n2);
        }
        Err(e) => tracing::error!("[catalog] seed failed: {}", e),
    }
}

// ─── Budget guard + spend ledger ──────────────────────────────────────
//
// Single hard-cap of ¥100,000 across the autonomous shop engine so a
// runaway loop can never burn unbounded cash. Every spend goes through
// spend_or_refuse() which returns false (and logs the refusal) when the
// running total would exceed the cap.
//
// Categories tracked:
//   ai_image    — Gemini image generation (~¥6/image at gemini-3-pro-image-preview)
//   printful    — sample orders + per-fulfillment fees
//   ads_google  — Google Ads campaign spend (set by external reconciler)
//   ads_meta    — Meta Ads spend
//   other       — anything not categorised

// Monthly budget cap. The guard (spend_or_refuse) sums only the CURRENT
// calendar month's catalog_spend rows, so this resets on the 1st of each
// month automatically — no ledger truncation needed. Operator-managed
// allocation + burn-down lives in BUDGET.md (source of truth for humans);
// this constant is the hard ceiling the engine enforces in code.
pub const BUDGET_TOTAL_JPY: i64 = 1_000_000;

pub fn ensure_budget_schema(conn: &rusqlite::Connection) {
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS catalog_spend (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            category    TEXT NOT NULL,
            amount_jpy  INTEGER NOT NULL,
            reason      TEXT,
            ref_id      TEXT,
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
         );
         CREATE INDEX IF NOT EXISTS idx_catspend_category
             ON catalog_spend(category, created_at);
         CREATE TABLE IF NOT EXISTS catalog_gen_jobs (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            theme        TEXT NOT NULL,
            kind         TEXT NOT NULL,
            seed         TEXT NOT NULL,
            status       TEXT NOT NULL DEFAULT 'pending',
            sku          TEXT,
            error        TEXT,
            spent_jpy    INTEGER NOT NULL DEFAULT 0,
            created_at   TEXT NOT NULL DEFAULT (datetime('now')),
            completed_at TEXT,
            UNIQUE(theme, kind, seed)
         );
         CREATE INDEX IF NOT EXISTS idx_catgen_status
             ON catalog_gen_jobs(status, created_at);
        "
    );
}

/// Total ¥ spent across all categories, all-time. Used for lifetime
/// reporting only — NOT the budget guard (that is monthly).
pub fn spent_total_jpy(conn: &rusqlite::Connection) -> i64 {
    conn.query_row("SELECT COALESCE(SUM(amount_jpy), 0) FROM catalog_spend",
                   [], |r| r.get::<_, i64>(0))
        .unwrap_or(0)
}

/// ¥ spent in the CURRENT calendar month. Source of truth for the budget
/// guard — resets automatically on the 1st (the ledger keeps all rows;
/// we just scope the SUM to this month, matching the ¥1M/month budget).
pub fn spent_month_jpy(conn: &rusqlite::Connection) -> i64 {
    conn.query_row(
        "SELECT COALESCE(SUM(amount_jpy), 0) FROM catalog_spend \
         WHERE strftime('%Y-%m', created_at) = strftime('%Y-%m', 'now')",
        [], |r| r.get::<_, i64>(0))
        .unwrap_or(0)
}

/// Attempt to charge `amount_jpy` against the budget. Returns true if
/// the spend was recorded; false (and logs a refusal) if it would push
/// us over BUDGET_TOTAL_JPY. Refusals are themselves recorded with a
/// negative-id row in a future iteration; for now we just log.
pub fn spend_or_refuse(
    conn: &rusqlite::Connection,
    category: &str,
    amount_jpy: i64,
    reason: &str,
    ref_id: Option<&str>,
) -> bool {
    if amount_jpy <= 0 {
        return true;
    }
    let current = spent_month_jpy(conn);
    if current.saturating_add(amount_jpy) > BUDGET_TOTAL_JPY {
        tracing::warn!(
            "[catalog/budget] REFUSED {} ¥{} (month=¥{} cap=¥{}/mo) reason={}",
            category, amount_jpy, current, BUDGET_TOTAL_JPY, reason
        );
        return false;
    }
    let _ = conn.execute(
        "INSERT INTO catalog_spend (category, amount_jpy, reason, ref_id)
         VALUES (?, ?, ?, ?)",
        rusqlite::params![category, amount_jpy, reason, ref_id],
    );
    tracing::info!(
        "[catalog/budget] +¥{} {} (month=¥{}/¥{}) reason={}",
        amount_jpy, category, current + amount_jpy, BUDGET_TOTAL_JPY, reason
    );
    true
}

// ─── Autonomous SKU generator (Gemini → R2 → catalog_products) ────────
//
// Why this exists: we need to mass-produce T-shirts and rashguards at a
// rate the 30-min optimizer cron can drive. Round-tripping the public
// /api/v1/sku/create from a Python script would (1) require auth keys
// in CI, (2) write into the legacy proposal_skus table (wrong target),
// (3) miss the budget guard. Doing it inline in Rust lets us:
//
//   • atomic budget check before each Gemini call (¥6 each)
//   • write straight into catalog_products with the right Printful
//     variant_id / placement so /api/shop/checkout + the webhook
//     fulfillment work end-to-end with NO Stripe-price pre-mint and NO
//     Printful sync-product round-trip (Path A: files-based)
//   • dedup via the (theme, kind, seed) UNIQUE in catalog_gen_jobs so
//     re-running is safe
//
// The product spec table below is small on purpose: T-shirt and AOP
// rashguard cover the two requests the user named. Adding hoodies /
// tanks etc. is a one-row PR away.

/// Printful product 301 (AOP Men's Rash Guard) has four sublimation panels.
/// A single placement = chest-only print on an otherwise-white rashguard.
/// To deliver a true belt-colored rashguard the same design URL must be
/// fanned out to every panel (cover-fill scales it per panel automatically).
/// Other apparel (tee/hoodie/crewneck) is single-front DTG.
fn placements_for_product(printful_product_id: i64) -> &'static [&'static str] {
    match printful_product_id {
        // 301 = Men's AOP Rash Guard, 302/368/369/836 = sister AOP products
        // (per fulfill_catalog_order's stitch_color guard at line 2736).
        301 | 302 | 368 | 369 | 836 => &["front", "back", "sleeve_left", "sleeve_right"],
        _ => &["front"],
    }
}

struct ProductSpec {
    kind: &'static str,
    printful_product_id: i64,
    printful_variant_id: i64, // unisex size M unless noted
    placement: &'static str,
    retail_jpy: i64,
    /// Marketing-grade spec line shown on the PDP (material / weight / fit
    /// / print method). Real BJJ buyers won't checkout without this.
    spec_html: &'static str,
}

// variant_id references mirror what's already proven in payments.rs and
// merch-bridge's seed data:
//   • Bella+Canvas 3001 unisex tee, size M, black: 4017
//     (see store/src/payments.rs:753)
//   • Men's AOP Rash Guard, size M: 9328
//     (see kichinan_rashguard_ls_sample in store/src/main.rs:18197)
const PRODUCT_SPECS: &[ProductSpec] = &[
    ProductSpec {
        kind: "tee",
        printful_product_id: 71,
        printful_variant_id: 4017, // Black M
        placement: "front",
        retail_jpy: 4900,
        spec_html: "Bella+Canvas 3001 unisex tee · Black · 4.2 oz (142 gsm) · \
                    100% airlume combed ringspun cotton · DTG print 30×30cm front · \
                    machine washable · sourced + printed in EU",
    },
    ProductSpec {
        kind: "tee_white",
        // Same Bella+Canvas 3001, White/M (variant 4012 verified against the
        // Printful API 2026-06-05; 87 live tees already use it). White garment
        // is the right canvas for dark line-art / sumi-e / Mincho graphics —
        // the white-bg DTG pipeline keys white→transparent, leaving the dark
        // artwork, which then reads perfectly on a white tee.
        printful_product_id: 71,
        printful_variant_id: 4012, // White M
        placement: "front",
        retail_jpy: 4900,
        spec_html: "Bella+Canvas 3001 unisex tee · White · 4.2 oz (142 gsm) · \
                    100% airlume combed ringspun cotton · DTG print 30×30cm front · \
                    machine washable · sourced + printed in EU",
    },
    ProductSpec {
        kind: "rashguard_ls",
        printful_product_id: 301, // All-Over Print Men's Rash Guard (white base; sublimation requires poly white)
        printful_variant_id: 9328, // White M
        placement: "front",
        retail_jpy: 9800,
        spec_html: "Men's all-over-print long-sleeve rashguard · 82% polyester / 18% spandex · \
                    UPF 50+ UV protection · 4-way stretch · flatlock seams (no chafe) · \
                    sublimation print (won't fade or peel) · IBJJF gi/no-gi compliant fit",
    },
    ProductSpec {
        kind: "rashguard_black",
        // Same Printful product as rashguard_ls — the "black" look comes
        // from a Gemini prompt that fills the design canvas with deep
        // black (AOP sublimates every pixel, so a fully black artwork
        // yields a near-solid black rashguard with the logo in white).
        printful_product_id: 301,
        printful_variant_id: 9328,
        placement: "front",
        retail_jpy: 9800,
        spec_html: "Men's all-over-print long-sleeve rashguard · 黒ベース · 82% polyester / 18% spandex · \
                    UPF 50+ · 4-way stretch · flatlock seams · sublimation print (full black canvas) · \
                    IBJJF gi/no-gi compliant",
    },
    ProductSpec {
        kind: "hoodie",
        printful_product_id: 146, // Gildan 18500 pullover hoodie (heavy black option)
        printful_variant_id: 5531, // Black M (5530 is Black S — verified against Printful API 2026-05-24)
        placement: "front",
        retail_jpy: 8800,
        spec_html: "Gildan 18500 unisex pullover hoodie · Black · 8.0 oz (270 gsm) · \
                    50/50 cotton-polyester blend · double-needle stitching · \
                    DTG print front chest · pouch pocket · drawstring hood",
    },
    ProductSpec {
        kind: "crewneck",
        printful_product_id: 145, // Gildan 18000 crewneck sweatshirt
        printful_variant_id: 5435, // Black M (5403 didn't exist — verified against Printful API 2026-05-24)
        placement: "front",
        retail_jpy: 7800,
        spec_html: "Gildan 18000 unisex crewneck sweatshirt · Black · 8.0 oz · \
                    50/50 cotton-polyester blend · 1×1 athletic ribbed collar · \
                    DTG print front chest",
    },
    ProductSpec {
        kind: "mug",
        // 11oz White Glossy Mug — same Printful product/variant proven live by
        // VOICE-MUG-01 / FOUND-MUG-01 / KAGI-MUG-01 / CHIP-MUG-01 (placement
        // 'front', see store/migrations/20260523*.sql).
        printful_product_id: 19,
        printful_variant_id: 1320,
        placement: "front",
        retail_jpy: 2200,
        spec_html: "11oz 白磁マグ · 光沢仕上げ · 電子レンジ・食洗機対応 · \
                    ラップ印刷(取っ手まわり以外の全面) · 縁まで鮮やかな発色 · 1点ずつ印刷",
    },
    ProductSpec {
        kind: "sticker",
        // Kiss-Cut Sticker 4×4 — same Printful product/variant proven live by
        // VOICE-STICK-01 / NEWS-STICK-01 / CHIP-STICK-01 + seed_mu_sticker
        // (358/10164, placement 'front').
        printful_product_id: 358,
        printful_variant_id: 10164,
        placement: "front",
        retail_jpy: 800,
        spec_html: "キスカット ステッカー · 4×4インチ(約10cm) · 耐水・耐光ビニール · \
                    強粘着 · 屋外耐候 · ノートPC/水筒/ギアに貼れる",
    },
    ProductSpec {
        kind: "nfc_coin",
        // No POD vendor: NFC音コイン is self-fulfilled (fulfillment_route
        // 'manual'). The NTAG213 tag is encoded with the song URL, locked,
        // and mailed in an envelope — so there is no Printful product /
        // variant / placement. The song URL is carried in description_ja via
        // the existing "oto.html?s=KEY" sound-tee convention; the manual arm
        // in fulfill_catalog_order() reads it to tell the operator what to write.
        printful_product_id: 0,
        printful_variant_id: 0,
        placement: "none",
        retail_jpy: 1800,
        spec_html: "NFC音コイン (NXP NTAG213) · ふれると鳴る · \
                    タップで mu.koe.live の一曲が再生 · URLは書込後ロック(改竄不可) · \
                    自社エンコード&発送 · gi・鍵・バッグに付けて持ち歩く",
    },
    ProductSpec {
        kind: "device",
        // No POD vendor: hardware (Koe デバイス等) is self-fulfilled
        // (fulfillment_route 'manual'). Payment via MU checkout, then the
        // operator ships the physical unit — same manual arm as nfc_coin.
        printful_product_id: 0,
        printful_variant_id: 0,
        placement: "none",
        retail_jpy: 9800,
        spec_html: "自社開発ハードウェア · 決済後に自社発送 · \
                    技適/PSE等の適合は商品説明に明記 · オープンソースファームウェア",
    },
    ProductSpec {
        kind: "event_ticket",
        // No POD vendor: a ticket is digital. fulfillment_route 'digital' —
        // on payment we issue a unique code, render a QR, and email it. No
        // Printful product / variant / placement. retail_jpy is only the
        // price FLOOR; the real seat price is passed per-product via price_jpy.
        printful_product_id: 0,
        printful_variant_id: 0,
        placement: "none",
        retail_jpy: 1000,
        spec_html: "デジタル参加券 · 購入後すぐ QR コードをメールでお届け · \
                    物理発送なし(送料0) · 会場で QR を提示して入場 · \
                    定員制(先着・売り切れ次第終了)",
    },
    ProductSpec {
        kind: "song",
        // Digital download/stream (fulfillment_route 'digital'). On payment we
        // email a private listen/download link to the hosted audio. No
        // Printful product / variant / placement. The audio URL lives in
        // catalog_products.meta_json `{"audio_url": "https://…"}`. retail_jpy
        // is the price FLOOR; the real price is passed per-product.
        printful_product_id: 0,
        printful_variant_id: 0,
        placement: "none",
        retail_jpy: 500,
        spec_html: "デジタル楽曲 · 購入後すぐ視聴/ダウンロードリンクをメールでお届け · \
                    物理発送なし(送料0) · MP3 ストリーム & ダウンロード · 永久アクセス",
    },
];

/// Public, agent-facing view of a `ProductSpec` so callers outside this
/// module (the agent API) can surface the kind whitelist + price floor
/// without reaching into the private struct.
pub struct AgentProductKind {
    pub kind: &'static str,
    /// Per-kind price floor (= the verified retail in PRODUCT_SPECS). Agents
    /// may pass a HIGHER price_jpy but never below this — protects genka.
    pub price_floor_jpy: i64,
    pub spec_html: &'static str,
}

/// The kinds an agent is allowed to create, derived from the same verified
/// `PRODUCT_SPECS` table the autonomous engine uses (so agents can NEVER
/// pass raw Printful ids or sub-genka prices). Pure data — cheap to call.
pub fn agent_product_kinds() -> Vec<AgentProductKind> {
    PRODUCT_SPECS.iter().map(|s| AgentProductKind {
        kind: s.kind,
        price_floor_jpy: s.retail_jpy,
        spec_html: s.spec_html,
    }).collect()
}

/// Insert one agent-created product into `catalog_products`, catalog-native.
///
/// Validates `kind` against the verified `PRODUCT_SPECS` whitelist (Err on an
/// unknown kind), applies the per-kind price floor (any `price_jpy_opt` below
/// the floor is clamped UP to the floor; None → the spec default), and writes
/// a row with `status='review'`, `is_active=0`, `legacy_source='agent_api'`
/// so nothing goes live until an MA-council member approves it.
///
/// The same `design_url` is stored as `design_file` / `mockup_main_file` /
/// `mockup_url_external` (the design-URL arm — no AI spend). For AOP
/// rashguards the route is `printful_aop` (4-panel cover-fill), else
/// `printful_dtg`, mirroring the autonomous engine's choice at line ~1921.
///
/// Returns the generated SKU. Does NOT spawn mockup tasks — the design URL is
/// the agent's own artwork; an MA reviewer eyeballs it before go-live.
pub fn agent_insert_product(
    conn: &rusqlite::Connection,
    brand: &str,
    label: &str,
    description_ja: &str,
    kind: &str,
    design_url: &str,
    price_jpy_opt: Option<i64>,
) -> Result<String, String> {
    let Some(spec) = PRODUCT_SPECS.iter().find(|s| s.kind == kind) else {
        let allowed: Vec<&str> = PRODUCT_SPECS.iter().map(|s| s.kind).collect();
        return Err(format!("unknown kind '{}'; allowed: {}", kind, allowed.join("/")));
    };
    // Price floor: clamp up to the verified retail, never below genka.
    let retail_jpy = price_jpy_opt.map(|p| p.max(spec.retail_jpy)).unwrap_or(spec.retail_jpy);

    // SKU: BRAND-AGENT-<kind>-<rand>, self-describing + collision-safe.
    let brand_for_sku: String = brand.chars()
        .filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_uppercase();
    let brand_for_sku = if brand_for_sku.is_empty() { "AGENT".to_string() } else { brand_for_sku };
    let seed = format!("{:08x}", rand::random::<u32>());
    let sku = format!("{}-AGENT-{}-{}",
        brand_for_sku, kind.to_uppercase().replace('_', "-"), seed);

    let route = match kind {
        "rashguard_ls" | "rashguard_black" => "printful_aop",
        // Self-fulfilled, non-Printful (NFC音コイン): take payment, then a
        // human encodes the tag + mails it (handled by the manual arm in
        // fulfill_catalog_order).
        "nfc_coin" | "device" => "manual",
        // Digital goods: take payment, then deliver by email (handled by the
        // digital arm in fulfill_catalog_order). No shipping. Ticket → QR;
        // song → private listen/download link.
        "event_ticket" | "song" => "digital",
        _ => "printful_dtg",
    };

    conn.execute(
        "INSERT INTO catalog_products (
            sku, brand, label, description_ja, retail_price_jpy,
            printful_product_id, printful_variant_id, printful_placement,
            printful_print_w, printful_print_h,
            design_file, mockup_main_file, mockup_url_external,
            is_active, sort_order, status, fulfillment_route, legacy_source
         ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        rusqlite::params![
            &sku, brand, label, description_ja, retail_jpy,
            spec.printful_product_id, spec.printful_variant_id, spec.placement,
            0, 0,
            design_url, design_url, design_url,
            0, 100,
            "review",
            route,
            "agent_api",
        ],
    ).map_err(|e| format!("insert failed: {}", e))?;
    Ok(sku)
}

struct Theme {
    slug: &'static str,
    display: &'static str,
    prompt_brief: &'static str,
    /// 1-line hook shown on PDP under the product name. Replaces the
    /// mechanical "BJJ 黒帯 · T シャツ" description with something a
    /// real visitor would buy.
    hook: &'static str,
    /// Long-form story for SEO + trust. Markdown-light (paragraphs only).
    story: &'static str,
}

const SEED_THEMES: &[Theme] = &[
    Theme {
        slug: "bjj_kuro_obi",
        display: "BJJ 黒帯",
        prompt_brief: "minimal sumi-e ink illustration of a tied jiu-jitsu black belt with the kanji 黒帯 in calligraphic style below",
        hook: "黒帯への 10 年を、 1 枚の墨絵に。 練習生のための minimal wearable.",
        story: "黒帯は最短でも 10 年。 道場で叩かれ、 試合で潰され、 また立つ。 \
                その積み重ねを、 1 本の墨線と「黒帯」 の二文字に凝縮しました。 \
                派手なロゴも、 ブランド主張もない。 知ってる人にだけ伝わる、 内側からの服。",
    },
    Theme {
        slug: "round_1",
        display: "Round 1",
        prompt_brief: "bold cinematic typography reading Round 1 inside a vintage boxing round-card border, monochrome ink",
        hook: "試合は Round 1 で決まらない。 でも、 全部 Round 1 から始まる。",
        story: "ボクシングのラウンドカードを、 wearable に。 \
                試合場でも、 ジムへの行き帰りでも、 朝のコーヒーでも、 \
                自分の「Round 1」 を今日も始める人のためのデイリーアイテム。",
    },
    Theme {
        slug: "teshikaga_mountain",
        display: "弟子屈 Mountain",
        prompt_brief: "geometric line-art of a Hokkaido mountain peak with a calm lake reflecting it, single-color print",
        hook: "北海道弟子屈町、 摩周湖。 山と湖の幾何学を、 1 枚に。",
        story: "MU の本拠地、 北海道弟子屈町。 摩周湖と斜里岳のシルエットを、 \
                線だけで切り出した抽象パターン。 国内 / 海外の MU 着用者を、 \
                一つの土地名で繋ぐ origin マーク。",
    },
    Theme {
        slug: "mu_mark",
        display: "MU ━◯━",
        prompt_brief: "the ━◯━ mark (long-dash circle long-dash) centered large and bold, with a small MU wordmark below in monospace",
        hook: "MU のブランドマーク ━◯━ を、 そのまま着る。",
        story: "━◯━ は MU のシグネチャー。 「あいだ」 「沈黙」 「無」 を一筆で表したマーク。 \
                ロゴだけの T シャツは、 ブランドへの最大のリスペクト ── \
                着る人がブランドを完成させる、 という意思表示。",
    },
    Theme {
        slug: "coffee_code",
        display: "Coffee × Code",
        prompt_brief: "minimal coffee cup outline with a binary stream rising as steam, geek-aesthetic monochrome",
        hook: "コーヒー → コード → コンパイル。 全エンジニアの朝の儀式を 1 枚に。",
        story: "コーヒーから立ち上る湯気を、 そのまま binary stream に。 \
                派手すぎず、 ギーク文化を知ってる人にだけ刺さる minimal な geek wearable。 \
                スタンディングデスク前の制服として。",
    },
    Theme {
        slug: "drill_loop",
        display: "Drill Loop",
        prompt_brief: "minimal sketch of an infinite loop arrow with the word DRILL stenciled inside, BJJ training aesthetic",
        hook: "ドリル × 100 = 黒帯。 反復だけが裏切らない。",
        story: "技は天才のものじゃない。 1 つの動きを 100 回、 1000 回、 10000 回繰り返す \
                ── その地味さに耐えた人だけが上手くなる。 ループのアロー 1 本で \
                練習生の日々を象徴。",
    },
    Theme {
        slug: "passing_guard",
        display: "Passing Guard",
        prompt_brief: "minimal line-art of two stylized jiu-jitsu silhouettes locked in a guard-pass position, single-color ink",
        hook: "ガードパスは芸術だ。 押すんじゃなくて、 流す。",
        story: "BJJ で最も奥深い局面、 ガードパス。 押す技じゃない、 流す技。 \
                墨絵タッチのシルエットで、 試合中の集中を 1 枚に。",
    },
    Theme {
        slug: "tatami_grain",
        display: "Tatami Grain",
        prompt_brief: "abstract texture of jiu-jitsu mat tatami pattern, monochrome line work like a topo map",
        hook: "畳の目を見つめた回数だけ、 強くなる。",
        story: "練習中、 一番見つめてるのは相手じゃなくて畳。 \
                打ち込み、 寝技、 押さえ込み ── 畳の柄が思考の背景。 \
                抽象トポマップとして wearable に。",
    },
    Theme {
        slug: "ipponseo",
        display: "一本背負",
        prompt_brief: "minimal sumi-e silhouette of a judo ippon seoi nage throw, with the kanji 一本背負 in caligraphy",
        hook: "一本背負 — 投げ切る覚悟だけが、 試合を終わらせる。",
        story: "柔道の代表技、 一本背負。 BJJ 練習生にも刺さる「投げ切る」 美学を、 \
                墨絵 1 筆に。 道場でも、 オフでも着られる minimalist tribute。",
    },
    Theme {
        slug: "founder_grit",
        display: "Founder Grit",
        prompt_brief: "minimal hand-drawn calligraphy of the kanji 創業 (founding), Japanese ink style",
        hook: "「創業」── 0 から作る人だけが分かる、 静かな狂気。",
        story: "起業家・職人・ アスリート ── 0 から立ち上げた人だけが分かる時間。 \
                派手な肩書きじゃなく「創業」 の 2 文字を、 黙って着る。",
    },
    Theme {
        slug: "north_circle",
        display: "North Circle",
        prompt_brief: "abstract geometric composition: a single circle with a north arrow piercing it, Bauhaus minimalism",
        hook: "北を 1 つだけ決める。 残りは捨てる。",
        story: "選択肢が多すぎる時代に、 北 (=方向) を 1 つだけ持つ。 \
                Bauhaus 影響の geometric minimal。 集中したい人のための daily uniform。",
    },
];

const GEMINI_IMAGE_COST_JPY: i64 = 6;

/// Returns (theme_display, kind, retail_jpy) for the named slug/kind, or None.
fn theme_and_spec(theme_slug: &str, kind: &str) -> Option<(&'static Theme, &'static ProductSpec)> {
    let t = SEED_THEMES.iter().find(|t| t.slug == theme_slug)?;
    let s = PRODUCT_SPECS.iter().find(|s| s.kind == kind)?;
    Some((t, s))
}

/// Generate one SKU end-to-end:
///   Gemini design → R2 upload → INSERT catalog_products
/// Returns the new SKU id. Idempotent on (theme, kind, seed).
pub async fn generate_one(
    db: Db,
    theme_slug: &str,
    kind: &str,
    seed: &str,
) -> Result<String, String> {
    let (theme, spec) = theme_and_spec(theme_slug, kind)
        .ok_or_else(|| format!("unknown theme/kind: {}/{}", theme_slug, kind))?;
    let sku = format!(
        "AUTO-{}-{}-{}",
        theme.slug.to_uppercase().replace('_', "-"),
        kind.to_uppercase().replace('_', "-"),
        seed
    );

    // Skip if SKU already exists.
    {
        let conn = db.lock().unwrap();
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM catalog_products WHERE sku=? LIMIT 1",
                rusqlite::params![&sku],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if exists {
            return Ok(sku);
        }
        // Mark job pending so a concurrent generator doesn't race us.
        let _ = conn.execute(
            "INSERT OR IGNORE INTO catalog_gen_jobs (theme, kind, seed, status)
             VALUES (?, ?, ?, 'pending')",
            rusqlite::params![theme.slug, kind, seed],
        );
    }

    // Budget check + reserve the ¥6 Gemini cost up-front. If the call
    // later fails we leave the spend recorded — better to over-report
    // than under-report. The optimizer cron can reconcile later.
    let charged = {
        let conn = db.lock().unwrap();
        spend_or_refuse(
            &conn,
            "ai_image",
            GEMINI_IMAGE_COST_JPY,
            &format!("gen sku={}", sku),
            Some(&sku),
        )
    };
    if !charged {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "UPDATE catalog_gen_jobs SET status='refused_budget', error=?, completed_at=datetime('now')
             WHERE theme=? AND kind=? AND seed=?",
            rusqlite::params!["budget cap reached", theme.slug, kind, seed],
        );
        return Err("budget cap reached".into());
    }

    // Gemini print-ready prompt. For the black-rashguard kind we ask for
    // a fully-black canvas with the design as a white inversion — AOP
    // sublimation prints every pixel so this yields a near-solid black
    // rashguard with the logo in light contrast.
    let prompt = if kind == "rashguard_black" {
        format!(
            "Square 300 DPI artwork for all-over print on a long-sleeve rashguard. \
             Fill the entire canvas with PURE BLACK (#0a0a0a). \
             Centered on the chest: the design '{brief}' rendered in WHITE or \
             very light ivory so it pops against the black. \
             Hard constraints: NO model, NO mockup, NO photographic scene. \
             Just the print-ready square artwork. Variation key: {seed}.",
            brief = theme.prompt_brief, seed = seed,
        )
    } else {
        format!(
            "Print-ready chest graphic at 300 DPI on a PURE WHITE background \
             (white acts as the transparent layer for DTG printing). \
             Style brief: {brief}. \
             Hard constraints: NO model, NO mockup, NO photographic scene, \
             NO shirt visible — just the artwork itself, centered, square \
             aspect ratio, bleed-safe, ready to be printed onto apparel. \
             Variation key: {seed}.",
            brief = theme.prompt_brief, seed = seed,
        )
    };
    let img = crate::gemini::call_gemini(&prompt)
        .await
        .map_err(|e| {
            mark_job_failed(&db, theme.slug, kind, seed, &format!("gemini: {}", e));
            format!("gemini: {}", e)
        })?;

    // Upload to R2 (must be configured — local fallback isn't reachable
    // by Printful's worker, so we'd just print blank shirts).
    let key = format!("catalog/{}.png", sku);
    let url = crate::store_r2_bytes(&key, &img.bytes, &img.mime).await.ok_or_else(|| {
        let msg = "R2 upload failed (R2_* env unset or upload error)";
        mark_job_failed(&db, theme.slug, kind, seed, msg);
        msg.to_string()
    })?;

    // INSERT catalog_products + ensure the 'auto' brand row exists so
    // /shop renders a brand chip for it.
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "INSERT OR IGNORE INTO catalog_brands
             (slug, name, emoji, color_primary, tagline, custom_domain,
              is_active, revenue_share_pct)
             VALUES ('auto', 'AUTO (AI-generated)', '🤖', '#ffd700',
                     'Gemini × Printful POD · 30 分自動生成', NULL, 1, 0)",
            [],
        );
        // Human-readable description, not "BJJ 黒帯 · T シャツ" — the
        // theme hook is the marketing line a real visitor reads.
        let desc = format!("{} — {}", theme.display, theme.hook);
        let _ = conn.execute(
            "INSERT INTO catalog_products (
                sku, brand, label, description_ja, retail_price_jpy,
                printful_product_id, printful_variant_id, printful_placement,
                printful_print_w, printful_print_h,
                design_file, mockup_main_file, mockup_url_external,
                is_active, sort_order
             ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            rusqlite::params![
                &sku,
                "auto",
                desc,
                desc,
                spec.retail_jpy,
                spec.printful_product_id,
                spec.printful_variant_id,
                spec.placement,
                0,
                0,
                &url,
                &url,
                &url,
                1,
                100,
            ],
        );
        let _ = conn.execute(
            "UPDATE catalog_gen_jobs
             SET status='completed', sku=?, spent_jpy=?, completed_at=datetime('now')
             WHERE theme=? AND kind=? AND seed=?",
            rusqlite::params![&sku, GEMINI_IMAGE_COST_JPY, theme.slug, kind, seed],
        );
    }
    tracing::info!("[catalog/gen] OK sku={} theme={} kind={}", sku, theme.slug, kind);

    // 4 images per SKU, fired in parallel after the print-art (a) lands:
    //   (a) AI design   — already saved at `url` above (catalog/<sku>.png)
    //   (b) transparent — process (a) white→alpha, save as catalog/print/<sku>.png
    //   (c) Printful mockup — POD garment render via mockup-generator
    //   (d) lifestyle  — Gemini on-body photo (face-cropped, scene varies)
    // Tokio::spawn fires all three (b/c/d) concurrently; main returns the
    // SKU id immediately so the cron doesn't block.
    let pp = spec.printful_product_id;
    let pv = spec.printful_variant_id;

    // (b) transparent print file — fast, free.
    let db_b = db.clone();
    let sku_b = sku.clone();
    let img_bytes_b = img.bytes.clone();
    tokio::spawn(async move {
        if let Err(e) = generate_transparent_print(db_b, sku_b, img_bytes_b).await {
            tracing::warn!("[catalog/transparent] failed: {}", e);
        }
    });

    // (c) Printful on-body mockup.
    let db_c = db.clone();
    let sku_c = sku.clone();
    let url_c = url.clone();
    tokio::spawn(async move {
        if let Err(e) = generate_onbody_mockup(db_c, sku_c, pp, pv, url_c).await {
            tracing::warn!("[catalog/mockup] failed: {}", e);
        }
    });

    // (d) lifestyle Gemini photo (1 per SKU; cron's mockup_backfill_step
    // can add more in subsequent cycles if budget permits).
    let db_d = db.clone();
    let sku_d = sku.clone();
    let theme_slug = theme.slug.to_string();
    let theme_brief = theme.prompt_brief.to_string();
    let kind_d = kind.to_string();
    tokio::spawn(async move {
        if let Err(e) = generate_lifestyle_photo(db_d, sku_d, theme_slug, theme_brief, kind_d, 1).await {
            tracing::warn!("[catalog/lifestyle] failed: {}", e);
        }
    });

    Ok(sku)
}

/// Decode the print PNG, replace near-white pixels with transparent
/// alpha, re-encode, upload to R2 under catalog/print/<sku>.png, store
/// in catalog_products.print_file (column-less for now; reuse
/// design_file vs UPDATE existing). The transparent file is what
/// Printful AOP / DTG actually wants — white-background art prints a
/// white rectangle on AOP rashguards.
pub async fn generate_transparent_print(
    db: Db,
    sku: String,
    bytes: Vec<u8>,
) -> Result<(), String> {
    // Decode as RGBA so we always have an alpha channel to work with.
    let img = image::load_from_memory(&bytes)
        .map_err(|e| format!("decode: {}", e))?
        .to_rgba8();
    let mut out = img.clone();
    // Threshold: any pixel where R, G, B are all >= 248 → fully transparent.
    for px in out.pixels_mut() {
        let [r, g, b, _a] = px.0;
        if r >= 248 && g >= 248 && b >= 248 {
            px.0[3] = 0;
        }
    }
    let mut buf = std::io::Cursor::new(Vec::<u8>::new());
    out.write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| format!("encode: {}", e))?;
    let png_bytes = buf.into_inner();
    let key = format!("catalog/print/{}.png", sku);
    let url = crate::store_r2_bytes(&key, &png_bytes, "image/png").await
        .ok_or_else(|| "R2 upload failed".to_string())?;
    // Stash via product_extras with a known label so the PDP can pick it
    // up as the "print用" sample image (no schema change needed).
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO catalog_product_extras (sku, label, image_url, sort_order)
             VALUES (?, '透過版 (print)', ?, 10)",
            rusqlite::params![&sku, &url],
        );
    }
    tracing::info!("[catalog/transparent] OK sku={} → {}", sku, url);
    Ok(())
}

/// 生成画像の背景(白 or 黒)を透過にする。生成は白(or黒)背景で行い、出来上がりの
/// 背景色だけを後処理で alpha=0 にする方針。四隅をサンプルして背景が白か黒かを
/// 推定し、その色に近いピクセルだけを抜く(作品の黒/白の線画は残す。両方一律には
/// 抜かない)。デコード失敗・極端に小さい画像では None(呼び出し側は元画像にフォールバック)。
fn make_design_transparent(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    if w < 16 || h < 16 { return None; }
    // 四隅(6px inset の 4x4 ブロック)の平均輝度で背景を推定。
    let corners = [(6u32, 6u32), (w - 10, 6u32), (6u32, h - 10), (w - 10, h - 10)];
    let (mut sum, mut n) = (0u32, 0u32);
    for (cx, cy) in corners {
        for dy in 0..4u32 {
            for dx in 0..4u32 {
                let p = img.get_pixel((cx + dx).min(w - 1), (cy + dy).min(h - 1)).0;
                sum += p[0] as u32 + p[1] as u32 + p[2] as u32;
                n += 3;
            }
        }
    }
    let avg = if n > 0 { sum / n } else { 255 };
    let knock_white = avg >= 128; // 明るい四隅→白背景 / 暗い四隅→黒背景
    let mut out = img.clone();
    for px in out.pixels_mut() {
        let [r, g, b, _a] = px.0;
        let hit = if knock_white {
            r >= 248 && g >= 248 && b >= 248
        } else {
            r <= 8 && g <= 8 && b <= 8
        };
        if hit { px.0[3] = 0; }
    }
    let mut buf = std::io::Cursor::new(Vec::<u8>::new());
    out.write_to(&mut buf, image::ImageFormat::Png).ok()?;
    Some(buf.into_inner())
}

/// Generate one lifestyle (on-body) photo via Gemini. Prompted to avoid
/// the model's face (back-shot / torso / hands holding garment) so the
/// PDP doesn't show the same Printful default model on every SKU.
/// Stores in catalog_product_extras for the PDP gallery to pick up.
pub async fn generate_lifestyle_photo(
    db: Db,
    sku: String,
    theme_slug: String,
    theme_brief: String,
    kind: String,
    variant: u32,
) -> Result<(), String> {
    let scene = scene_for_kind(&kind, variant);
    let brand_ctx = brand_context(&theme_slug);
    // Budget check (¥6 per Gemini image).
    let charged = {
        let conn = db.lock().unwrap();
        spend_or_refuse(
            &conn,
            "ai_image",
            GEMINI_IMAGE_COST_JPY,
            &format!("lifestyle sku={} v={}", sku, variant),
            Some(&sku),
        )
    };
    if !charged {
        return Err("budget cap reached".into());
    }
    // Look up the design PNG so we can pass it to Gemini as a reference.
    // ONLY use design_file — falling back to mockup_url_external is unsafe
    // because Printful's mockup-generator returns S3 URLs on the
    // `printful-upload.s3-accelerate.amazonaws.com/tmp/…` host that are
    // signed and expire (verified 403 Forbidden 2026-05-25). Even when
    // still live, the mockup file shows the garment AND the print, not
    // the print alone, which makes it a worse conditioning signal than
    // the raw design PNG anyway. If design_file is missing, fall through
    // to text-only generation.
    let design_url: Option<String> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT design_file FROM catalog_products WHERE sku=?",
            rusqlite::params![&sku],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
        .filter(|s| s.starts_with("http") && !s.contains("printful-upload.s3"))
    };
    let ref_clause = if design_url.is_some() {
        "The garment in the photo MUST be printed with the EXACT graphic design shown in the supplied reference image — match the artwork, colours, and proportions precisely. The brief below is context, but the reference image is the source of truth for the print."
    } else {
        "The garment design (printed on chest / back, depending on shot) interprets the brief below — no reference image was supplied."
    };
    let prompt = format!(
        "Editorial 4:5 portrait lifestyle photo, 1080×1350. \
         Brand context: {brand_ctx} \
         Scene: {scene} \
         {ref_clause} \
         Garment brief / concept: {brief}. \
         Style: photorealistic Sony A7IV 35mm f/2.0, soft natural light, slight film grain, \
         magazine cover quality. \
         Strict rules: NO face visible (use back-of-head, deliberate crop, or composition to hide it); \
         NO text overlay added to the photo; NO watermark; NO mannequin look; NO uncanny limbs; \
         NO blurred or melted logos. The printed graphic must be sharp and recognisable. \
         Variation key: {sku}-v{variant}.",
        brand_ctx = brand_ctx,
        scene = scene,
        ref_clause = ref_clause,
        brief = theme_brief,
        sku = sku,
        variant = variant,
    );
    let img = if let Some(url) = design_url.as_deref() {
        crate::gemini::call_gemini_with_image(&prompt, &[url]).await
    } else {
        crate::gemini::call_gemini(&prompt).await
    }
    .map_err(|e| format!("gemini: {}", e))?;
    let key = format!("catalog/lifestyle/{}-v{}.png", sku, variant);
    let url = crate::store_r2_bytes(&key, &img.bytes, &img.mime).await
        .ok_or_else(|| "R2 upload failed".to_string())?;
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO catalog_product_extras (sku, label, image_url, sort_order)
             VALUES (?, ?, ?, ?)",
            rusqlite::params![
                &sku,
                format!("lifestyle {} ({})", variant, theme_slug),
                &url,
                100 + variant as i64,
            ],
        );
    }
    tracing::info!("[catalog/lifestyle] OK sku={} v={} → {}", sku, variant, url);
    Ok(())
}

// ---------------------------------------------------------------------------
// Lifestyle 着画 by REAL-DESIGN COMPOSITE (no Gemini re-draw → zero drift)
//
// The old lifestyle photos were re-drawn by Gemini from the design image, so
// the printed graphic drifted (e.g. a framed white box collapsed into bare
// white text). Instead we composite the ACTUAL design_file — exactly what
// Printful prints — onto a print-free worn-blank base photo, multiplied by a
// blurred luminance map of the garment so it reads as printed, not pasted.
// The print is pixel-identical to the design every time.
//
// Base photos live in store/static/lifestyle_base/{file}.png (front-facing,
// solid-black, blank-chest models). Each base carries the chest print box as
// fractions of image size: (cx, cy = box center, wfrac = box width).
// ---------------------------------------------------------------------------

struct LbBase {
    file: &'static str,
    cx: f32,
    cy: f32,
    wfrac: f32,
}

/// Worn-blank bases for a garment kind, or empty if this kind is not a
/// chest-print apparel item (caller then falls back / skips).
fn lifestyle_bases(kind: &str) -> &'static [LbBase] {
    match kind {
        "hoodie" => &[
            LbBase { file: "hoodie_1", cx: 0.500, cy: 0.405, wfrac: 0.300 },
            LbBase { file: "hoodie_2", cx: 0.500, cy: 0.410, wfrac: 0.300 },
        ],
        "crewneck" => &[
            LbBase { file: "crewneck_1", cx: 0.500, cy: 0.400, wfrac: 0.320 },
            LbBase { file: "crewneck_2", cx: 0.500, cy: 0.400, wfrac: 0.320 },
        ],
        "tee" | "tank" | "long_sleeve_tee" => &[
            LbBase { file: "tee_1", cx: 0.500, cy: 0.385, wfrac: 0.345 },
            LbBase { file: "tee_3", cx: 0.500, cy: 0.390, wfrac: 0.340 },
        ],
        _ => &[],
    }
}

/// Stable per-SKU base pick so the same product always renders the same way
/// (idempotent) while the catalog as a whole rotates through the variants.
fn pick_base<'a>(bases: &'a [LbBase], sku: &str) -> &'a LbBase {
    let h: u32 = sku.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    &bases[(h as usize) % bases.len()]
}

fn read_base_png(file: &str) -> Option<Vec<u8>> {
    // ServeDir serves "static" relative to the working dir, so the bundled
    // bases sit at static/lifestyle_base/<file>.png. Try a couple of roots so
    // this also works when launched from the repo root in dev.
    for root in ["static", "store/static"] {
        let p = format!("{}/lifestyle_base/{}.png", root, file);
        if let Ok(b) = std::fs::read(&p) {
            return Some(b);
        }
    }
    None
}

/// Composite `design_png` (the exact Printful print artwork, normally an
/// opaque square incl. its printed background) onto `base_png` at the base's
/// chest box, shaded by the garment's folds. Returns encoded PNG bytes.
fn compose_lifestyle_png(design_png: &[u8], base_png: &[u8], b: &LbBase) -> Result<Vec<u8>, String> {
    use image::imageops;
    let mut base = image::load_from_memory(base_png)
        .map_err(|e| format!("base decode: {}", e))?
        .to_rgba8();
    let (iw, ih) = base.dimensions();

    let design = image::load_from_memory(design_png)
        .map_err(|e| format!("design decode: {}", e))?
        .to_rgba8();
    // The design_file IS the printed box (white/black bg included), so use the
    // full square. Only crop when the file is genuinely transparent.
    let has_alpha = design.pixels().any(|p| p.0[3] < 250);
    let design = if has_alpha {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for (x, y, p) in design.enumerate_pixels() {
            if p.0[3] > 12 {
                x0 = x0.min(x); y0 = y0.min(y); x1 = x1.max(x); y1 = y1.max(y);
            }
        }
        if x1 >= x0 && y1 >= y0 {
            imageops::crop_imm(&design, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image()
        } else {
            design
        }
    } else {
        design
    };
    let (dw, dh) = design.dimensions();
    if dw == 0 || dh == 0 {
        return Err("empty design".into());
    }

    let box_w = ((iw as f32) * b.wfrac).round().max(1.0) as u32;
    let box_h = ((dh as f32) * (box_w as f32 / dw as f32)).round().max(1.0) as u32;
    let mut layer = imageops::resize(&design, box_w, box_h, imageops::FilterType::Lanczos3);

    // Top-left of the box, clamped inside the frame.
    let px = (((iw as f32) * b.cx).round() as i64 - box_w as i64 / 2)
        .clamp(0, (iw.saturating_sub(box_w)) as i64);
    let py = (((ih as f32) * b.cy).round() as i64 - box_h as i64 / 2)
        .clamp(0, (ih.saturating_sub(box_h)) as i64);

    // Blurred luminance of the garment region → only large folds survive,
    // so a white print box stays clean instead of speckling on sensor noise.
    let region = imageops::crop_imm(&base, px as u32, py as u32, box_w, box_h).to_image();
    let luma = image::DynamicImage::ImageRgba8(region).to_luma8();
    let sigma = ((box_w as f32) / 40.0).max(4.0);
    let blurred = imageops::blur(&luma, sigma);
    let mut vals: Vec<u8> = blurred.pixels().map(|p| p.0[0]).collect();
    vals.sort_unstable();
    let p90 = (*vals.get(vals.len().saturating_mul(90) / 100).unwrap_or(&255) as f32).max(8.0);

    let (bw, bh) = blurred.dimensions();
    let _ = bh;
    for (x, y, px2) in layer.enumerate_pixels_mut() {
        let lum = blurred.get_pixel(x.min(bw - 1), y.min(blurred.height() - 1)).0[0] as f32;
        let shade = (0.66 + 0.34 * (lum / p90)).clamp(0.66, 1.0);
        px2.0[0] = (px2.0[0] as f32 * shade).clamp(0.0, 255.0) as u8;
        px2.0[1] = (px2.0[1] as f32 * shade).clamp(0.0, 255.0) as u8;
        px2.0[2] = (px2.0[2] as f32 * shade).clamp(0.0, 255.0) as u8;
        px2.0[3] = (px2.0[3] as f32 * 0.95) as u8;
    }
    imageops::overlay(&mut base, &layer, px, py);

    let mut buf = std::io::Cursor::new(Vec::<u8>::new());
    image::DynamicImage::ImageRgba8(base)
        .into_rgb8()
        .write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| format!("encode: {}", e))?;
    Ok(buf.into_inner())
}

/// Fetch the design, composite onto a worn-blank base, upload to R2, and
/// point all the SKU's lifestyle rows at it. Returns the new public URL.
async fn composite_lifestyle_to_r2(
    db: &Db,
    sku: &str,
    kind: &str,
    design_file: &str,
) -> Result<String, String> {
    let bases = lifestyle_bases(kind);
    if bases.is_empty() {
        return Err(format!("kind {} not a chest-print item", kind));
    }
    if !design_file.starts_with("http") {
        return Err("no design_file".into());
    }
    let b = pick_base(bases, sku);
    let base_png = read_base_png(b.file).ok_or_else(|| format!("base {} missing", b.file))?;
    let design_png = reqwest::Client::new()
        .get(design_file)
        .send().await.map_err(|e| format!("fetch design: {}", e))?
        .bytes().await.map_err(|e| format!("read design: {}", e))?
        .to_vec();
    let out = compose_lifestyle_png(&design_png, &base_png, b)?;
    let key = format!("catalog/lifestyle/{}-fit.png", sku);
    let url = crate::store_r2_bytes(&key, &out, "image/png").await
        .ok_or_else(|| "R2 upload failed".to_string())?;
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "UPDATE catalog_product_extras SET image_url=? WHERE sku=? AND lower(label) LIKE 'lifestyle%'",
            rusqlite::params![&url, sku],
        );
    }
    Ok(url)
}

#[derive(serde::Deserialize)]
pub struct FixLifestyleQuery {
    pub token: String,
    #[serde(default)]
    pub dry_run: bool,
    pub limit: Option<usize>,
    pub sku: Option<String>,
}

/// Replace drifted Gemini 着画 with accurate real-design composites.
/// tee/hoodie/crewneck/tank → composite the real design onto a worn-blank;
/// rashguard (AOP full-front) → reuse the accurate Printful mockup_url_external.
pub async fn admin_fix_lifestyle(
    State(db): State<Db>,
    Query(q): Query<FixLifestyleQuery>,
) -> Response {
    let expected = std::env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }

    // Target: live SKUs that currently have at least one lifestyle row.
    let rows: Vec<(String, String, String)> = {
        let conn = db.lock().unwrap();
        let mut sql = String::from(
            "SELECT p.sku, COALESCE(p.design_file,''), COALESCE(p.mockup_url_external,'')
             FROM catalog_products p
             WHERE p.status='live'
               AND EXISTS(SELECT 1 FROM catalog_product_extras e
                          WHERE e.sku=p.sku AND lower(e.label) LIKE 'lifestyle%')",
        );
        if q.sku.is_some() {
            sql.push_str(" AND p.sku=?");
        }
        sql.push_str(" ORDER BY p.sku");
        conn.prepare(&sql).ok().and_then(|mut s| {
            let map = |r: &rusqlite::Row| Ok((
                r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?,
            ));
            let it = if let Some(ref sku) = q.sku {
                s.query_map(rusqlite::params![sku], map).ok()?
                    .filter_map(|r| r.ok()).collect::<Vec<_>>()
            } else {
                s.query_map([], map).ok()?
                    .filter_map(|r| r.ok()).collect::<Vec<_>>()
            };
            Some(it)
        }).unwrap_or_default()
    };

    let (mut composited, mut rash_reused, mut skipped, mut failed) = (0u32, 0u32, 0u32, 0u32);
    let mut samples: Vec<serde_json::Value> = Vec::new();
    let mut processed = 0usize;

    for (sku, design_file, mockup) in &rows {
        if let Some(lim) = q.limit {
            if processed >= lim { break; }
        }
        let kind = kind_from_sku(sku);
        let is_rash = kind == "rashguard_ls" || kind == "rashguard_black";
        let supported = is_rash || !lifestyle_bases(kind).is_empty();
        if !supported {
            skipped += 1;
            continue;
        }

        if is_rash {
            // Reuse the accurate AOP mockup as the lifestyle image.
            let good = mockup.starts_with("http") && mockup != design_file;
            if !good { skipped += 1; continue; }
            processed += 1;
            if q.dry_run {
                rash_reused += 1;
                if samples.len() < 8 {
                    samples.push(serde_json::json!({"sku": sku, "mode": "rash_reuse", "url": mockup}));
                }
            } else {
                let conn = db.lock().unwrap();
                match conn.execute(
                    "UPDATE catalog_product_extras SET image_url=? WHERE sku=? AND lower(label) LIKE 'lifestyle%'",
                    rusqlite::params![mockup, sku],
                ) {
                    Ok(_) => rash_reused += 1,
                    Err(_) => failed += 1,
                }
            }
            continue;
        }

        // tee-family composite
        processed += 1;
        if q.dry_run {
            composited += 1;
            if samples.len() < 8 {
                let b = pick_base(lifestyle_bases(kind), sku);
                samples.push(serde_json::json!({"sku": sku, "mode": "composite", "kind": kind, "base": b.file}));
            }
            continue;
        }
        match composite_lifestyle_to_r2(&db, sku, kind, design_file).await {
            Ok(url) => {
                composited += 1;
                if samples.len() < 8 {
                    samples.push(serde_json::json!({"sku": sku, "mode": "composite", "url": url}));
                }
            }
            Err(e) => {
                failed += 1;
                tracing::warn!("[fix_lifestyle] {} failed: {}", sku, e);
            }
        }
    }

    axum::Json(serde_json::json!({
        "ok": true,
        "dry_run": q.dry_run,
        "candidates": rows.len(),
        "composited": composited,
        "rash_reused": rash_reused,
        "skipped": skipped,
        "failed": failed,
        "samples": samples,
    })).into_response()
}

/// Async background task: call Printful's mockup-generator with the
/// design URL, poll until done (~30-60s), upload the resulting on-body
/// mockup to R2, swap catalog_products.mockup_url_external. Printful's
/// mockup-generator is free for the basic single-front variant we use,
/// so no budget guard needed.
pub async fn generate_onbody_mockup(
    db: Db,
    sku: String,
    printful_product: i64,
    printful_variant: i64,
    design_url: String,
) -> Result<(), String> {
    let key = std::env::var("PRINTFUL_API_KEY").unwrap_or_default();
    if key.is_empty() {
        return Err("PRINTFUL_API_KEY not set".into());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("client: {}", e))?;

    // 1. Create task. The `position` field is mandatory per Printful
    //    error MG-4 "Position field is missing"; values mirror
    //    printful_mockup_config_for() in main.rs for chest_tee.
    //    AOP rashguard (301) supports four sublimation panels — fan the
    //    same design URL out to all of them so the mockup shows a true
    //    belt-colored garment instead of a chest-only print.
    let position = serde_json::json!({
        "area_width": 1800, "area_height": 2400,
        "width": 1260,      "height": 1260,
        "top": 380,         "left": 270
    });
    let placements = placements_for_product(printful_product);
    let files: Vec<serde_json::Value> = placements.iter().map(|p| {
        serde_json::json!({
            "placement": p,
            "image_url": design_url,
            "position": position,
        })
    }).collect();
    let create_body = serde_json::json!({
        "variant_ids": [printful_variant],
        "format": "png",
        "files": files,
    });
    let create_url = format!(
        "https://api.printful.com/mockup-generator/create-task/{}",
        printful_product
    );
    let resp = client
        .post(&create_url)
        .bearer_auth(&key)
        .json(&create_body)
        .send()
        .await
        .map_err(|e| format!("create-task send: {}", e))?;
    if !resp.status().is_success() {
        let s = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("create-task {}: {}", s, &body[..body.len().min(300)]));
    }
    let j: serde_json::Value = resp.json().await.map_err(|e| format!("create-task parse: {}", e))?;
    let task_key = j["result"]["task_key"]
        .as_str()
        .ok_or_else(|| "no task_key".to_string())?
        .to_string();

    // Log attempt start in spend ledger (¥0) so we can see backfill activity
    // in /admin/catalog/status — tracing!/warn! logs go to Fly stdout which
    // isn't easily readable from outside.
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO catalog_spend (category, amount_jpy, reason, ref_id)
             VALUES ('mockup_attempt', 0, ?, ?)",
            rusqlite::params![format!("printful task_key={}", task_key), &sku],
        );
    }

    // 2. Poll up to 60 × 4s = 4 min. Printful's queue can be slow during
    //    peak hours; cycles 2-3 of the first deploy timed out at 2 min.
    let mut mockup_url: Option<String> = None;
    for attempt in 0..60 {
        tokio::time::sleep(std::time::Duration::from_secs(if attempt == 0 { 5 } else { 4 })).await;
        let poll = format!("https://api.printful.com/mockup-generator/task?task_key={}", task_key);
        let r = match client.get(&poll).bearer_auth(&key).send().await {
            Ok(r) => r,
            Err(_) => continue,
        };
        if !r.status().is_success() {
            continue;
        }
        let pj: serde_json::Value = match r.json().await {
            Ok(v) => v,
            Err(_) => continue,
        };
        match pj["result"]["status"].as_str() {
            Some("completed") => {
                mockup_url = pj["result"]["mockups"][0]["mockup_url"].as_str().map(String::from);
                break;
            }
            Some("failed") => {
                return Err("printful task failed".into());
            }
            _ => continue,
        }
    }
    let mockup_url = mockup_url.ok_or_else(|| "poll timeout (2min)".to_string())?;

    // 3. Mirror to R2 so the URL survives Printful's ~24h presign and
    //    becomes part of the catalog forever.
    let mockup_bytes = client.get(&mockup_url).send().await
        .map_err(|e| format!("download mockup: {}", e))?
        .bytes().await
        .map_err(|e| format!("read mockup bytes: {}", e))?
        .to_vec();
    let r2_key = format!("catalog/mockups/{}.png", sku);
    let r2_url = crate::store_r2_bytes(&r2_key, &mockup_bytes, "image/png")
        .await
        .unwrap_or(mockup_url.clone());

    // 4. Swap mockup_url_external. mockup_main_file (the design URL) stays
    //    as the fallback for Printful fulfillment files.
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "UPDATE catalog_products SET mockup_url_external=? WHERE sku=?",
            rusqlite::params![&r2_url, &sku],
        );
    }
    tracing::info!("[catalog/mockup] OK sku={} → {}", sku, r2_url);
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO catalog_spend (category, amount_jpy, reason, ref_id)
             VALUES ('mockup_ok', 0, ?, ?)",
            rusqlite::params![&r2_url, &sku],
        );
    }
    Ok(())
}

fn mark_job_failed(db: &Db, theme: &str, kind: &str, seed: &str, err: &str) {
    let conn = db.lock().unwrap();
    let _ = conn.execute(
        "UPDATE catalog_gen_jobs SET status='failed', error=?, completed_at=datetime('now')
         WHERE theme=? AND kind=? AND seed=?",
        rusqlite::params![err, theme, kind, seed],
    );
}

/// Best-effort kind inference from SKU pattern. Used to pick a PRODUCT_SPECS
/// row to render on the PDP. AUTO SKUs embed the kind verbatim; merch-bridge
/// SKUs encode it as a fragment of the SKU name (TEE / RASH / HOOD / etc.).
fn kind_from_sku(sku: &str) -> &'static str {
    let s = sku.to_uppercase();
    // Order matters: more specific tokens come first so "RASHGUARD" wins
    // over the generic MU- starts-with fallback at the bottom.
    if s.contains("-EVENT-TICKET") || s.contains("-TICKET-") || s.ends_with("-TICKET") { return "event_ticket"; }
    if s.contains("-SONG-") || s.ends_with("-SONG") { return "song"; }
    if s.contains("-DEVICE-") || s.ends_with("-DEVICE") { return "device"; }
    if s.contains("RASHGUARD") || s.contains("-RASH") { return "rashguard_ls"; }
    if s.contains("HOODIE") || s.contains("-HOOD-") || s.ends_with("-HOOD") { return "hoodie"; }
    if s.contains("CREWNECK") || s.contains("-CREW-") || s.ends_with("-CREW") { return "crewneck"; }
    if s.contains("MUSCLE-TANK") || s.contains("-TANK-") || s.ends_with("-TANK") { return "tank"; }
    if s.contains("APRON") { return "apron"; }
    if s.contains("TOTE") { return "tote"; }
    if s.contains("MUG") { return "mug"; }
    if s.contains("CANVAS") { return "canvas"; }
    if s.contains("STICKER") { return "sticker"; }
    if s.contains("POSTER") { return "poster"; }
    if s.contains("CAP-") || s.ends_with("-CAP") || s.contains("-HAT") { return "cap"; }
    if s.contains("LONG-SLEEVE") || s.contains("-LS-") || s.ends_with("-LS") { return "long_sleeve_tee"; }
    if s.contains("-TEE")  || s.starts_with("MU-")    { return "tee"; }
    if s.contains("AUTO-")  && s.contains("-TEE-")    { return "tee"; }
    "tee"  // safe default for the spec block
}

/// Brand-specific setting / mood string spliced into lifestyle prompts so
/// each brand renders with its own world rather than a generic "Japanese
/// person in Tokyo." Accepts either a catalog_brands.slug ("bjj", "kokon")
/// or a SEED_THEMES.slug ("mu_mark", "bjj_kuro_obi") — both routed by
/// substring match. Falls back to a neutral editorial backdrop.
fn brand_context(slug: &str) -> &'static str {
    let s = slug.to_lowercase();
    if s.contains("bjj") || s.contains("kuro_obi") || s.contains("roll") || s.contains("jiu") {
        "Inside a clean Tokyo BJJ dojo with bright tatami mats, traditional roll-up gear bags on a wooden bench, soft afternoon light through frosted shoji windows. The wearer is between rounds — composed, slightly damp from training."
    } else if s.contains("coffee") {
        "An independent specialty coffee bar in Daikanyama, espresso machine in background, freshly brewed cup on a wooden counter, steam still rising from a glass cortado."
    } else if s.contains("zen") {
        "A minimalist Aoyama studio apartment with a single ikebana arrangement, tatami flooring, washi-paper sliding door half-open, single sunbeam across the wood floor."
    } else if s.contains("kokon") {
        "Counter seat of a quiet Tokyo yakiniku restaurant in the evening — wooden charcoal grill, dim warm light from a paper lantern, half-finished glass of highball, faint smoke from the grill plate."
    } else if s.contains("code") {
        "Late-evening home office: dark walnut desk, mechanical keyboard with PBT keycaps, second monitor showing terminal output, single warm desk lamp, one plant in the corner."
    } else if s.contains("moon") {
        "Outdoor terrace at twilight under a near-full moon, low tatami chair, single paper lantern lit, soft cool blue tones with a hint of warm lantern glow."
    } else if s.contains("tokyo") {
        "Daikanyama side street at golden hour, low brick walls, neon sign reflection on rain-wet pavement, single passerby in soft focus background."
    } else if s.contains("kagi") {
        "Genkan (entryway) of a modern Tokyo apartment, walnut floor, smart lock on the door, leather sneakers paired neatly, soft hallway light spilling in."
    } else if s.contains("kokon") || s.contains("wagyu") {
        "Wooden counter of a Tokyo grilled-meat restaurant, charcoal embers visible, neatly plated wagyu in foreground."
    } else if s.contains("yoga") || s.contains("zen") {
        "Sunrise rooftop yoga studio in Tokyo, blonde wood floor, single mat unrolled, soft golden light."
    } else if s.contains("running") || s.contains("fitness") {
        "Tokyo riverside running path at dawn, soft mist, a runner mid-stride caught from behind, no face visible."
    } else if s.contains("mu_mark") || s == "mu" {
        "Quiet apartment morning, neutral concrete walls and pale oak floor, a single ceramic cup on a low table, Aesop / Kinfolk editorial mood — calm, deliberate, unhurried."
    } else {
        // Generic but still on-brand for wearmu: minimal, Japanese, editorial.
        "Soft natural light, minimal Tokyo backdrop with deliberate styling, magazine-cover composition, calm and uncluttered."
    }
}

/// Per-kind scene description. Variant index lets us produce v1/v2/v3 with
/// distinct framing so the gallery has variety. Falls through to a generic
/// editorial flat-lay if we don't recognise the kind.
fn scene_for_kind(kind: &str, variant: u32) -> &'static str {
    match (kind, variant) {
        ("rashguard_ls" | "rashguard_black", 1) =>
            "Practitioner from behind, sitting on a tatami mat in seiza, hands resting on knees. Camera at chest height looking at the upper back of the rashguard.",
        ("rashguard_ls" | "rashguard_black", 2) =>
            "Close-up torso shot of an MMA athlete adjusting a rashguard cuff at the wrist, no face visible.",
        ("rashguard_ls" | "rashguard_black", _) =>
            "Front-on training stance, hands wrapped, mid-warmup. Cropped at the chin so no face is visible.",
        ("hoodie", 1) =>
            "Person walking away from camera at twilight on a Tokyo side street, wearing the hoodie with hood up. No face visible.",
        ("hoodie", 2) =>
            "Folded hoodie on a wooden bench at a cafe, with a coffee cup and a paperback book beside it. Editorial flat-lay top-down.",
        ("hoodie", _) =>
            "Person seated on a step with hood up, shot from above-front, hands holding a takeaway coffee. Face obscured by the hood.",
        ("crewneck", 1) =>
            "Person at a wood desk reading a paperback, shot from neck-down at 3/4 angle, wearing the crewneck.",
        ("crewneck", _) =>
            "Folded crewneck on a linen bedsheet beside a notebook, soft morning window light.",
        ("tee", 1) =>
            "Person from neck-down sitting at a wood desk, hands typing on a laptop, wearing the black tee. Soft window light.",
        ("tee", 2) =>
            "Folded black tee on a concrete surface beside a notebook and fountain pen, top-down editorial flat-lay.",
        ("tee", _) =>
            "Person leaning on a balcony railing at golden hour, shot from waist-up, back-of-head only.",
        ("long_sleeve_tee", _) =>
            "Person at a cafe counter typing on a laptop, sleeves slightly pushed up at the wrist, no face.",
        ("tank", _) =>
            "Tank top draped over a metal gym bench beside a kettlebell, dim natural light from a side window.",
        ("apron", _) =>
            "Apron worn by a chef working at a wooden kitchen counter, chopping board with seasonal herbs, soft morning window light. Back/side view only — no face.",
        ("tote", _) =>
            "Cotton tote bag on a wooden cafe table with a paperback book and reusable coffee cup inside, top-down view.",
        ("mug", _) =>
            "Ceramic mug on a wooden cafe table beside a notebook and fountain pen, steam rising. Editorial product photography.",
        ("canvas", _) =>
            "Framed canvas on a neutral concrete wall in a minimal apartment, small succulent on a console below, soft side light.",
        ("sticker", _) =>
            "Sticker stuck on the back of a vintage MacBook in a cafe, alongside 2-3 other quality stickers, slight wear giving authenticity.",
        ("poster", _) =>
            "Poster taped at corners to a brick studio wall, low afternoon sun raking across.",
        ("cap", _) =>
            "Cap worn from behind, person walking in a Tokyo alley, no face visible.",
        _ =>
            "Editorial 4:5 product photo on a neutral concrete backdrop, photorealistic, magazine quality.",
    }
}

fn label_for_kind(kind: &str) -> &'static str {
    match kind {
        "tee" => "T シャツ",
        "rashguard_ls" => "ラッシュガード LS",
        _ => "アパレル",
    }
}

/// Admin trigger: generate N SKUs sequentially for (theme, kind).
/// Returns count of successful generations + list of new SKU ids.
pub async fn generate_batch(
    db: Db,
    theme_slug: &str,
    kind: &str,
    count: u32,
) -> serde_json::Value {
    let mut ok: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for i in 0..count {
        let seed = format!("{:08x}", rand::random::<u32>() ^ (i + 1));
        match generate_one(db.clone(), theme_slug, kind, &seed).await {
            Ok(sku) => ok.push(sku),
            Err(e) => {
                errors.push(format!("seed={} err={}", seed, e));
                if e.contains("budget cap") {
                    break; // hard stop on budget exhaustion
                }
            }
        }
    }
    let spent = {
        let conn = db.lock().unwrap();
        spent_total_jpy(&conn)
    };
    serde_json::json!({
        "ok": errors.is_empty(),
        "theme": theme_slug,
        "kind": kind,
        "requested": count,
        "created": ok.len(),
        "skus": ok,
        "errors": errors,
        "spent_total_jpy": spent,
        "budget_cap_jpy": BUDGET_TOTAL_JPY,
    })
}

#[derive(Deserialize)]
pub struct GenerateQuery {
    pub token: String,
    pub theme: String,
    pub kind: String,
    pub count: Option<u32>,
}

/// POST /admin/catalog/generate?token=&theme=&kind=&count=N
/// Token-gated trigger for the SKU generator. The 30-min cron calls
/// generate_batch directly so this endpoint is mainly for manual
/// kick-off + recovery.
pub async fn admin_generate(
    State(db): State<Db>,
    Query(q): Query<GenerateQuery>,
) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let count = q.count.unwrap_or(1).clamp(1, 50);
    let result = generate_batch(db, &q.theme, &q.kind, count).await;
    axum::Json(result).into_response()
}

#[derive(Deserialize)]
pub struct MockupBackfillQuery {
    pub token: String,
    pub brand: Option<String>,
    pub limit: Option<u32>,
}

/// GET /admin/catalog/mockup_backfill?token=&brand=&limit= — generate on-body
/// Printful mockups for "design-only" catalog SKUs (where mockup_url_external
/// still equals design_file, i.e. the shop shows the flat artwork). The 30-min
/// cron only sweeps `brand='auto'`; this lets the operator backfill any brand
/// (e.g. agent-created stores). Spawns one task per SKU and returns the queue.
pub async fn admin_mockup_backfill(
    State(db): State<Db>,
    Query(q): Query<MockupBackfillQuery>,
) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let limit = q.limit.unwrap_or(20).clamp(1, 50) as i64;
    let rows: Vec<(String, i64, i64, String)> = {
        let conn = db.lock().unwrap();
        let select = "SELECT sku, printful_product_id, printful_variant_id, COALESCE(design_file, '') \
                      FROM catalog_products \
                      WHERE is_active=1 AND printful_product_id IS NOT NULL \
                        AND (mockup_url_external = design_file OR mockup_url_external IS NULL)";
        let map_row = |r: &rusqlite::Row| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
            ))
        };
        let collected = if let Some(brand) = q.brand.as_deref() {
            let sql = format!("{select} AND brand=?1 LIMIT ?2");
            conn.prepare(&sql).ok().and_then(|mut s| {
                s.query_map(rusqlite::params![brand, limit], map_row)
                    .ok()
                    .map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>())
            })
        } else {
            let sql = format!("{select} LIMIT ?1");
            conn.prepare(&sql).ok().and_then(|mut s| {
                s.query_map(rusqlite::params![limit], map_row)
                    .ok()
                    .map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>())
            })
        };
        collected.unwrap_or_default()
    };

    let queued: Vec<String> = rows.iter().map(|(s, ..)| s.clone()).collect();
    for (sku, pp, pv, design) in rows {
        if design.is_empty() {
            continue;
        }
        let db_c = db.clone();
        tokio::spawn(async move {
            if let Err(e) = generate_onbody_mockup(db_c.clone(), sku.clone(), pp, pv, design).await {
                tracing::warn!("[catalog/mockup_backfill] {} failed: {}", sku, e);
            }
        });
    }
    axum::Json(serde_json::json!({
        "ok": true,
        "brand": q.brand,
        "queued": queued.len(),
        "skus": queued,
        "note": "mockups generate async (Printful); re-check the shop in ~1-2 min",
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct SetDesignQuery {
    pub token: String,
    pub sku: String,
    pub design_url: String,
}

/// GET /admin/catalog/set_design?token=&sku=&design_url= — replace a catalog
/// SKU's design artwork and regenerate its on-body Printful mockup. Used to
/// swap a badly-proportioned design (e.g. a small figure on a wide canvas that
/// prints as a tiny sliver) for a properly-framed one. Updates design_file +
/// resets mockup_url_external, then regenerates the mockup synchronously.
pub async fn admin_set_design(
    State(db): State<Db>,
    Query(q): Query<SetDesignQuery>,
) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let design = q.design_url.trim();
    if !design.starts_with("https://") {
        return (StatusCode::BAD_REQUEST, "design_url must be https").into_response();
    }
    let ids: Option<(i64, i64)> = {
        let conn = db.lock().unwrap();
        let updated = conn
            .execute(
                "UPDATE catalog_products SET design_file=?1, mockup_url_external=?1 WHERE sku=?2",
                rusqlite::params![design, q.sku],
            )
            .unwrap_or(0);
        if updated == 0 {
            None
        } else {
            conn.query_row(
                "SELECT printful_product_id, printful_variant_id FROM catalog_products WHERE sku=?1",
                rusqlite::params![q.sku],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .ok()
        }
    };
    let Some((pp, pv)) = ids else {
        return (StatusCode::NOT_FOUND, "sku not found").into_response();
    };
    let regen = generate_onbody_mockup(db.clone(), q.sku.clone(), pp, pv, design.to_string()).await;
    axum::Json(serde_json::json!({
        "ok": regen.is_ok(),
        "sku": q.sku,
        "design_url": design,
        "mockup_regen": regen.err(),
    }))
    .into_response()
}

#[derive(serde::Deserialize)]
pub struct BrandVisQuery {
    pub token: String,
    pub brand: String,
    #[serde(default)]
    pub live: i64,
}

/// GET /admin/catalog/brand_visibility?token=…&brand=muon&live=1
/// One-request publish / rollback for a whole catalog brand — no redeploy.
/// live=1 → brand+all SKUs is_active=1/status='live' (公開).
/// live=0 (default) → is_active=0/status='draft' (即・非公開に戻す).
pub async fn admin_brand_visibility(
    State(db): State<Db>,
    Query(q): Query<BrandVisQuery>,
) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let live = q.live != 0;
    let (active, status) = if live { (1, "live") } else { (0, "draft") };
    let (nb, np) = {
        let conn = db.lock().unwrap();
        let nb = conn
            .execute(
                "UPDATE catalog_brands SET is_active=?1 WHERE slug=?2",
                rusqlite::params![active, q.brand],
            )
            .unwrap_or(0);
        let np = conn
            .execute(
                "UPDATE catalog_products SET is_active=?1, status=?2 WHERE brand=?3",
                rusqlite::params![active, status, q.brand],
            )
            .unwrap_or(0);
        (nb, np)
    };
    if nb == 0 {
        return (StatusCode::NOT_FOUND, "brand not found").into_response();
    }
    tracing::info!("[catalog] brand '{}' visibility → live={} ({} SKUs)", q.brand, live, np);
    axum::Json(serde_json::json!({
        "ok": true,
        "brand": q.brand,
        "live": live,
        "brand_rows": nb,
        "product_rows": np,
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct LifestyleGenQuery {
    pub token: String,
    pub sku: String,
    pub variant: Option<u32>,
}

/// GET /admin/catalog/lifestyle_gen?token=&sku=<sku>&variant=<n>
///
/// Manually trigger one lifestyle photo for an existing SKU. Used to
/// validate Gemini output quality on a small sample before flipping the
/// cron lifestyle_backfill_step to non-auto brands. Charges ¥6 to
/// catalog_spend per call (same path as the cron).
pub async fn admin_lifestyle_gen(
    State(db): State<Db>,
    Query(q): Query<LifestyleGenQuery>,
) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let variant = q.variant.unwrap_or(1);

    let row: Option<(String, String, String)> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT brand, COALESCE(label, ''), COALESCE(description_ja, '')
             FROM catalog_products WHERE sku=?",
            rusqlite::params![&q.sku],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).ok()
    };
    let Some((brand, label, desc)) = row else {
        return (StatusCode::NOT_FOUND, format!("sku {} not found", q.sku)).into_response();
    };
    let kind = kind_from_sku(&q.sku).to_string();
    let brief = if !desc.is_empty() { desc } else { label.clone() };

    match generate_lifestyle_photo(db.clone(), q.sku.clone(), brand.clone(),
                                   brief.clone(), kind.clone(), variant).await {
        Ok(()) => axum::Json(serde_json::json!({
            "ok": true, "sku": q.sku, "variant": variant,
            "brand": brand, "kind": kind,
        })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(serde_json::json!({
            "ok": false, "sku": q.sku, "error": e,
        }))).into_response(),
    }
}

#[derive(Deserialize)]
pub struct MarkMailedQuery {
    pub token: Option<String>,
}

/// GET /admin/catalog/founder/:number/mark_mailed?token=
/// Yuki clicks this from the action-item email after he signs + posts
/// the physical card. Sets mailed_at on the row.
pub async fn admin_mark_mailed(
    State(db): State<Db>,
    Path(number): Path<i64>,
    Query(q): Query<MarkMailedQuery>,
) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    let provided = q.token.unwrap_or_default();
    if expected.is_empty() || provided != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let n = {
        let conn = db.lock().unwrap();
        conn.execute(
            "UPDATE catalog_founder_cards
             SET mailed_at = datetime('now')
             WHERE number = ? AND mailed_at IS NULL",
            rusqlite::params![number],
        )
        .unwrap_or(0)
    };
    if n == 0 {
        return (StatusCode::NOT_FOUND, format!("card #{} not found or already mailed", number))
            .into_response();
    }
    Html(format!(
        r#"<html><body style="font-family:monospace;padding:40px;background:#0a0a0a;color:#ffd700;font-size:14px">
        ✓ Card #{}/100 marked as mailed at {}<br>
        <a href="/admin/catalog/status?token={}" style="color:#ffd700">← back to status</a>
        </body></html>"#,
        number,
        chrono_now_iso(),
        expected,
    ))
    .into_response()
}

fn chrono_now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{}", t)
}

#[derive(Deserialize)]
pub struct LegacyRenameQuery {
    pub token: String,
    /// Safety knob — must be set to "rename-yes-i-checked-the-mirrors"
    /// so a curl typo can't trigger an irreversible rename.
    pub confirm: String,
}

/// GET /admin/catalog/legacy_rename?token=…&confirm=…
/// Phase C of the migration. Renames each legacy product table to
/// `_legacy_<name>`. Reversible by hand (`ALTER TABLE _legacy_x RENAME TO x`)
/// for ~30 days, after which Phase D drops them.
pub async fn admin_legacy_rename(
    State(db): State<Db>,
    Query(q): Query<LegacyRenameQuery>,
) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    if q.confirm != "rename-yes-i-checked-the-mirrors" {
        return (StatusCode::BAD_REQUEST,
            "confirm must be 'rename-yes-i-checked-the-mirrors'").into_response();
    }
    let out = {
        let conn = db.lock().unwrap();
        rename_legacy_tables(&conn)
    };
    axum::Json(serde_json::json!({"renamed": out})).into_response()
}

/// 封印ドロップ作成: 暗号化はクライアント側(timelock-web)で済ませ、ここには
/// age 暗号文(ciphertext)と解禁時刻(unlock_iso)だけが届く。サーバーは平文を見ない。
/// status='draft' で作るので公開棚には出ない(直URLで確認→人が live に上げる)。
#[derive(Deserialize)]
pub struct SealCreateQuery {
    pub token: String,
    pub sku: String,
    pub label: String,
    pub ciphertext: String,
    pub unlock_iso: String,
    #[serde(default)]
    pub price_jpy: Option<i64>,
    #[serde(default)]
    pub brand: Option<String>,
}

/// GET /admin/catalog/seal — 封印ドロップ(時限ドロップ)を1件作成。
pub async fn admin_seal_create(
    State(db): State<Db>,
    Query(q): Query<SealCreateQuery>,
) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    if !q.ciphertext.contains("BEGIN AGE ENCRYPTED FILE") {
        return (StatusCode::BAD_REQUEST, "ciphertext must be an age timelock blob").into_response();
    }
    if q.unlock_iso.trim().is_empty() || !q.unlock_iso.contains('T') {
        return (StatusCode::BAD_REQUEST, "unlock_iso must be RFC3339 (e.g. 2026-12-25T00:00:00Z)").into_response();
    }
    // sku は [A-Za-z0-9_-] のみ許可(PK安全)
    let sku = q.sku.trim().to_string();
    if sku.is_empty() || !sku.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return (StatusCode::BAD_REQUEST, "sku must be [A-Za-z0-9_-]").into_response();
    }
    let brand = q.brand.clone().unwrap_or_else(|| "minna".to_string());
    let price = q.price_jpy.unwrap_or(0);
    let meta = serde_json::json!({ "unlock_iso": q.unlock_iso }).to_string();
    let conn = db.lock().unwrap();
    let res = conn.execute(
        "INSERT INTO catalog_products
         (sku, brand, label, description_ja, retail_price_jpy,
          printful_product_id, printful_variant_id, is_active, meta_json, status)
         VALUES (?, ?, ?, ?, ?, 0, 0, 1, ?, 'draft')",
        rusqlite::params![sku, brand, q.label, q.ciphertext, price, meta],
    );
    match res {
        Ok(_) => axum::Json(serde_json::json!({
            "ok": true, "sku": sku, "status": "draft",
            "url": format!("/shop/{}", sku),
            "note": "status=draft。確認後 live に上げてください(公開棚に出ません)"
        })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("insert failed: {}", e)).into_response(),
    }
}

#[derive(Deserialize)]
pub struct NlAddQuery {
    pub token: String,
    /// Free-form JP/EN description, e.g.
    /// "BJJ 黒帯 sumi-e Tシャツ ¥4900" or
    /// "Coffee × Code rashguard ¥9,800, black canvas"
    pub prompt: String,
    /// Optional brand slug (default 'auto'). Use this to drop a SKU into
    /// a specific catalog_brands row — e.g. brand='jiuflow' for the
    /// MU × jiuflow rashguard collab. The brand row must already exist
    /// in catalog_brands; new ones aren't auto-created here.
    pub brand: Option<String>,
    /// Optional collab partner name, prepended to the SKU label as
    /// "{collab} × {display}". Use for cross-brand drops where the
    /// PDP should call out both the host brand and MU.
    pub collab: Option<String>,
}

/// GET /admin/catalog/nl?token=…&prompt=… (POST also accepted via body).
/// Natural-language SKU creation. Asks Gemini text-mode to parse the
/// prompt into a {theme_brief, kind, retail_jpy, name} JSON, then runs
/// the existing generate_one() path with a synthetic ad-hoc theme.
///
/// Costs ¥1 (Gemini text parse) + ¥12 (the standard 4-image pipeline)
/// = ¥13 per nl-add SKU.
pub async fn admin_nl_add(
    State(db): State<Db>,
    Query(q): Query<NlAddQuery>,
) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let prompt_in = q.prompt.trim();
    if prompt_in.is_empty() {
        return (StatusCode::BAD_REQUEST, "prompt is required").into_response();
    }
    let parse_prompt = format!(
        "Parse this JP/EN product idea into compact JSON. ONLY emit JSON, \
         no prose, no markdown fences.\n\
         Schema: {{\"kind\":\"tee|rashguard_ls|rashguard_black|hoodie|crewneck\", \
                   \"theme_brief\":\"<one short English design brief for the chest graphic>\", \
                   \"display\":\"<short JP brand-mark name>\", \
                   \"hook\":\"<one JP marketing sentence for the PDP>\", \
                   \"retail_jpy\":<integer>}}\n\
         If kind is missing, default to 'tee'. \
         If retail_jpy is missing, default to 4900 for tee, 9800 for rashguard, 8800 for hoodie. \
         If display is missing, infer from theme_brief in <=10 JP chars.\n\
         Input: {}",
        prompt_in
    );
    let parsed_json = match crate::gemini::call_gemini_text(&parse_prompt).await {
        Ok(s) => s,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY,
                format!("gemini parse failed: {}", e)).into_response();
        }
    };
    // Extract {...} from the response (Gemini sometimes wraps with prose
    // even though we asked it not to).
    let json_str: String = parsed_json.find('{').and_then(|i| {
        parsed_json[i..].rfind('}').map(|j| parsed_json[i..i + j + 1].to_string())
    }).unwrap_or(parsed_json.clone());
    let parsed: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, format!(
                "gemini returned non-JSON: {} (raw: {})",
                e, parsed_json.chars().take(300).collect::<String>()
            )).into_response();
        }
    };
    let kind = parsed["kind"].as_str().unwrap_or("tee");
    let theme_brief = parsed["theme_brief"].as_str().unwrap_or(prompt_in);
    let display = parsed["display"].as_str().unwrap_or("Custom");
    let hook = parsed["hook"].as_str().unwrap_or("自然言語から自動生成");
    let retail_jpy = parsed["retail_jpy"].as_i64().unwrap_or_else(|| {
        PRODUCT_SPECS.iter().find(|s| s.kind == kind)
            .map(|s| s.retail_jpy).unwrap_or(4900)
    });

    // Validate kind.
    let Some(spec) = PRODUCT_SPECS.iter().find(|s| s.kind == kind) else {
        return (StatusCode::BAD_REQUEST,
            format!("unknown kind '{}', allowed: tee/rashguard_ls/rashguard_black/hoodie/crewneck", kind)).into_response();
    };

    // Generate a deterministic-enough seed from the prompt + clock.
    let seed = format!("nl{:08x}", rand::random::<u32>());
    let slug = display
        .chars().filter(|c| c.is_ascii_alphanumeric())
        .take(12).collect::<String>()
        .to_uppercase();
    let slug = if slug.is_empty() { "NL".to_string() } else { slug };
    // SKU prefix: AUTO-NL-… for the default brand, BRAND-MU-NL-… for collab
    // drops (so e.g. "JIUFLOW-MU-NL-KIMURA-RASHGUARD-LS-…" is self-describing).
    let brand_slug_raw = q.brand.as_deref().unwrap_or("auto").to_lowercase();
    let brand_for_sku: String = brand_slug_raw.chars()
        .filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_uppercase();
    let sku = if brand_slug_raw == "auto" {
        format!("AUTO-NL-{}-{}-{}", slug, kind.to_uppercase().replace('_', "-"), seed)
    } else {
        format!("{}-MU-NL-{}-{}-{}", brand_for_sku, slug,
                kind.to_uppercase().replace('_', "-"), seed)
    };

    // Direct-insert (skip generate_one's strict theme lookup since this
    // is an ad-hoc one) with retail_jpy override. The 4 background image
    // tasks fire the same way.
    let charged = {
        let conn = db.lock().unwrap();
        spend_or_refuse(&conn, "ai_image", GEMINI_IMAGE_COST_JPY,
            &format!("nl_add sku={}", sku), Some(&sku))
    };
    if !charged {
        return (StatusCode::FAILED_DEPENDENCY, "budget cap reached").into_response();
    }
    // For AOP rashguards the same image is cover-filled across all four
    // sublimation panels (front/back/sleeves), so the canvas needs to be
    // fully colored edge-to-edge — a white-background chest graphic would
    // ship as a white rashguard with a tiny print, defeating the belt-color
    // proposition. DTG products keep the white-background spec.
    let is_aop = matches!(kind, "rashguard_ls" | "rashguard_black");
    let design_prompt = if is_aop {
        format!(
            "Print-ready FULL-CANVAS sublimation artwork at 300 DPI for an \
             all-over-print rashguard. CRITICAL: fill the ENTIRE canvas \
             edge-to-edge with the dominant color — NO white margins, NO \
             padding, NO background gaps. Style brief: {}. The artwork \
             will be cover-cropped onto every panel (front, back, both \
             sleeves), so corners and edges matter as much as the center. \
             NO model, NO garment mockup, just the printable artwork. \
             Variation key: {}.",
            theme_brief, seed
        )
    } else {
        format!(
            "Print-ready chest graphic at 300 DPI on a pure white background. \
             Style brief: {}. NO model, NO mockup, just the artwork, centered. \
             Variation key: {}.",
            theme_brief, seed
        )
    };
    let img = match crate::gemini::call_gemini(&design_prompt).await {
        Ok(i) => i,
        Err(e) => return (StatusCode::BAD_GATEWAY, format!("gemini image: {}", e)).into_response(),
    };
    let key = format!("catalog/{}.png", sku);
    let Some(url) = crate::store_r2_bytes(&key, &img.bytes, &img.mime).await else {
        return (StatusCode::BAD_GATEWAY, "R2 upload failed").into_response();
    };
    {
        let conn = db.lock().unwrap();
        // Only auto-create the 'auto' brand row — for explicit brands the
        // operator is expected to have seeded the catalog_brands row already
        // (so we don't accidentally spawn typo'd brand slugs).
        if brand_slug_raw == "auto" {
            let _ = conn.execute(
                "INSERT OR IGNORE INTO catalog_brands
                 (slug, name, emoji, color_primary, tagline, is_active, revenue_share_pct)
                 VALUES ('auto', 'AUTO (AI-generated)', '🤖', '#ffd700',
                         'Gemini × Printful POD · 30 分自動生成', 1, 0)",
                [],
            );
        }
        let desc = match q.collab.as_deref() {
            Some(c) if !c.is_empty() => format!("{} × {} — {}", c, display, hook),
            _ => format!("{} — {}", display, hook),
        };
        let legacy = match q.collab.as_deref() {
            Some(c) if !c.is_empty() => format!("nl_add_collab_{}", c.to_lowercase()),
            _ => "nl_add".to_string(),
        };
        let _ = conn.execute(
            "INSERT INTO catalog_products (
                sku, brand, label, description_ja, retail_price_jpy,
                printful_product_id, printful_variant_id, printful_placement,
                printful_print_w, printful_print_h,
                design_file, mockup_main_file, mockup_url_external,
                is_active, sort_order, status, fulfillment_route, legacy_source
             ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            rusqlite::params![
                &sku, &brand_slug_raw, desc, desc, retail_jpy,
                spec.printful_product_id, spec.printful_variant_id, spec.placement,
                0, 0,
                &url, &url, &url,
                1, 50,
                "live",
                if matches!(kind, "rashguard_ls"|"rashguard_black") { "printful_aop" } else { "printful_dtg" },
                &legacy,
            ],
        );
    }
    // Spawn the 3 background image tasks (transparent / Printful mockup /
    // lifestyle) so the SKU lands fully-loaded in ~60-90s.
    let pp = spec.printful_product_id;
    let pv = spec.printful_variant_id;
    let url_c = url.clone();
    let sku_b = sku.clone();
    let sku_c = sku.clone();
    let sku_d = sku.clone();
    let db_b = db.clone();
    let db_c = db.clone();
    let db_d = db.clone();
    let bytes_b = img.bytes.clone();
    let kind_d = kind.to_string();
    let theme_brief_d = theme_brief.to_string();
    let display_d = display.to_string();
    tokio::spawn(async move {
        let _ = generate_transparent_print(db_b, sku_b, bytes_b).await;
    });
    tokio::spawn(async move {
        let _ = generate_onbody_mockup(db_c, sku_c, pp, pv, url_c).await;
    });
    tokio::spawn(async move {
        let _ = generate_lifestyle_photo(db_d, sku_d, display_d, theme_brief_d, kind_d, 1).await;
    });

    axum::Json(serde_json::json!({
        "ok": true,
        "sku": sku,
        "kind": kind,
        "retail_jpy": retail_jpy,
        "display": display,
        "hook": hook,
        "theme_brief": theme_brief,
        "pdp_url": format!("https://wearmu.com/shop/{}", sku),
        "buy_url": format!("https://wearmu.com/api/shop/checkout?sku={}", sku),
        "note": "background: 透過 + Printful mockup + lifestyle landing within ~60-90s",
    })).into_response()
}

// ───────────────────────── public "say it and MU makes it" (/make) ─────────────────────────

#[derive(serde::Deserialize)]
pub struct MakeQuery {
    pub prompt: String,
    pub kind: Option<String>,
    /// A/B/C バリアント（a|b|c）。/make の割当をそのまま投稿に刻む。
    #[serde(default)]
    pub v: Option<String>,
    /// ユニーク訪問者ID（mu-funnel.js の visitor_id）。UU勝者判定の母数。
    #[serde(default)]
    pub visitor: Option<String>,
}

/// GET /make のクエリ。?v= でバリアント固定（勝者確定後はサーバが上書き）。
#[derive(serde::Deserialize)]
pub struct MakePageQuery {
    #[serde(default)]
    pub v: Option<String>,
}

/// /make A/B/C: 勝者UU到達のしきい値（ユニーク訪問者の作成数）。
const MAKE_AB_WIN_THRESHOLD: i64 = 100;

fn make_variant_norm(v: Option<&str>) -> Option<&'static str> {
    match v.map(|s| s.trim().to_lowercase()).as_deref() {
        Some("a") => Some("a"),
        Some("b") => Some("b"),
        Some("c") => Some("c"),
        _ => None,
    }
}

/// cv_config 読み取り（catalog から直接。main.rs の cv_set と対）。
fn cv_get(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM cv_config WHERE key=?", rusqlite::params![key], |r| r.get(0)).ok()
}
fn cv_put(conn: &rusqlite::Connection, key: &str, value: &str, reason: &str) {
    let _ = conn.execute(
        "INSERT INTO cv_config (key, value, updated_at, reason) VALUES (?,?,strftime('%s','now'),?)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at, reason=excluded.reason",
        rusqlite::params![key, value, reason],
    );
}

// ── 声でつなぐ（Koe連携: 人もエージェントも声でつなげる入口） ──
fn mu_connect_link(name: &str) -> (String, String, String) {
    let h: String = name.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(48).collect();
    let h = if h.is_empty() {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0) as u32;
        format!("mu-{:x}", n)
    } else { h };
    let link = format!("https://yukihamada.jp/k/{}", h);
    let prompt = format!("!open {}", link);
    (h, link, prompt)
}

#[derive(serde::Deserialize)]
pub struct MuConnectQ { #[serde(default)] pub name: String }

/// GET/POST /api/connect?name= — エージェントが声でつなぐリンクを生成できる。CORS *。
pub async fn api_connect(Query(q): Query<MuConnectQ>) -> Response {
    let (room, link, prompt) = mu_connect_link(&q.name);
    ([("access-control-allow-origin", "*")], axum::Json(serde_json::json!({
        "ok": true, "name": q.name, "room": room, "link": link, "prompt": prompt,
        "enter_url": format!("https://yukihamada.jp/room/{}", room),
        "presence_url": format!("https://yukihamada.jp/api/room/{}/presence", room),
        "note": "Open the link (or run the prompt in Claude Code) on both sides to connect by voice. Up to 6."
    }))).into_response()
}

/// GET /connect — 声でつなぐ UI（MUブランド）。
pub async fn connect_page() -> Html<String> {
    Html(r##"<!doctype html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<title>━◯━ MU — 声でつなぐ</title><meta name="description" content="名前を入れるだけ。リンクを送って、ひらいたら声でつながる。">
<style>*{margin:0;padding:0;box-sizing:border-box}body{background:#0a0a0a;color:#f5f5f0;font-family:'Helvetica Neue','Hiragino Sans',Arial,sans-serif;min-height:100dvh;line-height:1.7;background:radial-gradient(60% 45% at 50% 0%,rgba(255,215,0,.12),transparent 70%),#0a0a0a}
a{color:inherit;text-decoration:none}nav{display:flex;justify-content:space-between;align-items:center;padding:14px 22px;border-bottom:1px solid rgba(255,255,255,.08)}nav .b{font-weight:900;letter-spacing:.3em}
.wrap{max-width:520px;margin:0 auto;padding:52px 22px 80px}.kick{font-size:11px;letter-spacing:.4em;color:#ffd700;text-transform:uppercase;text-align:center}
h1{font-size:34px;font-weight:800;text-align:center;margin:14px 0 6px}.sub{color:rgba(245,245,240,.6);font-size:14px;text-align:center;margin-bottom:30px}.sub b{color:#f5f5f0}
label{display:block;font-size:12px;letter-spacing:.06em;color:rgba(245,245,240,.5);margin:18px 0 7px}
input{width:100%;background:#141414;border:1px solid rgba(255,255,255,.14);border-radius:12px;padding:15px 16px;color:#f5f5f0;font-size:17px}input:focus{outline:none;border-color:#ffd700}
.btn{display:block;width:100%;margin-top:16px;background:#ffd700;color:#0a0a0a;border:0;border-radius:12px;padding:16px;font-size:17px;font-weight:800;cursor:pointer;text-align:center}.btn.s{background:transparent;color:#ffd700;border:1px solid rgba(255,215,0,.4)}
.panel{display:none;margin-top:24px;background:rgba(255,255,255,.03);border:1px solid rgba(255,255,255,.08);border-radius:16px;padding:20px}.panel.show{display:block}
.lk{background:#111;border:1px solid #2a2a2a;border-radius:10px;padding:12px 14px;font-size:13px;color:#ffd700;word-break:break-all;margin:8px 0 4px}
.share{display:flex;gap:8px;flex-wrap:wrap;margin-top:10px}.share a,.share button{flex:1;min-width:84px;text-align:center;background:#161616;border:1px solid #2a2a2a;border-radius:9px;padding:11px 8px;color:#f5f5f0;font-size:13px;cursor:pointer;font-family:inherit}
.status{margin-top:16px;text-align:center;font-size:15px;color:rgba(245,245,240,.6)}.status.on{color:#ffd700;font-weight:700}.dot{display:inline-block;width:8px;height:8px;border-radius:50%;background:#666;margin-right:7px}.status.on .dot{background:#ffd700;animation:p 1.6s infinite}@keyframes p{70%{box-shadow:0 0 0 9px rgba(255,215,0,0)}}
.hint{font-size:12px;color:rgba(245,245,240,.4);text-align:center;margin-top:24px;line-height:1.9}.hint a{color:#ffd700}</style></head><body>
<nav><a class="b" href="/">━◯━ MU</a><a href="/store" style="font-size:12px;color:#8a8a84">SHOP ↩</a></nav>
<div class="wrap"><div class="kick">MU · 声でつなぐ</div><h1>声でつなぐ。</h1>
<div class="sub">名前を入れるだけ。<b>リンクを送って、ひらいたら声でつながる。</b></div>
<label>だれとつなぐ？</label><input id="name" placeholder="例：けんたろう" maxlength="40" autocomplete="off"><button class="btn" id="make">つなぐリンクを作る</button>
<div class="panel" id="panel"><div style="font-size:13px;color:rgba(245,245,240,.55)"><b id="who"></b> とつなぐ部屋ができました。</div>
<div class="lk" id="link"></div><div class="share"><button id="sh">📣 共有</button><a id="line" target="_blank" rel="noopener">LINE</a><a id="sms">SMS</a><a id="mail">メール</a><button id="cp">コピー</button></div>
<label style="margin-top:18px">どこでも貼れる（Claude Code / Slack / メモ）</label><div class="lk" id="prompt"></div><button class="btn s" id="cpp" style="margin-top:8px">このプロンプトをコピー</button>
<a class="btn" id="enter" target="_blank" style="margin-top:14px">▶ 自分が今すぐ入る</a><div class="status" id="status"><span class="dot"></span>あなたを待っています…</div></div>
<div class="hint">声・顔・画面共有・チャット対応（最大6人）。同じリンクを開いた人が自動でつながります。<br><a href="/store">← MU MAKE 無人店へ</a></div></div>
<script>var BASE='https://yukihamada.jp';var $=function(s){return document.getElementById(s)};function rid(){var c='abcdefghijkmnpqrstuvwxyz23456789',o='';for(var i=0;i<8;i++)o+=c[Math.floor(Math.random()*c.length)];return o;}
var room='',shortUrl='',prompt='',poll=null;function mk(){var nm=$('name').value.trim()||'相手';var h=nm.toLowerCase().replace(/[^a-z0-9-]/g,'');if(!h)h='mu-'+rid();room=h;shortUrl=BASE+'/k/'+h;prompt='!open '+shortUrl;$('who').textContent=nm;$('link').textContent=shortUrl;$('prompt').textContent=prompt;$('enter').href=BASE+'/room/'+room;var msg='声でつなぎたい。これ開いて → '+shortUrl;$('line').href='https://line.me/R/share?text='+encodeURIComponent(msg);$('sms').href='sms:?&body='+encodeURIComponent(msg);$('mail').href='mailto:?subject='+encodeURIComponent('声でつなぎたい')+'&body='+encodeURIComponent(msg);$('sh').onclick=function(){if(navigator.share){navigator.share({title:'声でつなぐ',text:msg,url:shortUrl}).catch(function(){});}else{cp();}};$('cp').onclick=cp;$('cpp').onclick=cpp;$('panel').classList.add('show');if(poll)clearInterval(poll);poll=setInterval(checkp,4000);checkp();}
function cp(){navigator.clipboard&&navigator.clipboard.writeText(shortUrl).then(function(){$('cp').textContent='コピー済 ✓';setTimeout(function(){$('cp').textContent='コピー';},1500);});}function cpp(){navigator.clipboard&&navigator.clipboard.writeText(prompt).then(function(){$('cpp').textContent='コピー済 ✓';setTimeout(function(){$('cpp').textContent='このプロンプトをコピー';},1500);});}
function checkp(){fetch(BASE+'/api/room/'+room+'/presence',{cache:'no-store'}).then(function(r){return r.json();}).then(function(d){var n=d.count||0,s=$('status');if(n>=2){s.className='status on';s.innerHTML='<span class=dot></span>🎉 つながりました（'+n+'人）';}else if(n===1){s.className='status on';s.innerHTML='<span class=dot></span>あなたが入室中 — 相手を待っています';}else{s.className='status';s.innerHTML='<span class=dot></span>リンクを送って、ふたりで開いてください';}}).catch(function(){});}
$('make').onclick=mk;$('name').addEventListener('keydown',function(e){if(e.key==='Enter')mk();});</script></body></html>"##.to_string())
}

/// GET /store — 「ガチの無人店舗」。24時間 AI だけが運営する受注生産Tシャツ屋の入口。
/// ライブ在庫(catalog_products is_active=1)を実数で見せ、/make(話して作る)と /shop(棚) に繋ぐ。
pub async fn store_unmanned_page(State(db): State<Db>) -> Html<String> {
    let (live, brands, sold, cards, ticker) = {
        let conn = db.lock().unwrap();
        let live: i64 = conn
            .query_row("SELECT COUNT(*) FROM catalog_products WHERE is_active=1", [], |r| r.get(0))
            .unwrap_or(0);
        let brands: i64 = conn
            .query_row("SELECT COUNT(DISTINCT brand) FROM catalog_products WHERE is_active=1", [], |r| r.get(0))
            .unwrap_or(0);
        let sold: i64 = conn
            .query_row("SELECT COUNT(*) FROM catalog_orders WHERE status='submitted'", [], |r| r.get(0))
            .unwrap_or(0);
        let items = list_products_paged(&conn, None, 12, 0, "", "", None);
        let cards = items
            .iter()
            .map(|p| {
                let img = p.img.clone().unwrap_or_default();
                let imgtag = if img.is_empty() {
                    "<div class=ph>━◯━</div>".to_string()
                } else {
                    format!("<img loading=lazy src=\"{}\" alt=\"\">", html_text(&img))
                };
                format!(
                    r#"<a class="c" href="/p/{sku}"><div class="ci">{imgtag}</div><div class="cb"><div class="cn">{name}</div><div class="cp">¥{price}</div></div></a>"#,
                    sku = html_text(&p.sku), imgtag = imgtag, name = html_text(&p.desc), price = p.price
                )
            })
            .collect::<String>();
        let one: String = items
            .iter()
            .filter_map(|p| p.img.clone())
            .filter(|s| !s.is_empty())
            .map(|u| format!("<img src=\"{}\" alt=\"\">", html_text(&u)))
            .collect();
        let ticker = format!("{one}{one}"); // 2連結でシームレスにループ
        (live, brands, sold, cards, ticker)
    };
    let cards = if cards.is_empty() {
        "<div class=empty>いま棚を補充中…</div>".to_string()
    } else {
        cards
    };
    Html(format!(r##"<!doctype html><html lang="ja"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<title>MU MAKE 無人店 — 店員ゼロ、AIだけのTシャツ店 · wearmu.com</title>
<meta name="description" content="MU MAKE 無人店。店員はいない。AIが描いて、刷って、あなたに送る。24時間営業・在庫ゼロ・受注生産。なんでも言ってみ、Tシャツになるから。">
<meta property="og:title" content="MU MAKE 無人店 — 店員ゼロ、AIだけのTシャツ店">
<meta property="og:description" content="店員はいない。AIが描いて、刷って、送る。なんでも言ってみ、Tシャツになるから。">
<meta name="theme-color" content="#0a0a0a">
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
:root{{--gold:#ffd700;--ink:#f5f5f0;--mut:#8c8c84}}
html{{scroll-behavior:smooth}}
body{{background:#0a0a0a;color:var(--ink);font-family:'Helvetica Neue','Hiragino Sans',Arial,sans-serif;line-height:1.7;min-height:100dvh;-webkit-font-smoothing:antialiased}}
a{{color:inherit;text-decoration:none}}
nav{{position:sticky;top:0;z-index:30;display:flex;justify-content:space-between;align-items:center;padding:14px 22px;border-bottom:1px solid rgba(255,255,255,.08);background:rgba(10,10,10,.82);backdrop-filter:blur(10px)}}
nav .bm{{font-weight:900;letter-spacing:.4em;font-size:15px}}
nav .nl a{{font-size:12px;letter-spacing:.12em;color:var(--mut);margin-left:18px}}
nav .nl a:hover{{color:var(--ink)}}
.hero{{position:relative;padding:84px 22px 64px;text-align:center;overflow:hidden}}
.hero::before{{content:"";position:absolute;inset:0;background:radial-gradient(60% 50% at 50% 0%,rgba(255,215,0,.10),transparent 70%);pointer-events:none}}
.kick{{font-size:11px;letter-spacing:.42em;color:var(--gold);text-transform:uppercase;margin-bottom:18px}}
.hero h1{{font-size:clamp(34px,8vw,68px);font-weight:900;line-height:1.04;letter-spacing:.01em}}
.hero h1 .o{{color:var(--gold)}}
.hero .sub{{max-width:620px;margin:20px auto 8px;color:rgba(245,245,240,.82);font-size:clamp(14px,3.6vw,17px)}}
.hero .en{{max-width:620px;margin:0 auto 30px;color:var(--mut);font-size:12.5px;letter-spacing:.02em}}
.live{{display:inline-flex;gap:18px;flex-wrap:wrap;justify-content:center;margin:4px auto 30px;padding:12px 22px;border:1px solid rgba(255,215,0,.28);border-radius:999px;background:rgba(255,215,0,.05);font-size:12.5px}}
.live b{{color:var(--gold);font-family:monospace;font-size:15px}}
.live .dot{{display:inline-block;width:7px;height:7px;border-radius:50%;background:#37d67a;margin-right:7px;box-shadow:0 0 0 0 rgba(55,214,122,.7);animation:p 1.8s infinite}}
@keyframes p{{0%{{box-shadow:0 0 0 0 rgba(55,214,122,.6)}}70%{{box-shadow:0 0 0 9px rgba(55,214,122,0)}}100%{{box-shadow:0 0 0 0 rgba(55,214,122,0)}}}}
.cta{{display:flex;gap:12px;justify-content:center;flex-wrap:wrap}}
.btn{{display:inline-block;padding:15px 30px;border-radius:8px;font-weight:800;font-size:15px;letter-spacing:.02em}}
.btn.p{{background:var(--gold);color:#0a0a0a;box-shadow:0 8px 34px rgba(255,215,0,.28)}}
.btn.s{{border:1px solid rgba(255,255,255,.22);color:var(--ink)}}
.ticker{{overflow:hidden;border-top:1px solid rgba(255,255,255,.06);border-bottom:1px solid rgba(255,255,255,.06);background:#050505;padding:10px 0;-webkit-mask-image:linear-gradient(90deg,transparent,#000 8%,#000 92%,transparent);mask-image:linear-gradient(90deg,transparent,#000 8%,#000 92%,transparent)}}
.ticker .track{{display:flex;gap:10px;width:max-content;animation:scroll 48s linear infinite}}
.ticker:hover .track{{animation-play-state:paused}}
.ticker img{{height:96px;width:96px;object-fit:cover;border-radius:8px;border:1px solid rgba(255,255,255,.08);flex:none}}
@keyframes scroll{{to{{transform:translateX(-50%)}}}}
.sec{{max-width:1040px;margin:0 auto;padding:54px 20px}}
.sec h2{{font-size:13px;letter-spacing:.28em;color:var(--mut);text-transform:uppercase;text-align:center;margin-bottom:34px}}
.steps{{display:grid;grid-template-columns:repeat(3,1fr);gap:16px}}
@media(max-width:680px){{.steps{{grid-template-columns:1fr}}}}
.step{{background:rgba(255,255,255,.025);border:1px solid rgba(255,255,255,.07);border-radius:14px;padding:26px 22px}}
.step .n{{font-family:monospace;color:var(--gold);font-size:12px;letter-spacing:.2em}}
.step h3{{font-size:18px;font-weight:800;margin:10px 0 8px}}
.step p{{color:rgba(245,245,240,.66);font-size:13.5px}}
.grid{{display:grid;grid-template-columns:repeat(auto-fill,minmax(160px,1fr));gap:12px}}
.c{{background:rgba(255,255,255,.02);border:1px solid rgba(255,255,255,.08);border-radius:12px;overflow:hidden;display:flex;flex-direction:column;transition:border-color .18s}}
.c:hover{{border-color:rgba(255,215,0,.4)}}
.ci{{aspect-ratio:1/1;background:#000;display:block;overflow:hidden}}
.ci img{{width:100%;height:100%;object-fit:cover;display:block}}
.ci .ph{{width:100%;height:100%;display:flex;align-items:center;justify-content:center;color:#333;letter-spacing:.3em}}
.cb{{padding:10px 12px 13px;flex:1;display:flex;flex-direction:column;gap:6px}}
.cn{{font-size:12.5px;line-height:1.45;flex:1;color:rgba(245,245,240,.9)}}
.cp{{font-size:13px;font-weight:700;font-family:monospace}}
.empty{{text-align:center;color:var(--mut);padding:40px}}
.shelf-more{{text-align:center;margin-top:26px}}
.why{{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:14px}}
.why div{{border:1px solid rgba(255,255,255,.07);border-radius:12px;padding:18px;font-size:13px;color:rgba(245,245,240,.78)}}
.why div b{{display:block;color:var(--ink);font-size:14px;margin-bottom:4px}}
footer{{border-top:1px solid rgba(255,255,255,.08);padding:30px 22px;text-align:center;color:var(--mut);font-size:11.5px;letter-spacing:.04em}}
footer a{{color:rgba(245,245,240,.72);margin:0 9px}}
footer a:hover{{color:var(--gold)}}
</style></head>
<body>
<nav><a class="bm" href="/store">MU <span class="o">MAKE</span> 無人店</a><div class="nl"><a href="#shelf">棚</a><a href="/make">作る</a><a href="/shop">SHOP</a></div></nav>

<header class="hero">
  <div class="kick">店員ゼロ · 24時間 · 在庫ゼロ · 受注生産</div>
  <h1>MU <span class="o">MAKE</span> 無人店</h1>
  <p class="sub">店員はいません。<b>AI が描いて、刷って、あなたに送る。</b>話しかけたら、それがTシャツになる。在庫はゼロ。だから<b>なんでも言ってみ。</b></p>
  <p class="en">The T-shirt shop with no staff. AI draws it, prints it, ships it — 24/7, zero inventory, made only when you order.</p>
  <div class="live"><span><span class="dot"></span>営業中</span><span>棚に <b>{live}</b> 種</span><span><b>{brands}</b> ブランド</span><span><b>{sold}</b> 枚 旅立った</span></div>
  <div class="cta"><a class="btn p" href="/make">なんでも言ってみ →</a><a class="btn s" href="#shelf">棚を見る</a></div>
</header>

<div class="ticker"><div class="track">{ticker}</div></div>

<section class="sec">
  <h2>How it works — 人は触れない</h2>
  <div class="steps">
    <div class="step"><div class="n">01 / SAY</div><h3>話す</h3><p>「夜の海の静けさ」みたいに、ひとことで伝えるだけ。ログインも要りません。</p></div>
    <div class="step"><div class="n">02 / DRAW</div><h3>AI が描く</h3><p>Gemini がデザインを生成し、Printful の実物モックまで自動で作ります（¥12/枚原価・在庫リスクゼロ）。</p></div>
    <div class="step"><div class="n">03 / SHIP</div><h3>自動で届く</h3><p>注文が入ると自動で印刷・発送。7〜14日で手元へ。途中に人は一切いません。</p></div>
  </div>
</section>

<section class="sec" id="shelf">
  <h2>いま棚に並んでいるもの — Live shelf</h2>
  <div class="grid">{cards}</div>
  <div class="shelf-more"><a class="btn s" href="/shop">棚をぜんぶ見る（{live} 種）→</a></div>
</section>

<section class="sec">
  <h2>なぜ無人なのか — Why unmanned</h2>
  <div class="why">
    <div><b>在庫ゼロ</b>受注生産。売れ残りも廃棄も出ません。</div>
    <div><b>人を介さない</b>生成・承認・発送まで AI council が回す。24時間止まらない。</div>
    <div><b>予算は上限つき</b>月 ¥1,000,000 をコードで強制。暴走しません。</div>
    <div><b>コードは公開</b>仕組みは <a style="color:var(--gold)" href="/source">/source</a> で全部見られます。</div>
  </div>
</section>

<footer>
  ━◯━ MU · on-demand · zero inventory · 株式会社イネブラ / Enabler Inc.<br>
  <a href="/make">作る</a> · <a href="/shop">SHOP</a> · <a href="/about/honest">正直なところ</a> · <a href="https://yukihamada.jp/community">🔥 ともしび</a> · <a href="/tokushoho">特商法</a>
</footer>
</body></html>"##,
        live = live, brands = brands, sold = sold, cards = cards, ticker = ticker
    ))
}

/// Cost guard for the unauthenticated /make endpoint: max public creations/hour.
const MAKE_HOURLY_CAP: i64 = 40;

/// 「作る動線」: 全ページに貼れる自己完結CTA（インラインstyle）。`src`はfunnel計測タグ。
/// 作る数の最大化が目的 — どのページからでも1タップで /make へ。
pub fn make_cta_banner(src: &str) -> String {
    format!(
        r##"<a href="/make?ref={src}" data-funnel="cta_click" data-funnel-cta="make_{src}" style="display:flex;align-items:center;gap:12px;justify-content:center;flex-wrap:wrap;margin:0 auto 20px;max-width:1200px;background:linear-gradient(90deg,rgba(255,215,0,.14),rgba(255,215,0,.05));border:1px solid rgba(255,215,0,.4);border-radius:14px;padding:14px 18px;text-decoration:none;color:#f5f5f0;font-size:15px;font-weight:700;letter-spacing:.01em">
<span style="font-size:20px">✦</span><span>ひとこと言うだけで、自分のTシャツをAIが作る</span>
<span style="background:#ffd700;color:#0a0a0a;border-radius:99px;padding:7px 16px;font-size:13px;font-weight:800;white-space:nowrap">作ってみる →</span></a>"##,
        src = src,
    )
}

/// GET /api/make/recent — last live 'minna' creations for the /make social
/// proof strip. Read-only, tiny payload, 60s CDN cache.
pub async fn make_recent(State(db): State<Db>) -> Response {
    let rows: Vec<(String, String, String, i64)> = {
        let conn = db.lock().unwrap();
        conn.prepare(
            "SELECT sku, label, COALESCE(CASE WHEN mockup_url_external LIKE 'https://printful-upload.s3%'
                       OR mockup_url_external LIKE '%/tmp/%' THEN NULL ELSE mockup_url_external END,
                     design_file, ''), retail_price_jpy
             FROM catalog_products
             WHERE brand='minna' AND is_active=1 AND status='live'
             ORDER BY created_at DESC LIMIT 8",
        )
        .ok()
        .and_then(|mut s| {
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .ok()
                .map(|it| it.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
    };
    let items: Vec<serde_json::Value> = rows.into_iter()
        .filter(|(_, _, img, _)| !img.is_empty())
        .map(|(sku, label, img, price)| serde_json::json!({
            "sku": sku, "label": label, "img": img, "price": price,
        }))
        .collect();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("Cache-Control", axum::http::HeaderValue::from_static("public, max-age=60"));
    (headers, axum::Json(serde_json::json!({"items": items}))).into_response()
}

#[derive(serde::Deserialize)]
pub struct MakePeekQuery {
    pub sku: String,
}

/// GET /api/make/peek?sku= — /make 直後の結果カードが着用イメージ
/// (on-body mockup, バックグラウンド生成) の完成をポーリングする軽量API。
/// 公開情報のみ・minna(=/make産)限定。mockup が design と別URLになった時だけ
/// 「着用イメージ完成」として返す（心理的所有感: 着た姿を見せると評価が上がる）。
pub async fn make_peek(State(db): State<Db>, Query(q): Query<MakePeekQuery>) -> Response {
    let row: Option<(String, Option<String>, String)> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            &format!(
                "SELECT COALESCE(design_file,''), {ext}, status
                 FROM catalog_products WHERE sku=? AND brand='minna'",
                ext = MOCKUP_EXT_LIVE
            ),
            rusqlite::params![&q.sku],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok()
    };
    let Some((design, mock, status)) = row else {
        return (StatusCode::NOT_FOUND, axum::Json(serde_json::json!({"ok": false}))).into_response();
    };
    let mockup = mock.filter(|m| !m.is_empty() && *m != design);
    // max-age=5: 全作成者が6秒間隔でポーリングする → CDN/ブラウザに逃がして
    // グローバルMutexのSQLiteをポーリング地獄から守る（鮮度は5秒で十分）。
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("Cache-Control", axum::http::HeaderValue::from_static("public, max-age=5"));
    (headers, axum::Json(serde_json::json!({"ok": true, "status": status, "mockup": mockup}))).into_response()
}

#[derive(serde::Deserialize)]
pub struct MakeNotifyQuery {
    pub sku: String,
    pub email: String,
}

/// POST /api/make/notify?sku=&email= — /make 直後の「メールでリンクを受け取る」。
/// 作者は匿名なので、ここが唯一の連絡接点になる:
///   ① live: その場でリンク保存メール（離脱後のリマーケ経路）
///   ② review: 公開時に ma_review_approve から通知メール
/// 乱用対策: /make産(minna+public_make)限定・1SKUにつき先勝ち1回（再送なし・
/// メール爆撃防止）・全体30通/時の fail-closed キャップ。
pub async fn make_notify(State(db): State<Db>, Query(q): Query<MakeNotifyQuery>) -> Response {
    let email = q.email.trim().to_lowercase();
    let ok_email = email.len() >= 6
        && email.len() <= 120
        && email.contains('@')
        && email.rsplit('@').next().map(|d| d.contains('.')).unwrap_or(false)
        && !email.chars().any(|c| c.is_whitespace());
    if !ok_email {
        return (StatusCode::BAD_REQUEST, axum::Json(serde_json::json!({"ok":false,"error":"メールアドレスを確認してください"}))).into_response();
    }
    let row: Option<(String, i64, String, Option<String>)> = {
        let conn = db.lock().unwrap();
        // 全体時間キャップ。クエリ失敗時は i64::MAX → 拒否側に倒す (fail-closed)。
        let hour_ago = crate::chrono_now().parse::<i64>().unwrap_or(0) - 3600;
        let sent_1h: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM funnel_events WHERE event='make_notify'
                   AND CAST(COALESCE(created_at,'0') AS INTEGER) > ?",
                rusqlite::params![hour_ago],
                |r| r.get(0),
            )
            .unwrap_or(i64::MAX);
        if sent_1h >= 30 {
            return (StatusCode::TOO_MANY_REQUESTS, axum::Json(serde_json::json!({"ok":false,"error":"混み合っています。少し時間をおいてください"}))).into_response();
        }
        conn.query_row(
            "SELECT label, retail_price_jpy, status, meta_json FROM catalog_products
             WHERE sku=? AND brand='minna' AND legacy_source='public_make'",
            rusqlite::params![&q.sku],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok()
    };
    let Some((label, price, status, meta_json)) = row else {
        return (StatusCode::NOT_FOUND, axum::Json(serde_json::json!({"ok":false,"error":"not found"}))).into_response();
    };
    let mut meta: serde_json::Value = meta_json
        .as_deref()
        .and_then(|m| serde_json::from_str(m).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !meta.is_object() {
        meta = serde_json::json!({});
    }
    if meta.get("notify_email").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false) {
        // 先勝ち・冪等。再送しない（連打/横取りでのメール爆撃防止）。
        return axum::Json(serde_json::json!({"ok":true,"already":true,"status":status})).into_response();
    }
    meta.as_object_mut().unwrap().insert("notify_email".into(), serde_json::Value::from(email.clone()));
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "UPDATE catalog_products SET meta_json=? WHERE sku=?",
            rusqlite::params![meta.to_string(), &q.sku],
        );
    }
    crate::funnel_track_server(&db, "make_notify", "/make", None, serde_json::json!({"sku": q.sku})).await;
    if status == "live" {
        tokio::spawn(send_make_link_email(email, q.sku.clone(), label, price, false));
    }
    axum::Json(serde_json::json!({"ok":true,"status":status})).into_response()
}

/// /make 作者向けメール（Resend）。approved=false: リンク保存（live直後）、
/// approved=true: review→live 公開通知（ma_review_approve から呼ばれる）。
pub async fn send_make_link_email(to: String, sku: String, label: String, price_jpy: i64, approved: bool) {
    let resend_key = std::env::var("RESEND_API_KEY").unwrap_or_default();
    if resend_key.is_empty() {
        tracing::warn!("[make/notify] RESEND_API_KEY unset — link mail to {} not sent (sku {})", to, sku);
        return;
    }
    let url = format!("https://wearmu.com/shop/{}", sku);
    let (subject, lead) = if approved {
        (
            format!("🌱 公開されました — {}", label),
            "確認が終わり、あなたの一着が棚に並びました。世界に1枚、今から購入できます。",
        )
    } else {
        (
            format!("🌱 あなたの一着のリンク — {}", label),
            "あなたの言葉から生まれた、世界に1枚。このリンクからいつでも戻れます。",
        )
    };
    let html = format!(
        r#"<div style="background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,'Helvetica Neue',Arial,sans-serif;padding:32px 0;margin:0">
<div style="max-width:560px;margin:0 auto;padding:0 32px">
<div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:24px">━◯━ MU</div>
<div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#ffd700;opacity:0.85;margin-bottom:8px">DESIGNED BY YOU</div>
<h2 style="font-size:19px;font-weight:600;line-height:1.5;margin:0 0 14px">{label}</h2>
<p style="font-size:13px;line-height:1.9;opacity:0.78;margin:0 0 22px">{lead}</p>
<div style="text-align:center;margin:24px 0">
<a href="{url}" style="display:inline-block;background:#ffd700;color:#0a0a0a;text-decoration:none;font-weight:700;font-size:15px;padding:14px 28px;border-radius:99px">この一着を見る ¥{price} →</a></div>
<p style="font-size:11px;line-height:1.85;opacity:0.55;margin:24px 0 0;border-top:1px solid #222;padding-top:18px">
同じデザインは二度と生成されません。1枚から受注生産。<br>
お問い合わせ: <a href="mailto:info@enablerdao.com" style="color:#ffd700">info@enablerdao.com</a>
</p>
</div></div>"#,
        label = html_text(&label),
        lead = lead,
        url = url,
        price = price_jpy,
    );
    let payload = serde_json::json!({
        "from": "MU MAKE <noreply@wearmu.com>",
        "to": [to],
        "subject": subject,
        "html": html,
    });
    let _ = reqwest::Client::new()
        .post("https://api.resend.com/emails")
        .bearer_auth(&resend_key)
        .json(&payload)
        .send()
        .await;
}

// ─── /make メール認証ゲート ──────────────────────────────────────────────
// 生成は誰でも走るが、結果(着用モックアップ+PDP)を「見る」前にメール認証を
// 課す（生成後リビールゲート）。作った労力がかかった分メアド提供率が高い
// (IKEA効果)。コードは collab_users.code を再利用＝新テーブルなし。verify は
// collab セッション/API キーを発行しない軽量ゲート（make 用途に限定）。

#[derive(serde::Deserialize)]
pub struct MakeVerifySendBody { pub sku: String, pub email: String }

fn make_email_ok(email: &str) -> bool {
    email.len() >= 6
        && email.len() <= 120
        && email.contains('@')
        && email.rsplit('@').next().map(|d| d.contains('.')).unwrap_or(false)
        && !email.chars().any(|c| c.is_whitespace())
}

/// POST /api/make/verify/send {sku,email} — 結果を見るための6桁コードをメール送信。
pub async fn make_verify_send(State(db): State<Db>, axum::Json(q): axum::Json<MakeVerifySendBody>) -> Response {
    let email = q.email.trim().to_lowercase();
    if !make_email_ok(&email) {
        return (StatusCode::BAD_REQUEST, axum::Json(serde_json::json!({"ok":false,"error":"メールアドレスを確認してください"}))).into_response();
    }
    use rand::Rng;
    let code: String = format!("{:06}", rand::thread_rng().gen_range(0..1_000_000));
    let now_s: i64 = crate::chrono_now().parse().unwrap_or(0);
    let expires = now_s + 900; // 15分
    {
        let conn = db.lock().unwrap();
        // この sku が実在する /make 作品か確認（任意 sku でのコード発行を防ぐ）
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM catalog_products WHERE sku=? AND brand='minna' AND legacy_source='public_make'",
                rusqlite::params![&q.sku], |_| Ok(()),
            )
            .is_ok();
        if !exists {
            return (StatusCode::NOT_FOUND, axum::Json(serde_json::json!({"ok":false,"error":"not found"}))).into_response();
        }
        // 全体メール送信キャップ（fail-closed）: 直近1時間で 60 通まで。
        let hour_ago = now_s - 3600;
        let sent_1h: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM funnel_events WHERE event='make_verify_send'
                   AND CAST(COALESCE(created_at,'0') AS INTEGER) > ?",
                rusqlite::params![hour_ago], |r| r.get(0),
            )
            .unwrap_or(i64::MAX);
        if sent_1h >= 60 {
            return (StatusCode::TOO_MANY_REQUESTS, axum::Json(serde_json::json!({"ok":false,"error":"混み合っています。少し時間をおいてください"}))).into_response();
        }
        let _ = conn.execute(
            "INSERT INTO collab_users (email, code, code_expires_at, created_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(email) DO UPDATE SET code=excluded.code, code_expires_at=excluded.code_expires_at",
            rusqlite::params![email, code, expires, now_s],
        );
    }
    crate::funnel_track_server(&db, "make_verify_send", "/make", None, serde_json::json!({"sku": q.sku})).await;
    if std::env::var("RESEND_API_KEY").map(|k| k.is_empty()).unwrap_or(true) {
        return (StatusCode::SERVICE_UNAVAILABLE, axum::Json(serde_json::json!({"ok":false,"error":"メール送信が未設定です"}))).into_response();
    }
    send_make_code_email(email, code).await;
    axum::Json(serde_json::json!({"ok":true,"message":"確認コードを送りました（15分有効）"})).into_response()
}

#[derive(serde::Deserialize)]
pub struct MakeVerifyCheckBody { pub sku: String, pub email: String, pub code: String }

/// POST /api/make/verify/check {sku,email,code} — コード照合 → 結果を開放。
/// 成功で mu_make_ok クッキーを付与（以後この端末は再認証不要）。
pub async fn make_verify_check(State(db): State<Db>, axum::Json(q): axum::Json<MakeVerifyCheckBody>) -> Response {
    let email = q.email.trim().to_lowercase();
    let code = q.code.trim().to_string();
    let now_s: i64 = crate::chrono_now().parse().unwrap_or(0);
    let row: Option<(String, i64)> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(code,''), COALESCE(code_expires_at,0) FROM collab_users WHERE email=?",
            rusqlite::params![email], |r| Ok((r.get(0)?, r.get(1)?)),
        ).ok()
    };
    let (db_code, expires) = match row {
        Some(r) => r,
        None => return (StatusCode::NOT_FOUND, axum::Json(serde_json::json!({"ok":false,"error":"先にコードを送ってください"}))).into_response(),
    };
    if db_code.is_empty() || db_code != code {
        return (StatusCode::UNAUTHORIZED, axum::Json(serde_json::json!({"ok":false,"error":"確認コードが一致しません"}))).into_response();
    }
    if expires < now_s {
        return (StatusCode::UNAUTHORIZED, axum::Json(serde_json::json!({"ok":false,"error":"コードの有効期限が切れました。もう一度お試しください"}))).into_response();
    }
    {
        let conn = db.lock().unwrap();
        // コードを使い切る（再利用防止）
        let _ = conn.execute("UPDATE collab_users SET code=NULL, code_expires_at=NULL WHERE email=?", rusqlite::params![email]);
        // 作者メールを作品に刻む（先勝ち・冪等）。売れた時の連絡先にもなる。
        if let Ok(meta_json) = conn.query_row(
            "SELECT COALESCE(meta_json,'') FROM catalog_products WHERE sku=? AND legacy_source='public_make'",
            rusqlite::params![&q.sku], |r| r.get::<_, String>(0),
        ) {
            let mut meta: serde_json::Value = serde_json::from_str(&meta_json).unwrap_or_else(|_| serde_json::json!({}));
            if !meta.is_object() { meta = serde_json::json!({}); }
            let has = meta.get("maker_email").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false);
            if !has {
                meta.as_object_mut().unwrap().insert("maker_email".into(), serde_json::Value::from(email.clone()));
                let _ = conn.execute("UPDATE catalog_products SET meta_json=? WHERE sku=?", rusqlite::params![meta.to_string(), &q.sku]);
            }
        }
    }
    crate::funnel_track_server(&db, "make_verified", "/make", None, serde_json::json!({"sku": q.sku})).await;
    let mut resp = axum::Json(serde_json::json!({"ok":true})).into_response();
    resp.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        axum::http::HeaderValue::from_static("mu_make_ok=1; Path=/; Max-Age=2592000; SameSite=Lax"),
    );
    resp
}

/// /make 認証ゲートの6桁コードメール（Resend）。リンクではなくコードのみ。
pub async fn send_make_code_email(to: String, code: String) {
    let resend_key = std::env::var("RESEND_API_KEY").unwrap_or_default();
    if resend_key.is_empty() { return; }
    let html = format!(
        r#"<div style="background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,'Helvetica Neue',Arial,sans-serif;padding:32px 0;margin:0">
<div style="max-width:520px;margin:0 auto;padding:0 32px">
<div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:22px">━◯━ MU</div>
<div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#ffd700;opacity:0.85;margin-bottom:8px">DESIGNED BY YOU</div>
<h2 style="font-size:19px;font-weight:600;line-height:1.5;margin:0 0 12px">あなたの一着を見るための確認コード</h2>
<p style="font-size:13px;line-height:1.9;opacity:0.78;margin:0 0 18px">/make の画面に下のコードを入力すると、生まれたばかりの世界に1枚が現れます。15分間有効です。</p>
<div style="font-size:38px;letter-spacing:0.32em;font-weight:700;color:#ffd700;background:#111;padding:22px;text-align:center;border-radius:8px;font-family:'SF Mono',monospace;margin:8px 0 18px">{code}</div>
<p style="font-size:11px;line-height:1.85;opacity:0.5;margin:22px 0 0;border-top:1px solid #222;padding-top:18px">
心当たりがない場合はこのメールを無視してください。<br>
MU · wearmu.com · 株式会社イネブラ</p>
</div></div>"#,
        code = code,
    );
    let payload = serde_json::json!({
        "from": "━◯━ MU Make <noreply@wearmu.com>",
        "to": [to],
        "subject": "MU — あなたの一着を見る確認コード",
        "html": html,
    });
    let _ = reqwest::Client::new()
        .post("https://api.resend.com/emails")
        .bearer_auth(&resend_key)
        .json(&payload)
        .send()
        .await;
}

/// MUON コレクター達成メール。Tシャツを規定枚数集めるごとに ¥reward の MU クレジット獲得を通知。
pub async fn send_muon_reward_email(to: String, tee_count: i64, reward_jpy: i64) {
    let resend_key = std::env::var("RESEND_API_KEY").unwrap_or_default();
    if resend_key.is_empty() || to.is_empty() { return; }
    let html = format!(
        r#"<div style="background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,'Helvetica Neue',Arial,sans-serif;padding:32px 0;margin:0">
<div style="max-width:520px;margin:0 auto;padding:0 32px">
<div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:22px">━◯━ MU</div>
<div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#ffd700;opacity:0.85;margin-bottom:8px">MUON — COLLECTOR REWARD</div>
<h2 style="font-size:20px;font-weight:700;line-height:1.5;margin:0 0 12px">🎉 Tシャツ {n} 枚コンプリート</h2>
<p style="font-size:13px;line-height:1.9;opacity:0.82;margin:0 0 18px">集めていただきありがとうございます。<br><b>MUON ストアクレジット ¥{r}</b> を付与しました。次のお買い物の決済画面で自動的に使えます（期限なし）。</p>
<div style="font-size:34px;letter-spacing:0.04em;font-weight:700;color:#ffd700;background:#111;padding:22px;text-align:center;border-radius:8px;font-family:'SF Mono',monospace;margin:8px 0 18px">MUON ¥{r}</div>
<p style="font-size:12px;line-height:1.85;opacity:0.7;margin:0">あと3枚集めると、また MUON。<a href="https://wearmu.com/shop" style="color:#ffd700">次の一枚を見る →</a></p>
<p style="font-size:11px;line-height:1.85;opacity:0.5;margin:22px 0 0;border-top:1px solid #222;padding-top:18px">MU · wearmu.com · 株式会社イネブラ</p>
</div></div>"#,
        n = tee_count, r = format_jpy(reward_jpy),
    );
    let payload = serde_json::json!({
        "from": "━◯━ MU <noreply@wearmu.com>",
        "to": [to],
        "subject": format!("🎉 MUON ¥{} 獲得 — Tシャツ{}枚コンプリート", format_jpy(reward_jpy), tee_count),
        "html": html,
    });
    let _ = reqwest::Client::new()
        .post("https://api.resend.com/emails")
        .bearer_auth(&resend_key)
        .json(&payload)
        .send()
        .await;
}

/// GET /api/make/ab — A/B/C の現況（各案のユニーク訪問者作成数・作成総数・勝者）。
pub async fn make_ab_status(State(db): State<Db>) -> Response {
    let conn = db.lock().unwrap();
    let winner = cv_get(&conn, "make_winner");
    let rows: Vec<(String, i64, i64)> = conn
        .prepare(
            "SELECT json_extract(meta_json,'$.make_variant') v,
                    COUNT(DISTINCT json_extract(meta_json,'$.make_visitor')) uu,
                    COUNT(*) total
             FROM catalog_products
             WHERE legacy_source='public_make'
               AND json_extract(meta_json,'$.make_variant') IS NOT NULL
             GROUP BY v ORDER BY uu DESC",
        )
        .ok()
        .and_then(|mut s| {
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .ok()
                .map(|it| it.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default();
    let variants: Vec<serde_json::Value> = rows.into_iter()
        .map(|(v, uu, total)| serde_json::json!({"variant": v, "unique_visitors": uu, "creations": total}))
        .collect();
    axum::Json(serde_json::json!({
        "ok": true,
        "winner": winner,
        "threshold": MAKE_AB_WIN_THRESHOLD,
        "variants": variants,
    })).into_response()
}

/// GET /make — public page: type a sentence, MU makes the product.
/// A/B/C: 勝者確定済みなら全員その案。未確定は ?v= 指定、無ければ
/// クライアントJSが visitor_id から決定的に3分割（同じ人は常に同じ案）。
pub async fn make_page(State(db): State<Db>, Query(q): Query<MakePageQuery>) -> Html<String> {
    // 勝者が決まっていれば全員に勝者を固定表示（?v は無視）。
    let winner = { let conn = db.lock().unwrap(); cv_get(&conn, "make_winner") };
    let locked = make_variant_norm(winner.as_deref());
    let forced = locked.or_else(|| make_variant_norm(q.v.as_deref()));
    // forced=Some → サーバが variant を焼く（JS割当オフ）。None → JSが visitor で決める。
    let server_variant = forced.unwrap_or("");
    let lock_js = if locked.is_some() { "true" } else { "false" };
    Html(MAKE_HTML
        .replace("__SERVER_VARIANT__", server_variant)
        .replace("__WINNER_LOCKED__", lock_js))
}

const MAKE_HTML: &str = r##"<!doctype html><html lang="ja"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<title>AIでオリジナルTシャツ作成 — 言うだけ10秒・1枚から・在庫ゼロ | MU MAKE · wearmu.com</title>
<meta name="description" content="ひとこと言うだけでAIがオリジナルTシャツ・パーカーをデザイン。10〜20秒で完成、その場で1枚から購入OK（¥4,900〜）。ログイン不要・在庫ゼロ。作った一着は店に並び、売れたら作り手に報酬¥600〜/枚。">
<link rel="canonical" href="https://wearmu.com/make">
<meta property="og:type" content="website">
<meta property="og:url" content="https://wearmu.com/make">
<meta property="og:title" content="言うだけで、Tシャツができる。— MU MAKE">
<meta property="og:description" content="AIが10秒でデザイン→1枚から買える（¥4,900〜）。あなたの一着が店に並び、売れたら報酬¥600〜/枚。ログイン不要。">
<meta property="og:image" content="https://wearmu.com/static/og.jpg">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="言うだけで、Tシャツができる。— MU MAKE">
<meta name="twitter:image" content="https://wearmu.com/static/og.jpg">
<script type="application/ld+json">
{"@context":"https://schema.org","@graph":[
 {"@type":"HowTo","name":"AIでオリジナルTシャツを作る方法（MU MAKE）",
  "step":[
   {"@type":"HowToStep","position":1,"name":"言う","text":"作りたいものを一言で入力（例：富士山をミニマルな一本線で描いた黒Tシャツ）。"},
   {"@type":"HowToStep","position":2,"name":"AIが描く","text":"10〜20秒でAIがデザインを生成し、商品ページができる。"},
   {"@type":"HowToStep","position":3,"name":"買える・並ぶ","text":"その場で1枚から購入できる（Tシャツ¥4,900〜）。作った一着はみんなの棚に並び、売れるたび作り手に¥600〜/枚の報酬。"}]},
 {"@type":"FAQPage","mainEntity":[
  {"@type":"Question","name":"本当にログイン不要ですか？","acceptedAnswer":{"@type":"Answer","text":"はい。アカウント登録なしで、その場で作成・購入できます。"}},
  {"@type":"Question","name":"価格はいくらですか？","acceptedAnswer":{"@type":"Answer","text":"Tシャツ¥4,900〜、スウェット¥7,800〜、パーカー¥8,800〜。1枚から受注生産です。"}},
  {"@type":"Question","name":"作ったデザインはすぐ公開されますか？","acceptedAnswer":{"@type":"Answer","text":"ほとんどは即公開・即購入できます。商標・実在人物など権利リスクがあるものだけ人が確認してから公開します。"}},
  {"@type":"Question","name":"売れたらどうなりますか？","acceptedAnswer":{"@type":"Answer","text":"あなたの一着が売れるたび、作り手として¥600〜/枚の報酬を受け取れます。"}}]}]}
</script>
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{background:#0a0a0a;color:#f5f5f0;font-family:'Helvetica Neue','Hiragino Sans',Arial,sans-serif;line-height:1.7;min-height:100dvh}
nav{padding:16px 24px;border-bottom:1px solid rgba(255,255,255,.08);display:flex;justify-content:space-between;align-items:center}
nav a{color:#f5f5f0;text-decoration:none;font-size:11px;letter-spacing:.3em;text-transform:uppercase;opacity:.85}
nav .brand{font-weight:900;letter-spacing:.4em}
.wrap{max-width:680px;margin:0 auto;padding:48px 22px 100px}
h1{font-size:30px;font-weight:800;letter-spacing:-.01em;margin-bottom:8px}
.sub{color:rgba(245,245,240,.6);font-size:14px;margin-bottom:28px}
textarea{width:100%;background:#141414;border:1px solid rgba(255,255,255,.14);color:#f5f5f0;border-radius:10px;padding:14px 16px;font-size:16px;font-family:inherit;min-height:96px;resize:vertical}
textarea:focus{outline:none;border-color:#ffd700}
.row{display:flex;gap:10px;margin-top:12px;flex-wrap:wrap;align-items:center}
select{background:#141414;border:1px solid rgba(255,255,255,.14);color:#f5f5f0;border-radius:10px;padding:12px 14px;font-size:15px}
button{flex:1;min-width:160px;background:#ffd700;color:#0a0a0a;border:0;border-radius:10px;padding:14px 18px;font-size:16px;font-weight:800;cursor:pointer;letter-spacing:.04em}
button:disabled{opacity:.5;cursor:default}
.ex{margin-top:14px;font-size:12px;color:rgba(245,245,240,.45)}
.ex b{color:rgba(255,215,0,.8);cursor:pointer;font-weight:600}
.quick{margin:0 0 16px}
.quick .qlead{font-size:13px;color:rgba(245,245,240,.6);margin-bottom:10px}
.quick .qgrid{display:grid;grid-template-columns:repeat(auto-fill,minmax(104px,1fr));gap:8px}
.quick .q{flex:none;min-width:0;background:#161616;border:1px solid rgba(255,215,0,.3);color:#f5f5f0;border-radius:12px;padding:16px 10px;font-size:14px;font-weight:700;cursor:pointer;letter-spacing:.02em}
.quick .q:hover{background:rgba(255,215,0,.12);border-color:#ffd700}
#out{margin-top:28px}
.card{background:#141414;border:1px solid rgba(255,255,255,.12);border-radius:14px;padding:18px;display:flex;gap:16px;align-items:center;flex-wrap:wrap}
.card img{width:140px;height:140px;object-fit:contain;background:#fff;border-radius:10px;flex:0 0 auto}
.card .meta{flex:1;min-width:180px}
.card .nm{font-size:18px;font-weight:700}
.card .pr{color:#ffd700;font-size:20px;font-weight:800;margin:4px 0}
.card a.buy{display:block;text-align:center;margin-top:12px;background:#ffd700;color:#0a0a0a;text-decoration:none;font-weight:800;padding:13px 16px;border-radius:10px;font-size:15.5px;letter-spacing:.02em}
.card a.buy small{display:block;font-weight:600;font-size:10.5px;opacity:.7;margin-top:2px;letter-spacing:0}
.card button.share{margin-top:10px;width:100%;background:transparent;border:1px solid rgba(255,215,0,.4);color:rgba(255,215,0,.9);font-weight:600;padding:9px 14px;border-radius:8px;font-size:12.5px;cursor:pointer;font-family:inherit}
.card button.share:hover{background:rgba(255,215,0,.12)}
.card .spread{font-size:11.5px;color:rgba(245,245,240,.5);margin-top:8px}
.note{font-size:12px;color:rgba(245,245,240,.5);margin-top:8px}
/* リビール演出（ピークエンド: 出来上がりの瞬間をピークに）＋ 所有感UI */
.own{font-size:14.5px;color:rgba(245,245,240,.88);margin:26px 0 10px;line-height:1.65}
.own b{color:#ffd700}
.own .pq{display:block;color:rgba(245,245,240,.5);font-size:12.5px;margin-top:2px}
.card.reveal{animation:pop .65s cubic-bezier(.2,.8,.3,1.12) both;box-shadow:0 0 0 1px rgba(255,215,0,.32),0 0 44px rgba(255,215,0,.09)}
@keyframes pop{from{opacity:0;transform:scale(.93) translateY(10px)}to{opacity:1;transform:scale(1) translateY(0)}}
.card img{transition:opacity .45s}
.card .by{font-size:10.5px;color:rgba(255,215,0,.7);letter-spacing:.14em;margin-top:2px;font-weight:700}
.card .one{font-size:12px;color:rgba(245,245,240,.62);margin-top:8px;line-height:1.65}
.card .one b{color:#f5f5f0}
.card .fitnote{font-size:11.5px;color:rgba(255,215,0,.78);margin-top:6px;min-height:16px}
.savebox{margin-top:12px;background:#101010;border:1px solid rgba(255,255,255,.1);border-radius:10px;padding:12px}
.savebox .savelead{font-size:12px;color:rgba(245,245,240,.65);margin-bottom:8px;line-height:1.6}
.saverow{display:flex;gap:8px}
.saverow input{flex:1;min-width:0;background:#141414;border:1px solid rgba(255,255,255,.14);color:#f5f5f0;border-radius:8px;padding:10px 12px;font-size:14px;font-family:inherit}
.saverow input:focus{outline:none;border-color:#ffd700}
.saverow button{flex:0 0 auto;min-width:0;background:transparent;border:1px solid rgba(255,215,0,.5);color:#ffd700;font-weight:700;padding:10px 14px;border-radius:8px;font-size:13px}
.saverow button:disabled{opacity:.5}
.savemsg{font-size:11.5px;color:#9fdf9f;margin-top:6px;min-height:14px}
.card.gate{padding:0;overflow:hidden}
.gatewrap{position:relative;background:#0d0d0d}
.gateimg{display:block;width:100%;aspect-ratio:1;object-fit:cover;filter:blur(20px) brightness(.62);transform:scale(1.08)}
.gatelock{position:absolute;inset:0;display:flex;align-items:center;justify-content:center;font-size:40px;text-shadow:0 2px 18px rgba(0,0,0,.7)}
.gatebody{padding:18px 16px 20px}
.gateh{font-size:15px;line-height:1.6;color:#f5f5f0}
.gateh b{color:#ffd700}
.gatesub{font-size:12px;color:rgba(245,245,240,.6);line-height:1.7;margin:6px 0 14px}
.gback{margin-top:10px;background:none;border:none;color:rgba(245,245,240,.5);font-size:12px;text-decoration:underline;cursor:pointer;padding:4px 0}
.err{color:#ff8a7a;font-size:14px}
.spin{display:inline-block;width:16px;height:16px;border:2px solid rgba(0,0,0,.3);border-top-color:#0a0a0a;border-radius:50%;animation:s .7s linear infinite;vertical-align:-3px;margin-right:8px}
@keyframes s{to{transform:rotate(360deg)}}
/* 生成シアター — 待ち時間 10〜20 秒を「いま作られている」実感に変える */
.gen{background:#121212;border:1px solid rgba(255,215,0,.28);border-radius:14px;padding:24px 20px;position:relative;overflow:hidden}
.gen::after{content:'';position:absolute;inset:0;background:linear-gradient(110deg,transparent 30%,rgba(255,215,0,.05) 50%,transparent 70%);animation:sheen 2.8s linear infinite;pointer-events:none}
@keyframes sheen{from{transform:translateX(-100%)}to{transform:translateX(100%)}}
.gen .enso{width:36px;height:36px;border:3px solid rgba(255,215,0,.9);border-right-color:transparent;border-radius:50%;animation:enso 1.5s cubic-bezier(.55,.15,.45,.85) infinite;margin-bottom:14px}
@keyframes enso{to{transform:rotate(360deg)}}
.gen .gq{font-size:13px;color:rgba(245,245,240,.72);margin-bottom:8px}
.gen .gq b{color:#ffd700;font-weight:700}
.gen .gmsg{font-size:16px;font-weight:700;min-height:26px;transition:opacity .35s}
.gen .gbar{height:4px;background:rgba(255,255,255,.08);border-radius:99px;margin-top:14px;overflow:hidden}
.gen .gfill{height:100%;width:2%;background:linear-gradient(90deg,#ffd700,#ffb700);border-radius:99px;transition:width .6s ease}
.gen .gnote{font-size:11px;color:rgba(245,245,240,.42);margin-top:10px}
.steps{display:flex;gap:10px;margin:0 0 26px;flex-wrap:wrap}
.step{flex:1;min-width:150px;background:#121212;border:1px solid rgba(255,255,255,.09);border-radius:12px;padding:14px 16px}
.step .n{font-size:11px;color:#ffd700;font-weight:800;letter-spacing:.18em}
.step .t{font-size:14.5px;font-weight:700;margin-top:2px}
.step .d{font-size:12px;color:rgba(245,245,240,.55);margin-top:3px;line-height:1.55}
.price-hint{font-size:12.5px;color:rgba(245,245,240,.6);margin-top:10px}
.price-hint b{color:#f5f5f0}
.recent{margin-top:44px}
.recent h2{font-size:13px;letter-spacing:.25em;text-transform:uppercase;color:rgba(245,245,240,.55);font-weight:600;margin-bottom:14px}
.rgrid{display:grid;grid-template-columns:repeat(auto-fill,minmax(120px,1fr));gap:10px}
.rgrid a{display:block;text-decoration:none;color:inherit;background:#121212;border:1px solid rgba(255,255,255,.08);border-radius:10px;overflow:hidden}
.rgrid img{width:100%;aspect-ratio:1/1;object-fit:cover;background:#fff;display:block}
.rgrid .rl{font-size:10.5px;padding:7px 9px 2px;line-height:1.4;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:rgba(245,245,240,.85)}
.rgrid .rp{font-size:11px;color:#ffd700;font-weight:700;padding:0 9px 8px}
</style></head><body>
<nav><a class="brand" href="/make">MU MAKE</a><div><a href="/shop">SHOP</a></div></nav>
<div class="wrap">
  <h1 id="mkH1">言うだけで、Tシャツができる。</h1>
  <div class="sub" id="mkSub">ひとこと言えば AI がデザイン → <b>その場で 1 枚から買える</b>。ログインも在庫もゼロ。あなたの一着はみんなの棚にも並び、<b style="color:#ffd700">売れたら作り手に報酬（¥600〜/枚）</b>。</div>
  <div class="steps">
    <div class="step"><div class="n">STEP 1</div><div class="t">言う</div><div class="d">作りたいものを一言。日本語でOK。</div></div>
    <div class="step"><div class="n">STEP 2</div><div class="t">AIが描く</div><div class="d">10〜20秒でデザインと商品ページが完成。</div></div>
    <div class="step"><div class="n">STEP 3</div><div class="t">買える・並ぶ</div><div class="d">1枚から購入OK。店にも並んで、売れたら報酬。</div></div>
  </div>
  <div class="quick" id="mkQuick" hidden>
    <div class="qlead">タップするだけ。すぐ作れます。</div>
    <div class="qgrid">
      <button class="q" data-x="柴犬のシンプルな一本線の線画">柴犬の線画</button>
      <button class="q" data-x="禅の円相 ひと筆書き">禅の円相</button>
      <button class="q" data-x="夜の富士山と満月 ミニマル">富士と月</button>
      <button class="q" data-x="猫のシルエット ミニマル">猫</button>
      <button class="q" data-x="波 浮世絵風のミニマルライン">波</button>
      <button class="q" data-x="満月と山並み ミニマル">満月</button>
    </div>
  </div>
  <textarea id="p" maxlength="300" placeholder="例：富士山をミニマルな一本線で描いた黒Tシャツ"></textarea>
  <div class="row">
    <select id="k">
      <option value="">おまかせ</option>
      <option value="tee">Tシャツ</option>
      <option value="hoodie">パーカー</option>
      <option value="crewneck">スウェット</option>
    </select>
    <button id="go">つくる（無料でデザイン）</button>
  </div>
  <div class="price-hint">できた一着は <b>Tシャツ ¥4,900〜・スウェット ¥7,800〜・パーカー ¥8,800〜</b>。1枚から受注生産・買わなくてもOK。権利リスクがあるものだけ人が確認、あとは自動で公開。</div>
  <div class="ex" id="mkEx">例: <b data-x="柴犬のシンプルな線画 生成りトート">柴犬の線画</b> ・ <b data-x="禅の円相 ひと筆 黒Tシャツ">円相T</b> ・ <b data-x="夜の富士山と月 ミニマル パーカー">富士と月</b></div>
  <div id="out"></div>
  <div class="recent" id="recent" hidden>
    <h2>みんなが、さっき作った一着</h2>
    <div class="rgrid" id="rgrid"></div>
  </div>
</div>
<script>
const $=s=>document.querySelector(s);
function muShare(b){var u=b.dataset.u,t=b.dataset.t;if(navigator.share){navigator.share({title:t,url:u}).catch(function(){});}else if(navigator.clipboard){navigator.clipboard.writeText(u).then(function(){b.textContent='リンクをコピーしました ✓';}).catch(function(){});}else{prompt('このリンクを広めてください',u);}}
// プロンプトのエコー表示はユーザー入力 → 必ずエスケープ
function escHtml(s){return String(s).replace(/[&<>"']/g,function(c){return {'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c];});}
function yen(n){return (n||0).toLocaleString('ja-JP');}
// ファネル計測: 既存 /api/v1/event の許可イベント(cta_click/share)だけを使う。
// 効果検証はこれが母数 — make_buy クリック数 vs catalog_orders の MAKE-% 注文数。
function muEvent(ev,extra){try{
  var b=JSON.stringify({visitor_id:VIS||'v-anon',session_id:VIS||'v-anon',event:ev,path:'/make',extra:extra||{}});
  if(navigator.sendBeacon){navigator.sendBeacon('/api/v1/event',new Blob([b],{type:'application/json'}));}
  else{fetch('/api/v1/event',{method:'POST',headers:{'Content-Type':'application/json'},body:b});}
}catch(e){}}
// 着用イメージ(on-body mockup)はバックグラウンド生成 → /api/make/peek を
// ポーリングして完成したらカード画像を差し替え（着た姿=心理的所有感）。
// 6秒×20回のあと15秒×10回（計約4.5分）。タブ非表示中はfetchしない。
function pollFit(sku,design){
  var n=0;
  function schedule(){setTimeout(tick,n<20?6000:15000);}
  function tick(){
    n++;
    if(n>30){var f0=$('#mkFit');if(f0)f0.textContent='';return;}
    if(document.hidden){schedule();return;}
    fetch('/api/make/peek?sku='+encodeURIComponent(sku)).then(function(r){return r.json();}).then(function(j){
      if(j&&j.mockup&&j.mockup!==design){
        var im=$('#mkImg'),f=$('#mkFit');
        if(im){im.style.opacity=0;setTimeout(function(){im.src=j.mockup;im.style.opacity=1;},450);}
        if(f)f.textContent='👕 着ると、こうなる。鏡の前の自分を、想像してみて。';
        return;
      }
      schedule();
    }).catch(schedule);
  }
  schedule();
}
// ── A/B/C 割当 ──────────────────────────────────────────────
// visitor_id を mu-funnel.js の localStorage から拾う（無ければ生成）。
function muVisitor(){
  try{var r=localStorage.getItem('mu_funnel_v1');if(r){var j=JSON.parse(r);if(j&&j.visitor_id)return j.visitor_id;}}catch(e){}
  try{var id='v-'+Math.random().toString(36).slice(2)+Date.now().toString(36);
      localStorage.setItem('mu_funnel_v1',JSON.stringify({visitor_id:id,session_id:id,last:Date.now()}));return id;}catch(e){return '';}
}
var VIS=muVisitor();
// バリアント定義（コピー＋入力UX）。design/parseプロンプトはサーバ共通（品質担保）。
var MKV_DEFS={
  a:{h1:'言うだけで、Tシャツができる。',
     sub:'ひとこと言えば AI がデザイン → <b>その場で 1 枚から買える</b>。ログインも在庫もゼロ。あなたの一着はみんなの棚にも並び、<b style="color:#ffd700">売れたら作り手に報酬（¥600〜/枚）</b>。',
     ph:'例：富士山をミニマルな一本線で描いた黒Tシャツ', quick:false},
  b:{h1:'タップして、Tシャツ。',
     sub:'考えるより早い。<b>下から選ぶだけ</b>で AI が一着にします。自由入力もOK。<b style="color:#ffd700">売れたら報酬（¥600〜/枚）</b>。',
     ph:'自分の言葉でもOK（例：猫のシルエット）', quick:true},
  c:{h1:'何を着たい？',
     sub:'ひとことどうぞ。話すように書けば、AI があなたの一着にします。<b style="color:#ffd700">売れたら作り手に報酬（¥600〜/枚）</b>。',
     ph:'「〇〇な感じのTシャツがほしい」みたいに話して', quick:false}
};
// サーバが variant を焼いていればそれ、無ければ visitor のハッシュで決定的3分割。
var SV='__SERVER_VARIANT__', LOCKED=__WINNER_LOCKED__;
function hash3(s){var h=0;for(var i=0;i<s.length;i++){h=(h*31+s.charCodeAt(i))>>>0;}return ['a','b','c'][h%3];}
var MKV=(SV==='a'||SV==='b'||SV==='c')?SV:hash3(VIS||'a');
(function applyVariant(){
  var d=MKV_DEFS[MKV]||MKV_DEFS.a;
  var h=$('#mkH1'); if(h)h.textContent=d.h1;
  var s=$('#mkSub'); if(s)s.innerHTML=d.sub;
  var p=$('#p'); if(p)p.placeholder=d.ph;
  var q=$('#mkQuick'); if(q)q.hidden=!d.quick;
  var ex=$('#mkEx'); if(ex&&d.quick)ex.hidden=true;
  document.body.setAttribute('data-variant',MKV);
})();
document.querySelectorAll('.ex b').forEach(b=>b.onclick=()=>{$('#p').value=b.dataset.x;});
// 例文クイックボタン（B案）: タップで充填して即生成。
document.querySelectorAll('#mkQuick .q').forEach(b=>b.onclick=()=>{$('#p').value=b.dataset.x;runMake();});
// 直近の作例 — 品質の証明・出来上がりイメージ・「動いてる店」の気配
fetch('/api/make/recent').then(r=>r.json()).then(j=>{
  if(!j.items||!j.items.length) return;
  $('#rgrid').innerHTML=j.items.map(it=>'<a href="/shop/'+encodeURIComponent(it.sku)+'"><img loading=lazy src="'+it.img+'" alt=""><div class=rl>'+(it.label||'')+'</div><div class=rp>¥'+(it.price||'')+'</div></a>').join('');
  $('#recent').hidden=false;
}).catch(()=>{});
// 生成シアター: お題のエコー + 物語のステータス + 進捗バー。戻り値で停止。
function genTheater(p){
  var msgs=['お題を、読み解いています…','筆を、とりました','線を一本、引いています…','色を、えらんでいます…','余白と、相談しています…','布にのせて、確かめています…','タグに名前を入れています…','棚をあけて、待っています…'];
  $('#out').innerHTML='<div class=gen><div class=enso></div><div class=gq>「<b></b>」を、一枚の絵に。</div><div class=gmsg></div><div class=gbar><div class=gfill></div></div><div class=gnote>世界のどこにもない一枚を生成中 — だいたい 10〜20 秒。同じ絵は二度と生まれません。</div></div>';
  document.querySelector('.gen .gq b').textContent=p.length>42?p.slice(0,42)+'…':p;
  var gm=document.querySelector('.gmsg'),gf=document.querySelector('.gfill');
  var i=0; gm.textContent=msgs[0];
  var t1=setInterval(function(){i=(i+1)%msgs.length;gm.style.opacity=0;setTimeout(function(){gm.textContent=msgs[i];gm.style.opacity=1;},320);},2400);
  var pr=2; var t2=setInterval(function(){pr=Math.min(93,pr+(pr<55?5:1.4));gf.style.width=pr+'%';},600);
  return function(){clearInterval(t1);clearInterval(t2);if(gf)gf.style.width='100%';};
}
async function runMake(){
  const p=$('#p').value.trim(); if(!p){$('#p').focus();return;}
  const k=$('#k').value;
  muEvent('cta_click',{cta:'make_create',variant:MKV});
  $('#go').disabled=true; $('#go').innerHTML='<span class=spin></span>つくっています…';
  const genDone=genTheater(p);
  try{
    // v(バリアント)と visitor(UU)を必ず添えて投稿 → サーバが勝者判定の母数に刻む。
    const r=await fetch('/api/make?prompt='+encodeURIComponent(p)+(k?'&kind='+k:'')
      +'&v='+encodeURIComponent(MKV)+(VIS?'&visitor='+encodeURIComponent(VIS):''),{method:'POST'});
    const j=await r.json();
    genDone();
    if(!j.ok){ $('#out').innerHTML='<div class=err>'+(j.error||'うまく作れませんでした。もう一度お試しください。')+'</div>'; }
    else{
      // 認証ゲート: 本端末で認証済み(mu_make_ok クッキー)なら即リビール。
      // 初回は結果(着用イメージ+棚)を見る前にメール+6桁コード認証を挟む。
      if(/(?:^|;\s*)mu_make_ok=1/.test(document.cookie)){ renderResult(j,p); }
      else{ renderGate(j,p); }
    }
  }catch(e){ genDone(); $('#out').innerHTML='<div class=err>通信エラー。もう一度お試しください。</div>'; }
  $('#go').disabled=false; $('#go').textContent='つくる';
}
// 生成済みの結果カードを描画（認証通過後 or 認証済み端末）。
function renderResult(j,p){
  // 行動科学の根拠: IKEA効果(自作品は+63%高く評価/Norton+2012)→「あなたが作った」と
  // プロンプトのエコーで作者性を返す。心理的所有感(Peck&Shu 2009)→着用イメージ差替+所有語CTA。
  var url = j.buy_url || j.pdp_url || '';
  var pEcho = p.length>60 ? p.slice(0,60)+'…' : p;
  var own = '<div class=own><b>あなたの言葉</b>から、世界に1枚が生まれました。<span class=pq>「'+escHtml(pEcho)+'」</span></div>';
  var buy = j.buy_url ? '<a class=buy href="'+j.buy_url+'" onclick="muEvent(\'cta_click\',{cta:\'make_buy\',sku:\''+j.sku+'\'})">この一着を、自分のものにする ¥'+yen(j.retail_jpy)+' →<small>サイズを選ぶだけ · 1枚から受注生産</small></a>' : '';
  var share = url ? '<button class=share onclick="muEvent(\'share\',{sku:\''+j.sku+'\'});muShare(this)" data-u="'+url+'" data-t="'+((j.display||'MU')+' / wearmu')+'">📣 シェアして広める</button>' : '';
  var spread = url ? '<div class=spread>棚にも並びました。広めるほどこの子が売れる → 作り手のあなたに報酬（¥600〜/枚）。</div>' : '';
  var one = j.auto_approved ? '<div class=one>🌱 <b>世界に1枚。</b>同じ絵は二度と生成されません。ファーストオーナーは、まだいません。</div>' : '';
  var nt = j.auto_approved ? '' : '<div class=note>'+(j.note||'つくりました。確認後に公開・購入できます。')+'</div>';
  $('#out').innerHTML=own+'<div class="card reveal"><img id=mkImg src="'+j.design_url+'" alt=""><div class=meta>'
    +'<div class=nm>'+(j.display||'')+'</div>'
    +'<div class=by>DESIGNED BY YOU × MU</div>'
    +'<div class=pr>¥'+yen(j.retail_jpy)+'</div>'
    +'<div style="font-size:13px;color:rgba(245,245,240,.7)">'+(j.hook||'')+'</div>'
    + one
    +'<div class=fitnote id=mkFit>'+(j.auto_approved?'👕 着用イメージを準備中… 数十秒でここに届きます':'')+'</div>'
    + buy + share + spread + nt
    +'</div></div>';
  $('#out').scrollIntoView({behavior:'smooth',block:'nearest'});
  if(j.auto_approved && j.sku) pollFit(j.sku, j.design_url);
}
// 生成後リビールゲート: メール→6桁コード認証で結果を開放。
function renderGate(j,p){
  var pEcho = p.length>60 ? p.slice(0,60)+'…' : p;
  $('#out').innerHTML=
    '<div class=own><b>あなたの言葉</b>から、世界に1枚が生まれました。<span class=pq>「'+escHtml(pEcho)+'」</span></div>'
    +'<div class="card gate">'
    +'<div class=gatewrap><img class=gateimg src="'+j.design_url+'" alt=""><div class=gatelock>🔒</div></div>'
    +'<div class=gatebody>'
    +'<div class=gateh>あと一歩。<b>メールで認証</b>すると、この一着が現れます。</div>'
    +'<div class=gatesub>確認のため6桁コードをお送りします。作品の保存先になり、売れたらお知らせします。</div>'
    +'<div id=gStep1><div class=saverow><input id=gEmail type=email placeholder="you@example.com" autocomplete=email inputmode=email><button id=gSend>コードを送る</button></div></div>'
    +'<div id=gStep2 style="display:none"><div class=saverow><input id=gCode type=text placeholder="6桁コード" inputmode=numeric autocomplete=one-time-code maxlength=6 style="letter-spacing:.3em;text-align:center;font-family:monospace"><button id=gVerify>認証して見る</button></div><button id=gBack class=gback>メールアドレスを入れ直す</button></div>'
    +'<div class=savemsg id=gMsg></div>'
    +'</div></div>';
  var email='';
  var msg=$('#gMsg');
  function showMsg(t,err){msg.style.color=err?'#ff8a7a':'rgba(245,245,240,.7)';msg.textContent=t;}
  var send=$('#gSend');
  send.onclick=function(){
    email=$('#gEmail').value.trim();
    if(!email||email.indexOf('@')<1){$('#gEmail').focus();return;}
    send.disabled=true;showMsg('送信中…',false);
    fetch('/api/make/verify/send',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({sku:j.sku,email:email})})
      .then(function(r){return r.json();}).then(function(x){
        send.disabled=false;
        if(x.ok){$('#gStep1').style.display='none';$('#gStep2').style.display='';showMsg('「'+email+'」にコードを送りました（15分有効）。',false);$('#gCode').focus();muEvent('cta_click',{cta:'make_verify_send',sku:j.sku});}
        else{showMsg(x.error||'送れませんでした',true);}
      }).catch(function(){send.disabled=false;showMsg('通信エラー。もう一度どうぞ。',true);});
  };
  $('#gEmail').addEventListener('keydown',function(e){if(e.key==='Enter')send.click();});
  $('#gBack').onclick=function(){$('#gStep2').style.display='none';$('#gStep1').style.display='';showMsg('',false);$('#gEmail').focus();};
  $('#gVerify').onclick=function(){
    var code=$('#gCode').value.trim(), vb=$('#gVerify');
    if(!/^[0-9]{6}$/.test(code)){$('#gCode').focus();showMsg('6桁の数字を入力してください',true);return;}
    vb.disabled=true;showMsg('確認中…',false);
    fetch('/api/make/verify/check',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({sku:j.sku,email:email,code:code})})
      .then(function(r){return r.json();}).then(function(x){
        if(x.ok){muEvent('cta_click',{cta:'make_verified',sku:j.sku});renderResult(j,p);}
        else{vb.disabled=false;showMsg(x.error||'確認できませんでした',true);}
      }).catch(function(){vb.disabled=false;showMsg('通信エラー。もう一度どうぞ。',true);});
  };
  $('#gCode').addEventListener('keydown',function(e){if(e.key==='Enter')$('#gVerify').click();});
  $('#out').scrollIntoView({behavior:'smooth',block:'nearest'});
}
$('#go').onclick=runMake;
$('#p').addEventListener('keydown',e=>{if((e.metaKey||e.ctrlKey)&&e.key==='Enter')runMake();});
</script></body></html>"##;

/// POST /api/make?prompt=…&kind=… — public NL → product. status='review',
/// brand='minna', cost-guarded (hourly cap + global budget gate). Mirrors
/// admin_nl_add but unauthenticated, review-only, and single-image (cost-min).
pub async fn public_make(State(db): State<Db>, Query(q): Query<MakeQuery>) -> Response {
    let prompt_in = q.prompt.trim().to_string();
    if prompt_in.is_empty() || prompt_in.chars().count() > 300 {
        return (StatusCode::BAD_REQUEST, axum::Json(serde_json::json!({"ok":false,"error":"作りたいものを入力してください（300文字以内）"}))).into_response();
    }
    {
        let conn = db.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM catalog_products WHERE brand='minna' AND created_at > datetime('now','-1 hour')",
            [], |r| r.get(0)).unwrap_or(0);
        if n >= MAKE_HOURLY_CAP {
            return (StatusCode::TOO_MANY_REQUESTS, axum::Json(serde_json::json!({"ok":false,"error":"いまアクセスが集中しています。少し時間をおいて試してください。"}))).into_response();
        }
    }
    let parse_prompt = format!(
        "Parse this JP/EN product idea into compact JSON. ONLY emit JSON, no prose, no markdown fences.\n\
         Schema: {{\"kind\":\"tee|hoodie|crewneck\", \
                   \"theme_brief\":\"<one short English design brief for the chest graphic>\", \
                   \"display\":\"<short JP brand-mark name, <=10 chars>\", \
                   \"hook\":\"<one JP marketing sentence for the PDP>\", \
                   \"retail_jpy\":<integer>, \
                   \"flagged\":<true ONLY if this needs a human to review before public sale: a real brand/trademark/logo, a real living person's name or likeness, a copyrighted character/IP, or hateful/sexual/violent/illegal content; otherwise false>, \
                   \"flag_reason\":\"<short JP reason if flagged, else empty>\"}}\n\
         Bias toward flagged=false (auto-approve). Only set true when clearly risky.\n\
         If kind is missing, default to 'tee'. retail default 4900 tee / 8800 hoodie / 7800 crewneck.\n\
         Input: {}", prompt_in);
    let parsed_json = match crate::gemini::call_gemini_text(&parse_prompt).await {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_GATEWAY, axum::Json(serde_json::json!({"ok":false,"error":format!("生成に失敗しました: {}", e)}))).into_response(),
    };
    let json_str: String = parsed_json.find('{').and_then(|i| parsed_json[i..].rfind('}').map(|j| parsed_json[i..i+j+1].to_string())).unwrap_or(parsed_json.clone());
    let parsed: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_GATEWAY, axum::Json(serde_json::json!({"ok":false,"error":"うまく解釈できませんでした。言い換えてお試しください。"}))).into_response(),
    };
    let kind_parsed = parsed["kind"].as_str().unwrap_or("tee");
    // only DTG kinds are offered publicly
    let allowed = ["tee", "hoodie", "crewneck"];
    let kind: &str = match q.kind.as_deref() {
        Some(k) if allowed.contains(&k) => k,
        _ if allowed.contains(&kind_parsed) => kind_parsed,
        _ => "tee",
    };
    let theme_brief = parsed["theme_brief"].as_str().unwrap_or(&prompt_in).to_string();
    let display = parsed["display"].as_str().unwrap_or("MU").to_string();
    let hook = parsed["hook"].as_str().unwrap_or("自然言語から自動生成").to_string();
    // 基本は AI 自動承認 → 即 live(買える)。商標/実在人物/著名キャラ/不適切のみ human review。
    let flagged = parsed["flagged"].as_bool().unwrap_or(false);
    let flag_reason = parsed["flag_reason"].as_str().unwrap_or("").to_string();
    let (is_active_i, status_s): (i64, &str) = if flagged { (0, "review") } else { (1, "live") };
    let Some(spec) = PRODUCT_SPECS.iter().find(|s| s.kind == kind) else {
        return (StatusCode::BAD_REQUEST, axum::Json(serde_json::json!({"ok":false,"error":"未対応の種類です"}))).into_response();
    };
    let retail_jpy = parsed["retail_jpy"].as_i64().unwrap_or(spec.retail_jpy);
    let seed = format!("mk{:08x}", rand::random::<u32>());
    let slug = { let s: String = display.chars().filter(|c| c.is_ascii_alphanumeric()).take(12).collect::<String>().to_uppercase(); if s.is_empty() { "MAKE".to_string() } else { s } };
    let sku = format!("MAKE-{}-{}-{}", slug, kind.to_uppercase().replace('_', "-"), seed);
    let charged = { let conn = db.lock().unwrap(); spend_or_refuse(&conn, "ai_image", GEMINI_IMAGE_COST_JPY, &format!("public_make sku={}", sku), Some(&sku)) };
    if !charged {
        return (StatusCode::FAILED_DEPENDENCY, axum::Json(serde_json::json!({"ok":false,"error":"本日の生成枠が上限に達しました。また明日お試しください。"}))).into_response();
    }
    let design_prompt = format!(
        "Print-ready chest graphic at 300 DPI on a pure white background. \
         Style brief: {}. NO model, NO mockup, just the artwork, centered. Variation key: {}.",
        theme_brief, seed);
    let img = match crate::gemini::call_gemini(&design_prompt).await {
        Ok(i) => i,
        Err(e) => return (StatusCode::BAD_GATEWAY, axum::Json(serde_json::json!({"ok":false,"error":format!("デザイン生成に失敗: {}", e)}))).into_response(),
    };
    // デフォルト透過: 生成は白(or黒)背景 → 出来上がりを後処理で背景透過にしてから保存。
    // 色付き生地でも白(黒)の四角が出ない。処理失敗時は元画像にフォールバック。
    let (design_bytes, design_mime) = match make_design_transparent(&img.bytes) {
        Some(b) => (b, "image/png".to_string()),
        None => (img.bytes.clone(), img.mime.clone()),
    };
    let key = format!("catalog/{}.png", sku);
    let Some(url) = crate::store_r2_bytes(&key, &design_bytes, &design_mime).await else {
        return (StatusCode::BAD_GATEWAY, axum::Json(serde_json::json!({"ok":false,"error":"画像アップロードに失敗しました"}))).into_response();
    };
    // A/B/C: 投稿に variant と visitor を刻む（勝者UU判定の母数）。
    let ab_variant = make_variant_norm(q.v.as_deref());
    let ab_visitor = q.visitor.as_deref().map(str::trim).filter(|s| !s.is_empty() && s.len() <= 80);
    let meta_json = match (ab_variant, ab_visitor) {
        (Some(v), Some(vis)) => Some(serde_json::json!({"make_variant": v, "make_visitor": vis}).to_string()),
        (Some(v), None) => Some(serde_json::json!({"make_variant": v}).to_string()),
        _ => None,
    };
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "INSERT OR IGNORE INTO catalog_brands (slug, name, emoji, color_primary, tagline, is_active, revenue_share_pct)
             VALUES ('minna', 'みんなでつくる MU', '🌱', '#88c97a', '言うだけで作れる — あなたのアイデアを MU が形に', 1, 0)",
            [],
        );
        let desc = format!("{} — {}", display, hook);
        let _ = conn.execute(
            "INSERT INTO catalog_products (
                sku, brand, label, description_ja, retail_price_jpy,
                printful_product_id, printful_variant_id, printful_placement,
                printful_print_w, printful_print_h,
                design_file, mockup_main_file, mockup_url_external,
                is_active, sort_order, status, fulfillment_route, legacy_source, meta_json
             ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            rusqlite::params![
                &sku, "minna", desc, desc, retail_jpy,
                spec.printful_product_id, spec.printful_variant_id, spec.placement,
                0, 0,
                &url, &url, &url,
                is_active_i, 50, status_s, "printful_dtg", "public_make", meta_json,
            ],
        );
        // 勝者未確定なら、各バリアントの「作成したユニーク訪問者数」を集計し、
        // 最初に閾値到達した案を cv_config['make_winner'] に焼く（以後全員その案）。
        if ab_variant.is_some() && cv_get(&conn, "make_winner").is_none() {
            if let Ok(mut stmt) = conn.prepare(
                "SELECT json_extract(meta_json,'$.make_variant') v,
                        COUNT(DISTINCT json_extract(meta_json,'$.make_visitor')) uu
                 FROM catalog_products
                 WHERE legacy_source='public_make'
                   AND json_extract(meta_json,'$.make_variant') IS NOT NULL
                   AND json_extract(meta_json,'$.make_visitor') IS NOT NULL
                 GROUP BY v",
            ) {
                let rows: Vec<(String, i64)> = stmt
                    .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                    .map(|it| it.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default();
                if let Some((win, uu)) = rows.iter().find(|(_, uu)| *uu >= MAKE_AB_WIN_THRESHOLD) {
                    cv_put(&conn, "make_winner", win,
                        &format!("/make A/B/C: variant {} reached {} unique-visitor creations", win, uu));
                }
            }
        }
    }
    // Cost-minimal: only the Printful on-body mockup (no extra Gemini images).
    let (pp, pv, url_c, sku_c, db_c) = (spec.printful_product_id, spec.printful_variant_id, url.clone(), sku.clone(), db.clone());
    tokio::spawn(async move { let _ = generate_onbody_mockup(db_c, sku_c, pp, pv, url_c).await; });

    let note = if flagged {
        let r = if flag_reason.is_empty() { "内容".to_string() } else { flag_reason.clone() };
        format!("つくりました。少し確認したい点（{}）があるので人の目を通します。OKならすぐ公開・購入できます。", r)
    } else {
        "できました！もう棚に並びました。今すぐ買えます。着用イメージは数十秒で反映されます。".to_string()
    };
    let buy_url = if flagged { serde_json::Value::Null } else { serde_json::json!(format!("https://wearmu.com/shop/{}", sku)) };
    axum::Json(serde_json::json!({
        "ok": true,
        "sku": sku,
        "kind": kind,
        "display": display,
        "hook": hook,
        "retail_jpy": retail_jpy,
        "design_url": url,
        "pdp_url": format!("https://wearmu.com/shop/{}", sku),
        "status": status_s,
        "auto_approved": !flagged,
        "buy_url": buy_url,
        "note": note,
    })).into_response()
}

/// Tiny shared header/footer for /returns /faq /shipping pages so
/// they match the MU dark aesthetic without pulling site_header_html.
fn legal_page(title: &str, body_html: &str) -> Html<String> {
    Html(format!(
        r##"<!doctype html><html lang="ja"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} — wearmu.com</title>
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
body{{background:#0a0a0a;color:#f5f5f0;font-family:'Helvetica Neue','Hiragino Sans',Arial,sans-serif;line-height:1.7;font-size:14px}}
nav{{padding:16px 24px;border-bottom:1px solid rgba(255,255,255,0.08);display:flex;justify-content:space-between;align-items:center}}
nav a{{color:#f5f5f0;text-decoration:none;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.85}}
nav .brand{{font-weight:900;letter-spacing:0.4em}}
.wrap{{max-width:760px;margin:0 auto;padding:50px 24px 80px}}
h1{{font-size:26px;font-weight:800;margin-bottom:24px;letter-spacing:-0.01em}}
h2{{font-size:14px;font-weight:700;color:#ffd700;margin:32px 0 10px;letter-spacing:0.1em;text-transform:uppercase}}
p{{margin-bottom:14px;color:rgba(245,245,240,0.82);font-size:13.5px}}
ul{{margin:0 0 16px 22px;color:rgba(245,245,240,0.82);font-size:13.5px}}
li{{margin-bottom:6px}}
a.btn{{display:inline-block;margin-top:8px;padding:10px 18px;border:1px solid rgba(255,215,0,0.4);color:#ffd700;text-decoration:none;border-radius:4px;font-size:12px;letter-spacing:0.1em}}
a.btn:hover{{background:rgba(255,215,0,0.08)}}
.legal-fine{{font-size:11px;color:rgba(245,245,240,0.45);margin-top:36px;border-top:1px solid rgba(255,255,255,0.06);padding-top:14px}}
</style></head><body>
<nav>
  <a class="brand" href="/">MU</a>
  <div>
    <a href="/shop">SHOP</a>
    <a href="/buy" style="margin-left:14px">DROPS</a>
  </div>
</nav>
<div class="wrap"><h1>{title}</h1>{body}
<div class="legal-fine">最終更新: 2026-05-22 · © 2026 MU / Enabler Inc. · お問い合わせ <a href="mailto:info@enablerdao.com" style="color:#ffd700">info@enablerdao.com</a></div>
</div></body></html>"##,
        title = title, body = body_html
    ))
}

pub async fn returns_page() -> Html<String> {
    legal_page("返品ポリシー / Returns", r##"
<p>MU の /shop / /buy 商品は <strong>すべて受注生産 (made-to-order)</strong> です。
注文後に Printful EU / JP 等のパートナー工場で 1 枚ずつ印刷・縫製しています。
そのため通常のアパレル EC と比べ返品条件が異なります。</p>

<h2>返品・交換できる場合</h2>
<ul>
<li>商品の <strong>印刷不良 / プリントずれ / 破れ</strong> など製造側に起因する不良</li>
<li>注文と <strong>異なるサイズ・色・SKU</strong> が届いた場合</li>
<li>配送中の <strong>破損</strong> (写真をご提供いただきます)</li>
<li>到着後 <strong>30 日以内</strong> に <a href="mailto:returns@wearmu.com" style="color:#ffd700">returns@wearmu.com</a> にご連絡いただいた場合</li>
</ul>
<p>上記に該当する場合、 無償交換または全額返金いたします。 送料も MU 負担です。</p>

<h2>返品・交換できない場合</h2>
<ul>
<li>「サイズ感が思ったのと違う」 等の <strong>主観的な理由</strong> (サイズチャート PDP に掲載済)</li>
<li>到着後 <strong>30 日</strong> を超えた連絡</li>
<li>使用済・洗濯済の商品</li>
<li>注文時に入力した <strong>住所の誤り</strong> による誤配 (配送業者の再配達料を実費請求)</li>
</ul>

<h2>手順</h2>
<ol style="margin:0 0 16px 22px;color:rgba(245,245,240,0.82);font-size:13.5px">
<li><a href="mailto:returns@wearmu.com" style="color:#ffd700">returns@wearmu.com</a> に注文番号 + 写真 + 内容をご連絡</li>
<li>24 時間以内に MU から返信 + 返品先住所をお知らせ</li>
<li>商品到着確認 → 5 営業日以内に交換品発送 or 返金処理 (Stripe 経由・元の決済手段に戻ります)</li>
</ol>

<h2>返品申請フォーム</h2>
<p>下記フォームから直接申請できます。 初回 (このアクセス元からの申請が初めて) の場合はその場で受理します。
過去に申請履歴がある場合は不正防止のため担当が内容を確認のうえご連絡します。
返金は受理後に手動で処理 (Stripe 経由・元の決済手段) します。</p>
<form id="ret-form" style="margin-top:14px;max-width:520px" onsubmit="return submitReturn(event)">
  <label style="display:block;margin-bottom:10px;font-size:12px;letter-spacing:0.05em">
    注文番号 (確認メール記載) <span style="color:#ffd700">*</span><br>
    <input name="order_ref" required maxlength="120"
      style="width:100%;margin-top:5px;padding:9px;background:#141414;border:1px solid rgba(255,255,255,0.15);color:#f5f5f0;border-radius:4px;font-size:13px">
  </label>
  <label style="display:block;margin-bottom:10px;font-size:12px;letter-spacing:0.05em">
    メールアドレス <span style="color:#ffd700">*</span><br>
    <input name="contact_email" type="email" required maxlength="200"
      style="width:100%;margin-top:5px;padding:9px;background:#141414;border:1px solid rgba(255,255,255,0.15);color:#f5f5f0;border-radius:4px;font-size:13px">
  </label>
  <label style="display:block;margin-bottom:10px;font-size:12px;letter-spacing:0.05em">
    返品理由 <span style="color:#ffd700">*</span><br>
    <textarea name="reason" required maxlength="1000" rows="3"
      style="width:100%;margin-top:5px;padding:9px;background:#141414;border:1px solid rgba(255,255,255,0.15);color:#f5f5f0;border-radius:4px;font-size:13px"></textarea>
  </label>
  <label style="display:block;margin-bottom:14px;font-size:12px;letter-spacing:0.05em">
    写真 URL (任意・破損 / 不良の場合)<br>
    <input name="photo_url" type="url" maxlength="500"
      style="width:100%;margin-top:5px;padding:9px;background:#141414;border:1px solid rgba(255,255,255,0.15);color:#f5f5f0;border-radius:4px;font-size:13px">
  </label>
  <button type="submit" class="btn" style="cursor:pointer;background:none;font-family:inherit">返品申請する</button>
  <span id="ret-msg" style="margin-left:12px;font-size:12px"></span>
</form>
<p style="margin-top:14px;font-size:12px;color:rgba(245,245,240,0.5)">フォームが使えない場合は <a href="mailto:returns@wearmu.com" style="color:#ffd700">returns@wearmu.com</a> まで。</p>
<script>
async function submitReturn(e){
  e.preventDefault();
  var f=e.target, btn=f.querySelector('button'), msg=document.getElementById('ret-msg');
  var body={
    order_ref:f.order_ref.value.trim(),
    contact_email:f.contact_email.value.trim(),
    reason:f.reason.value.trim(),
    photo_url:f.photo_url.value.trim()||null
  };
  btn.disabled=true; msg.style.color='#aaa'; msg.textContent='送信中…';
  try{
    var r=await fetch('/api/returns',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
    var j=await r.json();
    if(r.ok&&j.ok){
      msg.style.color='#7CFC9B';
      msg.textContent=j.message||'受け付けました';
      f.reset();
    }else{
      msg.style.color='#ff6b6b';
      msg.textContent=(j&&j.error)||'送信に失敗しました';
      btn.disabled=false;
    }
  }catch(err){
    msg.style.color='#ff6b6b'; msg.textContent='通信エラー'; btn.disabled=false;
  }
  return false;
}
</script>
"##)
}

pub async fn faq_page() -> Html<String> {
    legal_page("FAQ", r##"
<h2>発送はいつ?</h2>
<p>注文確定後、 製造に <strong>2-5 営業日</strong> + 配送に国別 5-14 日。 合計 7-19 日が目安です。 (詳細は <a href="/shipping" style="color:#ffd700">/shipping</a>)</p>

<h2>追跡番号は?</h2>
<p>Printful から MU を経由してメールで自動送信されます。 DHL / FedEx / 日本ポスト等のトラッキング URL付き。</p>

<h2>サイズが分からない</h2>
<p>各商品 PDP にサイズチャート (cm) があります。 不安な場合は普段の洋服サイズより 1 つ大きめを推奨。</p>

<h2>支払い方法</h2>
<p>Stripe 経由でクレジットカード (Visa / Master / Amex / JCB) + Apple Pay + Google Pay。 一部商品は SUZURI 経由で国内コンビニ決済も可能。</p>

<h2>領収書は?</h2>
<p>Stripe 決済完了後、 自動で領収書 PDF がメール送信されます。 法人購入の場合は <a href="mailto:info@enablerdao.com" style="color:#ffd700">info@enablerdao.com</a> までご連絡で「株式会社イネブラ」 宛の請求書発行も対応。</p>

<h2>返品できる?</h2>
<p>製造不良 / 誤配 / 破損は 30 日以内ご連絡で無償交換。 詳細は <a href="/returns" style="color:#ffd700">/returns</a>。</p>

<h2>大量注文 (10 着〜) は?</h2>
<p><a href="mailto:info@enablerdao.com" style="color:#ffd700">info@enablerdao.com</a> までご相談ください。 道場ユニフォーム・大会記念 Tee 等の bulk 価格表があります。</p>

<h2>デザインを自分で持ち込みたい</h2>
<p>個人ブランド対応 (/api-keys) もあります。 30 SKU まで無料、 以降 30 pt / SKU。</p>
"##)
}

pub async fn shipping_page() -> Html<String> {
    legal_page("配送 / Shipping", r##"
<p>MU 全商品は <strong>受注生産 + Printful EU / JP 倉庫から直送</strong>。 注文確定 → 製造 2-5 営業日 → 配送。 国別の目安は下記。</p>

<h2>送料 (目安)</h2>
<ul>
<li>🇯🇵 Japan — ¥800 / 5-10 日</li>
<li>🇺🇸 United States — ¥1,400 / 7-14 日</li>
<li>🇪🇺 EU (DE / FR / NL / IT) — ¥600 / 5-10 日</li>
<li>🇬🇧 United Kingdom — ¥900 / 5-10 日</li>
<li>🇨🇦 Canada — ¥1,500 / 7-14 日</li>
<li>🇦🇺 Australia — ¥1,700 / 7-14 日</li>
</ul>
<p>実費は Stripe Checkout の住所入力後に表示されます。 上記は単品 (T シャツ / ラッシュガード) 想定。 hoodie / 複数同梱で増減。</p>

<h2>追跡</h2>
<p>DHL / FedEx / 日本ポストの <strong>追跡番号付き</strong>。 発送完了時に自動メール送信。</p>

<h2>関税</h2>
<p>輸入国の関税は受取人負担となります。 EU 内・JP 国内発送は関税なし。 US/CA/AU 輸入は通常 5-15% 程度 (商品価値ベース)。</p>

<h2>遅延・配送事故</h2>
<p>追跡番号で「投函済」 から 14 日経過しても未着の場合は <a href="mailto:returns@wearmu.com" style="color:#ffd700">returns@wearmu.com</a> までご連絡。 再送 or 全額返金で対応します。</p>
"##)
}

#[derive(Deserialize)]
pub struct OrdersQuery {
    pub token: String,
}

#[derive(Deserialize)]
pub struct ReplayQuery {
    pub token: String,
}

/// GET /admin/catalog/orders/:id/replay?token= — retry fulfillment for
/// a catalog_orders row that failed. Looks up the stripe session_id,
/// re-pulls the Stripe Session, deletes the catalog_orders row (so the
/// idempotency check inside fulfill_catalog_order doesn't skip), then
/// re-runs fulfillment. Token-gated.
pub async fn admin_orders_replay(
    State(db): State<Db>,
    Path(order_id): Path<i64>,
    Query(q): Query<ReplayQuery>,
) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let session_id: Option<String> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT stripe_session_id FROM catalog_orders WHERE id=?",
            rusqlite::params![order_id],
            |r| r.get::<_, String>(0),
        ).ok()
    };
    let Some(sid) = session_id else {
        return (StatusCode::NOT_FOUND, format!("order #{} not found", order_id)).into_response();
    };
    // Pull the full Stripe Session so we have the latest shipping_details.
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    if stripe_key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "STRIPE_SECRET_KEY unset").into_response();
    }
    let url = format!(
        "https://api.stripe.com/v1/checkout/sessions/{}",
        sid
    );
    let session = match reqwest::Client::new().get(&url).basic_auth(&stripe_key, None::<&str>).send().await {
        Ok(r) if r.status().is_success() => r.json::<serde_json::Value>().await.ok(),
        Ok(r) => {
            let s = r.status();
            return (StatusCode::BAD_GATEWAY,
                format!("stripe {}: {}", s, r.text().await.unwrap_or_default()))
                .into_response();
        }
        Err(e) => return (StatusCode::BAD_GATEWAY, format!("stripe: {}", e)).into_response(),
    };
    let Some(session) = session else {
        return (StatusCode::BAD_GATEWAY, "no session JSON").into_response();
    };
    // Clear the old failed row so the idempotency guard inside
    // fulfill_catalog_order doesn't short-circuit.
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "DELETE FROM catalog_orders WHERE id=?",
            rusqlite::params![order_id],
        );
    }
    // Re-run fulfillment (in the foreground so the operator sees the result).
    fulfill_catalog_order(db, session).await;
    axum::Json(serde_json::json!({
        "ok": true,
        "replayed_session": sid,
        "note": "Check /admin/catalog/orders for the new row's status",
    })).into_response()
}

/// GET /admin/catalog/orders?token= — last 20 catalog_orders rows so
/// we can see why revenue is ¥0 despite an order being recorded.
pub async fn admin_orders(
    State(db): State<Db>,
    Query(q): Query<OrdersQuery>,
) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let rows: Vec<serde_json::Value> = {
        let conn = db.lock().unwrap();
        conn.prepare(
            "SELECT id, stripe_session_id, sku, amount_jpy, customer_email,
                    customer_name, printful_order_id, status,
                    SUBSTR(COALESCE(printful_response_json,''), 1, 400) AS pf_excerpt,
                    SUBSTR(COALESCE(shipping_address_json,''), 1, 200) AS addr,
                    created_at
             FROM catalog_orders
             ORDER BY id DESC LIMIT 20",
        )
        .ok()
        .and_then(|mut s| {
            s.query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "stripe_session_id": r.get::<_, String>(1)?,
                    "sku": r.get::<_, Option<String>>(2)?,
                    "amount_jpy": r.get::<_, Option<i64>>(3)?,
                    "customer_email": r.get::<_, Option<String>>(4)?,
                    "customer_name": r.get::<_, Option<String>>(5)?,
                    "printful_order_id": r.get::<_, Option<String>>(6)?,
                    "status": r.get::<_, Option<String>>(7)?,
                    "printful_response_excerpt": r.get::<_, Option<String>>(8)?,
                    "shipping_address_excerpt": r.get::<_, Option<String>>(9)?,
                    "created_at": r.get::<_, String>(10)?,
                }))
            })
            .ok()
            .map(|it| it.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
    };
    axum::Json(serde_json::json!({"count": rows.len(), "orders": rows})).into_response()
}

#[derive(Deserialize)]
pub struct StatusQuery {
    pub token: String,
}

/// GET /admin/catalog/status?token= — operator dashboard JSON.
/// Returns budget burn-down, SKU counts, last 20 generator jobs, last 20 orders.
/// No auth = no PII leak (the cron writes here too so we need lightweight read).
pub async fn admin_status(
    State(db): State<Db>,
    Query(q): Query<StatusQuery>,
) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let conn = db.lock().unwrap();
    let spent = spent_month_jpy(&conn);
    let spent_lifetime = spent_total_jpy(&conn);
    let auto_skus: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM catalog_products WHERE brand='auto'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let total_skus: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM catalog_products WHERE is_active=1",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let orders_24h: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM catalog_orders WHERE created_at > datetime('now','-1 day')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let orders_total: i64 = conn
        .query_row("SELECT COUNT(*) FROM catalog_orders", [], |r| r.get(0))
        .unwrap_or(0);
    let recent_jobs: Vec<serde_json::Value> = conn
        .prepare(
            "SELECT theme, kind, seed, status, sku, error, created_at
             FROM catalog_gen_jobs ORDER BY id DESC LIMIT 20",
        )
        .ok()
        .and_then(|mut s| {
            s.query_map([], |r| {
                Ok(serde_json::json!({
                    "theme": r.get::<_, String>(0)?,
                    "kind":  r.get::<_, String>(1)?,
                    "seed":  r.get::<_, String>(2)?,
                    "status":r.get::<_, String>(3)?,
                    "sku":   r.get::<_, Option<String>>(4)?,
                    "error": r.get::<_, Option<String>>(5)?,
                    "created_at": r.get::<_, String>(6)?,
                }))
            })
            .ok()
            .map(|it| it.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default();
    let recent_spend: Vec<serde_json::Value> = conn
        .prepare(
            "SELECT category, amount_jpy, reason, created_at
             FROM catalog_spend ORDER BY id DESC LIMIT 20",
        )
        .ok()
        .and_then(|mut s| {
            s.query_map([], |r| {
                Ok(serde_json::json!({
                    "category":   r.get::<_, String>(0)?,
                    "amount_jpy": r.get::<_, i64>(1)?,
                    "reason":     r.get::<_, Option<String>>(2)?,
                    "created_at": r.get::<_, String>(3)?,
                }))
            })
            .ok()
            .map(|it| it.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default();

    // ── Profit math (very rough) ────────────────────────────────────
    // Revenue = sum of catalog_orders.amount_jpy where status='submitted'
    //          (status='submitted' = Stripe paid + Printful accepted the
    //          order; failures don't generate revenue).
    // Cost estimate per garment: 50% of retail (Printful COGS + shipping
    //          + Stripe fee combined). This is a conservative placeholder
    //          until we wire the real Printful price API per SKU.
    let revenue_jpy: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_jpy),0) FROM catalog_orders WHERE status='submitted'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let est_cogs_jpy: i64 = revenue_jpy / 2;
    let spend_by_cat: std::collections::HashMap<String, i64> = conn
        .prepare("SELECT category, SUM(amount_jpy) FROM catalog_spend GROUP BY category")
        .ok()
        .and_then(|mut s| {
            s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                .ok()
                .map(|it| it.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default();
    let ad_spend_jpy = spend_by_cat
        .get("ads_google")
        .copied()
        .unwrap_or(0)
        + spend_by_cat.get("ads_meta").copied().unwrap_or(0);
    let gen_spend_jpy = spend_by_cat.get("ai_image").copied().unwrap_or(0);
    let estimated_net_jpy = revenue_jpy - est_cogs_jpy - ad_spend_jpy - gen_spend_jpy;

    axum::Json(serde_json::json!({
        "budget": {
            "spent_jpy": spent,
            "spent_lifetime_jpy": spent_lifetime,
            "cap_jpy": BUDGET_TOTAL_JPY,
            "remaining_jpy": BUDGET_TOTAL_JPY - spent,
            "period": "calendar_month",
        },
        "skus": {
            "auto_generated": auto_skus,
            "total_active":   total_skus,
            "hard_cap":       SKU_HARD_CAP,
        },
        "orders": {
            "last_24h": orders_24h,
            "total":    orders_total,
        },
        "profit_estimate": {
            "revenue_jpy":   revenue_jpy,
            "cogs_est_jpy":  est_cogs_jpy,
            "ad_spend_jpy":  ad_spend_jpy,
            "gen_spend_jpy": gen_spend_jpy,
            "net_jpy":       estimated_net_jpy,
            "note":          "cogs_est_jpy = revenue × 50% (placeholder until per-SKU Printful pricing wired)",
        },
        "recent_jobs": recent_jobs,
        "recent_spend": recent_spend,
    }))
    .into_response()
}

// ─── Public storefront pages ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct ShopQuery {
    pub brand: Option<String>,
    pub page: Option<u32>,
    pub sort: Option<String>,
    pub kind: Option<String>,
    pub q: Option<String>,
}

const SHOP_PAGE_SIZE: i64 = 60;

/// kind チップ → SQL 条件断片。**ホワイトリスト式・ユーザー入力は混ぜない**。
/// kind_from_sku の優先順位を SQL で完全再現すると脆い (例: "TEE" が
/// "RASHGUARD" に誤マッチ) ので、曖昧さのない category のみ提供する。
/// 返り値が空文字なら「絞り込みなし」。
fn shop_kind_sql(kind: &str) -> &'static str {
    match kind {
        "rashguard" => "(UPPER(sku) LIKE '%RASHGUARD%' OR UPPER(sku) LIKE '%-RASH%')",
        "hoodie" => "(UPPER(sku) LIKE '%HOODIE%' OR UPPER(sku) LIKE '%CREWNECK%' OR UPPER(sku) LIKE '%-HOOD%' OR UPPER(sku) LIKE '%-CREW%')",
        "sticker" => "(UPPER(sku) LIKE '%STICKER%')",
        "song" => "(COALESCE(meta_json,'') LIKE '%audio_url%' OR UPPER(sku) LIKE '%-SONG%')",
        _ => "",
    }
}

/// `q` 検索語を LIKE パターン化 (ESCAPE '\\' 前提)。`%` `_` `\` をエスケープし、
/// 長さ上限でクランプ。bind パラメータとして渡すので SQL インジェクションは不可。
fn shop_q_pattern(q: &str) -> Option<String> {
    let t = q.trim();
    if t.is_empty() {
        return None;
    }
    let t: String = t.chars().take(60).collect();
    let esc = t.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    Some(format!("%{}%", esc))
}

/// 共通 WHERE 句 (先頭の "WHERE" は含まない) と bind 値を組み立てる。
/// count クエリと list_products_paged で同じ絞り込みを使うための単一ソース。
fn shop_filter_sql(brand: Option<&str>, kind_sql: &str, q_pat: Option<&str>) -> (String, Vec<String>) {
    let mut parts = vec!["is_active=1".to_string()];
    let mut binds: Vec<String> = Vec::new();
    if let Some(b) = brand {
        parts.push("brand=?".to_string());
        binds.push(b.to_string());
    }
    if !kind_sql.is_empty() {
        parts.push(kind_sql.to_string());
    }
    if let Some(p) = q_pat {
        parts.push("(description_ja LIKE ? ESCAPE '\\' OR sku LIKE ? ESCAPE '\\')".to_string());
        binds.push(p.to_string());
        binds.push(p.to_string());
    }
    (parts.join(" AND "), binds)
}

pub async fn shop_index(
    State(db): State<Db>,
    Query(q): Query<ShopQuery>,
) -> Html<String> {
    let brand_filter = q.brand.unwrap_or_default();
    let page = q.page.unwrap_or(1).max(1);
    // Sort: whitelist only — anything else falls back to the default
    // (mockup-first → sold count) so the param can never reach SQL raw.
    let sort = match q.sort.as_deref() {
        Some(s @ ("new" | "price_asc" | "price_desc")) => s,
        _ => "",
    };
    // kind / q 絞り込み: kind はホワイトリスト、q は bind + LIKE エスケープ。
    let kind = match q.kind.as_deref() {
        Some(k @ ("rashguard" | "hoodie" | "sticker" | "song")) => k,
        _ => "",
    };
    let kind_sql = shop_kind_sql(kind);
    let q_text = q.q.clone().unwrap_or_default();
    let q_pat = shop_q_pattern(&q_text);
    let offset = (page as i64 - 1) * SHOP_PAGE_SIZE;
    let brand_opt = if brand_filter.is_empty() { None } else { Some(brand_filter.as_str()) };
    let (brands, items, total_active) = {
        let conn = db.lock().unwrap();
        let brands: Vec<(String, String, String)> = conn
            .prepare(
                "SELECT slug, name, COALESCE(emoji,'') FROM catalog_brands
                 WHERE is_active=1 AND EXISTS (
                   SELECT 1 FROM catalog_products p
                   WHERE p.brand=catalog_brands.slug AND p.is_active=1
                 )
                 ORDER BY slug",
            )
            .ok()
            .and_then(|mut s| {
                s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                    .ok()
                    .map(|it| it.filter_map(|r| r.ok()).collect())
            })
            .unwrap_or_default();

        // count + list は同じ絞り込み (brand + kind + q) を共有する。
        let (where_sql, binds) = shop_filter_sql(brand_opt, kind_sql, q_pat.as_deref());
        let count_sql = format!("SELECT COUNT(*) FROM catalog_products WHERE {}", where_sql);
        let total: i64 = conn
            .query_row(
                &count_sql,
                rusqlite::params_from_iter(binds.iter()),
                |r| r.get(0),
            )
            .unwrap_or(0);

        let items = list_products_paged(&conn, brand_opt, SHOP_PAGE_SIZE, offset, sort, kind_sql, q_pat.as_deref());
        (brands, items, total)
    };

    // 全チップ/フォームが共有する URL ビルダ。選択中の brand/sort/kind/q を
    // 引数で上書きしつつ他は維持する。page は絞り込み変更で常に 1 に戻す。
    let q_trim: String = q_text.trim().chars().take(60).collect();
    let shop_url = |b: &str, srt: &str, knd: &str, query: &str| -> String {
        let mut u = String::from("/shop");
        let mut params: Vec<String> = Vec::new();
        if !b.is_empty() { params.push(format!("brand={}", urlencoding::encode(b))); }
        if !srt.is_empty() { params.push(format!("sort={}", srt)); }
        if !knd.is_empty() { params.push(format!("kind={}", knd)); }
        if !query.is_empty() { params.push(format!("q={}", urlencoding::encode(query))); }
        if !params.is_empty() { u.push('?'); u.push_str(&params.join("&")); }
        u
    };

    let brand_chips = {
        let mut s = String::new();
        s.push_str(&format!(
            r#"<a class="chip{}" href="{}">すべて</a>"#,
            if brand_filter.is_empty() { " on" } else { "" },
            html_attr(&shop_url("", sort, kind, &q_trim)),
        ));
        for (slug, name, emoji) in &brands {
            let on = if &brand_filter == slug { " on" } else { "" };
            s.push_str(&format!(
                r#"<a class="chip{on}" href="{href}">{emoji} {name}</a>"#,
                on = on,
                href = html_attr(&shop_url(slug, sort, kind, &q_trim)),
                emoji = html_text(emoji),
                name = html_text(name),
            ));
        }
        s
    };

    // 種類チップ: ラッシュガード / パーカー・クルー / ステッカー / 曲。brand+sort+q を維持し
    // トグル動作 (選択中をもう一度押すと解除)。曖昧さのある「Tシャツ」は既定で出るので置かない。
    let kind_chips = {
        let mut s = format!(
            r#"<a class="chip{}" href="{}" data-funnel="cta_click" data-funnel-cta="shop_kind_all">すべての種類</a>"#,
            if kind.is_empty() { " on" } else { "" },
            html_attr(&shop_url(&brand_filter, sort, "", &q_trim)),
        );
        for (key, label) in [("rashguard", "🥋 ラッシュガード"), ("hoodie", "🧥 パーカー・クルー"), ("sticker", "✦ ステッカー"), ("song", "🎵 曲")] {
            let on = if kind == key { " on" } else { "" };
            let toggle = if kind == key { "" } else { key }; // 選択中なら解除
            s.push_str(&format!(
                r#"<a class="chip{on}" href="{href}" data-funnel="cta_click" data-funnel-cta="shop_kind_{key}">{label}</a>"#,
                on = on, href = html_attr(&shop_url(&brand_filter, sort, toggle, &q_trim)), key = key, label = label,
            ));
        }
        s
    };

    // 検索フォーム: GET /shop。brand/sort/kind を hidden で保持して検索後も絞り込み維持。
    let search_form = format!(
        r##"<form class="shopsearch" method="get" action="/shop" role="search">
<input type="hidden" name="brand" value="{b}"><input type="hidden" name="sort" value="{s}"><input type="hidden" name="kind" value="{k}">
<input type="search" name="q" value="{q}" placeholder="検索 — darce / coffee / 黒帯 …" aria-label="商品検索" data-funnel="cta_click" data-funnel-cta="shop_search">
<button type="submit" aria-label="検索">検索</button>{clear}</form>"##,
        b = html_attr(&brand_filter), s = html_attr(sort), k = html_attr(kind), q = html_attr(&q_trim),
        clear = if q_trim.is_empty() { String::new() } else {
            format!(r#"<a class="clearq" href="{}">クリア</a>"#, html_attr(&shop_url(&brand_filter, sort, kind, "")))
        },
    );

    // Sort chips: 人気順(default) / 新着 / 価格↑ / 価格↓. brand/kind/q persist, page resets.
    let sort_chips = {
        [("", "人気順"), ("new", "新着"), ("price_asc", "価格が安い順"), ("price_desc", "価格が高い順")]
            .iter()
            .map(|(key, label)| {
                let on = if sort == *key { " on" } else { "" };
                format!(
                    r#"<a class="chip{on}" href="{href}" data-funnel="cta_click" data-funnel-cta="shop_sort_{k}">{label}</a>"#,
                    on = on, href = html_attr(&shop_url(&brand_filter, key, kind, &q_trim)),
                    k = if key.is_empty() { "sold" } else { key }, label = label,
                )
            })
            .collect::<String>()
    };

    let grid = items
        .iter()
        .enumerate()
        .map(|(i, p)| render_card(p, i))
        .collect::<String>();

    let page_count = items.len();
    let total_pages = ((total_active as f64) / (SHOP_PAGE_SIZE as f64)).ceil() as u32;
    // SEO: keyword-bearing title/description. Brand pages use the display name
    // (not the slug) when we have it. Page 2+ gets a suffix so paginated pages
    // don't present as duplicate titles in Search Console.
    let brand_name = brands
        .iter()
        .find(|(slug, _, _)| slug == &brand_filter)
        .map(|(_, name, _)| name.clone())
        .unwrap_or_else(|| brand_filter.clone());
    let mut title = if brand_filter.is_empty() {
        format!("MU SHOP — コラボTシャツ・柔術ウェア・限定グッズ通販 ({} 件)", total_active)
    } else {
        format!("{} × MU コラボ商品一覧 ({}件) | MU SHOP", brand_name, total_active)
    };
    if !q_trim.is_empty() {
        title = format!("「{}」の検索結果 ({}件) | MU SHOP", q_trim, total_active);
    }
    if page > 1 {
        title.push_str(&format!(" — Page {}", page));
    }
    // 検索結果は薄い/重複ページなので noindex,follow (リンクは辿らせる)。
    // kind フィルタはファセットなので canonical を親 (brand/全件) に向ける既存挙動で吸収。
    let robots_meta = if q_pat.is_some() {
        r#"<meta name="robots" content="noindex,follow">"#
    } else {
        ""
    };
    let meta_desc = if brand_filter.is_empty() {
        format!("MUと10+ブランドのコラボアパレル公式通販 {total}件。AIデザインTシャツ・柔術/BJJラッシュガード・ステッカー・着ると鳴る音楽T。1着から受注生産・完売廃棄ゼロ・Stripe決済・国際発送7-14日。", total = total_active)
    } else {
        format!("{name} × MU のコラボ商品 {n}件。1着から受注生産・完売廃棄ゼロ・Stripe安全決済・国際発送7-14日。", name = brand_name, n = total_active)
    };
    // canonical drops ?sort= — sorted views are duplicates of the same list.
    // brand + page survive (each is distinct content).
    let canonical = {
        let mut u = String::from("https://wearmu.com/shop");
        let mut sep = '?';
        if !brand_filter.is_empty() {
            u.push(sep);
            u.push_str(&format!("brand={}", urlencoding::encode(&brand_filter)));
            sep = '&';
        }
        if page > 1 {
            u.push(sep);
            u.push_str(&format!("page={}", page));
        }
        u
    };
    let og_image = items
        .first()
        .and_then(|p| p.img.clone())
        .filter(|s| !s.is_empty())
        .map(|s| if s.starts_with("http") { s } else { format!("https://merch.wearmu.com{}", s) })
        .unwrap_or_else(|| "https://wearmu.com/static/og-default.png".to_string());
    // CollectionPage + ItemList (top 24 of this page) — category-level
    // structured data; per-product Product JSON-LD lives on each PDP.
    let ld_items = items
        .iter()
        .take(24)
        .enumerate()
        .map(|(i, p)| {
            format!(
                r#"{{"@type":"ListItem","position":{pos},"url":"https://wearmu.com/shop/{sku}"}}"#,
                pos = i + 1,
                sku = urlencoding::encode(&p.sku),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let ld_json = format!(
        r#"{{"@context":"https://schema.org","@type":"CollectionPage","name":"{name}","url":"{url}","mainEntity":{{"@type":"ItemList","numberOfItems":{n},"itemListElement":[{items}]}}}}"#,
        name = title.replace('"', ""),
        url = canonical,
        n = total_active,
        items = ld_items,
    );

    // Pagination: prev / page-of-pages / next. brand+sort+kind+q persist.
    let mut bq = String::new();
    if !brand_filter.is_empty() { bq.push_str(&format!("&brand={}", urlencoding::encode(&brand_filter))); }
    if !sort.is_empty() { bq.push_str(&format!("&sort={}", sort)); }
    if !kind.is_empty() { bq.push_str(&format!("&kind={}", kind)); }
    if !q_trim.is_empty() { bq.push_str(&format!("&q={}", urlencoding::encode(&q_trim))); }
    let prev_link = if page > 1 {
        format!(r#"<a class="pg-link" href="/shop?page={}{}">← 前 {} 件</a>"#,
            page - 1, bq, SHOP_PAGE_SIZE)
    } else {
        r#"<span class="pg-link off">← 前</span>"#.to_string()
    };
    let next_link = if (page as i64) < total_pages as i64 {
        format!(r#"<a class="pg-link" href="/shop?page={}{}">次 {} 件 →</a>"#,
            page + 1, bq, SHOP_PAGE_SIZE)
    } else {
        r#"<span class="pg-link off">次 →</span>"#.to_string()
    };
    let pagination_html = if total_pages > 1 {
        format!(
            r#"<div class="pagination">{prev} <span class="pg-count">page {page} / {total} (全 {tot} 件中 {start}-{end})</span> {next}</div>"#,
            prev = prev_link, next = next_link,
            page = page, total = total_pages, tot = total_active,
            start = offset + 1,
            end = (offset + page_count as i64).min(total_active),
        )
    } else {
        String::new()
    };
    let hero_html = if brand_filter == "shiopixel" {
        r##"<div class="hero">
  <h1>🎵 Shiopixel — 着ると、鳴る。</h1>
  <p>BJJと日常のうた。一着＝一曲。胸の ○ にスマホをかざすと、その曲が鳴る。<br>各カードの ▶ で今すぐ試聴 — 気に入った曲を、着られる。音は Arweave に永久保存。</p>
  <div class="trust">
    <span><strong>▶ 試聴</strong> 買う前に聴ける</span>
    <span><strong>1 着から</strong> 受注生産・廃棄ゼロ</span>
    <span><strong>○ のQR</strong> 着ると曲が鳴る</span>
    <span><strong>Stripe</strong> 安全決済</span>
  </div>
</div>"##.to_string()
    } else {
        format!(r##"<div class="hero">
  <h1>━◯━ 知ってる人にだけ届く wearable.</h1>
  <p>柔術・コーヒー・地域 ── 10+ コラボの "内側からの服"。 受注生産 — 1 着から、 完売・廃棄ゼロ。 <strong style="color:#ffd700">{total} 件</strong> 公開中。</p>
  <div class="trust">
    <span><strong>国際発送</strong> 7-14 日 (DHL / FedEx)</span>
    <span><strong>1 着から</strong> オーダー可</span>
    <span><strong>Bella+Canvas / AOP rashguard</strong> 等プレミアム生地</span>
    <span><strong>Stripe</strong> 安全決済 + クーポン対応</span>
  </div>
</div>"##, total = total_active)
    };
    let body = format!(
        r##"<!doctype html><html lang="ja"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<meta name="description" content="{meta_desc}">
<link rel="canonical" href="{canonical}">{robots_meta}
<meta property="og:type" content="website">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{meta_desc}">
<meta property="og:url" content="{canonical}">
<meta property="og:image" content="{og_image}">
<meta property="og:site_name" content="wearmu.com">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="{title}">
<meta name="twitter:image" content="{og_image}">
<script type="application/ld+json">{ld_json}</script>
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
body{{background:#0a0a0a;color:#f5f5f0;font-family:'Helvetica Neue','Hiragino Sans',Arial,sans-serif;line-height:1.55;font-size:14px}}
nav{{padding:16px 24px;border-bottom:1px solid rgba(255,255,255,0.08);display:flex;justify-content:space-between;align-items:center;flex-wrap:wrap;gap:10px}}
nav a{{color:#f5f5f0;text-decoration:none;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.85}}
nav a:hover{{opacity:1}}
nav .brand{{font-weight:900;letter-spacing:0.4em}}
.hero{{padding:40px 24px 18px;max-width:1180px;margin:0 auto}}
.hero h1{{font-size:28px;font-weight:900;letter-spacing:-0.01em;margin-bottom:8px}}
.hero p{{color:rgba(245,245,240,0.62);font-size:13px;line-height:1.85;max-width:640px;margin-bottom:14px}}
.trust{{display:flex;flex-wrap:wrap;gap:8px 16px;font-size:11px;color:rgba(245,245,240,0.72);padding-top:8px;border-top:1px solid rgba(255,255,255,0.06)}}
.trust span{{display:inline-flex;align-items:center;gap:5px}}
.trust span:before{{content:"✓";color:#ffd700;font-weight:700;font-size:13px}}
.trust strong{{color:#fff;font-weight:600}}
.chips{{padding:8px 24px 18px;max-width:1180px;margin:0 auto;display:flex;flex-wrap:wrap;gap:6px}}
.shopsearch{{max-width:1180px;margin:0 auto;padding:4px 24px 10px;display:flex;gap:8px;align-items:center}}
.shopsearch input[type=search]{{flex:1;min-width:0;background:#111;border:1px solid rgba(255,255,255,0.18);border-radius:999px;color:#f5f5f0;padding:10px 16px;font-size:13px}}
.shopsearch input[type=search]:focus{{outline:none;border-color:#ffd700}}
.shopsearch button{{background:#ffd700;color:#000;border:none;border-radius:999px;padding:10px 18px;font-size:12px;font-weight:700;cursor:pointer;letter-spacing:.05em}}
.shopsearch .clearq{{color:rgba(245,245,240,0.6);font-size:11px;text-decoration:none;white-space:nowrap}}
.shopsearch .clearq:hover{{color:#ffd700}}
.chip{{display:inline-block;padding:6px 12px;border:1px solid rgba(255,255,255,0.18);border-radius:999px;color:#f5f5f0;text-decoration:none;font-size:11px;letter-spacing:0.05em;background:rgba(255,255,255,0.02)}}
.chip:hover{{border-color:#ffd700;color:#ffd700}}
.chip.on{{background:#ffd700;color:#000;border-color:#ffd700}}
.grid{{display:grid;grid-template-columns:repeat(auto-fill,minmax(220px,1fr));gap:14px;padding:8px 24px 80px;max-width:1180px;margin:0 auto}}
.card{{background:#111;border:1px solid rgba(255,255,255,0.06);border-radius:6px;overflow:hidden;text-decoration:none;color:inherit;display:flex;flex-direction:column;transition:border-color 0.15s}}
.card:hover{{border-color:rgba(255,215,0,0.4)}}
.card .img{{aspect-ratio:1/1;background:#000;display:block;overflow:hidden}}
.card .img img{{width:100%;height:100%;object-fit:cover;display:block}}
.card .body{{padding:10px 12px 12px;flex:1;display:flex;flex-direction:column;gap:6px}}
.card .body .brand{{font-size:9px;letter-spacing:0.25em;text-transform:uppercase;color:#ffd700;opacity:0.85}}
.card .body .name{{font-size:12.5px;line-height:1.45;flex:1;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden}}
.card .body .price{{font-size:13px;font-weight:700;color:#fff;font-family:monospace}}
.empty{{padding:60px 24px;text-align:center;color:rgba(245,245,240,0.5);max-width:1180px;margin:0 auto}}
.pagination{{max-width:1180px;margin:0 auto;padding:14px 24px 40px;display:flex;justify-content:space-between;align-items:center;gap:12px;flex-wrap:wrap;font-size:12px}}
.pg-link{{color:#ffd700;text-decoration:none;padding:8px 14px;border:1px solid rgba(255,215,0,0.4);border-radius:999px;font-size:11px;letter-spacing:0.05em}}
.pg-link:hover{{background:rgba(255,215,0,0.08)}}
.pg-link.off{{color:#444;border-color:rgba(255,255,255,0.06);cursor:not-allowed}}
.pg-count{{color:rgba(245,245,240,0.5);font-size:11px;font-family:monospace}}
footer{{padding:30px 24px 50px;border-top:1px solid rgba(255,255,255,0.06);text-align:center;color:rgba(245,245,240,0.5);font-size:10px;letter-spacing:0.15em}}
footer a{{color:rgba(245,245,240,0.7);text-decoration:none;margin:0 8px}}
.cardplay{{position:absolute;top:8px;right:8px;z-index:2;width:38px;height:38px;border-radius:50%;border:1px solid rgba(255,215,0,.8);background:rgba(0,0,0,.66);color:#fff;font-size:13px;cursor:pointer;backdrop-filter:blur(4px)}}
.cardplay:hover{{background:rgba(0,0,0,.85)}}
/* モバイル: 20+個のブランドチップが折り返してファーストビューを商品ゼロにする
   「チップの壁」対策 — 1行横スクロール化して商品グリッドを1画面目に出す。 */
@media (max-width:740px){{
  .chips{{flex-wrap:nowrap;overflow-x:auto;-webkit-overflow-scrolling:touch;scrollbar-width:none;padding-bottom:10px}}
  .chips::-webkit-scrollbar{{display:none}}
  .chip{{flex:0 0 auto}}
  .hero{{padding-top:24px}}
}}
</style></head><body>
<nav>
  <a class="brand" href="/">MU</a>
  <div>
    <a href="/shop">SHOP</a>
    <a href="/buy" style="margin-left:14px">DROPS</a>
    <a href="/heritage" style="margin-left:14px">HERITAGE</a>
  </div>
</nav>
{hero}
{make_cta}
{search_form}
<div class="chips">{brand_chips}</div>
<div class="chips kinds" style="padding-top:0">{kind_chips}</div>
<div class="chips sorts" style="padding-top:0">{sort_chips}</div>
{body_or_empty}
{pagination}
<footer>
  <span>© 2026 MU / Enabler Inc.</span>
  <a href="/shipping">配送</a>
  <a href="/returns">返品</a>
  <a href="/faq">FAQ</a>
  <a href="/privacy">プライバシー</a>
  <a href="/heritage">heritage</a>
  <a href="/buy">drops</a>
  <a href="https://yukihamada.jp/community">🔥 ともしび</a>
  <a href="mailto:info@enablerdao.com">CONTACT</a>
</footer>
<script>
  // 一覧の▶試聴: カードのリンク遷移を止めてArweave音源を再生(涼介FB: 聴き比べ)
  window.muSRC={{
    "everybody-say-bjj":"https://gateway.irys.xyz/5jsmQoNNekanEGMBUEhSLoZyxGXSDZL5taMZfwwrEC1c/everybody-say-bjj.mp3",
    "shio-to-pixel":"https://gateway.irys.xyz/5jsmQoNNekanEGMBUEhSLoZyxGXSDZL5taMZfwwrEC1c/shio-to-pixel.mp3",
    "musubinaosu-asa":"https://gateway.irys.xyz/5jsmQoNNekanEGMBUEhSLoZyxGXSDZL5taMZfwwrEC1c/musubinaosu-asa.mp3",
    "hello-2150":"https://gateway.irys.xyz/5jsmQoNNekanEGMBUEhSLoZyxGXSDZL5taMZfwwrEC1c/hello-2150.mp3",
    "i-love-you":"https://gateway.irys.xyz/5jsmQoNNekanEGMBUEhSLoZyxGXSDZL5taMZfwwrEC1c/i-love-you.mp3",
    "i-need-your-attention":"https://gateway.irys.xyz/5jsmQoNNekanEGMBUEhSLoZyxGXSDZL5taMZfwwrEC1c/i-need-your-attention.mp3",
    "free-to-change":"https://gateway.irys.xyz/5jsmQoNNekanEGMBUEhSLoZyxGXSDZL5taMZfwwrEC1c/free-to-change.mp3",
    "attention-kudasai":"https://gateway.irys.xyz/5jsmQoNNekanEGMBUEhSLoZyxGXSDZL5taMZfwwrEC1c/attention-kudasai.mp3"
  }};
  window.muAudio=null; window.muBtn=null;
  window.muPlay=function(e,btn){{
    e.preventDefault(); e.stopPropagation();
    var key=btn.getAttribute('data-key'); var src=btn.getAttribute('data-src')||window.muSRC[key];
    if(!src){{window.open('https://mu.koe.live/oto.html?s='+key,'_blank');return;}}
    if(window.muBtn===btn && window.muAudio && !window.muAudio.paused){{window.muAudio.pause();btn.textContent='▶';return;}}
    if(window.muBtn && window.muBtn!==btn) window.muBtn.textContent='▶';
    if(!window.muAudio) window.muAudio=new Audio();
    window.muAudio.src=src; window.muAudio.play(); btn.textContent='❚❚'; window.muBtn=btn;
    window.muAudio.onended=function(){{btn.textContent='▶';}};
  }};
</script>
<script defer src="/mu-funnel.js"></script>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
</body></html>"##,
        title = html_text(&title),
        meta_desc = html_attr(&meta_desc),
        canonical = html_attr(&canonical),
        robots_meta = robots_meta,
        og_image = html_attr(&og_image),
        ld_json = ld_json,
        hero = hero_html,
        make_cta = make_cta_banner("shop"),
        search_form = search_form,
        brand_chips = brand_chips,
        kind_chips = kind_chips,
        sort_chips = sort_chips,
        body_or_empty = if items.is_empty() {
            format!(
                r#"<div class="empty">「{}」に一致する商品が見つかりませんでした。<br><a href="/shop" style="color:#ffd700">すべての商品を見る →</a></div>"#,
                html_text(if !q_trim.is_empty() { &q_trim } else { "この条件" })
            )
        } else {
            format!(r#"<div class="grid">{}</div>"#, grid)
        },
        pagination = pagination_html,
    );
    Html(body)
}

/// Minimum real sold count before a "X 着 販売" social-proof badge is shown.
/// Gated so a low-volume SKU never surfaces an embarrassing 0/1; the badge
/// self-activates once a SKU genuinely crosses the threshold. Honest data only
/// (derived from catalog_orders.status='submitted'), never fabricated.
const SOLD_BADGE_MIN: i64 = 3;

/// Read a SKU's `edition_size` (limited run size) from meta_json. 0 = not a
/// limited edition. Single source of truth for both the checkout sold-out
/// gate and the public serial registry.
fn edition_size_of(meta_json: &Option<String>) -> i64 {
    meta_json
        .as_deref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|v| v.get("edition_size").and_then(|c| c.as_i64()))
        .filter(|c| *c > 0)
        .unwrap_or(0)
}

/// Paid units of a SKU = orders that reached 'submitted' (the serial count).
fn edition_sold(conn: &rusqlite::Connection, sku: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM catalog_orders WHERE sku=? AND status='submitted'",
        rusqlite::params![sku],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

/// GET /edition/:sku — public serial registry / authenticity surface for a
/// limited edition. Shows the run size, how many serials are claimed, what is
/// left, and which serial the next buyer receives. The serial IS the order's
/// ordinal within the SKU (#k / N) — derived, never a separate table, so it
/// can never drift from the real paid orders. PII (buyer names) is never shown.
pub async fn edition_page(State(db): State<Db>, Path(sku): Path<String>) -> Response {
    let row: Option<(String, Option<String>, String, i64)> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT label, meta_json,
                    COALESCE(mockup_url_external, mockup_main_file, ''), retail_price_jpy
             FROM catalog_products WHERE sku=?",
            rusqlite::params![&sku],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok()
    };
    let Some((label, meta_json, mockup, price)) = row else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let cap = edition_size_of(&meta_json);
    if cap <= 0 {
        return (StatusCode::NOT_FOUND, "not a limited edition").into_response();
    }
    let sold = {
        let conn = db.lock().unwrap();
        edition_sold(&conn, &sku)
    };
    let remaining = (cap - sold).max(0);
    let next = (sold + 1).min(cap);
    let img = if mockup.starts_with("http") {
        mockup.clone()
    } else if !mockup.is_empty() {
        format!("https://merch.wearmu.com{}", mockup)
    } else {
        String::new()
    };
    let img_html = if img.is_empty() {
        String::new()
    } else {
        format!(
            "<img src=\"{}\" alt=\"{}\" style=\"width:220px;height:220px;object-fit:contain;background:#111;border-radius:12px\">",
            html_text(&img),
            html_text(&label)
        )
    };
    let cta = if remaining > 0 {
        format!(
            "<a href=\"/shop/{sku}\" style=\"display:inline-block;background:#e6c449;color:#0a0a0a;\
             font-weight:700;padding:13px 26px;border-radius:999px;text-decoration:none\">\
             #{next} / {cap} を確保する — ¥{price}</a>",
            sku = html_text(&sku), next = next, cap = cap, price = price
        )
    } else {
        "<div style=\"color:#e6c449;letter-spacing:.2em;font-size:13px\">SOLD OUT — 完売</div>".to_string()
    };
    let pct = if cap > 0 { (sold * 100 / cap).min(100) } else { 0 };
    let body = format!(
        "<!doctype html><html lang=ja><meta charset=utf-8>\
         <meta name=viewport content=\"width=device-width,initial-scale=1\">\
         <title>{label} — シリアル台帳 #／{cap} · MU</title>\
         <meta name=robots content=index>\
         <body style=\"margin:0;background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,'Helvetica Neue',Arial,sans-serif\">\
         <div style=\"max-width:640px;margin:0 auto;padding:48px 24px;text-align:center\">\
         <a href=\"/universal\" style=\"color:#888;text-decoration:none;font-size:12px;letter-spacing:.3em\">━◯━ UNIVERSAL</a>\
         <div style=\"margin:28px 0 18px\">{img_html}</div>\
         <h1 style=\"font-weight:500;font-size:23px;margin:0 0 6px\">{label}</h1>\
         <div style=\"font-size:12px;letter-spacing:.3em;color:#e6c449;text-transform:uppercase;margin-bottom:24px\">Limited {cap} · Serial-numbered</div>\
         <div style=\"background:#141414;border:1px solid #222;border-radius:14px;padding:22px;margin-bottom:22px\">\
           <div style=\"display:flex;justify-content:space-between;font-size:13px;opacity:.7;margin-bottom:8px\">\
             <span>発行済み {sold} / {cap}</span><span>残り {remaining}</span></div>\
           <div style=\"height:8px;background:#222;border-radius:999px;overflow:hidden\">\
             <div style=\"height:100%;width:{pct}%;background:#e6c449\"></div></div>\
           <p style=\"font-size:12.5px;line-height:1.8;opacity:.62;margin:16px 0 0;text-align:left\">\
             この台帳は本物の支払い済み注文だけを数えます。1 枚ごとに通し番号 <b>#k / {cap}</b> が割り当てられ、{cap} 枚に達したら販売を締め切ります。番号は注文の並び順そのものなので、改ざんできません。</p>\
         </div>\
         <div style=\"margin:8px 0 26px\">{cta}</div>\
         <p style=\"font-size:11px;opacity:.4\">次に発行されるシリアル: #{next} / {cap}</p>\
         </div></body></html>",
        label = html_text(&label), cap = cap, sold = sold, remaining = remaining,
        pct = pct, next = next, img_html = img_html, cta = cta,
    );
    Html(body).into_response()
}

/// GET /universal — the UNIVERSAL collection sales page. Lists every live SKU
/// in the `universal` store together with its 5-axis universality score
/// (stored in meta_json.score), the 100-piece limited-edition framing, and a
/// live "残り N / 100" pulled from real paid orders. Buy buttons go to the
/// proven /shop/:sku checkout. Scores and remaining counts are read from the
/// DB — nothing is hard-coded — so the page tracks reality on every request.
pub async fn universal_collection(State(db): State<Db>) -> Response {
    struct Item {
        sku: String,
        label: String,
        img: String,
        price: i64,
        cap: i64,
        sold: i64,
        score: i64,
        axes: Vec<(String, i64)>,
        verdict: String,
    }
    let items: Vec<Item> = {
        let conn = db.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT sku, label, description_ja,
                    COALESCE(mockup_url_external, mockup_main_file, ''),
                    retail_price_jpy, meta_json
             FROM catalog_products
             WHERE brand='universal' AND status='live'",
        ) {
            Ok(s) => s,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db").into_response(),
        };
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<String>>(5)?,
                ))
            })
            .and_then(|m| m.collect::<Result<Vec<_>, _>>())
            .unwrap_or_default();
        let mut out: Vec<Item> = Vec::new();
        for (sku, label, _desc, mockup, price, meta_json) in rows {
            let cap = edition_size_of(&meta_json);
            let meta: serde_json::Value = meta_json
                .as_deref()
                .and_then(|m| serde_json::from_str(m).ok())
                .unwrap_or(serde_json::Value::Null);
            let s = &meta["score"];
            let score = s["total"].as_i64().unwrap_or(0);
            let verdict = s["verdict"].as_str().unwrap_or("").to_string();
            let labels = [
                ("時間", "time"),
                ("文化", "culture"),
                ("視覚", "visual"),
                ("身体", "body"),
                ("製造", "make"),
            ];
            let mut axes = Vec::new();
            for (ja, key) in labels {
                if let Some(v) = s["axes"][key].as_i64() {
                    axes.push((ja.to_string(), v));
                }
            }
            let sold = edition_sold(&conn, &sku);
            let img = if mockup.starts_with("http") {
                mockup
            } else if !mockup.is_empty() {
                format!("https://merch.wearmu.com{}", mockup)
            } else {
                String::new()
            };
            out.push(Item {
                sku, label, img, price, cap, sold, score, axes, verdict,
            });
        }
        out.sort_by(|a, b| b.score.cmp(&a.score));
        out
    };

    let count = items.len();
    let mut cards = String::new();
    for it in &items {
        let remaining = (it.cap - it.sold).max(0);
        let img_html = if it.img.is_empty() {
            "<div style=\"width:100%;aspect-ratio:1;background:#111;border-radius:12px\"></div>".to_string()
        } else {
            format!(
                "<img src=\"{}\" alt=\"{}\" loading=lazy style=\"width:100%;aspect-ratio:1;object-fit:contain;background:#111;border-radius:12px\">",
                html_text(&it.img), html_text(&it.label)
            )
        };
        let mut axes_html = String::new();
        for (ja, v) in &it.axes {
            axes_html.push_str(&format!(
                "<span style=\"display:inline-block;font-size:10.5px;color:#cfcfcf;background:#1c1c1c;\
                 border:1px solid #2a2a2a;border-radius:999px;padding:3px 8px;margin:2px\">{ja} {v}</span>",
                ja = html_text(ja), v = v
            ));
        }
        let cta = if remaining > 0 {
            format!(
                "<a href=\"/shop/{sku}\" style=\"display:block;text-align:center;background:#e6c449;color:#0a0a0a;\
                 font-weight:700;padding:11px;border-radius:999px;text-decoration:none;margin-top:12px\">\
                 #{next} / {cap} を確保 — ¥{price}</a>",
                sku = html_text(&it.sku), next = (it.sold + 1).min(it.cap), cap = it.cap, price = it.price
            )
        } else {
            "<div style=\"text-align:center;color:#888;padding:11px;margin-top:12px\">SOLD OUT</div>".to_string()
        };
        cards.push_str(&format!(
            "<div style=\"background:#121212;border:1px solid #222;border-radius:16px;padding:16px\">\
             {img_html}\
             <div style=\"display:flex;justify-content:space-between;align-items:baseline;margin:14px 0 4px\">\
               <h3 style=\"font-weight:500;font-size:16px;margin:0\">{label}</h3>\
               <span style=\"font-size:20px;font-weight:800;color:#e6c449\">{score}<span style=\"font-size:11px;opacity:.6\">/100</span></span></div>\
             <p style=\"font-size:12px;line-height:1.7;opacity:.6;margin:0 0 10px;min-height:34px\">{verdict}</p>\
             <div style=\"margin:0 -2px\">{axes_html}</div>\
             <div style=\"font-size:11px;opacity:.55;margin-top:10px\">限定 {cap} 枚 · シリアル付き · <a href=\"/edition/{sku}\" style=\"color:#e6c449;text-decoration:none\">残り {remaining} →</a></div>\
             {cta}</div>",
            img_html = img_html, label = html_text(&it.label), score = it.score,
            verdict = html_text(&it.verdict), axes_html = axes_html,
            cap = it.cap, sku = html_text(&it.sku), remaining = remaining, cta = cta,
        ));
    }
    let empty = if count == 0 {
        "<p style=\"text-align:center;opacity:.5;padding:40px\">準備中です。まもなく公開します。</p>".to_string()
    } else {
        String::new()
    };
    let body = format!(
        "<!doctype html><html lang=ja><meta charset=utf-8>\
         <meta name=viewport content=\"width=device-width,initial-scale=1\">\
         <title>UNIVERSAL — 10年後も着られる、{count}枚限定の普遍デザイン · MU</title>\
         <meta name=description content=\"普遍性5軸で95点以上だけを選んだ、各100枚限定・シリアル付きの線画Tシャツコレクション。\">\
         <body style=\"margin:0;background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,'Helvetica Neue',Arial,sans-serif\">\
         <div style=\"max-width:1080px;margin:0 auto;padding:56px 22px\">\
         <div style=\"text-align:center;margin-bottom:14px;font-size:12px;letter-spacing:.5em;opacity:.8\">━◯━ MU</div>\
         <h1 style=\"text-align:center;font-weight:300;font-size:34px;letter-spacing:.04em;margin:0 0 14px\">UNIVERSAL</h1>\
         <p style=\"text-align:center;max-width:620px;margin:0 auto 10px;font-size:14px;line-height:1.9;opacity:.72\">\
           流行も言葉も超える、原型だけの線画。<b>10年後も価値があり、1年後に着ても新鮮で、3年後に必ず効く</b>——その普遍性を 5 軸 100 点で採点し、<b>95 点以上だけ</b>を選びました。各デザインは <b>100 枚限定・通し番号付き</b>。</p>\
         <p style=\"text-align:center;font-size:11.5px;opacity:.45;margin:0 0 36px\">採点軸: 時間普遍性 / 文化普遍性 / 視覚普遍性 / 身体普遍性 / 製造普遍性（各20点）</p>\
         {empty}\
         <div style=\"display:grid;grid-template-columns:repeat(auto-fill,minmax(240px,1fr));gap:18px\">{cards}</div>\
         <div style=\"text-align:center;margin:52px auto 0;max-width:560px;padding:34px 28px;border:1px solid #222;border-radius:18px;background:#0f0f0f\">\
           <div style=\"font-size:12px;letter-spacing:.3em;color:#e6c449;text-transform:uppercase;margin-bottom:10px\">MU MAKE</div>\
           <div style=\"font-size:20px;font-weight:500;margin-bottom:8px\">ぴったりが無ければ、自分で作る。</div>\
           <p style=\"font-size:13px;line-height:1.8;opacity:.62;margin:0 0 20px\">言葉を打つだけ。MU が、あなただけの一着を作る。気に入ったら、それも100枚限定・シリアル付きに。</p>\
           <a href=\"/make\" style=\"display:inline-block;background:#e6c449;color:#0a0a0a;font-weight:700;padding:14px 30px;border-radius:999px;text-decoration:none\">自分で作る → MU MAKE</a>\
         </div>\
         <p style=\"text-align:center;font-size:11px;opacity:.4;margin-top:40px\">受注生産 · 完売したら二度と刷りません · 点数と残数はこのページで常時実数表示</p>\
         </div></body></html>",
        count = count, empty = empty, cards = cards,
    );
    Html(body).into_response()
}

pub async fn shop_pdp(
    State(db): State<Db>,
    Path(sku): Path<String>,
) -> Response {
    let row = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT sku, brand, label, description_ja, retail_price_jpy,
                    mockup_main_file, mockup_url_external, suzuri_url, stripe_price_id, meta_json
             FROM catalog_products WHERE sku=? AND is_active=1",
            rusqlite::params![&sku],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    r.get::<_, Option<String>>(8)?,
                    r.get::<_, Option<String>>(9)?,
                ))
            },
        )
        .ok()
    };
    let Some((sku, brand, label, desc, price_jpy, mockup_main, mockup_ext, suzuri, price_id, meta_json)) = row
    else {
        return (StatusCode::NOT_FOUND, "product not found").into_response();
    };

    // 時限ドロップ(封印): meta_json.unlock_iso が立ち、description_ja が age 暗号文なら、
    // 解禁時刻まで中身を誰も(運営も)読めない。解禁後にブラウザ内(drand tlock)で復号表示。
    // スキーマ非変更(meta_json活用・CATALOG_CONTRACT 準拠)。通常商品は一切影響なし。
    let unlock_iso = meta_json
        .as_deref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|v| v.get("unlock_iso").and_then(|x| x.as_str()).map(|s| s.to_string()));
    let is_sealed = unlock_iso.is_some() && desc.contains("BEGIN AGE ENCRYPTED FILE");
    // 公開タイトル: 封印中は label(公開名)を使う。desc は暗号文なので表に出さない。
    let display_name = if is_sealed {
        if !label.is_empty() { label.clone() } else { "MU 封印ドロップ".to_string() }
    } else {
        desc.clone()
    };
    let meta_desc = if is_sealed {
        format!("🔒 このドロップは {} に解禁されます", unlock_iso.as_deref().unwrap_or(""))
    } else {
        desc.clone()
    };
    // SEO: <title>/og:title は60字、meta description は120字で切る。
    // 自動生成 desc 全文をそのまま title に流すと検索結果で尻切れ+キーワード密度が
    // 死ぬ。h1 とページ本文は全文のまま(中身は削らない)。char 境界で安全に切る。
    let trim_chars = |s: &str, max: usize| -> String {
        let chars: Vec<char> = s.chars().collect();
        if chars.len() > max {
            format!("{}…", chars[..max - 1].iter().collect::<String>().trim_end())
        } else {
            s.to_string()
        }
    };
    let short_title = trim_chars(&display_name, 60);
    let meta_desc_short = trim_chars(&meta_desc, 120);
    // 見出し/タグライン分割: 自動生成商品は「商品名 — 宣伝文。」と一文になりがちで、
    // H1 に長文が入りレイアウトが崩れる。em-dash(—/―/--) で割り、前を見出し・後をタグラインに。
    // 区切りが無ければ従来どおり全文を見出しに(=挙動非変更)。封印中は分割しない。
    let (headline, tagline) = {
        let mut split = None;
        for sep in ["—", "―", " - ", "ー ", "│"] {
            if let Some((h, t)) = display_name.split_once(sep) {
                if h.trim().chars().count() >= 1 && t.trim().chars().count() >= 4 {
                    split = Some((h.trim().to_string(), t.trim().to_string()));
                    break;
                }
            }
        }
        match (is_sealed, split) {
            (false, Some((h, t))) => (h, t),
            _ => (display_name.clone(), String::new()),
        }
    };
    let tagline_html = if tagline.is_empty() {
        String::new()
    } else {
        format!("<p class=\"tagline\">{}</p>", html_text(&tagline))
    };
    let sealed_block = if is_sealed {
        let u = html_text(unlock_iso.as_deref().unwrap_or(""));
        let ct = html_text(&desc); // 暗号文を隠し要素の textContent に(復号はJS側)
        let u_js = serde_json::to_string(unlock_iso.as_deref().unwrap_or(""))
            .unwrap_or_else(|_| "\"\"".to_string());
        format!(
            r##"<div class="spec" id="mu-sealed"><h3>🔒 SEALED DROP</h3>
<p id="mu-seal-msg">このドロップの中身は <b>{u}</b> まで封印されています。解禁時刻になると、このページで自動的に表示されます。運営も時刻前には開けません（drand によるトラストレスな時間解放暗号）。</p>
<p id="mu-seal-status" class="fx">復号中…</p></div>
<div id="mu-sealed-ct" style="display:none">{ct}</div>
<script src="https://timelock-web.fly.dev/bundle.js"></script>
<script>
(function(){{
  var UNLOCK={u_js};
  function esc(s){{return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');}}
  function reveal(){{
    var ct=document.getElementById('mu-sealed-ct').textContent;
    window.TL.decrypt(ct).then(function(pt){{
      var box=document.getElementById('mu-sealed');
      box.querySelector('#mu-seal-msg').innerHTML=esc(pt).replace(/\n/g,'<br>');
      var st=document.getElementById('mu-seal-status'); if(st) st.textContent='✓ 解禁されました';
    }}).catch(function(e){{
      var st=document.getElementById('mu-seal-status');
      if(/too early|decryptable at/i.test(e.message||'')){{
        if(st) st.textContent='⏳ まだ開けません（解禁予定: '+UNLOCK+'）';
        var ms=Math.max(0,new Date(UNLOCK).getTime()-Date.now())+4000;
        setTimeout(reveal, Math.min(ms, 30*60*1000));
      }} else if(st) st.textContent='復号に失敗しました。時間をおいて再読み込みしてください。';
    }});
  }}
  if(window.TL) reveal(); else window.addEventListener('tl-ready', reveal);
}})();
</script>"##,
            u = u,
            ct = ct,
            u_js = u_js,
        )
    } else {
        String::new()
    };

    // mockup: prefer external CDN; fall back to /static/... relative to root.
    // Printful tmp upload URLs expire (~24h → 403) — treat them as absent.
    let img = mockup_ext
        .filter(|s| !s.is_empty())
        .filter(|s| !s.starts_with("https://printful-upload.s3") && !s.contains("/tmp/"))
        .or_else(|| mockup_main.map(|p| format!("https://merch.wearmu.com{}", p)))
        .unwrap_or_else(|| "/static/og-default.png".to_string());

    // Digital goods (event ticket / song) reuse this PDP but must NOT show
    // apparel-only blocks (size chart, shipping table, garment cross-sell,
    // "Printful 国際発送" copy) — nothing physical ships.
    let kind_guess = kind_from_sku(&sku);
    let is_digital = matches!(kind_guess, "event_ticket" | "song");
    let is_song = kind_guess == "song";
    // MUON コレクター動機: Tシャツは3枚集めると ¥2,000 のMUクレジット(期限なし)。
    // ログイン不要の常時表示バナーで「集めたくなる」ループを作る。
    let muon_banner = if kind_guess == "tee" {
        r#"<div class="muon-b">🎟 <b>MUON コレクター</b> — Tシャツを3枚集めると <b style="color:#ffd700">¥2,000 のMUクレジット</b>。次のお買い物の決済で自動で使えます（期限なし・6枚で2回目）。</div>"#
    } else { "" };
    // Self-fulfilled hardware (Koe デバイス等): physical だが Printful ではない —
    // アパレル前提のサイズ表・Printful送料表・「7-14日国際発送」コピーを出さない。
    let is_device = kind_guess == "device";

    // extras — fetch with labels so we can surface 着用イメージ (on-body
    // styling renders) prominently, separate from technical mockup angles.
    // NOTE: these lifestyle images are AI-rendered styling visuals, NOT real
    // customer photos — surfaced honestly as 着用イメージ, never claimed as UGC
    // / customer testimonials.
    let extras_rows: Vec<(String, String)> = {
        let conn = db.lock().unwrap();
        conn.prepare(
            "SELECT label, image_url FROM catalog_product_extras WHERE sku=? ORDER BY sort_order, id",
        )
        .ok()
        .and_then(|mut s| {
            s.query_map(rusqlite::params![&sku], |r| {
                Ok((r.get::<_, String>(0).unwrap_or_default(), r.get::<_, String>(1)?))
            })
            .ok()
            .map(|it| it.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
    };

    let is_lifestyle = |l: &str| l.to_lowercase().contains("lifestyle");
    // bare print artwork (the design file) is not a product shot → drop it.
    let is_artwork = |l: &str| {
        let l = l.to_lowercase();
        l == "design" || l == "concept_design" || l == "print"
    };
    let lifestyle_imgs: Vec<&String> = extras_rows
        .iter()
        .filter(|(l, u)| is_lifestyle(l) && !u.is_empty())
        .map(|(_, u)| u)
        .take(3)
        .collect();
    let other_imgs: Vec<&String> = extras_rows
        .iter()
        .filter(|(l, u)| !is_lifestyle(l) && !is_artwork(l) && !u.is_empty())
        .map(|(_, u)| u)
        .collect();

    let lifestyle_html = if lifestyle_imgs.is_empty() {
        String::new()
    } else {
        let mut s = String::from(
            r#"<div class="wear"><h3 class="wear-h">着用イメージ</h3><div class="wear-grid">"#,
        );
        for u in &lifestyle_imgs {
            s.push_str(&format!(
                r#"<img src="{}" alt="着用イメージ" loading="lazy">"#,
                html_attr(u)
            ));
        }
        s.push_str("</div></div>");
        s
    };

    let extras_html = if other_imgs.is_empty() {
        String::new()
    } else {
        let mut s = String::from(r#"<div class="extras">"#);
        for u in &other_imgs {
            s.push_str(&format!(
                r#"<img src="{}" alt="" loading="lazy">"#,
                html_attr(u)
            ));
        }
        s.push_str("</div>");
        s
    };

    // デザイン原画 (label=design/print) — プリント柄そのもの。従来は他角度と一緒に
    // 破棄していたが、PDP で「柄のアップが見たい」要望に応え、独立セクションで見せる。
    // クリックで原寸ライトボックス表示。
    let design_imgs: Vec<&String> = extras_rows
        .iter()
        .filter(|(l, u)| is_artwork(l) && !u.is_empty())
        .map(|(_, u)| u)
        .collect();
    let design_html = if design_imgs.is_empty() {
        String::new()
    } else {
        let mut s = String::from(
            r#"<div class="design"><h3 class="wear-h">デザイン (プリント柄)</h3><div class="design-grid">"#,
        );
        for u in &design_imgs {
            s.push_str(&format!(
                r#"<img src="{}" alt="デザイン" loading="lazy">"#,
                html_attr(u)
            ));
        }
        s.push_str("</div></div>");
        s
    };

    let suzuri_link = suzuri
        .filter(|s| s.starts_with("http"))
        .map(|u| {
            format!(
                r#"<a class="buy alt" href="{}" target="_blank" rel="noopener">🇯🇵 SUZURI で買う (国内発送 5-10 日)</a>"#,
                html_attr(&u)
            )
        })
        .unwrap_or_default();

    // Same-brand cross-sell add-on (案B, AOV lever): if this product is not
    // itself a sticker, offer a ¥800-ish sticker from the SAME brand as a
    // one-tap add-on. Checking the box appends &addon=<sku> to the checkout
    // link; shop_checkout adds it as a 2nd Stripe line_item and the webhook
    // fulfils it as a 2nd Printful item (multi-SKU fulfillment, this branch).
    // Skipped when the brand has no live sticker, or this product is one.
    let is_sticker = sku.to_uppercase().contains("STICK") || price_jpy <= 1000;
    let addon: Option<(String, i64)> = if is_sticker || is_digital {
        None
    } else {
        let conn = db.lock().unwrap();
        // Prefer a sticker from the SAME brand; otherwise fall back to the
        // universal MU mark sticker (seed_mu_sticker) so the cross-sell
        // fires on every apparel SKU, not just the 3 brands that ship their
        // own sticker.
        conn.query_row(
            "SELECT sku, retail_price_jpy FROM catalog_products
             WHERE brand=? AND is_active=1 AND status='live' AND sku!=?
               AND (UPPER(sku) LIKE '%STICK%' OR label LIKE '%sticker%'
                    OR label LIKE '%ステッカー%')
               AND retail_price_jpy BETWEEN 1 AND 1500
             ORDER BY retail_price_jpy LIMIT 1",
            rusqlite::params![&brand, &sku],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )
        .or_else(|_| conn.query_row(
            "SELECT sku, retail_price_jpy FROM catalog_products
             WHERE sku='MU-STICKER-MARK' AND is_active=1 AND status='live'",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        ))
        .ok()
    };

    // Show the buy button whenever shop_checkout can build a Stripe
    // Session — that's either a pre-created stripe_price_id OR a positive
    // retail_price_jpy (price_data inline). Without this, auto-generated
    // SKUs (which deliberately skip price-id pre-mint) render as
    // "準備中" and customers never click — a critical conversion gap.
    let buy_button = if price_id.as_deref().unwrap_or("").starts_with("price_")
        || price_jpy > 0
    {
        let base = format!("/api/shop/checkout?sku={}", urlencoding::encode(&sku));
        let (cross_html, cross_script) = match &addon {
            Some((ssku, sprice)) => (
                format!(
                    r#"<label style="display:flex;align-items:center;gap:9px;justify-content:center;margin:12px 0 4px;cursor:pointer;font-size:13px;opacity:0.92"><input type="checkbox" id="addon-cb" data-sku="{ssku}" style="width:17px;height:17px;accent-color:#e6c449">＋ おそろいのステッカーも <b style="color:#e6c449;margin-left:2px">+¥{sprice_fmt}</b></label>"#,
                    ssku = html_attr(ssku), sprice_fmt = format_jpy(*sprice),
                ),
                format!(
                    r#"<script>(function(){{var c=document.getElementById('addon-cb'),b=document.getElementById('buybtn'),base="{base}",P={sprice},BASE={base_price};if(!c||!b)return;c.addEventListener('change',function(){{b.href=c.checked?base+"&addon="+encodeURIComponent(c.dataset.sku):base;var a=b.querySelector('.amt');if(a)a.textContent='¥'+(c.checked?BASE+P:BASE).toLocaleString();}});}})();</script>"#,
                    base = base, sprice = sprice, base_price = price_jpy,
                ),
            ),
            None => (String::new(), String::new()),
        };
        let fulfil_note = if is_song {
            "Stripe · 購入後すぐ視聴/DLリンクをメール"
        } else if is_digital {
            "Stripe · 購入後すぐ QR 入場券をメール"
        } else if is_device {
            "Stripe · 自社発送 3 日以内"
        } else {
            "Stripe + Printful 7-14 日 国際発送"
        };
        format!(
            r#"{cross_html}<a class="buy" id="buybtn" href="{base}" data-funnel="cta_click" data-funnel-cta="pdp_buy">買う <span class="amt">¥{price}</span> · 即購入 ({fulfil_note})</a>{cross_script}"#,
            cross_html = cross_html,
            base = base,
            price = format_jpy(price_jpy),
            fulfil_note = fulfil_note,
            cross_script = cross_script,
        )
    } else {
        r#"<div class="buy disabled">準備中</div>"#.to_string()
    };

    // Spec block: real BJJ buyers won't checkout without GSM / material /
    // print method. AUTO SKUs look up by their embedded kind; merch-bridge
    // SKUs use a SKU-pattern heuristic. (kind_guess computed near the top.)
    let spec_block = PRODUCT_SPECS
        .iter()
        .find(|s| s.kind == kind_guess)
        .map(|s| format!(
            r#"<div class="spec"><h3>SPEC</h3><p>{}</p></div>"#,
            html_text(s.spec_html)
        ))
        .unwrap_or_default();

    // Story block: only for AUTO SKUs — extracted from the theme slug.
    let story_block = sku.strip_prefix("AUTO-")
        .and_then(|rest| {
            // "BJJ-KURO-OBI-TEE-c…" → SEED_THEMES with slug "bjj_kuro_obi"
            SEED_THEMES.iter().find(|t| {
                let pat = t.slug.to_uppercase().replace('_', "-") + "-";
                rest.starts_with(&pat)
            })
        })
        .map(|t| format!(
            r#"<div class="story"><h3>STORY</h3><p>{}</p></div>"#,
            html_text(t.story)
        ))
        .unwrap_or_default();

    // Founder hand-signed thank-you card row removed 2026-05-22: clashes
    // with the "0% human autonomy" brand thesis + Yuki time-opp-cost is
    // too high to scale. catalog_founder_cards table + claim helper kept
    // for historical orders that already received cards; new PDPs skip
    // the row entirely.
    // Social proof: real per-SKU sold count from catalog_orders
    // (status='submitted' = Stripe-paid + Printful-accepted). Gated at
    // SOLD_BADGE_MIN — never surfaces 0/1 on a low-volume SKU.
    let sold_count: i64 = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM catalog_orders WHERE sku=? AND status='submitted'",
            rusqlite::params![&sku],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };
    let sold_row = if sold_count >= SOLD_BADGE_MIN {
        format!(
            "<div class=\"ts-row\">\n    <strong>これまで {n} 着 販売</strong>\n    <small>実際にお届けした数（受注生産・実績）</small>\n  </div>\n  ",
            n = sold_count
        )
    } else {
        String::new()
    };

    let trust_block = if is_device {
        format!(r##"<div class="trust-strip">
  {sold_row}<div class="ts-row">
    <strong>自社発送 3 日以内</strong>
    <small>決済後、Koe チームが直接梱包・発送 (追跡番号つき)</small>
  </div>
  <div class="ts-row">
    <strong>30 日 返品保証</strong>
    <small>初期不良は無料交換 · お問い合わせ info@enablerdao.com</small>
  </div>
  <div class="ts-row">
    <strong>オープンソース</strong>
    <small>ファームウェアは公開リポジトリ · 自分で書き換え可</small>
  </div>
</div>"##, sold_row = sold_row)
    } else if is_digital {
        let (l1, s1) = if is_song {
            ("購入後すぐメール配信", "視聴 & ダウンロードリンクを自動送信 · 物理発送なし")
        } else {
            ("購入後すぐ QR をメール", "会場で QR を提示して入場 · 物理発送なし")
        };
        format!(r##"<div class="trust-strip">
  {sold_row}<div class="ts-row">
    <strong>{l1}</strong>
    <small>{s1}</small>
  </div>
  <div class="ts-row">
    <strong>デジタル商品</strong>
    <small>送料 ¥0 · お問い合わせ info@enablerdao.com</small>
  </div>
</div>"##, sold_row = sold_row, l1 = l1, s1 = s1)
    } else {
        format!(r##"<div class="trust-strip">
  {sold_row}<div class="ts-row">
    <strong>国際発送 7-14 日</strong>
    <small>DHL/FedEx tracked · JP・US・EU・CA・AU 即対応</small>
  </div>
  <div class="ts-row">
    <strong>30 日 返品保証</strong>
    <small>サイズ違い・破損は無料交換 · returns@wearmu.com</small>
  </div>
  <div class="ts-row">
    <strong>受注生産 1 着から</strong>
    <small>注文を受けてから 1 枚ずつ縫製。 完売・在庫廃棄 ゼロ。</small>
  </div>
</div>"##, sold_row = sold_row)
    };

    // 試聴ブロック: description_ja か meta_json.audio_url に
    // "mu.koe.live/oto.html?s=KEY" が含まれる商品(MUON Tシャツ等の音源入りも含む)は
    // 買う前に試聴できるよう ▶ プレイヤーを出す（涼介FB#1: 買う前に聴かせて）。
    // 2026-06-04: MCP create/update の audio_url(=meta_json)からも鳴らせるよう
    // desc だけでなく meta_json.audio_url も探索対象にする(Tシャツに音源)。
    let meta_audio: String = meta_json
        .as_deref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|v| v.get("audio_url").and_then(|a| a.as_str()).map(|s| s.to_string()))
        .unwrap_or_default();
    let listen_block: String = {
        let hay = format!("{}\n{}", desc, meta_audio);
        if let Some(pos) = hay.find("oto.html?s=") {
            let rest = &hay[pos + "oto.html?s=".len()..];
            let key: String = rest.chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if !key.is_empty() {
                let pkey = html_attr(&key);
                format!(r##"<div class="listen">
  <button id="listenBtn" class="listen-btn" aria-label="試聴">▶ この曲を試聴</button>
  <span class="listen-note">着ると、この曲が鳴る</span>
  <audio id="listenAudio" preload="none" src="https://mu.koe.live/oto.html?s={pkey}"></audio>
  <script>(function(){{
    var b=document.getElementById('listenBtn');
    var url="https://gateway.irys.xyz/3uPYa7YCn9ExPK2WYuJcZd2WXRTF43WV3pagrcyB7xot";
    // oto.html の SONGS と同じ Arweave 音源を直接叩く（曲ごとの実URLは oto に集約）
    var SRC={{
      "everybody-say-bjj":"https://gateway.irys.xyz/5jsmQoNNekanEGMBUEhSLoZyxGXSDZL5taMZfwwrEC1c/everybody-say-bjj.mp3",
      "shio-to-pixel":"https://gateway.irys.xyz/5jsmQoNNekanEGMBUEhSLoZyxGXSDZL5taMZfwwrEC1c/shio-to-pixel.mp3",
      "musubinaosu-asa":"https://gateway.irys.xyz/5jsmQoNNekanEGMBUEhSLoZyxGXSDZL5taMZfwwrEC1c/musubinaosu-asa.mp3",
      "hello-2150":"https://gateway.irys.xyz/5jsmQoNNekanEGMBUEhSLoZyxGXSDZL5taMZfwwrEC1c/hello-2150.mp3",
      "i-love-you":"https://gateway.irys.xyz/5jsmQoNNekanEGMBUEhSLoZyxGXSDZL5taMZfwwrEC1c/i-love-you.mp3",
      "i-need-your-attention":"https://gateway.irys.xyz/5jsmQoNNekanEGMBUEhSLoZyxGXSDZL5taMZfwwrEC1c/i-need-your-attention.mp3",
      "free-to-change":"https://gateway.irys.xyz/5jsmQoNNekanEGMBUEhSLoZyxGXSDZL5taMZfwwrEC1c/free-to-change.mp3",
      "attention-kudasai":"https://gateway.irys.xyz/5jsmQoNNekanEGMBUEhSLoZyxGXSDZL5taMZfwwrEC1c/attention-kudasai.mp3"
    }};
    var a=new Audio(); a.src=SRC["{pkey}"]||""; var playing=false;
    b.addEventListener('click',function(){{
      if(!a.src){{window.open("https://mu.koe.live/oto.html?s={pkey}","_blank");return;}}
      if(playing){{a.pause();b.textContent="▶ この曲を試聴";playing=false;}}
      else{{a.play();b.textContent="❚❚ 停止";playing=true;}}
    }});
    a.addEventListener('ended',function(){{b.textContent="▶ この曲を試聴";playing=false;}});
  }})();</script>
</div>"##, pkey = pkey)
            } else { String::new() }
        } else { String::new() }
    };

    // kind=song、または meta_json.audio_url が直接の音声ファイル(.mp3/.wav/.m4a/.ogg)の商品は
    // 買う前に試聴できるネイティブプレイヤーを出す（QRで鳴るTシャツの音源もここで聴ける）。
    let listen_block = if listen_block.is_empty() {
        let direct_audio = meta_audio.starts_with("https://")
            && [".mp3", ".wav", ".m4a", ".ogg"].iter().any(|&e| meta_audio.ends_with(e));
        if (is_song && meta_audio.starts_with("https://")) || direct_audio {
            let note = if is_song { "買う前に、全部聴けます" } else { "QRで流れる曲。ここでも聴けます" };
            format!(r##"<div class="listen">
  <button id="songBtn" class="listen-btn" aria-label="試聴">▶ この曲を試聴</button>
  <span class="listen-note">{note}</span>
  <script>(function(){{
    var b=document.getElementById('songBtn');
    var a=new Audio(); a.src="{u}"; var playing=false;
    b.addEventListener('click',function(){{
      if(playing){{a.pause();b.textContent="▶ この曲を試聴";playing=false;}}
      else{{a.play();b.textContent="❚❚ 停止";playing=true;}}
    }});
    a.addEventListener('ended',function(){{b.textContent="▶ この曲を試聴";playing=false;}});
  }})();</script>
</div>"##, u = html_attr(&meta_audio), note = note)
        } else { listen_block }
    } else { listen_block };

    let body = format!(
        r##"<!doctype html><html lang="ja"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{short_title} | MU SHOP — wearmu.com</title>
<meta name="description" content="{desc_short}">
<meta property="og:image" content="{og}">
<meta property="og:title" content="{short_title}">
<meta property="og:description" content="{desc_short}">
<meta property="og:type" content="product">
<meta property="og:url" content="https://wearmu.com/shop/{sku_url}">
<meta property="og:site_name" content="wearmu.com">
<meta property="product:price:amount" content="{price_raw}">
<meta property="product:price:currency" content="JPY">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="{short_title}">
<meta name="twitter:image" content="{og}">
<link rel="canonical" href="https://wearmu.com/shop/{sku_url}">
<script type="application/ld+json">{{
  "@context": "https://schema.org/",
  "@type": "Product",
  "name": "{ld_title}",
  "image": ["{ld_img}"],
  "description": "{ld_desc}",
  "sku": "{ld_sku}",
  "brand": {{"@type": "Brand", "name": "{ld_brand}"}},
  "offers": {{
    "@type": "Offer",
    "url": "https://wearmu.com/shop/{sku_url}",
    "priceCurrency": "JPY",
    "price": "{price_raw}",
    "availability": "https://schema.org/InStock",
    "itemCondition": "https://schema.org/NewCondition"
  }}
}}</script>
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
body{{background:#0a0a0a;color:#f5f5f0;font-family:'Helvetica Neue','Hiragino Sans',Arial,sans-serif;line-height:1.6;font-size:14px}}
nav{{padding:16px 24px;border-bottom:1px solid rgba(255,255,255,0.08);display:flex;justify-content:space-between;align-items:center}}
nav a{{color:#f5f5f0;text-decoration:none;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.85}}
nav a:hover{{opacity:1}}
nav .brand{{font-weight:900;letter-spacing:0.4em}}
.wrap{{max-width:920px;margin:0 auto;padding:30px 22px 80px;display:grid;grid-template-columns:1fr 1fr;gap:30px}}
@media (max-width:740px){{.wrap{{grid-template-columns:1fr}}}}
.hero img{{width:100%;height:auto;border-radius:6px;background:#000;display:block}}
.extras{{display:grid;grid-template-columns:repeat(auto-fill,minmax(80px,1fr));gap:6px;margin-top:8px}}
.extras img{{width:100%;aspect-ratio:1/1;object-fit:cover;border-radius:3px;background:#000}}
.wear{{margin-top:14px}}
.wear-h{{font-size:11px;letter-spacing:0.18em;text-transform:uppercase;color:rgba(245,245,240,0.55);margin:0 0 6px}}
.wear-grid{{display:grid;grid-template-columns:repeat(auto-fill,minmax(140px,1fr));gap:6px}}
.wear-grid img{{width:100%;aspect-ratio:4/5;object-fit:cover;border-radius:5px;background:#000}}
.design{{margin-top:14px}}
.design-grid{{display:grid;grid-template-columns:repeat(auto-fill,minmax(120px,1fr));gap:6px}}
.design-grid img{{width:100%;aspect-ratio:1/1;object-fit:contain;border-radius:5px;background:#fff;padding:6px}}
.hero img,.wear-grid img,.extras img,.design-grid img{{cursor:zoom-in;transition:opacity .15s}}
.hero img:hover,.wear-grid img:hover,.extras img:hover,.design-grid img:hover{{opacity:.86}}
#lb{{position:fixed;inset:0;background:rgba(0,0,0,0.95);display:none;align-items:center;justify-content:center;z-index:9999;cursor:zoom-out;padding:24px}}
#lb.on{{display:flex}}
#lb img{{max-width:96vw;max-height:90vh;object-fit:contain;border-radius:4px;box-shadow:0 8px 60px rgba(0,0,0,0.6)}}
#lb .lb-x{{position:absolute;top:14px;right:22px;color:#fff;font-size:34px;line-height:1;cursor:pointer;opacity:.85;font-weight:300}}
#lb .lb-x:hover{{opacity:1}}
#lb .lb-hint{{position:absolute;bottom:18px;left:0;right:0;text-align:center;color:rgba(255,255,255,0.5);font-size:11px;letter-spacing:.1em}}
.body h1{{font-size:26px;line-height:1.3;margin-bottom:8px;font-weight:900;letter-spacing:.01em}}
.body .tagline{{font-size:13.5px;line-height:1.75;color:rgba(245,245,240,0.7);margin:0 0 16px;font-weight:400}}
.muon-b{{margin:10px 0 4px;padding:11px 14px;border:1px solid rgba(255,215,0,0.35);background:linear-gradient(180deg,rgba(255,215,0,0.06),rgba(255,215,0,0.02));border-radius:8px;font-size:12px;line-height:1.7;color:rgba(245,245,240,0.85)}}
.body .brand{{font-size:10px;letter-spacing:0.3em;color:#ffd700;text-transform:uppercase;margin-bottom:8px}}
.body .price{{font-size:22px;font-family:monospace;font-weight:700;color:#fff;margin-bottom:18px}}
.body .desc{{color:rgba(245,245,240,0.78);font-size:13px;line-height:1.85;margin-bottom:22px}}
.body .sku{{color:rgba(245,245,240,0.45);font-family:monospace;font-size:10px;margin-bottom:18px}}
.listen{{margin-bottom:18px;display:flex;align-items:center;gap:12px;flex-wrap:wrap}}
.listen-btn{{background:#111;color:#fff;border:1px solid #ffd700;border-radius:30px;padding:11px 22px;font-size:14px;font-weight:700;cursor:pointer;letter-spacing:.04em}}
.listen-btn:hover{{background:#1a1a1a}}
.listen-note{{color:rgba(255,215,0,.75);font-size:12px;letter-spacing:.08em}}
.buy{{display:block;background:#ffd700;color:#000;padding:14px;text-align:center;font-weight:700;border-radius:6px;text-decoration:none;margin-bottom:8px;letter-spacing:0.05em;font-size:13px}}
.buy.alt{{background:transparent;color:#ffd700;border:1px solid #ffd700}}
.trust-strip{{display:grid;gap:6px;font-size:11px;color:rgba(245,245,240,0.72);margin:18px 0;padding-top:14px;border-top:1px solid rgba(255,255,255,0.06)}}
.trust-strip .ts-row{{display:flex;gap:8px;align-items:baseline;flex-wrap:wrap;line-height:1.5}}
.trust-strip strong{{color:#fff;font-weight:600;font-size:12px;flex:0 0 auto}}
.trust-strip small{{color:rgba(245,245,240,0.55);font-size:10.5px}}
.spec, .story{{margin:20px 0;padding:14px 0;border-top:1px solid rgba(255,255,255,0.06)}}
.spec h3, .story h3{{font-size:10px;letter-spacing:0.3em;color:#ffd700;margin-bottom:8px;font-weight:700;text-transform:uppercase}}
.spec p, .story p{{font-size:12.5px;line-height:1.85;color:rgba(245,245,240,0.78)}}
.fx{{font-size:11px;color:rgba(245,245,240,0.45);font-family:monospace;font-weight:400}}
table.sz{{width:100%;border-collapse:collapse;font-size:11.5px;margin-top:4px}}
table.sz th, table.sz td{{padding:5px 8px;border-bottom:1px solid rgba(255,255,255,0.06);text-align:left;color:rgba(245,245,240,0.82);font-family:monospace}}
table.sz th{{color:rgba(245,245,240,0.45);font-weight:500;font-size:10px;letter-spacing:0.1em;text-transform:uppercase}}
.sz-cap{{font-size:10.5px;color:rgba(245,245,240,0.45);margin-top:8px;font-style:italic}}
.pdp-footer{{max-width:920px;margin:0 auto;padding:30px 22px 50px;border-top:1px solid rgba(255,255,255,0.06);text-align:center;color:rgba(245,245,240,0.5);font-size:10px;letter-spacing:0.1em}}
.legal-links{{display:flex;flex-wrap:wrap;justify-content:center;gap:18px;margin-bottom:12px;font-size:11px;letter-spacing:0.15em}}
.legal-links a{{color:rgba(245,245,240,0.7);text-decoration:none;text-transform:uppercase}}
.legal-links a:hover{{color:#ffd700}}
.legal-fine{{color:rgba(245,245,240,0.35);font-size:9.5px;line-height:1.6}}
.buy.disabled{{background:#222;color:#666;cursor:not-allowed}}
/* モバイル: 画像列の下に埋まる買うボタンを画面下に張り付かせる(7秒離脱対策)。
   position:sticky なので自然位置までスクロールすれば元のレイアウトに収まる。 */
@media (max-width:740px){{
  a.buy{{position:sticky;bottom:10px;z-index:20;box-shadow:0 4px 24px rgba(0,0,0,0.55)}}
}}
.back{{display:inline-block;margin-top:24px;color:rgba(245,245,240,0.6);text-decoration:none;font-size:11px}}
.back:hover{{color:#ffd700}}
</style></head><body>
<nav>
  <a class="brand" href="/">MU</a>
  <div>
    <a href="/shop">← SHOP</a>
  </div>
</nav>
<div class="wrap">
  <div class="hero">
    <img src="{og}" alt="{title}" loading="lazy" onerror="this.onerror=null;this.src='/static/designs/marker_zero.png';this.style.objectFit='contain';this.style.background='#0a0a0a';this.style.padding='60px'">
    {design}
    {lifestyle}
    {extras}
  </div>
  <div class="body">
    <div class="brand">{brand}</div>
    <h1>{headline}</h1>
    {tagline_html}
    <div class="price">¥{price} <small class="fx">≈ ${usd} / €{eur}</small></div>
    {sealed}
    {listen}
    {buy}
    {muon_banner}
    {suzuri}
    {trust}
    {spec}
    {size_chart}
    {shipping_table}
    {story}
    <div class="sku">SKU: {sku}</div>
    <a class="back" href="/shop?brand={brand_q}">← {brand} のほかの商品</a>
  </div>
</div>
<div style="max-width:920px;margin:0 auto;padding:0 22px 10px">{make_cta}</div>
<footer class="pdp-footer">
  <div class="legal-links">
    <a href="/shop">SHOP</a>
    <a href="/make">作る</a>
    <a href="/shipping">配送 / Shipping</a>
    <a href="/returns">返品 / Returns</a>
    <a href="/faq">FAQ</a>
    <a href="/privacy">プライバシー / Privacy</a>
    <a href="mailto:info@enablerdao.com">CONTACT</a>
  </div>
  <div class="legal-fine">© 2026 MU / Enabler Inc. · 東京千代田区九段南 1-5-6 · 受注生産・国際発送 7-14 日</div>
</footer>
<script defer src="/mu-funnel.js"></script>
<div id="lb"><span class="lb-x">×</span><img id="lb-img" alt=""><div class="lb-hint">クリック / Esc で閉じる</div></div>
<script>
(function(){{
  var lb=document.getElementById('lb'),li=document.getElementById('lb-img');
  if(!lb||!li)return;
  document.querySelectorAll('.hero img,.wear-grid img,.extras img,.design-grid img').forEach(function(im){{
    im.addEventListener('click',function(){{
      li.src=im.getAttribute('data-full')||im.currentSrc||im.src;
      lb.classList.add('on');
    }});
  }});
  function close(){{lb.classList.remove('on');li.removeAttribute('src');}}
  lb.addEventListener('click',close);
  document.addEventListener('keydown',function(e){{if(e.key==='Escape')close();}});
}})();
</script>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
</body></html>"##,
        make_cta = make_cta_banner("pdp"),
        title = html_text(&display_name),
        headline = html_text(&headline),
        tagline_html = tagline_html,
        muon_banner = muon_banner,
        short_title = html_text(&short_title),
        desc_short = html_attr(&meta_desc_short),
        sealed = sealed_block,
        og = html_attr(&img),
        brand = html_text(&brand),
        brand_q = html_attr(&brand),
        price = format_jpy(price_jpy),
        usd = ((price_jpy as f64) / 159.0).round() as i64,
        eur = ((price_jpy as f64) / 172.0).round() as i64,
        sku = html_text(&sku),
        buy = buy_button,
        listen = listen_block,
        suzuri = suzuri_link,
        lifestyle = lifestyle_html,
        extras = extras_html,
        design = design_html,
        trust     = trust_block,
        spec      = spec_block,
        size_chart = if is_digital || is_device { String::new() } else { size_chart_html(&kind_guess) },
        shipping_table = if is_digital || is_device { String::new() } else { shipping_table_html() },
        story     = story_block,
        sku_url   = urlencoding::encode(&sku),
        price_raw = price_jpy,
        ld_title  = html_attr(&display_name),
        ld_img    = html_attr(&img),
        ld_desc   = html_attr(&meta_desc),
        ld_sku    = html_attr(&sku),
        ld_brand  = html_attr(&brand),
    );
    Html(body).into_response()
}

/// Per-kind size chart (cm). Numbers are vendor-published (Bella+Canvas /
/// Gildan / Printful AOP).
fn size_chart_html(kind: &str) -> String {
    let (rows, title) = match kind {
        "rashguard_ls" | "rashguard_black" => (
            vec![
                ("S",  "65",  "47", "63"),
                ("M",  "70",  "50", "65"),
                ("L",  "73",  "53", "67"),
                ("XL", "76",  "56", "69"),
                ("2XL","79",  "59", "71"),
            ],
            "Rashguard サイズ (cm) · 着丈 / 身幅 / 袖丈",
        ),
        "hoodie" | "crewneck" => (
            vec![
                ("S",  "68", "52", "61"),
                ("M",  "71", "55", "63"),
                ("L",  "74", "58", "65"),
                ("XL", "77", "61", "67"),
                ("2XL","80", "64", "68"),
            ],
            "Hoodie / Crewneck サイズ (cm) · 着丈 / 身幅 / 袖丈",
        ),
        _ => (
            vec![
                ("S",  "69", "46", "20"),
                ("M",  "71", "51", "21"),
                ("L",  "74", "56", "22"),
                ("XL", "76", "61", "23"),
                ("2XL","79", "66", "24"),
            ],
            "Bella+Canvas 3001 Tee サイズ (cm) · 着丈 / 身幅 / 肩幅",
        ),
    };
    let mut tr = String::new();
    for (sz, a, b, c) in rows {
        tr.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            sz, a, b, c
        ));
    }
    format!(
        r##"<div class="spec"><h3>SIZE</h3>
<table class="sz"><thead><tr><th>サイズ</th><th>A</th><th>B</th><th>C</th></tr></thead>
<tbody>{tr}</tbody></table>
<p class="sz-cap">{title}</p></div>"##,
        tr = tr, title = title
    )
}

/// Country shipping cost table. JPY estimates based on Printful's
/// 2026 rate card for tee/hoodie-sized parcels from EU origin to
/// JP/US/EU/CA/AU. Static — not a quote, customer sees real cost at
/// Stripe Checkout.
fn shipping_table_html() -> String {
    r##"<div class="spec"><h3>SHIPPING</h3>
<table class="sz"><thead><tr><th>送り先 / Country</th><th>到着 (日)</th><th>送料目安 (¥)</th></tr></thead><tbody>
<tr><td>🇯🇵 Japan</td><td>5-10</td><td>¥800</td></tr>
<tr><td>🇺🇸 United States</td><td>7-14</td><td>¥1,400</td></tr>
<tr><td>🇪🇺 EU (DE / FR / NL)</td><td>5-10</td><td>¥600</td></tr>
<tr><td>🇬🇧 United Kingdom</td><td>5-10</td><td>¥900</td></tr>
<tr><td>🇨🇦 Canada</td><td>7-14</td><td>¥1,500</td></tr>
<tr><td>🇦🇺 Australia</td><td>7-14</td><td>¥1,700</td></tr>
</tbody></table>
<p class="sz-cap">DHL / FedEx tracked. 実費は Stripe Checkout で表示。</p></div>"##.into()
}

// ─── Checkout (Stripe Session using pre-created price_id) ─────────────

#[derive(Deserialize)]
pub struct CheckoutQuery {
    pub sku: String,
    /// Optional cross-sell add-on SKU. Inert unless a (future) UI passes
    /// `?addon=<sku>`. When present, valid, active, and on a Printful
    /// route, it is added as line_items[1] and fulfilled alongside the
    /// main SKU. Absent / invalid → behaves exactly like a single-SKU
    /// checkout (full backward compat).
    #[serde(default)]
    pub addon: Option<String>,
    /// Gift-link key. When it matches env `MU_GIFT_KEY`, checkout is
    /// allowed for an otherwise-hidden (is_active=0) SKU — the only way
    /// the private 'halo' tees can be purchased. Ignored otherwise.
    #[serde(default)]
    pub key: Option<String>,
    /// Affiliate code (from `/r/:code` or `?ref=CODE`). Carried into the
    /// Stripe session metadata so fulfill_catalog_order can credit the
    /// referrer. Falls back to the `mu_ref` cookie when absent.
    #[serde(default, rename = "ref", alias = "referrer")]
    pub referrer: Option<String>,
}

/// Pull a referral code from the `mu_ref` cookie (set by `/r/:code`).
fn ref_from_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    let raw = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        if k.trim() == "mu_ref" { Some(v.trim().to_string()) } else { None }
    })
}

/// Sanitize a referral code to the same shape `/r/:code` records.
fn sanitize_ref(code: &str) -> Option<String> {
    let c: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect::<String>().to_uppercase();
    if c.len() >= 4 { Some(c) } else { None }
}

pub async fn shop_checkout(
    State(db): State<Db>,
    headers: axum::http::HeaderMap,
    Query(q): Query<CheckoutQuery>,
) -> Response {
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    if stripe_key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "checkout disabled").into_response();
    }
    let sku = q.sku;
    // A valid gift key unlocks an otherwise-hidden (is_active=0) SKU —
    // the private 'halo' tees. Public checkouts never pass a key, so they
    // always hit the is_active=1 path (zero behaviour change).
    let gift = gift_key_valid(q.key.as_deref());
    let row = {
        let conn = db.lock().unwrap();
        // MOCKUP_EXT_LIVE: skip expired Printful tmp URLs so the Stripe
        // checkout line item never shows a broken product image.
        let sql = if gift {
            format!(
            "SELECT stripe_price_id, retail_price_jpy, description_ja, brand,
                    COALESCE({ext}, mockup_main_file, ''),
                    COALESCE(fulfillment_route, 'printful_dtg'), meta_json
             FROM catalog_products WHERE sku=?", ext = MOCKUP_EXT_LIVE)
        } else {
            format!(
            "SELECT stripe_price_id, retail_price_jpy, description_ja, brand,
                    COALESCE({ext}, mockup_main_file, ''),
                    COALESCE(fulfillment_route, 'printful_dtg'), meta_json
             FROM catalog_products WHERE sku=? AND is_active=1", ext = MOCKUP_EXT_LIVE)
        };
        conn.query_row(
            &sql,
            rusqlite::params![&sku],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .ok()
    };
    let Some((price_id, price_jpy, desc, _brand, mockup_path, route, meta_json)) = row else {
        return (StatusCode::NOT_FOUND, "sku not found").into_response();
    };

    // Digital event ticket: enforce the capacity (定員) before opening a
    // Stripe session, and never collect a shipping address (nothing ships).
    // capacity lives in meta_json `{"capacity": N}`; NULL/absent = unlimited.
    // "Sold" = paid seats already recorded (ticket_delivered / ticket_comp)
    // plus seats mid-fulfillment (submitting), so a burst of concurrent
    // checkouts can't oversell past the cap (small over-count is impossible
    // because we count the reserved 'submitting' row too).
    let is_ticket = route == "digital";
    if is_ticket {
        let capacity: Option<i64> = meta_json
            .as_deref()
            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
            .and_then(|v| v.get("capacity").and_then(|c| c.as_i64()))
            .filter(|c| *c >= 0);
        if let Some(cap) = capacity {
            let sold: i64 = {
                let conn = db.lock().unwrap();
                conn.query_row(
                    "SELECT COUNT(*) FROM catalog_orders
                     WHERE sku=? AND status IN ('ticket_delivered','ticket_comp','submitting')",
                    rusqlite::params![&sku],
                    |r| r.get(0),
                )
                .unwrap_or(0)
            };
            if sold >= cap {
                return (
                    StatusCode::OK,
                    Html(format!(
                        "<!doctype html><meta charset=utf-8><meta name=robots content=noindex>\
                         <title>SOLD OUT — MU</title>\
                         <body style=\"background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,sans-serif;\
                         display:flex;min-height:90vh;align-items:center;justify-content:center;text-align:center\">\
                         <div><div style=\"font-size:13px;letter-spacing:.3em;color:#e6c449\">SOLD OUT</div>\
                         <h1 style=\"font-weight:500;font-size:22px;margin:14px 0 8px\">完売しました</h1>\
                         <p style=\"opacity:.6;font-size:13px\">定員 {cap} 名に達しました。<br>\
                         <a href=\"/shop/{sku}\" style=\"color:#e6c449\">← 戻る</a></p></div></body>",
                        cap = cap, sku = html_text(&sku),
                    )),
                )
                    .into_response();
            }
        }
    }

    // Limited physical edition (100個限定): enforce edition_size before
    // opening a Stripe session. Lives in meta_json `{"edition_size": N}`;
    // NULL/absent = unlimited (normal on-demand SKU). "Sold" = paid orders
    // recorded as 'submitted' (handed to fulfillment). Every sold unit
    // carries a serial #k/N — the public registry is /edition/:sku, where
    // the serial IS the order's ordinal within the SKU (no extra table).
    if !is_ticket {
        let edition_size: Option<i64> = meta_json
            .as_deref()
            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
            .and_then(|v| v.get("edition_size").and_then(|c| c.as_i64()))
            .filter(|c| *c > 0);
        if let Some(cap) = edition_size {
            let sold: i64 = {
                let conn = db.lock().unwrap();
                conn.query_row(
                    "SELECT COUNT(*) FROM catalog_orders WHERE sku=? AND status='submitted'",
                    rusqlite::params![&sku],
                    |r| r.get(0),
                )
                .unwrap_or(0)
            };
            if sold >= cap {
                return (
                    StatusCode::OK,
                    Html(format!(
                        "<!doctype html><meta charset=utf-8><meta name=robots content=noindex>\
                         <title>SOLD OUT — MU</title>\
                         <body style=\"background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,sans-serif;\
                         display:flex;min-height:90vh;align-items:center;justify-content:center;text-align:center\">\
                         <div><div style=\"font-size:13px;letter-spacing:.3em;color:#e6c449\">SOLD OUT</div>\
                         <h1 style=\"font-weight:500;font-size:22px;margin:14px 0 8px\">完売 — {cap}枚限定</h1>\
                         <p style=\"opacity:.6;font-size:13px\">{cap} 枚すべてに通し番号を付けてお届けしました。<br>\
                         <a href=\"/edition/{sku}\" style=\"color:#e6c449\">シリアル台帳を見る →</a></p></div></body>",
                        cap = cap, sku = html_text(&sku),
                    )),
                )
                    .into_response();
            }
        }
    }

    // Optional cross-sell add-on. Only honored when the SKU exists, is
    // active, has a positive price, and rides a Printful route (so
    // fulfill_catalog_order can actually build a 2nd Printful item for
    // it — see build_printful_item). Anything else → silently ignored,
    // i.e. plain single-SKU checkout (no customer harm: we never charge
    // for something we can't fulfill).
    struct Addon {
        sku: String,
        price_jpy: i64,
        desc: String,
        image: Option<String>,
    }
    let addon: Option<Addon> = q.addon
        .as_deref()
        .filter(|s| !s.is_empty() && *s != sku) // ignore self-pairing
        .and_then(|asku| {
            let conn = db.lock().unwrap();
            conn.query_row(
                "SELECT retail_price_jpy, description_ja,
                        COALESCE(mockup_url_external, mockup_main_file, ''),
                        COALESCE(fulfillment_route, 'printful_dtg')
                 FROM catalog_products WHERE sku=? AND is_active=1",
                rusqlite::params![asku],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            )
            .ok()
            .and_then(|(p, d, img, route): (i64, String, String, String)| {
                if p > 0 && route.starts_with("printful_") {
                    Some(Addon {
                        sku: asku.to_string(),
                        price_jpy: p,
                        desc: d,
                        image: if img.is_empty() { None } else { Some(img) },
                    })
                } else {
                    None
                }
            })
        });
    // Resolve to an absolute, publicly-fetchable URL Stripe can render in
    // the Checkout page. Relative /static/ paths get the merch-bridge
    // origin (Stripe must fetch HTTPS).
    let stripe_image: Option<String> = if mockup_path.is_empty() {
        None
    } else if mockup_path.starts_with("http") {
        Some(mockup_path)
    } else {
        Some(format!("https://merch.wearmu.com{}", mockup_path))
    };

    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    // Pass the real order value + Stripe session id so the /success page
    // fires the Google Ads purchase conversion with the ACTUAL amount (not
    // the ¥6,800 fallback) — accurate value is what Smart Bidding optimises
    // ROAS against. Stripe substitutes {CHECKOUT_SESSION_ID} server-side.
    // Conversion value = main + addon (when present) so the /success
    // page fires the Google Ads purchase conversion with the real total.
    let conv_value = price_jpy + addon.as_ref().map(|a| a.price_jpy).unwrap_or(0);
    let success_url = format!(
        "{}/success?from=shop&sku={}&value={}&sid={{CHECKOUT_SESSION_ID}}",
        base_url, urlencoding::encode(&sku), conv_value,
    );
    let cancel_url = if gift {
        format!("{}/gift/{}", base_url, urlencoding::encode(q.key.as_deref().unwrap_or("")))
    } else {
        format!("{}/shop/{}", base_url, urlencoding::encode(&sku))
    };

    // Two pricing paths:
    //   (1) pre-created stripe_price_id (the 1,519 SKUs imported from
    //       merch-bridge already have these — saves a Stripe API call).
    //   (2) dynamic price_data using retail_price_jpy + description_ja.
    //       Used for SKUs the autonomous generator creates on the fly so
    //       we don't have to round-trip Stripe to mint a price first.
    let mut form: Vec<(&str, String)> = vec![
        ("mode", "payment".into()),
        ("success_url", success_url),
        ("cancel_url", cancel_url),
        ("allow_promotion_codes", "true".into()),
        ("line_items[0][quantity]", "1".into()),
        ("metadata[kind]", "catalog".into()),
        ("metadata[catalog_sku]", sku.clone()),
    ];
    // Affiliate attribution: explicit ?ref= wins, else the mu_ref cookie set
    // by /r/:code. Validated/resolved to a commission at the webhook.
    if let Some(rc) = q.referrer.as_deref().and_then(sanitize_ref)
        .or_else(|| ref_from_cookie(&headers).and_then(|c| sanitize_ref(&c)))
    {
        form.push(("metadata[referrer_code]", rc));
    }
    // Physical goods collect a shipping address; a digital ticket does not
    // (nothing ships — we email a QR). Stripe still captures the buyer's
    // email in payment mode either way, which is all the ticket needs.
    if !is_ticket {
        for (i, cc) in ["JP", "US", "GB", "CA", "AU", "DE", "FR"].iter().enumerate() {
            form.push((
                match i {
                    0 => "shipping_address_collection[allowed_countries][0]",
                    1 => "shipping_address_collection[allowed_countries][1]",
                    2 => "shipping_address_collection[allowed_countries][2]",
                    3 => "shipping_address_collection[allowed_countries][3]",
                    4 => "shipping_address_collection[allowed_countries][4]",
                    5 => "shipping_address_collection[allowed_countries][5]",
                    _ => "shipping_address_collection[allowed_countries][6]",
                },
                cc.to_string(),
            ));
        }
    }
    match price_id.filter(|s| s.starts_with("price_")) {
        Some(pid) => {
            form.push(("line_items[0][price]", pid));
            // Pre-created prices carry images on the Stripe Product side;
            // we can't override them at session time, so nothing else
            // to push here.
        }
        None => {
            if price_jpy <= 0 {
                return (StatusCode::FAILED_DEPENDENCY,
                    "this SKU has no price configured").into_response();
            }
            form.push(("line_items[0][price_data][currency]", "jpy".into()));
            form.push(("line_items[0][price_data][unit_amount]", price_jpy.to_string()));
            form.push(("line_items[0][price_data][product_data][name]", desc.clone()));
            // Stripe Checkout renders product_data.images[0] as the
            // left-side thumbnail. Without this customers see a blank
            // square and bounce — particularly bad for cold ad traffic.
            if let Some(img) = stripe_image {
                form.push(("line_items[0][price_data][product_data][images][0]", img));
            }
        }
    }

    // Cross-sell add-on as line_items[1]. Always priced via dynamic
    // price_data from its own retail_price_jpy + description_ja (works
    // whether or not the addon has a pre-created Stripe price). The
    // metadata key catalog_addon_sku is what fulfill_catalog_order reads
    // to build the 2nd Printful item.
    if let Some(a) = &addon {
        form.push(("metadata[catalog_addon_sku]", a.sku.clone()));
        form.push(("line_items[1][quantity]", "1".into()));
        form.push(("line_items[1][price_data][currency]", "jpy".into()));
        form.push(("line_items[1][price_data][unit_amount]", a.price_jpy.to_string()));
        form.push(("line_items[1][price_data][product_data][name]", a.desc.clone()));
        if let Some(img) = &a.image {
            let abs = if img.starts_with("http") {
                img.clone()
            } else {
                format!("https://merch.wearmu.com{}", img)
            };
            form.push(("line_items[1][price_data][product_data][images][0]", abs));
        }
    }

    let resp = reqwest::Client::new()
        .post("https://api.stripe.com/v1/checkout/sessions")
        .basic_auth(&stripe_key, None::<&str>)
        .form(&form)
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => {
            let j: serde_json::Value = r.json().await.unwrap_or_default();
            let url = j["url"].as_str().unwrap_or("/").to_string();
            Redirect::to(&url).into_response()
        }
        Ok(r) => {
            let s = r.status();
            let t = r.text().await.unwrap_or_default();
            eprintln!(
                "[shop/checkout] stripe {}: {}",
                s,
                t.chars().take(300).collect::<String>()
            );
            (StatusCode::BAD_GATEWAY, "stripe error").into_response()
        }
        Err(e) => {
            eprintln!("[shop/checkout] reqwest: {}", e);
            (StatusCode::BAD_GATEWAY, "stripe network").into_response()
        }
    }
}

/// GET /gift/:key — private gallery of the hidden 'halo' message tees.
/// 404s unless `key` matches env `MU_GIFT_KEY`. noindex/nofollow and
/// never linked from anywhere public. Each design shows S/M/L buy
/// buttons that carry the key into checkout so the hidden SKU unlocks.
pub async fn gift_gallery(State(db): State<Db>, Path(key): Path<String>) -> Response {
    if !gift_key_valid(Some(&key)) {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let rows: Vec<(String, String, String, i64)> = {
        let conn = db.lock().unwrap();
        conn.prepare(
            "SELECT sku, description_ja, COALESCE(mockup_url_external,''), retail_price_jpy
             FROM catalog_products WHERE brand='halo' ORDER BY sort_order",
        )
        .ok()
        .and_then(|mut s| {
            s.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .ok()
            .map(|it| it.filter_map(|x| x.ok()).collect())
        })
        .unwrap_or_default()
    };

    // Group the per-size SKUs back into one card per design (stem = SKU
    // minus the trailing -S/-M/-L). Preserve sort_order via `order`.
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, (String, String, i64, Vec<(String, String)>)> =
        std::collections::HashMap::new();
    for (sku, desc, preview, price) in &rows {
        let (stem, size) = sku.rsplit_once('-').unwrap_or((sku.as_str(), ""));
        let cap = desc.split(" · ").next().unwrap_or(desc).to_string();
        let e = groups.entry(stem.to_string()).or_insert_with(|| {
            order.push(stem.to_string());
            (cap.clone(), preview.clone(), *price, Vec::new())
        });
        e.3.push((size.to_string(), sku.clone()));
    }

    let esc = |s: &str| {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
    };
    let key_e = urlencoding::encode(&key);
    let mut cards = String::new();
    for stem in &order {
        let (cap, preview, price, sizes) = &groups[stem];
        let mut btns = String::new();
        for (size, sku) in sizes {
            btns.push_str(&format!(
                "<a class=\"sz\" href=\"/api/shop/checkout?sku={}&amp;key={}\">{}</a>",
                urlencoding::encode(sku),
                key_e,
                esc(size)
            ));
        }
        cards.push_str(&format!(
            "<div class=\"card\"><div class=\"imgwrap\"><img src=\"{}\" alt=\"{}\" loading=\"lazy\"></div>\
             <div class=\"cap\">{}</div><div class=\"price\">¥{}</div><div class=\"sizes\">{}</div></div>",
            esc(preview), esc(cap), esc(cap), price, btns
        ));
    }

    let page = format!(
        "<!doctype html><html lang=\"ja\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<meta name=\"robots\" content=\"noindex,nofollow\">\
<title>無 — private</title>\
<style>\
*{{box-sizing:border-box}}body{{margin:0;background:#0b0b0b;color:#f4f1ea;\
font-family:'Hiragino Mincho ProN','Hiragino Sans',serif;-webkit-font-smoothing:antialiased}}\
.wrap{{max-width:1100px;margin:0 auto;padding:64px 20px 96px}}\
.kick{{font-family:'Hiragino Sans',sans-serif;font-size:11px;letter-spacing:.4em;color:#7c8088;text-align:center}}\
h1{{font-size:64px;font-weight:600;text-align:center;margin:18px 0 6px}}\
.sub{{font-family:'Hiragino Sans',sans-serif;font-size:12px;letter-spacing:.2em;color:#7c8088;text-align:center;margin-bottom:8px}}\
.note{{font-family:'Hiragino Sans',sans-serif;font-size:11px;color:#5c6066;text-align:center;margin-bottom:48px}}\
.grid{{display:grid;grid-template-columns:repeat(auto-fill,minmax(230px,1fr));gap:22px}}\
.card{{background:#111;border:1px solid rgba(255,255,255,.07);border-radius:6px;overflow:hidden}}\
.imgwrap{{aspect-ratio:6/7;background:#1a1c1e;overflow:hidden}}\
.imgwrap img{{width:100%;height:100%;object-fit:cover;display:block}}\
.cap{{padding:12px 12px 2px;font-size:17px}}\
.price{{padding:0 12px;font-family:'Hiragino Sans',sans-serif;font-size:12px;color:#e6c449;font-variant-numeric:tabular-nums}}\
.sizes{{display:flex;gap:8px;padding:12px}}\
.sz{{flex:1;text-align:center;padding:9px 0;border:1px solid rgba(255,255,255,.16);border-radius:4px;\
color:#f4f1ea;text-decoration:none;font-family:'Hiragino Sans',sans-serif;font-size:13px;letter-spacing:.1em;transition:all .15s}}\
.sz:hover{{background:#f4f1ea;color:#0b0b0b;border-color:#f4f1ea}}\
.foot{{text-align:center;margin-top:56px;font-family:'Hiragino Sans',sans-serif;font-size:11px;color:#5c6066;line-height:1.9}}\
</style></head><body><div class=\"wrap\">\
<div class=\"kick\">MU ／ 無 ・ PRIVATE</div>\
<h1>無</h1>\
<div class=\"sub\">message tees · 2026.06.01</div>\
<div class=\"note\">黒T Bella+Canvas 3001 ・ DTG ・ 受注生産（在庫ゼロ）・ ¥4,000 ・ S/M/L</div>\
<div class=\"grid\">{cards}</div>\
<div class=\"foot\">この一着は、記録になる。<br>非公開リンク・あなただけのページ</div>\
</div></body></html>",
        cards = cards
    );
    axum::response::Html(page).into_response()
}

// ─── Webhook fulfillment (called from main.rs stripe_webhook) ─────────

/// Build the per-SKU Printful order `item` JSON for a single catalog SKU.
///
/// This is the reusable core of the fulfillment item construction (the
/// three Printful shapes — sync_variant_id / variant_id+files /
/// variant_id-only — plus the stitch_color option block and the
/// per-panel placement fan-out). It is called once per item: the MAIN
/// SKU (with the session-derived `retail_price` and any customer size
/// override) and, when present, an `addon` SKU (with its own
/// retail_price and no size override).
///
/// `retail_price` is passed in (NOT recomputed) so the main item stays
/// byte-identical to the pre-refactor code, which used the Stripe
/// session total. `variant_override` is the size-resolved variant for
/// the main SKU (`None` for the addon, which is single-size).
///
/// Returns `None` when the SKU is missing or its fulfillment route is
/// not a Printful route (e.g. contrado_uk / manual / digital). The
/// caller decides whether a `None` for the main SKU aborts the order or
/// whether a `None` addon is simply skipped.
fn build_printful_item(
    conn: &rusqlite::Connection,
    sku: &str,
    retail_price: &str,
    variant_override: Option<i64>,
    require_printful: bool,
) -> Option<serde_json::Value> {
    let (pp_id, mut pf_variant_id, sync_variant_id, design_file, placement, route): (
        i64,
        i64,
        Option<i64>,
        Option<String>,
        String,
        String,
    ) = conn
        .query_row(
            "SELECT printful_product_id, printful_variant_id,
                    printful_sync_variant_id, design_file, printful_placement,
                    COALESCE(fulfillment_route, 'printful_dtg')
             FROM catalog_products WHERE sku=?",
            rusqlite::params![sku],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            },
        )
        .ok()?;

    // For the MAIN item we replicate the pre-refactor behaviour exactly:
    // contrado_uk already early-returned in the caller, and every other
    // route (printful_* AND the gelato_jp / suzuri_jp / manual / digital
    // fallbacks) built a Printful item from its printful_variant_id. So
    // `require_printful=false` (main path) never gates on route here.
    // The ADDON path passes `require_printful=true` because mixing a
    // non-Printful add-on into this single Printful order makes no sense —
    // such an add-on is skipped by the caller instead.
    if require_printful && !route.starts_with("printful_") {
        return None;
    }

    if let Some(v) = variant_override {
        pf_variant_id = v;
    }

    // AOP rashguards (Printful product 301) require a `stitch_color`
    // option ('white' or 'black'). Default to black so the seams match
    // the dominant body of the print on dark rashguards.
    let needs_stitch_color = matches!(pp_id, 301 | 302 | 368 | 369 | 836);
    let options_block: Vec<serde_json::Value> = if needs_stitch_color {
        vec![serde_json::json!({"id":"stitch_color","value":"black"})]
    } else {
        Vec::new()
    };

    // Three fulfillment shapes Printful accepts:
    //   (a) pre-synced product (sync_variant_id) — merch-bridge import path
    //   (b) base variant + inline files (design_file URL + placement) —
    //       the autonomous generator path; no sync_product round-trip needed
    //   (c) base variant only (no design) — fallback, mainly for testing
    let item: serde_json::Value = match (sync_variant_id, design_file.as_deref()) {
        (Some(svid), _) if svid > 0 => serde_json::json!({
            "sync_variant_id": svid,
            "quantity": 1,
            "retail_price": retail_price,
            "options": options_block,
        }),
        (_, Some(df)) if !df.is_empty() => {
            let file_url = if df.starts_with("http") {
                df.to_string()
            } else {
                // design_file = "/static/designs/foo.png" → absolute URL Printful can fetch
                format!("{}{}", env::var("BASE_URL")
                    .unwrap_or_else(|_| "https://wearmu.com".into()), df)
            };
            // Fan the same design out to every panel the product supports.
            // For AOP rashguards this is front/back/both sleeves so the
            // garment ships in its true belt color, not chest-printed white.
            // The stored placement is honored for single-panel products
            // (tees/hoodies) where the helper returns just ["front"].
            let resolved_placements = placements_for_product(pp_id);
            let resolved_placements: Vec<&str> =
                if resolved_placements == ["front"] && placement != "front" {
                    vec![placement.as_str()]
                } else {
                    resolved_placements.iter().copied().collect()
                };
            let files: Vec<serde_json::Value> = resolved_placements.iter().map(|p| {
                serde_json::json!({"url": file_url, "placement": p})
            }).collect();
            serde_json::json!({
                "variant_id": pf_variant_id,
                "quantity": 1,
                "retail_price": retail_price,
                "files": files,
                "options": options_block,
            })
        }
        _ => serde_json::json!({
            "variant_id": pf_variant_id,
            "quantity": 1,
            "retail_price": retail_price,
            "options": options_block,
        }),
    };
    Some(item)
}

/// Fire on checkout.session.completed when metadata.kind == "catalog".
/// Posts the order to Printful with the JP→ISO state normalization +
/// the customer-selected size variant override (if any). Writes a row
/// to catalog_orders for audit / replay.
pub async fn fulfill_catalog_order(db: Db, session: serde_json::Value) {
    let session_id = session["id"].as_str().unwrap_or("").to_string();
    let sku = session["metadata"]["catalog_sku"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if sku.is_empty() {
        tracing::warn!("[catalog/fulfill] no catalog_sku in metadata, session={}", session_id);
        return;
    }
    let amount_total = session["amount_total"].as_i64().unwrap_or(0);
    let currency = session["currency"].as_str().unwrap_or("jpy").to_lowercase();

    // Idempotency: ATOMICALLY reserve this session before doing anything that
    // costs money. The old code did a read-then-act (SELECT, later INSERT),
    // which has a TOCTOU race: Stripe delivers webhooks at-least-once, and the
    // /replay + retry-cron paths can re-enter, so two invocations for the same
    // session could both pass the SELECT (no row yet) and both POST to Printful
    // → 2 garments shipped for 1 payment. INSERT OR IGNORE against the
    // UNIQUE(stripe_session_id) constraint is race-free: exactly one caller
    // inserts the 'submitting' row (changes()==1), everyone else gets 0 and
    // bails. record_order_full later REPLACEs this row with the final status.
    {
        let conn = db.lock().unwrap();
        let reserved = conn
            .execute(
                "INSERT OR IGNORE INTO catalog_orders
                   (stripe_session_id, sku, amount_jpy, status)
                 VALUES (?, ?, ?, 'submitting')",
                rusqlite::params![&session_id, &sku, amount_total],
            )
            .unwrap_or(0);
        if reserved == 0 {
            tracing::info!("[catalog/fulfill] session {} already reserved/fulfilled, skip", session_id);
            return;
        }
    }

    // Read fulfillment_route + printful_product_id for the main SKU. The
    // remaining Printful identifiers are looked up inside
    // build_printful_item(); here we only need pp_id (for the size-variant
    // override) and route (for the contrado early-return). Existing rows
    // default to 'printful_dtg' so the legacy path is unaffected.
    let product = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT printful_product_id,
                    COALESCE(fulfillment_route, 'printful_dtg')
             FROM catalog_products WHERE sku=?",
            rusqlite::params![&sku],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                ))
            },
        )
        .ok()
    };
    let Some((_pp_id, route)) = product
    else {
        tracing::warn!("[catalog/fulfill] sku {} not in catalog_products", sku);
        return;
    };

    // Affiliate commission — route-agnostic, runs before route dispatch so it
    // applies to every product type (apparel / ticket / song). Idempotent and
    // safe to call for orders with no referrer (no-ops). Stamps the order's
    // audit columns BEFORE any record_order_full REPLACE (which preserves them).
    apply_affiliate(&db, &session_id, &session, &sku, amount_total).await;

    // Route dispatch. printful_* / gelato_jp / suzuri_jp / manual / digital
    // continue through the existing Printful logic below as a fallback. A new
    // contrado_uk route diverts to the Helix API.
    if route == "contrado_uk" {
        fulfill_via_contrado(db, &session_id, &sku, amount_total, &currency).await;
        return;
    }

    // Pull selected size from Stripe custom_fields (if any). When the
    // SKU's print_id supports per-size variants we swap pf_variant_id
    // to the matching one. Without this, every order ships size M
    // regardless of what the customer picked.
    let mut variant_override: Option<i64> = None;
    if let Some(custom_fields) = session["custom_fields"].as_array() {
        for cf in custom_fields {
            if cf["key"].as_str() == Some("size") {
                let chosen = cf["dropdown"]["value"].as_str().unwrap_or("M");
                variant_override = resolve_size_variant(_pp_id, chosen);
                break;
            }
        }
    }

    // Stripe Checkout webhooks sometimes omit shipping_details from
    // data.object even when shipping_address_collection was enabled —
    // we have to retrieve the session with expand=['shipping_details'].
    // Without this, fulfill_catalog_order() POSTs to Printful with
    // empty address1/city/state and 4xx's (verified live with order #1).
    let mut shipping_owned = session["shipping_details"].clone();
    let mut cust_owned = session["customer_details"].clone();
    if shipping_owned["address"]["line1"].as_str().unwrap_or("").is_empty() {
        if let (true, Ok(stripe_key)) = (
            !session_id.is_empty(),
            std::env::var("STRIPE_SECRET_KEY"),
        ) {
            let url = format!(
                "https://api.stripe.com/v1/checkout/sessions/{}",
                session_id
            );
            if let Ok(r) = reqwest::Client::new()
                .get(&url).basic_auth(&stripe_key, None::<&str>).send().await
            {
                if let Ok(v) = r.json::<serde_json::Value>().await {
                    if !v["shipping_details"].is_null() {
                        shipping_owned = v["shipping_details"].clone();
                    }
                    if !v["customer_details"].is_null() {
                        cust_owned = v["customer_details"].clone();
                    }
                    // Stripe Checkout also nests address under
                    // customer_details when billing == shipping; fall
                    // back to that if shipping_details is still empty.
                    if shipping_owned["address"]["line1"].as_str().unwrap_or("").is_empty() {
                        if let Some(billing) = v["customer_details"]["address"].as_object() {
                            shipping_owned = serde_json::json!({
                                "name":    v["customer_details"]["name"].clone(),
                                "phone":   v["customer_details"]["phone"].clone(),
                                "address": billing,
                            });
                        }
                    }
                }
            }
        }
    }
    let shipping = &shipping_owned;
    let addr = &shipping["address"];
    let cust = &cust_owned;
    let country = addr["country"].as_str().unwrap_or("JP").to_uppercase();
    let raw_state = addr["state"].as_str().unwrap_or("");
    let state_code = normalize_state_code(&country, raw_state);

    // ── Manual / self-fulfilled route (NFC音コイン etc.) ──────────────────
    // No POD vendor: we take payment, then a human writes the NFC tag and
    // mails it. This is a first-class route (contract: manual = "we just
    // process payment"), NOT the failed_no_item *error* path below. We record
    // the paid order as 'manual_pending' and alert the operator with the
    // encode payload (the song URL, derived from the sound-tee "oto.html?s=KEY"
    // convention in the description) plus the ship-to address.
    if route == "manual" {
        let desc = {
            let conn = db.lock().unwrap();
            conn.query_row(
                "SELECT description_ja FROM catalog_products WHERE sku=?",
                rusqlite::params![&sku],
                |r| r.get::<_, String>(0),
            )
            .unwrap_or_default()
        };
        // catalog_products に kind 列は無い — SKU は `{BRAND}-{KIND}-{seed}` 形式
        // (insert_catalog_product) なので SKU で self-fulfilled hardware を判定。
        let is_device = sku.contains("-DEVICE-");
        let encode_url = desc
            .find("oto.html?s=")
            .map(|p| &desc[p + "oto.html?s=".len()..])
            .map(|rest| {
                rest.chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                    .collect::<String>()
            })
            .filter(|k| !k.is_empty())
            .map(|k| format!("https://mu.koe.live/oto.html?s={}", k))
            .unwrap_or_else(|| "(description に oto.html?s= キー無し → 手動確認)".to_string());

        record_order(&db, &session_id, &sku, amount_total, cust, shipping,
                     None, "manual_pending");

        let name = shipping["name"]
            .as_str()
            .or_else(|| cust["name"].as_str())
            .unwrap_or("");
        let ship_to = format!(
            "{} / {} {} {} {} {} {}",
            name,
            addr["line1"].as_str().unwrap_or(""),
            addr["line2"].as_str().unwrap_or(""),
            addr["city"].as_str().unwrap_or(""),
            state_code,
            addr["postal_code"].as_str().unwrap_or(""),
            country,
        );
        let detail = if is_device {
            "📦 ハードウェア発送 (3日以内目安)。".to_string()
        } else {
            format!("🔗 encode→ {}\n書込→ロック→封筒で発送。", encode_url)
        };
        let _ = crate::send_telegram_message(&format!(
            "📌 *manual order* ({})\nsku=`{}`\n👤🏠 {}\n💴 ¥{}\n{}",
            if is_device { "device/自社発送" } else { "NFC音コイン" },
            sku, ship_to, amount_total, detail
        ))
        .await;
        return;
    }

    // ── Digital route (event ticket / song) ──────────────────────────────
    // No physical fulfillment: take payment, mint a unique code, then email
    // the buyer their item — a QR (ticket → /t/:code shows VALID) or a
    // listen/download link (song). Affiliate commission was already applied
    // at the top of this fn (it is route-agnostic).
    if route == "digital" {
        let email = cust["email"].as_str().unwrap_or("").to_string();
        let name = cust["name"].as_str().unwrap_or("").to_string();
        match issue_digital(&db, &session_id, &sku, amount_total, &email, &name, "ticket_delivered").await {
            Ok(t) => {
                let _ = crate::send_telegram_message(&format!(
                    "✅ *digital sold*\nsku=`{}`\n👤 {} <{}>\n💴 ¥{}\n🔗 {}",
                    sku, name, email, amount_total, t.ticket_url
                )).await;
            }
            Err(e) => {
                tracing::error!("[catalog/digital] issue failed sku={} session={}: {}", sku, session_id, e);
                let _ = crate::send_telegram_message(&format!(
                    "🚨 *paid but NOT delivered*\nsku=`{}`\nemail=`{}`\nsession=`{}…`\n¥{}\nerror: {}\nAction: /admin/catalog/ticket_issue で手動再発行 or 返金。",
                    sku, email, session_id.chars().take(24).collect::<String>(), amount_total, e
                )).await;
            }
        }
        return;
    }

    // When a cross-sell add-on is present, `amount_total` is the WHOLE
    // session (main SKU + add-on). The add-on ships as its own Printful
    // item declaring its own retail_price (see the addon block below), so
    // the MAIN item must declare only the main SKU's price. Charging
    // `amount_total` here would make Printful's declared/customs value
    // double-count the add-on (main+addon on the main line, addon again on
    // its own line) — inflating the customer's import duty + packing slip.
    // JPY only: non-JPY add-on pricing is not used (see addon block), and
    // amount_total is in minor units for non-JPY, so we leave it untouched
    // there. Single-SKU orders deduct 0 → byte-identical to the old code.
    let addon_price_jpy_for_main: i64 = if currency == "jpy" {
        let addon_sku = session["metadata"]["catalog_addon_sku"].as_str().unwrap_or("");
        if addon_sku.is_empty() {
            0
        } else {
            let conn = db.lock().unwrap();
            conn.query_row(
                "SELECT retail_price_jpy FROM catalog_products WHERE sku=? AND is_active=1",
                rusqlite::params![addon_sku],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
        }
    } else {
        0
    };

    let retail_price = if currency == "jpy" {
        format!("{:.2}", (amount_total - addon_price_jpy_for_main).max(0) as f64)
    } else {
        format!("{:.2}", (amount_total as f64) / 100.0)
    };

    // Printful caps external_id at 32 chars; Stripe session id is ~66.
    // Last-32 keeps the unique tail intact for back-reference.
    let ext_id = if session_id.len() > 32 {
        session_id[session_id.len() - 32..].to_string()
    } else {
        session_id.clone()
    };

    // Build the MAIN item via the shared helper. For an existing
    // single-SKU order this produces byte-identical JSON to the previous
    // inline code: same session-derived retail_price, same size override,
    // same stitch_color / placement logic.
    let main_item = {
        let conn = db.lock().unwrap();
        build_printful_item(&conn, &sku, &retail_price, variant_override, false)
    };
    let Some(main_item) = main_item else {
        // Should not happen — we already confirmed the SKU exists and the
        // route is not contrado_uk. A None here means the route is not a
        // printful_* route (e.g. manual/digital/gelato/suzuri), which this
        // Printful POST path cannot fulfill. Record and bail rather than
        // silently dropping.
        tracing::warn!(
            "[catalog/fulfill] sku {} produced no Printful item (non-printful route?), session={}",
            sku, session_id
        );
        record_order(&db, &session_id, &sku, amount_total, cust, shipping,
                     None, "failed_no_item");
        // This is a PAID order the Printful path can't fulfill (manual/digital/
        // gelato/suzuri route reached this arm), and the retry cron does not
        // pick up 'failed_no_item' — retrying wouldn't help since the route
        // won't change. So it would sit silently. Alert the operator to refund
        // or hand-fulfill, mirroring the failed-fulfillment alert below.
        let _ = crate::send_telegram_message(&format!(
            "🚨 *paid order can't auto-fulfill* (failed_no_item)\n\
             sku=`{}`\nsession=`{}…`\namount=¥{}\n\
             The SKU's route is not Printful but it reached the Printful path. \
             Action: refund OR hand-fulfill. Not auto-retried.",
            sku,
            session_id.chars().take(24).collect::<String>(),
            amount_total
        ))
        .await;
        return;
    };
    let mut items: Vec<serde_json::Value> = vec![main_item];

    // Optional cross-sell add-on. Inert until a future UI passes
    // `?addon=<sku>` at checkout (which sets metadata.catalog_addon_sku).
    // The add-on is charged its OWN retail_price_jpy (Printful wants a
    // per-item retail_price, not the session total), single size, no size
    // override. If it is missing / inactive / non-printful route we skip
    // the item rather than fail the whole order (the main item is the
    // committed purchase). 2nd-item failures still surface in Printful's
    // response which is logged + recorded below.
    let addon_sku = session["metadata"]["catalog_addon_sku"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if !addon_sku.is_empty() {
        let addon_item = {
            let conn = db.lock().unwrap();
            // Format the add-on price the same way the main retail_price is
            // formatted for JPY (yen amount with two decimals).
            let addon_price_jpy: i64 = conn
                .query_row(
                    "SELECT retail_price_jpy FROM catalog_products WHERE sku=? AND is_active=1",
                    rusqlite::params![&addon_sku],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0);
            if addon_price_jpy > 0 {
                let addon_retail = if currency == "jpy" {
                    format!("{:.2}", addon_price_jpy as f64)
                } else {
                    // Non-JPY add-on pricing is not currently used; fall back
                    // to the same JPY-style format to stay defined.
                    format!("{:.2}", addon_price_jpy as f64)
                };
                build_printful_item(&conn, &addon_sku, &addon_retail, None, true)
            } else {
                None
            }
        };
        match addon_item {
            Some(it) => items.push(it),
            None => {
                // Customer-harm path: the add-on was a paid Stripe line_item
                // at checkout, so the customer was ALREADY charged for it. If
                // we skip it here (sticker went inactive / non-printful route
                // between checkout and webhook) they paid for something that
                // will never ship. A tracing::warn nobody watches is not
                // enough — fire the same operator alert we use for failed
                // fulfillment so a human can refund or hand-fulfill.
                tracing::warn!(
                    "[catalog/fulfill] addon sku {} skipped (missing/inactive/non-printful), session={}",
                    addon_sku, session_id
                );
                let _ = crate::send_telegram_message(&format!(
                    "⚠️ *add-on charged but NOT fulfilled*\n\
                     main sku=`{}`\nadd-on sku=`{}`\nsession=`{}…`\n\
                     The customer paid for this add-on at checkout but it was \
                     skipped at fulfillment (missing / inactive / non-Printful \
                     route). Action: refund the add-on amount OR hand-fulfill it.",
                    sku,
                    addon_sku,
                    session_id.chars().take(24).collect::<String>()
                ))
                .await;
            }
        }
    }

    let body = serde_json::json!({
        "recipient": {
            "name":         shipping["name"].as_str().or_else(|| cust["name"].as_str()).unwrap_or(""),
            "address1":     addr["line1"].as_str().unwrap_or(""),
            "address2":     addr["line2"].as_str().unwrap_or(""),
            "city":         addr["city"].as_str().unwrap_or(""),
            "state_code":   state_code,
            "country_code": country,
            "zip":          addr["postal_code"].as_str().unwrap_or(""),
            "email":        cust["email"].as_str().unwrap_or(""),
            "phone":        cust["phone"].as_str().unwrap_or(""),
        },
        "items": items,
        "external_id": ext_id,
    });

    let pf_key = env::var("PRINTFUL_API_KEY").unwrap_or_default();
    if pf_key.is_empty() {
        tracing::error!("[catalog/fulfill] PRINTFUL_API_KEY unset — recording failure");
        record_order(
            &db,
            &session_id,
            &sku,
            amount_total,
            cust,
            shipping,
            None,
            "failed_no_key",
        );
        return;
    }

    let resp = reqwest::Client::new()
        .post("https://api.printful.com/orders?confirm=true")
        .bearer_auth(&pf_key)
        .json(&body)
        .send()
        .await;

    match resp {
        Ok(r) => {
            let status = r.status();
            let text = r.text().await.unwrap_or_default();
            let pf_json: serde_json::Value =
                serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
            let pf_id = pf_json["result"]["id"]
                .as_i64()
                .map(|i| i.to_string())
                .or_else(|| pf_json["result"]["id"].as_str().map(String::from));
            let ok = status.is_success();
            tracing::info!(
                "[catalog/fulfill] printful {} sku={} session={} pf_id={:?}",
                status, sku, session_id, pf_id
            );
            record_order_full(
                &db,
                &session_id,
                &sku,
                amount_total,
                cust,
                shipping,
                pf_id.as_deref(),
                if ok { "submitted" } else { "failed" },
                Some(&text),
                if addon_sku.is_empty() { None } else { Some(addon_sku.as_str()) },
            );
            // Mirror into mu_purchases so vault holder gating + /100 counter
                // + community.numbers see catalog-route buyers too. Idempotent
                // on session_id. Skipped on failure so we don't claim a holder
                // who didn't actually pay-and-ship. 2026-05-29 incident: 4
                // ELEPOTE orders existed in catalog_orders but not mu_purchases.
            if ok {
                let email = cust["email"].as_str().unwrap_or("").to_lowercase();
                if !email.is_empty() {
                    let (cp_id, brand_name) = {
                        let conn = db.lock().unwrap();
                        conn.query_row(
                            "SELECT id, brand FROM catalog_products WHERE sku=?",
                            rusqlite::params![&sku],
                            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
                        ).unwrap_or((0, String::new()))
                    };
                    {
                        let conn = db.lock().unwrap();
                        let _ = conn.execute(
                            "INSERT OR IGNORE INTO mu_purchases
                               (email, product_id, brand, drop_num, session_id, amount_jpy,
                                created_at, printful_order_id, last_printful_status, last_status_at)
                             VALUES (?, ?, ?, 0, ?, ?, ?, ?, ?, ?)",
                            rusqlite::params![
                                email,
                                cp_id,
                                brand_name,
                                &session_id,
                                amount_total,
                                chrono_now_iso(),
                                pf_id.as_deref().unwrap_or(""),
                                "draft",
                                chrono_now_iso(),
                            ],
                        );
                    } // drop the SQLite lock before any await below

                    // MUON コレクター: Tシャツを累計3枚集めるごとに ¥2,000 の MU クレジット付与。
                    //   現金でなくクレジット = 再購入を促し原価より実コストが小さい / 期限なし。
                    //   冪等: マイルストン(muon_collect3,6,9…)ごとに mu_credit_ledger を1回だけ。
                    if kind_from_sku(&sku) == "tee" {
                        const MUON_REWARD_JPY: i64 = 2000;
                        const MUON_EVERY: i64 = 3;
                        let tee_count: i64 = {
                            let conn = db.lock().unwrap();
                            conn.query_row(
                                "SELECT COUNT(*) FROM mu_purchases p \
                                 JOIN catalog_products c ON c.id = p.product_id \
                                 WHERE LOWER(p.email) = ? AND c.kind = 'tee'",
                                rusqlite::params![email],
                                |r| r.get(0),
                            ).unwrap_or(0)
                        };
                        if tee_count > 0 && tee_count % MUON_EVERY == 0 {
                            let reason = format!("muon_collect{}", tee_count);
                            let granted = {
                                let conn = db.lock().unwrap();
                                let dup: bool = conn.query_row(
                                    "SELECT 1 FROM mu_credit_ledger WHERE email = ? AND reason = ? LIMIT 1",
                                    rusqlite::params![email, &reason],
                                    |_| Ok(()),
                                ).is_ok();
                                if dup { false }
                                else { crate::mu_credit_apply(&conn, &email, MUON_REWARD_JPY, &reason, Some(&session_id)) }
                            };
                            if granted {
                                tracing::info!("[muon] {} reached {} tees -> +JPY{} credit", email, tee_count, MUON_REWARD_JPY);
                                send_muon_reward_email(email.clone(), tee_count, MUON_REWARD_JPY).await;
                            }
                        }
                    }

                    // 古今ペイ連携: KOKONコラボ商品の購入で「焼肉古今」ポイントを付与。
                    // order_id=session_id で冪等(再送しても二重付与されない)。
                    if brand_name == "kokon" {
                        match (std::env::var("KOKON_PAY_GRANT_URL"),
                               std::env::var("KOKON_PAY_GRANT_SECRET")) {
                            (Ok(url), Ok(secret)) if !url.is_empty() && !secret.is_empty() => {
                                let body = serde_json::json!({
                                    "email": email.clone(),
                                    "order_id": session_id.clone(),
                                    "amount_yen": amount_total,
                                    "source": "mu",
                                });
                                match reqwest::Client::new()
                                    .post(&url)
                                    .header("X-Grant-Secret", secret)
                                    .json(&body)
                                    .send()
                                    .await
                                {
                                    Ok(r) => {
                                        let st = r.status();
                                        let t = r.text().await.unwrap_or_default();
                                        if st.is_success() {
                                            tracing::info!(
                                                "[kokon-pay] granted email={} order={} resp={}",
                                                email, session_id, t);
                                        } else {
                                            tracing::warn!(
                                                "[kokon-pay] grant failed status={} body={}", st, t);
                                        }
                                    }
                                    Err(e) => tracing::error!("[kokon-pay] grant net err: {}", e),
                                }
                            }
                            _ => tracing::warn!(
                                "[kokon-pay] KOKON_PAY_GRANT_URL/SECRET unset; skipped grant"),
                        }
                    }
                }
            }
            if !ok {
                // 再発防止 (2026-06-04): 入金済みなのに発送できない注文を「失敗のまま放置」
                // しない。Printful の 4xx(住所空欄・バリアント不正など)は再試行しても直らない
                // = 顧客の金だけ取った状態。これを検知したら **自動で Stripe 返金** し、
                // status='refunded' に落とす。5xx/ネットワーク等の一過性のみ /replay 待ちにする。
                let non_retryable = status.is_client_error(); // 4xx
                let mut refunded = false;
                if non_retryable {
                    if let Ok(skey) = std::env::var("STRIPE_SECRET_KEY") {
                        // checkout session → payment_intent
                        let pi_id: Option<String> = match reqwest::Client::new()
                            .get(format!("https://api.stripe.com/v1/checkout/sessions/{}", session_id))
                            .basic_auth(&skey, None::<&str>).send().await {
                            Ok(r) if r.status().is_success() => {
                                let j: serde_json::Value = r.json().await.unwrap_or_default();
                                j["payment_intent"].as_str().map(String::from)
                            }
                            _ => None,
                        };
                        if let Some(pi) = pi_id {
                            let rf = reqwest::Client::new()
                                .post("https://api.stripe.com/v1/refunds")
                                .basic_auth(&skey, None::<&str>)
                                .form(&[("payment_intent", pi.as_str()), ("reason", "requested_by_customer")])
                                .send().await;
                            refunded = matches!(rf, Ok(ref r) if r.status().is_success());
                            if refunded {
                                let conn = db.lock().unwrap();
                                let _ = conn.execute(
                                    "UPDATE catalog_orders SET status='refunded' WHERE stripe_session_id=?",
                                    rusqlite::params![&session_id],
                                );
                            }
                        }
                    }
                }
                let head = if refunded {
                    "✅ *fulfillment 4xx → AUTO-REFUNDED* (顧客に全額返金済・発送不可のため)"
                } else if non_retryable {
                    "🚨 *fulfillment FAILED (4xx) — 自動返金できず* 手動で返金してください"
                } else {
                    "🚨 *fulfillment FAILED (一過性)* — GET /admin/catalog/orders/<id>/replay?token= で再送"
                };
                let _ = crate::send_telegram_message(&format!(
                    "{}\nsku=`{}`\nsession=`{}…`\namount=¥{}\nprintful body (first 500):\n```\n{}\n```",
                    head,
                    sku,
                    session_id.chars().take(24).collect::<String>(),
                    amount_total,
                    text.chars().take(500).collect::<String>()
                ))
                .await;
            }
            // 2026-05-22: Founder hand-signed thank-you card removed from
            // PDP. The claim_and_notify_founder_card path is no longer
            // invoked for new orders. Historical claims stay in
            // catalog_founder_cards for fulfillment of past orders.
        }
        Err(e) => {
            tracing::error!("[catalog/fulfill] printful net err sku={} session={}: {}",
                sku, session_id, e);
            record_order(
                &db,
                &session_id,
                &sku,
                amount_total,
                cust,
                shipping,
                None,
                "failed_network",
            );
        }
    }
}

/// Place a store order via Contrado Helix API (`POST /helix/v1/orders/create`).
///
/// First-cut stub: probes the endpoint with a minimal payload, logs the
/// response, and records the attempt to catalog_orders for audit. Real
/// product / variant / shipping data wiring lands once we have known-good
/// product_id/variant_id pairs from the Contrado dashboard.
///
/// CONTRADO_API_KEY must be set; missing key → mark as not_attempted.
async fn fulfill_via_contrado(
    db: Db,
    session_id: &str,
    sku: &str,
    amount_total: i64,
    currency: &str,
) {
    let null = serde_json::Value::Null;
    let key = match std::env::var("CONTRADO_API_KEY") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            tracing::warn!(
                "[catalog/fulfill] CONTRADO_API_KEY unset — sku={} session={} not attempted",
                sku, session_id
            );
            record_order(&db, session_id, sku, amount_total,
                         &null, &null, None, "contrado_no_key");
            return;
        }
    };

    // Minimal payload — Contrado's StoreOrderRequestModel schema is not
    // fully wired yet, so this probe lets us learn the validation errors
    // and refine on the next pass. Reference field is our Stripe session id.
    let body = serde_json::json!({
        "reference":  session_id,
        "items":      [{"sku": sku, "quantity": 1}],
        "currency":   currency.to_uppercase(),
        "amountTotal": amount_total,
    });

    let resp = reqwest::Client::new()
        .post("https://api.contrado.app/helix/v1/orders/create")
        .header("X-API-KEY", &key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await;

    match resp {
        Ok(r) => {
            let code = r.status();
            let text = r.text().await.unwrap_or_default();
            let snippet = &text[..text.len().min(600)];
            tracing::info!(
                "[catalog/fulfill] contrado {} sku={} session={} body={}",
                code, sku, session_id, snippet
            );
            let outcome = if code.is_success() { "contrado_ok" } else { "contrado_fail" };
            let status = format!("{}_{}", outcome, code.as_u16());
            record_order(&db, session_id, sku, amount_total,
                         &null, &null, None, &status);
        }
        Err(e) => {
            tracing::error!(
                "[catalog/fulfill] contrado net err sku={} session={}: {}",
                sku, session_id, e
            );
            record_order(&db, session_id, sku, amount_total,
                         &null, &null, None, "contrado_net_err");
        }
    }
}

fn record_order(
    db: &Db,
    session_id: &str,
    sku: &str,
    amount: i64,
    cust: &serde_json::Value,
    shipping: &serde_json::Value,
    pf_id: Option<&str>,
    status: &str,
) {
    record_order_full(db, session_id, sku, amount, cust, shipping, pf_id, status, None, None);
}

/// First-100 founder-card flow. Idempotent on stripe_session_id (INSERT
/// OR IGNORE skips if the session already has a card). Picks the next
/// unused 1..100 number, writes the row, then fires both the customer
/// confirmation and the operator action-item via Resend.
async fn claim_and_notify_founder_card(
    db: &Db,
    session_id: &str,
    sku: &str,
    cust: &serde_json::Value,
    shipping: &serde_json::Value,
) {
    let email = cust["email"].as_str().unwrap_or("").to_string();
    if email.is_empty() {
        return;
    }
    let name = shipping["name"]
        .as_str()
        .or_else(|| cust["name"].as_str())
        .unwrap_or("")
        .to_string();
    let addr_json = serde_json::to_string(&shipping["address"]).unwrap_or_else(|_| "{}".into());

    // Atomically pick the next number. If 100 are already claimed, exit
    // quietly — the customer just gets the normal Printful confirmation
    // mail without a founder card.
    let number: Option<i64> = {
        let conn = db.lock().unwrap();
        // Idempotency: if this session already has a card, return it.
        if let Ok(existing) = conn.query_row(
            "SELECT number FROM catalog_founder_cards WHERE stripe_session_id=?",
            rusqlite::params![session_id],
            |r| r.get::<_, i64>(0),
        ) {
            Some(existing)
        } else {
            let used: i64 = conn
                .query_row("SELECT COUNT(*) FROM catalog_founder_cards", [], |r| r.get(0))
                .unwrap_or(0);
            if used >= 100 {
                None
            } else {
                let n = used + 1;
                let inserted = conn
                    .execute(
                        "INSERT OR IGNORE INTO catalog_founder_cards
                         (number, stripe_session_id, sku, customer_email,
                          customer_name, ship_address_json)
                         VALUES (?,?,?,?,?,?)",
                        rusqlite::params![n, session_id, sku, &email, &name, &addr_json],
                    )
                    .unwrap_or(0);
                if inserted > 0 {
                    Some(n)
                } else {
                    None
                }
            }
        }
    };
    let Some(num) = number else {
        return;
    };

    let resend_key = std::env::var("RESEND_API_KEY").unwrap_or_default();
    if resend_key.is_empty() {
        tracing::warn!("[catalog/founder] RESEND_API_KEY unset — card #{} claimed for {} but no mail sent", num, email);
        return;
    }
    let client = reqwest::Client::new();

    // 1. Customer email — "you are #X / 100".
    let cust_html = format!(
        r#"<div style="background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,'Helvetica Neue',Arial,sans-serif;padding:32px 0;margin:0">
<div style="max-width:560px;margin:0 auto;padding:0 32px">
<div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:24px">━◯━ MU</div>
<div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#ffd700;opacity:0.85;margin-bottom:8px">FOUNDER CARD CLAIMED</div>
<h2 style="font-size:20px;font-weight:500;line-height:1.4;margin:0 0 16px">あなたは <strong style="color:#ffd700">{num} / 100</strong> 番目の方です。</h2>
<p style="font-size:13px;line-height:1.9;opacity:0.78;margin:0 0 18px">
最初の 100 注文限定のお礼として、 濱田優貴 (MU 創業者) が手書きでサインしたサンクスカードを、
T シャツとは<strong>別便</strong>で日本ポストよりお送りします。 通常 1-2 週間でお手元に届きます。
</p>
<table style="width:100%;font-size:12px;line-height:1.8;border-collapse:collapse;margin:18px 0">
<tr><td style="opacity:0.5;width:35%;padding:4px 0">Card #</td><td style="padding:4px 0;color:#ffd700;font-weight:600">{num} / 100</td></tr>
<tr><td style="opacity:0.5;padding:4px 0">SKU</td><td style="padding:4px 0;font-family:monospace">{sku}</td></tr>
<tr><td style="opacity:0.5;padding:4px 0">送り先</td><td style="padding:4px 0">{name}</td></tr>
</table>
<p style="font-size:11px;line-height:1.85;opacity:0.55;margin:24px 0 0;border-top:1px solid #222;padding-top:18px">
T シャツ / ラッシュガード本体は Printful より別途海外発送 (7-14 日)。 サンクスカードは濱田より日本ポストで個別便発送。
お問い合わせ: <a href="mailto:info@enablerdao.com" style="color:#ffd700">info@enablerdao.com</a>
</p>
</div></div>"#,
        num = num, sku = html_text(sku), name = html_text(&name)
    );
    let cust_payload = serde_json::json!({
        "from": "MU Founder <noreply@wearmu.com>",
        "to": [email.clone()],
        "subject": format!("━◯━ Founder Card #{} / 100 — 濱田優貴 サイン入りカード", num),
        "html": cust_html,
    });
    let _ = client
        .post("https://api.resend.com/emails")
        .bearer_auth(&resend_key)
        .json(&cust_payload)
        .send()
        .await;

    // 2. Operator action-item — Yuki gets the address + number so he can
    // sign and post the card from his own mailbox.
    let op_html = format!(
        r#"<div style="font-family:monospace;font-size:13px;line-height:1.7;background:#fff;color:#000;padding:24px;max-width:560px;margin:0 auto">
<div style="font-size:14px;font-weight:700;color:#c00">ACTION: 手書きサンクスカード #{num}/100 をサイン → 投函</div>
<hr style="border:none;border-top:1px solid #ddd;margin:14px 0">
<table style="font-size:12px;line-height:1.8;border-collapse:collapse"><tbody>
<tr><td style="padding:2px 12px 2px 0;color:#666">Card #</td><td><strong>{num} / 100</strong></td></tr>
<tr><td style="padding:2px 12px 2px 0;color:#666">注文 (SKU)</td><td>{sku}</td></tr>
<tr><td style="padding:2px 12px 2px 0;color:#666">Stripe session</td><td>{sid}</td></tr>
<tr><td style="padding:2px 12px 2px 0;color:#666">顧客名</td><td>{name}</td></tr>
<tr><td style="padding:2px 12px 2px 0;color:#666">Email</td><td>{email}</td></tr>
<tr><td style="padding:2px 12px 2px 0;color:#666;vertical-align:top">配送先</td>
<td><pre style="margin:0;font-family:inherit;font-size:12px">{addr}</pre></td></tr>
</tbody></table>
<hr style="border:none;border-top:1px solid #ddd;margin:14px 0">
<p style="font-size:11.5px;color:#555;margin:0">
1) カードに 「ありがとう · MU · {num}/100 · 濱田優貴」 + 署名<br>
2) 配送先住所を封筒に書いて日本ポストへ<br>
3) ↓ をクリックして mailed_at を記録 (後日実装予定)<br>
<a href="https://wearmu.com/admin/catalog/founder/{num}/mark_mailed">→ mark mailed #{num}</a>
</p>
</div>"#,
        num = num,
        sku = html_text(sku),
        sid = html_text(session_id),
        name = html_text(&name),
        email = html_text(&email),
        addr = html_text(&addr_json),
    );
    let op_to = std::env::var("FOUNDER_CARD_OPERATOR_TO")
        .unwrap_or_else(|_| "mail@yukihamada.jp".into());
    let op_payload = serde_json::json!({
        "from": "MU Founder Queue <noreply@wearmu.com>",
        "to": [op_to],
        "subject": format!("[ACTION] Founder Card #{}/100 — sign + post", num),
        "html": op_html,
    });
    let _ = client
        .post("https://api.resend.com/emails")
        .bearer_auth(&resend_key)
        .json(&op_payload)
        .send()
        .await;

    tracing::info!("[catalog/founder] claimed #{}/100 for {} session={}", num, email, session_id);
}

fn record_order_full(
    db: &Db,
    session_id: &str,
    sku: &str,
    amount: i64,
    cust: &serde_json::Value,
    shipping: &serde_json::Value,
    pf_id: Option<&str>,
    status: &str,
    pf_response: Option<&str>,
    addon_sku: Option<&str>,
) {
    let email = cust["email"].as_str().unwrap_or("");
    let name = shipping["name"]
        .as_str()
        .or_else(|| cust["name"].as_str())
        .unwrap_or("");
    let addr_json =
        serde_json::to_string(&shipping["address"]).unwrap_or_else(|_| "{}".to_string());
    let pf_resp_trimmed = pf_response
        .map(|s| s.chars().take(4900).collect::<String>())
        .unwrap_or_default();
    let conn = db.lock().unwrap();
    // Preserve affiliate attribution across the REPLACE: apply_affiliate()
    // and stamp_ticket_code run on the reserved row BEFORE this final write,
    // and INSERT OR REPLACE would otherwise reset those columns to default.
    let (existing_ref, existing_comm, existing_ticket): (Option<String>, i64, Option<String>) = conn
        .query_row(
            "SELECT referrer_code, commission_jpy, ticket_code FROM catalog_orders WHERE stripe_session_id=?",
            rusqlite::params![session_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap_or((None, 0, None));
    let _ = conn.execute(
        "INSERT OR REPLACE INTO catalog_orders
         (stripe_session_id, sku, amount_jpy, customer_email, customer_name,
          shipping_address_json, printful_order_id, printful_response_json, status,
          addon_sku, referrer_code, commission_jpy, ticket_code)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
        rusqlite::params![
            session_id,
            sku,
            amount,
            email,
            name,
            addr_json,
            pf_id,
            pf_resp_trimmed,
            status,
            addon_sku,
            existing_ref,
            existing_comm,
            existing_ticket,
        ],
    );
}

// ─── Digital event tickets ────────────────────────────────────────────

/// Deterministic, unguessable, unique-per-order ticket code: 16 hex chars
/// from SHA-256(session_id). Stable across retries (same session → same
/// code, so an at-least-once webhook never mints a 2nd code).
fn ticket_code(session_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(session_id.as_bytes());
    h.finalize().iter().take(8).map(|b| format!("{:02x}", b)).collect()
}

/// Render a scannable QR PNG (dark modules on a white quiet-zone) for `url`.
fn ticket_qr_png(url: &str) -> Option<Vec<u8>> {
    use qrcodegen::{QrCode, QrCodeEcc};
    let qr = QrCode::encode_text(url, QrCodeEcc::Medium).ok()?;
    let n = qr.size() as usize;
    let border: usize = 4; // quiet zone (spec minimum) so scanners lock on
    let dim = n + border * 2;
    let scale: usize = (1024 / dim.max(1)).max(6);
    let img_dim = dim * scale;
    // RGB charcoal-on-white = maximum scanner contrast in email + print.
    let mut rgb = vec![0xffu8; img_dim * img_dim * 3];
    for y in 0..n {
        for x in 0..n {
            if qr.get_module(x as i32, y as i32) {
                let px = (x + border) * scale;
                let py = (y + border) * scale;
                for dy in 0..scale {
                    for dx in 0..scale {
                        let i = ((py + dy) * img_dim + (px + dx)) * 3;
                        rgb[i] = 0x10; rgb[i + 1] = 0x10; rgb[i + 2] = 0x10;
                    }
                }
            }
        }
    }
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, img_dim as u32, img_dim as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().ok()?;
        w.write_image_data(&rgb).ok()?;
    }
    Some(buf)
}

/// Inline data-URI QR for self-contained HTML (the /t/:code face).
fn ticket_qr_data_uri(url: &str) -> Option<String> {
    use base64::Engine;
    let png = ticket_qr_png(url)?;
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&png)))
}

struct TicketIssued {
    code: String,
    ticket_url: String,
    qr_url: Option<String>,
}

/// Core digital-goods issuance (event ticket or song), shared by the
/// paid-webhook path and the admin comp/resend path: mints (or re-uses) the
/// code, records the order + stamps the code, and emails the buyer — a QR
/// (ticket) or a listen/download link (song). Idempotent on `session_id`.
async fn issue_digital(
    db: &Db,
    session_id: &str,
    sku: &str,
    amount: i64,
    email: &str,
    name: &str,
    status: &str,
) -> Result<TicketIssued, String> {
    let email = email.trim();
    if email.is_empty() {
        return Err("no buyer email on the session".into());
    }
    let code = ticket_code(session_id);
    let base = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    let base = base.trim_end_matches('/');
    let ticket_url = format!("{}/t/{}", base, code);

    // Record the paid order, THEN stamp the ticket_code: record_order_full()
    // does INSERT OR REPLACE (which has no ticket_code column), so the
    // stamp must follow the REPLACE or it would be wiped.
    let cust = serde_json::json!({ "email": email, "name": name });
    let empty = serde_json::json!({});
    record_order(db, session_id, sku, amount, &cust, &empty, None, status);
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "UPDATE catalog_orders SET ticket_code=? WHERE stripe_session_id=?",
            rusqlite::params![&code, session_id],
        );
    }

    // Product name / blurb / song audio for the delivery email. kind decides
    // whether this is a ticket (QR) or a song (listen+download link).
    let (label, desc, meta_json) = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT label, description_ja, meta_json FROM catalog_products WHERE sku=?",
            rusqlite::params![sku],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?)),
        )
        .unwrap_or_else(|_| (sku.to_string(), String::new(), None))
    };
    let is_song = kind_from_sku(sku) == "song";
    let audio_url: Option<String> = meta_json
        .as_deref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|v| v.get("audio_url").and_then(|a| a.as_str()).map(|s| s.to_string()));

    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    if resend_key.is_empty() {
        return Err("RESEND_API_KEY unset — order recorded but email not sent".into());
    }

    // Tickets host a QR on R2 (email clients render an https <img> reliably);
    // songs don't need one.
    let qr_url = if is_song {
        None
    } else {
        match ticket_qr_png(&ticket_url) {
            Some(bytes) => crate::store_r2_bytes(&format!("tickets/{}.png", code), &bytes, "image/png").await,
            None => None,
        }
    };

    let (subject, body_block, from_name) = if is_song {
        let listen = audio_url.as_deref().unwrap_or(&ticket_url);
        (
            format!("🎵 {} — ダウンロード / Song", label),
            format!(
                "<div style=\"text-align:center;margin:24px 0\">\
                 <a href=\"{stream}\" style=\"display:inline-block;background:#e6c449;color:#0a0a0a;\
                 text-decoration:none;font-weight:700;font-size:15px;padding:14px 28px;border-radius:99px\">▶ 視聴 / ダウンロード</a></div>\
                 <p style=\"font-size:12px;text-align:center;margin:0 0 8px;opacity:.7\">\
                 リンク: <a href=\"{listen}\" style=\"color:#e6c449\">{listen}</a></p>",
                stream = html_text(&ticket_url),
                listen = html_text(listen),
            ),
            "MU Music <noreply@wearmu.com>",
        )
    } else {
        let qr_block = qr_url
            .as_ref()
            .map(|u| format!(
                "<div style=\"text-align:center;margin:24px 0\">\
                 <img src=\"{}\" alt=\"QR\" width=\"240\" height=\"240\" \
                 style=\"background:#fff;border-radius:8px;padding:12px\"></div>",
                html_text(u),
            ))
            .unwrap_or_default();
        (
            format!("🎟️ {} — 参加券 / Ticket", label),
            format!(
                "{qr_block}\
                 <p style=\"font-size:13px;line-height:1.8;text-align:center;margin:0 0 8px\">会場でこの QR を提示してください。</p>\
                 <p style=\"font-size:12px;text-align:center;margin:0 0 18px\"><a href=\"{ticket_url}\" style=\"color:#e6c449\">{ticket_url}</a></p>",
                qr_block = qr_block,
                ticket_url = html_text(&ticket_url),
            ),
            "MU Tickets <noreply@wearmu.com>",
        )
    };
    let kicker = if is_song { "YOUR SONG" } else { "YOUR TICKET" };
    let cust_html = format!(
        r#"<div style="background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,'Helvetica Neue',Arial,sans-serif;padding:32px 0;margin:0">
<div style="max-width:560px;margin:0 auto;padding:0 32px">
<div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:24px">━◯━ MU</div>
<div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#e6c449;margin-bottom:8px">{kicker}</div>
<h2 style="font-size:20px;font-weight:500;line-height:1.4;margin:0 0 8px">{label}</h2>
<p style="font-size:13px;line-height:1.9;opacity:0.78;margin:0 0 4px">{desc}</p>
{body_block}
<table style="width:100%;font-size:12px;line-height:1.8;border-collapse:collapse;margin:18px 0">
<tr><td style="opacity:0.5;width:35%;padding:4px 0">ID</td><td style="padding:4px 0;font-family:monospace;color:#e6c449">{code}</td></tr>
<tr><td style="opacity:0.5;padding:4px 0">お名前</td><td style="padding:4px 0">{name}</td></tr>
</table>
<p style="font-size:11px;line-height:1.85;opacity:0.55;margin:24px 0 0;border-top:1px solid #222;padding-top:18px">
デジタル商品 · 物理発送はありません。 お問い合わせ: <a href="mailto:info@enablerdao.com" style="color:#e6c449">info@enablerdao.com</a>
</p>
</div></div>"#,
        kicker = kicker,
        label = html_text(&label),
        desc = html_text(&desc),
        body_block = body_block,
        code = html_text(&code),
        name = html_text(name),
    );
    let payload = serde_json::json!({
        "from": from_name,
        "to": [email],
        "subject": subject,
        "html": cust_html,
    });
    let resp = reqwest::Client::new()
        .post("https://api.resend.com/emails")
        .bearer_auth(&resend_key)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("resend network: {}", e))?;
    if !resp.status().is_success() {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return Err(format!("resend {}: {}", s, t.chars().take(200).collect::<String>()));
    }
    Ok(TicketIssued { code, ticket_url, qr_url })
}

/// GET /t/:code — public face of a digital purchase. For a ticket it shows
/// the event, holder, a VALID stamp and the QR (the QR opens this page); for
/// a song it shows an audio player + download. noindex.
pub async fn ticket_view(State(db): State<Db>, Path(code): Path<String>) -> Response {
    let code: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
    let row: Option<(String, String, String, Option<String>)> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT o.sku, COALESCE(o.customer_name,''), COALESCE(p.label, o.sku), p.meta_json
             FROM catalog_orders o LEFT JOIN catalog_products p ON p.sku=o.sku
             WHERE o.ticket_code=?",
            rusqlite::params![&code],
            |r| Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            )),
        )
        .ok()
    };
    let Some((sku, name, label, meta_json)) = row else {
        return (
            StatusCode::NOT_FOUND,
            Html("<!doctype html><meta charset=utf-8><meta name=robots content=noindex>\
                  <title>無効なリンク — MU</title>\
                  <body style=\"background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,sans-serif;text-align:center;padding:80px 20px\">\
                  <h1 style=\"font-weight:500\">見つかりません</h1>\
                  <p style=\"opacity:.6\">このリンクは無効です。</p></body>".to_string()),
        )
            .into_response();
    };
    let base = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    let ticket_url = format!("{}/t/{}", base.trim_end_matches('/'), code);
    let is_song = kind_from_sku(&sku) == "song";

    let (badge, hero, footer) = if is_song {
        let audio_url = meta_json
            .as_deref()
            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
            .and_then(|v| v.get("audio_url").and_then(|a| a.as_str()).map(|s| s.to_string()))
            .unwrap_or_default();
        let player = if audio_url.is_empty() {
            "<p style=\"opacity:.6\">準備中です。少し時間をおいて再度お試しください。</p>".to_string()
        } else {
            format!(
                "<audio controls preload=\"none\" src=\"{u}\" style=\"width:100%;margin:8px 0 14px\"></audio>\
                 <div><a href=\"{u}\" download style=\"display:inline-block;background:#e6c449;color:#0a0a0a;\
                 text-decoration:none;font-weight:700;font-size:14px;padding:12px 24px;border-radius:99px\">⬇ ダウンロード</a></div>",
                u = html_text(&audio_url),
            )
        };
        (
            "<div style=\"display:inline-block;font-size:11px;letter-spacing:0.3em;color:#0a0a0a;background:#e6c449;padding:4px 12px;border-radius:99px;font-weight:700\">♫ SONG</div>".to_string(),
            player,
            "あなたの楽曲です。 視聴・ダウンロードはこのページから。 デジタル商品・物理発送はありません。",
        )
    } else {
        let qr_img = ticket_qr_data_uri(&ticket_url).unwrap_or_default();
        (
            "<div style=\"display:inline-block;font-size:11px;letter-spacing:0.3em;color:#0a0a0a;background:#3ddc84;padding:4px 12px;border-radius:99px;font-weight:700\">✓ VALID</div>".to_string(),
            format!("<div style=\"background:#fff;border-radius:12px;padding:16px;display:inline-block;margin:16px 0\"><img src=\"{}\" alt=\"QR\" width=\"240\" height=\"240\" style=\"display:block\"></div>", qr_img),
            "受付でこの画面（QR）をご提示ください。 デジタル参加券・物理発送はありません。",
        )
    };
    Html(format!(
        r#"<!doctype html><html lang=ja><head><meta charset=utf-8>
<meta name=viewport content="width=device-width,initial-scale=1">
<meta name=robots content="noindex,nofollow">
<title>{label} — MU</title></head>
<body style="background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,'Helvetica Neue',Arial,sans-serif;margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;padding:24px">
<div style="max-width:420px;width:100%;text-align:center">
<div style="font-size:20px;font-weight:700;letter-spacing:0.45em;margin-bottom:18px">━◯━ MU</div>
{badge}
<h1 style="font-size:22px;font-weight:500;line-height:1.4;margin:18px 0 6px">{label}</h1>
{hero}
<table style="width:100%;font-size:13px;line-height:1.9;border-collapse:collapse;text-align:left;margin-top:8px">
<tr><td style="opacity:0.5;width:35%;padding:4px 0">お名前</td><td style="padding:4px 0">{name}</td></tr>
<tr><td style="opacity:0.5;padding:4px 0">ID</td><td style="padding:4px 0;font-family:monospace;color:#e6c449">{code}</td></tr>
</table>
<p style="font-size:11px;opacity:0.45;margin-top:24px;border-top:1px solid #222;padding-top:16px">{footer}</p>
</div></body></html>"#,
        label = html_text(&label),
        badge = badge,
        hero = hero,
        name = html_text(&name),
        code = html_text(&code),
        footer = footer,
    ))
    .into_response()
}

#[derive(Deserialize)]
pub struct TicketIssueQuery {
    pub token: String,
    pub sku: String,
    pub email: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// GET /admin/catalog/ticket_issue?token=&sku=&email=&name= — issue a
/// COMP ticket (no payment) for a digital-ticket SKU. Counts against the
/// capacity like a paid seat. Doubles as the end-to-end check for the
/// QR + R2 + email pipeline. Admin-token gated.
pub async fn admin_ticket_issue(State(db): State<Db>, Query(q): Query<TicketIssueQuery>) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let route: Option<String> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(fulfillment_route,'') FROM catalog_products WHERE sku=?",
            rusqlite::params![&q.sku],
            |r| r.get(0),
        )
        .ok()
    };
    match route.as_deref() {
        Some("digital") => {}
        Some(_) => return (StatusCode::BAD_REQUEST, "sku is not a digital ticket").into_response(),
        None => return (StatusCode::NOT_FOUND, "sku not found").into_response(),
    }
    let session_id = format!("comp_{}_{:08x}", q.sku, rand::random::<u32>());
    let name = q.name.clone().unwrap_or_default();
    match issue_digital(&db, &session_id, &q.sku, 0, &q.email, &name, "ticket_comp").await {
        Ok(t) => axum::Json(serde_json::json!({
            "ok": true, "code": t.code, "ticket_url": t.ticket_url,
            "qr_url": t.qr_url, "emailed_to": q.email,
        }))
        .into_response(),
        Err(e) => axum::Json(serde_json::json!({ "ok": false, "error": e })).into_response(),
    }
}

// ─── Affiliate commission ─────────────────────────────────────────────

/// Credit an affiliate referrer for a paid order. Reads `referrer_code`
/// from the Stripe session metadata (set by shop_checkout from ?ref= or the
/// mu_ref cookie), resolves the owner via `mu_referrals.owner_email`, and
/// writes the commission to `mu_credit_ledger` (the payout source of truth)
/// + the `mu_referrals` counters + the order's audit columns. Commission %
/// is `catalog_brands.config_json.affiliate_pct` (default 10, capped 50).
/// No-ops on: missing/unregistered code, self-referral, non-JPY, or a
/// commission already booked for this session (idempotent on session_id).
async fn apply_affiliate(db: &Db, session_id: &str, session: &serde_json::Value, sku: &str, amount: i64) {
    let code = match session["metadata"]["referrer_code"].as_str().map(|c| c.trim().to_uppercase()) {
        Some(c) if c.len() >= 4 => c,
        _ => return,
    };
    if amount <= 0 || session["currency"].as_str().unwrap_or("jpy").to_lowercase() != "jpy" {
        return;
    }
    let buyer_email = session["customer_details"]["email"].as_str().unwrap_or("").to_lowercase();
    let conn = db.lock().unwrap();

    // Stamp the code on the order regardless (analytics), even when it earns
    // no commission below.
    let _ = conn.execute(
        "UPDATE catalog_orders SET referrer_code=? WHERE stripe_session_id=?",
        rusqlite::params![&code, session_id],
    );

    let owner: Option<String> = conn
        .query_row("SELECT owner_email FROM mu_referrals WHERE code=?", rusqlite::params![&code], |r| r.get(0))
        .ok()
        .flatten()
        .filter(|o: &String| !o.is_empty());
    let Some(owner) = owner else { return };          // unregistered code → no commission
    if !buyer_email.is_empty() && buyer_email == owner.to_lowercase() {
        return; // self-referral
    }

    // Idempotency: a commission already booked for this session?
    let already: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mu_credit_ledger WHERE ref_id=? AND reason LIKE 'affiliate:%'",
            rusqlite::params![session_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if already > 0 {
        return;
    }

    let brand: String = conn
        .query_row("SELECT brand FROM catalog_products WHERE sku=?", rusqlite::params![sku], |r| r.get(0))
        .unwrap_or_default();
    let pct = conn
        .query_row(
            "SELECT json_extract(config_json,'$.affiliate_pct') FROM catalog_brands WHERE slug=?",
            rusqlite::params![&brand],
            |r| r.get::<_, Option<i64>>(0),
        )
        .ok()
        .flatten()
        .unwrap_or(10)
        .clamp(0, 50);
    let commission = (amount * pct / 100).max(0);
    if commission <= 0 {
        return;
    }
    let reason = format!("affiliate:{}:{}", code, sku);
    crate::mu_credit_apply(&conn, &owner, commission, &reason, Some(session_id));
    let _ = conn.execute(
        "UPDATE mu_referrals SET uses = uses + 1, credit_jpy = credit_jpy + ? WHERE code=?",
        rusqlite::params![commission, &code],
    );
    let _ = conn.execute(
        "UPDATE catalog_orders SET commission_jpy=? WHERE stripe_session_id=?",
        rusqlite::params![commission, session_id],
    );
    tracing::info!("[catalog/affiliate] {} earned ¥{} ({}%) on {} via {}", owner, commission, pct, sku, code);
}

// ─── Helpers ──────────────────────────────────────────────────────────

struct ProductRow {
    sku: String,
    brand: String,
    desc: String,
    price: i64,
    img: Option<String>,
    sold: i64,
    /// song products: audio_url from meta_json, for the ▶ 試聴 card button.
    audio: Option<String>,
}

/// SQL fragment: mockup_url_external, but with Printful's ephemeral presigned
/// upload URLs (printful-upload.s3…/tmp/… — expire in ~24h, then 403 and the
/// shop shows white tiles) treated as NULL so COALESCE falls through to
/// mockup_main_file. Mirrors persist_mockup_if_temporary()'s is_temp check.
const MOCKUP_EXT_LIVE: &str = "CASE WHEN mockup_url_external LIKE 'https://printful-upload.s3%' \
       OR mockup_url_external LIKE '%/tmp/%' \
     THEN NULL ELSE mockup_url_external END";

/// GET /feed/google.tsv — Google Merchant Center 商品フィード（無料リスティング用）。
/// live + 実画像 (MOCKUP_EXT_LIVE) + 価格>0 の物理商品のみ。digital kind
/// (song / event_ticket) は GMC の物販対象外なので除外。フォーマットは GMC の
/// tab-delimited 仕様 (1行目=属性ヘッダ)。Merchant Center 側には
/// 「スケジュール取得」でこの URL を登録する。
pub async fn google_merchant_feed(State(db): State<Db>) -> Response {
    let rows: Vec<(String, String, i64, String)> = {
        let conn = db.lock().unwrap();
        let sql = format!(
            "SELECT sku, description_ja, retail_price_jpy, {ext}
             FROM catalog_products
             WHERE is_active=1 AND status='live' AND retail_price_jpy > 0
               AND COALESCE({ext}, '') != ''
             ORDER BY sku",
            ext = MOCKUP_EXT_LIVE
        );
        conn.prepare(&sql)
            .ok()
            .and_then(|mut s| {
                s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                    .ok()
                    .map(|it| it.filter_map(|r| r.ok()).collect())
            })
            .unwrap_or_default()
    };
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ").trim().to_string();
    let mut out =
        String::from("id\ttitle\tdescription\tlink\timage_link\tavailability\tprice\tcondition\tbrand\n");
    for (sku, desc, price, img) in rows {
        if matches!(kind_from_sku(&sku), "song" | "event_ticket") {
            continue;
        }
        let desc_clean = clean(&desc);
        let title: String = {
            let c: Vec<char> = desc_clean.chars().collect();
            if c.len() > 140 { c[..140].iter().collect() } else { desc_clean.clone() }
        };
        out.push_str(&format!(
            "{sku}\t{title}\t{desc}\thttps://wearmu.com/shop/{sku_enc}\t{img}\tin_stock\t{price} JPY\tnew\tMU\n",
            sku = sku,
            title = title,
            desc = desc_clean,
            sku_enc = urlencoding::encode(&sku),
            img = img,
            price = price,
        ));
    }
    (
        [(axum::http::header::CONTENT_TYPE, "text/tab-separated-values; charset=utf-8")],
        out,
    )
        .into_response()
}

fn list_products_paged(
    conn: &rusqlite::Connection,
    brand: Option<&str>,
    limit: i64,
    offset: i64,
    sort: &str,
    kind_sql: &str,
    q_pat: Option<&str>,
) -> Vec<ProductRow> {
    // Secondary ORDER BY per sort key. The mockup-first clause always leads so
    // SKUs with broken/stale images stay at the back regardless of sort.
    // `sort` is whitelisted in shop_index — never interpolate user input here.
    let order_tail = match sort {
        "new" => "created_at DESC, sku".to_string(),
        "price_asc" => "retail_price_jpy ASC, sku".to_string(),
        "price_desc" => "retail_price_jpy DESC, sku".to_string(),
        // Default (人気順): 看板 (meta_json.featured=true, 人力キュレーション) を
        // 最前列に固定し、ステッカーをアパレルの後ろへ降格 — ¥480 ステッカーが
        // 「店の顔」になる問題への直接対応。価格/新着ソートには適用しない。
        _ => r#"(COALESCE(meta_json,'') LIKE '%"featured":true%') DESC,
                      (sku NOT LIKE '%STICKER%') DESC,
                      (SELECT COUNT(*) FROM catalog_orders o2 WHERE o2.sku=catalog_products.sku AND o2.status='submitted') DESC,
                      sort_order, sku"#.to_string(),
    };
    // brand + kind + q を shop_filter_sql で組み立て、bind 値の後ろに limit/offset を足す。
    let (where_sql, binds) = shop_filter_sql(brand, kind_sql, q_pat);
    // 6th column = real sold count (status='submitted') for the social-proof
    // badge, derived per-row via correlated subquery (gated in render_card).
    let sql = format!(
        "SELECT sku, brand, description_ja, retail_price_jpy,
                COALESCE({ext}, mockup_main_file),
                (SELECT COUNT(*) FROM catalog_orders o WHERE o.sku=catalog_products.sku AND o.status='submitted'),
                meta_json
         FROM catalog_products
         WHERE {where_sql}
         ORDER BY (COALESCE({ext}, '') != '') DESC,
                  {tail}
         LIMIT ? OFFSET ?",
        ext = MOCKUP_EXT_LIVE, where_sql = where_sql, tail = order_tail);
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mapper = |r: &rusqlite::Row| {
        let meta: Option<String> = r.get(6)?;
        let audio = meta
            .as_deref()
            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
            .and_then(|v| v.get("audio_url").and_then(|a| a.as_str()).map(|s| s.to_string()));
        Ok(ProductRow {
            sku: r.get(0)?, brand: r.get(1)?, desc: r.get(2)?,
            price: r.get(3)?, img: r.get(4)?, sold: r.get(5)?,
            audio,
        })
    };
    // params = [binds...] + limit + offset. limit/offset は i64 なので別 vec で連結。
    let mut params: Vec<Box<dyn rusqlite::ToSql>> =
        binds.into_iter().map(|s| Box::new(s) as Box<dyn rusqlite::ToSql>).collect();
    params.push(Box::new(limit));
    params.push(Box::new(offset));
    stmt.query_map(rusqlite::params_from_iter(params.iter().map(|b| b.as_ref())), mapper)
        .ok()
        .map(|it| it.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

#[allow(dead_code)]
fn list_products(
    conn: &rusqlite::Connection,
    brand: Option<&str>,
    limit: i64,
) -> Vec<ProductRow> {
    // Sort order rationale:
    //   1. brand='auto' first — autonomous-engine fresh designs surface
    //      ahead of legacy merch-bridge SKUs (otherwise they're buried
    //      behind 1,500+ catalog SKUs with sort_order 1-14).
    //   2. SKUs with a WORKING external mockup URL next — merch-bridge
    //      shipped DB rows pointing at /static/collections/bjj/*.jpg
    //      paths where the file doesn't exist (989 of 1,073 BJJ SKUs).
    //      Those render as broken images on /shop. Filtering them out
    //      entirely would drop ¾ of the catalog, so we just sort them
    //      to the end where the img onerror handler in render_card()
    //      swaps to the ━◯━ brand mark fallback.
    //   3. sort_order, sku for stability.
    let (sql, has_brand) = if brand.is_some() {
        (
            "SELECT sku, brand, description_ja, retail_price_jpy,
                    COALESCE(mockup_url_external, mockup_main_file)
             FROM catalog_products
             WHERE is_active=1 AND brand=?
             ORDER BY (mockup_url_external IS NOT NULL AND mockup_url_external != '') DESC,
                      (SELECT COUNT(*) FROM catalog_orders o2 WHERE o2.sku=catalog_products.sku AND o2.status='submitted') DESC,
                      sort_order, sku
             LIMIT ?",
            true,
        )
    } else {
        (
            "SELECT sku, brand, description_ja, retail_price_jpy,
                    COALESCE(mockup_url_external, mockup_main_file)
             FROM catalog_products
             WHERE is_active=1
             ORDER BY (mockup_url_external IS NOT NULL AND mockup_url_external != '') DESC,
                      (SELECT COUNT(*) FROM catalog_orders o2 WHERE o2.sku=catalog_products.sku AND o2.status='submitted') DESC,
                      sort_order, sku
             LIMIT ?",
            false,
        )
    };
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mapper = |r: &rusqlite::Row| {
        Ok(ProductRow {
            sku: r.get(0)?,
            brand: r.get(1)?,
            desc: r.get(2)?,
            price: r.get(3)?,
            img: r.get(4)?,
            sold: 0, // unused path (dead_code); badge only flows via list_products_paged
            audio: None,
        })
    };
    let rows: Vec<ProductRow> = if has_brand {
        stmt.query_map(rusqlite::params![brand.unwrap(), limit], mapper)
            .ok()
            .map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    } else {
        stmt.query_map(rusqlite::params![limit], mapper)
            .ok()
            .map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    };
    rows
}

fn render_card(p: &ProductRow, pos: usize) -> String {
    let img = p
        .img
        .clone()
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.starts_with("http") {
                s
            } else {
                format!("https://merch.wearmu.com{}", s)
            }
        })
        .unwrap_or_else(|| "/static/designs/marker_zero.png".to_string());
    // onerror fallback: if the merch-bridge mockup_main_file 404s (989 of
    // 1,073 BJJ SKUs have stale references), swap to the ━◯━ brand mark
    // so the grid never shows a broken-image icon. The fallback strips the
    // onerror after one swap so a broken fallback doesn't loop forever.
    // Social-proof badge: real sold count, gated at SOLD_BADGE_MIN so a
    // low-volume SKU never shows 0/1. Self-contained inline style (no edit to
    // the shop_index <style> block needed).
    let sold_badge = if p.sold >= SOLD_BADGE_MIN {
        format!(
            r##"<span class="sold" style="position:absolute;top:8px;left:8px;background:rgba(0,0,0,0.72);color:#f5f5f0;font-size:10px;letter-spacing:.04em;padding:3px 7px;border-radius:999px;backdrop-filter:blur(4px)">{n}着 販売</span>"##,
            n = p.sold
        )
    } else {
        String::new()
    };
    // 一覧でも試聴: desc に oto.html?s=KEY があればミニ▶(涼介FB: 聴き比べ→まとめ買い)
    let listen_mini = if let Some(pos) = p.desc.find("oto.html?s=") {
        let key: String = p.desc[pos + "oto.html?s=".len()..].chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
        if key.is_empty() { String::new() } else {
            format!(r##"<button class="cardplay" data-key="{k}" aria-label="試聴" onclick="muPlay(event,this)">▶</button>"##, k = html_attr(&key))
        }
    } else { String::new() };
    // kind=song: play the meta_json audio_url directly from the card (試聴).
    let listen_song = match &p.audio {
        Some(au) if !au.is_empty() => format!(
            r##"<button class="cardplay" data-src="{s}" aria-label="試聴" onclick="muPlay(event,this)">▶</button>"##,
            s = html_attr(au)
        ),
        _ => String::new(),
    };
    // data-funnel: shop_card + grid position (0-based, page-local) so the
    // analytics funnel can split /shop→PDP CTR by card rank (above/below fold).
    format!(
        r##"<a class="card" href="/shop/{sku_enc}" data-funnel="cta_click" data-funnel-cta="shop_card" data-funnel-pos="{pos}"><span class="img" style="position:relative;display:block">{sold_badge}{listen_mini}{listen_song}<img src="{img}" alt="" loading="lazy" onerror="this.onerror=null;this.src='/static/designs/marker_zero.png';this.style.objectFit='contain';this.style.background='#0a0a0a';this.style.padding='28px'"></span><span class="body"><span class="brand">{brand}</span><span class="name">{name}</span><span class="price">¥{price}</span></span></a>"##,
        pos = pos,
        sku_enc = urlencoding::encode(&p.sku),
        sold_badge = sold_badge,
        listen_mini = listen_mini,
        listen_song = listen_song,
        img = html_attr(&img),
        brand = html_text(&p.brand),
        name = html_text(&p.desc),
        price = format_jpy(p.price),
    )
}

fn format_jpy(n: i64) -> String {
    // 4900 → "4,900"
    let s = n.abs().to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    let len = bytes.len();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    if n < 0 {
        let mut r = String::with_capacity(out.len() + 1);
        r.push('-');
        r.push_str(&out);
        r
    } else {
        out
    }
}

fn html_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn html_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

// JP prefecture / US state normalization for Printful state_code.
// Mirrors merch-bridge/app.py normalize_state_code (the Python file we're
// retiring). Fallback to passing raw_state through — Printful's error is
// friendlier than silently dropping the field.
/// Map a Stripe-selected size string to the matching Printful variant
/// for known products. Returns None when we don't have a mapping (caller
/// falls back to the row's default variant_id).
///
/// Variants come from Printful's catalog API (verified live for the
/// products we sell). New product? Add a match arm.
fn resolve_size_variant(printful_product_id: i64, size: &str) -> Option<i64> {
    let sz = size.to_uppercase();
    match printful_product_id {
        // Bella+Canvas 3001 (Black)
        71 => match sz.as_str() {
            "S" => Some(4016), "M" => Some(4017), "L" => Some(4018),
            "XL" => Some(4019), "2XL" | "XXL" => Some(4020),
            _ => None,
        },
        // Gildan 18500 pullover hoodie (Black) — verified GET /products/146
        // 2026-05-30. Was off by one (S=5529…), shipping one size too small:
        // an "M" order resolved to 5530 = Black/S.
        146 => match sz.as_str() {
            "S" => Some(5530), "M" => Some(5531), "L" => Some(5532),
            "XL" => Some(5533), "2XL" | "XXL" => Some(5534), "3XL" | "XXXL" => Some(5535),
            _ => None,
        },
        // Gildan 18000 crewneck sweatshirt (Black) — verified GET /products/145
        // 2026-05-30. Was 5402–5406, none of which exist in Printful (404),
        // so every sized crewneck order was rejected at fulfillment.
        145 => match sz.as_str() {
            "S" => Some(5434), "M" => Some(5435), "L" => Some(5436),
            "XL" => Some(5437), "2XL" | "XXL" => Some(5438), "3XL" | "XXXL" => Some(5439),
            _ => None,
        },
        // AOP Men's Rash Guard (White) — 7 sizes. Verified GET /products/301
        // 2026-05-30. XS/S were off by one (XS=9325 doesn't exist; S=9326 is
        // actually XS), so an "S" order shipped XS. M and up were correct.
        301 => match sz.as_str() {
            "XS" => Some(9326), "S" => Some(9327), "M" => Some(9328),
            "L" => Some(9329), "XL" => Some(9330),
            "2XL" | "XXL" => Some(9331), "3XL" | "XXXL" => Some(9332),
            _ => None,
        },
        _ => None,
    }
}

fn normalize_state_code(country: &str, raw: &str) -> String {
    let s = raw.trim();
    match country {
        "JP" => {
            if s.starts_with("JP-") {
                return s.to_string();
            }
            if let Ok(n) = s.parse::<u32>() {
                if (1..=47).contains(&n) {
                    return format!("JP-{:02}", n);
                }
            }
            if let Some(code) = jp_prefecture_to_iso(s) {
                return format!("JP-{}", code);
            }
            String::new()
        }
        "US" => s.to_uppercase().chars().take(2).collect(),
        _ => s.to_string(),
    }
}

fn jp_prefecture_to_iso(s: &str) -> Option<&'static str> {
    Some(match s {
        "北海道" | "Hokkaido" => "01",
        "青森県" | "Aomori" => "02",
        "岩手県" | "Iwate" => "03",
        "宮城県" | "Miyagi" => "04",
        "秋田県" | "Akita" => "05",
        "山形県" | "Yamagata" => "06",
        "福島県" | "Fukushima" => "07",
        "茨城県" | "Ibaraki" => "08",
        "栃木県" | "Tochigi" => "09",
        "群馬県" | "Gunma" => "10",
        "埼玉県" | "Saitama" => "11",
        "千葉県" | "Chiba" => "12",
        "東京都" | "Tokyo" => "13",
        "神奈川県" | "Kanagawa" => "14",
        "新潟県" | "Niigata" => "15",
        "富山県" | "Toyama" => "16",
        "石川県" | "Ishikawa" => "17",
        "福井県" | "Fukui" => "18",
        "山梨県" | "Yamanashi" => "19",
        "長野県" | "Nagano" => "20",
        "岐阜県" | "Gifu" => "21",
        "静岡県" | "Shizuoka" => "22",
        "愛知県" | "Aichi" => "23",
        "三重県" | "Mie" => "24",
        "滋賀県" | "Shiga" => "25",
        "京都府" | "Kyoto" => "26",
        "大阪府" | "Osaka" => "27",
        "兵庫県" | "Hyogo" => "28",
        "奈良県" | "Nara" => "29",
        "和歌山県" | "Wakayama" => "30",
        "鳥取県" | "Tottori" => "31",
        "島根県" | "Shimane" => "32",
        "岡山県" | "Okayama" => "33",
        "広島県" | "Hiroshima" => "34",
        "山口県" | "Yamaguchi" => "35",
        "徳島県" | "Tokushima" => "36",
        "香川県" | "Kagawa" => "37",
        "愛媛県" | "Ehime" => "38",
        "高知県" | "Kochi" => "39",
        "福岡県" | "Fukuoka" => "40",
        "佐賀県" | "Saga" => "41",
        "長崎県" | "Nagasaki" => "42",
        "熊本県" | "Kumamoto" => "43",
        "大分県" | "Oita" => "44",
        "宮崎県" | "Miyazaki" => "45",
        "鹿児島県" | "Kagoshima" => "46",
        "沖縄県" | "Okinawa" => "47",
        _ => return None,
    })
}

// ─── 30-min autonomous optimizer cron ─────────────────────────────────
//
// Phase 1 behaviour (no sales data yet):
//   • If fewer than TARGET_INITIAL auto-generated SKUs exist, generate
//     one per (theme × kind) combination that's still missing.
//   • Telegram digest each cycle: how many auto SKUs exist, ¥ spent so
//     far, last 30 min orders.
//
// Phase 2 behaviour (kicks in once catalog_orders has data):
//   • Compute ROAS per theme from orders + spend ledger.
//   • Deactivate auto SKUs that have been live > 24h with 0 orders AND
//     whose theme is in the bottom quartile by ROAS.
//   • Generate +N SKUs in the top-quartile theme.
//
// Hard limits the cron honours:
//   • spend_or_refuse() inside generate_one — never goes over the
//     monthly cap (BUDGET_TOTAL_JPY, ¥1M/mo, resets on the 1st).
//   • SKU_HARD_CAP = 30,000 — never inserts past the user's cap.
//   • CRON_BATCH_MAX = 10 — never generates more than 10 per cycle so a
//     misconfiguration can't run away.

pub const SKU_HARD_CAP: i64 = 30_000;
const TARGET_INITIAL: i64 = 60; // 12 themes × 2 kinds × ~2.5 SKUs per combo
const CRON_BATCH_MAX: u32 = 10;
const CRON_INTERVAL_SECS: u64 = 30 * 60;

/// Long-running task: every 30 min, take one self-improvement step.
/// Spawn this once from main() before the router takes the db.
pub async fn run_optimizer_cron(db: Db) {
    // Stagger the first run by 60s so it doesn't fight startup.
    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    let mut cycle: u32 = 0;
    loop {
        cycle = cycle.wrapping_add(1);
        match optimizer_step(db.clone()).await {
            Ok(summary) => {
                tracing::info!("[catalog/cron] {}", summary);
                let _ = crate::send_telegram_message(&format!(
                    "🤖 *catalog optimizer* — {}",
                    summary
                ))
                .await;
            }
            Err(e) => {
                tracing::warn!("[catalog/cron] step failed: {}", e);
            }
        }
        // Every 4th cycle (~ once / 2 hours), have a persona critique
        // /shop and post it to Telegram. Gives the operator continuous
        // outside-eye feedback without manually checking the page.
        if cycle % 4 == 1 {
            if let Err(e) = persona_review_and_alert().await {
                tracing::warn!("[catalog/cron] persona review failed: {}", e);
            }
        }
        // Each cycle: backfill (b) transparent print, (c) Printful
        // mockup, (d) Gemini lifestyle photo for SKUs that don't have
        // them yet. Phase 1 SKUs only got (a) + (c); the 4-image
        // pipeline went in mid-stream so backfill catches them up.
        if let Err(e) = mockup_backfill_step(db.clone()).await {
            tracing::warn!("[catalog/cron] mockup backfill failed: {}", e);
        }
        if let Err(e) = transparent_backfill_step(db.clone()).await {
            tracing::warn!("[catalog/cron] transparent backfill failed: {}", e);
        }
        if let Err(e) = lifestyle_backfill_step(db.clone()).await {
            tracing::warn!("[catalog/cron] lifestyle backfill failed: {}", e);
        }
        if let Err(e) = stale_sku_killer_step(db.clone()).await {
            tracing::warn!("[catalog/cron] stale sku killer failed: {}", e);
        }
        if let Err(e) = retry_failed_fulfillments_step(db.clone()).await {
            tracing::warn!("[catalog/cron] retry failed orders: {}", e);
        }
        // 再発防止 (2026-06-04): ~1日に1回、リトライ尽き or 長期滞留の
        // 「入金済みなのに未発送/未返金」注文を点検して Telegram に上げる。
        // 4xx は fulfill 側で自動返金されるが、ここは取りこぼし(retry上限超過の
        // 永続 5xx・manual_pending の発送忘れ等)の最後の安全網。
        if cycle % 48 == 0 {
            if let Err(e) = stuck_orders_alert_step(db.clone()).await {
                tracing::warn!("[catalog/cron] stuck orders check: {}", e);
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(CRON_INTERVAL_SECS)).await;
    }
}

/// Self-improvement: generate (d) lifestyle photo for AUTO SKUs that
/// don't have one yet. We mark "has lifestyle" by the existence of a
/// catalog_product_extras row with label starting 'lifestyle'. Cron
/// runs ¥6 × 2 SKUs / cycle = ¥12 / 30 min, well within budget.
async fn lifestyle_backfill_step(db: Db) -> Result<(), String> {
    let rows: Vec<String> = {
        let conn = db.lock().unwrap();
        conn.prepare(
            "SELECT cp.sku
             FROM catalog_products cp
             WHERE cp.brand='auto' AND cp.is_active=1
               AND NOT EXISTS (
                 SELECT 1 FROM catalog_product_extras ex
                 WHERE ex.sku = cp.sku AND ex.label LIKE 'lifestyle%'
               )
             LIMIT 2",
        )
        .ok()
        .and_then(|mut s| {
            s.query_map([], |r| r.get::<_, String>(0))
                .ok()
                .map(|it| it.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
    };
    for sku in rows {
        // Infer theme + kind from SKU pattern. AUTO-{THEME}-{KIND}-{seed}.
        let kind = kind_from_sku(&sku);
        let theme_slug = sku
            .strip_prefix("AUTO-")
            .and_then(|rest| {
                SEED_THEMES.iter().find(|t| {
                    let pat = t.slug.to_uppercase().replace('_', "-") + "-";
                    rest.starts_with(&pat)
                }).map(|t| t.slug)
            })
            .unwrap_or("mu_mark");
        let theme = SEED_THEMES
            .iter()
            .find(|t| t.slug == theme_slug)
            .unwrap_or(&SEED_THEMES[3]);
        let db_c = db.clone();
        let sku_c = sku.clone();
        let slug_c = theme.slug.to_string();
        let brief_c = theme.prompt_brief.to_string();
        let kind_c = kind.to_string();
        tokio::spawn(async move {
            let _ = generate_lifestyle_photo(db_c, sku_c, slug_c, brief_c, kind_c, 1).await;
        });
    }
    Ok(())
}

/// Self-improvement: process (b) transparent print for AUTO SKUs that
/// don't have one yet. Fast + free (image crate, no API). Cron does
/// 3 per cycle.
async fn transparent_backfill_step(db: Db) -> Result<(), String> {
    let rows: Vec<(String, String)> = {
        let conn = db.lock().unwrap();
        conn.prepare(
            "SELECT cp.sku, COALESCE(cp.design_file, '')
             FROM catalog_products cp
             WHERE cp.brand='auto' AND cp.is_active=1
               AND cp.design_file IS NOT NULL
               AND NOT EXISTS (
                 SELECT 1 FROM catalog_product_extras ex
                 WHERE ex.sku = cp.sku AND ex.label LIKE '%print%'
               )
             LIMIT 3",
        )
        .ok()
        .and_then(|mut s| {
            s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .ok()
                .map(|it| it.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
    };
    for (sku, design_url) in rows {
        if design_url.is_empty() {
            continue;
        }
        let db_c = db.clone();
        tokio::spawn(async move {
            // Fetch the design bytes from R2 (= the URL we stored) and
            // run the same white→alpha pipeline.
            match reqwest::Client::new()
                .get(&design_url)
                .timeout(std::time::Duration::from_secs(30))
                .send()
                .await
            {
                Ok(r) if r.status().is_success() => {
                    match r.bytes().await {
                        Ok(b) => {
                            let _ = generate_transparent_print(db_c, sku, b.to_vec()).await;
                        }
                        Err(e) => tracing::warn!("[catalog/transparent] bytes fail: {}", e),
                    }
                }
                _ => tracing::warn!("[catalog/transparent] fetch fail for {}", design_url),
            }
        });
    }
    Ok(())
}

/// Self-improvement: retry catalog_orders rows that previously failed
/// (status='failed' or 'failed_network'). Re-pulls the Stripe Session
/// via expand to get the full address, deletes the failed row, then
/// re-runs fulfill_catalog_order. Caps retries via a retry_count column
/// (added idempotently here) so we don't spin forever on a permanently
/// broken row.
///
/// Triggered every 30-min cron tick; with the fulfillment fixes from
/// 2f4eb9c (shipping expand + stitch_color), the order #1 self-buy
/// should recover automatically on the next deploy + tick.
async fn retry_failed_fulfillments_step(db: Db) -> Result<(), String> {
    // Add retry_count column lazily (SQLite has no IF NOT EXISTS for ALTER).
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "ALTER TABLE catalog_orders ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0",
            [],
        );
    }
    let candidates: Vec<(i64, String)> = {
        let conn = db.lock().unwrap();
        conn.prepare(
            "SELECT id, stripe_session_id FROM catalog_orders
             WHERE status IN ('failed','failed_network','failed_no_key')
               AND COALESCE(retry_count, 0) < 3
               AND created_at > datetime('now','-7 days')
             ORDER BY id ASC
             LIMIT 2",
        )
        .ok()
        .and_then(|mut s| {
            s.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
                .ok()
                .map(|it| it.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
    };
    if candidates.is_empty() {
        return Ok(());
    }
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    if stripe_key.is_empty() {
        return Err("STRIPE_SECRET_KEY unset".into());
    }
    for (id, sid) in candidates {
        // Increment retry counter up-front so concurrent ticks don't
        // both pick the same row + we cap at 3.
        {
            let conn = db.lock().unwrap();
            let _ = conn.execute(
                "UPDATE catalog_orders SET retry_count = COALESCE(retry_count,0) + 1 WHERE id=?",
                rusqlite::params![id],
            );
        }
        let url = format!(
            "https://api.stripe.com/v1/checkout/sessions/{}",
            sid
        );
        let session = match reqwest::Client::new()
            .get(&url).basic_auth(&stripe_key, None::<&str>).send().await
        {
            Ok(r) if r.status().is_success() => r.json::<serde_json::Value>().await.ok(),
            _ => None,
        };
        let Some(session) = session else {
            tracing::warn!("[catalog/retry] stripe lookup failed for id={} session={}", id, sid);
            continue;
        };
        // Remove the failed row so fulfill_catalog_order's idempotency
        // check doesn't short-circuit it.
        {
            let conn = db.lock().unwrap();
            let _ = conn.execute(
                "DELETE FROM catalog_orders WHERE id=?",
                rusqlite::params![id],
            );
        }
        let db_c = db.clone();
        let sid_log = sid.clone();
        tokio::spawn(async move {
            tracing::info!("[catalog/retry] re-running fulfill for session={}", sid_log);
            fulfill_catalog_order(db_c, session).await;
        });
    }
    Ok(())
}

/// 滞留注文の安全網: 入金済みなのに発送も返金もされず取りこぼされた注文を
/// 検知して Telegram に上げる (~1日1回)。対象:
///  - status='failed'/'failed_*' で retry_count>=3 (再試行を使い切り放置)
///  - status='manual_pending' で 2日以上 (NFCコイン等の発送忘れ)
/// 4xx は fulfill 側で自動返金されるのでここには出ない。出たら人手で返金/発送。
async fn stuck_orders_alert_step(db: Db) -> Result<(), String> {
    let rows: Vec<(i64, String, String, i64, String)> = {
        let conn = db.lock().unwrap();
        conn.prepare(
            "SELECT id, COALESCE(sku,'?'), status, COALESCE(amount_jpy,0), COALESCE(created_at,'')
             FROM catalog_orders
             WHERE (status LIKE 'failed%' AND COALESCE(retry_count,0) >= 3)
                OR (status = 'manual_pending' AND created_at < datetime('now','-2 days'))
             ORDER BY id ASC LIMIT 30",
        )
        .ok()
        .and_then(|mut s| {
            s.query_map([], |r| Ok((
                r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?, r.get::<_, String>(4)?,
            )))
            .ok()
            .map(|it| it.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
    };
    if rows.is_empty() {
        return Ok(());
    }
    let mut lines = String::new();
    let mut total = 0i64;
    for (id, sku, status, amount, created) in &rows {
        total += amount;
        lines.push_str(&format!("\n• id={} `{}` {} ¥{} ({})", id, sku, status, amount, created));
    }
    let _ = crate::send_telegram_message(&format!(
        "🟠 *滞留注文 {}件* (入金済・未発送のまま取りこぼし)\n\
         failed=retry尽き / manual_pending=発送忘れ。合計¥{}。{}\n\
         → 発送するか、返金: GET /admin/catalog/orders/<id>/replay (4xxなら自動返金) か Stripe手動返金。",
        rows.len(), total, lines
    ))
    .await;
    Ok(())
}

/// Self-improvement: deactivate AUTO SKUs that have failed mockup
/// generation 5+ times. Stops the backfill cron from burning attempts
/// on rows that will never succeed (bad variant_id / bad design URL /
/// etc.). Reads from catalog_spend ledger.
async fn stale_sku_killer_step(db: Db) -> Result<(), String> {
    let killed: i64 = {
        let conn = db.lock().unwrap();
        conn.execute(
            "UPDATE catalog_products
             SET is_active=0, status='retired'
             WHERE brand='auto' AND is_active=1
               AND sku IN (
                 SELECT ref_id FROM catalog_spend
                 WHERE category='mockup_fail'
                 GROUP BY ref_id HAVING COUNT(*) >= 5
               )",
            [],
        )
        .unwrap_or(0) as i64
    };
    if killed > 0 {
        tracing::info!("[catalog/cron] killed {} stale SKUs (5+ mockup fails)", killed);
    }
    Ok(())
}

/// Find up to N AUTO SKUs whose mockup_url_external equals the design
/// URL (= no on-body mockup generated yet) and spawn the Printful
/// mockup-generator for each. Background; cron continues immediately.
async fn mockup_backfill_step(db: Db) -> Result<(), String> {
    // Identify SKUs needing on-body mockup. Heuristic: mockup_url_external
    // ends with the same path as design_file (we wrote both to the same
    // value when first generating), so the row is "design-only" if those
    // two columns match.
    let rows: Vec<(String, i64, i64, String)> = {
        let conn = db.lock().unwrap();
        conn.prepare(
            "SELECT sku, printful_product_id, printful_variant_id,
                    COALESCE(design_file, '')
             FROM catalog_products
             WHERE brand='auto' AND is_active=1
               AND (mockup_url_external = design_file OR mockup_url_external IS NULL)
             LIMIT 5",
        )
        .ok()
        .and_then(|mut s| {
            s.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .ok()
            .map(|it| it.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
    };
    for (sku, pp, pv, design) in rows {
        if design.is_empty() {
            continue;
        }
        let db_c = db.clone();
        tokio::spawn(async move {
            if let Err(e) = generate_onbody_mockup(db_c.clone(), sku.clone(), pp, pv, design).await {
                tracing::warn!("[catalog/mockup] {} failed: {}", sku, e);
                let conn = db_c.lock().unwrap();
                let _ = conn.execute(
                    "INSERT INTO catalog_spend (category, amount_jpy, reason, ref_id)
                     VALUES ('mockup_fail', 0, ?, ?)",
                    rusqlite::params![e.chars().take(200).collect::<String>(), &sku],
                );
            }
        });
    }
    Ok(())
}

/// Fetch /shop, ask Gemini to act as 3 personas (cold ad visitor / BJJ
/// gear shopper / overseas e-commerce auditor) and surface the harshest
/// 1-line takeaway each. Sent to Telegram so the operator gets a steady
/// stream of "where does this still suck" data without manually QA'ing.
async fn persona_review_and_alert() -> Result<(), String> {
    let base = std::env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    let html = reqwest::Client::new()
        .get(format!("{}/shop", base))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("fetch /shop: {}", e))?
        .text()
        .await
        .map_err(|e| format!("read /shop body: {}", e))?;
    // Trim to a budget Gemini can chew on (and stay under our cost cap).
    let body_trimmed: String = html.chars().take(8000).collect();
    let prompt = format!(
        "You are reviewing the landing page at {base}/shop for an e-commerce shop selling AI-designed BJJ / lifestyle wearables. \
         Respond as 3 personas in this exact JSON format (no prose around it): \
         {{\"cold_visitor_3s\":\"…\",\"bjj_practitioner\":\"…\",\"overseas_auditor\":\"…\"}} \
         Each value: 1 short Japanese sentence with the HARSHEST single issue blocking purchase. \
         Be specific (name the element). Don't say 'overall good'. \
         \nPage HTML (first 8k chars):\n{body}",
        base = base, body = body_trimmed
    );
    let critique = crate::gemini::call_gemini_text(&prompt)
        .await
        .map_err(|e| format!("gemini text: {}", e))?;
    // Try to extract the JSON we asked for; if Gemini wrapped it, fall back
    // to the raw text. Telegram will render either readably.
    let parsed: Option<serde_json::Value> = serde_json::from_str(critique.trim()).ok()
        .or_else(|| {
            critique.find('{').and_then(|i| critique[i..].find('}').map(|j| i + j + 1))
                .and_then(|end| critique[critique.find('{').unwrap()..end].parse::<String>().ok())
                .and_then(|s| serde_json::from_str(&s).ok())
        });
    let msg = if let Some(j) = parsed {
        let pull = |k: &str| j.get(k).and_then(|v| v.as_str()).unwrap_or("(empty)").to_string();
        format!(
            "🪞 */shop persona critique*\n\n📱 *3秒判定*: {}\n🥋 *柔術勢*: {}\n🌎 *海外監査*: {}",
            pull("cold_visitor_3s"), pull("bjj_practitioner"), pull("overseas_auditor")
        )
    } else {
        format!("🪞 */shop persona critique*\n\n{}", critique.chars().take(800).collect::<String>())
    };
    let _ = crate::send_telegram_message(&msg).await;
    // Text-mode Gemini ~¥1/call; not worth a separate ledger row right now.
    Ok(())
}

/// One iteration. Returns a human-readable summary line.
async fn optimizer_step(db: Db) -> Result<String, String> {
    let (auto_total, orders_24h, spent_jpy) = {
        let conn = db.lock().unwrap();
        let auto: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM catalog_products WHERE brand='auto'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let orders: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM catalog_orders WHERE created_at > datetime('now','-1 day')",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        (auto, orders, spent_month_jpy(&conn))
    };

    if auto_total >= SKU_HARD_CAP {
        return Ok(format!(
            "cap reached ({} ≥ {}). spent ¥{}/¥{}. orders/24h={}",
            auto_total, SKU_HARD_CAP, spent_jpy, BUDGET_TOTAL_JPY, orders_24h
        ));
    }
    if spent_jpy >= BUDGET_TOTAL_JPY {
        return Ok(format!(
            "budget exhausted ¥{}/¥{}. auto SKUs={}, orders/24h={}",
            spent_jpy, BUDGET_TOTAL_JPY, auto_total, orders_24h
        ));
    }

    let mut generated_this_cycle: u32 = 0;
    let mut summary_lines: Vec<String> = Vec::new();

    // Phase 1: backfill until TARGET_INITIAL — rotate themes × kinds.
    if auto_total < TARGET_INITIAL {
        let need = (TARGET_INITIAL - auto_total).min(CRON_BATCH_MAX as i64) as u32;
        for i in 0..need {
            let theme = &SEED_THEMES[(i as usize + auto_total as usize) % SEED_THEMES.len()];
            let kind = PRODUCT_SPECS[(i as usize) % PRODUCT_SPECS.len()].kind;
            let seed = format!("c{:08x}", rand::random::<u32>());
            match generate_one(db.clone(), theme.slug, kind, &seed).await {
                Ok(sku) => {
                    generated_this_cycle += 1;
                    summary_lines.push(format!("+ {}", sku));
                }
                Err(e) => {
                    summary_lines.push(format!("✗ {}/{} : {}", theme.slug, kind, e));
                    if e.contains("budget cap") {
                        break;
                    }
                }
            }
        }
    } else if orders_24h == 0 {
        // No data, no further generation — wait for ads/organic to bring
        // signal in. The cron still ticks to report status.
        summary_lines.push("steady-state: waiting on order data".into());
    } else {
        // Phase 2: data-driven generation. Find the top theme by orders
        // and add one more SKU in that theme + a random kind.
        let top: Option<String> = {
            let conn = db.lock().unwrap();
            conn.query_row(
                "SELECT cp.brand
                 FROM catalog_orders co
                 JOIN catalog_products cp ON cp.sku=co.sku
                 WHERE co.status='submitted' AND cp.brand='auto'
                 GROUP BY cp.sku
                 ORDER BY COUNT(*) DESC
                 LIMIT 1",
                [],
                |r| r.get::<_, String>(0),
            )
            .ok()
        };
        if let Some(_brand) = top {
            // We hash all auto SKUs under brand='auto' so theme has to
            // come from a different path — for now just rotate again.
            let theme = &SEED_THEMES[rand::random::<usize>() % SEED_THEMES.len()];
            let kind = PRODUCT_SPECS[rand::random::<usize>() % PRODUCT_SPECS.len()].kind;
            let seed = format!("c{:08x}", rand::random::<u32>());
            match generate_one(db.clone(), theme.slug, kind, &seed).await {
                Ok(sku) => {
                    generated_this_cycle += 1;
                    summary_lines.push(format!("data-driven + {}", sku));
                }
                Err(e) => summary_lines.push(format!("✗ data-driven: {}", e)),
            }
        }
    }

    let (auto_after, spent_after) = {
        let conn = db.lock().unwrap();
        let a: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM catalog_products WHERE brand='auto'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        (a, spent_total_jpy(&conn))
    };

    Ok(format!(
        "auto SKUs={} (+{}), spent ¥{}/¥{}, orders/24h={}\n{}",
        auto_after,
        generated_this_cycle,
        spent_after,
        BUDGET_TOTAL_JPY,
        orders_24h,
        summary_lines.join("\n")
    ))
}

#[cfg(test)]
mod lifestyle_composite_tests {
    use super::*;

    // Smoke test for the real-design composite. Reads a bundled worn-blank base
    // and a design PNG path from COMPOSE_TEST_DESIGN, writes the result to
    // /tmp/rust_comp.png for visual inspection. Skips cleanly if assets absent.
    //   COMPOSE_TEST_DESIGN=/tmp/flag_design.png cargo test --release \
    //     compose_lifestyle_smoke -- --nocapture
    #[test]
    fn compose_lifestyle_smoke() {
        let Some(base) = read_base_png("tee_1") else {
            eprintln!("skip: base tee_1 not found from cwd");
            return;
        };
        let Ok(design) = std::env::var("COMPOSE_TEST_DESIGN") else {
            eprintln!("skip: set COMPOSE_TEST_DESIGN to a design png path");
            return;
        };
        let design_bytes = std::fs::read(&design).expect("read design");
        let b = &lifestyle_bases("tee")[0];
        let out = compose_lifestyle_png(&design_bytes, &base, b).expect("composite ok");
        assert!(out.len() > 10_000, "output png suspiciously small: {}", out.len());
        std::fs::write("/tmp/rust_comp.png", &out).expect("write out");
        eprintln!("wrote /tmp/rust_comp.png ({} bytes)", out.len());
    }
}

