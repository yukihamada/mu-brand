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

    let route = if matches!(kind, "rashguard_ls" | "rashguard_black") {
        "printful_aop"
    } else {
        "printful_dtg"
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

<p><a class="btn" href="mailto:returns@wearmu.com">返品申請する</a></p>
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
}

const SHOP_PAGE_SIZE: i64 = 60;

pub async fn shop_index(
    State(db): State<Db>,
    Query(q): Query<ShopQuery>,
) -> Html<String> {
    let brand_filter = q.brand.unwrap_or_default();
    let page = q.page.unwrap_or(1).max(1);
    let offset = (page as i64 - 1) * SHOP_PAGE_SIZE;
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

        let total: i64 = if brand_filter.is_empty() {
            conn.query_row(
                "SELECT COUNT(*) FROM catalog_products WHERE is_active=1",
                [], |r| r.get(0)
            ).unwrap_or(0)
        } else {
            conn.query_row(
                "SELECT COUNT(*) FROM catalog_products WHERE is_active=1 AND brand=?",
                rusqlite::params![&brand_filter], |r| r.get(0)
            ).unwrap_or(0)
        };

        let items: Vec<ProductRow> = if brand_filter.is_empty() {
            list_products_paged(&conn, None, SHOP_PAGE_SIZE, offset)
        } else {
            list_products_paged(&conn, Some(&brand_filter), SHOP_PAGE_SIZE, offset)
        };
        (brands, items, total)
    };

    let brand_chips = {
        let mut s = String::new();
        s.push_str(&format!(
            r#"<a class="chip{}" href="/shop">すべて</a>"#,
            if brand_filter.is_empty() { " on" } else { "" }
        ));
        for (slug, name, emoji) in &brands {
            let on = if &brand_filter == slug { " on" } else { "" };
            s.push_str(&format!(
                r#"<a class="chip{on}" href="/shop?brand={slug}">{emoji} {name}</a>"#,
                on = on,
                slug = html_attr(slug),
                emoji = html_text(emoji),
                name = html_text(name),
            ));
        }
        s
    };

    let grid = items
        .iter()
        .map(|p| render_card(p))
        .collect::<String>();

    let page_count = items.len();
    let total_pages = ((total_active as f64) / (SHOP_PAGE_SIZE as f64)).ceil() as u32;
    let title = if brand_filter.is_empty() {
        format!("/shop — {} 件のコラボ商品 | MU", total_active)
    } else {
        format!("/shop — {} ({}件) | MU カタログ", brand_filter, total_active)
    };

    // Pagination: prev / page-of-pages / next. Brand filter persists.
    let bq = if brand_filter.is_empty() {
        String::new()
    } else {
        format!("&brand={}", urlencoding::encode(&brand_filter))
    };
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
    let body = format!(
        r##"<!doctype html><html lang="ja"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<meta name="description" content="MU × ブランド コラボ カタログ。 {total} 件。 Stripe + Printful 直配送 7-14日。">
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
.card .body .name{{font-size:12.5px;line-height:1.45;flex:1}}
.card .body .price{{font-size:13px;font-weight:700;color:#fff;font-family:monospace}}
.empty{{padding:60px 24px;text-align:center;color:rgba(245,245,240,0.5);max-width:1180px;margin:0 auto}}
.pagination{{max-width:1180px;margin:0 auto;padding:14px 24px 40px;display:flex;justify-content:space-between;align-items:center;gap:12px;flex-wrap:wrap;font-size:12px}}
.pg-link{{color:#ffd700;text-decoration:none;padding:8px 14px;border:1px solid rgba(255,215,0,0.4);border-radius:999px;font-size:11px;letter-spacing:0.05em}}
.pg-link:hover{{background:rgba(255,215,0,0.08)}}
.pg-link.off{{color:#444;border-color:rgba(255,255,255,0.06);cursor:not-allowed}}
.pg-count{{color:rgba(245,245,240,0.5);font-size:11px;font-family:monospace}}
footer{{padding:30px 24px 50px;border-top:1px solid rgba(255,255,255,0.06);text-align:center;color:rgba(245,245,240,0.5);font-size:10px;letter-spacing:0.15em}}
footer a{{color:rgba(245,245,240,0.7);text-decoration:none;margin:0 8px}}
</style></head><body>
<nav>
  <a class="brand" href="/">MU</a>
  <div>
    <a href="/shop">SHOP</a>
    <a href="/buy" style="margin-left:14px">DROPS</a>
    <a href="/heritage" style="margin-left:14px">HERITAGE</a>
  </div>
</nav>
<div class="hero">
  <h1>━◯━ 知ってる人にだけ届く wearable.</h1>
  <p>柔術・コーヒー・地域 ── 10+ コラボの "内側からの服"。 受注生産 — 1 着から、 完売・廃棄ゼロ。 <strong style="color:#ffd700">{total} 件</strong> 公開中。</p>
  <div class="trust">
    <span><strong>国際発送</strong> 7-14 日 (DHL / FedEx)</span>
    <span><strong>1 着から</strong> オーダー可</span>
    <span><strong>Bella+Canvas / AOP rashguard</strong> 等プレミアム生地</span>
    <span><strong>Stripe</strong> 安全決済 + クーポン対応</span>
  </div>
</div>
<div class="chips">{brand_chips}</div>
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
  <a href="mailto:info@enablerdao.com">CONTACT</a>
</footer>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
</body></html>"##,
        title = html_text(&title),
        total = total_active,
        brand_chips = brand_chips,
        body_or_empty = if items.is_empty() {
            r#"<div class="empty">該当する商品がありません。</div>"#.to_string()
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
const SOLD_BADGE_MIN: i64 = 5;

pub async fn shop_pdp(
    State(db): State<Db>,
    Path(sku): Path<String>,
) -> Response {
    let row = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT sku, brand, label, description_ja, retail_price_jpy,
                    mockup_main_file, mockup_url_external, suzuri_url, stripe_price_id
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
                ))
            },
        )
        .ok()
    };
    let Some((sku, brand, _label, desc, price_jpy, mockup_main, mockup_ext, suzuri, price_id)) = row
    else {
        return (StatusCode::NOT_FOUND, "product not found").into_response();
    };

    // mockup: prefer external CDN; fall back to /static/... relative to root
    let img = mockup_ext
        .filter(|s| !s.is_empty())
        .or_else(|| mockup_main.map(|p| format!("https://merch.wearmu.com{}", p)))
        .unwrap_or_else(|| "/static/og-default.png".to_string());

    // extras
    let extras_imgs: Vec<String> = {
        let conn = db.lock().unwrap();
        conn.prepare(
            "SELECT image_url FROM catalog_product_extras WHERE sku=? ORDER BY sort_order, id",
        )
        .ok()
        .and_then(|mut s| {
            s.query_map(rusqlite::params![&sku], |r| r.get::<_, String>(0))
                .ok()
                .map(|it| it.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
    };

    let extras_html = if extras_imgs.is_empty() {
        String::new()
    } else {
        let mut s = String::from(r#"<div class="extras">"#);
        for u in &extras_imgs {
            s.push_str(&format!(
                r#"<img src="{}" alt="" loading="lazy">"#,
                html_attr(u)
            ));
        }
        s.push_str("</div>");
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
    let addon: Option<(String, i64)> = if is_sticker {
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
        format!(
            r#"{cross_html}<a class="buy" id="buybtn" href="{base}">買う <span class="amt">¥{price}</span> · 即購入 (Stripe + Printful 7-14 日 国際発送)</a>{cross_script}"#,
            cross_html = cross_html,
            base = base,
            price = format_jpy(price_jpy),
            cross_script = cross_script,
        )
    } else {
        r#"<div class="buy disabled">準備中</div>"#.to_string()
    };

    // Spec block: real BJJ buyers won't checkout without GSM / material /
    // print method. AUTO SKUs look up by their embedded kind; merch-bridge
    // SKUs use a SKU-pattern heuristic.
    let kind_guess = kind_from_sku(&sku);
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

    let trust_block = format!(r##"<div class="trust-strip">
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
</div>"##, sold_row = sold_row);

    let body = format!(
        r##"<!doctype html><html lang="ja"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} — /shop / wearmu.com</title>
<meta name="description" content="{desc}">
<meta property="og:image" content="{og}">
<meta property="og:title" content="{title}">
<meta property="og:type" content="product">
<meta property="og:url" content="https://wearmu.com/shop/{sku_url}">
<meta property="og:site_name" content="wearmu.com">
<meta property="product:price:amount" content="{price_raw}">
<meta property="product:price:currency" content="JPY">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="{title}">
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
.body h1{{font-size:24px;line-height:1.35;margin-bottom:14px;font-weight:900}}
.body .brand{{font-size:10px;letter-spacing:0.3em;color:#ffd700;text-transform:uppercase;margin-bottom:8px}}
.body .price{{font-size:22px;font-family:monospace;font-weight:700;color:#fff;margin-bottom:18px}}
.body .desc{{color:rgba(245,245,240,0.78);font-size:13px;line-height:1.85;margin-bottom:22px}}
.body .sku{{color:rgba(245,245,240,0.45);font-family:monospace;font-size:10px;margin-bottom:18px}}
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
    {extras}
  </div>
  <div class="body">
    <div class="brand">{brand}</div>
    <h1>{title}</h1>
    <div class="price">¥{price} <small class="fx">≈ ${usd} / €{eur}</small></div>
    {buy}
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
<footer class="pdp-footer">
  <div class="legal-links">
    <a href="/shop">SHOP</a>
    <a href="/shipping">配送 / Shipping</a>
    <a href="/returns">返品 / Returns</a>
    <a href="/faq">FAQ</a>
    <a href="/privacy">プライバシー / Privacy</a>
    <a href="mailto:info@enablerdao.com">CONTACT</a>
  </div>
  <div class="legal-fine">© 2026 MU / Enabler Inc. · 東京千代田区九段南 1-5-6 · 受注生産・国際発送 7-14 日</div>
</footer>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
</body></html>"##,
        title = html_text(&desc),
        desc = html_text(&desc),
        og = html_attr(&img),
        brand = html_text(&brand),
        brand_q = html_attr(&brand),
        price = format_jpy(price_jpy),
        usd = ((price_jpy as f64) / 159.0).round() as i64,
        eur = ((price_jpy as f64) / 172.0).round() as i64,
        sku = html_text(&sku),
        buy = buy_button,
        suzuri = suzuri_link,
        extras = extras_html,
        trust     = trust_block,
        spec      = spec_block,
        size_chart = size_chart_html(&kind_guess),
        shipping_table = shipping_table_html(),
        story     = story_block,
        sku_url   = urlencoding::encode(&sku),
        price_raw = price_jpy,
        ld_title  = html_attr(&desc),
        ld_img    = html_attr(&img),
        ld_desc   = html_attr(&desc),
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
}

pub async fn shop_checkout(
    State(db): State<Db>,
    Query(q): Query<CheckoutQuery>,
) -> Response {
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    if stripe_key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "checkout disabled").into_response();
    }
    let sku = q.sku;
    let row = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT stripe_price_id, retail_price_jpy, description_ja, brand,
                    COALESCE(mockup_url_external, mockup_main_file, '')
             FROM catalog_products WHERE sku=? AND is_active=1",
            rusqlite::params![&sku],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        )
        .ok()
    };
    let Some((price_id, price_jpy, desc, _brand, mockup_path)) = row else {
        return (StatusCode::NOT_FOUND, "sku not found").into_response();
    };

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
    let cancel_url = format!("{}/shop/{}", base_url, urlencoding::encode(&sku));

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
        ("shipping_address_collection[allowed_countries][0]", "JP".into()),
        ("shipping_address_collection[allowed_countries][1]", "US".into()),
        ("shipping_address_collection[allowed_countries][2]", "GB".into()),
        ("shipping_address_collection[allowed_countries][3]", "CA".into()),
        ("shipping_address_collection[allowed_countries][4]", "AU".into()),
        ("shipping_address_collection[allowed_countries][5]", "DE".into()),
        ("shipping_address_collection[allowed_countries][6]", "FR".into()),
        ("metadata[kind]", "catalog".into()),
        ("metadata[catalog_sku]", sku.clone()),
    ];
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
                // Operator alert — fulfillment failure on a paid order
                // is the highest-priority signal we emit. Stripe already
                // charged the customer; the next 30-min cron will
                // auto-retry but human eyes should see the cause now.
                let _ = crate::send_telegram_message(&format!(
                    "🚨 *catalog fulfillment FAILED*\n\
                     sku=`{}`\nsession=`{}…`\n\
                     amount=¥{}\nprintful body (first 600):\n```\n{}\n```\n\
                     auto-retry will fire on next cron cycle (~30min). \
                     Manual retry: GET /admin/catalog/orders/<id>/replay?token=…",
                    sku,
                    session_id.chars().take(24).collect::<String>(),
                    amount_total,
                    text.chars().take(600).collect::<String>()
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
    let _ = conn.execute(
        "INSERT OR REPLACE INTO catalog_orders
         (stripe_session_id, sku, amount_jpy, customer_email, customer_name,
          shipping_address_json, printful_order_id, printful_response_json, status,
          addon_sku)
         VALUES (?,?,?,?,?,?,?,?,?,?)",
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
        ],
    );
}

// ─── Helpers ──────────────────────────────────────────────────────────

struct ProductRow {
    sku: String,
    brand: String,
    desc: String,
    price: i64,
    img: Option<String>,
    sold: i64,
}

fn list_products_paged(
    conn: &rusqlite::Connection,
    brand: Option<&str>,
    limit: i64,
    offset: i64,
) -> Vec<ProductRow> {
    // 6th column = real sold count (status='submitted') for the social-proof
    // badge, derived per-row via correlated subquery (gated in render_card).
    let (sql, has_brand) = if brand.is_some() {
        (
            "SELECT sku, brand, description_ja, retail_price_jpy,
                    COALESCE(mockup_url_external, mockup_main_file),
                    (SELECT COUNT(*) FROM catalog_orders o WHERE o.sku=catalog_products.sku AND o.status='submitted')
             FROM catalog_products
             WHERE is_active=1 AND brand=?
             ORDER BY (brand='auto') DESC,
                      (mockup_url_external IS NOT NULL AND mockup_url_external != '') DESC,
                      sort_order, sku
             LIMIT ? OFFSET ?",
            true,
        )
    } else {
        (
            "SELECT sku, brand, description_ja, retail_price_jpy,
                    COALESCE(mockup_url_external, mockup_main_file),
                    (SELECT COUNT(*) FROM catalog_orders o WHERE o.sku=catalog_products.sku AND o.status='submitted')
             FROM catalog_products
             WHERE is_active=1
             ORDER BY (brand='auto') DESC,
                      (mockup_url_external IS NOT NULL AND mockup_url_external != '') DESC,
                      sort_order, sku
             LIMIT ? OFFSET ?",
            false,
        )
    };
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mapper = |r: &rusqlite::Row| {
        Ok(ProductRow {
            sku: r.get(0)?, brand: r.get(1)?, desc: r.get(2)?,
            price: r.get(3)?, img: r.get(4)?, sold: r.get(5)?,
        })
    };
    if has_brand {
        stmt.query_map(rusqlite::params![brand.unwrap(), limit, offset], mapper)
    } else {
        stmt.query_map(rusqlite::params![limit, offset], mapper)
    }
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
             ORDER BY (brand='auto') DESC,
                      (mockup_url_external IS NOT NULL AND mockup_url_external != '') DESC,
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
             ORDER BY (brand='auto') DESC,
                      (mockup_url_external IS NOT NULL AND mockup_url_external != '') DESC,
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

fn render_card(p: &ProductRow) -> String {
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
    format!(
        r##"<a class="card" href="/shop/{sku_enc}"><span class="img" style="position:relative;display:block">{sold_badge}<img src="{img}" alt="" loading="lazy" onerror="this.onerror=null;this.src='/static/designs/marker_zero.png';this.style.objectFit='contain';this.style.background='#0a0a0a';this.style.padding='28px'"></span><span class="body"><span class="brand">{brand}</span><span class="name">{name}</span><span class="price">¥{price}</span></span></a>"##,
        sku_enc = urlencoding::encode(&p.sku),
        sold_badge = sold_badge,
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

