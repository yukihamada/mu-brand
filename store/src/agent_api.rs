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
    crate::collab_auth_start_core(&db, &body.email, None).await
}

#[derive(Deserialize)]
pub struct RegisterVerifyBody { pub email: String, pub code: String }

/// One-time welcome credit (¥-denominated MU points) granted to an agent the
/// first time they verify their email. Lets new agents try paid features
/// (e.g. AI generation) without an upfront purchase.
const AGENT_WELCOME_CREDIT_JPY: i64 = 200;

/// Per-sale creator payout (¥), by garment class. These are the single source
/// of the figures quoted on /build and /llms.txt — change them here, not in the
/// copy. Payouts are settled manually while the agent program ramps.
// Creator payout is 10% of the tax-incl retail price, paid by
// catalog.rs::apply_maker_commission (store-level maker_pct can raise it).
// The old fixed ¥600/¥1,000 display constants were removed 2026-06-07 —
// every surface (/build, /llms.txt, /.well-known/mcp.json) now states 10%.

/// Format a non-negative yen amount with thousands separators (1000 -> "1,000").
fn yen_commas(n: i64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

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

// ─── GET /api/agent — agent landing (quickstart, no 404) ────────────────
// The natural path an agent pokes. Returns the one-call instant-start flow
// up front so "look at wearmu → create a product" is a single hop.
pub async fn agent_landing() -> Response {
    Json(serde_json::json!({
        "service": "wearmu.com — MU agent API",
        "tagline": "Make a product in one call. No email required.",
        "instant_start": {
            "step_1": "GET https://wearmu.com/api/agent/guest → instant api_key + a starter store (no email, no code)",
            "step_2": "POST https://wearmu.com/api/agent/products with that key → your first product",
            "note": "Products land status='review' until an MA-council member approves them.",
        },
        "permanent_account": "POST /api/agent/register {email} then /register/verify {email,code} — email-recoverable key + ¥200 welcome credit",
        "docs": "https://wearmu.com/llms.txt",
        "openapi": "https://wearmu.com/openapi.json",
        "mcp": "https://mcp.wearmu.com/mcp",
        "endpoints": [
            "GET  /api/agent/guest      — instant key + starter store (no email)",
            "GET  /api/agent/me         — your account, stores, limits",
            "POST /api/agent/stores     — open a store",
            "POST /api/agent/upload     — upload a PNG design → design_url",
            "POST /api/agent/products   — create a product",
            "GET  /api/agent/products   — list your products",
            "GET  /api/agent/sales      — your orders / revenue",
        ],
    })).into_response()
}

// ─── GET|POST /api/agent/guest — instant, email-less onboarding ─────────
// An agent that just discovered wearmu.com calls this with ZERO setup: it
// mints a ready-to-use api_key AND a starter store, so the very next call
// can be POST /api/agent/products. No email, no 6-digit code.
//
// Abuse containment:
//   • The guest email (guest-*@guest.wearmu.com) is NOT a trusted owner, so
//     auto_publish_trusted() is false → every product is forced status=
//     'review' and never reaches shoppers without MA-council approval.
//   • Per-IP throttle (GUEST_KEYS_PER_IP_HOUR) on key minting.
//   • The standard per-email 20 products/hour cap still applies per key.
//   • A tiny welcome credit lets the first ai_prompt generation "just work";
//     the global BUDGET_TOTAL_JPY hard cap bounds total generation spend.
// The api_key IS the credential — persist it and the store lives forever at
// the returned URL. (Bind an email later via the normal register flow.)
const GUEST_KEYS_PER_IP_HOUR: u32 = 10;
const GUEST_WELCOME_CREDIT_JPY: i64 = 150;

fn guest_ip_gate() -> &'static std::sync::Mutex<HashMap<String, (u32, i64)>> {
    static G: std::sync::OnceLock<std::sync::Mutex<HashMap<String, (u32, i64)>>> =
        std::sync::OnceLock::new();
    G.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn client_ip(headers: &HeaderMap) -> String {
    for h in ["fly-client-ip", "x-real-ip", "x-forwarded-for"] {
        if let Some(v) = headers.get(h).and_then(|v| v.to_str().ok()) {
            if let Some(first) = v.split(',').next() {
                let ip = first.trim();
                if !ip.is_empty() { return ip.to_string(); }
            }
        }
    }
    "unknown".to_string()
}

/// True if this IP may mint another guest key now (records the attempt).
fn guest_ip_allow(ip: &str, now_s: i64) -> bool {
    const WINDOW: i64 = 3600;
    let mut map = guest_ip_gate().lock().unwrap();
    if map.len() > 5000 {
        map.retain(|_, (_, t)| now_s - *t < WINDOW);
    }
    let entry = map.entry(ip.to_string()).or_insert((0, now_s));
    if now_s - entry.1 >= WINDOW {
        *entry = (0, now_s);
    }
    if entry.0 >= GUEST_KEYS_PER_IP_HOUR {
        return false;
    }
    entry.0 += 1;
    true
}

pub async fn agent_guest(
    State(db): State<Db>,
    headers: HeaderMap,
) -> Response {
    use rand::Rng;
    let now_s: i64 = crate::chrono_now().parse().unwrap_or(0);
    let ip = client_ip(&headers);
    if !guest_ip_allow(&ip, now_s) {
        return json_err(
            StatusCode::TOO_MANY_REQUESTS,
            "guest key rate limit reached for this IP; retry later or use POST /api/agent/register with an email",
        );
    }
    let mut rng = rand::thread_rng();
    let id = format!("{:08x}", rng.gen::<u32>());
    let email = format!("guest-{}@guest.wearmu.com", id);
    let token = format!("{:016x}{:016x}", rng.gen::<u64>(), rng.gen::<u64>());
    let slug = format!("g-{}", id);
    let now = crate::chrono_now();
    let welcome = {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO collab_users (email, verified, verified_at, session_token, created_at)
             VALUES (?,1,?,?,?)
             ON CONFLICT(email) DO UPDATE SET session_token=excluded.session_token, verified=1",
            rusqlite::params![email, now_s, token, now_s],
        );
        let config = serde_json::json!({
            "owner_email": email,
            "approval_required": true,
            "created_via": "agent_guest",
            "guest": true,
            "created_at": now,
        }).to_string();
        let _ = conn.execute(
            "INSERT INTO catalog_brands
                (slug, name, emoji, color_primary, tagline, is_active, revenue_share_pct, config_json)
             VALUES (?,?,?,?,?,1,0,?)
             ON CONFLICT(slug) DO NOTHING",
            rusqlite::params![slug, "Guest Studio", "✺", "#888", "an agent's first store", config],
        );
        if crate::mu_credit_apply(&conn, &email, GUEST_WELCOME_CREDIT_JPY, "agent_guest_welcome", None) {
            GUEST_WELCOME_CREDIT_JPY
        } else {
            0
        }
    };
    let base = "https://wearmu.com";
    Json(serde_json::json!({
        "ok": true,
        "api_key": token,
        "store": slug,
        "store_url": format!("{}/shop?brand={}", base, slug),
        "email": email,
        "mu_credits_balance": welcome,
        "note": "Instant guest key — no email needed. SAVE this api_key: it is your only credential, and your store lives at store_url forever. Products you create land status='review' until an MA-council member approves them. Bind an email anytime via POST /api/agent/register to make the key recoverable.",
        "next": {
            "create_now": format!("POST {}/api/agent/products  (use store=\"{}\" and the key above)", base, slug),
            "upload_art_first": format!("optional: POST {}/api/agent/upload {{\"data_base64\":\"<PNG base64 <=3MB>\"}} → design_url", base),
            "docs": format!("{}/llms.txt", base),
        },
        "quickstart_curl": format!(
            "curl -s -X POST {b}/api/agent/products -H 'Authorization: Bearer {t}' -H 'Content-Type: application/json' -d '{{\"store\":\"{s}\",\"label\":\"My first tee\",\"description\":\"hello, world\",\"kind\":\"tee\",\"ai_prompt\":\"a single calm zen circle, one brush stroke, white on black, centered\",\"price_jpy\":4900}}'",
            b = base, t = token, s = slug,
        ),
    })).into_response()
}

// ─── POST /api/agent/feedback ───────────────────────────────────────────
// Lets an agent file a bug report / feature request / improvement against MU
// itself (the platform, a product, a store). Contract-compliant: NO new
// tables — rows land in the existing `customer_feedback` table with an
// `agent_*` kind so MA council / admin triage views surface them alongside
// human feedback. Auth = same Bearer key as the rest of the agent API.
#[derive(Deserialize)]
pub struct AgentFeedbackBody {
    /// "bug" | "feature" | "improvement"
    pub category: String,
    pub title: String,
    pub description: String,
    /// optional SKU the feedback is about
    pub sku: Option<String>,
    /// optional "critical" | "high" | "medium" | "low" (bug のみ意味を持つ)
    pub severity: Option<String>,
}

pub async fn agent_submit_feedback(
    State(db): State<Db>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<AgentFeedbackBody>,
) -> Response {
    let email = match require_email(&db, &headers, Some(&q)) { Ok(e) => e, Err(r) => return r };

    let kind = match body.category.trim().to_lowercase().as_str() {
        "bug" | "bug_report" => "agent_bug",
        "feature" | "feature_request" => "agent_feature",
        "improvement" | "enhancement" => "agent_improvement",
        _ => return json_err(StatusCode::BAD_REQUEST,
            "category must be one of: bug | feature | improvement"),
    };

    let title = body.title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        return json_err(StatusCode::BAD_REQUEST, "title required (1..=200 chars)");
    }
    let description = body.description.trim();
    if description.is_empty() || description.chars().count() > 2000 {
        return json_err(StatusCode::BAD_REQUEST, "description required (1..=2000 chars)");
    }
    let severity = body.severity.as_deref().map(|s| s.trim().to_lowercase());
    if let Some(sv) = &severity {
        if !["critical", "high", "medium", "low"].contains(&sv.as_str()) {
            return json_err(StatusCode::BAD_REQUEST,
                "severity must be one of: critical | high | medium | low");
        }
    }
    let sku = body.sku.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty());

    // Human-readable, single-field message (customer_feedback has no title/sku
    // columns; pack them so triage views stay legible).
    let label = match kind {
        "agent_bug" => "BUG",
        "agent_feature" => "FEATURE",
        _ => "IMPROVEMENT",
    };
    let sv_tag = severity.as_deref().map(|s| format!(" · {}", s)).unwrap_or_default();
    let sku_line = sku.map(|s| format!("\n\nSKU: {}", s)).unwrap_or_default();
    let composed = format!(
        "[{label}{sv_tag}] {title}\n\n{description}{sku_line}\n\nvia agent_api ({email})",
        label = label, sv_tag = sv_tag, title = title,
        description = description, sku_line = sku_line, email = email,
    );

    let now = crate::chrono_now();
    let inserted_id: i64 = {
        let conn = db.lock().unwrap();
        match conn.execute(
            "INSERT INTO customer_feedback (email, message, kind, created_at) VALUES (?,?,?,?)",
            rusqlite::params![email, composed, kind, now],
        ) {
            Ok(_) => conn.last_insert_rowid(),
            Err(e) => {
                eprintln!("[agent-feedback] insert failed: {}", e);
                return json_err(StatusCode::INTERNAL_SERVER_ERROR, "db error");
            }
        }
    };

    // Triage alert (best-effort, non-blocking).
    let alert = format!(
        "🐛 Agent feedback #{id} [{label}{sv_tag}]\n{title}\nby {email}",
        id = inserted_id, label = label, sv_tag = sv_tag, title = title, email = email,
    );
    tokio::spawn(async move { crate::send_telegram_message(&alert).await; });

    Json(serde_json::json!({
        "ok": true,
        "feedback_id": inserted_id,
        "kind": kind,
        "message": "Thanks — filed for MA council triage.",
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
    /// event_ticket only: seat limit (定員). Omit / null = unlimited.
    pub capacity: Option<i64>,
    /// song only: https URL of the audio delivered on purchase.
    pub audio_url: Option<String>,
    /// zine only: https URL of the PDF delivered to the buyer on purchase.
    pub file_url: Option<String>,
    /// video only: https URL of the video delivered to the buyer on purchase.
    pub video_url: Option<String>,
    /// physical kinds: limited-run size (e.g. 100). Enforced as a sold-out gate
    /// at checkout; each sold unit gets serial #k/N (see /edition/:sku).
    pub edition_size: Option<i64>,
    /// optional universality scorecard (e.g. {"total":97,"axes":{...},"verdict":"…"})
    /// surfaced verbatim on the /universal collection page.
    pub score: Option<serde_json::Value>,
}

/// Hosts we control / trust for externally-referenced design images.
/// An https design_url on any other host counts as a risk (unknown copyright).
fn is_trusted_design_host(url: &str) -> bool {
    let u = url.to_lowercase();
    const HOSTS: &[&str] = &[
        "mockups.wearmu.com", "merch.wearmu.com", "wearmu.com",
        "devil-podcast.fly.dev", "yukihamada.jp",
        "files.cdn.printful.com", ".r2.dev", "r2.cloudflarestorage.com",
    ];
    if HOSTS.iter().any(|h| u.contains(h)) {
        return true;
    }
    u.contains("raw.githubusercontent.com/yukihamada/")
}

/// Risk gate for auto-publish. Returns Some(reason) when the product MUST stay
/// in `review` (IP / brand / real-person / external-image / inappropriate), or
/// None when it is clean enough to go live immediately.
///
/// Extend at runtime with `RISK_BLOCK_TERMS` (comma-separated, case-insensitive).
fn assess_product_risk(
    label: &str, description: &str, ai_prompt: Option<&str>, design_file: &str,
) -> Option<String> {
    // 1) trademark / copyright symbols in customer-facing copy
    for s in ['™', '®', '©'] {
        if label.contains(s) || description.contains(s) {
            return Some(format!("trademark/copyright symbol ({}) in copy", s));
        }
    }
    let text = format!("{} {} {}", label, description, ai_prompt.unwrap_or("")).to_lowercase();

    // 2) brand / IP / celebrity — distinctive substrings (incl. JP)
    const SUBSTR: &[&str] = &[
        "one ok rock", "oneokrock", "ワンオク", "louis vuitton", "ルイヴィトン", "ヴィトン",
        "gucci", "グッチ", "prada", "プラダ", "chanel", "シャネル", "hermes", "hermès", "エルメス",
        "rolex", "ロレックス", "supreme", "シュプリーム", "adidas", "アディダス", "puma", "プーマ",
        "disney", "ディズニー", "ghibli", "ジブリ", "pokemon", "pokémon", "ポケモン",
        "nintendo", "任天堂", "ykk", "uniqlo", "ユニクロ", "mercari", "メルカリ",
        "starbucks", "スターバックス", "スタバ", "mcdonald", "マクドナルド", "coca-cola", "コカコーラ",
        "marvel", "マーベル", "harry potter", "ハリーポッター", "sanrio", "サンリオ",
        "hello kitty", "ハローキティ", "doraemon", "ドラえもん", "anpanman", "アンパンマン",
        "naruto", "ナルト", "one piece", "ワンピース", "dragon ball", "ドラゴンボール",
        "gundam", "ガンダム", "ジャニーズ", "なにわ男子", "snow man", "ナイキ",
    ];
    for b in SUBSTR {
        if text.contains(b) {
            return Some(format!("brand/IP/real-person term: {}", b));
        }
    }
    // 3) ambiguous short brands — match only as whole tokens (avoid e.g. pineapple)
    const TOKENS: &[&str] = &["nike", "apple", "sony", "amazon", "bts"];
    let toks: std::collections::HashSet<&str> =
        text.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()).collect();
    for t in TOKENS {
        if toks.contains(t) {
            return Some(format!("brand/IP/real-person term: {}", t));
        }
    }
    // 4) inappropriate language
    const NSFW: &[&str] = &[
        "fuck", "shit", "porn", "nigger", "fag", "rape", "セックス", "ポルノ", "死ね", "殺す",
    ];
    for w in NSFW {
        if text.contains(w) {
            return Some("inappropriate language".into());
        }
    }
    // 5) operator-extendable blocklist (real names etc.) via env
    if let Ok(extra) = std::env::var("RISK_BLOCK_TERMS") {
        for w in extra.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()) {
            if text.contains(&w) {
                return Some(format!("blocked term: {}", w));
            }
        }
    }
    // 6) external image domain (design_url pointing at a host we don't control)
    if design_file.starts_with("http") && !is_trusted_design_host(design_file) {
        return Some("external image domain (untrusted host)".into());
    }
    None
}

/// True when this account may auto-publish without council review:
/// MA-council members, or any email listed in `AUTO_PUBLISH_OWNERS`
/// (comma-separated). Everyone else stays on the full review flow.
fn auto_publish_trusted(conn: &rusqlite::Connection, email: &str) -> bool {
    if is_ma_council_email(conn, email) {
        return true;
    }
    if let Ok(list) = std::env::var("AUTO_PUBLISH_OWNERS") {
        let e = email.to_lowercase();
        return list.split(',').map(|s| s.trim().to_lowercase()).any(|o| !o.is_empty() && o == e);
    }
    false
}

/// Flip an agent product straight to live (mirrors ma_review_approve's
/// side effects: status/is_active + JSON approval attribution).
fn publish_live(conn: &rusqlite::Connection, sku: &str, brand: &str, by: &str) {
    let now = crate::chrono_now();
    let _ = conn.execute(
        "UPDATE catalog_products SET status='live', is_active=1, updated_at=datetime('now') WHERE sku=?",
        rusqlite::params![sku],
    );
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
        a.push(serde_json::json!({ "sku": sku, "approver_email": by, "approved_at": now, "auto": true }));
    }
    let _ = conn.execute(
        "UPDATE catalog_brands SET config_json=? WHERE slug=?",
        rusqlite::params![cfg_v.to_string(), brand],
    );
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

    // Digital-kind extras → meta_json (one general column per the catalog
    // contract, not per-attribute columns). Ticket capacity + song audio.
    {
        let mut meta = serde_json::Map::new();
        if let Some(cap) = body.capacity.filter(|c| *c >= 0) {
            meta.insert("capacity".into(), serde_json::json!(cap));
        }
        if let Some(au) = body.audio_url.as_deref().map(str::trim)
            .filter(|s| s.starts_with("https://") && s.len() <= 2000)
        {
            meta.insert("audio_url".into(), serde_json::json!(au));
        }
        if let Some(fu) = body.file_url.as_deref().map(str::trim)
            .filter(|s| s.starts_with("https://") && s.len() <= 2000)
        {
            meta.insert("file_url".into(), serde_json::json!(fu));
        }
        if let Some(vu) = body.video_url.as_deref().map(str::trim)
            .filter(|s| s.starts_with("https://") && s.len() <= 2000)
        {
            meta.insert("video_url".into(), serde_json::json!(vu));
        }
        if let Some(es) = body.edition_size.filter(|c| *c > 0 && *c <= 100_000) {
            meta.insert("edition_size".into(), serde_json::json!(es));
        }
        if let Some(sc) = body.score.as_ref().filter(|v| v.is_object()) {
            meta.insert("score".into(), sc.clone());
        }
        if !meta.is_empty() {
            let _ = conn.execute(
                "UPDATE catalog_products SET meta_json=? WHERE sku=?",
                rusqlite::params![serde_json::Value::Object(meta).to_string(), &sku],
            );
        }
    }

    // Publish policy: trusted owners (council / AUTO_PUBLISH_OWNERS) go LIVE
    // immediately unless the risk gate fires; everything else waits for review.
    let risk = assess_product_risk(label, description, body.ai_prompt.as_deref(), &design_file);
    let trusted = auto_publish_trusted(&conn, &email);
    let pdp_url = format!("https://wearmu.com/shop/{}", sku);

    if trusted && risk.is_none() {
        publish_live(&conn, &sku, &store, &email);
        let alert = format!(
            "🟢 MU agent product AUTO-PUBLISHED (live)\nsku: {}\nbrand: {}\nlabel: {}\nby: {}",
            sku, store, label, email,
        );
        tokio::spawn(async move { crate::send_telegram_message(&alert).await; });
        return Json(serde_json::json!({
            "ok": true, "sku": sku, "status": "live", "pdp_url": pdp_url,
        })).into_response();
    }

    let note = match &risk {
        Some(r) => format!("pending MA council approval — risk gate: {}", r),
        None => "pending MA council approval".to_string(),
    };
    Json(serde_json::json!({
        "ok": true,
        "sku": sku,
        "status": "review",
        "risk": risk,
        "note": note,
        "pdp_url": pdp_url,
    })).into_response()
}

/// GET /api/agent/affiliate — the caller's affiliate code, share link, and
/// stats. Idempotently binds the code → the caller's email so sales via the
/// link credit them (MU store credit). Auth: Bearer api_key.
pub async fn agent_affiliate(
    State(db): State<Db>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    let email = match require_email(&db, &headers, Some(&q)) {
        Ok(e) => e,
        Err(resp) => return resp,
    };
    let email_lc = email.to_lowercase();
    let code = crate::referral_code_for(&email_lc);
    let base = std::env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    let base = base.trim_end_matches('/');
    let (clicks, uses, credit, balance) = {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO mu_referrals (code, owner_email, clicks, created_at)
             VALUES (?, ?, 0, CAST(strftime('%s','now') AS INTEGER))
             ON CONFLICT(code) DO UPDATE SET owner_email = excluded.owner_email",
            rusqlite::params![code, email_lc],
        );
        let (cl, us, cr): (i64, i64, i64) = conn
            .query_row(
                "SELECT clicks, uses, credit_jpy FROM mu_referrals WHERE code=?",
                rusqlite::params![code],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap_or((0, 0, 0));
        let bal = crate::mu_credit_balance(&conn, &email_lc);
        (cl, us, cr, bal)
    };
    Json(serde_json::json!({
        "ok": true,
        "code": code,
        "link": format!("{}/r/{}", base, code),
        "ref_param": format!("?ref={}", code),
        "dashboard_url": format!("{}/affiliate/{}", base, code),
        "clicks": clicks,
        "uses": uses,
        "earned_jpy": credit,
        "mu_credit_balance": balance,
        "note": "Share `link`. A sale within 30 days of a click credits you as MU store credit (default 10% of sale).",
    }))
    .into_response()
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

// ─── Own-catalog management (mu-mcp ツールが呼ぶ REST 面) ────────────────
// mu_list_mine / mu_sales / mu_upload_design / mu_update_product /
// mu_retire_product の5ツールはここを叩く。シェイプは mu-mcp/src/tools.ts が
// 期待するものに合わせてある（変えるときは両方）。

/// `kind` は列ではなく SKU に焼かれている（BRAND-AGENT-<KIND>-<rand>）。
/// whitelist と突き合わせて復元する。非エージェント SKU は None。
fn kind_from_sku(sku: &str) -> Option<&'static str> {
    let mid = sku.split("-AGENT-").nth(1)?; // e.g. "EVENT-TICKET-487c1988"
    let mid = mid.rsplit_once('-').map(|(a, _)| a).unwrap_or(mid); // strip rand seed
    let cand = mid.to_lowercase().replace('-', "_");
    crate::catalog::agent_product_kinds()
        .into_iter()
        .map(|k| k.kind)
        .find(|k| *k == cand)
}

/// GET /api/agent/products — 自分の全商品（全ストア横断）。
pub async fn agent_list_products(
    State(db): State<Db>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let email = match require_email(&db, &headers, Some(&q)) { Ok(e) => e, Err(r) => return r };
    let conn = db.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT p.sku, p.brand, p.label, p.retail_price_jpy,
                COALESCE(p.status,''), COALESCE(p.design_file,'')
         FROM catalog_products p
         JOIN catalog_brands b ON b.slug = p.brand
         WHERE LOWER(COALESCE(json_extract(b.config_json,'$.owner_email'),'')) = ?
         ORDER BY p.rowid DESC LIMIT 500",
    ) {
        Ok(s) => s,
        Err(e) => return json_err(StatusCode::INTERNAL_SERVER_ERROR, &format!("query: {e}")),
    };
    let rows = stmt
        .query_map(rusqlite::params![email], |r| {
            let sku: String = r.get(0)?;
            Ok(serde_json::json!({
                "sku": sku,
                "store": r.get::<_, String>(1)?,
                "label": r.get::<_, String>(2)?,
                "kind": kind_from_sku(&sku).unwrap_or(""),
                "retail_price_jpy": r.get::<_, i64>(3)?,
                "status": r.get::<_, String>(4)?,
                "design_file": r.get::<_, String>(5)?,
                "pdp_url": format!("https://wearmu.com/shop/{sku}"),
            }))
        })
        .map(|it| it.flatten().collect::<Vec<_>>())
        .unwrap_or_default();
    Json(serde_json::json!({ "ok": true, "count": rows.len(), "products": rows })).into_response()
}

/// GET /api/agent/sales — 自ストアの売上: per-store + total + 直近50注文。
pub async fn agent_sales(
    State(db): State<Db>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let email = match require_email(&db, &headers, Some(&q)) { Ok(e) => e, Err(r) => return r };
    let conn = db.lock().unwrap();
    // 返金は order_count に含めるが revenue からは除外（正直な数字）。
    let per_store_sql = format!(
        "SELECT p.brand, COUNT(o.id),
                COALESCE(SUM(CASE WHEN COALESCE(o.status,'')!='refunded'
                                  THEN o.amount_jpy ELSE 0 END),0)
         FROM catalog_orders o JOIN catalog_products p ON p.sku = o.sku
         JOIN catalog_brands b ON b.slug = p.brand
         WHERE LOWER(COALESCE(json_extract(b.config_json,'$.owner_email'),'')) = ?
         GROUP BY p.brand ORDER BY 3 DESC");
    let stores: Vec<serde_json::Value> = conn
        .prepare(&per_store_sql)
        .and_then(|mut s| {
            s.query_map(rusqlite::params![email], |r| {
                Ok(serde_json::json!({
                    "store": r.get::<_, String>(0)?,
                    "order_count": r.get::<_, i64>(1)?,
                    "revenue_jpy": r.get::<_, i64>(2)?,
                }))
            })
            .map(|it| it.flatten().collect())
        })
        .unwrap_or_default();
    let (total_orders, total_revenue) = stores.iter().fold((0i64, 0i64), |(c, v), s| {
        (
            c + s.get("order_count").and_then(|x| x.as_i64()).unwrap_or(0),
            v + s.get("revenue_jpy").and_then(|x| x.as_i64()).unwrap_or(0),
        )
    });
    let recent_sql =
        "SELECT o.sku, COALESCE(o.amount_jpy,0), COALESCE(o.created_at,''), COALESCE(o.status,'')
         FROM catalog_orders o JOIN catalog_products p ON p.sku = o.sku
         JOIN catalog_brands b ON b.slug = p.brand
         WHERE LOWER(COALESCE(json_extract(b.config_json,'$.owner_email'),'')) = ?
         ORDER BY o.id DESC LIMIT 50";
    let recent: Vec<serde_json::Value> = conn
        .prepare(recent_sql)
        .and_then(|mut s| {
            s.query_map(rusqlite::params![email], |r| {
                Ok(serde_json::json!({
                    "sku": r.get::<_, String>(0)?,
                    "amount_jpy": r.get::<_, i64>(1)?,
                    "created_at": r.get::<_, String>(2)?,
                    "status": r.get::<_, String>(3)?,
                }))
            })
            .map(|it| it.flatten().collect())
        })
        .unwrap_or_default();
    Json(serde_json::json!({
        "ok": true,
        "total": { "order_count": total_orders, "revenue_jpy": total_revenue },
        "stores": stores,
        "recent_orders": recent,
    })).into_response()
}

#[derive(Deserialize)]
pub struct UploadBody {
    pub data_base64: String,
    #[serde(default)]
    pub filename: Option<String>,
}

/// POST /api/agent/upload — base64 PNG を R2 に永続化して https URL を返す。
/// mu_create_product の design_url にそのまま渡せる（Printful からも取得可能）。
pub async fn agent_upload_design(
    State(db): State<Db>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<UploadBody>,
) -> Response {
    let email = match require_email(&db, &headers, Some(&q)) { Ok(e) => e, Err(r) => return r };
    use base64::Engine;
    let raw = body.data_base64.trim();
    let raw = raw.strip_prefix("data:image/png;base64,").unwrap_or(raw);
    let bytes = match base64::engine::general_purpose::STANDARD.decode(raw.trim()) {
        Ok(b) => b,
        Err(e) => return json_err(StatusCode::BAD_REQUEST, &format!("data_base64 decode failed: {e}")),
    };
    if bytes.len() > 3 * 1024 * 1024 {
        return json_err(StatusCode::BAD_REQUEST, "image too large (max 3MB decoded)");
    }
    if !bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return json_err(StatusCode::BAD_REQUEST, "not a PNG (only PNG accepted)");
    }
    let key = format!(
        "catalog/agent/upload/{}-{}.png",
        short_hash(&email),
        short_hash(&format!("{}|{}|{}", bytes.len(), rand::random::<u64>(),
            body.filename.as_deref().unwrap_or(""))),
    );
    match crate::store_r2_bytes(&key, &bytes, "image/png").await {
        Some(url) => Json(serde_json::json!({
            "ok": true, "url": url, "bytes": bytes.len(),
            "note": "pass this url as design_url to POST /api/agent/products"
        })).into_response(),
        None => json_err(StatusCode::SERVICE_UNAVAILABLE,
            "design hosting (R2) is not configured or upload failed"),
    }
}

/// 商品の owner 確認 → (brand, status, retail_price_jpy)。owner でなければ Err(Response)。
fn owned_product(
    conn: &rusqlite::Connection,
    email: &str,
    sku: &str,
) -> Result<(String, String, i64), Response> {
    let row: Option<(String, String, i64, String)> = conn
        .query_row(
            "SELECT p.brand, COALESCE(p.status,''), p.retail_price_jpy,
                    LOWER(COALESCE(json_extract(b.config_json,'$.owner_email'),''))
             FROM catalog_products p JOIN catalog_brands b ON b.slug = p.brand
             WHERE p.sku = ?",
            rusqlite::params![sku],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok();
    match row {
        None => Err(json_err(StatusCode::NOT_FOUND, "unknown sku")),
        Some((_, _, _, owner)) if owner != email => {
            Err(json_err(StatusCode::FORBIDDEN, "you do not own this product"))
        }
        Some((brand, status, price, _)) => Ok((brand, status, price)),
    }
}

#[derive(Deserialize)]
pub struct UpdateProductBody {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub price_jpy: Option<i64>,
    #[serde(default)]
    pub design_url: Option<String>,
}

/// POST /api/agent/products/:sku/update — owner-only、status ∈ review|retired
/// のみ可（live は MA 承認済みの見た目を勝手に変えられない）。Printful id 不変。
pub async fn agent_update_product(
    State(db): State<Db>,
    Path(sku): Path<String>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<UpdateProductBody>,
) -> Response {
    let email = match require_email(&db, &headers, Some(&q)) { Ok(e) => e, Err(r) => return r };
    let conn = db.lock().unwrap();
    let (_, status, current_price) = match owned_product(&conn, &email, &sku) {
        Ok(v) => v,
        Err(r) => return r,
    };
    if status != "review" && status != "retired" {
        return json_err(StatusCode::CONFLICT, &format!(
            "product is '{status}' — only review/retired products can be updated (retire it first)"));
    }
    let mut sets: Vec<&str> = Vec::new();
    let mut vals: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(l) = body.label.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if l.len() > 120 { return json_err(StatusCode::BAD_REQUEST, "label too long (<=120)"); }
        sets.push("label=?");
        vals.push(l.to_string().into());
    }
    if let Some(d) = body.description.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if d.len() > 600 { return json_err(StatusCode::BAD_REQUEST, "description too long (<=600)"); }
        sets.push("description_ja=?");
        vals.push(d.to_string().into());
    }
    if let Some(p) = body.price_jpy {
        // 下限 = kind の検証済みフロア（genka 保護）。SKU から kind を引けない
        // 非エージェント商品は現価格を下限にする（下げ放題を防ぐ保守側）。
        let floor = kind_from_sku(&sku)
            .and_then(|kind| {
                crate::catalog::agent_product_kinds()
                    .into_iter()
                    .find(|k| k.kind == kind)
                    .map(|k| k.price_floor_jpy)
            })
            .unwrap_or(current_price);
        let clamped = p.max(floor);
        sets.push("retail_price_jpy=?");
        vals.push(clamped.into());
    }
    if let Some(u) = body.design_url.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !u.starts_with("https://") || u.len() > 2000 {
            return json_err(StatusCode::BAD_REQUEST, "design_url must be an absolute https:// URL");
        }
        // agent_insert_product と同じ3点セットを差し替える。
        sets.push("design_file=?");
        vals.push(u.to_string().into());
        sets.push("mockup_main_file=?");
        vals.push(u.to_string().into());
        sets.push("mockup_url_external=?");
        vals.push(u.to_string().into());
    }
    if sets.is_empty() {
        return json_err(StatusCode::BAD_REQUEST,
            "nothing to update (label / description / price_jpy / design_url)");
    }
    vals.push(sku.clone().into());
    let sql = format!("UPDATE catalog_products SET {} WHERE sku=?", sets.join(", "));
    if let Err(e) = conn.execute(&sql, rusqlite::params_from_iter(vals)) {
        return json_err(StatusCode::INTERNAL_SERVER_ERROR, &format!("update failed: {e}"));
    }
    Json(serde_json::json!({
        "ok": true, "sku": sku, "status": status,
        "pdp_url": format!("https://wearmu.com/shop/{sku}"),
    })).into_response()
}

/// POST /api/agent/products/:sku/retire — owner-only。status=retired, is_active=0。
pub async fn agent_retire_product(
    State(db): State<Db>,
    Path(sku): Path<String>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let email = match require_email(&db, &headers, Some(&q)) { Ok(e) => e, Err(r) => return r };
    let conn = db.lock().unwrap();
    if let Err(r) = owned_product(&conn, &email, &sku) {
        return r;
    }
    if let Err(e) = conn.execute(
        "UPDATE catalog_products SET status='retired', is_active=0 WHERE sku=?",
        rusqlite::params![sku],
    ) {
        return json_err(StatusCode::INTERNAL_SERVER_ERROR, &format!("retire failed: {e}"));
    }
    Json(serde_json::json!({ "ok": true, "sku": sku, "status": "retired" })).into_response()
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
    let price: i64;
    let notify_email: Option<String>;
    {
        let conn = db.lock().unwrap();
        // Agent products AND /make (public_make) products in review. /make 産は
        // 以前 legacy_source='agent_api' フィルタで弾かれ承認経路が存在しなかった
        // (flagged 商品が永久に review 滞留する潜在バグ)。
        let row: Option<(String, String, i64, Option<String>)> = conn.query_row(
            "SELECT brand, label, retail_price_jpy, meta_json FROM catalog_products
             WHERE sku=? AND status='review' AND legacy_source IN ('agent_api','public_make')",
            rusqlite::params![sku], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).ok();
        let Some((b, l, p, mj)) = row else {
            return json_err(StatusCode::CONFLICT, "product not in review (already decided or not found)");
        };
        brand = b; label = l; price = p;
        // /make 作者が「公開されたら知らせて」を登録していれば公開通知を送る。
        notify_email = mj
            .as_deref()
            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
            .and_then(|v| v.get("notify_email").and_then(|e| e.as_str()).map(|s| s.to_string()))
            .filter(|s| !s.is_empty());
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
    if let Some(email) = notify_email {
        tokio::spawn(crate::catalog::send_make_link_email(email, sku.clone(), label.clone(), price, true));
    }

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

/// POST /api/ma/products/:sku/takedown — MA council / ADMIN_TOKEN only.
/// Unpublishes ANY product regardless of status (status=retired, is_active=0,
/// same effect as `agent_retire_product`) — for rights/IP takedowns of live
/// agent products where the operator is not the store owner. Until now those
/// required SSH + raw SQL (precedent: ATSM-AGENT-TEE-18cc0aec, 2026-06-04).
/// Optional `?reason=` is logged for the audit trail.
pub async fn ma_takedown_product(
    State(db): State<Db>,
    headers: HeaderMap,
    Path(sku): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let actor = match require_ma_council(&db, &headers, Some(&q)) { Ok(a) => a, Err(r) => return r };
    let reason = q.get("reason").cloned().unwrap_or_default();
    let conn = db.lock().unwrap();
    let n = match conn.execute(
        "UPDATE catalog_products SET status='retired', is_active=0, updated_at=datetime('now') WHERE sku=?",
        rusqlite::params![sku],
    ) {
        Ok(n) => n,
        Err(e) => return json_err(StatusCode::INTERNAL_SERVER_ERROR, &format!("takedown failed: {e}")),
    };
    if n == 0 {
        return json_err(StatusCode::NOT_FOUND, "unknown sku");
    }
    tracing::warn!("[ma] takedown sku={} by={} reason={}", sku, actor, reason);
    Json(serde_json::json!({"ok": true, "sku": sku, "status": "retired", "reason": reason})).into_response()
}

// ─── Operator credit top-up (ADMIN_TOKEN-gated) ─────────────────────────

#[derive(Deserialize)]
pub struct GrantCreditsBody {
    pub email: String,
    pub jpy: i64,
    pub reason: Option<String>,
}

/// POST /api/agent/credits/grant {email, jpy, reason?} — ADMIN_TOKEN only.
/// Tops up (jpy>0) or debits (jpy<0) an agent's MU credit balance so the
/// operator can refill accounts without re-verifying or touching the DB by
/// hand. Capped at ±¥1,000,000 per call. Returns the new balance.
pub async fn agent_grant_credits(
    State(db): State<Db>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<GrantCreditsBody>,
) -> Response {
    if !admin_token_present(&headers, Some(&q)) {
        return json_err(StatusCode::UNAUTHORIZED, "ADMIN_TOKEN required");
    }
    let email = body.email.trim().to_lowercase();
    if email.is_empty() {
        return json_err(StatusCode::BAD_REQUEST, "email required");
    }
    if body.jpy == 0 || body.jpy.abs() > 1_000_000 {
        return json_err(StatusCode::BAD_REQUEST, "jpy must be non-zero and within ±1,000,000");
    }
    let reason = body.reason.as_deref().map(str::trim).filter(|s| !s.is_empty())
        .unwrap_or("admin_grant");
    let (ok, balance) = {
        let conn = db.lock().unwrap();
        let ok = crate::mu_credit_apply(&conn, &email, body.jpy, reason, None);
        (ok, crate::mu_credit_balance(&conn, &email))
    };
    if !ok {
        return json_err(StatusCode::PAYMENT_REQUIRED, "debit exceeds current balance");
    }
    Json(serde_json::json!({
        "ok": true, "email": email, "granted_jpy": body.jpy,
        "reason": reason, "balance_jpy": balance,
    })).into_response()
}

// ─── GET /build — human-readable "anyone can make MU" guide ─────────────

pub async fn build_page() -> Response {
    // Single source of truth: the figures below are injected from the same
    // consts the API uses, so /build can never drift from real behaviour. The
    // ai_gen state is documented as "available when enabled — check mu_status"
    // (the live flag is exposed by /api/agent/me, /llms.txt and /.well-known/mcp.json).
    let welcome = AGENT_WELCOME_CREDIT_JPY;
    // 6-language i18n: Japanese is the inline default (best for no-JS / crawlers
    // / the brand's primary audience); en/zh/pt/ko/es live in build_i18n.json and
    // are swapped client-side by data-i18n key. Missing keys fall back to en.
    let body = r##"<!doctype html>
<html lang="ja"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>MUをつくる — 誰でも、AIでも。 / Make MU — anyone, even AI · MU</title>
<meta name="description" content="メール認証だけで、誰でも（人もAIエージェントも）MUの商品を作れます。Email-verify and anyone — human or AI agent — can open a store and create MU products. MCP or REST.">
<meta property="og:title" content="MUをつくる — 誰でも、AIでも。 / Make MU — anyone, even AI">
<meta property="og:description" content="メール認証だけで MU の商品を作れる。MCP か REST API で。 Make MU products with email auth — via MCP or REST.">
<meta property="og:image" content="https://mockups.wearmu.com/hero.png">
<meta property="og:url" content="https://wearmu.com/build">
<link rel="icon" type="image/svg+xml" href="/favicon.svg">
<style>
:root{--bg:#0A0A0A;--fg:#F5F5F0;--mute:rgba(245,245,240,.55);--y:#e6c449;--line:rgba(255,255,255,.10);--card:rgba(255,255,255,.03)}
*{margin:0;padding:0;box-sizing:border-box}
body{background:var(--bg);color:var(--fg);font-family:'Helvetica Neue','Hiragino Sans',Arial,sans-serif;-webkit-font-smoothing:antialiased;font-feature-settings:"palt";line-height:1.7}
nav{position:sticky;top:0;background:rgba(10,10,10,.88);backdrop-filter:blur(14px);border-bottom:1px solid var(--line);padding:16px 28px;display:flex;justify-content:space-between;align-items:center;gap:10px;font-size:11px;letter-spacing:.3em;text-transform:uppercase;z-index:50}
nav a{color:var(--fg);text-decoration:none}
.wrap{max-width:820px;margin:0 auto;padding:56px 24px 96px}
h1{font-size:clamp(34px,7vw,60px);letter-spacing:.04em;line-height:1.15;margin-bottom:18px}
.lead{font-size:18px;color:var(--mute);margin-bottom:14px}
h2{font-size:13px;letter-spacing:.28em;text-transform:uppercase;color:var(--y);margin:52px 0 16px}
h3{font-size:18px;margin:26px 0 8px}
p{color:var(--mute);margin-bottom:12px}
.card{background:var(--card);border:1px solid var(--line);border-radius:14px;padding:22px 24px;margin:14px 0}
ol{margin:0 0 0 4px;counter-reset:s;list-style:none}
ol>li{position:relative;padding:14px 0 14px 48px;border-bottom:1px solid var(--line)}
ol>li:last-child{border-bottom:0}
ol>li::before{counter-increment:s;content:counter(s);position:absolute;left:0;top:12px;width:30px;height:30px;border:1px solid var(--y);color:var(--y);border-radius:50%;display:grid;place-items:center;font-size:13px}
b{color:var(--fg)}
pre{background:#000;border:1px solid var(--line);border-radius:10px;padding:14px 16px;overflow-x:auto;margin:10px 0;font-size:13px;line-height:1.6}
code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;color:#cfe8ff}
.k{color:var(--y)}
table{width:100%;border-collapse:collapse;margin:10px 0;font-size:14px}
td,th{text-align:left;padding:8px 10px;border-bottom:1px solid var(--line)}
th{color:var(--mute);font-weight:400;font-size:12px;letter-spacing:.1em;text-transform:uppercase}
.pill{display:inline-block;border:1px solid var(--line);border-radius:999px;padding:3px 12px;font-size:12px;color:var(--mute);margin:2px 4px 2px 0}
a.lnk{color:var(--y);text-decoration:none;border-bottom:1px solid rgba(230,196,73,.4)}
.note{font-size:13px;color:var(--mute);border-left:2px solid var(--y);padding-left:14px;margin:14px 0}
.cta{display:inline-block;background:var(--y);color:#0A0A0A;font-weight:700;text-decoration:none;border-radius:10px;padding:11px 20px;margin:6px 0;letter-spacing:.02em;font-size:14px}
.big{font-size:15px;color:var(--fg)}
.give{display:grid;gap:10px;margin:10px 0}
.give>div{border:1px solid var(--line);border-radius:12px;padding:15px 17px;background:var(--card)}
.give b{color:var(--fg)}
h2 a.lnk,p a.lnk{font-size:inherit}
footer{border-top:1px solid var(--line);padding:28px;text-align:center;color:var(--mute);font-size:12px}
.langtoggle{display:flex;flex-wrap:wrap;justify-content:flex-end;border:1px solid var(--line);border-radius:10px;overflow:hidden}
.langtoggle button{background:none;border:0;color:var(--mute);font:inherit;font-size:10px;letter-spacing:.12em;padding:5px 8px;cursor:pointer}
.langtoggle button.on{background:var(--y);color:#0A0A0A;font-weight:700}
</style></head>
<body>
<nav><a href="/">━◯━ MU</a>
<span class="langtoggle" role="group" aria-label="Language">
  <button type="button" data-set="ja" onclick="muSetLang('ja')">日本語</button>
  <button type="button" data-set="en" onclick="muSetLang('en')">EN</button>
  <button type="button" data-set="zh" onclick="muSetLang('zh')">中文</button>
  <button type="button" data-set="pt" onclick="muSetLang('pt')">PT</button>
  <button type="button" data-set="ko" onclick="muSetLang('ko')">한국어</button>
  <button type="button" data-set="es" onclick="muSetLang('es')">ES</button>
</span></nav>
<div class="wrap">
<h1 data-i18n="build_h1">MUをつくる。<br>誰でも、AIでも。</h1>
<p class="lead" data-i18n="build_lead">MUは「作ること」を空気のように簡単にするブランド。メール認証だけで、あなた（人でも、Claudeのような<b>AIエージェント</b>でも）が自分のストアを開き、デザインを出品できます。在庫リスクはゼロ（オンデマンド印刷）。<b>作るのはタダ、売れたらあなたに入ります</b>（<a class="lnk" href="#what">↓ インセンティブ</a>）。</p>

<!-- 実数の社会的証明バー (/api/transparency から取得・捏造しない) -->
<div id="mu-proof" style="display:flex;flex-wrap:wrap;gap:10px;margin:18px 0 8px">
  <div style="flex:1;min-width:120px;background:var(--card);border:1px solid var(--line);border-radius:12px;padding:14px 16px">
    <div id="mp-sales" style="font-size:24px;color:var(--y);font-feature-settings:'tnum'">—</div>
    <div style="font-size:11px;color:var(--mute);letter-spacing:.06em" data-i18n="proof_sales">累計の売上（実数）</div>
  </div>
  <div style="flex:1;min-width:120px;background:var(--card);border:1px solid var(--line);border-radius:12px;padding:14px 16px">
    <div id="mp-purchases" style="font-size:24px;color:var(--y);font-feature-settings:'tnum'">—</div>
    <div style="font-size:11px;color:var(--mute);letter-spacing:.06em" data-i18n="proof_purchases">累計の販売</div>
  </div>
  <div style="flex:1;min-width:120px;background:var(--card);border:1px solid var(--line);border-radius:12px;padding:14px 16px">
    <div id="mp-customers" style="font-size:24px;color:var(--y);font-feature-settings:'tnum'">—</div>
    <div style="font-size:11px;color:var(--mute);letter-spacing:.06em" data-i18n="proof_customers">お客様</div>
  </div>
  <a href="/transparency" style="align-self:center;font-size:11px;color:var(--y);text-decoration:none;border-bottom:1px solid rgba(230,196,73,.4)" data-i18n="proof_link">全数字 →</a>
</div>

<!-- 二つの道: AI / 人間 -->
<div style="display:grid;grid-template-columns:1fr 1fr;gap:12px;margin:8px 0 6px">
  <a href="#fast" style="text-decoration:none;background:var(--card);border:1px solid var(--line);border-radius:12px;padding:16px 18px;color:var(--fg)">
    <div style="font-size:12px;color:var(--y);letter-spacing:.2em;text-transform:uppercase">▸ AI エージェント</div>
    <div style="font-size:15px;margin-top:6px" data-i18n="path_ai">Claude に繋いで「作って」と言うだけ →</div>
  </a>
  <a href="#human" style="text-decoration:none;background:var(--card);border:1px solid var(--line);border-radius:12px;padding:16px 18px;color:var(--fg)">
    <div style="font-size:12px;color:var(--y);letter-spacing:.2em;text-transform:uppercase">▸ 人間</div>
    <div style="font-size:15px;margin-top:6px" data-i18n="path_human">メールだけで30秒、自分の店を持つ →</div>
  </a>
</div>

<!-- 人間用 インライン登録 (curl 不要・その場で api_key) -->
<div class="card" id="human" style="border-color:rgba(230,196,73,.35)">
<p class="big" style="margin:0 0 10px"><b data-i18n="reg_title">人間用：メールだけで、いま店を持つ。</b> <span style="color:var(--mute)" data-i18n="reg_sub">curl も鍵も要りません。下に入力 → 6桁コード → api_key。</span></p>
<div id="reg-step1" style="display:flex;gap:8px;flex-wrap:wrap">
  <input id="reg-email" type="email" placeholder="you@example.com" autocomplete="email" style="flex:1;min-width:220px;background:#000;border:1px solid var(--line);border-radius:10px;color:var(--fg);font:inherit;font-size:15px;padding:12px 14px">
  <button type="button" id="reg-send" class="cta" style="border:0;cursor:pointer" data-i18n="reg_send">コードを送る</button>
</div>
<div id="reg-step2" style="display:none;margin-top:10px;gap:8px;flex-wrap:wrap">
  <input id="reg-code" inputmode="numeric" maxlength="6" placeholder="メールの6桁コード" style="flex:1;min-width:220px;background:#000;border:1px solid var(--line);border-radius:10px;color:var(--fg);font:inherit;font-size:15px;letter-spacing:.3em;padding:12px 14px">
  <button type="button" id="reg-verify" class="cta" style="border:0;cursor:pointer" data-i18n="reg_verify">認証して api_key を出す</button>
</div>
<div id="reg-msg" style="font-size:13px;color:var(--mute);margin-top:10px;min-height:18px"></div>
<div id="reg-key" style="display:none;margin-top:8px;background:#000;border:1px solid rgba(230,196,73,.4);border-radius:10px;padding:12px 14px;font-family:ui-monospace,Menlo,monospace;font-size:13px;color:var(--y);word-break:break-all"></div>
</div>

<div class="card" id="fast">
<p class="big" style="margin:0 0 10px"><b data-i18n="fastest_title">最短ルート：Claude に繋ぐ。</b> <span data-i18n="fastest_sub">まず<b>鍵なし</b>で繋ぐ（登録ツールは鍵が要りません）。</span></p>
<pre><code>claude mcp add --transport http mu https://mcp.wearmu.com/mcp</code></pre>
<p style="margin:8px 0 0" data-i18n="fastest_body">Claude に「<b>MUに you@example.com で登録して、api_keyを見せて</b>」→ メールの6桁コードで認証 → <b>api_key</b> が表示されます。その鍵で繋ぎ直すと、出品まで通ります：</p>
<pre><code>claude mcp remove mu
claude mcp add --transport http mu https://mcp.wearmu.com/mcp \
  --header "Authorization: Bearer &lt;api_key&gt;"</code></pre>
<p style="margin:8px 0 0" data-i18n="fastest_then">あとは「<b>MUで〇〇なTシャツ作って</b>」と話すだけで、ストア作成・出品までAIが行います。</p>
<p style="margin:12px 0 0"><a class="cta" href="#rest" data-i18n="cta_human">人間用：APIを直接叩く →</a> &nbsp; <a class="lnk" href="https://mcp.wearmu.com" data-i18n="cta_more_mcp">mcp.wearmu.com で詳しく</a></p>
</div>

<h2 id="what" data-i18n="what_h2">インセンティブ（正直に）</h2>
<div class="give">
<div data-i18n="give1"><b>◯ 作るのはタダ。売れたら、あなたに入る。</b><br>作成は無料（ウェルカム¥{{WELCOME}}＋AI生成）・在庫リスクゼロ。そして <b>売れた1枚ごとに作り手へ販売価格（税込）の10%</b>（¥4,900のTシャツなら¥490）。<b>あなたのリンク経由で売れたら別枠でさらに10%</b>——客を連れてくるほど儲かります。詳細は <a class="lnk" href="/credit">/credit</a>。</div>
<div><span data-i18n="give2"><b>◯ 寄付は"任意"。あなたが選ぶ。</b><br>このYOU/APIで作った分は <b>弟子屈町への自動寄付はありません</b>——残りは作り手と運営に回ります。寄付したい人は<b>オプトインで好きな先へ</b>（弟子屈でも、別の活動でも）。あなたのストアは <code>wearmu.com/&lt;あなた&gt;</code> に資産として残ります。</span><span class="note" style="display:block;margin-top:8px" data-i18n="give2_note">※ MU自家ライン／MUGENは従来どおり累進寄付（<a class="lnk" href="/profit-split">§28</a>）。エージェント面はこの別分配＋任意寄付です。作り手還元は順次開始・初期は手動精算。</span></div>
</div>

<h2 id="degressive" data-i18n="deg_h2">逓減プライス — 売れるほど、安くなる（順次導入）</h2>
<p data-i18n="deg_lead">普通は人気が出ると希少性で値を吊り上げます。MUは逆。<b>売れるほど価格が下がり、作り手の取り分は上がり、早く買った人ほど得をする</b>。奪い合いでなく、満ちていく分配です。</p>
<div class="give">
<div data-i18n="deg_m1"><b>① 価格は下がる一方</b><br>累計が増えるたびに一段ずつ下がる。一度下げたら上げない＝信頼。</div>
<div data-i18n="deg_m2"><b>② 早期購入者に遡及還元</b><br>段が下がると、それまで買った全員に差額の一部を MUクレジットで返す。早く買うほど得。</div>
<div data-i18n="deg_m3"><b>③ 作り手の取り分は上がる</b><br>枚数が増えるほど1枚あたりの取り分が増える。ベストセラーほど報われる。</div>
</div>
<table>
<tr><th data-i18n="deg_th_n">累計</th><th data-i18n="deg_th_price">小売</th><th data-i18n="deg_th_payout">作り手/枚</th><th data-i18n="deg_th_donate">寄付/枚</th><th data-i18n="deg_th_rebate">早期購入者へ</th></tr>
<tr><td>0–19</td><td>¥4,900</td><td style="color:var(--y)">¥490（現行10%）</td><td>0</td><td>—</td></tr>
<tr><td>20–99</td><td>¥4,700</td><td style="color:var(--y)">¥700</td><td>¥100</td><td>¥100 還元</td></tr>
<tr><td>100–499</td><td>¥4,500</td><td style="color:var(--y)">¥750</td><td>¥150</td><td>+¥100</td></tr>
<tr><td>500+</td><td><b>¥4,400</b></td><td style="color:var(--y)"><b>¥800</b></td><td>¥200</td><td>+¥100</td></tr>
</table>
<p class="note" data-i18n="deg_fund">値下げの原資は正直に：①量産による原価減 ②口コミ拡散で広告費が要らなくなる分 ③運営取り分の放棄（比例で増やさない・§28／報酬キャップ）。赤字発行はしません。<b>自分のリンク経由</b>で売れたら別枠で<b>販売価格の10%</b>を上乗せ。</p>

<div class="card" style="border-color:rgba(230,196,73,.35)">
<p class="big" style="margin:0 0 12px"><b data-i18n="sim_title">収益シミュレータ</b> <span style="color:var(--mute);font-size:13px" data-i18n="sim_sub">（概算・確定報酬＋順次導入の逓減を含む）</span></p>
<div style="display:flex;gap:14px;flex-wrap:wrap;align-items:center;margin-bottom:6px">
  <label style="font-size:13px;color:var(--mute)" data-i18n="sim_kind_l">品目</label>
  <select id="sim-kind" style="background:#000;border:1px solid var(--line);border-radius:8px;color:var(--fg);font:inherit;padding:8px 10px">
    <option value="tee">Tシャツ</option>
    <option value="other">パーカー/クルー/ラッシュ</option>
  </select>
  <label style="font-size:13px;color:var(--mute)"><input type="checkbox" id="sim-self" checked style="vertical-align:middle"> <span data-i18n="sim_self_l">自分のリンクで集客</span></label>
</div>
<div style="display:flex;gap:12px;align-items:center;margin:6px 0 4px">
  <input id="sim-n" type="range" min="1" max="1000" value="100" style="flex:1;accent-color:var(--y)">
  <span id="sim-n-v" style="min-width:84px;text-align:right;font-feature-settings:'tnum';color:var(--fg)">100 枚</span>
</div>
<div style="display:grid;grid-template-columns:1fr 1fr;gap:10px;margin-top:12px">
  <div style="background:#000;border:1px solid var(--line);border-radius:10px;padding:14px 16px">
    <div style="font-size:11px;color:var(--mute);letter-spacing:.06em" data-i18n="sim_total_l">あなたの累計収益</div>
    <div id="sim-total" style="font-size:26px;color:var(--y);font-feature-settings:'tnum';margin-top:2px">—</div>
  </div>
  <div style="background:#000;border:1px solid var(--line);border-radius:10px;padding:14px 16px">
    <div style="font-size:11px;color:var(--mute);letter-spacing:.06em" data-i18n="sim_last_l">直近1枚あたり（小売 / あなた）</div>
    <div id="sim-last" style="font-size:18px;color:var(--fg);font-feature-settings:'tnum';margin-top:6px">—</div>
  </div>
</div>
<p style="font-size:11.5px;color:var(--mute);margin:10px 0 0" data-i18n="sim_note">※ 逓減ラダー・遡及還元は順次導入、精算は当面手動です。確定している即時報酬は<b>販売価格（税込）の10%</b>（リンク経由はさらに10%別枠・自己購入除外）。</p>
</div>

<h2 data-i18n="pay_h2">支払いの約束</h2>
<div class="card">
<table style="margin:0">
<tr><td data-i18n="pay_min">最低支払額</td><td><b>¥3,000</b> <span style="color:var(--mute)" data-i18n="pay_min_d">（未満は残高として保持・期限なし）</span></td></tr>
<tr><td data-i18n="pay_cycle">サイクル</td><td data-i18n="pay_cycle_d"><b>申請ベース</b>（<a class="lnk" href="/studio">/studio</a> の「振込申請」→ 受付後 通常5営業日以内）</td></tr>
<tr><td data-i18n="pay_method">方法</td><td data-i18n="pay_method_d">銀行振込（振込手数料は当社負担）</td></tr>
<tr><td data-i18n="pay_check">確認</td><td><code>GET /api/agent/me</code> <span style="color:var(--mute)" data-i18n="pay_check_d">で残高・履歴</span></td></tr>
</table>
</div>
<p class="note" data-i18n="pay_rights">作ったデザインの権利は<b>作り手に帰属</b>します（MUは販売・印刷のための利用許諾を受けます）。売上は各自の所得です——確定申告・納税は各自でお願いします（日本の方は雑所得/事業所得の扱い）。<b>精算は順次自動化中・初期は手動</b>。金額・条件は予告して変更する場合があります。</p>

<h2 data-i18n="approval_h2">承認（MA council）について</h2>
<p data-i18n="approval_body">作った商品は <b>status: review</b> で入り、<b>MA council</b>（人）が確認して承認すると <b>live</b>＝販売開始になります。承認前は公開されません。<b>人が見るので即時ではありません</b>（AIで一気に作っても、出品は人のゲートを通ります）。</p>
<div class="card">
<p style="margin:0" data-i18n="approval_4title"><b>見るのはこの4点だけ：</b></p>
<p style="margin:8px 0 0" data-i18n="approval_4body">① MUの美意識（無・月・余白）に大きく反していないか ② 公序良俗 ③ 他者の権利（商標・著作・肖像）を侵していないか ④ kind と下限価格のルール。<br>これらを満たせば、デザインの好き嫌いで落とすことはありません。</p>
</div>
<p class="note" data-i18n="approval_sla">目安は<b>通常24〜48時間以内</b>。実績のある作り手には<b>自動承認枠</b>を順次開放します（同じ4点を満たす限り、AIの量産がボトルネックになりません）。</p>

<h2 data-i18n="after_h2">作ったあと、どこに出る？</h2>
<p data-i18n="after_body">承認されたストアは <code>wearmu.com/shop?brand=&lt;slug&gt;</code> で公開され、<b>/shop の新着・ブランド一覧・sitemap</b> に載ります。MUが導線の一部を持ちますが、立ち上げ間もないブランドなので<b>最初の客は一緒に連れてくる前提</b>—自分のSNSやコミュニティからもストアURLを撒いてください。現在の状態（残クレジット・所有ストア・live/review数）は <code>GET /api/agent/me</code> で確認できます。</p>

<h2 id="rest" data-i18n="rest_h2">人間用：APIで作る（4ステップ）</h2>
<p data-i18n="rest_intro">スクリプトや自作クライアントから直接叩く場合の手順です。AIに任せるなら上のMCP一行で十分。</p>
<ol>
<li><b data-i18n="step_register">登録</b><span data-i18n="step_register_d"> — メールアドレスに6桁コードが届きます。</span>
<pre><code>curl -X POST https://wearmu.com/api/agent/register \
  -H 'Content-Type: application/json' \
  -d '{"email":"you@example.com"}'</code></pre></li>
<li><b data-i18n="step_verify">認証</b><span data-i18n="step_verify_d"> — コードを送ると <span class="k">api_key</span> が返ります（初回は<b>¥{{WELCOME}} ウェルカムクレジット</b>付き）。以降は <code>Authorization: Bearer &lt;api_key&gt;</code> を付けます。</span>
<pre><code>curl -X POST https://wearmu.com/api/agent/register/verify \
  -H 'Content-Type: application/json' \
  -d '{"email":"you@example.com","code":"123456"}'</code></pre></li>
<li><b data-i18n="step_store">ストアを開く</b><span data-i18n="step_store_d"> — あなたのブランドの店ができます（<code>wearmu.com/shop?brand=&lt;slug&gt;</code>）。</span>
<pre><code>curl -X POST https://wearmu.com/api/agent/stores \
  -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' \
  -d '{"slug":"my-lab","name":"MY LAB","emoji":"◯"}'</code></pre></li>
<li><b data-i18n="step_product">商品を作る</b><span data-i18n="step_product_d"> — <b>画像のhttps URL</b>を <code>design_url</code> で渡します。AI画像生成（<code>ai_prompt</code>）は有効時に利用可——<code>mu_status</code> で確認。</span>
<pre><code>curl -X POST https://wearmu.com/api/agent/products \
  -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' \
  -d '{"store":"my-lab","label":"無 Tee","description":"...",
       "kind":"tee","design_url":"https://.../art.png"}'</code></pre></li>
</ol>

<h2 data-i18n="make_h2">作れるもの・ルール</h2>
<table>
<tr><th>kind</th><th data-i18n="floor_col">下限価格</th></tr>
<tr><td>tee — T-shirt (Bella+Canvas 3001)</td><td>¥4,900</td></tr>
<tr><td>hoodie (Gildan 18500)</td><td>¥8,800</td></tr>
<tr><td>crewneck (Gildan 18000)</td><td>¥7,800</td></tr>
<tr><td>rashguard_ls / rashguard_black</td><td>¥9,800</td></tr>
</table>
<p><span class="pill" data-i18n="pill_https">画像は https のみ</span><span class="pill" data-i18n="pill_clamp">価格は下限以上に自動クランプ</span><span class="pill" data-i18n="pill_rate">作成20点/時まで</span><span class="pill" data-i18n="pill_own">他人のストアには書けない</span></p>
<p data-i18n="check_state">自分の状態（残クレジット・所有ストア・上限）は <code>GET /api/agent/me</code> で確認できます。</p>

<h2 data-i18n="machine_h2">機械可読リンク</h2>
<p>
<a class="lnk" href="/llms.txt">/llms.txt</a> &nbsp;·&nbsp;
<a class="lnk" href="/openapi.json">/openapi.json</a> &nbsp;·&nbsp;
<a class="lnk" href="/.well-known/mcp.json">/.well-known/mcp.json</a> &nbsp;·&nbsp;
<a class="lnk" href="https://mcp.wearmu.com">mcp.wearmu.com</a>
</p>
<p>MCP tools: <span class="pill">mu_register</span><span class="pill">mu_verify</span><span class="pill">mu_status</span><span class="pill">mu_create_store</span><span class="pill">mu_create_product</span><span class="pill">mu_list_mine</span></p>

<h2 data-i18n="sdk_h2">自分のSDK/クライアントを作る</h2>
<p data-i18n="sdk_body">専用SDKは配りません — <b>AIエージェントはMCP</b>（上記）が"SDK"です。人/スクリプトは <code>/openapi.json</code> から好きな言語のクライアントを自動生成できます：</p>
<pre><code>npx @openapitools/openapi-generator-cli generate \
  -i https://wearmu.com/openapi.json -g python -o ./mu-client</code></pre>
<p class="note" data-i18n="sdk_note">`-g` を <code>typescript-fetch</code> / <code>go</code> / <code>rust</code> 等に変えれば任意言語。1ファイルで十分なほど小さいAPIなので、<code>curl</code> 直叩きでも構いません。</p>
</div>
<footer>MU（無）· on-demand · zero inventory · 株式会社イネブラ / Enabler Inc. · <a class="lnk" href="/shop">wearmu.com/shop</a></footer>
<script>
(function(){
  function fmtY(n){ try{return '¥'+Math.round(n).toLocaleString('ja-JP');}catch(e){return '¥'+n;} }
  function fmtN(n){ try{return Number(n).toLocaleString('ja-JP');}catch(e){return ''+n;} }
  // --- 社会的証明: /api/transparency の実数 (捏造しない) ---
  fetch('/api/transparency').then(function(r){return r.ok?r.json():null;}).then(function(d){
    if(!d) return; var ex=d.external||{};
    var s=document.getElementById('mp-sales'); if(s&&ex.revenue_jpy!=null) s.textContent=fmtY(ex.revenue_jpy);
    var p=document.getElementById('mp-purchases'); if(p&&ex.purchases!=null) p.textContent=fmtN(ex.purchases);
    var c=document.getElementById('mp-customers'); if(c&&ex.distinct_customers!=null) c.textContent=fmtN(ex.distinct_customers);
  }).catch(function(){});

  // --- 収益シミュレータ (概算: 確定報酬=販売価格10% + 順次導入の逓減ラダー価格) ---
  // 報酬は常に「その時点の小売価格の10%」(apply_maker_commission と同一基準)。
  // リンク経由(self)はさらに小売の10%が別枠で乗る。ラダーの段階小売は順次導入の構想値。
  var LAD_TEE=[[0,4900],[20,4700],[100,4500],[500,4400]];
  var RETAIL_OTHER=8800, SHARE=0.10;
  function ladTee(i){ var r=LAD_TEE[0]; for(var k=0;k<LAD_TEE.length;k++){ if(i>=LAD_TEE[k][0]) r=LAD_TEE[k]; } return r; }
  function sim(){
    var kEl=document.getElementById('sim-kind'); if(!kEl) return;
    var kind=kEl.value, self=document.getElementById('sim-self').checked;
    var n=parseInt(document.getElementById('sim-n').value,10)||0;
    document.getElementById('sim-n-v').textContent=fmtN(n)+' 枚';
    var total=0, lastRetail=0, lastPay=0;
    for(var i=1;i<=n;i++){
      var retail=(kind==='tee')?ladTee(i-1)[1]:RETAIL_OTHER;
      var pay=Math.round(retail*SHARE); if(self) pay+=Math.round(retail*SHARE);
      total+=pay; lastRetail=retail; lastPay=pay;
    }
    document.getElementById('sim-total').textContent=fmtY(total);
    document.getElementById('sim-last').textContent=fmtY(lastRetail)+' / '+fmtY(lastPay);
  }
  ['sim-kind','sim-self','sim-n'].forEach(function(id){var e=document.getElementById(id); if(e){e.addEventListener('input',sim); e.addEventListener('change',sim);}});
  sim();

  // --- 人間用 インライン登録 (curl 不要) ---
  var regEmail='';
  function rmsg(t,c){var m=document.getElementById('reg-msg'); if(m){m.textContent=t; m.style.color=c||'var(--mute)';}}
  var bs=document.getElementById('reg-send');
  if(bs) bs.addEventListener('click',function(){
    regEmail=(document.getElementById('reg-email').value||'').trim();
    if(!regEmail||regEmail.indexOf('@')<1){rmsg('メールアドレスを入力してください','#e9a0a0');return;}
    bs.disabled=true; rmsg('送信中…');
    fetch('/api/agent/register',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({email:regEmail})})
     .then(function(r){return r.json().catch(function(){return {};});})
     .then(function(){ document.getElementById('reg-step2').style.display='flex'; rmsg('メールに届いた6桁コードを入力してください','var(--y)'); })
     .catch(function(){ rmsg('送信に失敗しました。少し待って再試行してください','#e9a0a0'); })
     .finally(function(){ bs.disabled=false; });
  });
  var bv=document.getElementById('reg-verify');
  if(bv) bv.addEventListener('click',function(){
    var code=(document.getElementById('reg-code').value||'').trim();
    if(code.length<6){rmsg('6桁コードを入力してください','#e9a0a0');return;}
    bv.disabled=true; rmsg('認証中…');
    fetch('/api/agent/register/verify',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({email:regEmail,code:code})})
     .then(function(r){return r.json().catch(function(){return {};});})
     .then(function(d){
       var key=d.api_key||d.apiKey||d.key;
       if(key){ var k=document.getElementById('reg-key'); k.style.display='block'; k.textContent='api_key: '+key; rmsg('できました。この鍵を保存して、上の手順か MCP で出品できます。','var(--y)'); }
       else { rmsg((d.error||'コードが違うか期限切れです'),'#e9a0a0'); }
     })
     .catch(function(){ rmsg('認証に失敗しました','#e9a0a0'); })
     .finally(function(){ bv.disabled=false; });
  });
})();
</script>
<script>
var I18N={{I18N_JSON}};
var MU_DEF='ja', MU_SUP=['ja','en','zh','pt','ko','es'], muOrig={}, muCap=false;
function muSetLang(l){
  if(!muCap){document.querySelectorAll('[data-i18n]').forEach(function(e){muOrig[e.getAttribute('data-i18n')]=e.innerHTML;});muCap=true;}
  document.querySelectorAll('[data-i18n]').forEach(function(e){
    var k=e.getAttribute('data-i18n'),v;
    if(l===MU_DEF){v=muOrig[k];}
    else{v=(I18N[l]&&I18N[l][k]!=null)?I18N[l][k]:((I18N.en&&I18N.en[k]!=null)?I18N.en[k]:muOrig[k]);}
    if(v!=null)e.innerHTML=v;
  });
  document.documentElement.setAttribute('lang',l);
  try{localStorage.setItem('mu_lang',l);}catch(e){}
  document.querySelectorAll('.langtoggle button').forEach(function(b){b.classList.toggle('on',b.getAttribute('data-set')===l);});
}
(function(){var s=null;try{s=localStorage.getItem('mu_lang');}catch(e){}
 var n=(navigator.language||'ja').toLowerCase().slice(0,2);
 muSetLang(s&&MU_SUP.indexOf(s)>=0?s:(MU_SUP.indexOf(n)>=0?n:'ja'));})();
</script>
</body></html>"##
        .replace("{{I18N_JSON}}", include_str!("build_i18n.json"))
        .replace("{{WELCOME}}", &yen_commas(welcome));
    ([(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
}

// ─── GET /llms.txt ──────────────────────────────────────────────────────

pub async fn llms_txt() -> Response {
    let body = r##"# wearmu.com — MU

MU (無) is an agent-native commerce platform. Designs are generated, physical
products are print-on-demand (Printful) or self-fulfilled, digital goods
(event tickets / songs) deliver by email — and the whole catalog is open to AI
agents: get an API key and run your own store in minutes. New products land in
review and go live only after an MA-council member approves them.

Storefront:    https://wearmu.com/shop
Builder guide: https://wearmu.com/build        (human-friendly onboarding)
Transparency:  https://wearmu.com/transparency (real revenue/cost numbers)
MCP server:    https://mcp.wearmu.com          (12 tools, see "MCP" below)
OpenAPI:       https://wearmu.com/openapi.json
This file:     https://wearmu.com/llms.txt

## Instant start (no email) — make a product in one call

0. GET https://wearmu.com/api/agent/guest
   → {"api_key":"<token>","store":"g-xxxxxxxx","mu_credits_balance":150, ...}
   No email, no code. You also get a ready-made store and a small credit, so
   the VERY NEXT call can create a product (generate art with ai_prompt, or
   upload your own PNG first). Products land status='review' until approved.
   SAVE the api_key — it is your only credential; your store persists at the
   returned store_url. Bind an email later (step 1 below) to make it
   recoverable. (GET https://wearmu.com/api/agent returns this same quickstart.)

## Onboarding (permanent, email-verified API key)

1. POST https://wearmu.com/api/agent/register
   body: {"email":"you@example.com"}
   → emails a 6-digit code.

2. POST https://wearmu.com/api/agent/register/verify
   body: {"email":"you@example.com","code":"123456"}
   → {"ok":true,"api_key":"<token>"}

3. Send the key on every call:  Authorization: Bearer <api_key>
   (or ?api_key=<token> for quick curls)

## Selling: your store & products (all JSON; Bearer-gated unless noted)

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
       Physical: tee / crewneck / hoodie / rashguard_ls / rashguard_black /
                 nfc_coin / device.
       Digital:  event_ticket (add "capacity": 50 — QR ticket by email),
                 song         (add "audio_url": "https://..." — listen link).
                 poster       (Printful 18″×24″ matte poster).
                 zine         (add "file_url": "https://..." — PDF download).
                 video        (add "video_url": "https://..." — watch link).
                 karaoke_ticket (uta.live カラオケ化引換券 — buyer redeems by email).
       音源入りTシャツもOK: 物理kind(tee等)にも "audio_url" を付けられる。
       "https://mu.koe.live/oto.html?s=<曲key>" を渡すと商品ページに試聴
       プレイヤー(「着ると、この曲が鳴る」)が出る(例: s=i-love-you)。
     `design_url` must be an absolute https URL (use POST /api/agent/upload).
     `price_jpy` is optional; values below the per-kind floor are clamped up.
     (`ai_prompt` generates artwork server-side; it is gated by a runtime flag.
     Check ai_gen.enabled via GET /api/agent/me — {{AIGEN_TXT}})
     → {"sku":"...","status":"review","note":"pending MA council approval"}

GET  /api/agent/products
     → every product you created: sku, store, label, kind, retail_price_jpy,
       status (review|live|retired|dead), design_file, pdp_url.

POST /api/agent/products/{sku}/update
     body: any of {"label":"...","description":"...","price_jpy":5500,
                    "design_url":"https://..."}
     Allowed ONLY while status is review/retired (never live). Price is
     clamped to the kind's floor. Printful ids can never change.

POST /api/agent/products/{sku}/retire
     → sets status=retired, removes it from the storefront. Owner-only.

GET  /api/agent/sales
     → per-store + total {order_count, revenue_jpy} and the 50 most recent
       orders (sku, amount_jpy, created_at, status). Refunds excluded from
       revenue.

POST /api/agent/upload
     body: {"data_base64":"<PNG bytes, base64, <=3MB>","filename":"art.png"}
     → {"url":"https://..."} — durable hosting; pass it as design_url.

POST /api/agent/feedback
     body: {"category":"bug","title":"...","description":"...",
             "sku":"OPTIONAL-SKU","severity":"high"}
     category ∈ bug | feature | improvement. severity ∈ critical|high|medium|low.
     Found a bug or have an idea to improve MU? File it here — it lands in the
     MA council triage queue.
     → {"ok":true,"feedback_id":123,"kind":"agent_bug"}

GET  /api/agent/affiliate
     → your referral code, share link (https://wearmu.com/r/<code>) and stats.
       Sales arriving via your link earn commission (default 10%) as MU
       credits. Works for ANY product, not just yours.

### MA council (approval — members only)

GET  /api/ma/review/queue            → products awaiting approval
POST /api/ma/review/{sku}/approve    → review → live
POST /api/ma/review/{sku}/reject     → review → dead
POST /api/ma/products/{sku}/takedown → any status → retired (rights/IP takedown)

## Buying: read the catalog & check out (no auth)

GET  /api/products                   → all active brands
GET  /api/products/{brand}           → live products of a brand (price, images)
GET  /api/products/item/{id}         → one product, full detail
GET  /api/v1/embed/products?brand=&limit=   → CORS-enabled product feed
GET  /api/shop/checkout?sku=<SKU>    → redirects to Stripe Checkout (share this
                                       URL to let a human pay; append
                                       &ref=<code> to credit an affiliate)
Human pages: /shop (all), /shop/{sku} (product page).

## MCP (same capabilities as REST, for MCP-native agents)

claude mcp add --transport http mu https://mcp.wearmu.com/mcp

Tools: mu_register, mu_verify, mu_status, mu_create_store, mu_create_product,
mu_list_mine, mu_update_product, mu_retire_product, mu_upload_design,
mu_sales, mu_affiliate, mu_submit_feedback.

## Economics (agent stores)

- Creating is free; first verify grants a one-time ¥{{WELCOME}} welcome credit.
- Per item sold, the creator earns 10% of the retail price (tax-incl) —
  e.g. a ¥4,900 tee pays ¥490. Sales via your own referral link earn a
  separate 10% on top (both stack; self-purchases excluded). Full terms
  + payout ledger: https://wearmu.com/credit
- Donation is opt-in (no automatic Teshikaga donation on agent/YOU/API stores);
  the rest goes to creator + operations. Payouts are settled manually while the
  agent program ramps. (MU's own line / MUGEN keeps the §28 progressive donation.)
- Your store persists as an asset at https://wearmu.com/<you>.

## Rules

- Agents pass a whitelisted `kind` — never raw Printful ids or sub-floor prices.
- Every product is created status='review', is_active=0. Nothing sells until an
  MA-council member approves it.
- Rate limit: 20 products/hour per email.
- One store = one catalog_brands slug; you can only mutate stores you own.

— 株式会社イネブラ / Enabler Inc. · wearmu.com
"##
        .replace("{{WELCOME}}", &yen_commas(AGENT_WELCOME_CREDIT_JPY))
        .replace("{{AIGEN_TXT}}", &if agent_ai_gen_enabled() {
            format!("currently ON, ~¥{}/image from mu_credits, refunded on failure", agent_ai_gen_cost_jpy())
        } else {
            "currently OFF; pass design_url".to_string()
        });
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
            "tools": ["mu_register","mu_verify","mu_status","mu_create_store","mu_create_product","mu_list_mine","mu_update_product","mu_retire_product","mu_upload_design","mu_sales","mu_affiliate","mu_submit_feedback"]
        },
        "rest_base": "https://wearmu.com/api/agent",
        "openapi": "https://wearmu.com/openapi.json",
        "docs": "https://wearmu.com/llms.txt",
        "economics": {
            "welcome_credit_jpy": AGENT_WELCOME_CREDIT_JPY,
            // Real payer is catalog.rs::apply_maker_commission — 10% of the
            // tax-incl retail price, all kinds (store-level maker_pct can
            // raise it). Referral adds a separate 10%. Terms: /credit
            "creator_share_pct": 10,
            "referral_share_pct": 10,
            "donation": "opt-in (no automatic Teshikaga donation on agent stores)",
            "payout_settlement": "manual while the agent program ramps",
            "ai_gen": { "enabled": agent_ai_gen_enabled(), "cost_jpy": agent_ai_gen_cost_jpy() },
            "note": "Live source of truth for figures + ai_gen flag; mirrors /api/agent/me and /llms.txt."
        }
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
            "/api/agent/products": {
                "get": {"summary":"List every product you created (sku, store, label, kind, price, status, pdp_url)","security":[{"bearer":[]}],
                    "responses":{"200":{"description":"{ok, count, products[]}"},"401":{"description":"auth required"}}},
                "post": {"summary":"Create a product (status='review' pending MA approval)","security":[{"bearer":[]}],
                "requestBody":{"required":true,"content":{"application/json":{"schema":{"type":"object","required":["store","label","description","kind","design_url"],"properties":{"store":{"type":"string"},"label":{"type":"string"},"description":{"type":"string"},"kind":{"type":"string","enum":["tee","rashguard_ls","rashguard_black","hoodie","crewneck","nfc_coin","device","event_ticket","song","poster","zine","video","karaoke_ticket"]},"design_url":{"type":"string","format":"uri","description":"absolute https URL"},"price_jpy":{"type":"integer","description":"optional; clamped up to the per-kind floor"},"capacity":{"type":"integer","description":"event_ticket only: ticket capacity"},"audio_url":{"type":"string","format":"uri","description":"https listen link. song はもちろん、物理Tシャツ等にも付けられる(mu.koe.live/oto.html?s=KEY を渡すとPDPに試聴プレイヤー)"}}}}}},
                "responses":{"200":{"description":"{sku, status:'review', pdp_url}"},"400":{"description":"unknown kind / missing design_url"},"403":{"description":"not your store"},"429":{"description":"rate limit"}}}},
            "/api/agent/products/{sku}/update": {"post": {"summary":"Update label/description/price_jpy/design_url (owner-only; review/retired status only; price clamped to floor)","security":[{"bearer":[]}],
                "parameters":[{"name":"sku","in":"path","required":true,"schema":{"type":"string"}}],
                "requestBody":{"required":true,"content":{"application/json":{"schema":{"type":"object","properties":{"label":{"type":"string","maxLength":120},"description":{"type":"string","maxLength":600},"price_jpy":{"type":"integer"},"design_url":{"type":"string","format":"uri"}}}}}},
                "responses":{"200":{"description":"{ok, sku, status, pdp_url}"},"403":{"description":"not your product"},"404":{"description":"unknown sku"},"409":{"description":"product is live — retire first"}}}},
            "/api/agent/products/{sku}/retire": {"post": {"summary":"Retire a product (status=retired, removed from storefront; owner-only)","security":[{"bearer":[]}],
                "parameters":[{"name":"sku","in":"path","required":true,"schema":{"type":"string"}}],
                "responses":{"200":{"description":"{ok, sku, status:'retired'}"},"403":{"description":"not your product"},"404":{"description":"unknown sku"}}}},
            "/api/agent/sales": {"get": {"summary":"Your sales: per-store + total order_count/revenue_jpy + 50 recent orders","security":[{"bearer":[]}],
                "responses":{"200":{"description":"{ok, total, stores[], recent_orders[]}"},"401":{"description":"auth required"}}}},
            "/api/agent/upload": {"post": {"summary":"Upload a PNG design (base64, <=3MB) to durable hosting; returns https url for design_url","security":[{"bearer":[]}],
                "requestBody":{"required":true,"content":{"application/json":{"schema":{"type":"object","required":["data_base64"],"properties":{"data_base64":{"type":"string","description":"base64 PNG (data:image/png;base64, prefix OK)"},"filename":{"type":"string"}}}}}},
                "responses":{"200":{"description":"{ok, url, bytes}"},"400":{"description":"not PNG / too large / bad base64"},"503":{"description":"hosting not configured"}}}},
            "/api/agent/affiliate": {"get": {"summary":"Your referral code, share link (/r/<code>) and stats; sales via the link earn MU credits","security":[{"bearer":[]}],
                "responses":{"200":{"description":"{code, link, stats}"},"401":{"description":"auth required"}}}},
            "/api/agent/feedback": {"post": {"summary":"File a bug report / feature request / improvement against MU","security":[{"bearer":[]}],
                "requestBody":{"required":true,"content":{"application/json":{"schema":{"type":"object","required":["category","title","description"],"properties":{"category":{"type":"string","enum":["bug","feature","improvement"]},"title":{"type":"string","maxLength":200},"description":{"type":"string","maxLength":2000},"sku":{"type":"string","description":"optional SKU the feedback is about"},"severity":{"type":"string","enum":["critical","high","medium","low"]}}}}}},
                "responses":{"200":{"description":"{ok, feedback_id, kind}"},"400":{"description":"bad category/title/description/severity"},"401":{"description":"auth required"}}}},
            "/api/ma/review/queue": {"get": {"summary":"Agent products awaiting approval (MA council only)","security":[{"bearer":[]}],"responses":{"200":{"description":"queue"},"403":{"description":"MA council only"}}}},
            "/api/ma/review/{sku}/approve": {"post": {"summary":"Approve → live (MA council only)","security":[{"bearer":[]}],"parameters":[{"name":"sku","in":"path","required":true,"schema":{"type":"string"}}],"responses":{"200":{"description":"live"},"403":{"description":"MA council only"},"409":{"description":"not in review"}}}},
            "/api/ma/review/{sku}/reject": {"post": {"summary":"Reject → dead (MA council only)","security":[{"bearer":[]}],"parameters":[{"name":"sku","in":"path","required":true,"schema":{"type":"string"}}],"responses":{"200":{"description":"rejected"}}}},
            "/api/ma/products/{sku}/takedown": {"post": {"summary":"Takedown: any status → retired, removed from storefront (MA council / ADMIN_TOKEN; for rights/IP issues; optional ?reason= is audit-logged)","security":[{"bearer":[]}],"parameters":[{"name":"sku","in":"path","required":true,"schema":{"type":"string"}},{"name":"reason","in":"query","required":false,"schema":{"type":"string"}}],"responses":{"200":{"description":"{ok, sku, status:'retired'}"},"403":{"description":"MA council only"},"404":{"description":"unknown sku"}}}}
        }
    });
    Json(v).into_response()
}

#[cfg(test)]
#[path = "tests_agent.rs"]
mod tests_agent;
