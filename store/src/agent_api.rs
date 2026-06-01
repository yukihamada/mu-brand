// agent_api.rs — catalog-native, email-keyed AGENT API for wearmu.com.
//
// Goal: make MU trivially extensible by AI agents. An agent discovers the
// site via GET /llms.txt, self-serves an email-verified API key (same
// magic-link path humans use), then creates its own store (a catalog_brands
// row) and products (catalog_products rows). Products land status='review',
// is_active=0 and stay invisible to shoppers until an MA-council member
// approves them.
//
// Contract compliance (docs/CATALOG_CONTRACT.md):
//   • NO new tables, NO new columns. Owner attribution lives ONLY in
//     catalog_brands.config_json ({owner_email, approval_required, ...}).
//   • Products are catalog_products rows via catalog::agent_insert_product,
//     which validates `kind` against the verified PRODUCT_SPECS whitelist —
//     agents NEVER pass raw Printful ids or sub-genka prices.
//   • Approval = MA council (mirrors the authoritative test at main.rs:58093).
//
// All handlers live here; routes are registered in main.rs near the Router
// block. Auth reuses crate::bearer_or_session_email (Bearer / ?api_key= /
// cookie). The register/verify endpoints delegate to the existing collab
// magic-link onboarding so there is exactly one key system.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use std::collections::HashMap;

use crate::Db;

const RESERVED_SLUGS: &[&str] = &[
    "auto", "ma", "mu", "mugen", "muon", "roll", "atsume", "yuma", "elepote",
    "bjj", "kokon", "jiuflow", "sweep", "coffee", "moon", "tokyo", "zen",
    "code", "nakamura", "shop", "you", "proposal",
];

/// Per-email product-creation cap per rolling hour. Reuses the blog_rate_limit
/// table/pattern (main.rs:45573) with a synthetic per-email bucket key.
const AGENT_PRODUCTS_PER_HOUR: i64 = 20;

// ─── Helpers ──────────────────────────────────────────────────────────

/// True if `email` is an MA-council member authorized to approve agent
/// products. Mirrors the authoritative test at main.rs:58093:
///   tier='full' member, OR owns an MA piece (mu_purchases brand='ma').
pub fn is_ma_council_email(conn: &rusqlite::Connection, email: &str) -> bool {
    let e = email.to_lowercase();
    let by_member: bool = conn.query_row(
        "SELECT 1 FROM ma_council_members WHERE LOWER(email)=? AND tier='full' LIMIT 1",
        rusqlite::params![e], |_| Ok(true),
    ).unwrap_or(false);
    if by_member { return true; }
    conn.query_row(
        "SELECT 1 FROM mu_purchases WHERE LOWER(email)=? AND brand='ma' LIMIT 1",
        rusqlite::params![e], |_| Ok(true),
    ).unwrap_or(false)
}

/// Per-email hourly rate limit using the existing blog_rate_limit table
/// (ip TEXT, hour_bucket INTEGER, hits INTEGER). We key the `ip` column with
/// a namespaced "agent:<email>" so we don't collide with real IP buckets.
/// Returns true if the request is allowed (and records the hit), false if the
/// caller is over AGENT_PRODUCTS_PER_HOUR for the current hour.
fn agent_rate_ok(conn: &rusqlite::Connection, email: &str) -> bool {
    let now_s: i64 = crate::chrono_now().parse().unwrap_or(0);
    let hour_bucket = now_s / 3600;
    let key = format!("agent:{}", email);
    let _ = conn.execute(
        "INSERT INTO blog_rate_limit (ip, hour_bucket, hits) VALUES (?,?,1)
         ON CONFLICT(ip, hour_bucket) DO UPDATE SET hits = hits + 1",
        rusqlite::params![key, hour_bucket],
    );
    let _ = conn.execute(
        "DELETE FROM blog_rate_limit WHERE hour_bucket < ?",
        rusqlite::params![hour_bucket - 24],
    );
    let hits: i64 = conn.query_row(
        "SELECT hits FROM blog_rate_limit WHERE ip=? AND hour_bucket=?",
        rusqlite::params![key, hour_bucket], |r| r.get(0),
    ).unwrap_or(0);
    hits <= AGENT_PRODUCTS_PER_HOUR
}

fn json_err(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({"error": msg}))).into_response()
}

/// MU-credit cost (¥) charged per AI-generated design. Env-tunable; the
/// 200pt welcome credit covers a handful of generations at the default.
fn agent_ai_gen_cost_jpy() -> i64 {
    std::env::var("AGENT_AI_GEN_COST_JPY")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|&c| c >= 0)
        .unwrap_or(50)
}

/// Whether the AI-generation arm of POST /api/agent/products is enabled.
fn agent_ai_gen_enabled() -> bool {
    std::env::var("AGENT_AI_GEN_ENABLED")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Short, deterministic hex digest used to name agent-uploaded artwork in R2.
fn short_hash(s: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Resolve the caller email from Bearer / api_key / cookie, or 401.
fn require_email(db: &Db, headers: &HeaderMap, q: Option<&HashMap<String, String>>) -> Result<String, Response> {
    crate::bearer_or_session_email(db, headers, q).ok_or_else(|| json_err(
        StatusCode::UNAUTHORIZED,
        "missing/invalid API key; register at POST /api/agent/register then send Authorization: Bearer <key>",
    ))
}

// ─── Onboarding (delegates to the existing collab magic-link path) ──────

#[derive(Deserialize)]
pub struct RegisterBody { pub email: String }

/// POST /api/agent/register {email} — emails a 6-digit code (reuses the
/// collab onboarding handler verbatim, so there is one key system).
pub async fn agent_register(
    State(db): State<Db>,
    Json(body): Json<RegisterBody>,
) -> Response {
    // Delegate to the existing handler (same {email} contract, same email).
    crate::collab_auth_start_core(&db, &body.email).await
}

#[derive(Deserialize)]
pub struct RegisterVerifyBody { pub email: String, pub code: String }

/// One-time welcome credit (¥-denominated MU points) granted to an agent the
/// first time they verify their email. Lets new agents try paid features
/// (e.g. AI generation) without an upfront purchase.
const AGENT_WELCOME_CREDIT_JPY: i64 = 200;

/// POST /api/agent/register/verify {email, code} — verifies the code, mints
/// the session token (= API key), returns it in an agent-friendly shape.
/// On the *first* successful verification per email we also grant a one-time
/// welcome credit (idempotent via the `agent_welcome` ledger reason, so
/// repeat logins via the same magic-link flow never re-grant).
pub async fn agent_register_verify(
    State(db): State<Db>,
    Json(body): Json<RegisterVerifyBody>,
) -> Response {
    match crate::collab_auth_verify_core(&db, &body.email, &body.code) {
        Ok(token) => {
            let welcome = {
                let conn = db.lock().unwrap();
                let email_lc = body.email.trim().to_lowercase();
                let already: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM mu_credit_ledger WHERE email=? AND reason='agent_welcome'",
                    rusqlite::params![email_lc], |r| r.get(0),
                ).unwrap_or(0);
                if already == 0
                    && crate::mu_credit_apply(&conn, &email_lc, AGENT_WELCOME_CREDIT_JPY, "agent_welcome", None)
                {
                    AGENT_WELCOME_CREDIT_JPY
                } else {
                    0
                }
            };
            Json(serde_json::json!({
                "ok": true,
                "api_key": token,
                "usage": "send as Authorization: Bearer <api_key>",
                "welcome_credit_jpy": welcome,
            })).into_response()
        }
        Err((status, msg)) => json_err(status, &msg),
    }
}

// ─── GET /api/agent/me ──────────────────────────────────────────────────

pub async fn agent_me(
    State(db): State<Db>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let email = match require_email(&db, &headers, Some(&q)) { Ok(e) => e, Err(r) => return r };
    let conn = db.lock().unwrap();
    let balance = crate::mu_credit_balance(&conn, &email);
    let is_council = is_ma_council_email(&conn, &email);

    // Owned stores + per-status product counts.
    let mut stores: Vec<serde_json::Value> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT slug, name FROM catalog_brands
         WHERE json_extract(config_json,'$.owner_email')=?"
    ) {
        let rows = stmt.query_map(rusqlite::params![email], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        });
        if let Ok(rows) = rows {
            for row in rows.flatten() {
                let (slug, name) = row;
                let review: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM catalog_products WHERE brand=? AND status='review'",
                    rusqlite::params![slug], |r| r.get(0)).unwrap_or(0);
                let live: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM catalog_products WHERE brand=? AND status='live'",
                    rusqlite::params![slug], |r| r.get(0)).unwrap_or(0);
                stores.push(serde_json::json!({
                    "slug": slug, "name": name,
                    "counts": {"review": review, "live": live},
                    "store_url": format!("https://wearmu.com/shop?brand={}", slug),
                }));
            }
        }
    }

    Json(serde_json::json!({
        "email": email,
        "mu_credits_balance": balance,
        "is_ma_council": is_council,
        "stores": stores,
        "limits": {
            "products_per_hour": AGENT_PRODUCTS_PER_HOUR,
            "kinds": catalog_kind_names(),
            "ai_gen": {
                "enabled": agent_ai_gen_enabled(),
                "cost_jpy": agent_ai_gen_cost_jpy(),
                "note": "pass ai_prompt instead of design_url to generate artwork; cost is deducted from mu_credits_balance and refunded if generation fails",
            },
        },
    })).into_response()
}

fn catalog_kind_names() -> Vec<serde_json::Value> {
    crate::catalog::agent_product_kinds().into_iter().map(|k| serde_json::json!({
        "kind": k.kind,
        "price_floor_jpy": k.price_floor_jpy,
        "spec": k.spec_html,
    })).collect()
}

// ─── POST /api/agent/stores ─────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateStoreBody {
    pub slug: String,
    pub name: String,
    pub emoji: Option<String>,
    pub color_primary: Option<String>,
    pub tagline: Option<String>,
}

fn slug_valid(s: &str) -> bool {
    let n = s.len();
    (3..=40).contains(&n) && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

pub async fn agent_create_store(
    State(db): State<Db>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<CreateStoreBody>,
) -> Response {
    let email = match require_email(&db, &headers, Some(&q)) { Ok(e) => e, Err(r) => return r };
    let slug = body.slug.trim().to_lowercase();
    if !slug_valid(&slug) {
        return json_err(StatusCode::BAD_REQUEST, "slug must match ^[a-z0-9_-]{3,40}$");
    }
    if RESERVED_SLUGS.contains(&slug.as_str()) {
        return json_err(StatusCode::CONFLICT, "slug is reserved");
    }
    let name = body.name.trim();
    if name.is_empty() || name.len() > 80 {
        return json_err(StatusCode::BAD_REQUEST, "name required (<=80 chars)");
    }
    let emoji = body.emoji.as_deref().unwrap_or("🛍️");
    let color = body.color_primary.as_deref().unwrap_or("#888");
    let tagline = body.tagline.as_deref().unwrap_or("");
    let now = crate::chrono_now();
    let config = serde_json::json!({
        "owner_email": email,
        "approval_required": true,
        "created_via": "agent_api",
        "created_at": now,
    }).to_string();

    let conn = db.lock().unwrap();
    // Reserved-against-existing: if a brand with this slug already exists and
    // is NOT owned by the caller, reject. The ON CONFLICT below also guards
    // ownership inside the SQL to avoid a TOCTOU race, but this gives a clean
    // 403 message for the common case.
    let existing_owner: Option<String> = conn.query_row(
        "SELECT json_extract(config_json,'$.owner_email') FROM catalog_brands WHERE slug=?",
        rusqlite::params![slug], |r| r.get(0),
    ).ok().flatten();
    if let Some(owner) = &existing_owner {
        if owner.to_lowercase() != email {
            return json_err(StatusCode::FORBIDDEN, "slug owned by another email");
        }
    } else {
        // Row exists but has no owner_email (a pre-seeded MU brand) → reject.
        let row_exists: bool = conn.query_row(
            "SELECT 1 FROM catalog_brands WHERE slug=?", rusqlite::params![slug], |_| Ok(true),
        ).unwrap_or(false);
        if row_exists {
            return json_err(StatusCode::FORBIDDEN, "slug owned by another email");
        }
    }

    // INSERT, or UPDATE only when the existing row's owner_email == caller.
    // The WHERE guard in DO UPDATE makes the owner check atomic.
    let n = conn.execute(
        "INSERT INTO catalog_brands
            (slug, name, emoji, color_primary, tagline, is_active, revenue_share_pct, config_json)
         VALUES (?,?,?,?,?,1,0,?)
         ON CONFLICT(slug) DO UPDATE SET
            name=excluded.name, emoji=excluded.emoji,
            color_primary=excluded.color_primary, tagline=excluded.tagline
         WHERE json_extract(catalog_brands.config_json,'$.owner_email')=?",
        rusqlite::params![slug, name, emoji, color, tagline, config, email],
    ).unwrap_or(0);
    if n == 0 {
        // ON CONFLICT matched but the owner-guard failed → not the caller's.
        return json_err(StatusCode::FORBIDDEN, "slug owned by another email");
    }

    Json(serde_json::json!({
        "ok": true,
        "slug": slug,
        "store_url": format!("https://wearmu.com/shop?brand={}", slug),
    })).into_response()
}

// ─── POST /api/agent/products ───────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateProductBody {
    pub store: String,
    pub label: String,
    pub description: String,
    pub kind: String,
    pub design_url: Option<String>,
    pub ai_prompt: Option<String>,
    pub price_jpy: Option<i64>,
}

pub async fn agent_create_product(
    State(db): State<Db>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<CreateProductBody>,
) -> Response {
    let email = match require_email(&db, &headers, Some(&q)) { Ok(e) => e, Err(r) => return r };
    let store = body.store.trim().to_lowercase();
    let label = body.label.trim();
    let description = body.description.trim();
    if label.is_empty() || label.len() > 120 {
        return json_err(StatusCode::BAD_REQUEST, "label required (<=120 chars)");
    }
    if description.is_empty() || description.len() > 600 {
        return json_err(StatusCode::BAD_REQUEST, "description required (<=600 chars)");
    }

    // Design source resolution (decide BEFORE taking the DB lock).
    // - design_url present → validate https, no AI spend.
    // - else ai_prompt present → gated OFF behind AGENT_AI_GEN_ENABLED.
    // - else → 400.
    let design_file: String = if let Some(url) = body.design_url.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !url.starts_with("https://") || url.len() > 2000 {
            return json_err(StatusCode::BAD_REQUEST, "design_url must be an absolute https:// URL");
        }
        url.to_string()
    } else if let Some(brief) = body.ai_prompt.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !agent_ai_gen_enabled() {
            return json_err(StatusCode::SERVICE_UNAVAILABLE, "ai_gen disabled; pass design_url");
        }
        if brief.len() > 600 {
            return json_err(StatusCode::BAD_REQUEST, "ai_prompt too long (<=600 chars)");
        }
        let cost = agent_ai_gen_cost_jpy();
        // Atomic ownership-check + credit-charge under ONE lock, BEFORE the
        // slow async Gemini call — so we never generate for free or for a
        // store we don't own. Refunded below if generation/upload fails.
        {
            let conn = db.lock().unwrap();
            let owner: Option<String> = conn.query_row(
                "SELECT json_extract(config_json,'$.owner_email') FROM catalog_brands WHERE slug=?",
                rusqlite::params![store], |r| r.get(0),
            ).ok().flatten();
            match owner {
                Some(o) if o.to_lowercase() == email => {}
                _ => return json_err(StatusCode::FORBIDDEN, "you do not own this store"),
            }
            if !crate::mu_credit_apply(&conn, &email, -cost, "agent_ai_gen", None) {
                let bal = crate::mu_credit_balance(&conn, &email);
                return json_err(StatusCode::PAYMENT_REQUIRED, &format!(
                    "insufficient MU credits for AI generation: need ¥{}, balance ¥{}", cost, bal));
            }
        }
        // Print-ready prompt (mirrors catalog::generate_one). rashguard_black
        // wants a full-black AOP canvas; everything else a white-bg DTG graphic.
        let gen_prompt = if body.kind.trim() == "rashguard_black" {
            format!(
                "Square 300 DPI artwork for all-over print on a long-sleeve rashguard. \
                 Fill the entire canvas with PURE BLACK (#0a0a0a). Centered on the chest: \
                 {brief}, rendered in WHITE or light ivory so it pops against the black. \
                 Hard constraints: NO model, NO mockup, NO photographic scene — just the \
                 print-ready square artwork.", brief = brief)
        } else {
            format!(
                "Print-ready chest graphic at 300 DPI on a PURE WHITE background (white acts \
                 as the transparent layer for DTG printing). Design brief: {brief}. \
                 Hard constraints: NO model, NO mockup, NO photographic scene, NO shirt \
                 visible — just the artwork itself, centered, square aspect ratio, bleed-safe.",
                brief = brief)
        };
        let img = match crate::gemini::call_gemini(&gen_prompt).await {
            Ok(i) => i,
            Err(e) => {
                let conn = db.lock().unwrap();
                let _ = crate::mu_credit_apply(&conn, &email, cost, "agent_ai_gen_refund", None);
                return json_err(StatusCode::BAD_GATEWAY, &format!("AI generation failed: {}", e));
            }
        };
        // Host on R2 (Printful's worker must be able to fetch it).
        let key = format!("catalog/agent/{}-{}.png", store, short_hash(&format!("{}|{}", brief, label)));
        match crate::store_r2_bytes(&key, &img.bytes, &img.mime).await {
            Some(u) => u,
            None => {
                let conn = db.lock().unwrap();
                let _ = crate::mu_credit_apply(&conn, &email, cost, "agent_ai_gen_refund", None);
                return json_err(StatusCode::BAD_GATEWAY, "AI image hosting (R2) upload failed");
            }
        }
    } else {
        return json_err(StatusCode::BAD_REQUEST, "provide either design_url or ai_prompt");
    };

    let conn = db.lock().unwrap();

    // Owner check: load the store's owner_email from config_json.
    let owner: Option<String> = conn.query_row(
        "SELECT json_extract(config_json,'$.owner_email') FROM catalog_brands WHERE slug=?",
        rusqlite::params![store], |r| r.get(0),
    ).ok().flatten();
    match owner {
        Some(o) if o.to_lowercase() == email => {}
        _ => return json_err(StatusCode::FORBIDDEN, "you do not own this store"),
    }

    // Per-email rate limit.
    if !agent_rate_ok(&conn, &email) {
        return json_err(StatusCode::TOO_MANY_REQUESTS,
            "rate limit: 20 products/hour per email");
    }

    // Catalog-native insert (validates kind + applies price floor).
    let sku = match crate::catalog::agent_insert_product(
        &conn, &store, label, description, body.kind.trim(), &design_file, body.price_jpy,
    ) {
        Ok(s) => s,
        Err(e) => return json_err(StatusCode::BAD_REQUEST, &e),
    };

    Json(serde_json::json!({
        "ok": true,
        "sku": sku,
        "status": "review",
        "note": "pending MA council approval",
        "pdp_url": format!("https://wearmu.com/shop/{}", sku),
    })).into_response()
}

// ─── MA approval (is_ma_council_email-gated) ────────────────────────────

/// Resolve caller email + assert MA-council membership, or return the error
/// Response (401 unauth / 403 not-council).
fn require_ma_council(db: &Db, headers: &HeaderMap, q: Option<&HashMap<String, String>>) -> Result<String, Response> {
    // Owner override: a valid ADMIN_TOKEN is the highest authority and may
    // approve/reject (bootstraps the council + lets the operator ship).
    if admin_token_present(headers, q) {
        return Ok("admin".to_string());
    }
    let email = require_email(db, headers, q)?;
    let is_council = {
        let conn = db.lock().unwrap();
        is_ma_council_email(&conn, &email)
    };
    if !is_council {
        return Err(json_err(StatusCode::FORBIDDEN, "MA council members only"));
    }
    Ok(email)
}

/// True when the request carries the ADMIN_TOKEN (Authorization: Bearer,
/// X-Admin-Token header, or ?token=/?admin_token= query). Constant-time-ish
/// compare so the operator can approve agent products as the highest authority.
fn admin_token_present(headers: &HeaderMap, q: Option<&HashMap<String, String>>) -> bool {
    let expected = std::env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() {
        return false;
    }
    let mut cand: Option<String> = None;
    if let Some(a) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(t) = a.strip_prefix("Bearer ").or_else(|| a.strip_prefix("bearer ")) {
            cand = Some(t.trim().to_string());
        }
    }
    if cand.is_none() {
        if let Some(t) = headers.get("x-admin-token").and_then(|v| v.to_str().ok()) {
            cand = Some(t.trim().to_string());
        }
    }
    if cand.is_none() {
        if let Some(qq) = q {
            cand = qq.get("token").or_else(|| qq.get("admin_token")).cloned();
        }
    }
    match cand {
        Some(t) => {
            t.len() == expected.len()
                && t.bytes().zip(expected.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0
        }
        None => false,
    }
}

pub async fn ma_review_queue(
    State(db): State<Db>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if let Err(r) = require_ma_council(&db, &headers, Some(&q)) { return r; }
    let conn = db.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT sku, brand, label, retail_price_jpy, COALESCE(design_file,'')
         FROM catalog_products
         WHERE status='review' AND legacy_source='agent_api'
         ORDER BY created_at DESC LIMIT 200"
    ) { Ok(s) => s, Err(e) => return json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()) };
    let rows: Vec<serde_json::Value> = stmt.query_map([], |r| {
        Ok(serde_json::json!({
            "sku": r.get::<_, String>(0)?,
            "brand": r.get::<_, String>(1)?,
            "label": r.get::<_, String>(2)?,
            "retail_price_jpy": r.get::<_, i64>(3)?,
            "design_file": r.get::<_, String>(4)?,
        }))
    }).map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default();
    Json(serde_json::json!({"queue": rows, "count": rows.len()})).into_response()
}

pub async fn ma_review_approve(
    State(db): State<Db>,
    headers: HeaderMap,
    Path(sku): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let approver = match require_ma_council(&db, &headers, Some(&q)) { Ok(e) => e, Err(r) => return r };
    let now = crate::chrono_now();
    let brand: String;
    let label: String;
    {
        let conn = db.lock().unwrap();
        // Must be an agent product currently in review.
        let row: Option<(String, String)> = conn.query_row(
            "SELECT brand, label FROM catalog_products
             WHERE sku=? AND status='review' AND legacy_source='agent_api'",
            rusqlite::params![sku], |r| Ok((r.get(0)?, r.get(1)?)),
        ).ok();
        let Some((b, l)) = row else {
            return json_err(StatusCode::CONFLICT, "product not in review (already decided or not found)");
        };
        brand = b; label = l;
        let _ = conn.execute(
            "UPDATE catalog_products SET status='live', is_active=1, updated_at=datetime('now') WHERE sku=?",
            rusqlite::params![sku],
        );
        // Append {approver_email, approved_at} into the brand's
        // config_json.approvals array (JSON-only attribution, no new column).
        let cfg: Option<String> = conn.query_row(
            "SELECT config_json FROM catalog_brands WHERE slug=?",
            rusqlite::params![brand], |r| r.get(0),
        ).ok().flatten();
        let mut cfg_v: serde_json::Value = cfg.as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if !cfg_v.is_object() { cfg_v = serde_json::json!({}); }
        let arr = cfg_v.as_object_mut().unwrap()
            .entry("approvals").or_insert_with(|| serde_json::json!([]));
        if let Some(a) = arr.as_array_mut() {
            a.push(serde_json::json!({
                "sku": sku, "approver_email": approver, "approved_at": now,
            }));
        }
        let _ = conn.execute(
            "UPDATE catalog_brands SET config_json=? WHERE slug=?",
            rusqlite::params![cfg_v.to_string(), brand],
        );
    }
    let alert = format!(
        "✅ MU agent product APPROVED\nsku: {}\nbrand: {}\nlabel: {}\nby: {}",
        sku, brand, label, approver,
    );
    tokio::spawn(async move { crate::send_telegram_message(&alert).await; });

    Json(serde_json::json!({
        "ok": true, "sku": sku, "status": "live", "approved_at": now,
    })).into_response()
}

pub async fn ma_review_reject(
    State(db): State<Db>,
    headers: HeaderMap,
    Path(sku): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if let Err(r) = require_ma_council(&db, &headers, Some(&q)) { return r; }
    let conn = db.lock().unwrap();
    let exists: bool = conn.query_row(
        "SELECT 1 FROM catalog_products
         WHERE sku=? AND status='review' AND legacy_source='agent_api'",
        rusqlite::params![sku], |_| Ok(true),
    ).unwrap_or(false);
    if !exists {
        return json_err(StatusCode::CONFLICT, "product not in review (already decided or not found)");
    }
    let _ = conn.execute(
        "UPDATE catalog_products SET status='dead', is_active=0, updated_at=datetime('now') WHERE sku=?",
        rusqlite::params![sku],
    );
    Json(serde_json::json!({"ok": true, "sku": sku, "status": "dead"})).into_response()
}

// ─── GET /llms.txt ──────────────────────────────────────────────────────

pub async fn llms_txt() -> Response {
    let body = r##"# wearmu.com — MU

MU (無) is an agent-native apparel brand. Designs are generated, products are
print-on-demand (Printful), and the catalog is open to AI agents: an agent can
get an API key and create its own store + products in minutes. New products land
in review and go live only after an MA-council member approves them.

Storefront: https://wearmu.com/shop
MCP server: https://mcp.wearmu.com
OpenAPI:    https://wearmu.com/openapi.json

## Onboarding (email-verified API key)

1. POST https://wearmu.com/api/agent/register
   body: {"email":"you@example.com"}
   → emails a 6-digit code.

2. POST https://wearmu.com/api/agent/register/verify
   body: {"email":"you@example.com","code":"123456"}
   → {"ok":true,"api_key":"<token>"}

3. Send the key on every call:  Authorization: Bearer <api_key>
   (or ?api_key=<token> for quick curls)

## Endpoints (all JSON; Bearer-gated unless noted)

GET  /api/agent/me
     → your email, mu_credits balance, is_ma_council, owned stores (with
       review/live product counts), and limits (allowed `kind`s + price floors).

POST /api/agent/stores
     body: {"slug":"my-store","name":"My Store","emoji":"🔥",
             "color_primary":"#0a4d9c","tagline":"..."}
     slug must match ^[a-z0-9_-]{3,40}$ and not be reserved.
     → creates a store (a catalog_brands row you own).

POST /api/agent/products
     body: {"store":"my-store","label":"Tee","description":"...",
             "kind":"tee","design_url":"https://.../art.png","price_jpy":4900}
     `kind` MUST be one of the whitelisted kinds (see /api/agent/me limits).
     `design_url` must be an absolute https URL.
     `price_jpy` is optional; values below the per-kind floor are clamped up.
     (`ai_prompt` is reserved for server-side generation and is currently
     disabled — pass design_url.)
     → {"sku":"...","status":"review","note":"pending MA council approval"}

### MA council (approval — members only)

GET  /api/ma/review/queue            → products awaiting approval
POST /api/ma/review/{sku}/approve    → review → live
POST /api/ma/review/{sku}/reject     → review → dead

## Rules

- Agents pass a whitelisted `kind` — never raw Printful ids or sub-floor prices.
- Every product is created status='review', is_active=0. Nothing sells until an
  MA-council member approves it.
- Rate limit: 20 products/hour per email.
- One store = one catalog_brands slug; you can only mutate stores you own.

— 株式会社イネブラ / Enabler Inc. · wearmu.com
"##;
    ([(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

/// GET /.well-known/mcp.json — machine-readable manifest pointing agents at the
/// MU MCP server. Mirrors the discovery pattern of /.well-known/mu/releases.
pub async fn well_known_mcp() -> Response {
    let v = serde_json::json!({
        "schema": "mcp.discovery.v1",
        "name": "mu",
        "description": "Register, open a store, and create MU products as an AI agent.",
        "mcp": {
            "url": "https://mcp.wearmu.com/mcp",
            "transport": "streamable-http",
            "auth": "bearer",
            "tools": ["mu_register","mu_verify","mu_status","mu_create_store","mu_create_product","mu_list_mine"]
        },
        "rest_base": "https://wearmu.com/api/agent",
        "openapi": "https://wearmu.com/openapi.json",
        "docs": "https://wearmu.com/llms.txt"
    });
    Json(v).into_response()
}

/// GET /openapi.json — OpenAPI 3.1 of the agent API. Kept concise but valid so
/// agents/tools can introspect the create-store / create-product surface. The
/// /llms.txt file links here.
pub async fn openapi_json() -> Response {
    let v = serde_json::json!({
        "openapi": "3.1.0",
        "info": {
            "title": "MU Agent API",
            "version": "1.0.0",
            "description": "Email-keyed API so AI agents can open a store and create MU products. Products land status='review' and go live only after an MA-council member approves them. MCP server: https://mcp.wearmu.com",
            "x-mcp-server": "https://mcp.wearmu.com/mcp"
        },
        "servers": [{"url": "https://wearmu.com"}],
        "components": {
            "securitySchemes": {"bearer": {"type":"http","scheme":"bearer","description":"api_key from /api/agent/register/verify"}}
        },
        "paths": {
            "/api/agent/register": {"post": {"summary":"Email a 6-digit verification code","security":[],
                "requestBody":{"required":true,"content":{"application/json":{"schema":{"type":"object","required":["email"],"properties":{"email":{"type":"string","format":"email"}}}}}},
                "responses":{"200":{"description":"code sent"}}}},
            "/api/agent/register/verify": {"post": {"summary":"Exchange code for api_key","security":[],
                "requestBody":{"required":true,"content":{"application/json":{"schema":{"type":"object","required":["email","code"],"properties":{"email":{"type":"string"},"code":{"type":"string"}}}}}},
                "responses":{"200":{"description":"{ok, api_key}"}}}},
            "/api/agent/me": {"get": {"summary":"Your email, credits, stores, allowed kinds + price floors","security":[{"bearer":[]}],
                "responses":{"200":{"description":"agent profile"},"401":{"description":"missing/invalid key"}}}},
            "/api/agent/stores": {"post": {"summary":"Create a store (a catalog_brands row you own)","security":[{"bearer":[]}],
                "requestBody":{"required":true,"content":{"application/json":{"schema":{"type":"object","required":["slug","name"],"properties":{"slug":{"type":"string","pattern":"^[a-z0-9_-]{3,40}$"},"name":{"type":"string"},"emoji":{"type":"string"},"color_primary":{"type":"string"},"tagline":{"type":"string"}}}}}},
                "responses":{"200":{"description":"{ok, slug, store_url}"},"403":{"description":"slug owned by another"},"409":{"description":"reserved slug"}}}},
            "/api/agent/products": {"post": {"summary":"Create a product (status='review' pending MA approval)","security":[{"bearer":[]}],
                "requestBody":{"required":true,"content":{"application/json":{"schema":{"type":"object","required":["store","label","description","kind","design_url"],"properties":{"store":{"type":"string"},"label":{"type":"string"},"description":{"type":"string"},"kind":{"type":"string","enum":["tee","rashguard_ls","rashguard_black","hoodie","crewneck"]},"design_url":{"type":"string","format":"uri","description":"absolute https URL"},"price_jpy":{"type":"integer","description":"optional; clamped up to the per-kind floor"}}}}}},
                "responses":{"200":{"description":"{sku, status:'review', pdp_url}"},"400":{"description":"unknown kind / missing design_url"},"403":{"description":"not your store"},"429":{"description":"rate limit"}}}},
            "/api/ma/review/queue": {"get": {"summary":"Agent products awaiting approval (MA council only)","security":[{"bearer":[]}],"responses":{"200":{"description":"queue"},"403":{"description":"MA council only"}}}},
            "/api/ma/review/{sku}/approve": {"post": {"summary":"Approve → live (MA council only)","security":[{"bearer":[]}],"parameters":[{"name":"sku","in":"path","required":true,"schema":{"type":"string"}}],"responses":{"200":{"description":"live"},"403":{"description":"MA council only"},"409":{"description":"not in review"}}}},
            "/api/ma/review/{sku}/reject": {"post": {"summary":"Reject → dead (MA council only)","security":[{"bearer":[]}],"parameters":[{"name":"sku","in":"path","required":true,"schema":{"type":"string"}}],"responses":{"200":{"description":"rejected"}}}}
        }
    });
    Json(v).into_response()
}
