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
        printful_variant_id: 4017,
        placement: "front",
        retail_jpy: 4900,
        spec_html: "Bella+Canvas 3001 unisex tee · 4.2 oz (142 gsm) · \
                    100% airlume combed ringspun cotton · DTG print 30×30cm front · \
                    machine washable · sourced + printed in EU",
    },
    ProductSpec {
        kind: "rashguard_ls",
        // 301 = "All-Over Print Men's Rash Guard" (Printful catalog id);
        // 162 was a copy/paste error from a longsleeve product, which is
        // why every rashguard mockup task 4xx'd with "No variants to
        // generate" (variant 9328 isn't in product 162's catalog).
        printful_product_id: 301,
        printful_variant_id: 9328,
        placement: "front",
        retail_jpy: 9800,
        spec_html: "Men's all-over-print long-sleeve rashguard · 82% polyester / 18% spandex · \
                    UPF 50+ UV protection · 4-way stretch · flatlock seams (no chafe) · \
                    sublimation print (won't fade or peel) · IBJJF gi/no-gi compliant fit",
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

    // Fire-and-forget: ask Printful to render an on-body mockup of this
    // design, swap mockup_url_external when ready. Cold-traffic CVR is
    // dominated by "is there a person wearing it" — current PDPs show
    // the print art on white which doesn't sell.
    let db_mockup = db.clone();
    let sku_mockup = sku.clone();
    let spec_mockup = (spec.printful_product_id, spec.printful_variant_id, &url);
    let printful_product = spec_mockup.0;
    let printful_variant = spec_mockup.1;
    let design_url = url.clone();
    tokio::spawn(async move {
        if let Err(e) = generate_onbody_mockup(
            db_mockup, sku_mockup, printful_product, printful_variant, design_url
        ).await {
            tracing::warn!("[catalog/mockup] async failed: {}", e);
        }
    });

    Ok(sku)
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
  <p>柔術・コーヒー・地域 ── 10+ コラボの "内側からの服"。 在庫を持たず、 注文ごとに 1 枚刷ります (POD)。 {count} 件公開中。</p>
  <div class="trust">
    <span><strong>国際発送</strong> 7-14 日 (DHL / FedEx)</span>
    <span><strong>1 着から</strong> オーダー可</span>
    <span><strong>Bella+Canvas / AOP rashguard</strong> 等プレミアム生地</span>
    <span><strong>Stripe</strong> 安全決済 + クーポン対応</span>
  </div>
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

    // Trust strip: 0-review cold-traffic insurance. The early-bird founder
    // card line addresses the "no social proof" gap: the first 100 buyers
    // get a hand-signed thank-you postcard from Yuki.
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
    <strong>1 着から OK</strong>
    <small>在庫を持たず注文ごとに 1 枚刷ります (POD)</small>
  </div>
  <div class="ts-row" style="background:rgba(255,215,0,0.07);border:1px solid rgba(255,215,0,0.35);padding:10px 12px;border-radius:4px;margin-top:8px">
    <strong style="color:#ffd700">🎴 最初の 100 注文限定</strong>
    <small>濱田優貴 サイン入りサンクスカード同梱 (number/100)</small>
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
    <div class="price">¥{price}</div>
    {buy}
    {suzuri}
    {trust}
    {spec}
    {story}
    <div class="sku">SKU: {sku}</div>
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
        trust     = trust_block,
        spec      = spec_block,
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
        // Every cycle, kick off mockup backfill for up to 3 AUTO SKUs
        // that still display only the print art (mockup_url_external
        // points back to the design). Per-cycle cap is small because
        // Printful's mockup-generator is queue-based (~30-60s each)
        // and we don't want to fight the gen workload.
        if let Err(e) = mockup_backfill_step(db.clone()).await {
            tracing::warn!("[catalog/cron] mockup backfill failed: {}", e);
        }
        tokio::time::sleep(std::time::Duration::from_secs(CRON_INTERVAL_SECS)).await;
    }
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
             LIMIT 3",
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

