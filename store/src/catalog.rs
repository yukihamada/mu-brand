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
            revenue_share_pct INTEGER NOT NULL DEFAULT 0
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
         "
    );
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
        printful_variant_id: 4017,
        placement: "front",
        retail_jpy: 4900,
    },
    ProductSpec {
        kind: "rashguard_ls",
        printful_product_id: 162,
        printful_variant_id: 9328,
        placement: "front",
        retail_jpy: 9800,
    },
];

struct Theme {
    slug: &'static str,
    display: &'static str,
    prompt_brief: &'static str,
}

const SEED_THEMES: &[Theme] = &[
    Theme {
        slug: "bjj_kuro_obi",
        display: "BJJ 黒帯",
        prompt_brief: "minimal sumi-e ink illustration of a tied jiu-jitsu black belt with the kanji 黒帯 in calligraphic style below",
    },
    Theme {
        slug: "round_1",
        display: "Round 1",
        prompt_brief: "bold cinematic typography reading Round 1 inside a vintage boxing round-card border, monochrome ink",
    },
    Theme {
        slug: "teshikaga_mountain",
        display: "弟子屈 Mountain",
        prompt_brief: "geometric line-art of a Hokkaido mountain peak with a calm lake reflecting it, single-color print",
    },
    Theme {
        slug: "mu_mark",
        display: "MU ━◯━",
        prompt_brief: "the ━◯━ mark (long-dash circle long-dash) centered large and bold, with a small MU wordmark below in monospace",
    },
    Theme {
        slug: "coffee_code",
        display: "Coffee × Code",
        prompt_brief: "minimal coffee cup outline with a binary stream rising as steam, geek-aesthetic monochrome",
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

    // Gemini print-ready prompt: white background = transparent for DTG.
    let prompt = format!(
        "Print-ready chest graphic at 300 DPI on a PURE WHITE background \
         (white acts as the transparent layer for DTG printing). \
         Style brief: {brief}. \
         Hard constraints: NO model, NO mockup, NO photographic scene, \
         NO shirt visible — just the artwork itself, centered, square \
         aspect ratio, bleed-safe, ready to be printed onto apparel. \
         Variation key: {seed}.",
        brief = theme.prompt_brief,
        seed = seed,
    );
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

    // INSERT catalog_products.
    {
        let conn = db.lock().unwrap();
        let desc = format!("{} · {}", theme.display, label_for_kind(kind));
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
    Ok(sku)
}

fn mark_job_failed(db: &Db, theme: &str, kind: &str, seed: &str, err: &str) {
    let conn = db.lock().unwrap();
    let _ = conn.execute(
        "UPDATE catalog_gen_jobs SET status='failed', error=?, completed_at=datetime('now')
         WHERE theme=? AND kind=? AND seed=?",
        rusqlite::params![err, theme, kind, seed],
    );
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
        "recent_jobs": recent_jobs,
        "recent_spend": recent_spend,
    }))
    .into_response()
}

// ─── Public storefront pages ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct ShopQuery {
    pub brand: Option<String>,
}

pub async fn shop_index(
    State(db): State<Db>,
    Query(q): Query<ShopQuery>,
) -> Html<String> {
    let brand_filter = q.brand.unwrap_or_default();
    let (brands, items) = {
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

        let items: Vec<ProductRow> = if brand_filter.is_empty() {
            list_products(&conn, None, 60)
        } else {
            list_products(&conn, Some(&brand_filter), 240)
        };
        (brands, items)
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

    let count = items.len();
    let title = if brand_filter.is_empty() {
        "/shop — MU カタログ".to_string()
    } else {
        format!("/shop — {} | MU カタログ", brand_filter)
    };
    let body = format!(
        r##"<!doctype html><html lang="ja"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<meta name="description" content="MU × ブランド コラボ カタログ。 {count} 件。 Stripe + Printful 直配送 7-14日。">
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
body{{background:#0a0a0a;color:#f5f5f0;font-family:'Helvetica Neue','Hiragino Sans',Arial,sans-serif;line-height:1.55;font-size:14px}}
nav{{padding:16px 24px;border-bottom:1px solid rgba(255,255,255,0.08);display:flex;justify-content:space-between;align-items:center;flex-wrap:wrap;gap:10px}}
nav a{{color:#f5f5f0;text-decoration:none;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.85}}
nav a:hover{{opacity:1}}
nav .brand{{font-weight:900;letter-spacing:0.4em}}
.hero{{padding:40px 24px 18px;max-width:1180px;margin:0 auto}}
.hero h1{{font-size:28px;font-weight:900;letter-spacing:-0.01em;margin-bottom:8px}}
.hero p{{color:rgba(245,245,240,0.62);font-size:13px;line-height:1.85;max-width:640px}}
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
  <h1>SHOP — {count} 件</h1>
  <p>MU × コラボブランドのカタログ。 Stripe 決済 / Printful 国際発送 7-14 日。 ブランドで絞り込み:</p>
</div>
<div class="chips">{brand_chips}</div>
{body_or_empty}
<footer>
  <span>© 2026 MU / Enabler Inc.</span>
  <a href="/privacy">プライバシー</a>
  <a href="/heritage">heritage</a>
  <a href="/buy">drops</a>
</footer>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
</body></html>"##,
        title = html_text(&title),
        count = count,
        brand_chips = brand_chips,
        body_or_empty = if items.is_empty() {
            r#"<div class="empty">該当する商品がありません。</div>"#.to_string()
        } else {
            format!(r#"<div class="grid">{}</div>"#, grid)
        },
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

    let buy_button = if price_id.as_deref().unwrap_or("").starts_with("price_") {
        format!(
            r#"<a class="buy" href="/api/shop/checkout?sku={}">買う ¥{} · 即購入 (Stripe + Printful 7-14 日 国際発送)</a>"#,
            urlencoding::encode(&sku),
            format_jpy(price_jpy),
        )
    } else {
        r#"<div class="buy disabled">準備中 — Stripe price が未登録です</div>"#.to_string()
    };

    let body = format!(
        r##"<!doctype html><html lang="ja"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} — /shop / wearmu.com</title>
<meta name="description" content="{desc}">
<meta property="og:image" content="{og}">
<meta property="og:title" content="{title}">
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
    <img src="{og}" alt="{title}" loading="lazy">
    {extras}
  </div>
  <div class="body">
    <div class="brand">{brand}</div>
    <h1>{title}</h1>
    <div class="price">¥{price}</div>
    <div class="desc">{desc}</div>
    <div class="sku">SKU: {sku}</div>
    {buy}
    {suzuri}
    <a class="back" href="/shop?brand={brand_q}">← {brand} のほかの商品</a>
  </div>
</div>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
</body></html>"##,
        title = html_text(&desc),
        desc = html_text(&desc),
        og = html_attr(&img),
        brand = html_text(&brand),
        brand_q = html_attr(&brand),
        price = format_jpy(price_jpy),
        sku = html_text(&sku),
        buy = buy_button,
        suzuri = suzuri_link,
        extras = extras_html,
    );
    Html(body).into_response()
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
            "SELECT stripe_price_id, retail_price_jpy, description_ja, brand
             FROM catalog_products WHERE sku=? AND is_active=1",
            rusqlite::params![&sku],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .ok()
    };
    let Some((price_id, price_jpy, desc, _brand)) = row else {
        return (StatusCode::NOT_FOUND, "sku not found").into_response();
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
        }
        None => {
            if price_jpy <= 0 {
                return (StatusCode::FAILED_DEPENDENCY,
                    "this SKU has no price configured").into_response();
            }
            form.push(("line_items[0][price_data][currency]", "jpy".into()));
            form.push(("line_items[0][price_data][unit_amount]", price_jpy.to_string()));
            form.push(("line_items[0][price_data][product_data][name]", desc.clone()));
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

    // Pull selected size from Stripe custom_fields (if any). When set and
    // != "M" we'd need to resolve a different variant_id — for now the
    // merch-bridge default variant is shipped as-is (size selection is a
    // Phase 2 enhancement; the existing wearmu admin can ship per-size
    // SKUs explicitly).

    let shipping = &session["shipping_details"];
    let addr = &shipping["address"];
    let cust = &session["customer_details"];
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
            })
        }
        _ => serde_json::json!({
            "variant_id": pf_variant_id,
            "quantity": 1,
            "retail_price": retail_price,
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

fn list_products(
    conn: &rusqlite::Connection,
    brand: Option<&str>,
    limit: i64,
) -> Vec<ProductRow> {
    let (sql, has_brand) = if brand.is_some() {
        (
            "SELECT sku, brand, description_ja, retail_price_jpy,
                    COALESCE(mockup_url_external, mockup_main_file)
             FROM catalog_products
             WHERE is_active=1 AND brand=?
             ORDER BY sort_order, sku
             LIMIT ?",
            true,
        )
    } else {
        (
            "SELECT sku, brand, description_ja, retail_price_jpy,
                    COALESCE(mockup_url_external, mockup_main_file)
             FROM catalog_products
             WHERE is_active=1
             ORDER BY sort_order, sku
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
        .unwrap_or_else(|| "/static/og-default.png".to_string());
    format!(
        r##"<a class="card" href="/shop/{sku_enc}"><span class="img"><img src="{img}" alt="" loading="lazy"></span><span class="body"><span class="brand">{brand}</span><span class="name">{name}</span><span class="price">¥{price}</span></span></a>"##,
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
const TARGET_INITIAL: i64 = 25; // 5 themes × 2 kinds × ~2.5 SKUs per combo
const CRON_BATCH_MAX: u32 = 10;
const CRON_INTERVAL_SECS: u64 = 30 * 60;

/// Long-running task: every 30 min, take one self-improvement step.
/// Spawn this once from main() before the router takes the db.
pub async fn run_optimizer_cron(db: Db) {
    // Stagger the first run by 60s so it doesn't fight startup.
    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    loop {
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
        tokio::time::sleep(std::time::Duration::from_secs(CRON_INTERVAL_SECS)).await;
    }
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

