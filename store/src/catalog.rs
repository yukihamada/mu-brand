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

pub const BUDGET_TOTAL_JPY: i64 = 100_000;

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

/// Total ¥ spent across all categories so far. Source of truth for the
/// budget guard.
pub fn spent_total_jpy(conn: &rusqlite::Connection) -> i64 {
    conn.query_row("SELECT COALESCE(SUM(amount_jpy), 0) FROM catalog_spend",
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
    let current = spent_total_jpy(conn);
    if current.saturating_add(amount_jpy) > BUDGET_TOTAL_JPY {
        tracing::warn!(
            "[catalog/budget] REFUSED {} ¥{} (current=¥{} cap=¥{}) reason={}",
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
        "[catalog/budget] +¥{} {} (total=¥{}/¥{}) reason={}",
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
        printful_variant_id: 5530, // Black M
        placement: "front",
        retail_jpy: 8800,
        spec_html: "Gildan 18500 unisex pullover hoodie · Black · 8.0 oz (270 gsm) · \
                    50/50 cotton-polyester blend · double-needle stitching · \
                    DTG print front chest · pouch pocket · drawstring hood",
    },
    ProductSpec {
        kind: "crewneck",
        printful_product_id: 145, // Gildan 18000 crewneck sweatshirt
        printful_variant_id: 5403, // Black M
        placement: "front",
        retail_jpy: 7800,
        spec_html: "Gildan 18000 unisex crewneck sweatshirt · Black · 8.0 oz · \
                    50/50 cotton-polyester blend · 1×1 athletic ribbed collar · \
                    DTG print front chest",
    },
];

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
    let scene = match (kind.as_str(), variant) {
        ("rashguard_ls" | "rashguard_black", 1) =>
            "Japanese BJJ practitioner from behind, sitting on tatami mat in a clean modern dojo, hands resting on knees. Camera at chest height looking at upper back of rashguard.",
        ("rashguard_ls" | "rashguard_black", _) =>
            "Close-up torso shot of a Japanese MMA athlete adjusting a rashguard cuff at the wrist, no face visible, gym light from window left.",
        ("hoodie" | "crewneck", 1) =>
            "Japanese person walking away from camera at sunset on a Tokyo street, wearing the black hoodie, hood up. No face visible.",
        ("hoodie" | "crewneck", _) =>
            "Folded hoodie on a wooden bench at a cafe, with a coffee cup beside it. Editorial flat-lay angle.",
        ("tee", 1) =>
            "Japanese person from neck-down sitting at a wood desk, hands typing on a laptop, wearing the black tee. Soft window light.",
        _ =>
            "Folded black tee on a concrete surface beside a notebook and pen, top-down editorial flat-lay.",
    };
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
    let prompt = format!(
        "Editorial 4:5 lifestyle photo. {scene} \
         The garment design (printed on chest / back, depending on shot): {brief}. \
         Photorealistic, magazine quality, soft natural light, slight film grain. \
         NO face visible (crop, back-of-head, or hidden by composition). \
         NO text overlay. Variation key: {sku}-v{variant}.",
        scene = scene, brief = theme_brief, sku = sku, variant = variant,
    );
    let img = crate::gemini::call_gemini(&prompt).await
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
    //    AOP rashguard (162) ignores chest position and prints all-over,
    //    but Printful still requires the field — pass the same chest box.
    let position = serde_json::json!({
        "area_width": 1800, "area_height": 2400,
        "width": 1260,      "height": 1260,
        "top": 380,         "left": 270
    });
    let create_body = serde_json::json!({
        "variant_ids": [printful_variant],
        "format": "png",
        "files": [{
            "placement": "front",
            "image_url": design_url,
            "position": position,
        }],
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
    if s.contains("-RASH") || s.contains("RASHGUARD") { return "rashguard_ls"; }
    if s.contains("-TEE")  || s.starts_with("MU-")    { return "tee"; }
    if s.contains("AUTO-")  && s.contains("-TEE-")    { return "tee"; }
    "tee"  // safe default for the spec block
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
    let sku = format!("AUTO-NL-{}-{}-{}", slug, kind.to_uppercase().replace('_', "-"), seed);

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
    let design_prompt = format!(
        "Print-ready chest graphic at 300 DPI on a pure white background. \
         Style brief: {}. NO model, NO mockup, just the artwork, centered. \
         Variation key: {}.",
        theme_brief, seed
    );
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
        let _ = conn.execute(
            "INSERT OR IGNORE INTO catalog_brands
             (slug, name, emoji, color_primary, tagline, is_active, revenue_share_pct)
             VALUES ('auto', 'AUTO (AI-generated)', '🤖', '#ffd700',
                     'Gemini × Printful POD · 30 分自動生成', 1, 0)",
            [],
        );
        let desc = format!("{} — {}", display, hook);
        let _ = conn.execute(
            "INSERT INTO catalog_products (
                sku, brand, label, description_ja, retail_price_jpy,
                printful_product_id, printful_variant_id, printful_placement,
                printful_print_w, printful_print_h,
                design_file, mockup_main_file, mockup_url_external,
                is_active, sort_order, status, fulfillment_route, legacy_source
             ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            rusqlite::params![
                &sku, "auto", desc, desc, retail_jpy,
                spec.printful_product_id, spec.printful_variant_id, spec.placement,
                0, 0,
                &url, &url, &url,
                1, 50,
                "live",
                if matches!(kind, "rashguard_ls"|"rashguard_black") { "printful_aop" } else { "printful_dtg" },
                "nl_add",
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
    let spent = spent_total_jpy(&conn);
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
            "cap_jpy": BUDGET_TOTAL_JPY,
            "remaining_jpy": BUDGET_TOTAL_JPY - spent,
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

    // Show the buy button whenever shop_checkout can build a Stripe
    // Session — that's either a pre-created stripe_price_id OR a positive
    // retail_price_jpy (price_data inline). Without this, auto-generated
    // SKUs (which deliberately skip price-id pre-mint) render as
    // "準備中" and customers never click — a critical conversion gap.
    let buy_button = if price_id.as_deref().unwrap_or("").starts_with("price_")
        || price_jpy > 0
    {
        format!(
            r#"<a class="buy" href="/api/shop/checkout?sku={}">買う ¥{} · 即購入 (Stripe + Printful 7-14 日 国際発送)</a>"#,
            urlencoding::encode(&sku),
            format_jpy(price_jpy),
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
    let trust_block = r##"<div class="trust-strip">
  <div class="ts-row">
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
</div>"##.to_string();

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
    let success_url = format!("{}/success?from=shop&sku={}", base_url, urlencoding::encode(&sku));
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

    // Idempotency: if catalog_orders already has this session id, skip.
    {
        let conn = db.lock().unwrap();
        let already: bool = conn
            .query_row(
                "SELECT 1 FROM catalog_orders WHERE stripe_session_id=? LIMIT 1",
                rusqlite::params![&session_id],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if already {
            tracing::info!("[catalog/fulfill] session {} already fulfilled, skip", session_id);
            return;
        }
    }

    let product = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT printful_product_id, printful_variant_id,
                    printful_sync_product_id, printful_sync_variant_id,
                    design_file, printful_placement
             FROM catalog_products WHERE sku=?",
            rusqlite::params![&sku],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, String>(5)?,
                ))
            },
        )
        .ok()
    };
    let Some((_pp_id, pf_variant_id, _sync_pid, sync_variant_id, design_file, placement)) = product
    else {
        tracing::warn!("[catalog/fulfill] sku {} not in catalog_products", sku);
        return;
    };

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
    let pf_variant_id = variant_override.unwrap_or(pf_variant_id);

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

    let retail_price = if currency == "jpy" {
        format!("{:.2}", amount_total as f64)
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

    // AOP rashguards (Printful product 301) require a `stitch_color`
    // option ('white' or 'black'). Default to black so the seams match
    // the dominant body of the print on dark rashguards. Verified live:
    // order #1 4xx'd with "Item 'stitch_color' option missing or has
    // an invalid value!" before this guard.
    let needs_stitch_color = matches!(_pp_id, 301 | 302 | 368 | 369 | 836);
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
            serde_json::json!({
                "variant_id": pf_variant_id,
                "quantity": 1,
                "retail_price": retail_price,
                "files": [{"url": file_url, "placement": placement}],
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
        "items": [item],
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
            );
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
    record_order_full(db, session_id, sku, amount, cust, shipping, pf_id, status, None);
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
          shipping_address_json, printful_order_id, printful_response_json, status)
         VALUES (?,?,?,?,?,?,?,?,?)",
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
}

fn list_products_paged(
    conn: &rusqlite::Connection,
    brand: Option<&str>,
    limit: i64,
    offset: i64,
) -> Vec<ProductRow> {
    let (sql, has_brand) = if brand.is_some() {
        (
            "SELECT sku, brand, description_ja, retail_price_jpy,
                    COALESCE(mockup_url_external, mockup_main_file)
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
                    COALESCE(mockup_url_external, mockup_main_file)
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
            price: r.get(3)?, img: r.get(4)?,
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
    format!(
        r##"<a class="card" href="/shop/{sku_enc}"><span class="img"><img src="{img}" alt="" loading="lazy" onerror="this.onerror=null;this.src='/static/designs/marker_zero.png';this.style.objectFit='contain';this.style.background='#0a0a0a';this.style.padding='28px'"></span><span class="body"><span class="brand">{brand}</span><span class="name">{name}</span><span class="price">¥{price}</span></span></a>"##,
        sku_enc = urlencoding::encode(&p.sku),
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
        // Gildan 18500 pullover hoodie (Black)
        146 => match sz.as_str() {
            "S" => Some(5529), "M" => Some(5530), "L" => Some(5531),
            "XL" => Some(5532), "2XL" | "XXL" => Some(5533),
            _ => None,
        },
        // Gildan 18000 crewneck sweatshirt (Black)
        145 => match sz.as_str() {
            "S" => Some(5402), "M" => Some(5403), "L" => Some(5404),
            "XL" => Some(5405), "2XL" | "XXL" => Some(5406),
            _ => None,
        },
        // AOP Men's Rash Guard (White) — 7 sizes
        301 => match sz.as_str() {
            "XS" => Some(9325), "S" => Some(9326), "M" => Some(9328),
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
//   • spend_or_refuse() inside generate_one — never goes over ¥100K.
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
        (auto, orders, spent_total_jpy(&conn))
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

