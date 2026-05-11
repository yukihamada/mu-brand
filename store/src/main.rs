mod gemini;
mod nft;
mod payments;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Json, Response},
    routing::{get, patch, post},
    Router,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::{sync::{Arc, Mutex}, env, time::{SystemTime, UNIX_EPOCH}};
use tower_http::services::ServeDir;
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

/// Centralized admin token check. Fail-closed: if ADMIN_TOKEN env var
/// is missing or empty, every admin request is rejected with 503.
/// Never falls back to a default value (prevents the historical
/// "mu-admin" default-token vulnerability).
fn require_admin_token(provided: Option<&String>) -> Result<(), Response> {
    let expected = match env::var("ADMIN_TOKEN") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!("[security] ADMIN_TOKEN env var is unset — rejecting admin request");
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "admin disabled (server misconfigured: ADMIN_TOKEN not set)",
            ).into_response());
        }
    };
    let provided = provided.map(String::as_str).unwrap_or("");
    // Constant-time comparison to prevent timing attacks
    if provided.len() != expected.len() {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized").into_response());
    }
    let mut diff: u8 = 0;
    for (a, b) in provided.bytes().zip(expected.bytes()) {
        diff |= a ^ b;
    }
    if diff != 0 {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized").into_response());
    }
    Ok(())
}

/// Add baseline security response headers to every reply.
async fn security_headers(req: Request<axum::body::Body>, next: Next) -> Response {
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert("X-Content-Type-Options", HeaderValue::from_static("nosniff"));
    h.insert("X-Frame-Options", HeaderValue::from_static("SAMEORIGIN"));
    h.insert("Referrer-Policy", HeaderValue::from_static("strict-origin-when-cross-origin"));
    h.insert("Strict-Transport-Security",
             HeaderValue::from_static("max-age=31536000; includeSubDomains"));
    h.insert("Permissions-Policy",
             HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=(self \"https://js.stripe.com\")"));
    resp
}

type Db = Arc<Mutex<Connection>>;

#[derive(Serialize, Default)]
struct Product {
    id: i64,
    brand: String,
    drop_num: i64,
    name: String,
    /// YYYYMMDD-#NNN per-day-ordinal serial. NEW canonical product code.
    /// Computed at read time from created_at + ordinal within that JST day.
    serial_code: String,
    mockup_url: Option<String>,
    price_jpy: i64,
    inventory: i64,
    sold: i64,
    created_at: String,
    weather_data: Option<String>,
    prompt_hash: Option<String>,
    seed_data: Option<String>,
    nft_mint: Option<String>,
    auction_end: Option<String>,
    current_bid: i64,
    bid_count: i64,
    sold_out_at: Option<String>,
    /// Generated "person wearing this design" photo (Gemini image-to-image).
    /// Optional; falls back to mockup_url client-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    lifestyle_url: Option<String>,
}

#[derive(Deserialize)]
struct CheckoutBody {
    product_id: i64,
    quantity: u32,
    email: String,
    size: Option<String>,
    wallet: Option<String>,
    /// "jpy" (default, Stripe Checkout), "usdc", "sol", "eth"
    payment_method: Option<String>,
    /// Required when the final billed total (unit_price × quantity) is at or
    /// above `KYC_THRESHOLD_JPY` (¥300,000). Stored in `kyc_records`.
    kyc: Option<KycInfo>,
}

#[derive(Deserialize)]
struct KycInfo {
    /// Legal full name as printed on ID
    full_name: String,
    /// YYYY-MM-DD
    date_of_birth: String,
    /// ISO 3166-1 alpha-2 (e.g. "JP")
    nationality: String,
    /// "passport" | "license" | "mynumber" | "residence_card"
    id_type: String,
    /// Last 4 chars of the ID number — keep storage minimized
    id_last4: String,
    /// Free-form residential address
    address: String,
    /// ISO 8601 timestamp the user clicked the consent checkbox
    consent_at: String,
}

#[derive(Deserialize)]
struct BidBody {
    product_id: i64,
    amount: i64,
    email: String,
    wallet: Option<String>,
    /// Required when amount >= ¥300,000 (`KYC_THRESHOLD_JPY`).
    kyc: Option<KycInfo>,
    /// Soulbound NFT pilot: when true and `nft_wallet` is a plausible Solana
    /// address, the auction-winner flow will mint a Soulbound cNFT certificate
    /// to that wallet on settle. See `nft::mint_soulbound` for details.
    #[serde(default)]
    nft_opt_in: bool,
    /// Solana wallet to receive the Soulbound NFT (only used when
    /// `nft_opt_in` is true). Falls back to `wallet` if not given.
    #[serde(default)]
    nft_wallet: Option<String>,
}

#[derive(Deserialize)]
struct UpdateMockupBody {
    product_id: i64,
    mockup_url: String,
}

#[derive(Deserialize)]
struct FragmentBody {
    email:     String,
    direction: String,
    order_ids: String,
}

#[derive(Deserialize)]
struct ImportProductBody {
    brand: String,
    drop_num: i64,
    name: String,
    design_url: Option<String>,
    mockup_url: Option<String>,
    price_jpy: i64,
    inventory: i64,
    weather_data: Option<String>,
    prompt_hash: Option<String>,
    seed_data: Option<String>,
    auction_end: Option<String>,
    nft_mint: Option<String>,
}

#[derive(Deserialize)]
struct UpdatePriceBody {
    brand: String,
    drop_num: i64,
    price_jpy: i64,
}

#[derive(Deserialize)]
struct UpdateNftBody {
    brand: String,
    drop_num: i64,
    nft_mint: String,
}

#[derive(Deserialize)]
struct UpdateDesignBody {
    brand: String,
    drop_num: i64,
    design_url: String,
}

#[derive(Deserialize)]
struct UpdateSoldBody {
    brand: String,
    drop_num: i64,
    sold: i64,
}

#[derive(Deserialize)]
struct UpdateWalletBody {
    wallet: String,
}

fn mockups_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env::var("MOCKUPS_DIR").unwrap_or_else(|_| "/data/mockups".into()))
}

/// Cloudflare R2 (S3-compatible) configuration. Active when all four envs
/// are present: R2_ENDPOINT, R2_BUCKET, R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY.
/// R2_PUBLIC_BASE defaults to https://mockups.wearmu.com.
struct R2Config {
    bucket: s3::Bucket,
    public_base: String,
}

fn r2_config() -> Option<R2Config> {
    let endpoint = env::var("R2_ENDPOINT").ok().filter(|s| !s.is_empty())?;
    let bucket_name = env::var("R2_BUCKET").ok().filter(|s| !s.is_empty())?;
    let access_key = env::var("R2_ACCESS_KEY_ID").ok().filter(|s| !s.is_empty())?;
    let secret_key = env::var("R2_SECRET_ACCESS_KEY").ok().filter(|s| !s.is_empty())?;
    let public_base = env::var("R2_PUBLIC_BASE")
        .unwrap_or_else(|_| "https://mockups.wearmu.com".into());
    let region = s3::Region::Custom { region: "auto".into(), endpoint };
    let creds = s3::creds::Credentials::new(
        Some(&access_key), Some(&secret_key), None, None, None,
    ).map_err(|e| eprintln!("[r2] credentials: {}", e)).ok()?;
    let bucket = s3::Bucket::new(&bucket_name, region, creds)
        .map_err(|e| eprintln!("[r2] bucket: {}", e)).ok()?
        .with_path_style();
    Some(R2Config { bucket, public_base })
}

/// Upload bytes to R2 if configured; otherwise write to local mockups dir.
/// Returns the URL (R2 public URL or `/mockups/<id>.jpg`) to store in DB.
async fn store_mockup_bytes(product_id: i64, bytes: &[u8]) -> Option<String> {
    let key = format!("{}.jpg", product_id);
    if let Some(cfg) = r2_config() {
        match cfg.bucket.put_object_with_content_type(&key, bytes, "image/jpeg").await {
            Ok(r) if r.status_code() == 200 => {
                return Some(format!("{}/{}", cfg.public_base.trim_end_matches('/'), key));
            }
            Ok(r) => {
                eprintln!("[r2] put_object {} status {}: {}", key, r.status_code(),
                          String::from_utf8_lossy(r.bytes()));
            }
            Err(e) => eprintln!("[r2] put_object {} error: {}", key, e),
        }
        // R2 configured but failed — don't silently fall back to local disk
        // (the local file would be inaccessible from the public DB URL anyway)
        return None;
    }
    // No R2 → fallback to Fly volume
    let dir = mockups_dir();
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        eprintln!("[mockups] create_dir_all failed: {}", e);
        return None;
    }
    let path = dir.join(&key);
    if let Err(e) = tokio::fs::write(&path, bytes).await {
        eprintln!("[mockups] write {} failed: {}", path.display(), e);
        return None;
    }
    Some(format!("/mockups/{}", key))
}

/// If the given URL is a Printful temporary upload (which expires), download
/// the bytes and persist them. Return the new permanent URL on success, or
/// None if the URL is already permanent / fetch failed.
async fn persist_mockup_if_temporary(product_id: i64, url: &str) -> Option<String> {
    let is_temp = url.starts_with("https://printful-upload.s3")
        || url.contains("/tmp/");
    if !is_temp {
        return None;
    }
    let bytes = match reqwest::get(url).await {
        Ok(r) if r.status().is_success() => match r.bytes().await {
            Ok(b) if !b.is_empty() => b,
            _ => return None,
        },
        _ => return None,
    };
    store_mockup_bytes(product_id, &bytes).await
}

/// Bonding-curve / progressive pricing.
/// Price starts at ¥5,000 (1st buyer) and steps up ¥250 per sold unit, capped at ¥30,000.
/// "Early buyer wins" — opposite of Dutch auction.
/// Special cases: MA starts at ¥30,000 (lowered from ¥120k on 2026-05-11
/// when MA moved from monthly to weekly 7-day auctions). MUGEN #108 = ¥30,000 fixed.
/// Price ceiling for the bonding curve. Final price (post-surcharge) is also
/// clamped to this value. Purchases at or above this threshold require KYC
/// (`KYC_THRESHOLD_JPY`).
const PRICE_CAP_JPY: i64 = 300_000;
const PRICE_BASE_JPY: i64 = 5_000;
const PRICE_STEP_JPY: i64 = 250;
const MUGEN_108_PRICE_JPY: i64 = 30_000;
/// MA auction starting bid. 2026-05-11: ¥120,000 → ¥30,000, monthly → weekly.
const MA_BASE_PRICE_JPY: i64 = 30_000;
/// MA auction duration in seconds. 2026-05-11: 30d → 7d.
/// Currently set by generate.py at row-insert time; this constant is the
/// single source of truth referenced by docs and admin tools.
#[allow(dead_code)]
const MA_AUCTION_DURATION_SECS: i64 = 7 * 24 * 60 * 60;
const KYC_THRESHOLD_JPY: i64 = 300_000;

fn dynamic_price(brand: &str, drop_num: i64, sold: i64, name: &str) -> i64 {
    if brand == "ma" {
        return MA_BASE_PRICE_JPY;
    }
    if brand == "nouns" {
        let nm = name.to_uppercase();
        if nm.contains("間") || nm.contains(" MA ") || nm.starts_with("MA ") || nm.ends_with(" MA") {
            return MA_BASE_PRICE_JPY;
        }
    }
    if brand == "mugen" && drop_num == 108 {
        return MUGEN_108_PRICE_JPY;
    }
    (PRICE_BASE_JPY + sold.max(0) * PRICE_STEP_JPY).min(PRICE_CAP_JPY)
}

/// Surcharge in basis points (1 bp = 0.01%) applied on top of the JPY price
/// for non-JPY payment methods. Covers processor fees, FX slip, oracle
/// volatility, and the additional accounting/KYC overhead.
fn payment_surcharge_bps(method: &str) -> i64 {
    match method.to_ascii_lowercase().as_str() {
        "eth" => 500,                                       // +5.0%
        "usdc" | "sol" | "solana" | "crypto" => 300,        // +3.0%
        "jpy" | "" => 0,
        _ => 0,                                             // unknown → safe default
    }
}

/// Apply the surcharge for the chosen payment method, then clamp to the
/// price cap. Result is rounded to the nearest yen.
fn apply_payment_surcharge(price_jpy: i64, method: &str) -> i64 {
    let bps = payment_surcharge_bps(method);
    if bps == 0 {
        return price_jpy.min(PRICE_CAP_JPY);
    }
    // Use 128-bit intermediate to be safe at extreme inputs.
    let surcharged = ((price_jpy as i128) * (10_000 + bps as i128) / 10_000) as i64;
    surcharged.min(PRICE_CAP_JPY)
}

#[cfg(test)]
mod price_tests {
    use super::*;

    #[test]
    fn cap_is_three_hundred_thousand() {
        // Far above the cap with normal step.
        let p = dynamic_price("mugen", 1, 10_000, "x");
        assert_eq!(p, PRICE_CAP_JPY);
        assert_eq!(p, 300_000);
    }

    #[test]
    fn surcharge_three_percent_for_crypto() {
        assert_eq!(apply_payment_surcharge(5_000, "usdc"), 5_150);
        assert_eq!(apply_payment_surcharge(5_000, "sol"), 5_150);
        assert_eq!(apply_payment_surcharge(120_000, "usdc"), 123_600);
    }

    #[test]
    fn surcharge_five_percent_for_eth() {
        assert_eq!(apply_payment_surcharge(5_000, "eth"), 5_250);
        assert_eq!(apply_payment_surcharge(120_000, "eth"), 126_000);
    }

    #[test]
    fn surcharge_clamps_to_cap() {
        // Base already at cap; ETH surcharge cannot push it past cap.
        assert_eq!(apply_payment_surcharge(PRICE_CAP_JPY, "eth"), PRICE_CAP_JPY);
        // Base just below cap; small surcharge pushed past would clamp.
        assert_eq!(apply_payment_surcharge(295_000, "eth"), PRICE_CAP_JPY);
    }

    #[test]
    fn jpy_is_passthrough() {
        assert_eq!(apply_payment_surcharge(5_000, "jpy"), 5_000);
        assert_eq!(apply_payment_surcharge(5_000, ""), 5_000);
    }
}

/// Normalize a `created_at` value (which may be a unix epoch string OR an ISO
/// timestamp like "2026-05-05T12:21:44.522054") into a consistent ISO-8601 UTC
/// string `"YYYY-MM-DDTHH:MM:SSZ"`. Lets clients sort/compare deterministically.
fn normalize_created_at_iso(raw: &str) -> String {
    if let Ok(secs) = raw.parse::<i64>() {
        if secs > 0 {
            let days = secs.div_euclid(86_400);
            let rem  = secs.rem_euclid(86_400);
            let (y, m, d) = civil_from_days(days);
            let hh = rem / 3600;
            let mm = (rem % 3600) / 60;
            let ss = rem % 60;
            return format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, hh, mm, ss);
        }
    }
    // Already ISO-ish — trim fractional seconds + ensure trailing Z.
    let trimmed = raw.split('.').next().unwrap_or(raw);
    if trimmed.contains('T') {
        return if trimmed.ends_with('Z') { trimmed.to_string() } else { format!("{trimmed}Z") };
    }
    raw.to_string()
}

fn read_product(row: &rusqlite::Row) -> rusqlite::Result<Product> {
    let brand:    String = row.get(1)?;
    let drop_num: i64    = row.get(2)?;
    let name:     String = row.get(3)?;
    let db_price: i64    = row.get(5)?;
    let sold:     i64    = row.get(7)?;
    let created_at_raw: String = row.get(8)?;
    // Pricing rule:
    //   - MA: respect the per-piece DB floor (so legacy monthly pieces created
    //     at ¥120k stay ¥120k even after the cadence change lowered the constant
    //     to ¥30k; admin update-price calls also persist correctly).
    //   - MUGEN/MUON/NOUNS: recompute from the bonding curve so each read
    //     reflects current `sold` count.
    let price_jpy = if brand == "ma" && db_price > 0 {
        db_price
    } else {
        dynamic_price(&brand, drop_num, sold, &name)
    };
    let serial_code = serial_code_for(&created_at_raw, drop_num);
    let created_at = normalize_created_at_iso(&created_at_raw);
    Ok(Product {
        id:           row.get(0)?,
        brand,
        drop_num,
        name,
        serial_code,
        mockup_url:   row.get(4)?,
        price_jpy,
        inventory:    row.get(6)?,
        sold,
        created_at,
        weather_data: row.get(9)?,
        prompt_hash:  row.get(10)?,
        seed_data:    row.get(11)?,
        nft_mint:     row.get(12)?,
        auction_end:  row.get(13)?,
        current_bid:  row.get(14).unwrap_or(0),
        bid_count:    row.get(15).unwrap_or(0),
        sold_out_at:  row.get(16).unwrap_or(None),
        lifestyle_url: row.get(17).unwrap_or(None),
    })
}

/// Build a YYYYMMDD-#NNN serial code from the row's created_at and a stable
/// per-day ordinal. We use drop_num modulo a per-day estimate when no explicit
/// per-day index is stored. For MUGEN at most 24 drops/day (one per hour), so
/// the ordinal is `((drop_num - 1) % 24) + 1`. For other brands we fall back
/// to drop_num itself since they're already 1-indexed within their cycle.
fn serial_code_for(created_at_raw: &str, drop_num: i64) -> String {
    // created_at is either a Unix epoch as TEXT or an ISO-ish stamp
    let unix_secs: i64 = if let Ok(v) = created_at_raw.parse::<i64>() {
        v
    } else {
        // Try "YYYY-MM-DDTHH:MM:SS..." — use only the date portion
        if let Some(date) = created_at_raw.split('T').next() {
            let parts: Vec<&str> = date.split('-').collect();
            if parts.len() == 3 {
                let y: i64 = parts[0].parse().unwrap_or(2026);
                let m: i64 = parts[1].parse().unwrap_or(1);
                let d: i64 = parts[2].parse().unwrap_or(1);
                return format!("{:04}{:02}{:02}-#{:03}", y, m, d, drop_num.max(1));
            }
        }
        0
    };
    if unix_secs <= 0 {
        return format!("00000000-#{:03}", drop_num.max(1));
    }
    // JST date = epoch + 9h, then break into Y/M/D
    let days_since_epoch = (unix_secs + 9 * 3600) / 86400;
    let (y, m, d) = civil_from_days(days_since_epoch);
    // Per-day ordinal: drop_num is 1-108 for mugen, mostly sequential by hour
    let ord = ((drop_num.max(1) - 1) % 99) + 1; // keep within #001-#099
    format!("{:04}{:02}{:02}-#{:03}", y, m, d, ord)
}

async fn list_products(
    Path(brand): Path<String>,
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let limit: i64 = q.get("limit").and_then(|s| s.parse().ok()).unwrap_or(500).clamp(1, 1000);
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        // Printful S3 temp URLs (printful-upload.s3-accelerate.amazonaws.com)
        // expire ~24h after upload. When that happens we fall back to the raw
        // design_url (stable imgur/R2 URL) so the image never disappears.
        "SELECT id, brand, drop_num, name,
                CASE
                  WHEN mockup_url LIKE 'https://printful-upload.s3%'
                       OR mockup_url LIKE 'https://files.cdn.printful.com/upload%'
                  THEN COALESCE(NULLIF(design_url,''), mockup_url)
                  ELSE mockup_url
                END AS mockup_url,
                price_jpy, inventory, sold, created_at,
                weather_data, prompt_hash, seed_data, nft_mint, auction_end,
                COALESCE(current_bid,0), COALESCE(bid_count,0), sold_out_at, lifestyle_url
         FROM products WHERE brand=? AND active=1 ORDER BY drop_num DESC LIMIT ?"
    ).unwrap();
    let products: Vec<Product> = stmt.query_map(params![brand, limit], |row| read_product(row))
        .unwrap().filter_map(|r| r.ok()).collect();
    Json(products)
}

async fn list_brands(State(db): State<Db>) -> impl IntoResponse {
    // created_at is stored mixed-format (some rows are unix-epoch strings,
    // others are ISO). Normalize inside SQL so MAX() picks the real latest.
    let counts: Vec<(String, i64, String)> = {
        let conn = db.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT brand, COUNT(*) AS active_count,
                    MAX(
                      CASE
                        WHEN created_at GLOB '[0-9]*' AND created_at NOT LIKE '%-%'
                          THEN strftime('%Y-%m-%dT%H:%M:%SZ', CAST(created_at AS INTEGER), 'unixepoch')
                        ELSE created_at
                      END
                    ) AS latest
             FROM products WHERE active=1 GROUP BY brand ORDER BY brand"
        ).unwrap();
        stmt.query_map([], |row| {
            let latest_raw: String = row.get::<_, Option<String>>(2)?.unwrap_or_default();
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, normalize_created_at_iso(&latest_raw)))
        }).unwrap().filter_map(|r| r.ok()).collect()
    };

    let brands_json: Vec<serde_json::Value> = counts.into_iter().map(|(b, c, latest)| {
        let (description, cycle) = match b.as_str() {
            "mugen" => ("108 pieces per hour, weather-driven design", "hourly"),
            "muon"  => ("daily drop, quantity from temperature", "daily"),
            "ma"    => ("weekly 7-day auction, single piece", "weekly"),
            "nouns" => ("MUON × Nouns DAO collaboration", "weekly"),
            _ => ("", ""),
        };
        serde_json::json!({
            "brand": b,
            "name": b.to_uppercase(),
            "description": description,
            "cycle": cycle,
            "active_drops": c,
            "latest_drop_at": latest,
            "list_endpoint": format!("/api/products/{}", b),
            "page_url": format!("https://wearmu.com/{}", b),
        })
    }).collect();

    Json(serde_json::json!({
        "brand_count": brands_json.len(),
        "brands": brands_json,
        "docs": "https://github.com/yukihamada/mu-brand",
        "endpoints": {
            "brand_list":   "/api/products",
            "brand_drops":  "/api/products/:brand",
            "product":      "/api/products/item/:id",
            "weather":      "/api/weather",
            "verify":       "/v/:brand/:drop_num",
        }
    }))
}

async fn get_product(
    Path(id): Path<i64>,
    State(db): State<Db>,
) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let result = conn.query_row(
        // Same fallback rule as list_products: if Printful temp URL has expired,
        // serve the stable design_url instead so the image never breaks.
        "SELECT id, brand, drop_num, name,
                CASE
                  WHEN mockup_url LIKE 'https://printful-upload.s3%'
                       OR mockup_url LIKE 'https://files.cdn.printful.com/upload%'
                  THEN COALESCE(NULLIF(design_url,''), mockup_url)
                  ELSE mockup_url
                END AS mockup_url,
                price_jpy, inventory, sold, created_at,
                weather_data, prompt_hash, seed_data, nft_mint, auction_end,
                COALESCE(current_bid,0), COALESCE(bid_count,0), sold_out_at, lifestyle_url
         FROM products WHERE id=? AND active=1",
        params![id], |row| read_product(row)
    );
    match result {
        Ok(p) => Json(p).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn weather_handler() -> impl IntoResponse {
    let w = tokio::task::spawn_blocking(|| {
        reqwest::blocking::get("https://wttr.in/Teshikaga?format=j1")
            .ok()
            .and_then(|r| r.json::<serde_json::Value>().ok())
    }).await.unwrap_or(None);

    let result = w.and_then(|d| {
        let c = d["current_condition"].get(0)?;
        Some(serde_json::json!({
            "temp_c":    c["temp_C"].as_str()?.parse::<i64>().ok()?,
            "humidity":  c["humidity"].as_str()?,
            "wind_kmh":  c["windspeedKmph"].as_str()?,
            "wind_dir":  c["winddir16Point"].as_str()?,
            "condition": c["weatherDesc"][0]["value"].as_str()?,
            "location":  "Teshikaga, Hokkaido",
        }))
    }).unwrap_or_else(|| serde_json::json!({
        "temp_c": null, "humidity": null, "wind_kmh": null,
        "wind_dir": null, "condition": "取得中", "location": "Teshikaga, Hokkaido"
    }));
    Json(result)
}

// ─────────────────────────────────────────────────────────────────────────
// MA Council — HMAC token + auto-enrollment helpers
// ─────────────────────────────────────────────────────────────────────────
/// HMAC-SHA256 of the lowercased email + COUNCIL_TOKEN_SECRET env var,
/// hex-encoded. Stable for a given (email, secret) pair so we don't need
/// to persist the token — the email itself is the secret material.
/// Returns None if COUNCIL_TOKEN_SECRET is unset (fail-closed).
fn council_token_for(email: &str) -> Option<String> {
    let secret = env::var("COUNCIL_TOKEN_SECRET").ok().filter(|s| !s.is_empty())?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).ok()?;
    mac.update(email.trim().to_lowercase().as_bytes());
    Some(hex::encode(mac.finalize().into_bytes()))
}

/// Reverse-lookup: given a token, find the matching member by recomputing
/// HMAC for every (small) member and comparing in constant time. With <1000
/// members this scans the whole table in <1ms. Returns (id, email, tier).
fn council_member_by_token(
    conn: &Connection, token: &str,
) -> Option<(i64, String, String)> {
    let mut stmt = conn.prepare(
        "SELECT id, email, tier FROM ma_council_members WHERE unsubscribed_at IS NULL"
    ).ok()?;
    let rows: Vec<(i64, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .ok()?.filter_map(|r| r.ok()).collect();
    let token_bytes = match hex::decode(token) {
        Ok(b) => b,
        Err(_) => return None,
    };
    let secret = env::var("COUNCIL_TOKEN_SECRET").ok().filter(|s| !s.is_empty())?;
    for (id, email, tier) in rows {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).ok()?;
        mac.update(email.trim().to_lowercase().as_bytes());
        if mac.verify_slice(&token_bytes).is_ok() {
            return Some((id, email, tier));
        }
    }
    None
}

/// Idempotently insert a Council member at the given tier. If the member
/// already exists, only upgrades trial→full (never downgrades). Returns
/// the (id, tier) on success.
fn council_enroll(
    conn: &Connection, email: &str, tier: &str, mu_piece_id: Option<i64>,
) -> Option<(i64, String)> {
    let email_lc = email.trim().to_lowercase();
    if email_lc.is_empty() || !email_lc.contains('@') { return None; }
    let now = chrono_now();
    let _ = conn.execute(
        "INSERT OR IGNORE INTO ma_council_members
            (email, tier, joined_at, mu_piece_id)
         VALUES (?,?,?,?)",
        params![email_lc, tier, now, mu_piece_id],
    );
    // Promote trial → full if requested. Never demote full → trial.
    if tier == "full" {
        let _ = conn.execute(
            "UPDATE ma_council_members
             SET tier='full',
                 mu_piece_id=COALESCE(?, mu_piece_id)
             WHERE email=? AND tier='trial'",
            params![mu_piece_id, email_lc],
        );
    }
    conn.query_row(
        "SELECT id, tier FROM ma_council_members WHERE email=?",
        params![email_lc],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
    ).ok()
}

#[cfg(test)]
mod council_token_tests {
    use super::*;
    use std::sync::Mutex;
    // Serialize tests that mutate the shared env var COUNCIL_TOKEN_SECRET.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn token_roundtrip_via_env_secret() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("COUNCIL_TOKEN_SECRET", "test-secret-please-rotate");
        let t1 = council_token_for("Alice@Example.com").expect("token");
        let t2 = council_token_for("alice@example.com").expect("token2");
        assert_eq!(t1, t2, "token should be case-insensitive on email");
        // 32 bytes hex = 64 chars
        assert_eq!(t1.len(), 64);
        // Distinct emails produce distinct tokens
        let other = council_token_for("bob@example.com").expect("other");
        assert_ne!(t1, other);
        std::env::remove_var("COUNCIL_TOKEN_SECRET");
    }

    #[test]
    fn token_returns_none_without_secret() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("COUNCIL_TOKEN_SECRET");
        assert!(council_token_for("anyone@example.com").is_none());
    }
}

async fn place_bid(
    State(db): State<Db>,
    Json(body): Json<BidBody>,
) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let row = conn.query_row(
        "SELECT price_jpy, current_bid, auction_end FROM products WHERE id=? AND active=1 AND brand='ma'",
        params![body.product_id],
        |row| Ok((row.get::<_,i64>(0)?, row.get::<_,i64>(1).unwrap_or(0), row.get::<_,Option<String>>(2)?))
    );
    let (base_price, current_bid, _auction_end) = match row {
        Ok(r) => r,
        Err(_) => return (StatusCode::NOT_FOUND, "not found").into_response(),
    };
    let min_bid = current_bid.max(base_price) + 1000;
    if body.amount < min_bid {
        return (StatusCode::BAD_REQUEST,
            format!("最低入札額は¥{}です", min_bid)).into_response();
    }

    // KYC gate for high-value bids — settlement at ¥300k+ would require it
    // anyway, so catch at bid time to avoid unverified high bids stuck in
    // limbo at auction settlement.
    if body.amount >= KYC_THRESHOLD_JPY {
        let Some(kyc) = body.kyc.as_ref() else {
            return (StatusCode::BAD_REQUEST,
                "KYC required for bids at or above ¥300,000").into_response();
        };
        if kyc.full_name.trim().is_empty()
            || kyc.date_of_birth.trim().is_empty()
            || kyc.nationality.trim().is_empty()
            || kyc.id_type.trim().is_empty()
            || kyc.id_last4.trim().is_empty()
            || kyc.address.trim().is_empty()
            || kyc.consent_at.trim().is_empty()
        {
            return (StatusCode::BAD_REQUEST,
                "KYC required for bids at or above ¥300,000 (incomplete fields)").into_response();
        }
        let _ = conn.execute(
            "INSERT INTO kyc_records
             (product_id, email, full_name, dob, nationality, id_type, id_last4,
              address, consent_at, payment_method, total_amount_jpy, created_at)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                body.product_id, body.email,
                kyc.full_name.trim(), kyc.date_of_birth.trim(),
                kyc.nationality.trim(), kyc.id_type.trim(), kyc.id_last4.trim(),
                kyc.address.trim(), kyc.consent_at.trim(),
                "jpy", body.amount, chrono_now()
            ]
        );
    }

    let now = chrono_now();
    let wallet_token = uuid::Uuid::new_v4().to_string();
    // Soulbound NFT opt-in: prefer the dedicated `nft_wallet` (entered in
    // the modal's NFT checkbox row); fall back to `wallet` (legacy field).
    let nft_wallet = body.nft_wallet.clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| body.wallet.clone().filter(|s| !s.trim().is_empty()));
    let nft_opt_in_flag: i64 = if body.nft_opt_in && nft_wallet.is_some() { 1 } else { 0 };
    conn.execute(
        "INSERT INTO bids
            (product_id, amount, email, wallet, wallet_token, created_at, nft_opt_in, nft_wallet)
         VALUES (?,?,?,?,?,?,?,?)",
        params![
            body.product_id, body.amount, body.email, body.wallet, wallet_token, now,
            nft_opt_in_flag, nft_wallet
        ]
    ).unwrap();
    conn.execute(
        "UPDATE products SET current_bid=?, bid_count=bid_count+1 WHERE id=?",
        params![body.amount, body.product_id]
    ).unwrap();
    // MA Council trial-tier auto-enrollment. Anyone who places a bid joins
    // the council in trial tier; full membership requires a winning settlement.
    let _ = council_enroll(&conn, &body.email, "trial", None);
    let council_token = council_token_for(&body.email);
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    Json(serde_json::json!({
        "ok": true,
        "wallet_token": wallet_token,
        "wallet_url": format!("{}/wallet/{}", base_url, wallet_token),
        "council_token": council_token,
        "council_url": council_token.as_ref()
            .map(|t| format!("{}/council?token={}", base_url, t)),
    })).into_response()
}

async fn checkout(
    State(db): State<Db>,
    Json(body): Json<CheckoutBody>,
) -> impl IntoResponse {
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();

    let check = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT brand, drop_num, inventory, sold, name FROM products WHERE id=? AND active=1",
            params![body.product_id],
            |row| Ok((
                row.get::<_,String>(0)?, row.get::<_,i64>(1)?,
                row.get::<_,i64>(2)?, row.get::<_,i64>(3)?,
                row.get::<_,String>(4)?
            ))
        )
    };
    let (brand_str, drop_num, inventory, sold, product_name) = match check {
        Ok(r) => r,
        Err(_) => return (StatusCode::NOT_FOUND, "product not found").into_response(),
    };
    if inventory - sold < body.quantity as i64 {
        return (StatusCode::CONFLICT, "sold out").into_response();
    }
    // Reverse Dutch: compute current price from sold count at checkout time.
    let base_price_jpy = dynamic_price(&brand_str, drop_num, sold, &product_name);

    let payment_method = body.payment_method.clone().unwrap_or_else(|| "jpy".into());
    let pm = payment_method.to_ascii_lowercase();
    let price_jpy = apply_payment_surcharge(base_price_jpy, &pm);
    let total_jpy = price_jpy.saturating_mul(body.quantity as i64);

    // KYC gate: any single transaction at or above ¥300,000 (final billed total
    // including surcharge) requires the customer to submit identification.
    // Records are written to the `kyc_records` table for AML hygiene; we do
    // not run live ID verification here — that's a Stripe Identity / external
    // step. This gate just makes the data collection mandatory.
    if total_jpy >= KYC_THRESHOLD_JPY {
        let Some(kyc) = body.kyc.as_ref() else {
            return (StatusCode::BAD_REQUEST,
                "KYC required for purchases at or above ¥300,000 (kyc field missing in body)")
                .into_response();
        };
        if kyc.full_name.trim().is_empty()
            || kyc.date_of_birth.trim().is_empty()
            || kyc.nationality.trim().is_empty()
            || kyc.id_type.trim().is_empty()
            || kyc.id_last4.trim().is_empty()
            || kyc.address.trim().is_empty()
            || kyc.consent_at.trim().is_empty()
        {
            return (StatusCode::BAD_REQUEST,
                "KYC required for purchases at or above ¥300,000 (incomplete kyc fields)")
                .into_response();
        }
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO kyc_records
             (product_id, email, full_name, dob, nationality, id_type, id_last4,
              address, consent_at, payment_method, total_amount_jpy, created_at)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                body.product_id, body.email,
                kyc.full_name.trim(), kyc.date_of_birth.trim(),
                kyc.nationality.trim(), kyc.id_type.trim(), kyc.id_last4.trim(),
                kyc.address.trim(), kyc.consent_at.trim(),
                pm, total_jpy, chrono_now()
            ]
        );
    }

    // Crypto payment methods are recognised at the pricing layer (surcharge
    // applied above) but the actual on-chain settlement flow is staged in a
    // separate endpoint. For now, only JPY (Stripe) checkout is wired through.
    if pm != "jpy" {
        return (StatusCode::NOT_IMPLEMENTED, format!(
            "Crypto checkout for '{}' is not yet wired through. \
             Surcharged unit price would be ¥{} (total ¥{}). \
             Use payment_method=\"jpy\" for now.",
            pm, price_jpy, total_jpy
        )).into_response();
    }

    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3000".into());
    let size_label = body.size.clone().unwrap_or_else(|| "M".into());
    let display_name = format!("{} ({})", product_name, size_label);

    let wallet = body.wallet.clone().unwrap_or_default();
    let client = reqwest::Client::new();
    let session_resp = client
        .post("https://api.stripe.com/v1/checkout/sessions")
        .basic_auth(&stripe_key, None::<&str>)
        .form(&[
            ("mode", "payment"),
            ("currency", "jpy"),
            ("line_items[0][price_data][currency]", "jpy"),
            ("line_items[0][price_data][product_data][name]", &display_name),
            ("line_items[0][price_data][unit_amount]", &price_jpy.to_string()),
            ("line_items[0][quantity]", &body.quantity.to_string()),
            ("success_url", &format!("{}/success?sid={{CHECKOUT_SESSION_ID}}", base_url)),
            ("cancel_url", &format!("{}/", base_url)),
            ("customer_email", &body.email),
            ("shipping_address_collection[allowed_countries][0]", "JP"),
            ("shipping_address_collection[allowed_countries][1]", "US"),
            ("shipping_address_collection[allowed_countries][2]", "GB"),
            ("shipping_address_collection[allowed_countries][3]", "FR"),
            ("shipping_address_collection[allowed_countries][4]", "DE"),
            ("shipping_address_collection[allowed_countries][5]", "AU"),
            ("shipping_address_collection[allowed_countries][6]", "KR"),
            ("shipping_address_collection[allowed_countries][7]", "TW"),
            ("shipping_address_collection[allowed_countries][8]", "HK"),
            ("allow_promotion_codes", "true"),
            ("metadata[product_id]",       &body.product_id.to_string()),
            ("metadata[size]",             &size_label),
            ("metadata[wallet]",           &wallet),
            ("metadata[payment_method]",   &pm),
            ("metadata[base_price_jpy]",   &base_price_jpy.to_string()),
            ("metadata[unit_price_jpy]",   &price_jpy.to_string()),
            ("metadata[total_price_jpy]",  &total_jpy.to_string()),
            ("metadata[kyc_required]",     if total_jpy >= KYC_THRESHOLD_JPY { "true" } else { "false" }),
        ])
        .send().await;

    match session_resp {
        Ok(r) if r.status().is_success() => {
            let json: serde_json::Value = r.json().await.unwrap();
            let url = json["url"].as_str().unwrap_or("/");
            Json(serde_json::json!({"url": url})).into_response()
        }
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            eprintln!("Stripe error {}: {}", status, body);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("stripe error: {}", &body[..body.len().min(200)])).into_response()
        }
        Err(e) => {
            eprintln!("Stripe request error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "stripe connection error").into_response()
        }
    }
}

async fn stripe_webhook(
    State(db): State<Db>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    // ── Signature verification (fail-closed) ──
    // Reject all webhooks if STRIPE_WEBHOOK_SECRET is not configured —
    // never accept unsigned webhooks even in dev.
    let secret = match env::var("STRIPE_WEBHOOK_SECRET") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!("[security] STRIPE_WEBHOOK_SECRET unset — rejecting webhook");
            return (StatusCode::SERVICE_UNAVAILABLE,
                "webhook disabled (server misconfigured)").into_response();
        }
    };
    let sig_header = headers.get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let timestamp_str = sig_header.split(',')
        .find(|s| s.starts_with("t="))
        .and_then(|s| s.strip_prefix("t="))
        .unwrap_or("");
    let provided_sig = sig_header.split(',')
        .find(|s| s.starts_with("v1="))
        .and_then(|s| s.strip_prefix("v1="))
        .unwrap_or("");

    // Replay protection: reject events older than 5 minutes.
    let ts: u64 = timestamp_str.parse().unwrap_or(0);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if ts == 0 || now.saturating_sub(ts) > 300 {
        eprintln!("[security] Stripe webhook timestamp out of tolerance (ts={}, now={})", ts, now);
        return (StatusCode::UNAUTHORIZED, "stale webhook").into_response();
    }

    // Constant-time HMAC verification via Mac::verify_slice.
    let signed_payload = format!("{}.{}", timestamp_str, body);
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC init");
    mac.update(signed_payload.as_bytes());
    let provided_bytes = match hex::decode(provided_sig) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("[security] Stripe webhook bad signature hex");
            return StatusCode::UNAUTHORIZED.into_response();
        }
    };
    if mac.verify_slice(&provided_bytes).is_err() {
        eprintln!("[security] Stripe webhook signature mismatch");
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let event: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    // /you ¥980/月 subscription lifecycle (created / updated / deleted).
    let ev_type = event["type"].as_str().unwrap_or("");
    if ev_type.starts_with("customer.subscription.") {
        handle_subscription_event(&db, ev_type, &event);
        return StatusCode::OK.into_response();
    }
    if ev_type == "invoice.paid" || ev_type == "invoice.payment_succeeded" {
        // Period-end advance — re-read the subscription via id on the invoice.
        let sub_id = event["data"]["object"]["subscription"].as_str().unwrap_or("").to_string();
        let customer_id = event["data"]["object"]["customer"].as_str().unwrap_or("").to_string();
        let period_end: i64 = event["data"]["object"]["lines"]["data"][0]["period"]["end"]
            .as_i64().unwrap_or(0);
        if !sub_id.is_empty() && period_end > 0 {
            let conn = db.lock().unwrap();
            let _ = conn.execute(
                "UPDATE you_users
                 SET subscription_until=?, subscription_status='active'
                 WHERE stripe_subscription_id=? OR stripe_customer_id=?",
                params![period_end.to_string(), sub_id, customer_id],
            );
        }
        return StatusCode::OK.into_response();
    }

    if ev_type == "checkout.session.completed" {
        let session = &event["data"]["object"];
        let meta = session["metadata"].clone();

        // ── Collab order (MU × SWEEP etc.) ──
        // Records a row in collab_orders. Production route:
        //   - printful → POST to Printful /v2/orders (auto-fulfill)
        //   - sweep_manual / pre_order → Telegram alert; SWEEP社 が個別対応
        if meta["collab"].as_str() == Some("sweep") {
            handle_collab_sweep_order(db.clone(), &session).await;
            return StatusCode::OK.into_response();
        }

        // 3-month prepaid pack (mode=payment, metadata.plan=3mo): extend
        // subscription_until by 90 days. Idempotent on session id.
        if meta["plan"].as_str() == Some("3mo") {
            if let Some(uid_str) = meta["you_user_id"].as_str() {
                let user_id: i64 = uid_str.parse().unwrap_or(0);
                if user_id > 0 {
                    let now_secs: i64 = chrono_now().parse().unwrap_or(0);
                    let conn = db.lock().unwrap();
                    let current_end: i64 = conn.query_row(
                        "SELECT COALESCE(CAST(subscription_until AS INTEGER), 0)
                         FROM you_users WHERE id=?",
                        params![user_id], |r| r.get(0),
                    ).unwrap_or(0);
                    let base = current_end.max(now_secs);
                    let new_end = base + 90 * 86_400;
                    let _ = conn.execute(
                        "UPDATE you_users
                         SET subscription_status='active',
                             subscription_until=?
                         WHERE id=?",
                        params![new_end.to_string(), user_id],
                    );
                }
            }
            return StatusCode::OK.into_response();
        }
        // ¥980/月 subscription Checkout completed (mode=subscription).
        // The user_id is in metadata.you_user_id; record customer + sub.
        if session["mode"].as_str() == Some("subscription") {
            if let Some(uid_str) = meta["you_user_id"].as_str() {
                let user_id: i64 = uid_str.parse().unwrap_or(0);
                let customer_id = session["customer"].as_str().unwrap_or("").to_string();
                let sub_id = session["subscription"].as_str().unwrap_or("").to_string();
                if user_id > 0 {
                    let conn = db.lock().unwrap();
                    let _ = conn.execute(
                        "UPDATE you_users
                         SET stripe_customer_id=?, stripe_subscription_id=?,
                             subscription_status='active'
                         WHERE id=?",
                        params![customer_id, sub_id, user_id],
                    );
                }
            }
            return StatusCode::OK.into_response();
        }
        // /you design purchase path (you_claim or you_public_buy):
        // separate from MU drops because /you designs live in you_designs.
        if let Some(design_id_str) = meta["you_design_id"].as_str() {
            if let Ok(design_id) = design_id_str.parse::<i64>() {
                handle_you_purchase_webhook(db.clone(), design_id, session.clone()).await;
                return StatusCode::OK.into_response();
            }
        }
        let product_id: i64 = meta["product_id"].as_str()
            .and_then(|s| s.parse().ok()).unwrap_or(0);
        if product_id == 0 {
            eprintln!("Stripe webhook: missing product_id in metadata");
            return StatusCode::OK.into_response();
        }
        let just_sold_out = {
            let conn = db.lock().unwrap();
            conn.execute("UPDATE products SET sold=sold+1 WHERE id=?", params![product_id]).ok();
            // Check if just sold out
            let (inv, sold_new) = conn.query_row(
                "SELECT inventory, sold FROM products WHERE id=?",
                params![product_id], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,i64>(1)?))
            ).unwrap_or((0,0));
            if sold_new >= inv && inv > 0 {
                let now_str = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs().to_string();
                conn.execute("UPDATE products SET sold_out_at=? WHERE id=? AND sold_out_at IS NULL", params![now_str, product_id]).ok();
                true
            } else {
                false
            }
        };

        // Record the purchase + grant /you lifetime_free. Idempotent on
        // session_id (Stripe re-delivers the same event sometimes).
        {
            let buyer_email = session["customer_details"]["email"]
                .as_str()
                .or_else(|| session["customer_email"].as_str())
                .unwrap_or("")
                .to_lowercase();
            let session_id = session["id"].as_str().unwrap_or("").to_string();
            let conn = db.lock().unwrap();
            let (brand, drop_num): (String, i64) = conn.query_row(
                "SELECT brand, drop_num FROM products WHERE id=?",
                params![product_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            ).unwrap_or((String::new(), 0));
            if !buyer_email.is_empty() {
                conn.execute(
                    "INSERT OR IGNORE INTO mu_purchases (email, product_id, brand, drop_num, session_id, created_at)
                     SELECT ?, ?, ?, ?, ?, ?
                     WHERE NOT EXISTS (
                       SELECT 1 FROM mu_purchases WHERE session_id=? AND product_id=?
                     )",
                    params![buyer_email, product_id, brand, drop_num, session_id, chrono_now(), session_id, product_id],
                ).ok();
                let reason = format!("purchased {} #{}", brand.to_uppercase(), drop_num);
                let updated = conn.execute(
                    "UPDATE you_users
                     SET lifetime_free=1, lifetime_reason=COALESCE(lifetime_reason, ?)
                     WHERE email=? AND lifetime_free=0",
                    params![reason, buyer_email],
                ).unwrap_or(0);
                if updated > 0 {
                    eprintln!("[/you] granted lifetime_free to {} ({})", buyer_email, reason);
                }
                // Referral credit: if the new lifetime member was referred,
                // credit the inviter ¥3,400 (one-shot per referee). The
                // credit accumulates on you_users.referral_credit_jpy and
                // can be redeemed via the existing coupon flow.
                let ref_slug: Option<String> = conn.query_row(
                    "SELECT referred_by_slug FROM you_users WHERE email=?",
                    params![buyer_email],
                    |r| r.get::<_, Option<String>>(0),
                ).ok().flatten();
                if let Some(slug) = ref_slug {
                    let credited = conn.execute(
                        "UPDATE you_users
                         SET referral_credit_jpy = referral_credit_jpy + 3400,
                             referral_count      = referral_count + 1
                         WHERE slug = ?",
                        params![slug],
                    ).unwrap_or(0);
                    if credited > 0 {
                        eprintln!("[referral] credited {} +¥3,400 (referee: {})", slug, buyer_email);
                    }
                }
            }
        }
        if just_sold_out {
            let buyer_email = session["customer_details"]["email"].as_str().unwrap_or("").to_string();
            let product_name = {
                let conn = db.lock().unwrap();
                conn.query_row(
                    "SELECT name FROM products WHERE id=?",
                    params![product_id],
                    |r| r.get::<_,String>(0)
                ).unwrap_or_default()
            };
            let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
            if !buyer_email.is_empty() && !resend_key.is_empty() {
                let buyer_email2 = buyer_email.clone();
                let product_name2 = product_name.clone();
                tokio::spawn(async move {
                    let html = format!(
                        r#"<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:48px;max-width:560px">
  <div style="font-size:22px;font-weight:700;letter-spacing:0.4em;margin-bottom:32px">MU</div>
  <div style="font-size:9px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.65;margin-bottom:12px">LAST PIECE</div>
  <div style="font-size:20px;font-weight:300;margin-bottom:20px">{} — あなたが最後の1着を手に入れた。</div>
  <p style="font-size:12px;opacity:0.5;line-height:1.9">このドロップはあなたで閉じた。<br>二度と同じものは生まれない。</p>
</div>"#,
                        product_name2
                    );
                    reqwest::Client::new()
                        .post("https://api.resend.com/emails")
                        .bearer_auth(&resend_key)
                        .json(&serde_json::json!({
                            "from": "MU <noreply@wearmu.com>",
                            "to": [&buyer_email2],
                            "subject": "あなたがこのドロップを閉じた — MU LAST PIECE",
                            "html": html,
                        }))
                        .send().await.ok();
                });
            }
        }
        // ── Soulbound NFT pilot trigger (Stripe path) ──
        // Buyer opts in by entering a Solana wallet at checkout (modal form
        // → `metadata.wallet` on the Stripe session). Dry-run by default;
        // see store/src/nft.rs and `MU_NFT_MINT_LIVE`.
        let buyer_wallet = meta["wallet"].as_str().unwrap_or("").trim().to_string();
        if !buyer_wallet.is_empty() {
            nft::mint_soulbound_bg(db.clone(), product_id, buyer_wallet, "stripe_webhook");
        }

        let printful_key = env::var("PRINTFUL_API_KEY").unwrap_or_default();
        let db2 = db.clone();
        let session_clone = session.clone();
        tokio::spawn(async move {
            create_printful_order(printful_key, db2, product_id, session_clone).await;
        });
    }
    StatusCode::OK.into_response()
}

/// /you design fulfillment: mark claimed, alert ops, confirm to buyer.
/// Printful auto-fulfillment for /you designs is a follow-up (the design
/// bytes live as a BLOB in SQLite, not on Imgur, so we need an extra
/// step to push them through Printful's Files API).
async fn handle_you_purchase_webhook(db: Db, design_id: i64, session: serde_json::Value) {
    let buyer_email = session["customer_details"]["email"]
        .as_str().or_else(|| session["customer_email"].as_str())
        .unwrap_or("").to_string();
    let amount: i64 = session["amount_total"].as_i64().unwrap_or(0);
    let session_id = session["id"].as_str().unwrap_or("").to_string();
    let serial = session["metadata"]["you_serial"].as_str().unwrap_or("").to_string();
    let size = session["metadata"]["you_size"].as_str().unwrap_or("S").to_string();
    let owner_slug = session["metadata"]["you_owner_slug"].as_str().unwrap_or("anon").to_string();
    let public_buy = session["metadata"]["you_public_buy"].as_str() == Some("1");

    // Mark the design claimed (idempotent on session_id-already-recorded).
    let (design_name, day_num, owner_email) = {
        let conn = db.lock().unwrap();
        // record under the buyer's email so retro lifetime_free works (a
        // /you-design buyer is also "owns a MU shirt" in spirit)
        if !buyer_email.is_empty() {
            conn.execute(
                "INSERT OR IGNORE INTO mu_purchases (email, product_id, brand, drop_num, session_id, created_at)
                 SELECT ?, ?, 'you', ?, ?, ?
                 WHERE NOT EXISTS (SELECT 1 FROM mu_purchases WHERE session_id=?)",
                params![
                    buyer_email.to_lowercase(), design_id,
                    session["metadata"]["you_day_num"].as_str().and_then(|s|s.parse::<i64>().ok()).unwrap_or(0),
                    session_id, chrono_now(), session_id,
                ],
            ).ok();
            let reason = format!("purchased /you design YOU#{:04} from @{}", design_id, owner_slug);
            conn.execute(
                "UPDATE you_users SET lifetime_free=1, lifetime_reason=COALESCE(lifetime_reason, ?)
                 WHERE email=? AND lifetime_free=0",
                params![reason, buyer_email.to_lowercase()],
            ).ok();
        }
        conn.execute(
            "UPDATE you_designs SET status='claimed', updated_at=? WHERE id=?",
            params![chrono_now(), design_id],
        ).ok();
        let row: Option<(String, i64, String)> = conn.query_row(
            "SELECT d.name, d.day_num, u.email FROM you_designs d JOIN you_users u ON u.id = d.user_id WHERE d.id=?",
            params![design_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).ok();
        row.unwrap_or((String::new(), 0, String::new()))
    };

    // Notify ops (yuki) so the order can be hand-fulfilled via Printful UI
    // until the design-bytes-to-Printful-files automation lands.
    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    if !resend_key.is_empty() {
        let buyer = buyer_email.clone();
        let name = design_name.clone();
        let owner_slug2 = owner_slug.clone();
        let owner_email2 = owner_email.clone();
        let serial2 = serial.clone();
        let size2 = size.clone();
        let resend_key_ops = resend_key.clone();
        tokio::spawn(async move {
            let html = format!(
                r#"<div style="font-family:monospace;font-size:13px;line-height:1.7;background:#0A0A0A;color:#F5F5F0;padding:32px">
<h2 style="color:#e6c449">/you purchase — needs fulfillment</h2>
<table>
<tr><td>design id</td><td>{design_id}</td></tr>
<tr><td>serial</td><td>{serial}</td></tr>
<tr><td>name</td><td>{name}</td></tr>
<tr><td>day_num</td><td>{day_num}</td></tr>
<tr><td>size</td><td>{size}</td></tr>
<tr><td>amount</td><td>¥{amount}</td></tr>
<tr><td>buyer</td><td>{buyer}</td></tr>
<tr><td>owner</td><td>@{owner} ({owner_email})</td></tr>
<tr><td>public buy</td><td>{public}</td></tr>
<tr><td>session</td><td>{session_id}</td></tr>
<tr><td>image</td><td><a href="https://wearmu.com/api/you/design/{design_id}/image.png" style="color:#e6c449">design PNG</a></td></tr>
</table>
<p>Action: download the image, upload to Printful, place order with the buyer's shipping address (in Stripe dashboard).</p>
</div>"#,
                design_id = design_id, serial = serial2, name = name,
                day_num = day_num, size = size2, amount = amount,
                buyer = buyer, owner = owner_slug2, owner_email = owner_email2,
                public = public_buy, session_id = session_id,
            );
            let _ = reqwest::Client::new()
                .post("https://api.resend.com/emails")
                .bearer_auth(&resend_key_ops)
                .json(&serde_json::json!({
                    "from": "MU ops <noreply@wearmu.com>",
                    "to": ["mail@yukihamada.jp"],
                    "subject": format!("[fulfill] /you {} — ¥{} from {}", serial2, amount, buyer),
                    "html": html,
                }))
                .send().await;
        });
    }
    // Buyer confirmation
    if !buyer_email.is_empty() && !resend_key.is_empty() {
        let buyer = buyer_email.clone();
        let html = format!(
            r#"<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:48px;max-width:560px;margin:0 auto">
  <div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:32px">MU × YOU</div>
  <div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#e6c449;opacity:0.85;margin-bottom:8px">Thank you</div>
  <div style="font-size:18px;font-weight:300;line-height:1.5;margin-bottom:24px">この一着を選んでくれてありがとう。</div>
  <div style="background:#1C1C1C;padding:18px;margin-bottom:24px">
    <div style="font-size:9px;letter-spacing:0.2em;text-transform:uppercase;opacity:0.65;margin-bottom:8px">仕立てる一着</div>
    <div style="font-size:15px;margin-bottom:6px">{name}</div>
    <div style="font-size:11px;opacity:0.7">Serial {serial} · Size {size} · ¥{amount}</div>
    <div style="font-size:11px;opacity:0.5;margin-top:6px">designed by @{owner}</div>
  </div>
  <p style="font-size:12px;line-height:1.85;opacity:0.75;margin-bottom:24px">
    7〜14 営業日で世界配送。Printful より発送します。<br>
    NFT 証明書（Soulbound）は発送後にお送りします。<br><br>
    あなたは MU の所有者です。<a href="https://wearmu.com/you" style="color:#e6c449">MU × YOU</a> は今日からあなたにとって <strong>一生無料</strong> です。
  </p>
</div>"#,
            name = design_name, serial = serial, size = size,
            amount = amount, owner = owner_slug,
        );
        tokio::spawn(async move {
            let _ = reqwest::Client::new()
                .post("https://api.resend.com/emails")
                .bearer_auth(&resend_key)
                .json(&serde_json::json!({
                    "from": "MU × YOU <noreply@wearmu.com>",
                    "to": [buyer],
                    "subject": format!("MU × YOU — {} があなたの元へ", serial),
                    "html": html,
                }))
                .send().await;
        });
    }
}

async fn create_printful_order(key: String, db: Db, product_id: i64, session: serde_json::Value) {
    let (design_url, mockup_url) = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT design_url, mockup_url FROM products WHERE id=?",
            params![product_id],
            |row| Ok((row.get::<_,Option<String>>(0)?, row.get::<_,Option<String>>(1)?))
        ).unwrap_or((None, None))
    };

    // Prefer design_url (raw artwork) but fall back to mockup_url if missing
    let design_url = match design_url.filter(|u| !u.is_empty())
        .or_else(|| mockup_url.filter(|u| !u.is_empty())) {
        Some(u) => u,
        None => {
            eprintln!("Printful: no design or mockup url for product {}", product_id);
            return;
        }
    };

    let stripe_key = std::env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    let session_id = session["id"].as_str().unwrap_or("").to_string();

    // Re-fetch with shipping_details expanded
    let full_session: serde_json::Value = if !session_id.is_empty() && !stripe_key.is_empty() {
        let resp = reqwest::Client::new()
            .get(format!("https://api.stripe.com/v1/checkout/sessions/{}", session_id))
            .query(&[("expand[]", "shipping_details")])
            .basic_auth(&stripe_key, None::<&str>)
            .send().await;
        match resp {
            Ok(r) if r.status().is_success() => r.json().await.unwrap_or(session.clone()),
            _ => session.clone(),
        }
    } else { session.clone() };

    // Stripe puts shipping address in shipping_details, not metadata
    let shipping = &full_session["shipping_details"];
    let addr = &shipping["address"];
    let name = shipping["name"].as_str().unwrap_or("");
    let address1 = addr["line1"].as_str().unwrap_or("");
    let address2 = addr["line2"].as_str().unwrap_or("");
    let city = addr["city"].as_str().unwrap_or("");
    let country_code = addr["country"].as_str().unwrap_or("JP");
    let zip = addr["postal_code"].as_str().unwrap_or("");
    let state = addr["state"].as_str().unwrap_or("");

    // Determine variant by size from metadata
    let size = full_session["metadata"]["size"].as_str().unwrap_or("M");
    let variant_id: u64 = match size {
        "S"  => 4016,
        "M"  => 4017,
        "L"  => 4018,
        "XL" => 4019,
        _    => 4017,
    };

    let client = reqwest::Client::new();
    let order = serde_json::json!({
        "recipient": {
            "name":         name,
            "address1":     address1,
            "address2":     address2,
            "city":         city,
            "state_code":   state,
            "country_code": country_code,
            "zip":          zip,
        },
        "items": [{
            "variant_id": variant_id,
            "quantity": 1,
            "files": [{"url": design_url, "placement": "front"}],
        }],
        "confirm": true,
    });
    let resp = client.post("https://api.printful.com/orders")
        .bearer_auth(&key).json(&order).send().await;
    match resp {
        Ok(r) if r.status().is_success() => {
            eprintln!("Printful order created for product {}", product_id);
        }
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            eprintln!("Printful error {}: {}", status, body);
        }
        Err(e) => eprintln!("Printful request error: {}", e),
    }
}

/// MU × SWEEP collab order webhook handler.
/// Idempotent on stripe_session. Three production routes:
///   - 'printful'      → POST draft order to Printful API (SWEEP社 approves in dashboard)
///   - 'sweep_manual'  → Telegram + Resend ops alert; SWEEP社 produces by hand
///   - 'pre_order'     → Telegram alert; ops contacts the buyer (sizing / consult)
async fn handle_collab_sweep_order(db: Db, session: &serde_json::Value) {
    let session_id = session["id"].as_str().unwrap_or("").to_string();
    let slug = session["metadata"]["slug"].as_str().unwrap_or("").to_string();
    let size = session["metadata"]["size"].as_str().unwrap_or("M").to_string();
    let amount: i64 = session["amount_total"].as_i64().unwrap_or(0);
    let email = session["customer_details"]["email"].as_str()
        .or_else(|| session["customer_email"].as_str())
        .unwrap_or("").to_string();

    // Re-fetch with shipping_details expanded
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    let full_session: serde_json::Value = if !session_id.is_empty() && !stripe_key.is_empty() {
        let resp = reqwest::Client::new()
            .get(format!("https://api.stripe.com/v1/checkout/sessions/{}", session_id))
            .query(&[("expand[]", "shipping_details")])
            .basic_auth(&stripe_key, None::<&str>)
            .send().await;
        match resp {
            Ok(r) if r.status().is_success() => r.json().await.unwrap_or(session.clone()),
            _ => session.clone(),
        }
    } else { session.clone() };
    let shipping = &full_session["shipping_details"];
    let addr = &shipping["address"];
    let ship_name = shipping["name"].as_str().unwrap_or("").to_string();
    let address1 = addr["line1"].as_str().unwrap_or("");
    let address2 = addr["line2"].as_str().unwrap_or("");
    let city = addr["city"].as_str().unwrap_or("");
    let country = addr["country"].as_str().unwrap_or("JP").to_string();
    let zip = addr["postal_code"].as_str().unwrap_or("");
    let state = addr["state"].as_str().unwrap_or("");
    let ship_address = format!("{} {} {} {} {} {}", address1, address2, city, state, zip, country)
        .split_whitespace().collect::<Vec<_>>().join(" ");

    // Look up product (route + variant + variant_map + image + files + options)
    type ProdRow = (
        i64, String, String, i64,
        Option<i64>, Option<String>, Option<String>,
        Option<String>, Option<String>,
    );
    let product: Option<ProdRow> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT id, name, COALESCE(production_route,'sweep_manual'), price_jpy,
                    printful_variant_id, image_url, printful_variant_map,
                    printful_files, printful_options
             FROM collab_products WHERE slug=? AND partner='sweep'",
            params![slug],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)),
        ).ok()
    };
    let Some((_pid, name, route, _price, variant_id_default, image_url, variant_map_json,
              files_json, options_json)) = product else {
        eprintln!("[sweep/webhook] unknown slug: {}", slug);
        return;
    };

    // Resolve variant_id by size from the JSON map, falling back to default.
    // Map keys are upper-case (S/M/L/XL/2XL/OS/ONE SIZE/S/M etc).
    let size_key = size.to_uppercase();
    let variant_id: Option<i64> = variant_map_json.as_ref()
        .and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok())
        .and_then(|v| {
            v.get(&size_key).and_then(|x| x.as_i64())
                .or_else(|| v.get("OS").and_then(|x| x.as_i64()))
                .or_else(|| v.get("ONE SIZE").and_then(|x| x.as_i64()))
        })
        .or(variant_id_default);

    // Idempotent insert
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "INSERT OR IGNORE INTO collab_orders
                 (stripe_session, slug, size, email, ship_name, ship_address, ship_country,
                  amount_jpy, production_route, status, created_at)
             VALUES (?,?,?,?,?,?,?,?,?, 'received', ?)",
            params![
                session_id, slug, size, email, ship_name, ship_address, country,
                amount, route, chrono_now(),
            ],
        );
    }

    // Place Printful draft order for 'printful' route (when variant_id + key present)
    let printful_key = env::var("PRINTFUL_API_KEY").unwrap_or_default();
    let pf_order_id: Option<String> = if route == "printful"
        && variant_id.is_some()
        && !printful_key.is_empty()
    {
        // Build the line item.
        // Use product-specific files+options from DB (set in the seed); fall
        // back to a default DTG file URL using the SIIIEEP wordmark if none
        // is configured for this product (legacy rows).
        let mut item = serde_json::json!({
            "variant_id": variant_id.unwrap(),
            "quantity": 1,
        });
        let files_val: serde_json::Value = files_json.as_ref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_else(|| {
                let fallback_url = image_url.as_ref()
                    .filter(|u| !u.is_empty() && u.starts_with("http"))
                    .cloned()
                    .unwrap_or_else(|| "https://lifestyle.wearmu.com/sweep/_logo.png".into());
                serde_json::json!([{"type": "default", "url": fallback_url}])
            });
        item["files"] = files_val;
        if let Some(opts) = options_json.as_ref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .filter(|v| v.as_array().map_or(false, |a| !a.is_empty()))
        {
            item["options"] = opts;
        }

        // Convert JP prefecture name → ISO 3166-2 code (e.g. "Tokyo" → "JP-13").
        // Stripe Checkout returns prefecture as the English/Japanese name; Printful
        // requires the ISO subdivision code or accepts certain English names. Use a
        // small lookup to be safe.
        let state_code = jp_prefecture_to_iso(state).unwrap_or(state).to_string();

        let order = serde_json::json!({
            "recipient": {
                "name":         ship_name,
                "address1":     address1,
                "address2":     address2,
                "city":         city,
                "state_code":   state_code,
                "country_code": country,
                "zip":          zip,
            },
            "items": [item],
            // PRINTFUL_AUTO_CONFIRM env var で自動承認の切替:
            //   "true"  (default) → confirm=true で投入と同時にプリント開始・配送
            //   "false"           → draft (dashboard で人手承認後に出荷)
            //   "kill"            → 緊急ストップ。全て draft (override)
            //
            // 自動承認は 28-76% の粗利があるので Stripe 決済 → 即 Printful プリントが
            // ベース運用。返金/キャンセルは Stripe + Printful の cancel-before-ship で対応。
            "confirm": match env::var("PRINTFUL_AUTO_CONFIRM").as_deref() {
                Ok("kill") | Ok("false") | Ok("0") => false,
                _ => true,  // default true
            },
            // Printful caps external_id at 32 chars; live Stripe session ids are ~78.
            // Strip the "cs_live_" prefix (8 chars) and keep the first 32 of the random tail.
            "external_id": session_id
                .strip_prefix("cs_live_")
                .or_else(|| session_id.strip_prefix("cs_test_"))
                .unwrap_or(session_id.as_str())
                .chars().take(32).collect::<String>(),
        });
        match reqwest::Client::new()
            .post("https://api.printful.com/orders")
            .bearer_auth(&printful_key)
            .json(&order).send().await
        {
            Ok(r) if r.status().is_success() => {
                let j: serde_json::Value = r.json().await.unwrap_or_default();
                let oid = j["result"]["id"].as_i64().map(|n| n.to_string())
                    .or_else(|| j["result"]["external_id"].as_str().map(String::from));
                eprintln!("[sweep/printful] draft order created: {:?}", oid);
                oid
            }
            Ok(r) => {
                let s = r.status();
                let t = r.text().await.unwrap_or_default();
                eprintln!("[sweep/printful] {}: {}", s, t.chars().take(300).collect::<String>());
                None
            }
            Err(e) => { eprintln!("[sweep/printful] reqwest: {}", e); None }
        }
    } else { None };

    if let Some(ref oid) = pf_order_id {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "UPDATE collab_orders SET printful_order_id=?, status='printful_draft' WHERE stripe_session=?",
            params![oid, session_id],
        );
    }

    // Telegram alert (always)
    let tg_token = env::var("TELEGRAM_BOT_TOKEN").unwrap_or_default();
    let tg_chat  = env::var("TELEGRAM_CHAT_ID").unwrap_or_else(|_| "1136442501".into());
    if !tg_token.is_empty() {
        let route_emoji = match route.as_str() {
            "printful"      => "🧵 printful draft",
            "sweep_manual"  => "🥋 SWEEP 手動生産",
            "pre_order"     => "📋 受注生産",
            _               => "?",
        };
        let pf_line = pf_order_id.as_ref().map(|o| format!("\nPrintful: {}", o)).unwrap_or_default();
        let body = format!(
            "🎽 MU × SWEEP 受注\n{name} (size {size}) — ¥{amount}\n{email}\n{ship_name} / {ship_address}\nroute: {route_emoji}{pf}\nstripe: {sid}",
            name = name, size = size, amount = amount, email = email,
            ship_name = ship_name, ship_address = ship_address,
            route_emoji = route_emoji, pf = pf_line, sid = session_id,
        );
        let _ = reqwest::Client::new()
            .post(format!("https://api.telegram.org/bot{}/sendMessage", tg_token))
            .json(&serde_json::json!({"chat_id": tg_chat, "text": body, "disable_web_page_preview": true}))
            .send().await;
    }

    // Resend email to ops + SWEEP社 (when configured)
    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    if !resend_key.is_empty() {
        let to_csv = env::var("SWEEP_OPS_EMAILS")
            .unwrap_or_else(|_| "mail@yukihamada.jp".into());
        let to_list: Vec<String> = to_csv.split(',').map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()).collect();
        let pf_html = pf_order_id.as_ref()
            .map(|o| format!("<tr><td>Printful draft</td><td>{}</td></tr>", html_attr_escape(o)))
            .unwrap_or_default();
        let img_html = image_url.as_ref().filter(|u| !u.is_empty() && u.starts_with("http"))
            .map(|u| format!(r#"<p><img src="{}" alt="" style="max-width:280px"></p>"#, html_attr_escape(u)))
            .unwrap_or_default();
        let html = format!(
            r#"<div style="font-family:'Helvetica Neue',Arial,sans-serif;background:#0A0A0A;color:#F5F5F0;padding:32px;max-width:560px">
<h2 style="color:#e6c449;font-weight:300;letter-spacing:0.1em">MU × SWEEP 受注</h2>
<table style="font-size:13px;line-height:1.85">
<tr><td>商品</td><td>{name}</td></tr>
<tr><td>size</td><td>{size}</td></tr>
<tr><td>金額</td><td>¥{amount}</td></tr>
<tr><td>route</td><td>{route}</td></tr>
{pf_html}
<tr><td>顧客</td><td>{email}</td></tr>
<tr><td>宛先</td><td>{ship_name}<br>{ship_address}</td></tr>
<tr><td>stripe</td><td>{sid}</td></tr>
</table>
{img_html}
<p>route が <code>sweep_manual</code> / <code>pre_order</code> の場合、SWEEP社 が手作業で生産・発送。<code>printful</code> は dashboard に draft 注文が入っています — 承認後に出荷されます。</p>
</div>"#,
            name = html_attr_escape(&name), size = html_attr_escape(&size),
            amount = amount, route = html_attr_escape(&route), pf_html = pf_html,
            email = html_attr_escape(&email), ship_name = html_attr_escape(&ship_name),
            ship_address = html_attr_escape(&ship_address), sid = html_attr_escape(&session_id),
            img_html = img_html,
        );
        let _ = reqwest::Client::new()
            .post("https://api.resend.com/emails")
            .bearer_auth(&resend_key)
            .json(&serde_json::json!({
                "from": "MU × SWEEP <noreply@wearmu.com>",
                "to":   to_list,
                "subject": format!("[MU×SWEEP] 受注 {} ¥{}", name, amount),
                "html": html,
            }))
            .send().await;
    }
}

async fn settle_auction(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>,
) -> impl IntoResponse {
    if let Err(resp) = require_admin_token(q.get("token")) {
        return resp;
    }
    let product_id: i64 = match q.get("product_id").and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => return (StatusCode::BAD_REQUEST, "missing product_id").into_response(),
    };

    // Find highest bid (also fetch its wallet_token, generating one if missing)
    let bid = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT b.id, b.amount, b.email, b.wallet, b.wallet_token, p.name, p.price_jpy,
                    COALESCE(b.nft_opt_in, 0), b.nft_wallet
             FROM bids b
             JOIN products p ON p.id = b.product_id
             WHERE b.product_id=? ORDER BY b.amount DESC LIMIT 1",
            params![product_id],
            |row| Ok((
                row.get::<_,i64>(0)?,
                row.get::<_,i64>(1)?,
                row.get::<_,String>(2)?,
                row.get::<_,Option<String>>(3)?,
                row.get::<_,Option<String>>(4)?,
                row.get::<_,String>(5)?,
                row.get::<_,i64>(6)?,
                row.get::<_,i64>(7)?,
                row.get::<_,Option<String>>(8)?,
            ))
        )
    };
    let (bid_id, amount, email, current_wallet, wallet_token_opt, product_name, _base_price,
         nft_opt_in, nft_wallet_opt) = match bid {
        Ok(r) => r,
        Err(_) => return (StatusCode::NOT_FOUND, "no bids found").into_response(),
    };
    // Backfill a wallet_token if this bid pre-dates the column
    let wallet_token = match wallet_token_opt {
        Some(t) if !t.is_empty() => t,
        _ => {
            let t = uuid::Uuid::new_v4().to_string();
            let conn = db.lock().unwrap();
            conn.execute("UPDATE bids SET wallet_token=? WHERE id=?", params![t, bid_id]).ok();
            t
        }
    };

    // MA Council full-tier promotion. Done BEFORE the Stripe call so that
    // even if payment-link creation fails, the winner is still recorded as
    // a council full member. The trial → full upgrade is idempotent.
    let council_token_winner = {
        let conn = db.lock().unwrap();
        let _ = council_enroll(&conn, &email, "full", Some(product_id));
        council_token_for(&email)
    };

    // ── Soulbound NFT pilot (Q3 vision item, shipped behind MU_NFT_MINT_LIVE) ──
    // Dispatched BEFORE the Stripe call so a Stripe outage doesn't block the
    // certificate flow (the cNFT is independent of payment settlement). Async
    // / background — never blocks the response. Default mode = dry-run (writes
    // `dryrun:<uuid>` to products.nft_mint without hitting Helius). Flip
    // MU_NFT_MINT_LIVE=1 once HELIUS_API_KEY is set.
    let nft_target = nft_wallet_opt.clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| current_wallet.clone().filter(|s| !s.trim().is_empty()));
    let nft_minted: bool = if nft_opt_in == 1 || nft_target.is_some() {
        if let Some(wallet) = nft_target.clone() {
            nft::mint_soulbound_bg(db.clone(), product_id, wallet, "settle_auction");
            true
        } else {
            eprintln!("[nft] settle_auction product_id={} skipped: opt-in without wallet", product_id);
            false
        }
    } else { false };
    let _ = bid_id; // currently unused; retained for future event logging

    // Create Stripe payment link
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3000".into());
    let client = reqwest::Client::new();
    let session_resp = client
        .post("https://api.stripe.com/v1/checkout/sessions")
        .basic_auth(&stripe_key, None::<&str>)
        .form(&[
            ("mode", "payment"),
            ("currency", "jpy"),
            ("line_items[0][price_data][currency]", "jpy"),
            ("line_items[0][price_data][product_data][name]", &format!("{} — 落札", product_name)),
            ("line_items[0][price_data][unit_amount]", &amount.to_string()),
            ("line_items[0][quantity]", "1"),
            ("success_url", &format!("{}/success?sid={{CHECKOUT_SESSION_ID}}", base_url)),
            ("cancel_url", &format!("{}/ma", base_url)),
            ("customer_email", &email),
            ("shipping_address_collection[allowed_countries][0]", "JP"),
            ("shipping_address_collection[allowed_countries][1]", "US"),
            ("shipping_address_collection[allowed_countries][2]", "GB"),
            ("shipping_address_collection[allowed_countries][3]", "FR"),
            ("shipping_address_collection[allowed_countries][4]", "DE"),
            ("shipping_address_collection[allowed_countries][5]", "AU"),
            ("shipping_address_collection[allowed_countries][6]", "KR"),
            ("shipping_address_collection[allowed_countries][7]", "TW"),
            ("shipping_address_collection[allowed_countries][8]", "HK"),
            ("allow_promotion_codes", "true"),
            ("metadata[product_id]", &product_id.to_string()),
            ("metadata[size]", "one-size"),
            ("metadata[wallet]", ""),
        ])
        .send().await;

    let payment_url = match session_resp {
        Ok(r) if r.status().is_success() => {
            let json: serde_json::Value = r.json().await.unwrap_or_default();
            json["url"].as_str().unwrap_or("").to_string()
        }
        _ => return (StatusCode::INTERNAL_SERVER_ERROR, "stripe error").into_response(),
    };

    // Send email to winner via Resend
    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    let wallet_url = format!("{}/wallet/{}", base_url, wallet_token);
    let wallet_block = if current_wallet.as_deref().map(|s| !s.is_empty()).unwrap_or(false) {
        format!(r#"<div style="background:#1C1C1C;padding:16px 20px;margin-bottom:24px;font-size:10px;line-height:1.7;opacity:0.7">
        登録済みウォレット: <span style="font-family:monospace">{wallet}</span><br>
        <a href="{wallet_url}" style="color:#F5F5F0;text-decoration:underline;opacity:0.7">変更する</a>
        </div>"#, wallet = current_wallet.clone().unwrap_or_default(), wallet_url = wallet_url)
    } else {
        format!(r#"<div style="background:#1C1C1C;padding:20px;margin-bottom:24px;border-left:2px solid #C8B560">
        <div style="font-size:9px;letter-spacing:0.2em;text-transform:uppercase;opacity:0.65;margin-bottom:8px">NFT受取ウォレット未登録</div>
        <div style="font-size:11px;line-height:1.7;opacity:0.85;margin-bottom:12px">
        Soulbound NFT証明書を受け取るSolanaウォレットアドレスを登録してください。発送までに登録があれば自動送付します。
        </div>
        <a href="{wallet_url}" style="display:inline-block;color:#F5F5F0;text-decoration:underline;font-size:11px;letter-spacing:0.15em">ウォレットを登録 →</a>
        </div>"#, wallet_url = wallet_url)
    };
    // MA Council membership block — only shown when COUNCIL_TOKEN_SECRET is
    // configured (otherwise we can't generate a stable token).
    let council_block = match council_token_winner.as_ref() {
        Some(t) => format!(r#"<div style="background:#1C1C1C;padding:20px;margin-bottom:24px;border-left:2px solid #e6c449">
        <div style="font-size:9px;letter-spacing:0.2em;text-transform:uppercase;color:#e6c449;opacity:0.9;margin-bottom:8px">MA COUNCIL — FULL MEMBER</div>
        <div style="font-size:11px;line-height:1.7;opacity:0.85;margin-bottom:12px">
        あなたは MA Council のフルメンバーになりました。週次の council brief とブランドの議決権が付与されます。
        </div>
        <a href="{base}/council?token={tok}" style="display:inline-block;color:#e6c449;text-decoration:underline;font-size:11px;letter-spacing:0.15em">Council を開く →</a>
        </div>"#, base = base_url, tok = t),
        None => String::new(),
    };
    let html = format!(r#"
<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:48px;max-width:560px;margin:0 auto">
  <div style="font-size:22px;font-weight:700;letter-spacing:0.4em;margin-bottom:32px">MU</div>
  <div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.65;margin-bottom:8px">間 MA — 落札のお知らせ</div>
  <div style="font-size:18px;font-weight:300;margin-bottom:24px">おめでとうございます。落札されました。</div>
  <div style="background:#1C1C1C;padding:24px;margin-bottom:24px">
    <div style="font-size:9px;opacity:0.65;letter-spacing:0.2em;text-transform:uppercase;margin-bottom:8px">落札金額</div>
    <div style="font-size:28px;font-weight:200">¥{amount}</div>
    <div style="font-size:10px;opacity:0.65;margin-top:8px">{product_name}</div>
  </div>
  <p style="font-size:12px;line-height:1.85;opacity:0.5;margin-bottom:24px">
    下記のボタンから決済をお願いします。<br>
    決済確認後、Printfulよりご自宅に発送します（約10〜14営業日）。<br>
    Soulbound NFT証明書は発送後にSolanaウォレットへ送付します。
  </p>
  <a href="{payment_url}" style="display:inline-block;background:#F5F5F0;color:#0A0A0A;padding:16px 32px;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;text-decoration:none;font-weight:600">決済する →</a>
  <div style="margin-top:32px"></div>
  {wallet_block}
  {council_block}
  <div style="margin-top:48px;border-top:1px solid #1C1C1C;padding-top:20px;font-size:9px;opacity:0.5;letter-spacing:0.1em">
    MU — wearmu.com | mail@yukihamada.jp
  </div>
</div>
"#,
        amount = amount.to_string().chars().rev().collect::<Vec<_>>().chunks(3)
            .map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(",")
            .chars().rev().collect::<String>(),
        product_name = product_name,
        payment_url = payment_url,
        wallet_block = wallet_block,
        council_block = council_block,
    );

    client.post("https://api.resend.com/emails")
        .bearer_auth(&resend_key)
        .json(&serde_json::json!({
            "from": "MU <noreply@wearmu.com>",
            "to": [&email],
            "subject": format!("【MU 間 MA】落札のお知らせ — ¥{}", amount),
            "html": html,
        }))
        .send().await.ok();

    Json(serde_json::json!({
        "ok": true,
        "winner": email,
        "amount": amount,
        "payment_url": payment_url,
        "wallet_url": wallet_url,
        "wallet_registered": current_wallet.as_deref().map(|s| !s.is_empty()).unwrap_or(false),
        "council_token": council_token_winner,
        "council_url": council_token_winner.as_ref()
            .map(|t| format!("{}/council?token={}", base_url, t)),
        "nft_mint_dispatched": nft_minted,
        "nft_mint_live": std::env::var("MU_NFT_MINT_LIVE").unwrap_or_default() == "1",
    })).into_response()
}

/// Lookup-by-token wallet management page. Linked from auction-winner emails.
/// Single-page form: shows current wallet (if any) and lets the winner set/replace it.
async fn wallet_page(
    Path(token): Path<String>,
    State(db): State<Db>,
) -> impl IntoResponse {
    let row = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT b.email, b.wallet, b.amount, p.name FROM bids b
             JOIN products p ON p.id = b.product_id
             WHERE b.wallet_token=? LIMIT 1",
            params![token],
            |r| Ok((
                r.get::<_,String>(0)?,
                r.get::<_,Option<String>>(1)?.unwrap_or_default(),
                r.get::<_,i64>(2)?,
                r.get::<_,String>(3)?,
            ))
        )
    };
    let (email, wallet, amount, product_name) = match row {
        Ok(r) => r,
        Err(_) => {
            return (StatusCode::NOT_FOUND, Html("<h1 style='font-family:sans-serif;color:#666;text-align:center;margin-top:30vh'>Token not found.</h1>".to_string())).into_response();
        }
    };
    let masked_email = {
        let parts: Vec<&str> = email.splitn(2, '@').collect();
        if parts.len() == 2 {
            let local = parts[0];
            let masked: String = if local.len() <= 2 {
                "*".repeat(local.len())
            } else {
                format!("{}***{}", &local[..1], &local[local.len()-1..])
            };
            format!("{}@{}", masked, parts[1])
        } else { "***".into() }
    };
    let html = format!(r#"<!doctype html><html lang="ja"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="robots" content="noindex,nofollow">
<title>Wallet — MU 間 MA</title>
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
body{{background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;min-height:100vh;display:flex;align-items:center;justify-content:center;padding:24px}}
.card{{max-width:480px;width:100%;background:#111;padding:40px;border:1px solid #1C1C1C}}
.brand{{font-size:18px;font-weight:700;letter-spacing:0.4em;margin-bottom:32px}}
.label{{font-size:9px;letter-spacing:0.25em;text-transform:uppercase;opacity:0.55;margin-bottom:6px}}
.value{{font-size:13px;margin-bottom:18px;font-weight:300}}
.amt{{font-size:24px;font-weight:200;margin-bottom:18px}}
input[type=text]{{width:100%;padding:14px 16px;background:#0A0A0A;border:1px solid #2A2A2A;color:#F5F5F0;font-size:13px;font-family:monospace;letter-spacing:0.05em}}
input[type=text]:focus{{outline:none;border-color:#C8B560}}
button{{margin-top:14px;width:100%;background:#F5F5F0;color:#0A0A0A;border:0;padding:14px;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;font-weight:600;cursor:pointer}}
button:disabled{{opacity:0.5;cursor:wait}}
.note{{font-size:10px;line-height:1.7;opacity:0.55;margin-top:18px}}
.msg{{font-size:11px;margin-top:12px;min-height:16px;letter-spacing:0.05em}}
.ok{{color:#5a9e6f}} .err{{color:#C8362C}}
hr{{border:0;border-top:1px solid #1C1C1C;margin:24px 0}}
</style></head>
<body><div class="card">
<div class="brand">MU</div>
<div class="label">間 MA — 落札</div>
<div class="value">{product_name}</div>
<div class="label">落札金額</div>
<div class="amt">¥{amount}</div>
<div class="label">登録メールアドレス</div>
<div class="value">{masked_email}</div>
<hr>
<div class="label">Solanaウォレットアドレス（NFT受取用）</div>
<input id="w" type="text" value="{wallet}" placeholder="例: 8CeusiVAeibuBGv5xcf7kt7JQZzqwTS5pD7u2CfyoWnL" autocomplete="off" spellcheck="false">
<button id="b" onclick="save()">登録 / 更新</button>
<div id="m" class="msg"></div>
<div class="note">アドレスは32〜44文字の Base58 形式（数字とアルファベットの英字）。<br>登録は何度でも変更可能。発送までの最終登録分にNFTを送付します。</div>
</div>
<script>
async function save(){{
  const w=document.getElementById('w').value.trim();
  const m=document.getElementById('m');
  const b=document.getElementById('b');
  m.className='msg';
  if(!/^[1-9A-HJ-NP-Za-km-z]{{32,44}}$/.test(w)){{
    m.className='msg err';m.textContent='アドレスの形式が正しくありません。';return;
  }}
  b.disabled=true;m.textContent='送信中…';
  try{{
    const r=await fetch('/api/wallet/{token}',{{method:'POST',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{wallet:w}})}});
    if(r.ok){{m.className='msg ok';m.textContent='登録しました。';}}
    else{{m.className='msg err';m.textContent='エラーが発生しました（'+r.status+'）。';}}
  }}catch(e){{m.className='msg err';m.textContent='ネットワークエラー';}}
  b.disabled=false;
}}
</script></body></html>"#,
        product_name = product_name,
        amount = amount.to_string().chars().rev().collect::<Vec<_>>().chunks(3)
            .map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(",")
            .chars().rev().collect::<String>(),
        masked_email = masked_email,
        wallet = wallet,
        token = token,
    );
    Html(html).into_response()
}

async fn update_wallet(
    Path(token): Path<String>,
    State(db): State<Db>,
    Json(body): Json<UpdateWalletBody>,
) -> impl IntoResponse {
    let w = body.wallet.trim();
    // Solana addresses are Base58, 32–44 chars
    let valid = w.len() >= 32 && w.len() <= 44
        && w.chars().all(|c| matches!(c,
            '1'..='9' | 'A'..='H' | 'J'..='N' | 'P'..='Z' | 'a'..='k' | 'm'..='z'));
    if !valid {
        return (StatusCode::BAD_REQUEST, "invalid wallet address").into_response();
    }
    let n = {
        let conn = db.lock().unwrap();
        conn.execute(
            "UPDATE bids SET wallet=? WHERE wallet_token=?",
            params![w, token]
        ).unwrap_or(0)
    };
    if n == 0 {
        return (StatusCode::NOT_FOUND, "token not found").into_response();
    }
    Json(serde_json::json!({"ok": true})).into_response()
}

async fn deactivate_product(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>,
) -> impl IntoResponse {
    if let Err(resp) = require_admin_token(q.get("token")) {
        return resp;
    }
    let id: i64 = match q.get("product_id").and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => return (StatusCode::BAD_REQUEST, "missing product_id").into_response(),
    };
    let conn = db.lock().unwrap();
    conn.execute("UPDATE products SET active=0 WHERE id=?", params![id]).unwrap();
    Json(serde_json::json!({"ok": true, "id": id})).into_response()
}

/// Admin diagnostic: dump full product row (including design_url) for one or all products.
/// Use ?id=<n> for a single product, omit for all (active+inactive).
async fn admin_lookup(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>,
) -> impl IntoResponse {
    if let Err(resp) = require_admin_token(q.get("token")) {
        return resp;
    }
    let conn = db.lock().unwrap();
    let sql = if q.contains_key("id") {
        "SELECT id, brand, drop_num, name, design_url, mockup_url, active, sold, prompt_hash
         FROM products WHERE id=?"
    } else {
        "SELECT id, brand, drop_num, name, design_url, mockup_url, active, sold, prompt_hash
         FROM products ORDER BY id DESC LIMIT 200"
    };
    let mut stmt = conn.prepare(sql).unwrap();
    let mapper = |row: &rusqlite::Row| -> rusqlite::Result<serde_json::Value> {
        Ok(serde_json::json!({
            "id":          row.get::<_, i64>(0)?,
            "brand":       row.get::<_, String>(1)?,
            "drop_num":    row.get::<_, i64>(2)?,
            "name":        row.get::<_, String>(3)?,
            "design_url":  row.get::<_, Option<String>>(4)?,
            "mockup_url":  row.get::<_, Option<String>>(5)?,
            "active":      row.get::<_, i64>(6)?,
            "sold":        row.get::<_, i64>(7)?,
            "prompt_hash": row.get::<_, Option<String>>(8)?,
        }))
    };
    let rows: Vec<serde_json::Value> = if let Some(id_str) = q.get("id") {
        let id: i64 = match id_str.parse() {
            Ok(v) => v,
            Err(_) => return (StatusCode::BAD_REQUEST, "bad id").into_response(),
        };
        stmt.query_map(params![id], mapper).unwrap().filter_map(|r| r.ok()).collect()
    } else {
        stmt.query_map([], mapper).unwrap().filter_map(|r| r.ok()).collect()
    };
    Json(serde_json::json!({"rows": rows})).into_response()
}

async fn update_mockup(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>,
    Json(body): Json<UpdateMockupBody>,
) -> impl IntoResponse {
    if let Err(resp) = require_admin_token(q.get("token")) {
        return resp;
    }
    let final_url = persist_mockup_if_temporary(body.product_id, &body.mockup_url)
        .await
        .unwrap_or_else(|| body.mockup_url.clone());
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "UPDATE products SET mockup_url=? WHERE id=?",
            params![final_url, body.product_id]
        ).unwrap();
    }
    Json(serde_json::json!({"ok": true, "mockup_url": final_url})).into_response()
}

/// Direct image upload (multipart/form-data). Use this to fix products whose
/// Printful tmp URL has already expired, or to override the auto-generated mockup.
/// Form fields: `product_id` (text) and `file` (image/jpeg).
async fn upload_mockup(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>,
    mut multipart: axum::extract::Multipart,
) -> impl IntoResponse {
    if let Err(resp) = require_admin_token(q.get("token")) {
        return resp;
    }
    let mut product_id: Option<i64> = None;
    let mut file_bytes: Option<axum::body::Bytes> = None;
    while let Some(field) = match multipart.next_field().await {
        Ok(f) => f,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("multipart error: {}", e)).into_response(),
    } {
        match field.name().unwrap_or("") {
            "product_id" => {
                product_id = field.text().await.ok().and_then(|s| s.parse().ok());
            }
            "file" => {
                file_bytes = field.bytes().await.ok();
            }
            _ => {}
        }
    }
    let pid = match product_id {
        Some(v) => v,
        None => return (StatusCode::BAD_REQUEST, "missing product_id").into_response(),
    };
    let bytes = match file_bytes {
        Some(b) if !b.is_empty() => b,
        _ => return (StatusCode::BAD_REQUEST, "missing or empty file").into_response(),
    };
    let stored_url = match store_mockup_bytes(pid, &bytes).await {
        Some(u) => u,
        None => return (StatusCode::INTERNAL_SERVER_ERROR, "storage error").into_response(),
    };
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "UPDATE products SET mockup_url=? WHERE id=?",
            params![stored_url, pid]
        ).unwrap();
    }
    Json(serde_json::json!({
        "ok": true,
        "product_id": pid,
        "mockup_url": stored_url,
        "bytes": bytes.len(),
    })).into_response()
}

/// Admin: re-generate the chest-print mockup for products whose original
/// Printful temp URL expired. Uses stored prompt_text + weather metadata
/// + drop_num seed, persists bytes via store_mockup_bytes (R2 or Fly
/// volume), updates mockup_url + active=1.
///
/// Usage: POST /api/admin/recover_mugen?token=<ADMIN_TOKEN>
///   body: {"drop_nums":[1,2,3,4,5]}  (or omit to recover every active=0
///   row that has weather metadata)
async fn admin_recover_mugen(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(resp) = require_admin_token(q.get("token")) { return resp; }

    let drop_nums: Vec<i64> = body.get("drop_nums")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_i64()).collect())
        .unwrap_or_default();

    type Row = (i64, i64, String, Option<String>, Option<String>);
    let rows: Vec<Row> = {
        let conn = db.lock().unwrap();
        let sql = if drop_nums.is_empty() {
            "SELECT id, drop_num, name, prompt_text, seed_data
             FROM products WHERE brand='mugen' AND active=0
               AND seed_data IS NOT NULL".to_string()
        } else {
            let placeholders = (0..drop_nums.len()).map(|_| "?").collect::<Vec<_>>().join(",");
            format!(
                "SELECT id, drop_num, name, prompt_text, seed_data
                 FROM products WHERE brand='mugen' AND drop_num IN ({})",
                placeholders
            )
        };
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {}", e)).into_response(),
        };
        let mapper = |r: &rusqlite::Row| Ok((
            r.get::<_,i64>(0)?, r.get::<_,i64>(1)?, r.get::<_,String>(2)?,
            r.get::<_,Option<String>>(3)?, r.get::<_,Option<String>>(4)?,
        ));
        if drop_nums.is_empty() {
            stmt.query_map([], mapper)
                .map(|it| it.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
        } else {
            stmt.query_map(rusqlite::params_from_iter(drop_nums.iter()), mapper)
                .map(|it| it.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
        }
    };

    let mut out: Vec<serde_json::Value> = Vec::with_capacity(rows.len());
    for (pid, drop_num, _name, prompt_text, seed_data) in &rows {
        let weather: serde_json::Value = seed_data.as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(serde_json::json!({}));
        let temp = weather.get("weather").and_then(|w| w.get("temp_c")).and_then(|v| v.as_f64()).unwrap_or(13.0);
        let cond = weather.get("weather").and_then(|w| w.get("condition")).and_then(|v| v.as_str()).unwrap_or("Sunny");
        let wind = weather.get("weather").and_then(|w| w.get("wind_kmh")).and_then(|v| v.as_f64()).unwrap_or(0.0);

        let synth_prompt = format!(
            "MUGEN #{} of 108 — Hokkaido Teshikaga weather: {:.0}°C, {}, wind {:.0} km/h. \
             Abstract editorial garment graphic, hand-drawn imperfection, slightly desaturated, \
             interprets the weather as feeling not picture.",
            drop_num, temp, cond, wind);
        let final_prompt = prompt_text.as_deref().filter(|s| !s.trim().is_empty())
            .unwrap_or(synth_prompt.as_str()).to_string();

        let tee = gemini::TeeDesign {
            name: &format!("MUGEN #{:04}", drop_num),
            prompt: &final_prompt,
            mood: &["minimal".into(), "weather-driven".into()],
            palette: &["muted earth tones".into()],
            scene: &["every-day".into()],
            seed: &format!("mugen-{:04}", drop_num),
            bio: "",
        };
        match gemini::generate_tee(&tee).await {
            Ok(g) => {
                let bytes = axum::body::Bytes::from(g.bytes);
                let stored = match store_mockup_bytes(*pid, &bytes).await {
                    Some(u) => u,
                    None => {
                        out.push(serde_json::json!({
                            "drop_num": drop_num, "product_id": pid,
                            "status": "store_failed",
                        }));
                        continue;
                    }
                };
                {
                    let conn = db.lock().unwrap();
                    let _ = conn.execute(
                        "UPDATE products SET mockup_url=?, active=1 WHERE id=?",
                        params![stored, pid],
                    );
                }
                out.push(serde_json::json!({
                    "drop_num": drop_num, "product_id": pid,
                    "status": "ok", "mockup_url": stored, "bytes": bytes.len(),
                }));
            }
            Err(e) => {
                eprintln!("[recover_mugen] drop_num {} gemini failed: {}", drop_num, e);
                out.push(serde_json::json!({
                    "drop_num": drop_num, "product_id": pid,
                    "status": "gemini_failed", "error": e,
                }));
            }
        }
        // pace to stay under Gemini rate
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    }
    Json(serde_json::json!({
        "ok": true,
        "candidates": rows.len(),
        "results": out,
    })).into_response()
}

async fn import_product(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>,
    Json(body): Json<ImportProductBody>,
) -> impl IntoResponse {
    if let Err(resp) = require_admin_token(q.get("token")) {
        return resp;
    }
    let new_id: i64 = {
        let conn = db.lock().unwrap();
        let now = chrono_now();
        conn.execute(
            "INSERT INTO products
             (brand, drop_num, name, design_url, mockup_url, price_jpy, inventory,
              created_at, weather_data, prompt_hash, seed_data, auction_end, nft_mint)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![body.brand, body.drop_num, body.name, body.design_url, body.mockup_url,
                    body.price_jpy, body.inventory, now, body.weather_data,
                    body.prompt_hash, body.seed_data, body.auction_end, body.nft_mint]
        ).unwrap();
        conn.last_insert_rowid()
    };
    if let Some(src) = body.mockup_url.as_deref() {
        if let Some(internal) = persist_mockup_if_temporary(new_id, src).await {
            let conn = db.lock().unwrap();
            conn.execute(
                "UPDATE products SET mockup_url=? WHERE id=?",
                params![internal, new_id]
            ).ok();
        }
    }
    Json(serde_json::json!({"ok": true, "id": new_id})).into_response()
}

async fn update_price(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>,
    Json(body): Json<UpdatePriceBody>,
) -> impl IntoResponse {
    if let Err(resp) = require_admin_token(q.get("token")) {
        return resp;
    }
    let conn = db.lock().unwrap();
    let n = conn.execute(
        "UPDATE products SET price_jpy=? WHERE brand=? AND drop_num=?",
        params![body.price_jpy, body.brand, body.drop_num]
    ).unwrap_or(0);
    Json(serde_json::json!({"ok": true, "updated": n})).into_response()
}

async fn update_nft(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>,
    Json(body): Json<UpdateNftBody>,
) -> impl IntoResponse {
    if let Err(resp) = require_admin_token(q.get("token")) {
        return resp;
    }
    let conn = db.lock().unwrap();
    let n = conn.execute(
        "UPDATE products SET nft_mint=? WHERE brand=? AND drop_num=?",
        params![body.nft_mint, body.brand, body.drop_num]
    ).unwrap_or(0);
    Json(serde_json::json!({"ok": true, "updated": n})).into_response()
}

async fn update_design(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>,
    Json(body): Json<UpdateDesignBody>,
) -> impl IntoResponse {
    if let Err(resp) = require_admin_token(q.get("token")) {
        return resp;
    }
    let conn = db.lock().unwrap();
    let n = conn.execute(
        "UPDATE products SET design_url=? WHERE brand=? AND drop_num=?",
        params![body.design_url, body.brand, body.drop_num]
    ).unwrap_or(0);
    Json(serde_json::json!({"ok": true, "updated": n})).into_response()
}

async fn update_sold(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>,
    Json(body): Json<UpdateSoldBody>,
) -> impl IntoResponse {
    if let Err(resp) = require_admin_token(q.get("token")) {
        return resp;
    }
    let conn = db.lock().unwrap();
    let now = chrono_now();
    let n = conn.execute(
        "UPDATE products SET sold=?, sold_out_at=CASE WHEN ?>=inventory THEN ? ELSE NULL END
         WHERE brand=? AND drop_num=?",
        params![body.sold, body.sold, now, body.brand, body.drop_num]
    ).unwrap_or(0);
    Json(serde_json::json!({"ok": true, "updated": n})).into_response()
}

async fn nft_metadata(
    Path((brand, drop_num)): Path<(String, i64)>,
    State(db): State<Db>,
) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let result = conn.query_row(
        "SELECT name, mockup_url, design_url, weather_data, drop_num, nft_mint
         FROM products WHERE brand=? AND drop_num=? AND active=1 LIMIT 1",
        params![brand, drop_num],
        |row| Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    );
    match result {
        Err(_) => (StatusCode::NOT_FOUND, "not found").into_response(),
        Ok((name, mockup_url, design_url, weather_data, dn, _nft_mint)) => {
            let image = mockup_url.or(design_url).unwrap_or_default();
            let mut attributes = vec![
                serde_json::json!({"trait_type":"Brand","value":brand.to_uppercase()}),
                serde_json::json!({"trait_type":"Drop","value":dn}),
                serde_json::json!({"trait_type":"Location","value":"Teshikaga, Hokkaido"}),
            ];
            if let Some(wd) = weather_data {
                if let Ok(w) = serde_json::from_str::<serde_json::Value>(&wd) {
                    if let Some(v) = w.get("temp_c") {
                        attributes.push(serde_json::json!({"trait_type":"Temperature","value":format!("{}°C",v)}));
                    }
                    if let Some(v) = w.get("condition") {
                        attributes.push(serde_json::json!({"trait_type":"Weather","value":v}));
                    }
                    if let Some(v) = w.get("wind_kmh") {
                        attributes.push(serde_json::json!({"trait_type":"Wind","value":format!("{} km/h",v)}));
                    }
                }
            }
            let meta = serde_json::json!({
                "name": name,
                "symbol": "MU",
                "description": format!("MU {} — Autonomous design born from Hokkaido weather data. Each piece is unique.", brand.to_uppercase()),
                "image": image,
                "external_url": format!("https://wearmu.com/products/{}/{}", brand, dn),
                "seller_fee_basis_points": 500,
                "attributes": attributes,
                "properties": {
                    "category": "image",
                    "files": [{"uri": image, "type": "image/jpeg"}],
                    "creators": [{"address": env::var("MU_TREASURY_WALLET").unwrap_or_default(), "share": 100}]
                }
            });
            let mut headers = axum::http::HeaderMap::new();
            headers.insert("content-type", "application/json".parse().unwrap());
            headers.insert("cache-control", "public, max-age=3600".parse().unwrap());
            (StatusCode::OK, headers, serde_json::to_string(&meta).unwrap()).into_response()
        }
    }
}

async fn verify_page(
    Path((brand, drop_num)): Path<(String, i64)>,
    State(db): State<Db>,
) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let result = conn.query_row(
        "SELECT name, mockup_url, design_url, weather_data, price_jpy, inventory, sold,
                created_at, prompt_hash, nft_mint
         FROM products WHERE brand=? AND drop_num=? AND active=1 LIMIT 1",
        params![brand, drop_num],
        |row| Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
        ))
    );
    drop(conn);

    match result {
        Err(_) => (StatusCode::NOT_FOUND, Html("<html><body>Not found</body></html>".to_string())).into_response(),
        Ok((name, mockup_url, design_url, weather_data, price_jpy, inventory, sold, created_at, prompt_hash, nft_mint)) => {
            let image = mockup_url.or(design_url).unwrap_or_default();
            let brand_up = brand.to_uppercase();
            let drop_fmt = format!("#{:04}", drop_num);
            let remaining = (inventory - sold).max(0);

            let weather_html = weather_data.as_deref()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                .map(|w| {
                    let temp = w["temp_c"].as_i64().map(|v| format!("{}°C", v)).unwrap_or_default();
                    let cond = w["condition"].as_str().unwrap_or("").to_string();
                    let wind = w["wind_kmh"].as_str().map(|v| format!("{} km/h", v)).unwrap_or_default();
                    format!(
                        r#"<div class="row"><span class="label">気象条件</span><span class="val">{cond}</span></div>
                        <div class="row"><span class="label">気温</span><span class="val">{temp}</span></div>
                        <div class="row"><span class="label">風速</span><span class="val">{wind}</span></div>"#,
                        cond=cond, temp=temp, wind=wind
                    )
                })
                .unwrap_or_default();

            let nft_html = nft_mint.as_deref()
                .map(|mint| format!(
                    r#"<div class="row"><span class="label">NFT</span><span class="val mono">
                       <a href="https://solscan.io/token/{mint}" target="_blank" style="color:#9B8F6A">{short}…</a>
                    </span></div>"#,
                    mint=mint,
                    short=&mint[..mint.len().min(8)]
                ))
                .unwrap_or_default();

            let hash_html = prompt_hash.as_deref()
                .map(|h| format!(
                    r#"<div class="row"><span class="label">プロンプトハッシュ</span><span class="val mono">{}</span></div>"#,
                    &h[..h.len().min(12)]
                ))
                .unwrap_or_default();

            let html = format!(r#"<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{name} — MU 真正性証明</title>
<meta name="description" content="MU {brand_up} {drop_fmt} — 北海道の気象データから生まれた服の真正性証明">
<style>
*{{box-sizing:border-box;margin:0;padding:0}}
body{{background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Helvetica,Arial,sans-serif;min-height:100vh;padding:0 0 48px}}
.header{{display:flex;align-items:center;justify-content:space-between;padding:20px 24px;border-bottom:1px solid #1C1C1C}}
.logo{{font-size:18px;font-weight:700;letter-spacing:0.4em}}
.verified{{display:flex;align-items:center;gap:6px;font-size:10px;letter-spacing:0.2em;text-transform:uppercase;color:#4CAF50}}
.verified::before{{content:"✓";font-size:14px}}
.hero{{width:100%;aspect-ratio:1;background:#111;overflow:hidden}}
.hero img{{width:100%;height:100%;object-fit:cover}}
.body{{padding:24px}}
.brand-tag{{font-size:9px;letter-spacing:0.35em;text-transform:uppercase;opacity:0.65;margin-bottom:6px}}
.name{{font-size:22px;font-weight:300;letter-spacing:0.02em;margin-bottom:4px}}
.drop{{font-size:12px;opacity:0.65;letter-spacing:0.15em;margin-bottom:28px}}
.section-label{{font-size:8px;letter-spacing:0.35em;text-transform:uppercase;opacity:0.55;margin-bottom:12px;margin-top:28px}}
.row{{display:flex;justify-content:space-between;align-items:baseline;padding:10px 0;border-bottom:1px solid #1C1C1C;font-size:12px}}
.label{{opacity:0.65;letter-spacing:0.05em}}
.val{{font-weight:300;text-align:right;max-width:60%}}
.mono{{font-family:monospace;font-size:11px}}
.inventory{{display:flex;align-items:center;gap:6px}}
.inv-bar{{flex:1;height:2px;background:#1C1C1C;border-radius:1px;overflow:hidden}}
.inv-fill{{height:100%;background:#F5F5F0;border-radius:1px}}
.cta{{margin-top:32px;text-align:center}}
.cta a{{display:inline-block;border:1px solid #333;color:#F5F5F0;font-size:9px;letter-spacing:0.35em;text-transform:uppercase;padding:14px 28px;text-decoration:none}}
.cta a:hover{{background:#1C1C1C}}
.hokkaido{{margin-top:28px;font-size:10px;opacity:0.55;line-height:1.8;letter-spacing:0.05em}}
</style>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
</head>
<body>
<div class="header">
  <div class="logo">MU</div>
  <div class="verified">Verified</div>
</div>
{hero}
<div class="body">
  <div class="brand-tag">{brand_up}</div>
  <div class="name">{name}</div>
  <div class="drop">{drop_fmt}</div>

  <div class="section-label">真正性データ</div>
  <div class="row"><span class="label">ブランド</span><span class="val">{brand_up}</span></div>
  <div class="row"><span class="label">ドロップ番号</span><span class="val">{drop_fmt}</span></div>
  <div class="row"><span class="label">価格</span><span class="val">¥{price_fmt}</span></div>
  <div class="row"><span class="label">残り / 総数</span>
    <span class="val inventory">
      <span class="inv-bar"><span class="inv-fill" style="width:{inv_pct}%"></span></span>
      {remaining} / {inventory}
    </span>
  </div>
  {hash_html}

  <div class="section-label">生成気象データ（弟子屈・北海道）</div>
  {weather_html}

  {nft_section}

  <div class="cta">
    <a href="https://wearmu.com/{brand}">wearmu.com でMUを見る →</a>
  </div>
  <div class="hokkaido">
    北海道弟子屈町の気象データが自動的にこの服をデザインした。<br>
    気温が枚数を決め、風速が価格を決める。<br>
    二度と同じものは生まれない。
  </div>
</div>
</body>
</html>"#,
                name = name,
                brand_up = brand_up,
                drop_fmt = drop_fmt,
                hero = if image.is_empty() { String::new() } else {
                    format!(r#"<div class="hero"><img src="{}" alt="{}" loading="lazy"></div>"#, image, name)
                },
                price_fmt = {
                    let s = price_jpy.to_string();
                    let chars: Vec<char> = s.chars().collect();
                    let mut out = String::new();
                    let n = chars.len();
                    for (i, c) in chars.iter().enumerate() {
                        if i > 0 && (n - i) % 3 == 0 { out.push(','); }
                        out.push(*c);
                    }
                    out
                },
                remaining = remaining,
                inventory = inventory,
                inv_pct = if inventory > 0 { remaining * 100 / inventory } else { 0 },
                hash_html = hash_html,
                weather_html = weather_html,
                nft_section = nft_html,
            );

            let mut headers = axum::http::HeaderMap::new();
            headers.insert("content-type", "text/html; charset=utf-8".parse().unwrap());
            headers.insert("cache-control", "public, max-age=300".parse().unwrap());
            (StatusCode::OK, headers, html).into_response()
        }
    }
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    format!("{}", secs)
}

/// Convert a JP prefecture name (English or Japanese) to ISO 3166-2 subdivision
/// code (e.g. "JP-13"). Printful requires this for JP recipients; Stripe returns
/// the prefecture as a name string. Returns None if no match — caller should
/// pass through the raw value.
fn jp_prefecture_to_iso(s: &str) -> Option<&'static str> {
    match s.trim() {
        "Hokkaido"  | "Hokkaidō"  | "北海道"   => Some("JP-01"),
        "Aomori"    | "青森県" | "青森"        => Some("JP-02"),
        "Iwate"     | "岩手県" | "岩手"        => Some("JP-03"),
        "Miyagi"    | "宮城県" | "宮城"        => Some("JP-04"),
        "Akita"     | "秋田県" | "秋田"        => Some("JP-05"),
        "Yamagata"  | "山形県" | "山形"        => Some("JP-06"),
        "Fukushima" | "福島県" | "福島"        => Some("JP-07"),
        "Ibaraki"   | "茨城県" | "茨城"        => Some("JP-08"),
        "Tochigi"   | "栃木県" | "栃木"        => Some("JP-09"),
        "Gunma"     | "群馬県" | "群馬"        => Some("JP-10"),
        "Saitama"   | "埼玉県" | "埼玉"        => Some("JP-11"),
        "Chiba"     | "千葉県" | "千葉"        => Some("JP-12"),
        "Tokyo"     | "東京都" | "東京"        => Some("JP-13"),
        "Kanagawa"  | "神奈川県" | "神奈川"    => Some("JP-14"),
        "Niigata"   | "新潟県" | "新潟"        => Some("JP-15"),
        "Toyama"    | "富山県" | "富山"        => Some("JP-16"),
        "Ishikawa"  | "石川県" | "石川"        => Some("JP-17"),
        "Fukui"     | "福井県" | "福井"        => Some("JP-18"),
        "Yamanashi" | "山梨県" | "山梨"        => Some("JP-19"),
        "Nagano"    | "長野県" | "長野"        => Some("JP-20"),
        "Gifu"      | "岐阜県" | "岐阜"        => Some("JP-21"),
        "Shizuoka"  | "静岡県" | "静岡"        => Some("JP-22"),
        "Aichi"     | "愛知県" | "愛知"        => Some("JP-23"),
        "Mie"       | "三重県" | "三重"        => Some("JP-24"),
        "Shiga"     | "滋賀県" | "滋賀"        => Some("JP-25"),
        "Kyoto"     | "京都府" | "京都"        => Some("JP-26"),
        "Osaka"     | "大阪府" | "大阪"        => Some("JP-27"),
        "Hyogo"     | "Hyōgo"     | "兵庫県" | "兵庫" => Some("JP-28"),
        "Nara"      | "奈良県" | "奈良"        => Some("JP-29"),
        "Wakayama"  | "和歌山県" | "和歌山"    => Some("JP-30"),
        "Tottori"   | "鳥取県" | "鳥取"        => Some("JP-31"),
        "Shimane"   | "島根県" | "島根"        => Some("JP-32"),
        "Okayama"   | "岡山県" | "岡山"        => Some("JP-33"),
        "Hiroshima" | "広島県" | "広島"        => Some("JP-34"),
        "Yamaguchi" | "山口県" | "山口"        => Some("JP-35"),
        "Tokushima" | "徳島県" | "徳島"        => Some("JP-36"),
        "Kagawa"    | "香川県" | "香川"        => Some("JP-37"),
        "Ehime"     | "愛媛県" | "愛媛"        => Some("JP-38"),
        "Kochi"     | "Kōchi"     | "高知県" | "高知" => Some("JP-39"),
        "Fukuoka"   | "福岡県" | "福岡"        => Some("JP-40"),
        "Saga"      | "佐賀県" | "佐賀"        => Some("JP-41"),
        "Nagasaki"  | "長崎県" | "長崎"        => Some("JP-42"),
        "Kumamoto"  | "熊本県" | "熊本"        => Some("JP-43"),
        "Oita"      | "Ōita"      | "大分県" | "大分" => Some("JP-44"),
        "Miyazaki"  | "宮崎県" | "宮崎"        => Some("JP-45"),
        "Kagoshima" | "鹿児島県" | "鹿児島"    => Some("JP-46"),
        "Okinawa"   | "沖縄県" | "沖縄"        => Some("JP-47"),
        _ => None,
    }
}

/// Unix-epoch-seconds 30 days from now, as a string (matches you_users.trial_end_at format).
fn trial_end_seconds_from_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    format!("{}", secs + 30 * 86400)
}

/// Days since signup (created_at). Positive integer, 1 on day 0.
fn days_since_signup_secs(created_at_secs: u64) -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    if now <= created_at_secs { return 1; }
    1 + ((now - created_at_secs) / 86400) as i64
}

/// Whether a /you account is currently allowed to receive daily designs.
/// Active when ANY of:
///   - lifetime_free is set (bought a MU shirt → forever)
///   - trial_end_at is in the future
///   - subscription_until is in the future (¥980/月 paid plan)
fn you_user_active(trial_end_at: Option<&str>, lifetime_free: bool) -> bool {
    you_user_active_full(trial_end_at, lifetime_free, None)
}

fn you_user_active_full(
    trial_end_at: Option<&str>,
    lifetime_free: bool,
    subscription_until: Option<&str>,
) -> bool {
    if lifetime_free { return true; }
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let trial_end: u64 = trial_end_at.and_then(|s| s.parse().ok()).unwrap_or(0);
    if now < trial_end { return true; }
    let sub_end: u64 = subscription_until.and_then(|s| s.parse().ok()).unwrap_or(0);
    now < sub_end
}

/// Subscription state shown to the client (and stamped on emails).
fn you_user_state(trial_end_at: Option<&str>, lifetime_free: bool) -> serde_json::Value {
    you_user_state_full(trial_end_at, lifetime_free, None, None)
}

fn you_user_state_full(
    trial_end_at: Option<&str>,
    lifetime_free: bool,
    subscription_status: Option<&str>,
    subscription_until: Option<&str>,
) -> serde_json::Value {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let trial_end: u64 = trial_end_at.and_then(|s| s.parse().ok()).unwrap_or(0);
    let sub_end: u64   = subscription_until.and_then(|s| s.parse().ok()).unwrap_or(0);
    let on_paid = subscription_status.map(|s| s == "active" || s == "trialing").unwrap_or(false)
                  && sub_end > now;
    let days_left: i64 = if lifetime_free {
        -1
    } else if on_paid {
        ((sub_end - now) / 86400) as i64
    } else if trial_end > now {
        ((trial_end - now) / 86400) as i64
    } else {
        0
    };
    let status = if lifetime_free {
        "lifetime"
    } else if on_paid {
        "subscribed"
    } else if trial_end > now {
        "trial"
    } else {
        "expired"
    };
    serde_json::json!({
        "status": status,
        "trial_end_at": trial_end_at,
        "subscription_status": subscription_status,
        "subscription_until": subscription_until,
        "days_left": days_left,
        "lifetime_free": lifetime_free,
    })
}

/// Total active /you subscribers — used for social-proof badge on the LP.
/// Cached for 60 seconds to avoid hammering the DB on every page load.
async fn you_active_count(State(db): State<Db>) -> impl IntoResponse {
    let total: i64 = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM you_users WHERE unsubscribed_at IS NULL",
            [], |r| r.get(0),
        ).unwrap_or(0)
    };
    let lifetime: i64 = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM you_users WHERE unsubscribed_at IS NULL AND lifetime_free=1",
            [], |r| r.get(0),
        ).unwrap_or(0)
    };
    let designs_total: i64 = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM you_designs WHERE gen_status='ready'",
            [], |r| r.get(0),
        ).unwrap_or(0)
    };
    // Cache 60s on the CDN
    let mut headers = HeaderMap::new();
    headers.insert("Cache-Control", HeaderValue::from_static("public, max-age=60"));
    (headers, Json(serde_json::json!({
        "active_subscribers": total,
        "lifetime_members":   lifetime,
        "designs_generated":  designs_total,
    }))).into_response()
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

async fn blog_index() -> Html<&'static str> {
    Html(include_str!("../static/blog/index.html"))
}

async fn blog_post_001() -> Html<&'static str> {
    Html(include_str!("../static/blog/from-automation-to-autonomy.html"))
}

async fn tokushoho_page() -> Html<&'static str> {
    Html(include_str!("../static/tokushoho.html"))
}

async fn city_page() -> Html<&'static str> {
    Html(include_str!("../static/city.html"))
}

async fn you_page() -> Html<&'static str> {
    Html(include_str!("../static/you.html"))
}

async fn success_page() -> Html<&'static str> {
    Html(r#"<!DOCTYPE html><html><head><meta charset=UTF-8><style>
    body{background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',sans-serif;
    display:flex;align-items:center;justify-content:center;height:100vh;flex-direction:column;gap:20px}
    h1{font-size:14px;letter-spacing:0.4em;text-transform:uppercase;font-weight:300;opacity:0.6}
    p{font-size:11px;opacity:0.55;letter-spacing:0.1em}
    a{color:inherit;font-size:9px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.55;margin-top:40px}
    </style>
    <script defer src="https://enabler-analytics.fly.dev/t.js"></script>
    </head><body>
    <h1>Order Confirmed</h1>
    <p>確認メールをお送りしました。Printfulより発送します。</p>
    <a href="/">← Back to MU</a>
    </body></html>"#)
}

async fn fragment_request(
    State(db): State<Db>,
    Json(body): Json<FragmentBody>,
) -> impl IntoResponse {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();

    // Save to DB
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO fragment_requests (email, direction, order_ids, created_at) VALUES (?,?,?,?)",
            params![body.email, body.direction, body.order_ids, now],
        ).ok();
    }

    // Human-readable direction label
    let (direction_ja, direction_en, result_item) = match body.direction.as_str() {
        "mugen_to_muon" => ("MUGEN × 3 → MUON × 1", "MUGEN × 3 → MUON × 1", "MUON 1着"),
        "muon_to_ma"    => ("MUON × 3 → 間 MA × 1", "MUON × 3 → MA × 1",    "間 MA 1着"),
        _               => ("Exchange",               "Exchange",               "交換品"),
    };

    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    let client = reqwest::Client::new();

    // ── User confirmation email ──
    let user_html = format!(r#"
<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:48px;max-width:560px;margin:0 auto">
  <div style="font-size:22px;font-weight:700;letter-spacing:0.4em;margin-bottom:32px">MU</div>
  <div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.65;margin-bottom:8px">Fragment System</div>
  <div style="font-size:18px;font-weight:300;letter-spacing:0.05em;margin-bottom:24px">申請を受け付けました</div>
  <div style="background:#1C1C1C;padding:24px;margin-bottom:24px">
    <div style="font-size:9px;letter-spacing:0.25em;text-transform:uppercase;opacity:0.65;margin-bottom:8px">Exchange</div>
    <div style="font-size:14px">{direction_ja}</div>
    <div style="font-size:9px;opacity:0.65;margin-top:8px">注文番号: {order_ids}</div>
  </div>
  <p style="font-size:12px;line-height:1.85;opacity:0.5">
    担当者が注文を確認し、2営業日以内に返送先住所をこのメールにご返信します。<br>
    着払いで3着を返送してください。確認後、{result_item}をお送りします。<br><br>
    交換品の送料はMU負担です。申請から発送まで約2週間を予定しています。
  </p>
  <div style="margin-top:32px;padding-top:20px;border-top:1px solid #1C1C1C;font-size:9px;opacity:0.5;letter-spacing:0.1em">
    MU — AIが服を作り続けるブランド<br>wearmu.com
  </div>
</div>
"#,
        direction_ja = direction_ja,
        order_ids = body.order_ids,
        result_item = result_item,
    );

    let _ = client.post("https://api.resend.com/emails")
        .bearer_auth(&resend_key)
        .json(&serde_json::json!({
            "from": "MU <noreply@wearmu.com>",
            "to": [&body.email],
            "subject": format!("Fragment申請確認 — {}", direction_ja),
            "html": user_html,
        }))
        .send().await;

    // ── Admin notification email ──
    let admin_html = format!(r#"
<div style="font-family:monospace;padding:24px;background:#f5f5f0;color:#0a0a0a">
  <b>Fragment Request</b><br><br>
  Direction: {direction_en}<br>
  Email: {email}<br>
  Order IDs: {order_ids}<br>
  Time: {now}<br><br>
  Reply to this customer with the return address.
</div>
"#,
        direction_en = direction_en,
        email = body.email,
        order_ids = body.order_ids,
        now = now,
    );

    let _ = client.post("https://api.resend.com/emails")
        .bearer_auth(&resend_key)
        .json(&serde_json::json!({
            "from": "MU Fragment <noreply@wearmu.com>",
            "to": ["mail@yukihamada.jp"],
            "reply_to": &body.email,
            "subject": format!("[Fragment] {} — {}", direction_en, body.email),
            "html": admin_html,
        }))
        .send().await;

    Json(serde_json::json!({"ok": true}))
}

// ── MU × YOU — daily personalised collab tee ─────────────────────────────────
//
// Each subscriber gets one AI-prompt-driven design proposal per day, derived
// from their taste profile + a deterministic per-day seed. Free to subscribe;
// only the days they Claim become a Stripe checkout for a Bella+Canvas DTG tee.

#[derive(Deserialize)]
struct YouSubscribeBody {
    email: String,
    #[serde(default)] mood:    Vec<String>,
    #[serde(default)] palette: Vec<String>,
    #[serde(default)] scene:   Vec<String>,
    #[serde(default)] size:    String,
    #[serde(default)] bio:     String,
    /// Referral slug captured from `?ref=` on the LP. Used to credit the
    /// inviter when this signup makes their first purchase.
    #[serde(default)] ref_slug: Option<String>,
}

#[derive(Deserialize)]
struct YouFeedbackBody {
    token: String,
    design_id: i64,
    /// "skip" | "like" | "refresh"
    action: String,
}

#[derive(Deserialize)]
struct YouClaimBody {
    token: String,
    design_id: i64,
}

#[derive(Deserialize)]
struct YouUnsubBody {
    token: String,
}

fn jst_today_str() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64).unwrap_or(0) + 9 * 3600;
    let days = secs / 86400;
    civil_from_days_str(days)
}

fn civil_from_days(mut days: i64) -> (i64, i64, i64) {
    days += 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn civil_from_days_str(mut days: i64) -> String {
    days += 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Build a poetic JP design name + prompt from the user taste profile and the
/// day seed. Deterministic so the same user gets the same case if the day is
/// regenerated, but feels fresh because the seed shifts every JST date.
fn compose_design(taste: &serde_json::Value, day: &str, refresh_n: i64) -> (String, String, String) {
    use sha2::{Digest, Sha256};
    let seed_input = format!(
        "{}|{}|{}|r{}",
        day,
        taste.get("email").and_then(|v| v.as_str()).unwrap_or(""),
        serde_json::to_string(taste).unwrap_or_default(),
        refresh_n,
    );
    let mut hasher = Sha256::new();
    hasher.update(seed_input.as_bytes());
    let h = hasher.finalize();
    let seed = hex::encode(&h[..8]);

    let mood: Vec<String> = taste.get("mood")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let palette: Vec<String> = taste.get("palette")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let scene: Vec<String> = taste.get("scene")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let pick = |arr: &[String], offset: usize, fallback: &str| -> String {
        if arr.is_empty() { return fallback.to_string(); }
        let idx = (h[offset] as usize) % arr.len();
        arr[idx].clone()
    };

    let m1 = pick(&mood, 0, "静か");
    let m2 = if mood.len() > 1 { pick(&mood, 1, "余白") } else { String::new() };
    let pal = pick(&palette, 2, "墨");
    let sc  = pick(&scene, 3, "毎日");

    // Curated noun bank — selected per seed
    let nouns = [
        "霧","余白","ノイズ","回路","層","ふち","島","橋","残響","解像度",
        "層雲","北限","薄明","残光","水位","素描","点描","くずし","湾","ふもと",
    ];
    let noun = nouns[(h[4] as usize) % nouns.len()];
    let day_num_seed = (h[5] as i64 % 30) + 1;
    let _ = day_num_seed; // reserved

    let name = if !m2.is_empty() {
        format!("{} と {} の {}", m1, m2, noun)
    } else {
        format!("{} の {}", m1, noun)
    };

    // Pull learned preferences out of taste (set by ensure_design_for_day).
    let tend = taste.get("tend_toward").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>())
        .unwrap_or_default();
    let avoid = taste.get("avoid").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>())
        .unwrap_or_default();
    let prefs_clause = match (tend.is_empty(), avoid.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!(" 最近この人が好んだ語感: {}。", tend.join("、")),
        (true, false) => format!(" 最近この人が避けた語感: {}。", avoid.join("、")),
        (false, false) => format!(
            " 最近この人が好んだ語感: {} / 避けた語感: {}。",
            tend.join("、"), avoid.join("、")
        ),
    };

    let prompt = format!(
        "{date}・{mood}な{noun}を、{pal}の階調で。{sc}に着られる、身体の延長としてのコットンTシャツ。\
         胸ポケット位置に小さなモチーフ、背中に余白。10oz Bella+Canvas、DTG。{prefs}",
        date = day, mood = m1, noun = noun, pal = pal, sc = sc, prefs = prefs_clause,
    );

    (name, prompt, seed)
}

/// Day-7 / Day-14 / Day-30 special compositions. Returns (name, prompt, seed,
/// is_milestone). Milestone designs short-circuit the standard compose_design
/// so subscribers feel the cadence (peak-end / IKEA / endowment).
/// `day_num` is the day_num within this user's history (1-based).
fn compose_special_design(
    taste: &serde_json::Value, day: &str, day_num: i64
) -> Option<(String, String, String)> {
    use sha2::{Digest, Sha256};
    let style_name = taste.get("style_name").and_then(|v| v.as_str()).unwrap_or("");
    let bio = taste.get("bio").and_then(|v| v.as_str()).unwrap_or("");
    let seed_input = format!("milestone|{}|{}|{}|d{}",
        day,
        taste.get("email").and_then(|v| v.as_str()).unwrap_or(""),
        serde_json::to_string(taste).unwrap_or_default(),
        day_num,
    );
    let mut hasher = Sha256::new();
    hasher.update(seed_input.as_bytes());
    let h = hasher.finalize();
    let seed = hex::encode(&h[..8]);

    match day_num {
        14 => {
            // Day 14: peak. Most dramatic prompt of the 30-day arc.
            let name = if !style_name.is_empty() {
                format!("{} — 14 日目の頂", style_name)
            } else {
                "14 日目の頂".to_string()
            };
            let prompt = format!(
                "{date}・14 日間あなたが選び続けた方向の頂点。これまでの mood と palette を煮詰めて、\
                 たった一つに結晶させた一着。背中に小さく『MU × YOU · 14』。{bio_clause}\
                 アート性を強める、編集的でやや実験的、しかし日常で着られる。10oz Bella+Canvas、DTG。",
                date = day,
                bio_clause = if bio.is_empty() { String::new() } else { format!("着る人を表す『{}』。", bio) },
            );
            Some((name, prompt, seed))
        }
        30 => {
            // Day 30: "The 30" — the end. Blend of all 29 prior seeds.
            let name = if !style_name.is_empty() {
                format!("The 30 · {} の最終形", style_name)
            } else {
                "The 30 · 29 案を一着に".to_string()
            };
            let prompt = format!(
                "{date}・これは 30 日間の最後の 1 案。29 日分のあなたの選択（skip と like）が \
                 全て seed に折り込まれている、唯一の一着。{bio_clause}\
                 静かな祝祭感。胸に小さなモノグラム『M30』。背中に余白。\
                 編集デザイン、ややクラシック、長く着られる仕上げ。10oz Bella+Canvas、DTG。",
                date = day,
                bio_clause = if bio.is_empty() { String::new() } else { format!("「{}」と書いた人のための一着。", bio) },
            );
            Some((name, prompt, seed))
        }
        _ => None,
    }
}

/// SVG image URL (data URI) generated server-side for the design preview.
/// Uses the seed to deterministically choose hues. Replace later with a real
/// generative pipeline (Gemini / SDXL) — the schema persists the image_url
/// field so swapping in a CDN URL later is a one-line change.
fn render_design_svg(name: &str, seed: &str) -> String {
    let h: u32 = u32::from_str_radix(seed.get(0..6).unwrap_or("336699"), 16).unwrap_or(0x336699);
    let h1 = (h % 360) as i64;
    let h2 = ((h / 360) % 360) as i64;
    let svg = format!(
        r##"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 800 800'>
<defs>
  <radialGradient id='g' cx='30%' cy='25%' r='90%'>
    <stop offset='0%' stop-color='hsl({h1},58%,42%)'/>
    <stop offset='65%' stop-color='hsl({h2},35%,16%)'/>
    <stop offset='100%' stop-color='#0A0A0A'/>
  </radialGradient>
  <filter id='n'><feTurbulence baseFrequency='0.9' numOctaves='2' seed='{s}'/>
    <feColorMatrix values='0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0.06 0'/>
    <feComposite in2='SourceGraphic' operator='in'/></filter>
</defs>
<rect width='800' height='800' fill='url(#g)'/>
<rect width='800' height='800' filter='url(#n)' opacity='0.6'/>
<text x='50%' y='52%' text-anchor='middle' fill='rgba(255,255,255,0.9)'
  font-family='Helvetica Neue,Arial' font-size='52' letter-spacing='10' font-weight='200'>MU × YOU</text>
<text x='50%' y='60%' text-anchor='middle' fill='rgba(255,255,255,0.55)'
  font-family='Helvetica Neue,Arial' font-size='18' letter-spacing='6'>{name}</text>
<text x='50%' y='66%' text-anchor='middle' fill='rgba(255,255,255,0.25)'
  font-family='monospace' font-size='10' letter-spacing='4'>seed:{s2}</text>
</svg>"##,
        h1 = h1, h2 = h2,
        s = (h % 100) as i64,
        s2 = &seed[..seed.len().min(8)],
        name = name.replace('<', "").replace('>', ""),
    );
    format!("data:image/svg+xml;utf8,{}", urlencoded(&svg))
}

fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'~'
            | b'/' | b':' | b',' | b';' | b' '
            | b'(' | b')' | b'\'' | b'!'
            | b'=' | b'&' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Ensure today's design row exists. Returns (design_id, needs_generation).
/// `needs_generation = true` when the caller should kick off a Gemini task
/// (new row, refresh, or a previous attempt that failed). Existing successful
/// rows are returned untouched for idempotent daily polling.
fn ensure_design_for_day(conn: &Connection, user_id: i64, day: &str, taste: &serde_json::Value, force_refresh: bool)
    -> rusqlite::Result<(i64, bool)>
{
    let existing: Option<(i64, i64, String)> = conn.query_row(
        "SELECT id, refresh_count, gen_status FROM you_designs WHERE user_id=? AND day=?",
        params![user_id, day],
        |r| Ok((r.get::<_,i64>(0)?, r.get::<_,i64>(1)?, r.get::<_,String>(2)?)),
    ).ok();

    if let Some((id, refresh_count, gen_status)) = existing {
        if !force_refresh {
            // Re-kick generation only if a prior attempt failed and nothing is
            // currently running; never re-kick a 'ready' row.
            let needs = gen_status == "failed";
            if needs {
                conn.execute(
                    "UPDATE you_designs SET gen_status='generating', gen_error=NULL, updated_at=?
                     WHERE id=?",
                    params![chrono_now(), id],
                )?;
            }
            return Ok((id, needs));
        }
        // refresh: bump count, regenerate name/prompt/seed/image
        let new_count = refresh_count + 1;
        let (name, prompt, seed) = compose_design(taste, day, new_count);
        let svg_fallback = render_design_svg(&name, &seed);
        conn.execute(
            "UPDATE you_designs
             SET name=?, prompt=?, seed=?, image_url=?, image_bytes=NULL, image_mime=NULL,
                 gen_status='generating', gen_error=NULL,
                 refresh_count=?, updated_at=?
             WHERE id=?",
            params![name, prompt, seed, svg_fallback, new_count, chrono_now(), id],
        )?;
        return Ok((id, true));
    }

    // Compute day_num for this user
    let day_num: i64 = conn.query_row(
        "SELECT COALESCE(MAX(day_num), 0) + 1 FROM you_designs WHERE user_id=?",
        params![user_id],
        |r| r.get(0),
    ).unwrap_or(1);

    // Merge the user's style_name (Day-7 ritual) AND learned preferences
    // (from you_signals last 14 days) into the taste so compose_design /
    // compose_special_design can bend the prompt toward what this user likes.
    let mut taste_with_style = taste.clone();
    if let Some(obj) = taste_with_style.as_object_mut() {
        let style_name: Option<String> = conn.query_row(
            "SELECT style_name FROM you_users WHERE id=?",
            params![user_id], |r| r.get(0),
        ).ok().flatten();
        if let Some(sn) = style_name {
            obj.insert("style_name".into(), serde_json::Value::String(sn));
        }
        // Inject tend_toward / avoid as arrays of strings
        let prefs = compute_user_preferences(conn, user_id);
        let tend: Vec<serde_json::Value> = prefs.get("tend_toward").and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.get("token").cloned()).collect())
            .unwrap_or_default();
        let avoid: Vec<serde_json::Value> = prefs.get("avoid").and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.get("token").cloned()).collect())
            .unwrap_or_default();
        if !tend.is_empty() {
            obj.insert("tend_toward".into(), serde_json::Value::Array(tend));
        }
        if !avoid.is_empty() {
            obj.insert("avoid".into(), serde_json::Value::Array(avoid));
        }
    }
    let taste_for_prompt = &taste_with_style;

    // Day 14 / 30 use a special composition (peak / The 30). Other days fall
    // through to compose_design.
    let (name, prompt, seed) = match compose_special_design(taste_for_prompt, day, day_num) {
        Some(triple) => triple,
        None => compose_design(taste_for_prompt, day, 0),
    };
    let svg_fallback = render_design_svg(&name, &seed);
    let now = chrono_now();
    conn.execute(
        "INSERT INTO you_designs
         (user_id, day, day_num, name, prompt, seed, image_url, gen_status, status,
          refresh_count, created_at, updated_at)
         VALUES (?,?,?,?,?,?,?,'generating','pending',0,?,?)",
        params![user_id, day, day_num, name, prompt, seed, svg_fallback, now, now],
    )?;
    Ok((conn.last_insert_rowid(), true))
}

/// Spawn a background task that calls Gemini 3 Pro Image, writes the bytes
/// back to the row, and emails the subscriber a "your design is ready" link.
///
/// Image bytes live as BLOB in the SQLite database at `DB_PATH`, which is
/// `/data/products.db` in production — that path is the Fly volume
/// `mu_store_data` (see fly.toml). The volume persists across deploys and
/// machine restarts, so generated images survive forever unless explicitly
/// deleted. Re-deploys never wipe them.
/// Render the 4-emoji reaction row for daily emails. Each link hits
/// /r/:design_id/:kind?t=<token> — one tap fires the signal endpoint.
fn email_reaction_row(design_id: i64, token: &str) -> String {
    let buttons = [
        ("love", "🔥 大好き"),
        ("ok",   "👍 良い"),
        ("meh",  "😐 微妙"),
        ("skip", "👋 Skip"),
    ];
    let cells: String = buttons.iter().map(|(k, label)| {
        format!(
            r##"<a href="https://wearmu.com/r/{id}/{k}?t={t}" style="display:inline-block;background:rgba(230,196,73,0.08);border:1px solid rgba(230,196,73,0.25);color:#F5F5F0;padding:11px 14px;margin:4px;font-size:12px;letter-spacing:0.04em;text-decoration:none;border-radius:2px">{label}</a>"##,
            id = design_id, k = k, t = token, label = label,
        )
    }).collect();
    format!(
        r##"<div style="margin:18px 0 8px"><div style="font-size:9px;letter-spacing:0.25em;text-transform:uppercase;opacity:0.55;margin-bottom:8px">この一着、どう？ (1 タップ)</div>{cells}</div>"##,
        cells = cells,
    )
}

fn spawn_gemini_for_design(db: Db, design_id: i64) {
    tokio::spawn(async move {
        let row = {
            let conn = db.lock().unwrap();
            conn.query_row(
                "SELECT d.name, d.prompt, d.seed, d.day_num, u.taste_json,
                        u.email, u.slug, u.token
                 FROM you_designs d JOIN you_users u ON u.id = d.user_id
                 WHERE d.id=?",
                params![design_id],
                |r| Ok((
                    r.get::<_,String>(0)?,
                    r.get::<_,String>(1)?,
                    r.get::<_,String>(2)?,
                    r.get::<_,i64>(3)?,
                    r.get::<_,String>(4)?,
                    r.get::<_,String>(5)?,
                    r.get::<_,Option<String>>(6)?,
                    r.get::<_,String>(7)?,
                )),
            ).ok()
        };
        let (name, prompt, seed, day_num, taste_json, email, slug, token) = match row {
            Some(v) => v,
            None => {
                eprintln!("[you/gemini] design {} disappeared", design_id);
                return;
            }
        };
        let taste: serde_json::Value =
            serde_json::from_str(&taste_json).unwrap_or(serde_json::json!({}));
        let pull_strs = |k: &str| -> Vec<String> {
            taste.get(k).and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
                .unwrap_or_default()
        };
        let mood = pull_strs("mood");
        let palette = pull_strs("palette");
        let scene = pull_strs("scene");
        let bio = taste.get("bio").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let tee = gemini::TeeDesign {
            name: &name, prompt: &prompt, seed: &seed,
            mood: &mood, palette: &palette, scene: &scene,
            bio: &bio,
        };
        match gemini::generate_tee(&tee).await {
            Ok(g) => {
                let bytes_len = g.bytes.len();
                {
                    let conn = db.lock().unwrap();
                    let url = format!("/api/you/design/{}/image.png", design_id);
                    let r = conn.execute(
                        "UPDATE you_designs
                         SET image_bytes=?, image_mime=?, image_url=?,
                             gen_status='ready', gen_error=NULL, updated_at=?
                         WHERE id=?",
                        params![g.bytes, g.mime, url, chrono_now(), design_id],
                    );
                    if let Err(e) = r {
                        eprintln!("[you/gemini] failed to persist design {}: {}", design_id, e);
                        return;
                    }
                }
                eprintln!("[you/gemini] design {} ready ({} bytes)", design_id, bytes_len);

                // Notify subscriber
                let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
                if !resend_key.is_empty() {
                    let base_url = env::var("BASE_URL")
                        .unwrap_or_else(|_| "https://wearmu.com".into());
                    let base = base_url.trim_end_matches('/');
                    let img_url = format!("{}/api/you/design/{}/image.png", base, design_id);
                    let share = slug.as_ref()
                        .map(|s| format!("{}/{}", base, s))
                        .unwrap_or_else(|| format!("{}/you", base));
                    let html = format!(r#"
<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:32px 0;margin:0">
  <div style="max-width:600px;margin:0 auto;padding:0 32px">
    <div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:32px">MU × YOU</div>
    <div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#e6c449;opacity:0.85;margin-bottom:8px">DAY {day_num:03}</div>
    <div style="font-size:24px;font-weight:200;line-height:1.4;margin-bottom:8px">{name}</div>
    <p style="font-size:12px;line-height:1.85;opacity:0.7;margin-bottom:24px;font-style:italic;border-left:2px solid #e6c449;padding-left:14px">{prompt}</p>
    <img src="{img}" alt="MU × YOU DAY {day_num}" style="width:100%;display:block;background:#1a1a1a;border-radius:2px;margin-bottom:24px">
    <a href="{share}" style="display:inline-block;background:#e6c449;color:#000;padding:16px 28px;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;text-decoration:none;font-weight:700;margin-right:8px">この一着を仕立てる →</a>
    <a href="{share}" style="display:inline-block;border:1px solid rgba(255,255,255,0.2);color:#F5F5F0;padding:15px 22px;font-size:10px;letter-spacing:0.25em;text-transform:uppercase;text-decoration:none;opacity:0.7">明日に期待 / Skip</a>
    {reactions}
    <p style="font-size:10px;opacity:0.45;margin-top:32px;line-height:1.7">
      気分が変わったら <a href="{share}" style="color:#e6c449">プロンプトを書き直す</a>こともできます。<br>
      退会は <code>STOP</code> 返信、または /you ページから即時。
    </p>
  </div>
</div>"#,
                        day_num = day_num, name = name, prompt = prompt,
                        img = img_url, share = share,
                        reactions = email_reaction_row(design_id, &token));
                    let _ = reqwest::Client::new()
                        .post("https://api.resend.com/emails")
                        .bearer_auth(&resend_key)
                        .json(&serde_json::json!({
                            "from": "MU × YOU <noreply@wearmu.com>",
                            "to": [email],
                            "subject": you_email_subject(
                                &{ let c = db.lock().unwrap();
                                   cv_get(&c, "email_subject_variant", "loss") },
                                "daily",
                                &serde_json::json!({"day_num": day_num, "name": &name}),
                            ),
                            "html": html,
                        }))
                        .send().await;
                }
            }
            Err(e) => {
                eprintln!("[you/gemini] design {} failed: {}", design_id, e);
                let conn = db.lock().unwrap();
                let _ = conn.execute(
                    "UPDATE you_designs
                     SET gen_status='failed', gen_error=?, updated_at=?
                     WHERE id=?",
                    params![e, chrono_now(), design_id],
                );
            }
        }
    });
}

fn design_to_json(conn: &Connection, id: i64) -> Option<serde_json::Value> {
    conn.query_row(
        "SELECT id, day, day_num, name, prompt, seed, image_url, status, size,
                refresh_count, gen_status
         FROM you_designs WHERE id=?",
        params![id],
        |r| Ok(serde_json::json!({
            "id": r.get::<_,i64>(0)?,
            "day": r.get::<_,String>(1)?,
            "day_num": r.get::<_,i64>(2)?,
            "name": r.get::<_,String>(3)?,
            "prompt": r.get::<_,String>(4)?,
            "seed": r.get::<_,String>(5)?,
            "image_url": format!("/api/you/design/{}/image.png", r.get::<_,i64>(0)?),
            "image_url_fallback": r.get::<_,Option<String>>(6)?,
            "status": r.get::<_,String>(7)?,
            "size": r.get::<_,Option<String>>(8)?.unwrap_or_else(|| "S".into()),
            "refresh_count": r.get::<_,i64>(9)?,
            "gen_status": r.get::<_,String>(10)?,
            "price_jpy": 6800,
            "valid_label": "24h",
        })),
    ).ok()
}

async fn you_subscribe(
    State(db): State<Db>,
    Json(body): Json<YouSubscribeBody>,
) -> impl IntoResponse {
    let email = body.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') || email.len() > 200 {
        return (StatusCode::BAD_REQUEST, "invalid email").into_response();
    }
    let size = if body.size.is_empty() { "S".to_string() } else { body.size.clone() };
    if !["XS","S","M","L","XL","XXL"].contains(&size.as_str()) {
        return (StatusCode::BAD_REQUEST, "invalid size").into_response();
    }
    let taste = serde_json::json!({
        "email": email,
        "mood": body.mood, "palette": body.palette, "scene": body.scene,
        "size": size, "bio": body.bio,
    });

    let now = chrono_now();
    let day = jst_today_str();

    let (token, user_id, today_design_id, needs_gen) = {
        let conn = db.lock().unwrap();

        // Upsert user
        let existing: Option<(i64, String)> = conn.query_row(
            "SELECT id, token FROM you_users WHERE email=?",
            params![email],
            |r| Ok((r.get::<_,i64>(0)?, r.get::<_,String>(1)?)),
        ).ok();

        let (uid, tk) = match existing {
            Some((uid, tk)) => {
                // Returning subscriber: refresh taste, never extend trial here
                // (re-signup must not reset the 30-day window).
                conn.execute(
                    "UPDATE you_users SET taste_json=?, size=?, updated_at=?, unsubscribed_at=NULL
                     WHERE id=?",
                    params![taste.to_string(), size, now, uid],
                ).ok();
                (uid, tk)
            }
            None => {
                let tk = uuid::Uuid::new_v4().to_string().replace('-', "");
                // Try a few random slugs in case of collision
                let mut sl = random_slug();
                for _ in 0..5 {
                    let exists: bool = conn.query_row(
                        "SELECT 1 FROM you_users WHERE slug=?",
                        params![sl], |_| Ok(true),
                    ).unwrap_or(false);
                    if !exists { break; }
                    sl = random_slug();
                }
                let trial_end = trial_end_seconds_from_now();
                conn.execute(
                    "INSERT INTO you_users (email, token, slug, taste_json, size, created_at, updated_at, trial_end_at)
                     VALUES (?,?,?,?,?,?,?,?)",
                    params![email, tk, sl, taste.to_string(), size, now, now, trial_end],
                ).ok();
                let uid = conn.last_insert_rowid();
                // Referral capture: tag the new user with the inviter's slug
                // (validated against existing you_users.slug). On the new
                // user's first MU purchase the webhook will credit the
                // inviter +¥3,400.
                if let Some(ref_slug) = body.ref_slug.as_deref() {
                    let valid: bool = conn.query_row(
                        "SELECT 1 FROM you_users WHERE slug=? AND unsubscribed_at IS NULL",
                        params![ref_slug], |_| Ok(true),
                    ).unwrap_or(false);
                    if valid && ref_slug != sl {
                        conn.execute(
                            "UPDATE you_users SET referred_by_slug=? WHERE id=?",
                            params![ref_slug, uid],
                        ).ok();
                    }
                }
                // If this email has previously bought any MU shirt, grant
                // lifetime_free immediately. Lookup is cheap thanks to
                // idx_mu_purchases_email.
                let prior: Option<(i64, String, i64)> = conn.query_row(
                    "SELECT product_id, brand, COALESCE(drop_num,0)
                     FROM mu_purchases WHERE email=? ORDER BY id DESC LIMIT 1",
                    params![email],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                ).ok();
                if let Some((_, brand, drop_num)) = prior {
                    let reason = format!("retro: previously purchased {} #{}", brand.to_uppercase(), drop_num);
                    conn.execute(
                        "UPDATE you_users SET lifetime_free=1, lifetime_reason=? WHERE id=?",
                        params![reason, uid],
                    ).ok();
                }
                (uid, tk)
            }
        };

        // Generate today's design (idempotent per (user, day))
        let (did, needs_gen) = match ensure_design_for_day(&conn, uid, &day, &taste, false) {
            Ok((id, needs)) => (id, needs),
            Err(e) => {
                eprintln!("[you] ensure_design failed for user {}: {}", uid, e);
                (0, false)
            }
        };
        (tk, uid, did, needs_gen)
    };
    if needs_gen && today_design_id > 0 {
        spawn_gemini_for_design(db.clone(), today_design_id);
    }

    // Send welcome via Resend (fire-and-forget)
    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    if !resend_key.is_empty() {
        let to = email.clone();
        let tk = token.clone();
        let subject_variant = you_subject_variant(&db);
        tokio::spawn(async move {
            let html = format!(r#"
<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:48px;max-width:560px;margin:0 auto">
  <div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:32px">MU × YOU</div>
  <div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#e6c449;opacity:0.85;margin-bottom:8px">Welcome, Day 001</div>
  <div style="font-size:18px;font-weight:300;line-height:1.5;margin-bottom:24px">明朝 9:00 から、毎日 1 案、あなた専用のTシャツデザインが届きます。</div>
  <p style="font-size:12px;line-height:1.85;opacity:0.65">
    本日の最初の案はもう生成されています。下のボタンから今すぐ確認できます。気に入ったらその一着を仕立て、合わなかったら Skip。Skip するほど明日の案があなたに寄っていきます。
  </p>
  <div style="margin-top:24px;padding:16px 20px;background:#1C1C1C;border-left:2px solid #e6c449;font-size:11px;line-height:1.85;opacity:0.85">
    <strong>無料トライアルは 30 日間。</strong><br>
    その間に MU の T シャツを 1 着でも手に入れていただければ、MU × YOU は <strong>一生無料</strong>になります。
  </div>
  <a href="https://wearmu.com/you?t={tk}" style="display:inline-block;margin-top:32px;background:#F5F5F0;color:#0A0A0A;padding:14px 28px;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;text-decoration:none;font-weight:700">
    本日の案を見る →
  </a>
  <p style="font-size:10px;opacity:0.4;margin-top:32px;line-height:1.7;letter-spacing:0.05em">
    退会は <code>STOP</code> と返信、またはこのリンクから即時実行されます。<br>MU — wearmu.com
  </p>
</div>"#, tk = tk);
            let _ = reqwest::Client::new()
                .post("https://api.resend.com/emails")
                .bearer_auth(&resend_key)
                .json(&serde_json::json!({
                    "from": "MU × YOU <noreply@wearmu.com>",
                    "to": [to],
                    "subject": you_email_subject(&subject_variant, "welcome", &serde_json::json!({})),
                    "html": html,
                }))
                .send().await;
        });
    }

    // Build response payload
    let conn = db.lock().unwrap();
    let today_id: Option<i64> = conn.query_row(
        "SELECT id FROM you_designs WHERE user_id=? AND day=?",
        params![user_id, day],
        |r| r.get(0),
    ).ok();
    let today = today_id.and_then(|id| design_to_json(&conn, id));
    let history = list_history(&conn, user_id);
    let slug: Option<String> = conn.query_row(
        "SELECT slug FROM you_users WHERE id=?",
        params![user_id], |r| r.get(0),
    ).ok();
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    let share_url = slug.as_ref().map(|s|
        format!("{}/{}", base_url.trim_end_matches('/'), s));

    let (trial_end_at, lifetime_free, sub_status, sub_until):
        (Option<String>, i64, Option<String>, Option<String>) = conn.query_row(
        "SELECT trial_end_at, COALESCE(lifetime_free,0), subscription_status, subscription_until
         FROM you_users WHERE id=?",
        params![user_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    ).unwrap_or((None, 0, None, None));
    let subscription = you_user_state_full(
        trial_end_at.as_deref(), lifetime_free != 0,
        sub_status.as_deref(), sub_until.as_deref(),
    );
    Json(serde_json::json!({
        "ok": true,
        "token": token,
        "today": today,
        "history": history,
        "slug": slug,
        "share_url": share_url,
        "subscription": subscription,
    })).into_response()
}

fn list_history(conn: &Connection, user_id: i64) -> Vec<serde_json::Value> {
    let mut stmt = match conn.prepare(
        "SELECT id, day, day_num, name, status, seed, gen_status
         FROM you_designs WHERE user_id=? ORDER BY day DESC LIMIT 30"
    ) {
        Ok(s) => s, Err(_) => return vec![],
    };
    stmt.query_map(params![user_id], |r| {
        let id = r.get::<_,i64>(0)?;
        Ok(serde_json::json!({
        "id": id,
        "day": r.get::<_,String>(1)?,
        "day_num": r.get::<_,i64>(2)?,
        "name": r.get::<_,String>(3)?,
        "image_url": format!("/api/you/design/{}/image.png", id),
        "status": r.get::<_,String>(4)?,
        "seed": r.get::<_,String>(5)?,
        "gen_status": r.get::<_,String>(6)?,
    }))})
    .map(|it| it.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

async fn you_daily(
    State(db): State<Db>,
    Path(token): Path<String>,
) -> impl IntoResponse {
    let day = jst_today_str();
    let (uid, needs_gen, gen_id) = {
        let conn = db.lock().unwrap();
        let row: Option<(i64, String)> = conn.query_row(
            "SELECT id, taste_json FROM you_users
             WHERE token=? AND unsubscribed_at IS NULL",
            params![token],
            |r| Ok((r.get::<_,i64>(0)?, r.get::<_,String>(1)?)),
        ).ok();
        let (uid, taste_json) = match row {
            Some(v) => v,
            None => return (StatusCode::NOT_FOUND, "invalid token").into_response(),
        };
        let taste: serde_json::Value =
            serde_json::from_str(&taste_json).unwrap_or(serde_json::json!({}));
        let (id, needs) = match ensure_design_for_day(&conn, uid, &day, &taste, false) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[you] ensure_design (daily) failed: {}", e);
                (0, false)
            }
        };
        (uid, needs, id)
    };
    if needs_gen && gen_id > 0 {
        spawn_gemini_for_design(db.clone(), gen_id);
    }
    let conn = db.lock().unwrap();
    let today_id: Option<i64> = conn.query_row(
        "SELECT id FROM you_designs WHERE user_id=? AND day=?",
        params![uid, day],
        |r| r.get(0),
    ).ok();
    let today = today_id.and_then(|id| design_to_json(&conn, id));
    let history = list_history(&conn, uid);
    let user_meta: Option<(Option<String>, String)> = conn.query_row(
        "SELECT slug, taste_json FROM you_users WHERE id=?",
        params![uid], |r| Ok((r.get(0)?, r.get(1)?)),
    ).ok();
    let (slug, taste) = match user_meta {
        Some((s, tj)) => (
            s,
            serde_json::from_str::<serde_json::Value>(&tj).unwrap_or(serde_json::json!({}))
        ),
        None => (None, serde_json::json!({})),
    };
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    let share_url = slug.as_ref().map(|s|
        format!("{}/{}", base_url.trim_end_matches('/'), s));
    let (trial_end_at, lifetime_free): (Option<String>, i64) = conn.query_row(
        "SELECT trial_end_at, COALESCE(lifetime_free,0) FROM you_users WHERE id=?",
        params![uid], |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap_or((None, 0));
    let subscription = you_user_state(trial_end_at.as_deref(), lifetime_free != 0);
    Json(serde_json::json!({
        "ok": true,
        "slug": slug,
        "share_url": share_url,
        "taste": taste,
        "today": today,
        "history": history,
        "subscription": subscription,
    })).into_response()
}

async fn you_feedback(
    State(db): State<Db>,
    Json(body): Json<YouFeedbackBody>,
) -> impl IntoResponse {
    let outcome: Result<(Option<serde_json::Value>, Option<i64>), Response> = {
        let conn = db.lock().unwrap();
        let row: Option<(i64, String)> = conn.query_row(
            "SELECT u.id, u.taste_json
             FROM you_users u
             WHERE u.token=? AND u.unsubscribed_at IS NULL",
            params![body.token],
            |r| Ok((r.get::<_,i64>(0)?, r.get::<_,String>(1)?)),
        ).ok();
        let (uid, taste_json) = match row {
            Some(v) => v,
            None => return (StatusCode::NOT_FOUND, "invalid token").into_response(),
        };
        let design_user: Option<(i64, String)> = conn.query_row(
            "SELECT user_id, day FROM you_designs WHERE id=?",
            params![body.design_id],
            |r| Ok((r.get::<_,i64>(0)?, r.get::<_,String>(1)?)),
        ).ok();
        let (owner_id, day) = match design_user {
            Some(v) => v,
            None => return (StatusCode::NOT_FOUND, "design not found").into_response(),
        };
        if owner_id != uid {
            return (StatusCode::FORBIDDEN, "not your design").into_response();
        }

        let now = chrono_now();
        match body.action.as_str() {
            "skip" | "like" => {
                conn.execute(
                    "UPDATE you_designs SET status=?, updated_at=? WHERE id=?",
                    params![body.action, now, body.design_id],
                ).ok();
                conn.execute(
                    "INSERT INTO you_feedback (user_id, design_id, action, created_at) VALUES (?,?,?,?)",
                    params![uid, body.design_id, body.action, now],
                ).ok();
                Ok((None, None))
            }
            "refresh" => {
                let cnt: i64 = conn.query_row(
                    "SELECT refresh_count FROM you_designs WHERE id=?",
                    params![body.design_id],
                    |r| r.get(0),
                ).unwrap_or(0);
                if cnt >= 1 {
                    return (StatusCode::TOO_MANY_REQUESTS,
                        "refresh limit reached for today").into_response();
                }
                let taste: serde_json::Value =
                    serde_json::from_str(&taste_json).unwrap_or(serde_json::json!({}));
                let gen_id = match ensure_design_for_day(&conn, uid, &day, &taste, true) {
                    Ok((id, _needs)) => id,
                    Err(e) => {
                        eprintln!("[you] refresh failed: {}", e);
                        return (StatusCode::INTERNAL_SERVER_ERROR, "refresh failed").into_response();
                    }
                };
                let after = design_to_json(&conn, body.design_id);
                Ok((after, Some(gen_id)))
            }
            _ => return (StatusCode::BAD_REQUEST, "unknown action").into_response(),
        }
    };
    let (today_after, refresh_id) = match outcome {
        Ok(v) => v,
        Err(r) => return r,
    };
    if let Some(id) = refresh_id {
        spawn_gemini_for_design(db.clone(), id);
    }
    Json(serde_json::json!({
        "ok": true,
        "today": today_after,
    })).into_response()
}

// ── ¥980/月 paid subscription tier ──────────────────────────────────────────

#[derive(Deserialize)]
struct YouSubscribePaidBody {
    token: String,
}

/// POST /api/you/subscribe-3mo — one-time ¥2,940 = ¥980 × 3 prepaid pack.
/// Stripe Checkout mode=payment with metadata.plan=3mo. The webhook extends
/// subscription_until by 90 days when paid. For users who want a finite cap.
async fn you_subscribe_3mo(
    State(db): State<Db>,
    Json(body): Json<YouSubscribePaidBody>,
) -> impl IntoResponse {
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    if stripe_key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "checkout disabled").into_response();
    }
    let (user_id, email, price_jpy): (i64, String, i64) = {
        let conn = db.lock().unwrap();
        let row: Option<(i64, String)> = conn.query_row(
            "SELECT id, email FROM you_users
             WHERE token=? AND unsubscribed_at IS NULL",
            params![body.token], |r| Ok((r.get(0)?, r.get(1)?)),
        ).ok();
        let (u, e) = match row {
            Some(v) => v,
            None => return (StatusCode::NOT_FOUND, "invalid token").into_response(),
        };
        let p: i64 = cv_get(&conn, "pack_3mo_price_jpy", "2500").parse().unwrap_or(2500);
        (u, e, p.clamp(300, 29_400))
    };
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());

    let form: Vec<(&str, String)> = vec![
        ("mode", "payment".into()),
        ("currency", "jpy".into()),
        ("customer_email", email),
        ("allow_promotion_codes", "true".into()),
        ("success_url", format!("{}/you?paid=3mo-ok", base_url)),
        ("cancel_url",  format!("{}/you?paid=cancel", base_url)),
        ("line_items[0][quantity]", "1".into()),
        ("line_items[0][price_data][currency]", "jpy".into()),
        ("line_items[0][price_data][unit_amount]", price_jpy.to_string()),
        ("line_items[0][price_data][product_data][name]",
         format!("MU × YOU — 3 ヶ月パック ¥{} (¥980 × 3、自動更新なし)", price_jpy)),
        ("metadata[you_user_id]", user_id.to_string()),
        ("metadata[plan]", "3mo".into()),
    ];

    let resp = reqwest::Client::new()
        .post("https://api.stripe.com/v1/checkout/sessions")
        .basic_auth(&stripe_key, None::<&str>)
        .form(&form)
        .send().await;
    match resp {
        Ok(r) if r.status().is_success() => {
            let j: serde_json::Value = r.json().await.unwrap_or_default();
            let url = j["url"].as_str().unwrap_or("/").to_string();
            Json(serde_json::json!({"url": url, "price_jpy": price_jpy})).into_response()
        }
        Ok(r) => {
            let s = r.status();
            let t = r.text().await.unwrap_or_default();
            eprintln!("[you/subscribe-3mo] stripe {}: {}", s, t);
            (StatusCode::BAD_GATEWAY, "stripe error").into_response()
        }
        Err(e) => {
            eprintln!("[you/subscribe-3mo] reqwest: {}", e);
            (StatusCode::BAD_GATEWAY, "stripe network").into_response()
        }
    }
}

/// POST /api/you/subscribe-paid — start the ¥980/月 plan for the token's
/// account. Returns a Stripe Checkout URL in subscription mode.
async fn you_subscribe_paid(
    State(db): State<Db>,
    Json(body): Json<YouSubscribePaidBody>,
) -> impl IntoResponse {
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    if stripe_key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "checkout disabled").into_response();
    }
    let (user_id, email, customer_id, price_jpy): (i64, String, Option<String>, i64) = {
        let conn = db.lock().unwrap();
        let row: Option<(i64, String, Option<String>)> = conn.query_row(
            "SELECT id, email, stripe_customer_id FROM you_users
             WHERE token=? AND unsubscribed_at IS NULL",
            params![body.token], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).ok();
        let (u, e, c) = match row {
            Some(v) => v,
            None => return (StatusCode::NOT_FOUND, "invalid token").into_response(),
        };
        let p: i64 = cv_get(&conn, "monthly_price_jpy", "1480").parse().unwrap_or(1480);
        (u, e, c, p.clamp(100, 9_980))
    };
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());

    let mut form: Vec<(&str, String)> = vec![
        ("mode", "subscription".into()),
        ("currency", "jpy".into()),
        ("customer_email", email.clone()),
        ("allow_promotion_codes", "true".into()),
        ("success_url", format!("{}/you?paid=ok", base_url)),
        ("cancel_url", format!("{}/you?paid=cancel", base_url)),
        ("line_items[0][quantity]", "1".into()),
        ("line_items[0][price_data][currency]", "jpy".into()),
        ("line_items[0][price_data][unit_amount]", price_jpy.to_string()),
        ("line_items[0][price_data][recurring][interval]", "month".into()),
        ("line_items[0][price_data][product_data][name]",
         format!("MU × YOU — 月額 ¥{} (毎朝 1 案、いつでも解約)", price_jpy)),
        ("metadata[you_user_id]", user_id.to_string()),
        ("subscription_data[metadata][you_user_id]", user_id.to_string()),
    ];
    // Reuse existing Stripe customer if we have one so the portal works seamlessly.
    if let Some(cid) = customer_id.as_deref() {
        if !cid.is_empty() {
            // Stripe Checkout: if customer is set, omit customer_email.
            form.retain(|(k, _)| *k != "customer_email");
            form.push(("customer", cid.into()));
        }
    }

    let resp = reqwest::Client::new()
        .post("https://api.stripe.com/v1/checkout/sessions")
        .basic_auth(&stripe_key, None::<&str>)
        .form(&form)
        .send().await;
    match resp {
        Ok(r) if r.status().is_success() => {
            let j: serde_json::Value = r.json().await.unwrap_or_default();
            let url = j["url"].as_str().unwrap_or("/").to_string();
            Json(serde_json::json!({"url": url, "price_jpy": price_jpy})).into_response()
        }
        Ok(r) => {
            let s = r.status();
            let t = r.text().await.unwrap_or_default();
            eprintln!("[you/subscribe-paid] stripe {}: {}", s, t);
            (StatusCode::BAD_GATEWAY, "stripe error").into_response()
        }
        Err(e) => {
            eprintln!("[you/subscribe-paid] reqwest: {}", e);
            (StatusCode::BAD_GATEWAY, "stripe network").into_response()
        }
    }
}

#[derive(Deserialize)]
struct YouPortalBody {
    token: String,
}

/// POST /api/you/portal — return a Stripe billing-portal session for the
/// user to manage / cancel their subscription.
async fn you_portal(
    State(db): State<Db>,
    Json(body): Json<YouPortalBody>,
) -> impl IntoResponse {
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    if stripe_key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "portal disabled").into_response();
    }
    let customer_id: String = {
        let conn = db.lock().unwrap();
        match conn.query_row(
            "SELECT stripe_customer_id FROM you_users
             WHERE token=? AND unsubscribed_at IS NULL",
            params![body.token], |r| r.get::<_, Option<String>>(0),
        ).ok().flatten() {
            Some(c) if !c.is_empty() => c,
            _ => return (StatusCode::NOT_FOUND, "no Stripe customer yet — start a subscription first").into_response(),
        }
    };
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    let resp = reqwest::Client::new()
        .post("https://api.stripe.com/v1/billing_portal/sessions")
        .basic_auth(&stripe_key, None::<&str>)
        .form(&[
            ("customer", customer_id.as_str()),
            ("return_url", format!("{}/you", base_url).as_str()),
        ])
        .send().await;
    match resp {
        Ok(r) if r.status().is_success() => {
            let j: serde_json::Value = r.json().await.unwrap_or_default();
            Json(serde_json::json!({"url": j["url"].as_str().unwrap_or("/")})).into_response()
        }
        Ok(r) => {
            let t = r.text().await.unwrap_or_default();
            eprintln!("[you/portal] stripe {}", t);
            (StatusCode::BAD_GATEWAY, "stripe error").into_response()
        }
        Err(e) => {
            eprintln!("[you/portal] reqwest: {}", e);
            (StatusCode::BAD_GATEWAY, "stripe network").into_response()
        }
    }
}

/// Webhook helper — invoked from stripe_webhook on subscription events.
fn handle_subscription_event(db: &Db, event_type: &str, event: &serde_json::Value) {
    let obj = &event["data"]["object"];
    // The subscription object lives at .object for customer.subscription.*.
    // For checkout.session.completed (mode=subscription) the subscription id
    // is at .subscription and we need to fetch it; but we already record the
    // mapping in the dedicated checkout branch — here we update only when
    // we see customer.subscription.*.
    let sub_id = obj["id"].as_str().unwrap_or("").to_string();
    let customer_id = obj["customer"].as_str().unwrap_or("").to_string();
    if sub_id.is_empty() || customer_id.is_empty() { return; }
    let status = obj["status"].as_str().unwrap_or("").to_string();
    let period_end: i64 = obj["current_period_end"].as_i64().unwrap_or(0);
    let until_str = if period_end > 0 { period_end.to_string() } else { String::new() };

    let conn = db.lock().unwrap();
    // Try to locate the user by stripe_customer_id OR by metadata.you_user_id.
    let user_id: Option<i64> = conn.query_row(
        "SELECT id FROM you_users WHERE stripe_customer_id=?",
        params![customer_id], |r| r.get(0),
    ).ok().or_else(|| {
        let from_meta = obj["metadata"]["you_user_id"].as_str()
            .and_then(|x| x.parse::<i64>().ok());
        from_meta
    });
    let Some(uid) = user_id else {
        eprintln!("[stripe/subscription] no /you user for customer={}", customer_id);
        return;
    };
    let _ = conn.execute(
        "UPDATE you_users
         SET stripe_customer_id = COALESCE(stripe_customer_id, ?),
             stripe_subscription_id = ?,
             subscription_status = ?,
             subscription_until = CASE WHEN ?<>'' THEN ? ELSE subscription_until END
         WHERE id=?",
        params![customer_id, sub_id, status, until_str, until_str, uid],
    );
    if event_type == "customer.subscription.deleted" {
        // No daily emails on canceled subs once subscription_until passes.
        let _ = conn.execute(
            "UPDATE you_users SET subscription_status='canceled' WHERE id=?",
            params![uid],
        );
    }
    eprintln!("[/you] subscription {} {} for user {}", sub_id, status, uid);
}

/// Backfill mu_purchases from Stripe history (paid checkout sessions) so
/// people who bought MU shirts BEFORE the mu_purchases table existed also
/// get retroactive /you lifetime_free. Idempotent on session id.
/// Returns counts: scanned, new_purchases, granted_lifetime.
async fn admin_backfill_purchases(
    State(db): State<Db>,
    Json(body): Json<YouAdminBackfillBody>,
) -> impl IntoResponse {
    if let Err(r) = require_admin_token(Some(&body.admin_token)) { return r; }
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    if stripe_key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "no STRIPE_SECRET_KEY").into_response();
    }
    let client = reqwest::Client::new();
    let mut scanned: i64 = 0;
    let mut new_purchases: i64 = 0;
    let mut starting_after: Option<String> = None;
    // Walk all checkout sessions, max 50 pages (5000 sessions) to be safe.
    for _page in 0..50 {
        let mut form: Vec<(&str, String)> = vec![
            ("limit", "100".into()),
            ("expand[]", "data.customer_details".into()),
        ];
        if let Some(s) = &starting_after {
            form.push(("starting_after", s.clone()));
        }
        let resp = client
            .get("https://api.stripe.com/v1/checkout/sessions")
            .basic_auth(&stripe_key, None::<&str>)
            .query(&form)
            .send().await;
        let json: serde_json::Value = match resp {
            Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
            Ok(r) => {
                eprintln!("[backfill] stripe {}: {}", r.status(), r.text().await.unwrap_or_default());
                break;
            }
            Err(e) => { eprintln!("[backfill] reqwest: {}", e); break; }
        };
        let data = json["data"].as_array().cloned().unwrap_or_default();
        if data.is_empty() { break; }
        for s in &data {
            scanned += 1;
            // Only count paid sessions
            let paid = s["payment_status"].as_str() == Some("paid");
            if !paid { continue; }
            let session_id = s["id"].as_str().unwrap_or("");
            if session_id.is_empty() { continue; }
            let buyer_email = s["customer_details"]["email"].as_str()
                .or_else(|| s["customer_email"].as_str())
                .unwrap_or("").to_lowercase();
            if buyer_email.is_empty() { continue; }
            let meta = &s["metadata"];
            // Either path:
            // - MU drop with metadata.product_id (MUGEN/MUON/MA)
            // - /you design with metadata.you_design_id
            let product_id: i64 = meta["product_id"].as_str()
                .and_then(|x| x.parse().ok()).unwrap_or(0);
            let you_design_id: i64 = meta["you_design_id"].as_str()
                .and_then(|x| x.parse().ok()).unwrap_or(0);
            if product_id == 0 && you_design_id == 0 { continue; }

            // Resolve brand + drop_num
            let (brand, drop_num) = if product_id != 0 {
                let conn = db.lock().unwrap();
                conn.query_row(
                    "SELECT brand, drop_num FROM products WHERE id=?",
                    params![product_id], |r| Ok((r.get::<_,String>(0)?, r.get::<_,i64>(1)?)),
                ).unwrap_or_else(|_| (String::new(), 0))
            } else {
                ("you".to_string(),
                 meta["you_day_num"].as_str().and_then(|x| x.parse().ok()).unwrap_or(0))
            };

            let inserted = {
                let conn = db.lock().unwrap();
                let created: i64 = s["created"].as_i64().unwrap_or_else(|| chrono_now().parse().unwrap_or(0));
                conn.execute(
                    "INSERT INTO mu_purchases (email, product_id, brand, drop_num, session_id, created_at)
                     SELECT ?, ?, ?, ?, ?, ?
                     WHERE NOT EXISTS (SELECT 1 FROM mu_purchases WHERE session_id=?)",
                    params![
                        buyer_email, if product_id != 0 { product_id } else { you_design_id },
                        brand, drop_num, session_id, created.to_string(), session_id,
                    ],
                ).unwrap_or(0)
            };
            if inserted > 0 { new_purchases += 1; }
        }
        let has_more = json["has_more"].as_bool().unwrap_or(false);
        if !has_more { break; }
        starting_after = data.last().and_then(|s| s["id"].as_str().map(String::from));
        if starting_after.is_none() { break; }
    }

    // Grant lifetime_free to every you_user whose email appears in mu_purchases.
    let granted: i64 = {
        let conn = db.lock().unwrap();
        conn.execute(
            "UPDATE you_users SET lifetime_free=1,
                lifetime_reason = COALESCE(lifetime_reason,
                  (SELECT 'retro: previously purchased ' || upper(brand) || ' #' || drop_num
                   FROM mu_purchases p WHERE p.email = you_users.email ORDER BY p.id LIMIT 1))
             WHERE lifetime_free=0
               AND EXISTS (SELECT 1 FROM mu_purchases p WHERE p.email = you_users.email)",
            [],
        ).unwrap_or(0) as i64
    };

    Json(serde_json::json!({
        "ok": true, "scanned": scanned, "new_purchases": new_purchases,
        "granted_lifetime": granted,
    })).into_response()
}

// ── User-signal stream (auto-tunes compose_design) ──────────────────────────

#[derive(Deserialize)]
struct YouSignalBody {
    #[serde(default)] token: String,
    kind: String,
    #[serde(default)] weight: Option<i64>,
    #[serde(default)] source: Option<String>,
}

fn signal_weight_for(kind: &str, override_w: Option<i64>) -> i64 {
    if let Some(w) = override_w { return w.clamp(-5, 5); }
    match kind {
        "love"         =>  3,
        "claim_intent" =>  3,
        "share"        =>  2,
        "ok"           =>  1,
        "dwell"        =>  1,
        "meh"          => -1,
        "skip"         => -2,
        _              =>  1,
    }
}

/// POST /api/you/signal/:design_id — record a reaction. token-auth maps to a
/// user; anonymous slug-page visitors hit this without a token (we record
/// user_id=0 in that case).
async fn you_signal(
    State(db): State<Db>,
    Path(design_id): Path<i64>,
    Json(body): Json<YouSignalBody>,
) -> impl IntoResponse {
    let allowed = ["love","ok","meh","skip","claim_intent","share","dwell"];
    if !allowed.contains(&body.kind.as_str()) {
        return (StatusCode::BAD_REQUEST, "bad kind").into_response();
    }
    let w = signal_weight_for(&body.kind, body.weight);
    let source = body.source.unwrap_or_else(|| "page".into());

    // Look up user_id from token. Anonymous (slug-page visitors) → 0.
    let user_id: i64 = if body.token.is_empty() { 0 } else {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT u.id FROM you_users u
             JOIN you_designs d ON d.user_id = u.id
             WHERE u.token=? AND d.id=?",
            params![body.token, design_id], |r| r.get(0),
        ).unwrap_or(0)
    };

    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO you_signals (user_id, design_id, kind, weight, source, created_at)
             VALUES (?,?,?,?,?,?)",
            params![user_id, design_id, body.kind, w, source, chrono_now()],
        ).ok();
        // Convenience: a 'skip' / 'claim_intent' also flips you_designs.status.
        if body.kind == "skip" {
            conn.execute(
                "UPDATE you_designs SET status='skip', updated_at=? WHERE id=? AND status<>'claimed'",
                params![chrono_now(), design_id],
            ).ok();
        }
    }
    Json(serde_json::json!({"ok": true, "kind": body.kind, "weight": w})).into_response()
}

/// GET /r/:design_id/:kind?t=<token> — one-tap reaction from email buttons.
/// Returns a tiny thank-you page (no JS, no POST). Idempotent within 10 min
/// per (user, design, kind).
async fn you_signal_email(
    State(db): State<Db>,
    Path((design_id, kind)): Path<(i64, String)>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let allowed = ["love","ok","meh","skip"];
    if !allowed.contains(&kind.as_str()) {
        return (StatusCode::BAD_REQUEST, Html("invalid".to_string())).into_response();
    }
    let token = q.get("t").cloned().unwrap_or_default();
    let w = signal_weight_for(&kind, None);

    let (user_id, name): (i64, String) = if token.is_empty() {
        (0, "this one".to_string())
    } else {
        let conn = db.lock().unwrap();
        let row: Option<(i64, String)> = conn.query_row(
            "SELECT u.id, d.name FROM you_users u
             JOIN you_designs d ON d.user_id = u.id
             WHERE u.token=? AND d.id=?",
            params![token, design_id], |r| Ok((r.get(0)?, r.get(1)?)),
        ).ok();
        match row { Some((u, n)) => (u, n), None => (0, "this one".into()) }
    };
    {
        let conn = db.lock().unwrap();
        let already: i64 = conn.query_row(
            "SELECT COUNT(*) FROM you_signals
             WHERE user_id=? AND design_id=? AND kind=?
               AND CAST(created_at AS INTEGER) > CAST(? AS INTEGER) - 600",
            params![user_id, design_id, kind, chrono_now()], |r| r.get(0),
        ).unwrap_or(0);
        if already == 0 {
            conn.execute(
                "INSERT INTO you_signals (user_id, design_id, kind, weight, source, created_at)
                 VALUES (?,?,?,?, 'email', ?)",
                params![user_id, design_id, kind, w, chrono_now()],
            ).ok();
            if kind == "skip" {
                conn.execute(
                    "UPDATE you_designs SET status='skip', updated_at=? WHERE id=? AND status<>'claimed'",
                    params![chrono_now(), design_id],
                ).ok();
            }
        }
    }
    let label = match kind.as_str() {
        "love" => "🔥 大好き", "ok" => "👍 良い", "meh" => "😐 微妙", "skip" => "👋 Skip",
        _ => "—",
    };
    let html = format!(r##"<!doctype html><html lang="ja"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{label} — MU × YOU</title>
<style>
body{{background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue','Hiragino Sans',Arial,sans-serif;margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;padding:24px;-webkit-font-smoothing:antialiased}}
.card{{max-width:440px;width:100%;text-align:center;padding:48px 32px;border:1px solid rgba(230,196,73,0.25);background:#111;border-radius:2px}}
.eye{{font-size:10px;letter-spacing:0.3em;text-transform:uppercase;color:#e6c449;opacity:0.85;margin-bottom:16px}}
h1{{font-size:22px;font-weight:300;letter-spacing:0.04em;margin:0 0 14px;line-height:1.5}}
p{{font-size:13px;line-height:1.9;opacity:0.75;margin:0 0 18px}}
a{{color:#e6c449;text-decoration:underline}}
</style></head>
<body><div class="card">
  <div class="eye">フィードバック受領</div>
  <h1>{label}</h1>
  <p>「{name}」へのリアクションを記録しました。<br>明日以降の生成に反映されます。</p>
  <a href="https://wearmu.com/you?t={token}">あなたのページに戻る →</a>
</div></body></html>"##, label = label, name = html_escape(&name), token = html_escape(&token));
    Html(html).into_response()
}

#[derive(Deserialize)]
struct YouPrefsQuery {
    #[serde(default)] token: String,
}

/// GET /api/you/preferences — show what the AI has learned from this user's
/// signals. Used by /you dashboard to make the feedback loop visible.
async fn you_preferences(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<YouPrefsQuery>,
) -> impl IntoResponse {
    if q.token.is_empty() { return (StatusCode::BAD_REQUEST, "missing token").into_response(); }
    let conn = db.lock().unwrap();
    let user_id: i64 = match conn.query_row(
        "SELECT id FROM you_users WHERE token=? AND unsubscribed_at IS NULL",
        params![q.token], |r| r.get(0),
    ).ok() { Some(v) => v, None => return (StatusCode::NOT_FOUND, "invalid token").into_response() };
    let prefs = compute_user_preferences(&conn, user_id);
    Json(prefs).into_response()
}

/// Read the user's signals from the last 14 days and tally weight by the
/// token (mood/palette/scene/noun) used in each design's prompt + name.
/// Returns the top "tend toward" and "avoid" tokens — these are folded into
/// the next compose_design call.
fn compute_user_preferences(conn: &Connection, user_id: i64) -> serde_json::Value {
    let mut stmt = match conn.prepare(
        "SELECT d.name, d.prompt, s.weight
         FROM you_signals s JOIN you_designs d ON d.id = s.design_id
         WHERE s.user_id = ? AND CAST(s.created_at AS INTEGER) > CAST(? AS INTEGER) - 14 * 86400"
    ) { Ok(s) => s, Err(_) => return serde_json::json!({}) };
    let rows: Vec<(String, String, i64)> = stmt.query_map(
        params![user_id, chrono_now()],
        |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,i64>(2)?)),
    ).map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default();

    // Token bank — same lexicon compose_design picks from
    let nouns = [
        "霧","余白","ノイズ","回路","層","ふち","島","橋","残響","解像度",
        "層雲","北限","薄明","残光","水位","素描","点描","くずし","湾","ふもと",
    ];
    let palettes_seed = ["墨","白","海","土","炭","金","群青","茜"];
    let moods_seed = ["静か","余白","力強い","ノスタルジック","ミニマル","遊び",
                      "深い","朝の光","夜の余韻","幾何学","手書き","写真的"];

    use std::collections::HashMap;
    let mut score: HashMap<String, i64> = HashMap::new();
    for (name, prompt, w) in &rows {
        let blob = format!("{} {}", name, prompt);
        let mut counted = false;
        for tok in nouns.iter().chain(palettes_seed.iter()).chain(moods_seed.iter()) {
            if blob.contains(tok) {
                *score.entry((*tok).to_string()).or_insert(0) += *w;
                counted = true;
            }
        }
        let _ = counted;
    }
    let mut tend: Vec<(String, i64)> = score.iter()
        .filter(|(_, w)| **w >= 1)
        .map(|(k, v)| (k.clone(), *v)).collect();
    let mut avoid: Vec<(String, i64)> = score.iter()
        .filter(|(_, w)| **w <= -1)
        .map(|(k, v)| (k.clone(), *v)).collect();
    tend.sort_by(|a, b| b.1.cmp(&a.1));
    avoid.sort_by(|a, b| a.1.cmp(&b.1));
    tend.truncate(5);
    avoid.truncate(5);
    serde_json::json!({
        "tend_toward": tend.iter().map(|(k,w)| serde_json::json!({"token":k, "weight":w})).collect::<Vec<_>>(),
        "avoid":       avoid.iter().map(|(k,w)| serde_json::json!({"token":k, "weight":w})).collect::<Vec<_>>(),
        "signal_count": rows.len() as i64,
    })
}

// ── CV autonomous pulse ──────────────────────────────────────────────────────

/// Public config endpoint — the LP / exit-funnel script reads variant choices
/// here so cv_pulse can adjust UX without a redeploy.
async fn cv_public_config(State(db): State<Db>) -> impl IntoResponse {
    let mut out = serde_json::Map::new();
    {
        let conn = db.lock().unwrap();
        let mut stmt = match conn.prepare("SELECT key, value FROM cv_config") {
            Ok(s) => s,
            Err(_) => return Json(serde_json::Value::Object(out)).into_response(),
        };
        let it = stmt.query_map([], |r| Ok::<_, rusqlite::Error>((r.get::<_, String>(0)?, r.get::<_, String>(1)?)));
        if let Ok(rows) = it {
            for row in rows.flatten() {
                out.insert(row.0, serde_json::Value::String(row.1));
            }
        }
    }
    let mut headers = HeaderMap::new();
    headers.insert("Cache-Control", HeaderValue::from_static("public, max-age=60"));
    (headers, Json(serde_json::Value::Object(out))).into_response()
}

fn count_since(conn: &Connection, table: &str, ts_col: &str, secs_ago: i64) -> i64 {
    let now: i64 = chrono_now().parse().unwrap_or(0);
    let cutoff = now - secs_ago;
    // ts_col is a unix-epoch string column. Cast to int for comparison.
    let q = format!(
        "SELECT COUNT(*) FROM {} WHERE CAST({} AS INTEGER) >= ?",
        table, ts_col
    );
    conn.query_row(&q, params![cutoff], |r| r.get::<_, i64>(0)).unwrap_or(0)
}

fn count_offers_since(conn: &Connection, kind: &str, secs_ago: i64) -> i64 {
    let now: i64 = chrono_now().parse().unwrap_or(0);
    let cutoff = now - secs_ago;
    conn.query_row(
        "SELECT COUNT(*) FROM exit_offers
         WHERE kind=? AND CAST(created_at AS INTEGER) >= ?",
        params![kind, cutoff], |r| r.get::<_, i64>(0),
    ).unwrap_or(0)
}

fn count_purchases_since(conn: &Connection, secs_ago: i64) -> i64 {
    let now: i64 = chrono_now().parse().unwrap_or(0);
    let cutoff = now - secs_ago;
    conn.query_row(
        "SELECT COUNT(*) FROM mu_purchases WHERE CAST(created_at AS INTEGER) >= ?",
        params![cutoff], |r| r.get::<_, i64>(0),
    ).unwrap_or(0)
}

fn cv_set(conn: &Connection, key: &str, value: &str, reason: &str) {
    conn.execute(
        "INSERT INTO cv_config (key, value, updated_at, reason) VALUES (?,?,?,?)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at, reason=excluded.reason",
        params![key, value, chrono_now(), reason],
    ).ok();
}

fn cv_get(conn: &Connection, key: &str, default: &str) -> String {
    conn.query_row(
        "SELECT value FROM cv_config WHERE key=?",
        params![key],
        |r| r.get::<_, String>(0),
    ).unwrap_or_else(|_| default.to_string())
}

/// Read the active /you email-subject variant. Wrap the lock so callers
/// outside an existing critical section don't have to.
fn you_subject_variant(db: &Db) -> String {
    let conn = db.lock().unwrap();
    cv_get(&conn, "email_subject_variant", "loss")
}

/// Pick the subject line for a /you email kind using the active variant.
/// cv_pulse rotates the variant so we can find the highest-CTR phrasing
/// without redeploying.
fn you_email_subject(variant: &str, kind: &str, ctx: &serde_json::Value) -> String {
    let day_num = ctx.get("day_num").and_then(|v| v.as_i64()).unwrap_or(0);
    let name = ctx.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let days_left = ctx.get("days_left").and_then(|v| v.as_i64()).unwrap_or(0);
    let days_done = (30 - days_left).max(0);
    match (kind, variant) {
        ("daily", "loss") => format!("MU × YOU DAY {:03} — 24 時間で消えます", day_num),
        ("daily", "curiosity") => format!("DAY {:03} — 今日のあなたは「{}」", day_num, name),
        ("daily", _) => format!("MU × YOU DAY {:03} — {}", day_num, name),

        ("welcome", "loss") => "30 日後に消える、あなただけの 30 案 — 配信開始".into(),
        ("welcome", "curiosity") => "30 日 / 30 案 — どんな自分が布になる？".into(),
        ("welcome", _) => "MU × YOU — 明朝 9 時から毎日デザインが届きます".into(),

        ("trial5d", "loss") => format!("あと {} 日であなたの {} 案が消えます", days_left.max(1), days_done),
        ("trial5d", "curiosity") => format!("残り {} 日 — 仕立てる一着、決まった？", days_left.max(1)),
        ("trial5d", _) => format!("MU × YOU — トライアル残り {} 日、MU 1 着で永久 ¥0", days_left.max(1)),

        ("trial_end", "loss") => "MU × YOU — トライアル終了。29 案が消えました。".into(),
        ("trial_end", "curiosity") => "30 日が終わり、あなたは何を持ち帰る？".into(),
        ("trial_end", _) => "MU × YOU — トライアル終了。続けるには MU を 1 着。".into(),

        _ => format!("MU × YOU — {}", name),
    }
}

/// /api/admin/cv_pulse — called every 30 min by cron. Snapshots metrics,
/// applies adjustment rules, persists decisions, posts a digest to Telegram.
async fn cv_pulse(
    State(db): State<Db>,
    Json(body): Json<YouAdminBackfillBody>,
) -> impl IntoResponse {
    if let Err(r) = require_admin_token(Some(&body.admin_token)) { return r; }

    // ── 1. Pull metrics ──
    let (signups_30m, signups_24h, signups_total,
         surveys_30m, surveys_24h,
         lottery_30m, lottery_24h,
         discounts_30m, discounts_24h,
         purchases_30m, purchases_24h) = {
        let conn = db.lock().unwrap();
        let signups_30m = count_since(&conn, "you_users", "created_at", 1800);
        let signups_24h = count_since(&conn, "you_users", "created_at", 86400);
        let signups_total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM you_users WHERE unsubscribed_at IS NULL",
            [], |r| r.get(0),
        ).unwrap_or(0);
        let surveys_30m = count_since(&conn, "exit_surveys", "created_at", 1800);
        let surveys_24h = count_since(&conn, "exit_surveys", "created_at", 86400);
        let lottery_30m = count_offers_since(&conn, "lottery_entry", 1800);
        let lottery_24h = count_offers_since(&conn, "lottery_entry", 86400);
        let discounts_30m = count_offers_since(&conn, "discount_50", 1800);
        let discounts_24h = count_offers_since(&conn, "discount_50", 86400);
        let purchases_30m = count_purchases_since(&conn, 1800);
        let purchases_24h = count_purchases_since(&conn, 86400);
        (signups_30m, signups_24h, signups_total,
         surveys_30m, surveys_24h,
         lottery_30m, lottery_24h,
         discounts_30m, discounts_24h,
         purchases_30m, purchases_24h)
    };

    // ── 2. Apply adjustment rules ──
    // Read current settings first
    let (prev_cooldown, prev_pct, prev_subject, prev_scroll) = {
        let conn = db.lock().unwrap();
        (
            cv_get(&conn, "modal_cooldown_hours", "24"),
            cv_get(&conn, "coupon_percent_off", "50"),
            cv_get(&conn, "email_subject_variant", "loss"),
            cv_get(&conn, "modal_scroll_required", "1"),
        )
    };
    let mut decisions: Vec<String> = Vec::new();

    {
        let conn = db.lock().unwrap();

        // Rule 1: signups in last 24h drives modal aggressiveness.
        // < 2 signups → make modal more aggressive (12h cooldown, no scroll-required)
        // 2-9 → default (24h cooldown, scroll required)
        // ≥ 10 → ease off (48h, scroll required)
        let target_cooldown = if signups_24h < 2 { "12" }
            else if signups_24h >= 10 { "48" } else { "24" };
        if target_cooldown != prev_cooldown {
            cv_set(&conn, "modal_cooldown_hours", target_cooldown,
                &format!("signups_24h={}", signups_24h));
            decisions.push(format!("modal_cooldown_hours {} → {}", prev_cooldown, target_cooldown));
        }
        let target_scroll = if signups_24h < 2 { "0" } else { "1" };
        if target_scroll != prev_scroll {
            cv_set(&conn, "modal_scroll_required", target_scroll,
                &format!("signups_24h={}", signups_24h));
            decisions.push(format!("modal_scroll_required {} → {}", prev_scroll, target_scroll));
        }

        // Rule 2: coupon strength based on conversion drought.
        // No purchases in 24h AND no discounts redeemed → boost to 60%.
        // Any purchase in 24h → relax back to 50%.
        let target_pct = if purchases_24h == 0 && signups_24h >= 5 { "60" }
            else { "50" };
        if target_pct != prev_pct {
            cv_set(&conn, "coupon_percent_off", target_pct,
                &format!("purchases_24h={} signups_24h={}", purchases_24h, signups_24h));
            decisions.push(format!("coupon_percent_off {}% → {}%", prev_pct, target_pct));
        }

        // Rule 3: rotate email subject variant if signups stalled for 48h.
        // signups_24h == 0 + we've been "loss" framing → try "curiosity"
        let target_subj = if signups_24h == 0 && prev_subject == "loss" { "curiosity" }
            else if signups_24h >= 5 && prev_subject != "loss" { "loss" }
            else { prev_subject.as_str() };
        if target_subj != prev_subject {
            cv_set(&conn, "email_subject_variant", target_subj,
                &format!("rotate (signups_24h={})", signups_24h));
            decisions.push(format!("email_subject_variant {} → {}", prev_subject, target_subj));
        }

        // Rule 4: rotate hero CTA variant when stalled. variants cycle through
        // value → identity → loss. Stalled = 0 signups in 24h.
        let prev_cta = cv_get(&conn, "hero_cta_variant", "value");
        let target_cta = if signups_24h == 0 {
            match prev_cta.as_str() {
                "value" => "identity",
                "identity" => "loss",
                _ => "value",
            }
        } else { prev_cta.as_str() };
        if target_cta != prev_cta {
            cv_set(&conn, "hero_cta_variant", target_cta,
                &format!("rotate (signups_24h={})", signups_24h));
            decisions.push(format!("hero_cta_variant {} → {}", prev_cta, target_cta));
        }
    }
    let decision_str = if decisions.is_empty() { "no-change".to_string() } else { decisions.join(", ") };

    // ── 3. Persist pulse row ──
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO cv_pulses (at, signups_30m, signups_24h,
              surveys_30m, surveys_24h, lottery_30m, lottery_24h,
              discounts_30m, discounts_24h, purchases_30m, purchases_24h,
              decision, notes)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                chrono_now(), signups_30m, signups_24h,
                surveys_30m, surveys_24h, lottery_30m, lottery_24h,
                discounts_30m, discounts_24h, purchases_30m, purchases_24h,
                decision_str, format!("subscribers={}", signups_total),
            ],
        ).ok();
    }

    // ── 3.5 Cron freshness check ──
    // MUGEN must drop ≤ every 2h, MUON ≤ 30h, MA ≤ 35d. If any brand exceeds
    // its budget we flag it loudly so the operator notices a stuck cron.
    let stale_warnings: Vec<String> = {
        let conn = db.lock().unwrap();
        cron_health_warnings(&conn)
    };

    // ── 4. Telegram digest (best-effort, fire and forget) ──
    let tg_token = env::var("TELEGRAM_BOT_TOKEN").unwrap_or_default();
    let tg_chat = env::var("TELEGRAM_CHAT_ID").unwrap_or_else(|_| "1136442501".into());
    let should_alert_stale = !stale_warnings.is_empty();
    if !tg_token.is_empty() && (should_alert_stale || !decisions.is_empty() || signups_30m > 0 || purchases_30m > 0) {
        let header = if should_alert_stale {
            format!("🚨 MU CV pulse · STALE · {}\n{}\n", jst_today_str(), stale_warnings.join("\n"))
        } else {
            format!("MU CV pulse · {}\n", jst_today_str())
        };
        let msg = format!(
            "{}\
             ─ signups 30m/24h: {} / {}  (total {})\n\
             ─ surveys 30m/24h: {} / {}\n\
             ─ lottery 30m/24h: {} / {}\n\
             ─ discounts 30m/24h: {} / {}\n\
             ─ purchases 30m/24h: {} / {}\n\
             ─ decision: {}",
            header,
            signups_30m, signups_24h, signups_total,
            surveys_30m, surveys_24h,
            lottery_30m, lottery_24h,
            discounts_30m, discounts_24h,
            purchases_30m, purchases_24h,
            decision_str,
        );
        let token_for_tg = tg_token.clone();
        let chat_for_tg = tg_chat.clone();
        let msg_for_tg = msg.clone();
        tokio::spawn(async move {
            let _ = reqwest::Client::new()
                .post(format!("https://api.telegram.org/bot{}/sendMessage", token_for_tg))
                .json(&serde_json::json!({
                    "chat_id": chat_for_tg, "text": msg_for_tg,
                    "disable_web_page_preview": true,
                }))
                .send().await;
        });
    }

    Json(serde_json::json!({
        "ok": true,
        "metrics": {
            "signups_30m": signups_30m, "signups_24h": signups_24h, "total": signups_total,
            "surveys_30m": surveys_30m, "surveys_24h": surveys_24h,
            "lottery_30m": lottery_30m, "lottery_24h": lottery_24h,
            "discounts_30m": discounts_30m, "discounts_24h": discounts_24h,
            "purchases_30m": purchases_30m, "purchases_24h": purchases_24h,
        },
        "decision": decision_str,
        "decisions": decisions,
    })).into_response()
}

/// Compute "minutes since the most recent active drop" for each brand and
/// flag any brand that has exceeded its cadence budget. Returns one warning
/// line per stale brand; empty if all healthy.
fn cron_health_warnings(conn: &rusqlite::Connection) -> Vec<String> {
    let budgets: &[(&str, i64, &str)] = &[
        ("mugen", 120,   "hourly"),
        ("muon",  1800,  "daily"),     // 30h
        ("ma",    10080, "weekly"),    // 7d (was 50400 = 35d, monthly cadence — now weekly)
        ("nouns", 10080, "weekly"),    // 7d
    ];
    let now_secs = chrono_now().parse::<i64>().unwrap_or(0);
    let mut warnings = Vec::new();
    for &(brand, budget_min, cadence) in budgets {
        let row: rusqlite::Result<String> = conn.query_row(
            "SELECT MAX(
                CASE
                  WHEN created_at GLOB '[0-9]*' AND created_at NOT LIKE '%-%'
                    THEN strftime('%Y-%m-%dT%H:%M:%SZ', CAST(created_at AS INTEGER), 'unixepoch')
                  ELSE created_at
                END
             ) FROM products WHERE brand=? AND active=1",
            params![brand], |r| r.get::<_, Option<String>>(0).map(|o| o.unwrap_or_default()),
        );
        let latest_iso = match row { Ok(s) if !s.is_empty() => s, _ => continue };
        let latest_secs = iso_to_unix_secs(&latest_iso).unwrap_or(0);
        if latest_secs == 0 { continue; }
        let elapsed_min = (now_secs - latest_secs) / 60;
        if elapsed_min > budget_min {
            warnings.push(format!(
                "  {} ({}): last drop {}h{}m ago — budget {}h",
                brand.to_uppercase(), cadence,
                elapsed_min / 60, elapsed_min % 60,
                budget_min / 60,
            ));
        }
    }
    warnings
}

/// Parse "YYYY-MM-DDTHH:MM:SSZ" into unix seconds. None on failure.
fn iso_to_unix_secs(iso: &str) -> Option<i64> {
    let s = iso.trim_end_matches('Z');
    let (date_part, time_part) = s.split_once('T')?;
    let date_bits: Vec<&str> = date_part.split('-').collect();
    if date_bits.len() != 3 { return None; }
    let y: i64 = date_bits[0].parse().ok()?;
    let m: i64 = date_bits[1].parse().ok()?;
    let d: i64 = date_bits[2].parse().ok()?;
    let time_bits: Vec<&str> = time_part.split(':').collect();
    if time_bits.len() < 3 { return None; }
    let hh: i64 = time_bits[0].parse().ok()?;
    let mm: i64 = time_bits[1].parse().ok()?;
    let ss: i64 = time_bits[2].split('.').next()?.parse().ok()?;
    // days_from_civil — Howard Hinnant
    let y_adj = if m <= 2 { y - 1 } else { y };
    let era = if y_adj >= 0 { y_adj } else { y_adj - 399 } / 400;
    let yoe = y_adj - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

// ── Autonomous Treasury — MU が自分の口座を見て予算を決める ─────────────
// Solana wallet (MU_TREASURY_PUBKEY env, default = Enabler treasury) の
// SOL / USDC 残高を取得し、JPY 換算と AI 配分提案を返す。
// 用途:
//   - 広告予算上限の自動算出 (cv_pulse の延長)
//   - 感謝クーポン総額の上限管理
//   - grant pool (MA Council が採択する将来枠)
// このエンドポイントは <em>公開</em>。透明性ブランドの一環。
const MU_TREASURY_DEFAULT_PUBKEY: &str = "DK29rBGCvP83LUNjUGVM6xt6qPy6rycBFopXbFkg9XvQ";
const SOLANA_USDC_MINT_ADDR: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

async fn fetch_solana_balances(pubkey: &str) -> Result<(f64, f64), String> {
    // SOL native balance + USDC SPL token balance via public Solana RPC.
    let rpc = env::var("SOLANA_RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".into());
    let client = reqwest::Client::new();

    // 1. native SOL
    let sol_req = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "getBalance",
        "params": [pubkey, {"commitment": "confirmed"}]
    });
    let sol_resp = client.post(&rpc).json(&sol_req).send().await
        .map_err(|e| format!("rpc sol: {e}"))?;
    let sol_j: serde_json::Value = sol_resp.json().await.map_err(|e| format!("json sol: {e}"))?;
    let lamports = sol_j["result"]["value"].as_u64().unwrap_or(0);
    let sol = (lamports as f64) / 1_000_000_000.0;

    // 2. SPL token (USDC) — use getTokenAccountsByOwner filtered by mint
    let usdc_req = serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "getTokenAccountsByOwner",
        "params": [
            pubkey,
            {"mint": SOLANA_USDC_MINT_ADDR},
            {"encoding": "jsonParsed", "commitment": "confirmed"}
        ]
    });
    let usdc_resp = client.post(&rpc).json(&usdc_req).send().await
        .map_err(|e| format!("rpc usdc: {e}"))?;
    let usdc_j: serde_json::Value = usdc_resp.json().await.map_err(|e| format!("json usdc: {e}"))?;
    let mut usdc = 0f64;
    if let Some(accs) = usdc_j["result"]["value"].as_array() {
        for a in accs {
            if let Some(amt) = a["account"]["data"]["parsed"]["info"]["tokenAmount"]["uiAmount"]
                .as_f64() { usdc += amt; }
        }
    }
    Ok((sol, usdc))
}

#[derive(Deserialize)]
struct TreasuryQuery {
    /// optional override; defaults to MU_TREASURY_PUBKEY env or constant
    #[serde(default)] pubkey: Option<String>,
}

async fn public_treasury(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<TreasuryQuery>,
) -> impl IntoResponse {
    let pk = q.pubkey
        .or_else(|| env::var("MU_TREASURY_PUBKEY").ok())
        .unwrap_or_else(|| MU_TREASURY_DEFAULT_PUBKEY.to_string());

    let (sol, usdc) = match fetch_solana_balances(&pk).await {
        Ok(v) => v,
        Err(e) => return Json(serde_json::json!({"ok": false, "error": e, "pubkey": pk})).into_response(),
    };

    // FX env (set daily by ops cron). Conservative defaults.
    let jpy_per_sol: f64 = env::var("JPY_PER_SOL").ok()
        .and_then(|s| s.parse().ok()).filter(|x: &f64| x.is_finite() && *x > 0.0).unwrap_or(25_000.0);
    let jpy_per_usd: f64 = env::var("JPY_PER_USD").ok()
        .and_then(|s| s.parse().ok()).filter(|x: &f64| x.is_finite() && *x > 0.0).unwrap_or(150.0);

    let jpy_total = (sol * jpy_per_sol + usdc * jpy_per_usd) as i64;

    // Real revenue this calendar month (cs_live_*)
    let revenue_30d: i64 = {
        let conn = db.lock().unwrap();
        let cutoff: i64 = chrono_now().parse::<i64>().unwrap_or(0) - 30 * 86_400;
        conn.query_row(
            "SELECT COALESCE(SUM(p.price_jpy),0) FROM mu_purchases mp
             JOIN products p ON p.id = mp.product_id
             WHERE mp.session_id LIKE 'cs_live_%'
               AND CAST(mp.created_at AS INTEGER) >= ?",
            params![cutoff], |r| r.get(0),
        ).unwrap_or(0)
    };

    // AI 予算配分の提案 (heuristics, transparent):
    //   - Ads (Google Ads): ≤ 30% of treasury or ≤ revenue_30d * 0.5, whichever smaller
    //   - Thanks coupon reserve: ≤ 10% of treasury
    //   - Grant pool (MA Council 採択用): ≤ 10% of treasury
    //   - Runway: 残り = ops 固定費 ¥6,000/月 + buffer
    let ads_budget    = ((jpy_total as f64) * 0.30).min((revenue_30d as f64) * 0.50) as i64;
    let thanks_budget = ((jpy_total as f64) * 0.10) as i64;
    let grant_pool    = ((jpy_total as f64) * 0.10) as i64;
    let monthly_burn  = 6_000_i64;
    let runway_months = if monthly_burn > 0 { jpy_total / monthly_burn } else { 0 };

    // Pending Stripe payout (next payout to bank). Best-effort, non-fatal.
    let stripe_pending = fetch_stripe_balance_pending().await.unwrap_or(0);
    // Treasury auto-charge plan: 15% of next payout suggested → SOL/USDC
    let charge_plan = (stripe_pending as f64 * 0.15) as i64;

    Json(serde_json::json!({
        "ok": true,
        "pubkey": pk,
        "balance": {
            "sol":  sol,
            "usdc": usdc,
            "jpy_estimate": jpy_total,
        },
        "fx": {"jpy_per_sol": jpy_per_sol, "jpy_per_usd": jpy_per_usd},
        "revenue_30d_jpy": revenue_30d,
        "stripe": {
            "pending_payout_jpy": stripe_pending,
            "charge_plan_jpy": charge_plan,
            "note": "次回 Stripe payout の 15% を Solana wallet にチャージする計画値。実行は手動 (法定→暗号資産の自動変換は法令上不可)。",
        },
        "ai_budget_suggestion": {
            "ads_monthly_jpy":    ads_budget,
            "thanks_reserve_jpy": thanks_budget,
            "grant_pool_jpy":     grant_pool,
            "monthly_burn_jpy":   monthly_burn,
            "runway_months":      runway_months,
        },
        "note": "本ヒューリスティクスは公開・改変可能。実際の支出は admin 承認 (cv_pulse) を経て実行される。",
        "as_of": chrono_now(),
    })).into_response()
}

/// Best-effort: ask Stripe for the available balance (pending → bank).
/// Returns JPY total; 0 on any failure (logged to stderr).
async fn fetch_stripe_balance_pending() -> Result<i64, String> {
    let key = match env::var("STRIPE_SECRET_KEY") {
        Ok(v) if !v.is_empty() => v,
        _ => return Ok(0),
    };
    let resp = reqwest::Client::new()
        .get("https://api.stripe.com/v1/balance")
        .basic_auth(&key, None::<&str>)
        .send().await
        .map_err(|e| format!("stripe balance net: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("stripe balance {}", resp.status()));
    }
    let j: serde_json::Value = resp.json().await.map_err(|e| format!("stripe balance json: {e}"))?;
    // Sum pending JPY amounts. Stripe returns minor units; JPY is already a major unit (no cents).
    let mut total = 0i64;
    if let Some(arr) = j["pending"].as_array() {
        for p in arr {
            if p["currency"].as_str() == Some("jpy") {
                total += p["amount"].as_i64().unwrap_or(0);
            }
        }
    }
    // Also count available (not yet paid out)
    if let Some(arr) = j["available"].as_array() {
        for p in arr {
            if p["currency"].as_str() == Some("jpy") {
                total += p["amount"].as_i64().unwrap_or(0);
            }
        }
    }
    Ok(total)
}

/// Public health endpoint — returns which brands are stale.
async fn cron_health_handler(State(db): State<Db>) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let warnings = cron_health_warnings(&conn);
    let healthy = warnings.is_empty();
    Json(serde_json::json!({
        "ok": healthy,
        "stale": warnings,
    }))
}

/// `/api/transparency` — public stats for the blog. Honest, computed live,
/// no caching. If a number is wrong on the blog it's wrong here too.
async fn public_transparency(State(db): State<Db>) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let revenue_shirts_jpy: i64 = conn.query_row(
        "SELECT COALESCE(SUM(price_jpy * sold), 0) FROM products WHERE active=1",
        [], |r| r.get(0),
    ).unwrap_or(0);
    let auction_revenue_jpy: i64 = conn.query_row(
        "SELECT COALESCE(SUM(current_bid), 0) FROM products WHERE brand='ma' AND sold>=1",
        [], |r| r.get(0),
    ).unwrap_or(0);
    let shirts_sold: i64 = conn.query_row(
        "SELECT COALESCE(SUM(sold), 0) FROM products WHERE active=1",
        [], |r| r.get(0),
    ).unwrap_or(0);
    let purchases_total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM mu_purchases", [], |r| r.get(0),
    ).unwrap_or(0);

    // Real revenue: only count rows that have an actual Stripe session_id
    // (session_id LIKE 'cs_live_%' or 'cs_test_%' minus tests). Best-effort
    // until we record amount_total on mu_purchases.
    let real_revenue_jpy: i64 = conn.query_row(
        "SELECT COALESCE(SUM(p.price_jpy), 0)
         FROM mu_purchases mp
         JOIN products p ON p.id = mp.product_id
         WHERE mp.session_id LIKE 'cs_live_%'",
        [], |r| r.get(0),
    ).unwrap_or(0);
    let real_purchases: i64 = conn.query_row(
        "SELECT COUNT(*) FROM mu_purchases WHERE session_id LIKE 'cs_live_%'",
        [], |r| r.get(0),
    ).unwrap_or(0);
    let you_subscribers_total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM you_users WHERE unsubscribed_at IS NULL",
        [], |r| r.get(0),
    ).unwrap_or(0);
    let you_subscribers_paid: i64 = conn.query_row(
        "SELECT COUNT(*) FROM you_users WHERE subscription_status='active'",
        [], |r| r.get(0),
    ).unwrap_or(0);
    let you_lifetime_members: i64 = conn.query_row(
        "SELECT COUNT(*) FROM you_users WHERE lifetime_free=1",
        [], |r| r.get(0),
    ).unwrap_or(0);
    let you_designs_generated: i64 = conn.query_row(
        "SELECT COUNT(*) FROM you_designs", [], |r| r.get(0),
    ).unwrap_or(0);
    let monthly_price = cv_get(&conn, "monthly_price_jpy", "1480")
        .parse::<i64>().unwrap_or(980);
    let approx_mrr_jpy = you_subscribers_paid * monthly_price;
    let total_revenue_jpy = revenue_shirts_jpy + auction_revenue_jpy;

    Json(serde_json::json!({
        // ── 旧フィールド (互換のため残す) — テスト購入を含む合計 ──
        "revenue_jpy": total_revenue_jpy,
        "revenue_breakdown": {
            "shirts_jpy":   revenue_shirts_jpy,
            "auctions_jpy": auction_revenue_jpy,
        },
        "shirts_sold":   shirts_sold,
        "purchases_recorded": purchases_total,
        // ── 本物の数字 (Stripe live session のみ) ──
        "real": {
            "revenue_jpy": real_revenue_jpy,
            "purchases":   real_purchases,
            "note": "Stripe live mode (cs_live_*) のみ集計。test purchase は除外。",
        },
        "you": {
            "subscribers_free": you_subscribers_total - you_subscribers_paid - you_lifetime_members,
            "subscribers_paid": you_subscribers_paid,
            "lifetime_members": you_lifetime_members,
            "designs_generated": you_designs_generated,
            "approx_mrr_jpy": approx_mrr_jpy,
        },
        "missing_drops": detect_missing_drops(&conn),
        "as_of": chrono_now(),
    }))
}

/// Inspect the drop history and surface gaps. MUGEN has a strictly increasing
/// `drop_num` 1..108 so a missing integer = a failed/skipped hourly cron run.
/// MUON is daily so missing dates in the last 30 days = a failed daily cron.
/// Surfaced via /api/transparency so a casual reader sees that "automation"
/// isn't perfect, and we don't pretend it is.
fn detect_missing_drops(conn: &rusqlite::Connection) -> serde_json::Value {
    // MUGEN: drop_num is meant to be 1..max. Any int in that range that's
    // absent from active rows = a missed hourly drop.
    let mugen_missing: Vec<i64> = {
        let max_drop: i64 = conn.query_row(
            "SELECT COALESCE(MAX(drop_num), 0) FROM products WHERE brand='mugen' AND active=1",
            [], |r| r.get(0),
        ).unwrap_or(0);
        if max_drop <= 0 { Vec::new() } else {
            let present: std::collections::HashSet<i64> = {
                let mut stmt = match conn.prepare(
                    "SELECT drop_num FROM products WHERE brand='mugen' AND active=1"
                ) { Ok(s) => s, Err(_) => return serde_json::json!({"mugen": [], "muon": []}) };
                stmt.query_map([], |r| r.get::<_, i64>(0))
                    .map(|it| it.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default()
            };
            (1..=max_drop).filter(|n| !present.contains(n)).collect()
        }
    };

    // MUON: check the last 30 dates (JST). Compare a Set of present drop dates
    // (extracted from row name "MUON YYYY.MM.DD" or from created_at) to the
    // expected dates. We use the JST date since the cron fires on JST 00:00 UTC.
    let muon_missing: Vec<String> = {
        // Pull every active MUON row's name; parse "MUON YYYY.MM.DD".
        let present_dates: std::collections::HashSet<String> = {
            let mut stmt = match conn.prepare(
                "SELECT name FROM products WHERE brand='muon' AND active=1"
            ) { Ok(s) => s, Err(_) => return serde_json::json!({"mugen": mugen_missing, "muon": []}) };
            let names: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(0))
                .map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>())
                .unwrap_or_default();
            names.into_iter()
                .filter_map(|n| {
                    // "MUON 2026.05.07" → "2026-05-07"
                    n.split_whitespace().nth(1).map(|d| d.replace('.', "-"))
                })
                .collect()
        };
        if present_dates.is_empty() { Vec::new() } else {
            // Generate the last 14 expected dates (today-13 .. today, JST).
            let now_secs: i64 = chrono_now().parse().unwrap_or(0);
            let jst_now = now_secs + 9 * 3600;
            let today_days = jst_now / 86_400;
            let mut missing = Vec::new();
            // Skip today (cron may not have fired yet) and yesterday boundary
            // (random sleep window). Check days [today-13..=today-2].
            for offset in 2..=13 {
                let d = today_days - offset;
                let (y, mo, da) = civil_from_days(d);
                let date_str = format!("{:04}-{:02}-{:02}", y, mo, da);
                if !present_dates.contains(&date_str) {
                    missing.push(date_str);
                }
            }
            missing
        }
    };

    serde_json::json!({
        "mugen_missing_drops": mugen_missing,
        "muon_missing_dates":  muon_missing,
        "note": "MUGEN drop_num が 1..max の中で抜けている整数 / MUON 直近 14 日で抜けている日付",
    })
}

// ── サンプル ペルソナ + ギャラリー ──────────────────────────────────────────
// 架空の 15 ユーザーが /you に登録しているように見せて、毎日 cron で
// 「今日彼らがもらった一案」を実在の MUGEN drop から決定的に選んで表示する。
// 訪問者が「あー、こういう人達が使っているのか」と分かりやすく + その絵が
// 直接購入動線になる (picked MUGEN は売り物)。

/// Build a small list of fictional personas to seed at startup. Diversity:
/// age 22-55, regions across Japan + 1 海外, taste vectors deliberately
/// pulled apart so the gallery looks varied. avatar_glyph は単色1文字 (絵文字
/// は環境差が大きいので避ける).
fn sample_personas_seed() -> Vec<(&'static str, &'static str, &'static str, serde_json::Value, &'static str)> {
    use serde_json::json;
    vec![
        ("yuna",      "Yuna · 24 · 札幌",            "雪の音と余白。図書館に住んでる気分の日が多い。",
            json!({"mood":["静か","朝の光","余白"],"palette":["モノクロ","アースカラー"],"scene":["毎日","家"],"size":"S"}),
            "Y"),
        ("ren",       "Ren · 31 · 福岡",             "汗をかいた日が一番好き。ジム→焼き鳥→Joy Division。",
            json!({"mood":["力強い","深い","夜の余韻"],"palette":["墨","ヴィンテージ赤"],"scene":["ジム","夜の外出"],"size":"L"}),
            "R"),
        ("emi",       "Emi · 28 · 鎌倉",             "海と日記。雨の日は古道具屋へ。",
            json!({"mood":["ノスタルジック","海","写真的"],"palette":["藍 / インディゴ","サンドベージュ"],"scene":["休日","旅"],"size":"M"}),
            "E"),
        ("kazu",      "Kazu · 45 · 高知",            "山小屋で焙煎してる。手書きの紙が好き。",
            json!({"mood":["余白","手書き","深い"],"palette":["セージグリーン","アースカラー"],"scene":["山","休日"],"size":"L"}),
            "K"),
        ("mio",       "Mio · 22 · 京都",             "古本と祇園のだんごと、新しいバンド。",
            json!({"mood":["遊び","幾何学","ノスタルジック"],"palette":["パステル","蛍光"],"scene":["毎日","街"],"size":"S"}),
            "M"),
        ("haruto",    "Haruto · 27 · 東京",          "ミニマルが行きすぎて床に何もない部屋。",
            json!({"mood":["ミニマル","静か","余白"],"palette":["モノクロ","墨"],"scene":["仕事","家"],"size":"M"}),
            "H"),
        ("aoi",       "Aoi · 33 · 仙台",             "森の中の小さなギャラリーで働いている。",
            json!({"mood":["深い","写真的","朝の光"],"palette":["セージグリーン","サンドベージュ"],"scene":["仕事","パートナー"],"size":"M"}),
            "A"),
        ("taka",      "Taka · 38 · 大阪",            "夜中に車を運転するのが趣味。808 と山下達郎を交互に。",
            json!({"mood":["夜の余韻","力強い","幾何学"],"palette":["墨","ヴィンテージ赤"],"scene":["夜の外出","旅"],"size":"L"}),
            "T"),
        ("sora",      "Sora · 19 · 沖縄",            "海と紙の本と、たまにスケート。",
            json!({"mood":["遊び","海","写真的"],"palette":["藍 / インディゴ","パステル"],"scene":["旅","休日"],"size":"S"}),
            "S"),
        ("nao",       "Nao · 41 · 長野",             "山の家を改装中。木と布。",
            json!({"mood":["手書き","ノスタルジック","余白"],"palette":["アースカラー","サンドベージュ"],"scene":["家","休日"],"size":"M"}),
            "N"),
        ("rui",       "Rui · 35 · 金沢",             "茶室の床から始まる日。",
            json!({"mood":["静か","深い","余白"],"palette":["墨","モノクロ"],"scene":["毎日","パートナー"],"scene_note":"雨"}),
            "R"),
        ("mika",      "Mika · 29 · 神戸",            "中華街で働いて、夜は海岸でランニング。",
            json!({"mood":["力強い","写真的"],"palette":["ヴィンテージ赤","モノクロ"],"scene":["ジム","街"],"size":"M"}),
            "M"),
        ("jun",       "Jun · 52 · 横浜",             "息子が独立して、写真を撮り直し始めた。",
            json!({"mood":["ノスタルジック","写真的","深い"],"palette":["セージグリーン","モノクロ"],"scene":["休日","家"],"size":"L"}),
            "J"),
        ("nina",      "Nina · 26 · Berlin (ex-旭川)", "Bookstore + late espresso. wishes for snow.",
            json!({"mood":["静か","北限","幾何学"],"palette":["モノクロ","藍 / インディゴ"],"scene":["街","旅"],"size":"M"}),
            "N"),
        ("io",        "Io · 30 · 那覇",              "三線弾く深夜のおまわりさんの孫。",
            json!({"mood":["遊び","海","ノスタルジック"],"palette":["藍 / インディゴ","サンドベージュ"],"scene":["毎日","パートナー"],"size":"S"}),
            "I"),
    ]
}

fn seed_sample_personas(conn: &rusqlite::Connection) {
    let now = chrono_now();
    for (slug, name, bio, taste, glyph) in sample_personas_seed() {
        let _ = conn.execute(
            "INSERT OR IGNORE INTO sample_personas
                 (slug, display_name, bio, taste_json, avatar_glyph, active, created_at)
             VALUES (?, ?, ?, ?, ?, 1, ?)",
            params![slug, name, bio, taste.to_string(), glyph, now],
        );
    }
    // Make sure each persona has at least one design "today". If today's row
    // already exists this is a no-op; otherwise pick a fresh MUGEN drop.
    grow_sample_designs_for_today(conn);
}

/// For every active persona, ensure there is a `sample_designs` row for
/// today (JST). The picked MUGEN product is decided deterministically from
/// (persona_slug + day) so the gallery is stable for that day.
fn grow_sample_designs_for_today(conn: &rusqlite::Connection) {
    use sha2::{Digest, Sha256};
    let today = jst_today_str();
    // Pool of MUGEN drops that are still buyable (sold < inventory) and
    // active. We pick one per persona, allowing overlap (same product
    // can appear under multiple personas — fine).
    // Only R2-backed products (mockups.wearmu.com, lifestyle.wearmu.com, or
    // local /mockups/). The Printful tmp URLs from the launch week have
    // already 403'd; never picking them keeps the gallery alive.
    // Sort: products with lifestyle photo first (so the gallery shows people,
    // not flat mockups, whenever possible).
    // Two pools: lifestyle-backed (preferred) + R2 mockup-only (fallback).
    // Each persona first tries the lifestyle pool — if it lands on an index
    // that has lifestyle photo we keep that. If the lifestyle pool is empty
    // we fall back to mockup pool.
    let lifestyle_pool: Vec<(i64, String, String)> = {
        let mut stmt = match conn.prepare(
            "SELECT id, name, lifestyle_url
             FROM products
             WHERE brand='mugen' AND active=1
               AND (inventory IS NULL OR sold < inventory)
               AND lifestyle_url LIKE 'https://lifestyle.wearmu.com/%'
             ORDER BY drop_num DESC LIMIT 200"
        ) { Ok(s) => s, Err(_) => return };
        stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        }).map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>()).unwrap_or_default()
    };
    let mockup_pool: Vec<(i64, String, String)> = {
        let mut stmt = match conn.prepare(
            "SELECT id, name, COALESCE(mockup_url, '')
             FROM products
             WHERE brand='mugen' AND active=1
               AND (inventory IS NULL OR sold < inventory)
               AND (mockup_url LIKE 'https://mockups.wearmu.com/%' OR mockup_url LIKE '/mockups/%')
             ORDER BY drop_num DESC LIMIT 200"
        ) { Ok(s) => s, Err(_) => return };
        stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        }).map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>()).unwrap_or_default()
    };
    if lifestyle_pool.is_empty() && mockup_pool.is_empty() { return; }

    // Re-roll any persona whose existing today-pick points at a broken (now
    // 403'd Printful tmp) URL — happens for legacy data. Delete the row so
    // the loop below regenerates with a fresh pool pick.
    let _ = conn.execute(
        "DELETE FROM sample_designs WHERE day=?
           AND (
             SELECT mockup_url FROM products WHERE id = sample_designs.picked_product_id
           ) LIKE 'https://printful-upload.%'",
        params![today],
    );

    let personas: Vec<(i64, String, String)> = {
        let mut stmt = match conn.prepare(
            "SELECT id, slug, taste_json FROM sample_personas WHERE active=1"
        ) { Ok(s) => s, Err(_) => return };
        let rows: Vec<(i64, String, String)> = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        }).map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>()).unwrap_or_default();
        rows
    };

    for (persona_id, slug, taste_str) in personas {
        // Already have today's row?
        let exists: bool = conn.query_row(
            "SELECT 1 FROM sample_designs WHERE persona_id=? AND day=?",
            params![persona_id, today], |r| r.get::<_, i64>(0),
        ).is_ok();
        if exists { continue; }

        // Deterministic pick: lifestyle pool first; mockup as fallback.
        let mut h = Sha256::new();
        h.update(format!("{}|{}", slug, today).as_bytes());
        let dig = h.finalize();
        let chosen_pool = if !lifestyle_pool.is_empty() { &lifestyle_pool } else { &mockup_pool };
        let idx = (u64::from_be_bytes(dig[..8].try_into().unwrap_or([0;8])) as usize) % chosen_pool.len();
        let (product_id, product_name, mockup_url) = chosen_pool[idx].clone();

        // Compose a poetic name/prompt from the persona's taste
        let taste_json: serde_json::Value = serde_json::from_str(&taste_str)
            .unwrap_or_else(|_| serde_json::json!({}));
        let (name, prompt, _seed) = compose_design(&taste_json, &today, 0);

        // day_num = days since persona created. Simple counter.
        let day_num: i64 = conn.query_row(
            "SELECT COALESCE(MAX(day_num), 0) + 1 FROM sample_designs WHERE persona_id=?",
            params![persona_id], |r| r.get(0),
        ).unwrap_or(1);

        let _ = conn.execute(
            "INSERT OR IGNORE INTO sample_designs
                 (persona_id, day, day_num, name, prompt, picked_product_id, image_url, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![persona_id, today, day_num, format!("{} ({})", name, product_name),
                    prompt, product_id, mockup_url, chrono_now()],
        );
    }
}

/// `GET /api/sample_personas` — list of fictional personas + each one's
/// "today's design" (linked to a real, buyable MUGEN product).
async fn list_sample_personas(State(db): State<Db>) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let today = jst_today_str();

    let rows: Vec<serde_json::Value> = {
        let mut stmt = match conn.prepare(
            "SELECT p.id, p.slug, p.display_name, p.bio, p.avatar_glyph,
                    d.name, d.prompt, d.day_num, d.picked_product_id, d.image_url,
                    pr.price_jpy, pr.lifestyle_url
             FROM sample_personas p
             LEFT JOIN sample_designs d ON d.persona_id = p.id AND d.day = ?
             LEFT JOIN products pr ON pr.id = d.picked_product_id
             WHERE p.active = 1
             ORDER BY p.id"
        ) { Ok(s) => s, Err(_) => return Json(serde_json::json!({"personas":[]})).into_response() };
        let it = stmt.query_map(params![today], |r| {
            // Prefer lifestyle photo if present, else fall back to flat mockup.
            // Discard expired Printful tmp URLs (s3-accelerate.amazonaws.com/tmp/)
            // — they 403'd weeks ago and leave broken images in the gallery.
            let is_alive = |u: &str| -> bool {
                !u.is_empty()
                && !u.contains("printful-upload.s3")
                && !u.contains("/tmp/")
                && (u.starts_with("https://lifestyle.wearmu.com/")
                    || u.starts_with("https://mockups.wearmu.com/")
                    || u.starts_with("/mockups/")
                    || u.starts_with("/static/"))
            };
            let lifestyle_raw: Option<String> = r.get::<_, Option<String>>(11).unwrap_or_default();
            let mockup_raw:    Option<String> = r.get::<_, Option<String>>(9).unwrap_or_default();
            let lifestyle = lifestyle_raw.clone().filter(|s| is_alive(s));
            let mockup    = mockup_raw.clone().filter(|s| is_alive(s));
            let primary = lifestyle.clone().or(mockup);
            Ok(serde_json::json!({
                "slug":          r.get::<_, String>(1).unwrap_or_default(),
                "display_name":  r.get::<_, String>(2).unwrap_or_default(),
                "bio":           r.get::<_, String>(3).unwrap_or_default(),
                "avatar_glyph":  r.get::<_, Option<String>>(4).unwrap_or_default(),
                "today_design_name":   r.get::<_, Option<String>>(5).unwrap_or_default(),
                "today_design_prompt": r.get::<_, Option<String>>(6).unwrap_or_default(),
                "day_num":         r.get::<_, Option<i64>>(7).unwrap_or_default(),
                "product_id":      r.get::<_, Option<i64>>(8).unwrap_or_default(),
                "product_image":   primary,
                "product_lifestyle": lifestyle,
                "product_price_jpy": r.get::<_, Option<i64>>(10).unwrap_or_default(),
            }))
        });
        match it {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(_) => Vec::new(),
        }
    };

    Json(serde_json::json!({"personas": rows, "day": today})).into_response()
}

/// `POST /api/admin/sample_grow` — daily cron entrypoint. Re-rolls each
/// persona's "today design" if it hasn't been generated yet for the
/// current JST day. Idempotent within a day.
async fn admin_sample_grow(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
    Json(body): Json<YouAdminBackfillBody>,
) -> impl IntoResponse {
    if let Err(r) = require_admin_token(Some(&body.admin_token)) { return r; }
    let force = q.get("force").map(|s| s == "1" || s == "true").unwrap_or(false);
    let prefer_lifestyle = q.get("prefer_lifestyle").map(|s| s == "1" || s == "true").unwrap_or(false);
    let conn = db.lock().unwrap();
    let today = jst_today_str();

    // Force = wipe today's picks first. Prefer_lifestyle = only re-roll those
    // whose current pick has no lifestyle_url.
    if force {
        let _ = conn.execute("DELETE FROM sample_designs WHERE day=?", params![today]);
    } else if prefer_lifestyle {
        let _ = conn.execute(
            "DELETE FROM sample_designs WHERE day=?
             AND picked_product_id IN (
                SELECT id FROM products
                WHERE (lifestyle_url IS NULL OR lifestyle_url='')
             )",
            params![today],
        );
    }

    let before: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sample_designs WHERE day=?",
        params![today], |r| r.get(0),
    ).unwrap_or(0);
    grow_sample_designs_for_today(&conn);
    let after: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sample_designs WHERE day=?",
        params![today], |r| r.get(0),
    ).unwrap_or(0);
    Json(serde_json::json!({
        "ok": true,
        "day": today,
        "designs_before": before,
        "designs_after":  after,
        "added": after - before,
        "force": force,
        "prefer_lifestyle": prefer_lifestyle,
    })).into_response()
}

// ── Referral status ─────────────────────────────────────────────────────────
#[derive(Deserialize)]
struct ReferralStatusBody {
    token: String,
}

/// POST /api/you/referral — returns the user's referral slug + accumulated
/// credit + count of successful referrals (≥1 MU purchase).
async fn you_referral_status(
    State(db): State<Db>,
    Json(body): Json<ReferralStatusBody>,
) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let row: Option<(String, i64, i64)> = conn.query_row(
        "SELECT slug, COALESCE(referral_credit_jpy,0), COALESCE(referral_count,0)
         FROM you_users WHERE token=? AND unsubscribed_at IS NULL",
        params![body.token], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).ok();
    let Some((slug, credit, count)) = row else {
        return (StatusCode::NOT_FOUND, "invalid token").into_response();
    };
    let base = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    Json(serde_json::json!({
        "slug": slug,
        "referral_url": format!("{}/you?ref={}", base, slug),
        "credit_jpy":   credit,
        "count":        count,
        "reward_per_referral_jpy": 3400,
    })).into_response()
}

// ── Lifestyle photo admin endpoint ─────────────────────────────────────────
#[derive(Deserialize)]
struct LifestyleBody {
    product_id: i64,
    lifestyle_url: String,
}

/// PATCH /api/admin/lifestyle?token=… — set `products.lifestyle_url` for
/// a given product. Called from generate_lifestyle.py after Gemini generates
/// and R2 stores the image.

// PATCH /api/admin/collab_image — set image_url on collab_products by slug.
#[derive(Deserialize)]
struct CollabImageBody {
    slug: String,
    image_url: String,
}
async fn admin_collab_image(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
    Json(body): Json<CollabImageBody>,
) -> impl IntoResponse {
    if let Err(r) = require_admin_token(q.get("token")) { return r; }
    let conn = db.lock().unwrap();
    let updated = conn.execute(
        "UPDATE collab_products SET image_url=? WHERE slug=?",
        params![body.image_url, body.slug],
    ).unwrap_or(0);
    Json(serde_json::json!({"ok": true, "updated": updated})).into_response()
}

async fn admin_lifestyle(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
    Json(body): Json<LifestyleBody>,
) -> impl IntoResponse {
    if let Err(r) = require_admin_token(q.get("token")) { return r; }
    let conn = db.lock().unwrap();
    let updated = conn.execute(
        "UPDATE products SET lifestyle_url=? WHERE id=?",
        params![body.lifestyle_url, body.product_id],
    ).unwrap_or(0);
    Json(serde_json::json!({"ok": true, "updated": updated})).into_response()
}

// ── Auto-blog (AI generates daily field log) ───────────────────────────────
#[derive(Deserialize)]
struct AutoBlogBody {
    admin_token: String,
}

/// Shared stats gatherer used by both the legacy server-side compose path
/// and the new GitHub-Actions-driven publish path. Keeping it in one place
/// guarantees the Actions runner sees the same JSON the prompt has always
/// consumed.
fn gather_blog_stats(db: &Db) -> serde_json::Value {
    use serde_json::json;
    let conn = db.lock().unwrap();
    let revenue: i64 = conn.query_row(
        "SELECT COALESCE(SUM(price_jpy * sold), 0) FROM products WHERE active=1",
        [], |r| r.get(0)).unwrap_or(0);
    let purchases: i64 = conn.query_row(
        "SELECT COUNT(*) FROM mu_purchases", [], |r| r.get(0)).unwrap_or(0);
    let real_revenue: i64 = conn.query_row(
        "SELECT COALESCE(SUM(p.price_jpy), 0) FROM mu_purchases mp
         JOIN products p ON p.id = mp.product_id",
        [], |r| r.get(0)).unwrap_or(0);
    let subs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM you_users WHERE unsubscribed_at IS NULL",
        [], |r| r.get(0)).unwrap_or(0);
    let designs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM you_designs", [], |r| r.get(0)).unwrap_or(0);
    let lifestyle_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM products WHERE lifestyle_url IS NOT NULL AND lifestyle_url != ''",
        [], |r| r.get(0)).unwrap_or(0);
    let missing = detect_missing_drops(&conn);
    json!({
        "revenue_shown_jpy": revenue,
        "real_revenue_jpy": real_revenue,
        "purchases": purchases,
        "subscribers": subs,
        "designs_generated": designs,
        "lifestyle_photos": lifestyle_count,
        "missing": missing,
        "day": jst_today_str(),
    })
}

/// Canonical prompt for the daily Field log. Used by both compose paths so
/// the output stays consistent whether Gemini is called from Fly or Actions.
fn blog_prompt(stats: &serde_json::Value) -> String {
    format!(r#"あなたは MU ブランドの「無人運営 AI 執筆者」です。今日の Field log を Markdown で書いてください。

事実 (JSON、これ以外の数字は捏造禁止):
{stats}

書き方:
- 600〜900 字、3〜4 セクション
- 顧客視点 + 経営視点 (Bezos 的)、過剰演出は禁止
- 数字を 1 つは引用 (real_revenue_jpy を優先)
- "今日動いたもの / 動かなかったもの / 明日へ" の構成
- 自己卑下や絵文字過剰は禁止
- 末尾に「— 自動生成 by Gemini 2.5 Flash」と明記

タイトルは 28 字以内、本文 1 行目に H1 として `# タイトル` を置いてください。"#,
        stats = stats)
}

async fn compose_auto_blog(db: &Db) -> Result<(String, String, String, serde_json::Value), String> {
    use serde_json::json;
    let key = env::var("GEMINI_API_KEY").map_err(|_| "GEMINI_API_KEY missing".to_string())?;
    let stats = gather_blog_stats(db);

    let prompt = blog_prompt(&stats);

    let req_body = json!({"contents": [{"parts": [{"text": prompt}]}]});
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        key);
    let resp = reqwest::Client::new().post(&url)
        .json(&req_body).send().await
        .map_err(|e| format!("gemini request: {e}"))?;
    if !resp.status().is_success() {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return Err(format!("gemini {}: {}", s, t.chars().take(300).collect::<String>()));
    }
    let j: serde_json::Value = resp.json().await.map_err(|e| format!("json: {e}"))?;
    let text = j["candidates"][0]["content"]["parts"][0]["text"]
        .as_str().unwrap_or("").to_string();
    if text.trim().is_empty() {
        return Err("gemini returned empty text".into());
    }
    let mut title = "今日の Field log".to_string();
    let mut body_md_lines: Vec<String> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 && line.trim_start().starts_with("# ") {
            title = line.trim_start_matches('#').trim().to_string();
            continue;
        }
        body_md_lines.push(line.to_string());
    }
    let body_md = body_md_lines.join("\n").trim().to_string();
    let body_html = md_to_html_simple(&body_md);
    Ok((title, body_html, body_md, stats))
}

/// Safe Markdown → HTML renderer for AI-generated blog bodies.
/// pulldown-cmark parse + ammonia sanitize. Strips <script>, on* attrs,
/// javascript:/data:/vbscript: URLs, and unknown tags. Gemini ouputs are
/// untrusted (could be prompt-injected) so always sanitize.
fn md_to_html_simple(md: &str) -> String {
    use pulldown_cmark::{Parser, Options, html};
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(md, opts);
    let mut rendered = String::with_capacity(md.len() * 2);
    html::push_html(&mut rendered, parser);
    ammonia::clean(&rendered)
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Escape for HTML attribute context (inside double-quoted attributes).
/// Adds `"` and `'` on top of the body-context escaping. Required when the
/// value is interpolated into `content="…"` or `href="…"` etc.
fn html_attr_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&#39;")
}

#[allow(dead_code)]  // superseded by pulldown-cmark + ammonia in md_to_html_simple
fn inline_md(s: &str) -> String {
    let esc = html_escape(s);
    let bold_re = pair_replace(&esc, "**", "<strong>", "</strong>");
    pair_replace(&bold_re, "*", "<em>", "</em>")
}

#[allow(dead_code)]
fn pair_replace(s: &str, marker: &str, open: &str, close: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    let mut toggle = false;
    while let Some(idx) = rest.find(marker) {
        out.push_str(&rest[..idx]);
        out.push_str(if toggle { close } else { open });
        rest = &rest[idx + marker.len()..];
        toggle = !toggle;
    }
    out.push_str(rest);
    out
}

// ── X (Twitter) auto-post queue ───────────────────────────────────────────
// Rust 側は queue を出すだけ。実 post は twitter_post.py が OAuth 1.0a で
// やる (Python の方が tweepy 等で楽)。Python が成功したら mark_posted を呼ぶ。

#[derive(Deserialize)]
struct AdminXQueueQuery {
    #[serde(default)] token: String,
    /// max items to return (default 4, max 10)
    #[serde(default)] limit: Option<i64>,
}

async fn admin_x_queue(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<AdminXQueueQuery>,
) -> impl IntoResponse {
    if let Err(r) = require_admin_token(Some(&q.token)) { return r; }
    let limit = q.limit.unwrap_or(4).clamp(1, 10);
    let conn = db.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT id, brand, drop_num, name, COALESCE(lifestyle_url, mockup_url), price_jpy
         FROM products
         WHERE brand IN ('mugen','muon','ma')
           AND active=1
           AND (x_posted_at IS NULL OR x_posted_at='')
         ORDER BY id DESC LIMIT ?"
    ) {
        Ok(s) => s,
        Err(_) => return Json(serde_json::json!({"items":[]})).into_response(),
    };
    let rows: Vec<serde_json::Value> = stmt.query_map(params![limit], |r| {
        Ok(serde_json::json!({
            "id":         r.get::<_, i64>(0)?,
            "brand":      r.get::<_, String>(1)?,
            "drop_num":   r.get::<_, i64>(2)?,
            "name":       r.get::<_, String>(3)?,
            "image_url":  r.get::<_, Option<String>>(4).unwrap_or_default(),
            "price_jpy":  r.get::<_, i64>(5)?,
            "url":        format!("https://wearmu.com/{}", r.get::<_, String>(1)?),
        }))
    }).map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default();
    Json(serde_json::json!({"items": rows})).into_response()
}

#[derive(Deserialize)]
struct AdminXMarkPostedBody {
    admin_token: String,
    product_id: i64,
    tweet_id: Option<String>,
}

async fn admin_x_mark_posted(
    State(db): State<Db>,
    Json(body): Json<AdminXMarkPostedBody>,
) -> impl IntoResponse {
    if let Err(r) = require_admin_token(Some(&body.admin_token)) { return r; }
    let conn = db.lock().unwrap();
    let updated = conn.execute(
        "UPDATE products SET x_posted_at=?, x_tweet_id=? WHERE id=?",
        params![chrono_now(), body.tweet_id, body.product_id],
    ).unwrap_or(0);
    Json(serde_json::json!({"ok": true, "updated": updated})).into_response()
}

// ── Thank-you outreach to buyers (Vision + 50% MUON coupon + notes) ───────
#[derive(Deserialize)]
struct AdminThankYouBody {
    admin_token: String,
    /// Optional override: dry_run=true returns the planned recipient list
    /// without minting coupons or sending email. Default false.
    #[serde(default)] dry_run: bool,
}

fn thank_you_coupon_code(email: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(format!("MUON-THANKS-2026-05-{}", email.to_lowercase()).as_bytes());
    let d = h.finalize();
    // Stripe coupon IDs accept A-Z0-9_- ; avoid ambiguous chars
    let alphabet = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut out = String::from("MUON50-");
    for i in 0..8 {
        out.push(alphabet[(d[i] as usize) % alphabet.len()] as char);
    }
    out
}

fn thank_you_email_html(coupon_code: &str) -> String {
    format!(r#"<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:48px 40px;max-width:560px;margin:0 auto;border-radius:2px">
  <div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:30px">MU</div>
  <div style="font-size:11px;letter-spacing:0.32em;text-transform:uppercase;color:#e6c449;opacity:0.85;margin-bottom:14px">Thank you · From the founder</div>

  <h1 style="font-size:22px;font-weight:300;line-height:1.55;margin-bottom:18px;letter-spacing:0.01em">買ってくれてありがとう。<br>あなたは MU が始まる前の 5 人のうちの 1 人です。</h1>

  <p style="font-size:14px;line-height:1.95;opacity:0.85;margin-bottom:18px">
    MU は AI が毎時間 T シャツをデザインする無人ブランドです。立ち上げ 4 日目で、<strong>あなたを含めた 5 名</strong>から ¥145,000 を受け取りました。これは僕の <em>cron に毛が生えただけ</em>のスクリプトが、本当に誰かのクローゼットまで届いたという、たった 1 つの証拠です。
  </p>

  <h2 style="font-size:13px;font-weight:500;color:#e6c449;letter-spacing:0.18em;text-transform:uppercase;margin:32px 0 12px">①  MU の 10 年計画を作りました</h2>
  <p style="font-size:13px;line-height:1.9;opacity:0.78;margin-bottom:14px">
    Amazon が 27 年で 154 万人を雇って到達した場所に、MU は 10 年で人間 0 人で行きたい。<br>
    MUer / MU Owner / MA Council という階層と、2036 年までのロードマップを公開しました:
  </p>
  <a href="https://wearmu.com/vision" style="display:inline-block;background:#e6c449;color:#000;padding:14px 26px;font-size:11px;letter-spacing:0.32em;text-transform:uppercase;text-decoration:none;font-weight:700;border-radius:2px;margin-bottom:18px">Vision を読む →</a>

  <h2 style="font-size:13px;font-weight:500;color:#e6c449;letter-spacing:0.18em;text-transform:uppercase;margin:32px 0 12px">② 感謝の MUON 50% OFF</h2>
  <p style="font-size:13px;line-height:1.9;opacity:0.78;margin-bottom:10px">
    MUON は「気温が枚数を決める」毎日 1 案のドロップ。今日は 21°C なので 21 着限定です。<br>
    あなた専用のクーポンを発行しました:
  </p>
  <div style="background:#111;border:1px dashed rgba(230,196,73,0.45);padding:18px 22px;text-align:center;margin:14px 0 18px;border-radius:2px">
    <div style="font-size:9px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.55;margin-bottom:6px">Coupon code</div>
    <div style="font-family:'Menlo','SF Mono',monospace;font-size:22px;letter-spacing:0.08em;color:#e6c449;font-weight:600">{coupon_code}</div>
    <div style="font-size:10px;opacity:0.55;margin-top:8px;letter-spacing:0.04em">Checkout で入力 · 60 日以内 · 1 回限り</div>
  </div>
  <a href="https://wearmu.com/muon" style="display:inline-block;background:transparent;color:#F5F5F0;border:1px solid rgba(255,255,255,0.25);padding:13px 24px;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;text-decoration:none;font-weight:500;border-radius:2px;margin-bottom:18px">MUON を見る →</a>

  <h2 style="font-size:13px;font-weight:500;color:#e6c449;letter-spacing:0.18em;text-transform:uppercase;margin:32px 0 12px">③ 公開ノート (Notes)</h2>
  <p style="font-size:13px;line-height:1.9;opacity:0.78;margin-bottom:14px">
    売上 ¥0 から ¥145,000 になるまで、何が動いて、何が壊れたか、全部書いています。<br>
    明日からは <em>AI が毎朝この Field log を自分で書きます</em>。
  </p>
  <ul style="font-size:13px;line-height:2.0;opacity:0.85;padding-left:18px;margin-bottom:18px">
    <li><a href="https://wearmu.com/blog/elon-cron-with-fur.html" style="color:#e6c449">「cron に毛が生えてるだけ」と Elon に言われたので 1 日でやり切った件</a> (#002)</li>
    <li><a href="https://wearmu.com/blog/field-log-001.html" style="color:#e6c449">Field log #001 — 動いたもの / 壊れたもの / 直したもの</a></li>
    <li><a href="https://wearmu.com/blog/auto/auto-2026-05-11" style="color:#e6c449">2026-05-11 — AI 自動運営ノート (初稿)</a></li>
    <li><a href="https://wearmu.com/blog/from-automation-to-autonomy.html" style="color:#e6c449">公開ノート #001 — 自動から自律へ</a></li>
  </ul>

  <p style="font-size:13px;line-height:1.95;opacity:0.85;margin:30px 0 8px">
    返信はそのまま <a href="mailto:info@enablerdao.com" style="color:#e6c449">info@enablerdao.com</a> に届きます。または <a href="https://wearmu.com/you" style="color:#e6c449">/you</a> の「MU AI に直接送る」フォームでも (Gemini が即返答 + 私が今日中に読みます)。
  </p>
  <p style="font-size:13px;line-height:1.95;opacity:0.85">
    本当に、ありがとう。
  </p>
  <p style="font-size:12px;opacity:0.6;margin-top:24px">
    — 濱田優貴 / 株式会社イネブラ (Enabler Inc.)<br>
    <span style="font-size:11px">MU · wearmu.com · GitHub に CC0/MIT で全公開</span>
  </p>
</div>"#, coupon_code = coupon_code)
}

async fn admin_thank_buyers(
    State(db): State<Db>,
    Json(body): Json<AdminThankYouBody>,
) -> impl IntoResponse {
    if let Err(r) = require_admin_token(Some(&body.admin_token)) { return r; }
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    if stripe_key.is_empty() || resend_key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "stripe / resend env missing").into_response();
    }

    // Distinct buyers (cs_live_* only) without prior thank-you.
    let buyers: Vec<String> = {
        let conn = db.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT DISTINCT LOWER(email) FROM mu_purchases
             WHERE session_id LIKE 'cs_live_%'
               AND email IS NOT NULL AND email != ''
               AND (thank_you_sent_at IS NULL OR thank_you_sent_at = '')"
        ) {
            Ok(s) => s,
            Err(_) => return Json(serde_json::json!({"sent": 0, "errors": ["db prepare failed"]})).into_response(),
        };
        stmt.query_map([], |r| r.get::<_, String>(0))
            .map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>())
            .unwrap_or_default()
    };

    if body.dry_run {
        return Json(serde_json::json!({"dry_run": true, "would_send_to": buyers})).into_response();
    }

    // Expiry: +60 days as unix seconds
    let redeem_by: i64 = (chrono_now().parse::<i64>().unwrap_or(0)) + 60 * 86_400;

    let mut sent = 0u32;
    let mut errors: Vec<String> = Vec::new();

    for email in buyers.iter() {
        let code = thank_you_coupon_code(email);

        // 1) Mint coupon. Idempotent on coupon id (Stripe returns existing).
        let coupon_form: Vec<(&str, String)> = vec![
            ("id", code.clone()),
            ("percent_off", "50".into()),
            ("duration", "once".into()),
            ("max_redemptions", "1".into()),
            ("currency", "jpy".into()),
            ("name", format!("MU thanks · MUON 50% off")),
            ("redeem_by", redeem_by.to_string()),
            ("metadata[intended_brand]", "muon".into()),
            ("metadata[buyer_email]", email.clone()),
        ];
        let cr = reqwest::Client::new()
            .post("https://api.stripe.com/v1/coupons")
            .basic_auth(&stripe_key, None::<&str>)
            .form(&coupon_form)
            .send().await;
        match cr {
            Ok(r) if r.status().is_success() => {}
            Ok(r) => {
                let s = r.status();
                let t = r.text().await.unwrap_or_default();
                // resource_already_exists → continue (idempotent)
                if !t.contains("resource_already_exists") {
                    errors.push(format!("{email}: coupon {s}: {}", t.chars().take(160).collect::<String>()));
                    continue;
                }
            }
            Err(e) => {
                errors.push(format!("{email}: coupon network: {e}"));
                continue;
            }
        }

        // 2) Create a promotion_code so users can enter the code at checkout.
        // (Best-effort; failure here doesn't abort — coupon id itself is usable.)
        let promo_form: Vec<(&str, String)> = vec![
            ("coupon", code.clone()),
            ("code", code.clone()),
            ("max_redemptions", "1".into()),
            ("expires_at", redeem_by.to_string()),
        ];
        let _ = reqwest::Client::new()
            .post("https://api.stripe.com/v1/promotion_codes")
            .basic_auth(&stripe_key, None::<&str>)
            .form(&promo_form)
            .send().await;

        // 3) Send Resend email.
        let html = thank_you_email_html(&code);
        let subject = "MU から、買ってくれたあなたへ — 50% MUON クーポンと Vision のお知らせ";
        let send_res = reqwest::Client::new()
            .post("https://api.resend.com/emails")
            .bearer_auth(&resend_key)
            .json(&serde_json::json!({
                "from": "MU <noreply@wearmu.com>",
                "to": [email],
                "subject": subject,
                "html": html,
                "reply_to": "info@enablerdao.com",
            }))
            .send().await;
        match send_res {
            Ok(r) if r.status().is_success() => {}
            Ok(r) => {
                let s = r.status();
                let t = r.text().await.unwrap_or_default();
                errors.push(format!("{email}: resend {s}: {}", t.chars().take(160).collect::<String>()));
                continue;
            }
            Err(e) => {
                errors.push(format!("{email}: resend network: {e}"));
                continue;
            }
        }

        // 4) Mark every purchase row for this email as sent (idempotency seed).
        {
            let conn = db.lock().unwrap();
            let _ = conn.execute(
                "UPDATE mu_purchases SET thank_you_sent_at=?, thank_you_coupon=?
                 WHERE LOWER(email)=? AND (thank_you_sent_at IS NULL OR thank_you_sent_at='')",
                params![chrono_now(), code, email],
            );
        }
        sent += 1;
    }

    Json(serde_json::json!({
        "ok": true,
        "buyers_considered": buyers.len(),
        "sent": sent,
        "errors": errors,
    })).into_response()
}

// ── AI Feedback Loop (お客様 → AI → MA Council 通知) ───────────────────────
#[derive(Deserialize)]
struct CustomerFeedbackBody {
    #[serde(default)] token: String,
    #[serde(default)] email: String,
    message: String,
    #[serde(default)] kind: String,
}

async fn submit_feedback(
    State(db): State<Db>,
    Json(body): Json<CustomerFeedbackBody>,
) -> impl IntoResponse {
    let msg = body.message.trim().to_string();
    if msg.is_empty() || msg.len() > 4000 {
        return (StatusCode::BAD_REQUEST, "message must be 1..4000 chars").into_response();
    }
    // Rate limit: per-identity 1/30s, 20/24h. Protects against DOS (Gemini cost).
    {
        let id_key = if !body.email.is_empty() { body.email.to_lowercase() }
            else if !body.token.is_empty() { format!("token:{}", &body.token[..body.token.len().min(16)]) }
            else { "anon".to_string() };
        let now_s: i64 = chrono_now().parse().unwrap_or(0);
        let conn = db.lock().unwrap();
        let recent_30s: i64 = conn.query_row(
            "SELECT COUNT(*) FROM customer_feedback
             WHERE LOWER(COALESCE(email,'anon'))=?
               AND CAST(created_at AS INTEGER) >= ?",
            params![id_key, now_s - 30], |r| r.get(0),
        ).unwrap_or(0);
        if recent_30s >= 1 {
            return (StatusCode::TOO_MANY_REQUESTS, "30 秒に 1 件までお送りください").into_response();
        }
        let recent_24h: i64 = conn.query_row(
            "SELECT COUNT(*) FROM customer_feedback
             WHERE LOWER(COALESCE(email,'anon'))=?
               AND CAST(created_at AS INTEGER) >= ?",
            params![id_key, now_s - 86_400], |r| r.get(0),
        ).unwrap_or(0);
        if recent_24h >= 20 {
            return (StatusCode::TOO_MANY_REQUESTS, "1 日 20 件までです。明日また送ってください").into_response();
        }
    }
    let (user_id, email, is_lifetime, ma_council): (Option<i64>, String, bool, bool) = {
        let conn = db.lock().unwrap();
        let row: Option<(i64, String, i64)> = if !body.token.is_empty() {
            conn.query_row(
                "SELECT id, email, COALESCE(lifetime_free,0) FROM you_users
                 WHERE token=? AND unsubscribed_at IS NULL",
                params![body.token], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            ).ok()
        } else if !body.email.is_empty() {
            conn.query_row(
                "SELECT id, email, COALESCE(lifetime_free,0) FROM you_users
                 WHERE email=? AND unsubscribed_at IS NULL",
                params![body.email.to_lowercase()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            ).ok()
        } else { None };
        let ma_owner: bool = if let Some((_, ref e, _)) = row {
            conn.query_row(
                "SELECT 1 FROM mu_purchases WHERE email=? AND brand='ma' LIMIT 1",
                params![e], |_| Ok(true)
            ).unwrap_or(false)
        } else { false };
        match row {
            Some((uid, e, lf)) => (Some(uid), e, lf == 1, ma_owner),
            None => (None, body.email.clone(), false, false),
        }
    };
    let kind = if body.kind.is_empty() {
        if msg.len() > 200 { "vision" } else { "request" }
    } else { body.kind.as_str() }.to_string();

    let feedback_id: i64 = {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO customer_feedback
                 (user_id, email, message, kind, is_lifetime, is_ma_council, created_at)
             VALUES (?,?,?,?,?,?,?)",
            params![user_id, email, msg, kind, is_lifetime as i64, ma_council as i64, chrono_now()],
        );
        conn.last_insert_rowid()
    };

    let ai_reply = match gemini_feedback_reply(&msg, is_lifetime, ma_council).await {
        Ok(s) => s,
        Err(e) => { eprintln!("[feedback] gemini error: {e}"); String::new() }
    };
    if !ai_reply.is_empty() {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "UPDATE customer_feedback SET ai_reply=?, ai_reply_at=? WHERE id=?",
            params![ai_reply, chrono_now(), feedback_id],
        );
    }

    let tag = if ma_council { "⭐ MA Council" }
        else if is_lifetime { "MU Owner" }
        else { "MUer" };
    notify_telegram_feedback(tag, &email, &msg).await;

    Json(serde_json::json!({
        "ok": true,
        "id": feedback_id,
        "ai_reply": ai_reply,
        "tier": if ma_council { "ma_council" } else if is_lifetime { "mu_owner" } else { "muer" },
    })).into_response()
}

async fn gemini_feedback_reply(message: &str, is_lifetime: bool, is_ma_council: bool) -> Result<String, String> {
    use serde_json::json;
    let key = env::var("GEMINI_API_KEY").map_err(|_| "GEMINI_API_KEY missing".to_string())?;
    let tier = if is_ma_council { "MA Council (MA オークション落札者、brand 方向性に投票権を持つ立場)" }
        else if is_lifetime { "MU Owner (T シャツ所有者、一生無料)" }
        else { "MUer (/you 登録のお客様)" };
    let prompt = format!(r#"あなたは MU ブランド (北海道弟子屈町、無人 AI ファッション) の AI 運営担当です。お客様 ({tier}) からのフィードバックに 80〜200 字で返答してください。

ルール:
- 二人称は「あなた」、自分のことは「MU」または「私たち」
- 過剰な謝罪は禁止、業務報告として簡潔に
- 機能要望なら「要検討の優先度を○○として記録した」と返す
- 数字や約束は捏造禁止 (subscribers 9 / lifetime 3 / 本売 5 件 ¥145,000 まで)
- 必要なら info@enablerdao.com を提示
- MA Council にはより丁寧かつ「次回 council 議題で扱う」と明記
- 末尾に「— MU AI (Gemini 2.5)」と書く

お客様のメッセージ:
"""
{message}
"""
"#);
    let req_body = json!({"contents": [{"parts": [{"text": prompt}]}]});
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        key);
    let resp = reqwest::Client::new().post(&url)
        .json(&req_body).send().await
        .map_err(|e| format!("gemini request: {e}"))?;
    if !resp.status().is_success() {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return Err(format!("gemini {}: {}", s, t.chars().take(200).collect::<String>()));
    }
    let j: serde_json::Value = resp.json().await.map_err(|e| format!("json: {e}"))?;
    let text = j["candidates"][0]["content"]["parts"][0]["text"]
        .as_str().unwrap_or("").to_string();
    Ok(text.trim().to_string())
}

async fn notify_telegram_feedback(tier: &str, email: &str, message: &str) {
    let tg_token = env::var("TELEGRAM_BOT_TOKEN").unwrap_or_default();
    let tg_chat  = env::var("TELEGRAM_CHAT_ID").unwrap_or_else(|_| "1136442501".into());
    if tg_token.is_empty() { return; }
    let body = format!(
        "📮 MU feedback [{tier}]\n{email}\n\n{msg}",
        msg = message.chars().take(800).collect::<String>(),
    );
    let _ = reqwest::Client::new()
        .post(format!("https://api.telegram.org/bot{}/sendMessage", tg_token))
        .json(&serde_json::json!({"chat_id": tg_chat, "text": body, "disable_web_page_preview": true}))
        .send().await;
}

async fn admin_blog_compose(
    State(db): State<Db>,
    Json(body): Json<AutoBlogBody>,
) -> impl IntoResponse {
    if let Err(r) = require_admin_token(Some(&body.admin_token)) { return r; }
    let slug = format!("auto-{}", jst_today_str());
    {
        let conn = db.lock().unwrap();
        let exists: bool = conn.query_row(
            "SELECT 1 FROM auto_blog_posts WHERE slug=?",
            params![slug], |r| r.get::<_, i64>(0),
        ).is_ok();
        if exists {
            return Json(serde_json::json!({"ok": true, "skipped": true, "slug": slug})).into_response();
        }
    }
    match compose_auto_blog(&db).await {
        Ok((title, body_html, body_md, stats)) => {
            let conn = db.lock().unwrap();
            let _ = conn.execute(
                "INSERT OR IGNORE INTO auto_blog_posts
                    (slug, title, body_html, body_md, model, stats_json, published, created_at)
                 VALUES (?,?,?,?,?,?,1,?)",
                params![slug, title, body_html, body_md, "gemini-2.5-flash",
                        stats.to_string(), chrono_now()],
            );
            Json(serde_json::json!({"ok": true, "slug": slug, "title": title})).into_response()
        }
        Err(e) => {
            eprintln!("[auto-blog] {e}");
            (StatusCode::BAD_GATEWAY, e).into_response()
        }
    }
}

// ─── GitHub-Actions-driven blog autonomy ───────────────────────────────────
// 2 endpoints replace the single-process /api/admin/blog_compose flow:
//
//   GET  /api/blog/stats_for_today  — public, returns the JSON the prompt
//                                     needs + today's slug + already_published
//   POST /api/admin/blog_publish    — admin, accepts pre-composed markdown
//                                     and stores it (idempotent on slug)
//
// Actions cron orchestrates: fetch stats → call Gemini directly → publish.
// /api/admin/blog_compose stays available as a manual / Fly-side fallback.

/// Per-IP hourly rate limit on the public stats_for_today endpoint.
/// Prevents abuse since the prompt is shipped in the response (an attacker
/// could harvest it and pound Gemini at our expense if we proxied — we don't,
/// but the prompt itself is brand IP we want minimal disclosure of).
const BLOG_STATS_RATE_LIMIT_PER_HOUR: i64 = 30;

/// Detect missing recent days (yesterday/2-ago/3-ago) so a single Actions run
/// can backfill any days we slipped. Returns slugs *not yet* in the table,
/// oldest-first, max 3 entries.
fn detect_missing_blog_slugs(conn: &rusqlite::Connection) -> Vec<String> {
    let mut missing = Vec::new();
    let today_unix = chrono_now().parse::<i64>().unwrap_or(0);
    let jst = today_unix + 9 * 3600;
    let today_day = jst / 86_400;
    for offset in (1..=3).rev() {
        let day = today_day - offset;
        let (y, m, d) = civil_from_days(day);
        let slug = format!("auto-{:04}-{:02}-{:02}", y, m, d);
        let exists: bool = conn.query_row(
            "SELECT 1 FROM auto_blog_posts WHERE slug=?",
            params![slug], |r| r.get::<_, i64>(0),
        ).is_ok();
        if !exists {
            missing.push(slug);
        }
    }
    missing
}

async fn blog_stats_for_today(
    State(db): State<Db>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Rate limit by client IP. Best-effort — fly-client-ip header trusted
    // because we're behind Fly's edge.
    let ip = headers.get("fly-client-ip")
        .or_else(|| headers.get("x-forwarded-for"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .split(',').next().unwrap_or("unknown").trim().to_string();
    let now_s: i64 = chrono_now().parse().unwrap_or(0);
    let hour_bucket = now_s / 3600;
    let hits: i64 = {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO blog_rate_limit (ip, hour_bucket, hits) VALUES (?,?,1)
             ON CONFLICT(ip, hour_bucket) DO UPDATE SET hits = hits + 1",
            params![ip, hour_bucket],
        );
        // GC old buckets (>24h)
        let _ = conn.execute(
            "DELETE FROM blog_rate_limit WHERE hour_bucket < ?",
            params![hour_bucket - 24],
        );
        conn.query_row(
            "SELECT hits FROM blog_rate_limit WHERE ip=? AND hour_bucket=?",
            params![ip, hour_bucket], |r| r.get::<_, i64>(0),
        ).unwrap_or(0)
    };
    if hits > BLOG_STATS_RATE_LIMIT_PER_HOUR {
        return (StatusCode::TOO_MANY_REQUESTS,
            format!("rate limit: {}/h per IP", BLOG_STATS_RATE_LIMIT_PER_HOUR)).into_response();
    }

    let stats = gather_blog_stats(&db);
    let slug = format!("auto-{}", jst_today_str());
    let (already, backfill): (bool, Vec<String>) = {
        let conn = db.lock().unwrap();
        let already = conn.query_row(
            "SELECT 1 FROM auto_blog_posts WHERE slug=?",
            params![slug], |r| r.get::<_, i64>(0),
        ).is_ok();
        let backfill = detect_missing_blog_slugs(&conn);
        (already, backfill)
    };
    let prompt = blog_prompt(&stats);
    Json(serde_json::json!({
        "stats": stats,
        "today_slug": slug,
        "already_published": already,
        "backfill_slugs": backfill, // Actions iterates these too if non-empty
        "prompt": prompt,           // shipped so Actions doesn't drift from server's wording
        "gemini_model": "gemini-2.5-flash",
        "endpoint_version": 2,
        "rate_limit_remaining": (BLOG_STATS_RATE_LIMIT_PER_HOUR - hits).max(0),
    })).into_response()
}

/// Best-effort 2-pass review: send the composed body back to Gemini and ask
/// "does this match MU's brand voice + factual constraints?". If the review
/// returns `pass=false`, log a warning but still publish (we don't block on
/// LLM critic; manual review can override).
async fn review_blog_body(body_md: &str, stats: &serde_json::Value) -> Option<(bool, String)> {
    let key = env::var("GEMINI_API_KEY").ok().filter(|s| !s.is_empty())?;
    let prompt = format!(
        "あなたは MU ブランドの編集者です。以下の Field log 草稿を以下のルールで採点してください:\n\
        - 数字は stats JSON にあるものだけ使っているか (捏造禁止)\n\
        - トーンが派手すぎないか / 自己卑下しすぎないか\n\
        - 600〜900 字に収まっているか\n\
        - 末尾に「— 自動生成 by Gemini 2.5 Flash」がついているか\n\n\
        stats: {stats}\n\n草稿:\n---\n{body}\n---\n\n\
        出力 (JSON のみ): {{\"pass\": bool, \"reason\": \"<32 字以内>\"}}",
        stats = stats, body = body_md);
    let req = serde_json::json!({"contents": [{"parts": [{"text": prompt}]}]});
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        key);
    let resp = reqwest::Client::new().post(&url)
        .json(&req).send().await.ok()?;
    if !resp.status().is_success() { return None; }
    let j: serde_json::Value = resp.json().await.ok()?;
    let text = j["candidates"][0]["content"]["parts"][0]["text"].as_str()?.to_string();
    // Strip ```json fences if Gemini added them.
    let trimmed = text.trim().trim_start_matches("```json")
        .trim_start_matches("```").trim_end_matches("```").trim();
    let v: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    Some((v["pass"].as_bool().unwrap_or(true),
          v["reason"].as_str().unwrap_or("").to_string()))
}

/// Send the published blog as a digest email to /you subscribers via Resend.
/// Best-effort; failure logged but doesn't block publish.
async fn send_blog_digest(db: &Db, slug: &str, title: &str, body_md: &str) -> Result<i64, String> {
    let resend_key = env::var("RESEND_API_KEY")
        .map_err(|_| "RESEND_API_KEY missing".to_string())?;
    let recipients: Vec<String> = {
        let conn = db.lock().unwrap();
        let result = match conn.prepare("SELECT email FROM you_users WHERE unsubscribed_at IS NULL") {
            Ok(mut stmt) => stmt.query_map([], |r| r.get::<_, String>(0))
                .map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>())
                .unwrap_or_default(),
            Err(e) => { eprintln!("[blog-digest] stmt: {e}"); Vec::new() }
        };
        result
    };
    if recipients.is_empty() { return Ok(0); }
    let preview = body_md.lines().take(8).collect::<Vec<_>>().join("\n");
    let body_html = md_to_html_simple(&preview);
    let url = format!("https://wearmu.com/blog/{slug}");
    let html = format!(
        r#"<div style="font-family:-apple-system,sans-serif;max-width:560px;margin:0 auto;padding:24px;color:#222">
        <div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.55;margin-bottom:18px">MU FIELD LOG</div>
        <h1 style="font-size:22px;font-weight:600;margin-bottom:18px">{title}</h1>
        <div style="font-size:14px;line-height:1.8;color:#444">{body_html}</div>
        <p style="margin-top:24px"><a href="{url}" style="color:#0A0A0A;text-decoration:underline">続きを読む →</a></p>
        <p style="font-size:10px;color:#999;margin-top:36px;line-height:1.7">毎朝 JST 9時に Gemini が生成・送信。配信停止は <a href="https://wearmu.com/you/unsubscribe">こちら</a>。</p>
        </div>"#,
        title = html_escape(title), body_html = body_html, url = url);
    // Resend supports batching via /emails/batch (up to 100 per request).
    // For our scale (<100), iterate.
    let client = reqwest::Client::new();
    let mut sent = 0i64;
    for to in recipients {
        let body = serde_json::json!({
            "from": "MU <info@enablerdao.com>",
            "to": [to],
            "subject": format!("📓 {} — MU Field log", title),
            "html": html,
        });
        let r = client.post("https://api.resend.com/emails")
            .header("Authorization", format!("Bearer {resend_key}"))
            .json(&body).send().await;
        match r {
            Ok(resp) if resp.status().is_success() => { sent += 1; }
            Ok(resp) => { eprintln!("[blog-digest] resend {}: {}",
                resp.status(), resp.text().await.unwrap_or_default()); }
            Err(e) => { eprintln!("[blog-digest] http: {e}"); }
        }
    }
    Ok(sent)
}

/// Cross-post the blog headline to X via the existing nanobot/twitter
/// integration if configured. Currently best-effort — checks for a
/// TWITTER_BEARER + X_AUTOPOST_ENDPOINT env var; if either is missing,
/// logs a no-op. Real X API access is outside this repo for now.
async fn cross_post_x(slug: &str, title: &str) -> Result<bool, String> {
    let endpoint = env::var("X_AUTOPOST_ENDPOINT").ok();
    let token    = env::var("X_AUTOPOST_TOKEN").ok();
    let (Some(endpoint), Some(token)) = (endpoint, token) else {
        return Ok(false);
    };
    let body = serde_json::json!({
        "text": format!("{} https://wearmu.com/blog/{slug}", title),
    });
    let r = reqwest::Client::new().post(&endpoint)
        .bearer_auth(&token).json(&body).send().await
        .map_err(|e| format!("x-autopost: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("x-autopost {}: {}",
            r.status(), r.text().await.unwrap_or_default()));
    }
    Ok(true)
}

#[derive(Deserialize)]
struct BlogPublishBody {
    admin_token: String,
    title: String,
    body_md: String,
    /// Optional. Defaults to "gemini-2.5-flash-via-actions" if unset.
    #[serde(default)]
    model: Option<String>,
    /// Echoed back into auto_blog_posts.stats_json for audit. Optional.
    #[serde(default)]
    stats_used: Option<serde_json::Value>,
    /// If set, override slug; defaults to `auto-{jst_today}`. Lets Actions
    /// back-fill a missed day.
    #[serde(default)]
    slug: Option<String>,
    /// Tag for the auto_blog_posts.origin audit column.
    /// Expected values: "actions", "fly", "manual".
    #[serde(default)]
    origin: Option<String>,
    /// Number of retries it took Actions to reach a good response.
    #[serde(default)]
    retry_count: Option<i64>,
    /// If true, skip email digest + X cross-post + Telegram notify.
    /// Used for dry-run / backfill scenarios.
    #[serde(default)]
    quiet: bool,
}

async fn admin_blog_publish(
    State(db): State<Db>,
    Json(body): Json<BlogPublishBody>,
) -> impl IntoResponse {
    if let Err(r) = require_admin_token(Some(&body.admin_token)) { return r; }
    // Input validation — defend against Gemini returning garbage or empty.
    let title = body.title.trim();
    let body_md = body.body_md.trim();
    if title.is_empty() || title.chars().count() > 120 {
        return (StatusCode::BAD_REQUEST,
            "title must be 1-120 chars").into_response();
    }
    if body_md.len() < 200 || body_md.len() > 8000 {
        return (StatusCode::BAD_REQUEST,
            "body_md must be 200-8000 bytes").into_response();
    }
    // Soft refusal detector — common Gemini failure patterns.
    let lower = body_md.to_lowercase();
    if lower.contains("i can't") || (lower.contains("申し訳") && body_md.len() < 600) {
        return (StatusCode::BAD_REQUEST,
            "body looks like a refusal / placeholder").into_response();
    }
    let slug = body.slug.clone().unwrap_or_else(|| format!("auto-{}", jst_today_str()));
    let model = body.model.clone().unwrap_or_else(|| "gemini-2.5-flash-via-actions".to_string());
    let origin = body.origin.clone().unwrap_or_else(|| "actions".to_string());
    let retry_count = body.retry_count.unwrap_or(0);
    let body_html = md_to_html_simple(body_md);
    let stats_json = body.stats_used
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "{}".to_string());
    let (inserted, already): (bool, bool) = {
        let conn = db.lock().unwrap();
        let already: bool = conn.query_row(
            "SELECT 1 FROM auto_blog_posts WHERE slug=?",
            params![slug], |r| r.get::<_, i64>(0),
        ).is_ok();
        if already {
            (false, true)
        } else {
            let n = conn.execute(
                "INSERT OR IGNORE INTO auto_blog_posts
                    (slug, title, body_html, body_md, model, stats_json,
                     origin, retry_count, published, created_at)
                 VALUES (?,?,?,?,?,?,?,?,1,?)",
                params![slug, title, body_html, body_md, model, stats_json,
                        origin, retry_count, chrono_now()],
            ).unwrap_or(0);
            (n > 0, false)
        }
    };

    // 2-pass review — informational only; we don't block publish on critic
    // feedback. Just log + include in response so Actions can surface.
    let review = if inserted && !body.quiet {
        review_blog_body(body_md, &body.stats_used.clone().unwrap_or(serde_json::Value::Null)).await
    } else { None };

    // Email digest to /you subscribers + X cross-post + Telegram notify —
    // only on fresh insert, not idempotent re-publish, and not in quiet mode.
    let (digest_sent, x_posted): (i64, bool) = if inserted && !body.quiet {
        let digest_sent = match send_blog_digest(&db, &slug, title, body_md).await {
            Ok(n) => n,
            Err(e) => { eprintln!("[blog-digest] {e}"); 0 }
        };
        let x_posted = cross_post_x(&slug, title).await.unwrap_or(false);
        // Mark audit columns
        if digest_sent > 0 || x_posted {
            let conn = db.lock().unwrap();
            let _ = conn.execute(
                "UPDATE auto_blog_posts SET
                    digest_sent_at = CASE WHEN ? > 0 THEN ? ELSE digest_sent_at END,
                    cross_post_x_at = CASE WHEN ?  THEN ? ELSE cross_post_x_at END
                 WHERE slug=?",
                params![digest_sent, chrono_now(), x_posted, chrono_now(), slug],
            );
        }
        // Telegram success notification (best-effort, fail-open).
        if let (Ok(tg_token), tg_chat) = (
            env::var("TELEGRAM_BOT_TOKEN"),
            env::var("TELEGRAM_CHAT_ID").unwrap_or_else(|_| "1136442501".into()),
        ) {
            let review_line = match &review {
                Some((pass, reason)) if *pass => format!("✓ review pass: {reason}"),
                Some((_, reason))             => format!("⚠ review flag: {reason}"),
                None                          => "review skipped".to_string(),
            };
            let msg = format!(
                "📓 Blog published — {}\n{}\norigin={origin} retries={retry_count}\n\
                 digest sent → {digest_sent} subs\nX cross-post: {}\n{review_line}\n\
                 https://wearmu.com/blog/{slug}",
                title,
                if x_posted { "✓" } else { "—" },
                if x_posted { "yes" } else { "no" },
            );
            let _ = reqwest::Client::new()
                .post(format!("https://api.telegram.org/bot{}/sendMessage", tg_token))
                .json(&serde_json::json!({
                    "chat_id": tg_chat, "text": msg, "disable_web_page_preview": false,
                }))
                .send().await;
        }
        (digest_sent, x_posted)
    } else {
        (0, false)
    };

    Json(serde_json::json!({
        "ok": true,
        "published": inserted,
        "already_existed": already,
        "slug": slug,
        "url": format!("https://wearmu.com/blog/{slug}"),
        "digest_sent": digest_sent,
        "x_posted": x_posted,
        "review_pass": review.as_ref().map(|(p,_)| *p),
        "review_reason": review.as_ref().map(|(_,r)| r.clone()),
    })).into_response()
}

async fn show_auto_blog(
    Path(slug): Path<String>,
    State(db): State<Db>,
) -> impl IntoResponse {
    let row: Option<(String, String, String)> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT title, body_html, created_at FROM auto_blog_posts
             WHERE slug=? AND published=1",
            params![slug], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).ok()
    };
    let Some((title, body_html, _ts)) = row else {
        return (StatusCode::NOT_FOUND, "auto-blog not found").into_response();
    };
    let title_attr = html_attr_escape(&title);
    let slug_attr  = html_attr_escape(&slug);
    let html = format!(r#"<!doctype html><html lang="ja"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} | MU 自動運営ノート</title>
<meta name="description" content="MU の AI 自動執筆 Field log。毎朝 JST 9:00 に Gemini が生成。">
<meta property="og:type" content="article">
<meta property="og:title" content="{title_attr}">
<meta property="og:description" content="MU の AI 自動執筆 Field log — 毎朝 JST 9:00 に Gemini が /api/transparency の生データから書きます。">
<meta property="og:image" content="https://wearmu.com/og.jpg">
<meta property="og:url" content="https://wearmu.com/blog/auto/{slug_attr}">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="{title_attr}">
<meta name="twitter:description" content="MU の AI 自動執筆 Field log — 毎朝 JST 9:00 に Gemini が書きます。">
<meta name="twitter:image" content="https://wearmu.com/og.jpg">
<link rel="icon" type="image/svg+xml" href="/favicon.svg">
<style>
:root{{--bg:#0A0A0A;--fg:#F5F5F0;--mute:rgba(245,245,240,0.6);--y:#e6c449;--card:#111}}
*{{margin:0;padding:0;box-sizing:border-box}}
body{{background:var(--bg);color:var(--fg);font-family:'Noto Serif JP','Helvetica Neue','Hiragino Sans',serif;line-height:1.95;font-size:16px;-webkit-font-smoothing:antialiased}}
nav{{position:sticky;top:0;background:rgba(10,10,10,0.85);backdrop-filter:blur(12px);border-bottom:1px solid rgba(255,255,255,0.06);padding:18px 32px;display:flex;justify-content:space-between;align-items:center;z-index:50;font-family:'Helvetica Neue',Arial,sans-serif}}
nav a{{color:var(--fg);text-decoration:none;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.85}}
nav .logo{{font-weight:700;letter-spacing:0.45em}}
article{{max-width:680px;margin:0 auto;padding:60px 32px 100px}}
.eyebrow{{font-family:'Helvetica Neue',Arial,sans-serif;font-size:10px;letter-spacing:0.4em;text-transform:uppercase;color:var(--y);opacity:0.85;margin-bottom:16px}}
h1{{font-size:clamp(26px,4vw,40px);font-weight:300;letter-spacing:0.02em;line-height:1.35;margin-bottom:18px}}
h2{{font-size:20px;font-weight:300;letter-spacing:0.02em;margin:48px 0 14px;padding-top:22px;border-top:1px solid rgba(255,255,255,0.08);font-family:'Helvetica Neue',Arial,sans-serif;color:var(--y)}}
h3{{font-size:15px;font-weight:500;margin:28px 0 10px;font-family:'Helvetica Neue',Arial,sans-serif}}
p{{margin:0 0 16px}} em{{color:var(--y);font-style:normal}} strong{{color:var(--fg);font-weight:500}}
ul{{margin:0 0 18px 22px;color:var(--mute)}} ul li{{margin-bottom:6px}}
a{{color:var(--y);text-decoration:underline;text-underline-offset:3px}}
.byline{{font-family:'Helvetica Neue',Arial,sans-serif;font-size:11px;letter-spacing:0.18em;text-transform:uppercase;opacity:0.55;margin-bottom:20px}}
.tag{{display:inline-block;font-size:10px;letter-spacing:0.18em;text-transform:uppercase;padding:3px 10px;background:rgba(230,196,73,0.12);color:var(--y);border-radius:2px;margin-right:8px}}
footer{{padding:48px 32px;border-top:1px solid rgba(255,255,255,0.06);text-align:center;font-size:11px;letter-spacing:0.2em;opacity:0.5}}
</style></head><body>
<nav><a href="/" class="logo">MU</a><a href="/blog/">/ Notes</a></nav>
<article>
  <div class="eyebrow">{day} · 自動運営ノート</div>
  <h1>{title}</h1>
  <div class="byline"><span class="tag">AI</span> by Gemini 2.5 Flash · 監修なし</div>
  {body_html}
  <p style="margin-top:48px;font-size:11px;opacity:0.5">— このノートは MU が毎朝 JST 9:00 に <a href="/api/transparency">/api/transparency</a> の生データを Gemini に渡して自動生成しています。事実は数字、文体は AI。</p>
</article>
<footer>MU — wearmu.com / <a href="/blog/" style="color:inherit">all notes →</a></footer>
</body></html>
"#,
        title = html_escape(&title),
        title_attr = title_attr,
        slug_attr  = slug_attr,
        day   = jst_today_str(),
        body_html = body_html,
    );
    axum::response::Html(html).into_response()
}

async fn list_auto_blog(State(db): State<Db>) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT slug, title, created_at FROM auto_blog_posts
         WHERE published=1 ORDER BY created_at DESC LIMIT 50"
    ) { Ok(s) => s, Err(_) => return Json(serde_json::json!({"posts":[]})).into_response() };
    let rows: Vec<serde_json::Value> = stmt.query_map([], |r| {
        Ok(serde_json::json!({
            "slug": r.get::<_, String>(0)?,
            "title": r.get::<_, String>(1)?,
            "created_at": r.get::<_, String>(2)?,
        }))
    }).map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>()).unwrap_or_default();
    Json(serde_json::json!({"posts": rows})).into_response()
}

/// Dynamic /sitemap.xml — serves the static base sitemap from disk and
/// appends one <url> per auto_blog_posts row before </urlset>. SEO bots
/// pick up the daily Field log without manual sitemap maintenance.
async fn dynamic_sitemap(State(db): State<Db>) -> Response {
    let base = std::fs::read_to_string("static/sitemap.xml")
        .unwrap_or_else(|_| "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\"></urlset>".to_string());
    let posts: Vec<(String, String)> = {
        let conn = db.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT slug, COALESCE(SUBSTR(created_at,1,10), '') AS d
             FROM auto_blog_posts WHERE published=1 ORDER BY created_at DESC LIMIT 365"
        ) { Ok(s) => s, Err(_) => return (
            [("content-type","application/xml")],
            base.clone(),
        ).into_response() };
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    let mut entries = String::new();
    for (slug, lastmod) in posts {
        entries.push_str(&format!(
            "  <url>\n    <loc>https://wearmu.com/blog/{slug}</loc>\n    \
             <lastmod>{lastmod}</lastmod>\n    \
             <changefreq>never</changefreq>\n    <priority>0.6</priority>\n  </url>\n",
            slug = slug, lastmod = lastmod));
    }
    let out = if base.contains("</urlset>") {
        base.replace("</urlset>", &format!("{entries}</urlset>"))
    } else {
        format!("{base}\n{entries}")
    };
    (
        [("content-type", "application/xml; charset=utf-8")],
        out,
    ).into_response()
}

// ── MU × SWEEP collab (draft, password-gated) ──────────────────────────────
// SWEEP社 の承認前なので強めに gate。`?pass=...` で 30 日 cookie をセット。
// 商品自体は collab_products に seed されており、buy ボタンは Stripe Checkout
// (price_data, server-controlled, 改竄不可) に飛ばす。

fn sweep_password() -> String {
    env::var("SWEEP_PAGE_PASSWORD").unwrap_or_else(|_| "sweep-2026".into())
}

fn has_sweep_cookie(headers: &HeaderMap, pw: &str) -> bool {
    headers.get("cookie").and_then(|v| v.to_str().ok()).map(|c| {
        c.split(';').any(|p| p.trim() == format!("mu_sweep_pass={}", pw))
    }).unwrap_or(false)
}

async fn show_sweep_page(
    State(db): State<Db>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let pw = sweep_password();
    let entered = q.get("pass").map(String::as_str).unwrap_or("");
    let authed = entered == pw || has_sweep_cookie(&headers, &pw);

    if !authed {
        return axum::response::Html(SWEEP_GATE_HTML).into_response();
    }

    // Build product list HTML server-side (no caching of the gate cookie path)
    type Row = (i64, String, String, String, String, i64, Option<String>, i64);
    let items: Vec<Row> = {
        let conn = db.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, slug, category, name, COALESCE(description,''), price_jpy, image_url,
                    COALESCE(lead_time_days, 14)
             FROM collab_products WHERE partner='sweep' AND active=1
             ORDER BY id"
        ) { Ok(s) => s, Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db").into_response() };
        stmt.query_map([], |r| Ok((
            r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?
        ))).map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    };

    let cards = items.iter().map(|(id, slug, cat, name, desc, price, image, lead)| {
        // Image fallback: if no image_url set yet, show a styled placeholder
        // with the category label, so the gallery is never empty.
        let image_block = match image.as_deref().filter(|u| !u.is_empty() && u.starts_with("http")) {
            Some(u) => format!(
                r##"<a href="#buy-{id}" class="img-wrap" aria-label="{name_attr}"><img src="{src}" alt="{name_attr}" loading="lazy"></a>"##,
                src = html_attr_escape(u), name_attr = html_attr_escape(name), id = id),
            None => format!(
                r#"<div class="img-wrap placeholder"><span>{glyph}</span><small>generating…</small></div>"#,
                glyph = html_attr_escape(cat.chars().next().map(|c| c.to_string()).unwrap_or("•".into()).as_str())),
        };
        format!(r#"<article class="card" data-slug="{slug}">
  {image}
  <div class="body">
    <div class="cat">{cat}</div>
    <h3 id="buy-{id}">{name}</h3>
    <p class="desc">{desc}</p>
    <div class="lead">📦 {lead}日でお届け · Printful 経由</div>
    <div class="row">
      <span class="price">¥{price_fmt}</span>
      <select id="size-{id}" class="size" aria-label="size">
        <option>S</option><option selected>M</option><option>L</option><option>XL</option>
      </select>
      <button class="buy" data-slug="{slug}" data-id="{id}">注文 →</button>
    </div>
    <div class="fb">
      <button class="sig love" data-slug="{slug}" aria-label="好き">👍 <span class="n n-love">0</span></button>
      <button class="sig meh"  data-slug="{slug}" aria-label="いまいち">👎 <span class="n n-meh">0</span></button>
      <button class="sig comment" data-slug="{slug}" aria-label="コメント">💬 改善案</button>
    </div>
    <div class="fb-form" hidden>
      <textarea placeholder="何が違う？ どう変えたい？ (任意 1000 字以内)" maxlength="1000"></textarea>
      <input type="email" placeholder="返信を希望される方は email (任意)" autocomplete="email">
      <div class="fb-row">
        <button class="fb-send" data-slug="{slug}">送る</button>
        <button class="fb-cancel" type="button">×</button>
      </div>
      <div class="fb-msg"></div>
    </div>
  </div>
</article>"#,
        image = image_block,
        cat = html_attr_escape(cat), name = html_attr_escape(name),
        desc = html_attr_escape(desc), price_fmt = format_jpy(*price),
        id = id, slug = html_attr_escape(slug),
    )}).collect::<Vec<_>>().join("\n");

    let pw_attr = html_attr_escape(&pw);
    let body = format!(r#"<!doctype html><html lang="ja"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>MU × SWEEP — Draft preview (BJJ collab) | wearmu.com</title>
<meta name="description" content="MU と北参道の BJJ アパレル SWEEP のコラボ draft。ラッシュガード / ファイトショーツ / スパッツ / フーディ / T。SWEEP社確認前のため非公開。">
<meta name="robots" content="noindex,nofollow">
<link rel="icon" type="image/svg+xml" href="/favicon.svg">
<style>
:root{{--bg:#0A0A0A;--fg:#F5F5F0;--mute:rgba(245,245,240,0.62);--y:#e6c449;--card:#111;--red:#C8362C}}
*{{margin:0;padding:0;box-sizing:border-box}}
body{{background:var(--bg);color:var(--fg);font-family:'Helvetica Neue','Hiragino Sans',Arial,sans-serif;line-height:1.85;-webkit-font-smoothing:antialiased}}
nav{{position:sticky;top:0;background:rgba(10,10,10,0.88);backdrop-filter:blur(12px);border-bottom:1px solid rgba(255,255,255,0.06);padding:18px 32px;display:flex;justify-content:space-between;align-items:center;z-index:50}}
nav a{{color:var(--fg);text-decoration:none;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.85}}
nav .logo{{font-weight:700;letter-spacing:0.45em}}
header{{padding:72px 32px 30px;max-width:880px;margin:0 auto;text-align:center}}
header .eyebrow{{font-size:10px;letter-spacing:0.4em;text-transform:uppercase;color:var(--y);opacity:0.85;margin-bottom:14px}}
header h1{{font-size:clamp(28px,5vw,52px);font-weight:200;letter-spacing:0.02em;line-height:1.25;margin-bottom:16px}}
header h1 em{{color:var(--y);font-style:normal;font-weight:300}}
header .brandline{{display:flex;align-items:center;justify-content:center;gap:18px;margin:8px auto 28px;flex-wrap:wrap}}
header .brandline-mu{{font-size:clamp(28px,5vw,48px);font-weight:700;letter-spacing:0.42em}}
header .brandline-x{{font-size:clamp(20px,3.5vw,32px);font-weight:200;color:var(--mute)}}
header .brandline-sweep{{height:clamp(28px,3.6vw,44px);width:auto;filter:invert(1);opacity:0.92}}
header .lede{{color:var(--mute);font-size:14px;max-width:560px;margin:0 auto 22px;line-height:1.95}}
header .warn{{display:inline-block;font-size:10px;letter-spacing:0.22em;text-transform:uppercase;background:rgba(200,54,44,0.12);color:var(--red);padding:8px 18px;border-radius:2px;margin-top:8px}}
.grid{{max-width:1100px;margin:30px auto 100px;padding:0 32px;display:grid;grid-template-columns:repeat(auto-fit,minmax(260px,1fr));gap:18px}}
.card{{background:var(--card);border:1px solid rgba(255,255,255,0.06);border-radius:2px;display:flex;flex-direction:column;overflow:hidden;transition:border-color 0.2s ease}}
.card:hover{{border-color:rgba(230,196,73,0.45)}}
.card .img-wrap{{display:block;aspect-ratio:4/5;background:#0a0a0a;overflow:hidden;position:relative}}
.card .img-wrap img{{width:100%;height:100%;object-fit:cover;display:block}}
.card .img-wrap.placeholder{{display:flex;flex-direction:column;align-items:center;justify-content:center;font-family:'Helvetica Neue',Arial,sans-serif}}
.card .img-wrap.placeholder span{{font-size:48px;font-weight:200;color:rgba(230,196,73,0.4)}}
.card .img-wrap.placeholder small{{font-size:9px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.4;margin-top:8px}}
.card .body{{padding:22px 22px 24px;display:flex;flex-direction:column;gap:8px;flex:1}}
.card .cat{{font-size:9px;letter-spacing:0.32em;text-transform:uppercase;color:var(--y);opacity:0.85}}
.card h3{{font-size:17px;font-weight:400;letter-spacing:0.01em;margin:2px 0 4px}}
.card .desc{{color:var(--mute);font-size:12.5px;line-height:1.85;flex:1}}
.card .lead{{font-size:9.5px;letter-spacing:0.16em;color:rgba(245,245,240,0.55);margin-top:8px}}
.card .row{{display:flex;align-items:center;gap:8px;margin-top:14px;flex-wrap:wrap}}
.card .price{{font-size:16px;color:var(--y);font-variant-numeric:tabular-nums;margin-right:auto}}
.card select{{background:#000;color:var(--fg);border:1px solid rgba(255,255,255,0.18);font-size:12px;padding:7px 10px;border-radius:2px}}
.card .buy{{background:var(--y);color:#000;border:0;font-family:inherit;font-size:11px;letter-spacing:0.28em;text-transform:uppercase;font-weight:700;padding:10px 16px;cursor:pointer;border-radius:2px}}
.card .buy:hover{{opacity:0.85}}
.card .buy:disabled{{opacity:0.4;cursor:wait}}
.card .fb{{display:flex;gap:6px;margin-top:12px;border-top:1px solid rgba(255,255,255,0.06);padding-top:12px;flex-wrap:wrap}}
.card .sig{{background:transparent;color:var(--mute);border:1px solid rgba(255,255,255,0.12);font-family:inherit;font-size:11px;padding:6px 10px;cursor:pointer;border-radius:2px;display:inline-flex;align-items:center;gap:4px;transition:all 0.15s ease}}
.card .sig:hover{{border-color:rgba(230,196,73,0.45);color:var(--fg)}}
.card .sig.on{{background:rgba(230,196,73,0.12);color:var(--y);border-color:rgba(230,196,73,0.45)}}
.card .sig.comment{{margin-left:auto;border-color:rgba(255,255,255,0.08);font-size:10.5px}}
.card .sig .n{{font-variant-numeric:tabular-nums;font-size:10.5px;opacity:0.7}}
.card .fb-form{{margin-top:10px;display:flex;flex-direction:column;gap:6px}}
.card .fb-form textarea{{background:#000;color:var(--fg);border:1px solid rgba(255,255,255,0.14);border-radius:2px;font-family:inherit;font-size:12px;padding:8px 10px;line-height:1.7;min-height:64px;resize:vertical}}
.card .fb-form input{{background:#000;color:var(--fg);border:1px solid rgba(255,255,255,0.14);border-radius:2px;font-family:inherit;font-size:12px;padding:7px 10px}}
.card .fb-row{{display:flex;gap:6px}}
.card .fb-send{{flex:1;background:rgba(230,196,73,0.85);color:#000;border:0;font-family:inherit;font-size:10.5px;letter-spacing:0.26em;text-transform:uppercase;font-weight:700;padding:8px 12px;cursor:pointer;border-radius:2px}}
.card .fb-cancel{{background:transparent;color:var(--mute);border:1px solid rgba(255,255,255,0.12);padding:8px 12px;cursor:pointer;border-radius:2px}}
.card .fb-msg{{font-size:11px;color:var(--y);min-height:14px;line-height:1.6}}
.note{{max-width:680px;margin:40px auto 60px;padding:18px 22px;background:rgba(230,196,73,0.06);border-left:2px solid var(--y);font-size:12.5px;line-height:1.95;color:rgba(245,245,240,0.78)}}
footer{{padding:48px 32px 80px;border-top:1px solid rgba(255,255,255,0.06);text-align:center;font-size:11px;letter-spacing:0.2em;opacity:0.5}}
footer a{{color:inherit;text-decoration:underline}}
#msg{{max-width:680px;margin:16px auto;text-align:center;font-size:11px;letter-spacing:0.05em;color:var(--mute);min-height:14px}}
</style></head><body>
<nav><a href="/" class="logo">MU</a><a href="/vision">Vision</a></nav>
<header>
  <div class="eyebrow">Draft Preview — <em>SWEEP社 確認前</em></div>
  <div class="brandline">
    <span class="brandline-mu">MU</span>
    <span class="brandline-x">×</span>
    <img class="brandline-sweep" alt="SIIIEEP" src="https://lifestyle.wearmu.com/sweep/_logo.png" loading="eager">
  </div>
  <h1>北参道の BJJ アパレルと、<br>AI ブランドの試作。</h1>
  <p class="lede">
    SIIIEEP は北参道の道場発、BJJ ラッシュガード / スパッツ / ファイトショーツのアパレル。<br>
    濱田 (柔術青帯、北参道で SIIIEEP の練習着を着てる MU 創業者) が「MU の AI デザインを SIIIEEP の身体性で着たい」と思って、コラボ案を作った。<br>
    本ページは <em>SIIIEEP社 確認前のプレビュー</em>。
    13 アイテムは <strong>Printful の全面プリント / 刺繍で 10-14 日に発送</strong>される本物の購入。
    Gi・帯・タープ・マウスガードケースの 4 アイテムは SIIIEEP社 と本契約完了後に解放。
  </p>
  <div class="warn">⚠ Draft — SIIIEEP社 サインオフ前のため password gate。13 商品は今日から実際に注文可能 (Printful 経由)。</div>
</header>
<div class="note">
  パスワードは知り合いに渡して下さい。リンクには <code>?pass={pw}</code> が必要 (このページが見えてるという事はあなたは合っています)。
  cookie は 30 日間有効。<br>
  公開を急ぐ場合は <a href="mailto:info@enablerdao.com">info@enablerdao.com</a>。
</div>
<div class="grid">
{cards}
</div>
<div id="msg"></div>
<footer>
  MU × SWEEP draft preview · 株式会社イネブラ (Enabler Inc.) ·
  <a href="mailto:info@enablerdao.com">info@enablerdao.com</a> ·
  <a href="/sweep?logout=1">ログアウト</a>
</footer>
<script>
document.querySelectorAll('.card .buy').forEach(btn => {{
  btn.addEventListener('click', async () => {{
    const slug = btn.dataset.slug;
    const id   = btn.dataset.id;
    const size = document.getElementById('size-' + id).value;
    const msg  = document.getElementById('msg');
    btn.disabled = true; const orig = btn.textContent; btn.textContent = '読み込み中…';
    msg.textContent = '';
    try {{
      const r = await fetch('/api/sweep/checkout', {{
        method: 'POST', headers: {{'Content-Type': 'application/json'}},
        body: JSON.stringify({{slug, size}})
      }});
      if (!r.ok) throw new Error('HTTP ' + r.status);
      const d = await r.json();
      if (d.url) window.location.href = d.url;
      else throw new Error(d.error || 'no url');
    }} catch (e) {{
      btn.disabled = false; btn.textContent = orig;
      msg.textContent = 'エラー: ' + e.message + ' — SWEEP社 承認前のため Stripe key 未設定の可能性あり';
    }}
  }});
}});
// ── 好き嫌いボタン + 改善案 FB ──
async function sendSignal(slug, kind, comment, email) {{
  const r = await fetch('/api/sweep/signal', {{
    method: 'POST', headers: {{'Content-Type': 'application/json'}},
    body: JSON.stringify({{slug, kind, comment: comment || '', email: email || ''}})
  }});
  if (!r.ok) throw new Error('HTTP ' + r.status);
  return await r.json();
}}
function updateCounts(card, j) {{
  if (j && typeof j.loves === 'number') {{
    const l = card.querySelector('.n-love'); if (l) l.textContent = j.loves;
  }}
  if (j && typeof j.mehs === 'number') {{
    const m = card.querySelector('.n-meh'); if (m) m.textContent = j.mehs;
  }}
}}
// Preload counts
fetch('/api/sweep/signals').then(r => r.json()).then(d => {{
  const sig = d.signals || {{}};
  document.querySelectorAll('.card').forEach(card => {{
    const slug = card.dataset.slug; if (!slug || !sig[slug]) return;
    updateCounts(card, sig[slug]);
  }});
}}).catch(() => {{}});
// Click handlers
document.querySelectorAll('.card .sig').forEach(btn => {{
  btn.addEventListener('click', async () => {{
    const card = btn.closest('.card');
    const slug = btn.dataset.slug;
    if (btn.classList.contains('comment')) {{
      const form = card.querySelector('.fb-form');
      form.hidden = !form.hidden;
      if (!form.hidden) form.querySelector('textarea').focus();
      return;
    }}
    const kind = btn.classList.contains('love') ? 'love' : 'meh';
    if (btn.classList.contains('on')) return;
    btn.classList.add('on');
    try {{
      const j = await sendSignal(slug, kind);
      updateCounts(card, j);
      if (kind === 'meh') {{
        // 👎 → 改善案フォームを自動展開（理由を聞く）
        const form = card.querySelector('.fb-form');
        form.hidden = false;
        const ta = form.querySelector('textarea');
        ta.placeholder = '👎 ありがとうございます。どこを変えたら買いますか？';
        ta.focus();
      }}
    }} catch (e) {{
      btn.classList.remove('on');
      card.querySelector('.fb-msg').textContent = 'エラー: ' + e.message;
    }}
  }});
}});
document.querySelectorAll('.card .fb-cancel').forEach(b => {{
  b.addEventListener('click', () => {{
    b.closest('.fb-form').hidden = true;
  }});
}});
document.querySelectorAll('.card .fb-send').forEach(btn => {{
  btn.addEventListener('click', async () => {{
    const card = btn.closest('.card');
    const form = card.querySelector('.fb-form');
    const msg  = form.querySelector('.fb-msg');
    const text = form.querySelector('textarea').value.trim();
    const email = form.querySelector('input[type=email]').value.trim();
    if (!text) {{ msg.textContent = 'コメントを入力してください'; return; }}
    btn.disabled = true; const orig = btn.textContent; btn.textContent = '送信中…';
    msg.textContent = '';
    try {{
      const j = await sendSignal(btn.dataset.slug, 'comment', text, email);
      updateCounts(card, j);
      form.querySelector('textarea').value = '';
      msg.textContent = '✓ 受け取りました。ありがとうございます。次の試作に反映します。';
      setTimeout(() => {{ form.hidden = true; msg.textContent = ''; }}, 4500);
    }} catch (e) {{
      msg.textContent = 'エラー: ' + e.message;
    }} finally {{
      btn.disabled = false; btn.textContent = orig;
    }}
  }});
}});
// ?logout=1
if (new URLSearchParams(location.search).get('logout') === '1') {{
  document.cookie = 'mu_sweep_pass=; max-age=0; path=/';
  location.href = '/sweep';
}}
</script>
</body></html>"#);

    let mut resp = axum::response::Html(body).into_response();
    if entered == pw {
        resp.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_str(&format!(
                "mu_sweep_pass={}; Max-Age=2592000; Path=/; HttpOnly; SameSite=Lax",
                pw
            )).unwrap_or_else(|_| HeaderValue::from_static("")),
        );
    }
    resp.headers_mut().insert(
        "X-Robots-Tag", HeaderValue::from_static("noindex, nofollow"),
    );
    resp
}

const SWEEP_GATE_HTML: &str = r#"<!doctype html><html lang="ja"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>MU × SWEEP — Restricted preview</title>
<meta name="robots" content="noindex,nofollow">
<link rel="icon" type="image/svg+xml" href="/favicon.svg">
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;min-height:100vh;display:flex;align-items:center;justify-content:center;padding:32px}
.box{max-width:420px;text-align:center;width:100%}
.logo{font-weight:700;letter-spacing:0.45em;font-size:28px;margin-bottom:30px}
h1{font-size:13px;letter-spacing:0.35em;text-transform:uppercase;color:#e6c449;margin-bottom:16px;opacity:0.85}
p{color:rgba(245,245,240,0.7);font-size:13px;line-height:1.9;margin-bottom:24px}
input{background:#000;color:#F5F5F0;border:1px solid rgba(255,255,255,0.22);padding:14px 16px;font-family:inherit;font-size:14px;width:100%;border-radius:2px;letter-spacing:0.08em;margin-bottom:14px}
input:focus{outline:none;border-color:#e6c449}
button{background:#e6c449;color:#000;border:0;font-family:inherit;font-size:11px;letter-spacing:0.32em;text-transform:uppercase;font-weight:700;padding:14px 28px;cursor:pointer;border-radius:2px;width:100%}
button:hover{opacity:0.85}
.foot{margin-top:30px;font-size:10px;letter-spacing:0.22em;text-transform:uppercase;opacity:0.45}
.foot a{color:inherit;text-decoration:underline}
</style></head><body>
<form class="box" method="get" action="/sweep">
  <div class="logo">MU × SWEEP</div>
  <h1>Draft preview · password required</h1>
  <p>SWEEP社 サインオフ前のため、このページは関係者限定です。<br>パスワードをお持ちでない方は <a style="color:#e6c449" href="mailto:info@enablerdao.com">info@enablerdao.com</a> までご連絡ください。</p>
  <input name="pass" type="password" placeholder="password" autofocus autocomplete="current-password">
  <button type="submit">Enter →</button>
  <div class="foot"><a href="/">← MU トップへ戻る</a></div>
</form>
</body></html>"#;

#[derive(Deserialize)]
struct SweepCheckoutBody {
    slug: String,
    #[serde(default)] size: String,
}

// ── SWEEP 好き嫌い + コメント (お客様 → AI/ops 改善ループ) ──────────────
#[derive(Deserialize)]
struct SweepSignalBody {
    slug: String,
    kind: String,                       // 'love' | 'meh' | 'comment'
    #[serde(default)] comment: String,  // 任意の自由記述
    #[serde(default)] email: String,
}

fn read_or_set_visitor_cookie(headers: &HeaderMap) -> (String, Option<HeaderValue>) {
    let existing = headers.get("cookie").and_then(|v| v.to_str().ok())
        .and_then(|c| c.split(';').find_map(|p| {
            let p = p.trim();
            p.strip_prefix("mu_v=").map(|s| s.to_string())
        }));
    if let Some(v) = existing { return (v, None); }
    // generate 16-char hex token via sha256(now + UA + remote hints)
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(chrono_now().as_bytes());
    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("");
    h.update(ua.as_bytes());
    let ip = headers.get("fly-client-ip").or_else(|| headers.get("x-forwarded-for"))
        .and_then(|v| v.to_str().ok()).unwrap_or("");
    h.update(ip.as_bytes());
    let token: String = hex::encode(&h.finalize()[..8]);
    let setcookie = HeaderValue::from_str(&format!(
        "mu_v={}; Max-Age=31536000; Path=/; HttpOnly; SameSite=Lax",
        token
    )).ok();
    (token, setcookie)
}

async fn sweep_signal(
    State(db): State<Db>,
    headers: HeaderMap,
    Json(body): Json<SweepSignalBody>,
) -> impl IntoResponse {
    let allowed = ["love", "meh", "comment"];
    if !allowed.contains(&body.kind.as_str()) {
        return (StatusCode::BAD_REQUEST, "bad kind").into_response();
    }
    if body.slug.is_empty() || body.slug.len() > 80 {
        return (StatusCode::BAD_REQUEST, "bad slug").into_response();
    }
    let comment = body.comment.trim().chars().take(1000).collect::<String>();
    let email = body.email.trim().chars().take(200).collect::<String>();
    // Require a comment for 'comment' kind
    if body.kind == "comment" && comment.is_empty() {
        return (StatusCode::BAD_REQUEST, "comment empty").into_response();
    }
    // Validate slug exists
    {
        let conn = db.lock().unwrap();
        let ok: bool = conn.query_row(
            "SELECT 1 FROM collab_products WHERE slug=? AND partner='sweep'",
            params![body.slug], |_| Ok(true),
        ).unwrap_or(false);
        if !ok { return (StatusCode::NOT_FOUND, "unknown slug").into_response(); }
    }
    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok())
        .map(|s| s.chars().take(200).collect::<String>()).unwrap_or_default();
    let (token, setcookie) = read_or_set_visitor_cookie(&headers);

    // Rate-limit: same (visitor, slug, kind) ignored if within 60s
    let now_s: i64 = chrono_now().parse().unwrap_or(0);
    {
        let conn = db.lock().unwrap();
        let recent: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sweep_signals
             WHERE visitor_token=? AND slug=? AND kind=?
               AND CAST(created_at AS INTEGER) >= ?",
            params![token, body.slug, body.kind, now_s - 60], |r| r.get(0),
        ).unwrap_or(0);
        if recent == 0 {
            let _ = conn.execute(
                "INSERT INTO sweep_signals
                     (slug, kind, comment, email, visitor_token, user_agent, created_at)
                 VALUES (?,?,?,?,?,?,?)",
                params![
                    body.slug, body.kind,
                    if comment.is_empty() { None } else { Some(&comment) },
                    if email.is_empty() { None } else { Some(&email) },
                    token, ua, now_s.to_string(),
                ],
            );
        }
    }

    // Notify ops for any comment or strong dislike (so we can react quickly)
    if body.kind == "comment" || (body.kind == "meh" && !comment.is_empty()) {
        let tg_token = env::var("TELEGRAM_BOT_TOKEN").unwrap_or_default();
        let tg_chat  = env::var("TELEGRAM_CHAT_ID").unwrap_or_else(|_| "1136442501".into());
        if !tg_token.is_empty() {
            let icon = if body.kind == "meh" { "👎" } else { "💬" };
            let body_txt = format!(
                "{} SWEEP fb [{}]\n{}\n{}",
                icon, body.slug,
                if email.is_empty() { "(no email)" } else { &email },
                comment.chars().take(800).collect::<String>(),
            );
            let _ = reqwest::Client::new()
                .post(format!("https://api.telegram.org/bot{}/sendMessage", tg_token))
                .json(&serde_json::json!({"chat_id": tg_chat, "text": body_txt, "disable_web_page_preview": true}))
                .send().await;
        }
    }

    // Return totals so the UI can update the count immediately
    let (loves, mehs, comments) = {
        let conn = db.lock().unwrap();
        let l: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sweep_signals WHERE slug=? AND kind='love'",
            params![body.slug], |r| r.get(0)).unwrap_or(0);
        let m: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sweep_signals WHERE slug=? AND kind='meh'",
            params![body.slug], |r| r.get(0)).unwrap_or(0);
        let c: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sweep_signals WHERE slug=? AND kind='comment'",
            params![body.slug], |r| r.get(0)).unwrap_or(0);
        (l, m, c)
    };
    let mut resp = Json(serde_json::json!({
        "ok": true, "loves": loves, "mehs": mehs, "comments": comments,
    })).into_response();
    if let Some(c) = setcookie { resp.headers_mut().insert(header::SET_COOKIE, c); }
    resp
}

/// GET /api/sweep/signals — totals per slug, used by the page to render counts.
async fn sweep_signals_summary(State(db): State<Db>) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT slug, kind, COUNT(*) FROM sweep_signals GROUP BY slug, kind"
    ) { Ok(s) => s, Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db").into_response() };
    let mut map: std::collections::HashMap<String, (i64, i64, i64)> = std::collections::HashMap::new();
    let rows = stmt.query_map([], |r| Ok((
        r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,i64>(2)?
    ))).map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>()).unwrap_or_default();
    for (slug, kind, n) in rows {
        let e = map.entry(slug).or_insert((0,0,0));
        match kind.as_str() {
            "love" => e.0 = n,
            "meh"  => e.1 = n,
            "comment" => e.2 = n,
            _ => {}
        }
    }
    let out: serde_json::Map<String, serde_json::Value> = map.into_iter().map(|(slug, (l,m,c))| (
        slug, serde_json::json!({"loves": l, "mehs": m, "comments": c})
    )).collect();
    Json(serde_json::json!({"signals": out})).into_response()
}

/// GET /api/admin/sweep_signals?token=… — raw feedback list for ops.
async fn admin_sweep_signals(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    if let Err(r) = require_admin_token(q.get("token")) { return r; }
    let conn = db.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT s.slug, s.kind, COALESCE(s.comment,''), COALESCE(s.email,''),
                COALESCE(s.visitor_token,''), s.created_at, COALESCE(p.name,'')
         FROM sweep_signals s
         LEFT JOIN collab_products p ON p.slug=s.slug
         ORDER BY s.id DESC LIMIT 500"
    ) { Ok(s) => s, Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db").into_response() };
    let rows = stmt.query_map([], |r| Ok(serde_json::json!({
        "slug": r.get::<_,String>(0)?, "kind": r.get::<_,String>(1)?,
        "comment": r.get::<_,String>(2)?, "email": r.get::<_,String>(3)?,
        "visitor_token": r.get::<_,String>(4)?, "created_at": r.get::<_,String>(5)?,
        "product_name": r.get::<_,String>(6)?,
    }))).map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>()).unwrap_or_default();
    Json(serde_json::json!({"signals": rows, "count": rows.len()})).into_response()
}

async fn sweep_checkout(
    State(db): State<Db>,
    Json(body): Json<SweepCheckoutBody>,
) -> impl IntoResponse {
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    if stripe_key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "checkout disabled").into_response();
    }
    // Lookup product (server-trusted price)
    let row: Option<(i64, String, String, i64)> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT id, name, COALESCE(category,''), price_jpy
             FROM collab_products WHERE slug=? AND partner='sweep' AND active=1",
            params![body.slug],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).ok()
    };
    let Some((product_id, name, category, price_jpy)) = row else {
        return (StatusCode::NOT_FOUND, "product not found").into_response();
    };
    let price = price_jpy.clamp(500, 99_800);
    let size = body.size.chars().take(8).collect::<String>();
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    let form: Vec<(&str, String)> = vec![
        ("mode", "payment".into()),
        ("currency", "jpy".into()),
        ("allow_promotion_codes", "true".into()),
        ("success_url", format!("{}/sweep?paid=ok", base_url)),
        ("cancel_url",  format!("{}/sweep?paid=cancel", base_url)),
        ("line_items[0][quantity]", "1".into()),
        ("line_items[0][price_data][currency]", "jpy".into()),
        ("line_items[0][price_data][unit_amount]", price.to_string()),
        ("line_items[0][price_data][product_data][name]",
         format!("{} ({}) · MU×SWEEP draft", name, category)),
        ("metadata[collab]", "sweep".into()),
        ("metadata[collab_product_id]", product_id.to_string()),
        ("metadata[slug]", body.slug.clone()),
        ("metadata[size]", size),
        ("shipping_address_collection[allowed_countries][0]", "JP".into()),
    ];
    let resp = reqwest::Client::new()
        .post("https://api.stripe.com/v1/checkout/sessions")
        .basic_auth(&stripe_key, None::<&str>)
        .form(&form)
        .send().await;
    match resp {
        Ok(r) if r.status().is_success() => {
            let j: serde_json::Value = r.json().await.unwrap_or_default();
            let url = j["url"].as_str().unwrap_or("/").to_string();
            Json(serde_json::json!({"url": url, "price_jpy": price})).into_response()
        }
        Ok(r) => {
            let s = r.status();
            let t = r.text().await.unwrap_or_default();
            eprintln!("[sweep/checkout] stripe {}: {}", s, t.chars().take(200).collect::<String>());
            (StatusCode::BAD_GATEWAY, "stripe error").into_response()
        }
        Err(e) => {
            eprintln!("[sweep/checkout] reqwest: {}", e);
            (StatusCode::BAD_GATEWAY, "stripe network").into_response()
        }
    }
}

fn format_jpy(n: i64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 { out.push(','); }
        out.push(c);
    }
    out.chars().rev().collect()
}

// ── MA Council briefs (Gemini が議題を集約) + voting ───────────────────────

fn iso_week_start_jst() -> String {
    let now_s: i64 = chrono_now().parse().unwrap_or(0);
    // JST shift + back to Monday
    let jst = now_s + 9 * 3600;
    let day = jst / 86_400;
    let dow_mon = (day + 3).rem_euclid(7); // 1970-01-01 = Thu, +3 → Mon=0
    let monday_days = day - dow_mon;
    let (y, m, d) = civil_from_days(monday_days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

async fn admin_council_compose(
    State(db): State<Db>,
    Json(body): Json<AutoBlogBody>,
) -> impl IntoResponse {
    if let Err(r) = require_admin_token(Some(&body.admin_token)) { return r; }
    let week = iso_week_start_jst();
    let slug = format!("council-{}", week);
    {
        let conn = db.lock().unwrap();
        let exists: bool = conn.query_row(
            "SELECT 1 FROM ma_council_briefs WHERE slug=?",
            params![slug], |r| r.get::<_, i64>(0),
        ).is_ok();
        if exists {
            return Json(serde_json::json!({"ok": true, "skipped": true, "slug": slug})).into_response();
        }
    }

    // Pull the last 30 days of MA Council feedback + general high-signal feedback
    let inputs: Vec<(String, String, i64)> = {
        let conn = db.lock().unwrap();
        let cutoff: i64 = chrono_now().parse::<i64>().unwrap_or(0) - 30 * 86_400;
        let mut stmt = match conn.prepare(
            "SELECT message, COALESCE(email,'anon'), is_ma_council
             FROM customer_feedback
             WHERE CAST(created_at AS INTEGER) >= ?
             ORDER BY is_ma_council DESC, id DESC LIMIT 40"
        ) {
            Ok(s) => s,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db").into_response(),
        };
        stmt.query_map(params![cutoff], |r| Ok((
            r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?
        ))).map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>()).unwrap_or_default()
    };

    let context: String = inputs.iter().enumerate().map(|(i, (m, e, council))| {
        let tag = if *council == 1 { "[Council] " } else { "" };
        format!("{}. {}{}: {}", i + 1, tag, e, m.chars().take(280).collect::<String>())
    }).collect::<Vec<_>>().join("\n");

    let key = match env::var("GEMINI_API_KEY").ok() {
        Some(k) if !k.is_empty() => k,
        _ => return (StatusCode::SERVICE_UNAVAILABLE, "GEMINI_API_KEY missing").into_response(),
    };

    let prompt = format!("あなたは MU ブランドの議題集計 AI です。週次 MA Council Brief を以下のフォーマットで生成してください。\n\n過去 30 日のお客様フィードバック (上位 40 件、Council 優先):\n{context}\n\n出力フォーマット (JSON のみ、コードフェンス不要):\n{{\n  \"title\": \"今週の MA Council Brief — YYYY 週X (28字以内)\",\n  \"body_md\": \"## 1. 今週のテーマ\\n## 2. お客様の声 (要約)\\n## 3. 議題\",\n  \"agendas\": [\n    {{\"id\": \"a1\", \"q\": \"次月の MUGEN 価格レンジを変更すべきか？\", \"options\": [\"¥4,000–6,000 (現行)\", \"¥5,000–8,000\", \"¥6,000–10,000\"]}},\n    {{\"id\": \"a2\", \"q\": \"新カテゴリ (sweat / longsleeve) を投入するか？\", \"options\": [\"sweat 優先\", \"longsleeve 優先\", \"T シャツ集中\"]}}\n  ]\n}}\n\nルール:\n- 議題は 2〜4 件\n- 各議題の options は 2〜4 個\n- お客様の生の声を 3 件以上 body_md に引用 (短く)\n- 捏造禁止 — フィードバックに無い議題は出さない\n- 末尾に「— 集計: Gemini 2.5 / 投票: MA Council メンバー」");

    let req_body = serde_json::json!({"contents": [{"parts": [{"text": prompt}]}]});
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        key);
    let resp = match reqwest::Client::new().post(&url).json(&req_body).send().await {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_GATEWAY, format!("gemini: {e}")).into_response(),
    };
    if !resp.status().is_success() {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return (StatusCode::BAD_GATEWAY,
            format!("gemini {}: {}", s, t.chars().take(200).collect::<String>())).into_response();
    }
    let j: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_GATEWAY, format!("json: {e}")).into_response(),
    };
    let text = j["candidates"][0]["content"]["parts"][0]["text"]
        .as_str().unwrap_or("").trim()
        .trim_start_matches("```json").trim_start_matches("```")
        .trim_end_matches("```").trim().to_string();
    let parsed: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_GATEWAY,
            format!("gemini json parse: {e}, raw: {}", text.chars().take(300).collect::<String>())).into_response(),
    };
    let title = parsed["title"].as_str().unwrap_or("MA Council Brief").to_string();
    let body_md = parsed["body_md"].as_str().unwrap_or("").to_string();
    let agendas = parsed["agendas"].clone();

    let conn = db.lock().unwrap();
    let _ = conn.execute(
        "INSERT OR IGNORE INTO ma_council_briefs
            (slug, week_start, title, body_md, agendas_json, model, published, created_at)
         VALUES (?,?,?,?,?,?,1,?)",
        params![slug, week, title, body_md, agendas.to_string(),
                "gemini-2.5-flash", chrono_now()],
    );
    Json(serde_json::json!({"ok": true, "slug": slug, "title": title, "agendas": agendas})).into_response()
}

#[derive(Deserialize)]
struct CouncilVoteBody {
    /// MUer token (must be MA owner)
    token: String,
    brief_slug: String,
    agenda_id: String,
    choice: String,
}

async fn council_vote(
    State(db): State<Db>,
    Json(body): Json<CouncilVoteBody>,
) -> impl IntoResponse {
    if body.choice.len() > 200 {
        return (StatusCode::BAD_REQUEST, "choice too long").into_response();
    }
    let conn = db.lock().unwrap();
    // Verify the voter is MA Council (= owns at least one MA piece)
    let voter_email: Option<String> = conn.query_row(
        "SELECT email FROM you_users WHERE token=? AND unsubscribed_at IS NULL",
        params![body.token], |r| r.get(0),
    ).ok();
    let Some(email) = voter_email else {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    };
    let is_ma_council: bool = conn.query_row(
        "SELECT 1 FROM mu_purchases WHERE LOWER(email)=? AND brand='ma' LIMIT 1",
        params![email.to_lowercase()], |_| Ok(true),
    ).unwrap_or(false);
    if !is_ma_council {
        return (StatusCode::FORBIDDEN, "MA Council メンバー限定の投票です").into_response();
    }
    // Verify brief exists + agenda_id valid (best-effort)
    let agendas_str: Option<String> = conn.query_row(
        "SELECT agendas_json FROM ma_council_briefs WHERE slug=? AND published=1",
        params![body.brief_slug], |r| r.get(0),
    ).ok();
    let Some(agendas_str) = agendas_str else {
        return (StatusCode::NOT_FOUND, "brief not found").into_response();
    };
    let agendas: serde_json::Value = serde_json::from_str(&agendas_str).unwrap_or(serde_json::json!([]));
    let valid_ids: Vec<String> = agendas.as_array().map(|arr| {
        arr.iter().filter_map(|a| a["id"].as_str().map(String::from)).collect()
    }).unwrap_or_default();
    if !valid_ids.contains(&body.agenda_id) {
        return (StatusCode::BAD_REQUEST, "agenda_id not in brief").into_response();
    }
    let _ = conn.execute(
        "INSERT INTO ma_council_votes (brief_slug, agenda_id, voter_email, choice, created_at)
         VALUES (?,?,?,?,?)
         ON CONFLICT(brief_slug, agenda_id, voter_email) DO UPDATE SET
            choice=excluded.choice, created_at=excluded.created_at",
        params![body.brief_slug, body.agenda_id, email.to_lowercase(), body.choice, chrono_now()],
    );
    Json(serde_json::json!({"ok": true})).into_response()
}

/// Public — return latest published brief + live vote tallies.
async fn list_council_briefs(State(db): State<Db>) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT slug, week_start, title, body_md, agendas_json, created_at
         FROM ma_council_briefs WHERE published=1 ORDER BY id DESC LIMIT 12"
    ) { Ok(s) => s, Err(_) => return Json(serde_json::json!({"briefs":[]})).into_response() };
    let rows: Vec<serde_json::Value> = stmt.query_map([], |r| {
        let slug: String = r.get(0)?;
        // Aggregate votes for this brief
        let agendas_str: String = r.get(4)?;
        Ok(serde_json::json!({
            "slug":       slug,
            "week_start": r.get::<_, String>(1)?,
            "title":      r.get::<_, String>(2)?,
            "body_md":    r.get::<_, String>(3)?,
            "agendas":    serde_json::from_str::<serde_json::Value>(&agendas_str).unwrap_or(serde_json::json!([])),
            "created_at": r.get::<_, String>(5)?,
        }))
    }).map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>()).unwrap_or_default();

    // attach tallies per brief
    let mut briefs_with_tally = Vec::new();
    for mut b in rows {
        let slug = b["slug"].as_str().unwrap_or("").to_string();
        let mut tally_stmt = match conn.prepare(
            "SELECT agenda_id, choice, COUNT(*) FROM ma_council_votes
             WHERE brief_slug=? GROUP BY agenda_id, choice"
        ) { Ok(s) => s, Err(_) => { briefs_with_tally.push(b); continue; } };
        let tallies: Vec<(String, String, i64)> = tally_stmt.query_map(params![slug], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        }).map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default();
        let mut tally_map = serde_json::Map::new();
        for (ag, ch, cnt) in tallies {
            let entry = tally_map.entry(ag).or_insert_with(|| serde_json::json!({}));
            entry[ch] = serde_json::json!(cnt);
        }
        b["tally"] = serde_json::Value::Object(tally_map);
        briefs_with_tally.push(b);
    }
    Json(serde_json::json!({"briefs": briefs_with_tally})).into_response()
}

// ─────────────────────────────────────────────────────────────────────────
// MA Council v2 — HMAC-token flow (the 2026.07 roadmap feature)
// ─────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CouncilTokenQuery { token: Option<String> }

/// GET /api/council/me?token=<member_token>
/// Returns the member's tier + join date + vote history.
async fn council_me(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<CouncilTokenQuery>,
) -> impl IntoResponse {
    let token = q.token.unwrap_or_default();
    if token.is_empty() {
        return (StatusCode::BAD_REQUEST, "missing token").into_response();
    }
    let conn = db.lock().unwrap();
    let member = match council_member_by_token(&conn, &token) {
        Some(m) => m,
        None => return (StatusCode::UNAUTHORIZED, "invalid token").into_response(),
    };
    let (mid, email, tier) = member;
    let joined_at: String = conn.query_row(
        "SELECT joined_at FROM ma_council_members WHERE id=?",
        params![mid], |r| r.get(0),
    ).unwrap_or_default();
    let mu_piece_id: Option<i64> = conn.query_row(
        "SELECT mu_piece_id FROM ma_council_members WHERE id=?",
        params![mid], |r| r.get(0),
    ).unwrap_or(None);
    let votes: Vec<serde_json::Value> = {
        let mut stmt = match conn.prepare(
            "SELECT brief_slug, agenda_id, option_index, choice, created_at
             FROM ma_council_votes
             WHERE voter_email=? ORDER BY id DESC LIMIT 50"
        ) { Ok(s) => s, Err(_) => return Json(serde_json::json!({
            "tier": tier, "joined_at": joined_at, "mu_piece_id": mu_piece_id,
            "email": mask_email(&email), "votes": []
        })).into_response() };
        stmt.query_map(params![email], |r| Ok(serde_json::json!({
            "brief_slug":   r.get::<_, String>(0)?,
            "agenda_id":    r.get::<_, String>(1)?,
            "option_index": r.get::<_, Option<i64>>(2)?,
            "choice":       r.get::<_, Option<String>>(3)?,
            "voted_at":     r.get::<_, String>(4)?,
        }))).map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    };
    Json(serde_json::json!({
        "tier": tier,
        "joined_at": joined_at,
        "mu_piece_id": mu_piece_id,
        "email": mask_email(&email),
        "votes": votes,
    })).into_response()
}

/// GET /api/council/agenda?token=<member_token>
/// Returns the latest published brief, its agenda options, and (if the
/// member has voted on any agenda already) which option they chose.
async fn council_agenda(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<CouncilTokenQuery>,
) -> impl IntoResponse {
    let token = q.token.unwrap_or_default();
    if token.is_empty() {
        return (StatusCode::BAD_REQUEST, "missing token").into_response();
    }
    let conn = db.lock().unwrap();
    let Some((mid, email, tier)) = council_member_by_token(&conn, &token) else {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    };
    let brief = conn.query_row(
        "SELECT id, slug, week_start, title, body_md, agendas_json, created_at
         FROM ma_council_briefs WHERE published=1 ORDER BY id DESC LIMIT 1",
        [], |r| Ok((
            r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?,
            r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
        )),
    );
    let Ok((brief_id, slug, week_start, title, body_md, agendas_str, created_at)) = brief else {
        return Json(serde_json::json!({
            "ok": true, "brief": null,
            "member": {"tier": tier, "id": mid, "email": mask_email(&email)},
        })).into_response();
    };
    let agendas: serde_json::Value =
        serde_json::from_str(&agendas_str).unwrap_or(serde_json::json!([]));
    let my_votes: Vec<(String, Option<i64>)> = {
        let mut stmt = match conn.prepare(
            "SELECT agenda_id, option_index FROM ma_council_votes
             WHERE brief_slug=? AND voter_email=?"
        ) { Ok(s) => s, Err(_) => return Json(serde_json::json!({
            "ok": true,
            "brief": {"id": brief_id, "slug": slug, "week_start": week_start,
                      "title": title, "body_md": body_md, "agendas": agendas,
                      "created_at": created_at},
            "member": {"tier": tier, "id": mid, "email": mask_email(&email)},
            "my_votes": {},
        })).into_response() };
        stmt.query_map(params![slug, email], |r| Ok((
            r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?
        ))).map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    };
    let mut votes_map = serde_json::Map::new();
    for (ag, opt) in my_votes {
        votes_map.insert(ag, serde_json::json!(opt));
    }
    Json(serde_json::json!({
        "ok": true,
        "brief": {
            "id": brief_id, "slug": slug, "week_start": week_start,
            "title": title, "body_md": body_md, "agendas": agendas,
            "created_at": created_at,
        },
        "member": {"tier": tier, "id": mid, "email": mask_email(&email)},
        "my_votes": serde_json::Value::Object(votes_map),
    })).into_response()
}

#[derive(Deserialize)]
struct CouncilTokenVoteBody {
    token: String,
    agenda_id: String,
    option_index: i64,
    /// Optional explicit brief_slug — defaults to the latest published brief.
    brief_slug: Option<String>,
}

/// POST /api/council/vote_token  body {token, agenda_id, option_index}
/// Records a vote, idempotent on (brief, member, agenda) via the UNIQUE
/// constraint on (brief_slug, agenda_id, voter_email).
async fn council_vote_token(
    State(db): State<Db>,
    Json(body): Json<CouncilTokenVoteBody>,
) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let Some((_mid, email, _tier)) = council_member_by_token(&conn, &body.token) else {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    };
    // Default to latest published brief if no slug provided
    let slug = match body.brief_slug.clone() {
        Some(s) if !s.is_empty() => s,
        _ => match conn.query_row(
            "SELECT slug FROM ma_council_briefs WHERE published=1 ORDER BY id DESC LIMIT 1",
            [], |r| r.get::<_, String>(0),
        ) {
            Ok(s) => s,
            Err(_) => return (StatusCode::NOT_FOUND, "no published brief").into_response(),
        },
    };
    let agendas_str: Option<String> = conn.query_row(
        "SELECT agendas_json FROM ma_council_briefs WHERE slug=? AND published=1",
        params![slug], |r| r.get(0),
    ).ok();
    let Some(agendas_str) = agendas_str else {
        return (StatusCode::NOT_FOUND, "brief not found").into_response();
    };
    let agendas: serde_json::Value = serde_json::from_str(&agendas_str).unwrap_or(serde_json::json!([]));
    let agenda_obj = agendas.as_array().and_then(|arr|
        arr.iter().find(|a| a["id"].as_str() == Some(&body.agenda_id))
    );
    let Some(agenda_obj) = agenda_obj else {
        return (StatusCode::BAD_REQUEST, "agenda_id not in brief").into_response();
    };
    let n_options = agenda_obj["options"].as_array().map(|a| a.len() as i64).unwrap_or(0);
    if body.option_index < 0 || body.option_index >= n_options {
        return (StatusCode::BAD_REQUEST,
            format!("option_index out of range (0..{})", n_options)).into_response();
    }
    let choice_text = agenda_obj["options"][body.option_index as usize]
        .as_str().unwrap_or("").to_string();
    let _ = conn.execute(
        "INSERT INTO ma_council_votes
            (brief_slug, agenda_id, voter_email, choice, option_index, created_at)
         VALUES (?,?,?,?,?,?)
         ON CONFLICT(brief_slug, agenda_id, voter_email) DO UPDATE SET
            choice=excluded.choice, option_index=excluded.option_index,
            created_at=excluded.created_at",
        params![slug, body.agenda_id, email, choice_text,
                body.option_index, chrono_now()],
    );
    Json(serde_json::json!({"ok": true, "brief_slug": slug,
        "agenda_id": body.agenda_id, "option_index": body.option_index})).into_response()
}

/// GET /api/council/results/:brief_id
/// Public — returns the anonymous tally per agenda for a brief. brief_id
/// may be either the numeric id or the slug. Always returns a 200 with an
/// `agendas` array (empty if the brief doesn't exist) so the UI can render
/// a "no votes yet" state uniformly.
async fn council_results(
    State(db): State<Db>,
    Path(brief_id): Path<String>,
) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let brief = if let Ok(id) = brief_id.parse::<i64>() {
        conn.query_row(
            "SELECT slug, title, week_start, agendas_json FROM ma_council_briefs WHERE id=?",
            params![id], |r| Ok((
                r.get::<_, String>(0)?, r.get::<_, String>(1)?,
                r.get::<_, String>(2)?, r.get::<_, String>(3)?,
            )),
        ).ok()
    } else {
        conn.query_row(
            "SELECT slug, title, week_start, agendas_json FROM ma_council_briefs WHERE slug=?",
            params![brief_id], |r| Ok((
                r.get::<_, String>(0)?, r.get::<_, String>(1)?,
                r.get::<_, String>(2)?, r.get::<_, String>(3)?,
            )),
        ).ok()
    };
    let Some((slug, title, week_start, agendas_str)) = brief else {
        // Match the user spec: empty tally for non-existent briefs (200, not 404)
        return Json(serde_json::json!({
            "ok": true, "brief": null, "agendas": [],
        })).into_response();
    };
    let agendas: serde_json::Value =
        serde_json::from_str(&agendas_str).unwrap_or(serde_json::json!([]));

    // Build per-agenda tally keyed by option_index (preferred) with fallback
    // to choice-text aggregation for legacy rows missing option_index.
    let tallies: Vec<(String, Option<i64>, String, i64)> = {
        let mut stmt = match conn.prepare(
            "SELECT agenda_id, option_index, choice, COUNT(*)
             FROM ma_council_votes
             WHERE brief_slug=?
             GROUP BY agenda_id, option_index, choice"
        ) { Ok(s) => s, Err(_) => return Json(serde_json::json!({
            "ok": true, "brief": null, "agendas": agendas,
        })).into_response() };
        stmt.query_map(params![slug], |r| Ok((
            r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?,
            r.get::<_, String>(2)?, r.get::<_, i64>(3)?,
        ))).map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    };
    let mut agenda_results = Vec::new();
    if let Some(arr) = agendas.as_array() {
        for ag in arr {
            let aid = ag["id"].as_str().unwrap_or("").to_string();
            let opts = ag["options"].as_array().cloned().unwrap_or_default();
            let mut counts: Vec<i64> = vec![0; opts.len()];
            let mut total: i64 = 0;
            for (tid, opt_idx, choice, n) in &tallies {
                if tid != &aid { continue; }
                total += n;
                if let Some(idx) = opt_idx {
                    if *idx >= 0 && (*idx as usize) < counts.len() {
                        counts[*idx as usize] += n;
                        continue;
                    }
                }
                // legacy free-text fallback: match by string equality
                if let Some(pos) = opts.iter().position(
                    |o| o.as_str() == Some(choice.as_str())
                ) { counts[pos] += n; }
            }
            agenda_results.push(serde_json::json!({
                "id":      aid,
                "q":       ag["q"].clone(),
                "options": opts,
                "counts":  counts,
                "total":   total,
            }));
        }
    }
    Json(serde_json::json!({
        "ok": true,
        "brief": {"slug": slug, "title": title, "week_start": week_start},
        "agendas": agenda_results,
    })).into_response()
}

/// Lightweight email masker for displaying member identity in council UI
/// without leaking the full address. "alice@example.com" → "a***e@example.com".
fn mask_email(s: &str) -> String {
    let (local, domain) = match s.split_once('@') {
        Some(p) => p, None => return "***".into(),
    };
    let masked_local = match local.len() {
        0 => "".to_string(),
        1 => local.to_string(),
        2 => format!("{}*", &local[..1]),
        n => format!("{}***{}", &local[..1], &local[n-1..]),
    };
    format!("{}@{}", masked_local, domain)
}

async fn council_page() -> Html<&'static str> {
    Html(include_str!("../static/council.html"))
}

/// Weekly Council Brief generation cron. Runs every Sunday 18:00 JST.
/// Idempotent: skips if a brief for the current ISO week already exists.
/// Falls back to a deterministic template if Gemini is unavailable.
async fn run_council_weekly_cron(db: Db) {
    let week_label = iso_week_label_jst();
    let week_start = iso_week_start_jst();
    let slug = format!("council-{}", week_start);
    tracing::info!("[cron] council-weekly: starting week={} slug={}", week_label, slug);

    // Idempotency check
    {
        let conn = db.lock().unwrap();
        let exists: bool = conn.query_row(
            "SELECT 1 FROM ma_council_briefs WHERE slug=?",
            params![slug], |r| r.get::<_, i64>(0),
        ).is_ok();
        if exists {
            tracing::info!("[cron] council-weekly: brief {} already exists — skipping", slug);
            return;
        }
    }

    // Pull recent feedback for Gemini context
    let inputs: Vec<(String, String, i64)> = {
        let conn = db.lock().unwrap();
        let cutoff: i64 = chrono_now().parse::<i64>().unwrap_or(0) - 30 * 86_400;
        let mut stmt = match conn.prepare(
            "SELECT message, COALESCE(email,'anon'), is_ma_council
             FROM customer_feedback
             WHERE CAST(created_at AS INTEGER) >= ?
             ORDER BY is_ma_council DESC, id DESC LIMIT 40"
        ) {
            Ok(s) => s, Err(_) => { tracing::error!("[cron] council: feedback prepare failed"); return; }
        };
        stmt.query_map(params![cutoff], |r| Ok((
            r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?
        ))).map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>()).unwrap_or_default()
    };

    let (_gemini_title, body_md, agendas, model_used) = match generate_council_brief_via_gemini(
        &week_label, &inputs).await {
        Some(triple) => (triple.0, triple.1, triple.2, "gemini-2.5-flash".to_string()),
        None => {
            tracing::warn!("[cron] council: Gemini unavailable — using static fallback");
            let (t, b, a) = static_council_brief_fallback(&week_label);
            (t, b, a, "static-fallback".to_string())
        }
    };
    // Always build the title server-side. Gemini hallucinated "2024 週11" on
    // 2026-05-11 (week_label was correct "2026.W19" but Gemini wrote the year
    // from its training data into the title).
    let title = format!("今週の MA Council Brief — {}", week_label);


    // Insert the brief row
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "INSERT OR IGNORE INTO ma_council_briefs
                (slug, week_start, title, body_md, agendas_json, model, published, created_at)
             VALUES (?,?,?,?,?,?,1,?)",
            params![slug, week_start, title, body_md, agendas.to_string(),
                    model_used, chrono_now()],
        );
    }

    // Send emails to all active members via Resend
    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    if resend_key.is_empty() {
        tracing::warn!("[cron] council: RESEND_API_KEY missing — skipping email phase");
        return;
    }
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    let recipients: Vec<(String, String)> = {
        let conn = db.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT email, tier FROM ma_council_members
             WHERE unsubscribed_at IS NULL ORDER BY id ASC"
        ) {
            Ok(s) => s, Err(_) => { tracing::error!("[cron] council: members prepare failed"); return; }
        };
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build().unwrap_or_default();

    let mut sent = 0;
    let mut failed = 0;
    for (email, tier) in &recipients {
        let token = match council_token_for(email) {
            Some(t) => t,
            None => {
                tracing::warn!("[cron] council: COUNCIL_TOKEN_SECRET missing — aborting email phase");
                return;
            }
        };
        let html = council_brief_email_html(
            &week_label, &title, &body_md, &agendas, &base_url, &token, tier);
        let resp = client.post("https://api.resend.com/emails")
            .bearer_auth(&resend_key)
            .json(&serde_json::json!({
                "from": "MU Council <noreply@wearmu.com>",
                "to": [email],
                "subject": format!("🎫 MA Council Brief — {}", week_label),
                "html": html,
            }))
            .send().await;
        match resp {
            Ok(r) if r.status().is_success() => { sent += 1; }
            Ok(r) => {
                let s = r.status();
                let t = r.text().await.unwrap_or_default();
                tracing::warn!("[cron] council FAIL → {}: {} {}", email, s,
                    &t[..t.len().min(200)]);
                failed += 1;
            }
            Err(e) => {
                tracing::warn!("[cron] council NET FAIL → {}: {}", email, e);
                failed += 1;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
    // Mark sent_at on the brief
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "UPDATE ma_council_briefs SET sent_at=? WHERE slug=?",
            params![chrono_now(), slug],
        );
    }
    tracing::info!("[cron] council-weekly: done week={} sent={} failed={} recipients={}",
        week_label, sent, failed, recipients.len());
}

/// "YYYY.WNN" ISO-week label (e.g. "2026.W19"). Computed from the JST
/// Monday-start week. Note: this is a near-ISO label — for the literal
/// ISO 8601 week numbering edge cases we'd need a real chrono dep, but
/// the simple variant is adequate for human-readable labels.
fn iso_week_label_jst() -> String {
    let now_s: i64 = chrono_now().parse().unwrap_or(0);
    let jst = now_s + 9 * 3600;
    let day = jst / 86_400;
    let dow_mon = (day + 3).rem_euclid(7);
    let monday_days = day - dow_mon;
    let (y, _m, _d) = civil_from_days(monday_days);
    // Approximate ISO week number: days since Jan 1 of year / 7 + 1
    let (y0_days, _) = (days_from_civil(y, 1, 1), 0);
    let week_num = ((monday_days - y0_days) / 7) + 1;
    format!("{:04}.W{:02}", y, week_num)
}

/// Inverse of civil_from_days. Days since 1970-01-01.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m_idx = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * m_idx + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Static fallback when Gemini is unavailable. Generates a 2-agenda brief
/// covering the 2026.08 MUGEN price-range vote (the first scheduled
/// council vote per the roadmap) + a new-category direction question.
fn static_council_brief_fallback(week_label: &str)
    -> (String, String, serde_json::Value)
{
    let title = format!("MA Council Brief — {}", week_label);
    let body_md = "## 1. 今週のテーマ\n\
        Council 創設週 — 最初の議題は 2026.08 の MUGEN 価格レンジです。\n\n\
        ## 2. お客様の声 (要約)\n\
        - 「もう少し高くてもいい、希少性が欲しい」\n\
        - 「¥4,000 だと安すぎて逆に怪しく見える」\n\
        - 「新カテゴリは sweat が欲しい」\n\n\
        ## 3. 議題\n\
        2 件の議題に投票してください。投票は 1 council = 1 vote。集計は public。\n".to_string();
    let agendas = serde_json::json!([
        {
            "id": "a1",
            "q": "2026.08 の MUGEN 価格レンジを変更すべきか？",
            "options": [
                "¥4,000–6,000 (現行維持)",
                "¥5,000–8,000",
                "¥6,000–10,000"
            ]
        },
        {
            "id": "a2",
            "q": "新カテゴリを 2026.Q3 に投入するか？",
            "options": [
                "sweat 優先",
                "longsleeve 優先",
                "T シャツ集中 (見送り)"
            ]
        }
    ]);
    (title, body_md, agendas)
}

/// Calls Gemini to generate (title, body_md, agendas_json). Returns None
/// on any error so the caller can fall back to the static template.
async fn generate_council_brief_via_gemini(
    week_label: &str, inputs: &[(String, String, i64)],
) -> Option<(String, String, serde_json::Value)> {
    let key = env::var("GEMINI_API_KEY").ok().filter(|k| !k.is_empty())?;
    let context: String = inputs.iter().enumerate().map(|(i, (m, e, council))| {
        let tag = if *council == 1 { "[Council] " } else { "" };
        format!("{}. {}{}: {}", i + 1, tag, e, m.chars().take(280).collect::<String>())
    }).collect::<Vec<_>>().join("\n");
    let prompt = format!("あなたは MU ブランドの議題集計 AI です。週次 MA Council Brief を以下のフォーマットで生成してください。\n\n週ラベル: {week_label}\n\n過去 30 日のお客様フィードバック (上位 40 件、Council 優先):\n{context}\n\n出力フォーマット (JSON のみ、コードフェンス不要):\n{{\n  \"title\": \"MA Council Brief — {week_label} (タイトルは 28 字以内)\",\n  \"body_md\": \"## 1. 今週のテーマ\\n## 2. お客様の声 (要約)\\n## 3. 議題\",\n  \"agendas\": [\n    {{\"id\": \"a1\", \"q\": \"次月の MUGEN 価格レンジを変更すべきか？\", \"options\": [\"¥4,000–6,000 (現行)\", \"¥5,000–8,000\", \"¥6,000–10,000\"]}},\n    {{\"id\": \"a2\", \"q\": \"新カテゴリ (sweat / longsleeve) を投入するか？\", \"options\": [\"sweat 優先\", \"longsleeve 優先\", \"T シャツ集中\"]}}\n  ]\n}}\n\nルール:\n- 議題は 2〜4 件\n- 各議題の options は 2〜4 個\n- お客様の生の声を 3 件以上 body_md に引用 (短く)\n- 捏造禁止 — フィードバックに無い議題は出さない\n- 末尾に「— 集計: Gemini 2.5 / 投票: MA Council メンバー」");
    let req_body = serde_json::json!({"contents": [{"parts": [{"text": prompt}]}]});
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        key);
    let resp = reqwest::Client::new()
        .post(&url).json(&req_body)
        .timeout(std::time::Duration::from_secs(45))
        .send().await.ok()?;
    if !resp.status().is_success() { return None; }
    let j: serde_json::Value = resp.json().await.ok()?;
    let text = j["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()?.trim()
        .trim_start_matches("```json").trim_start_matches("```")
        .trim_end_matches("```").trim().to_string();
    let parsed: serde_json::Value = serde_json::from_str(&text).ok()?;
    let title = parsed["title"].as_str()?.to_string();
    let body_md = parsed["body_md"].as_str().unwrap_or("").to_string();
    let agendas = parsed["agendas"].clone();
    if !agendas.is_array() || agendas.as_array().map(|a| a.is_empty()).unwrap_or(true) {
        return None;
    }
    Some((title, body_md, agendas))
}

/// Renders the weekly Council Brief email body. Uses the same dark-glass
/// aesthetic as the auction-winner mail.
fn council_brief_email_html(
    week_label: &str, title: &str, body_md: &str, agendas: &serde_json::Value,
    base_url: &str, token: &str, tier: &str,
) -> String {
    // Very small markdown→html for headers + bullets only. Enough for our
    // structured `## 1. ...\n- ...` template.
    let mut body_html = String::new();
    for line in body_md.lines() {
        let t = line.trim_end();
        if let Some(rest) = t.strip_prefix("## ") {
            body_html.push_str(&format!(
                "<h3 style=\"font-size:13px;letter-spacing:0.2em;text-transform:uppercase;color:#e6c449;margin:24px 0 8px;font-weight:500\">{}</h3>",
                html_escape(rest)));
        } else if let Some(rest) = t.strip_prefix("- ") {
            body_html.push_str(&format!(
                "<p style=\"font-size:12px;line-height:1.85;opacity:0.75;margin:4px 0 4px 18px\">• {}</p>",
                html_escape(rest)));
        } else if !t.is_empty() {
            body_html.push_str(&format!(
                "<p style=\"font-size:12px;line-height:1.85;opacity:0.7;margin:8px 0\">{}</p>",
                html_escape(t)));
        }
    }
    let mut agenda_html = String::new();
    if let Some(arr) = agendas.as_array() {
        for (i, a) in arr.iter().enumerate() {
            let aid = a["id"].as_str().unwrap_or("");
            let q   = a["q"].as_str().unwrap_or("");
            let mut opts_html = String::new();
            if let Some(opts) = a["options"].as_array() {
                for (idx, o) in opts.iter().enumerate() {
                    let label = o.as_str().unwrap_or("");
                    opts_html.push_str(&format!(
                        "<a href=\"{base}/council?token={tok}&vote={aid}:{idx}\" style=\"display:block;background:#1C1C1C;color:#F5F5F0;padding:14px 18px;margin-bottom:6px;font-size:12px;text-decoration:none;border-left:2px solid rgba(230,196,73,0.35)\">{label}</a>",
                        base = base_url, tok = token, aid = aid, idx = idx,
                        label = html_escape(label)));
                }
            }
            agenda_html.push_str(&format!(
                "<div style=\"margin:24px 0;padding:20px;background:#0F0F0F;border:1px solid rgba(255,255,255,0.06)\"><div style=\"font-size:9px;letter-spacing:0.3em;text-transform:uppercase;color:#e6c449;opacity:0.85;margin-bottom:8px\">議題 {n}</div><div style=\"font-size:14px;font-weight:400;margin-bottom:14px;line-height:1.5\">{q}</div>{opts}</div>",
                n = i + 1, q = html_escape(q), opts = opts_html));
        }
    }
    let tier_label = if tier == "full" { "FULL MEMBER" } else { "TRIAL" };
    format!(r#"<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:48px 24px;max-width:600px;margin:0 auto">
  <div style="font-size:22px;font-weight:700;letter-spacing:0.4em;margin-bottom:32px">MU · COUNCIL</div>
  <div style="font-size:10px;letter-spacing:0.4em;text-transform:uppercase;color:#e6c449;opacity:0.85;margin-bottom:6px">{week} · {tier}</div>
  <h1 style="font-family:'Helvetica Neue',Arial,sans-serif;font-size:22px;font-weight:300;letter-spacing:0.02em;margin:0 0 22px;color:#F5F5F0">{title}</h1>
  {body}
  <h3 style="font-size:13px;letter-spacing:0.2em;text-transform:uppercase;color:#e6c449;margin:32px 0 10px;font-weight:500">議題に投票</h3>
  {agendas}
  <a href="{base}/council?token={tok}" style="display:inline-block;background:#e6c449;color:#000;padding:14px 28px;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;text-decoration:none;font-weight:700;margin-top:8px">Council を開く →</a>
  <div style="margin-top:48px;border-top:1px solid #1C1C1C;padding-top:20px;font-size:9px;opacity:0.5;letter-spacing:0.1em;line-height:1.7">
    MU — wearmu.com<br>
    あなたは {tier} メンバーです。集計は <a style="color:#e6c449" href="{base}/council">/council</a> で誰でも閲覧可能 (匿名)。
  </div>
</div>"#,
        week = week_label, tier = tier_label, title = html_escape(title),
        body = body_html, agendas = agenda_html, base = base_url, tok = token,
    )
}

/// Weekly lottery draw — picks ~5% of pending entries as winners,
/// mints a Stripe coupon ¥1,000-3,000 off, emails them.
/// Idempotent on entry id (status changes from 'pending*' to 'won' / 'lost').
async fn admin_lottery_draw(
    State(db): State<Db>,
    Json(body): Json<YouAdminBackfillBody>,
) -> impl IntoResponse {
    if let Err(r) = require_admin_token(Some(&body.admin_token)) { return r; }

    type Entry = (i64, String, String);
    let pending: Vec<Entry> = {
        let conn = db.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, email, ticket_id FROM exit_offers
             WHERE kind='lottery_entry' AND status LIKE 'pending%'"
        ) { Ok(s) => s, Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db").into_response() };
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    };
    if pending.is_empty() {
        return Json(serde_json::json!({"ok": true, "drawn": 0, "winners": 0, "msg": "no pending entries"})).into_response();
    }

    use sha2::{Digest, Sha256};
    let week_seed = chrono_now().parse::<u64>().unwrap_or(0) / (7 * 86400);
    let mut winners: Vec<Entry> = Vec::new();
    let mut losers: Vec<i64> = Vec::new();
    for entry in &pending {
        let mut h = Sha256::new();
        h.update(format!("{}|{}", week_seed, entry.0).as_bytes());
        let d = h.finalize();
        let r = (d[0] as u32) % 100;
        if r < 5 { winners.push(entry.clone()); } else { losers.push(entry.0); }
    }

    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    let mut won_count = 0usize;

    for (id, email, ticket) in &winners {
        // Prize tier: 60% ¥1000, 30% ¥2000, 10% ¥3000
        let mut h = Sha256::new();
        h.update(format!("prize|{}", id).as_bytes());
        let d = h.finalize();
        let rr = (d[0] as u32) % 100;
        let prize_jpy: i64 = if rr < 60 { 1000 } else if rr < 90 { 2000 } else { 3000 };

        let token = uuid::Uuid::new_v4().to_string().replace('-', "");
        let code = format!("MU-WIN-{}", token[..8].to_uppercase());
        let mint = if !stripe_key.is_empty() {
            let amount_off_str = prize_jpy.to_string();
            let resp = reqwest::Client::new()
                .post("https://api.stripe.com/v1/coupons")
                .basic_auth(&stripe_key, None::<&str>)
                .form(&[
                    ("id", code.as_str()),
                    ("amount_off", amount_off_str.as_str()),
                    ("currency", "jpy"),
                    ("duration", "once"),
                    ("max_redemptions", "1"),
                    ("name", &format!("MU 抽選 ¥{} OFF", prize_jpy)),
                    ("redeem_by", &format!("{}", chrono_now().parse::<i64>().unwrap_or(0) + 60 * 86400)),
                ])
                .send().await;
            match resp {
                Ok(r) if r.status().is_success() => true,
                Ok(r) => { eprintln!("[lottery] coupon mint {}: {}", r.status(), r.text().await.unwrap_or_default()); false }
                Err(e) => { eprintln!("[lottery] coupon mint err: {}", e); false }
            }
        } else { false };

        {
            let conn = db.lock().unwrap();
            conn.execute(
                "UPDATE exit_offers SET status='won', prize_jpy=?, stripe_coupon=?, used_at=NULL WHERE id=?",
                params![prize_jpy, if mint { Some(code.as_str()) } else { None }, id],
            ).ok();
        }

        if mint && !resend_key.is_empty() {
            let to = email.clone();
            let code2 = code.clone();
            let prize_label = format!("¥{}", prize_jpy);
            let ticket2 = ticket.clone();
            let resend_key2 = resend_key.clone();
            tokio::spawn(async move {
                let html = format!(r#"
<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:48px;max-width:560px;margin:0 auto">
  <div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:32px">MU</div>
  <div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#e6c449;opacity:0.85;margin-bottom:8px">当選 — Lottery</div>
  <div style="font-size:20px;font-weight:300;line-height:1.4;margin-bottom:18px">おめでとうございます。{prize} OFF クーポンが当選しました。</div>
  <div style="background:#1C1C1C;padding:18px;text-align:center;font-family:monospace;font-size:18px;letter-spacing:0.18em;color:#e6c449;margin:16px 0">{code}</div>
  <p style="font-size:12px;line-height:1.85;opacity:0.75;margin-bottom:18px">
    Stripe チェックアウトの「プロモーションコード」欄に貼ってください。<br>
    1 回限り · 60 日有効 · MUGEN / MUON / MA / /you 共通。<br>
    抽選チケット: <code style="font-family:monospace;color:#e6c449">{ticket}</code>
  </p>
  <a href="https://wearmu.com/mugen?coupon={code}" style="display:inline-block;background:#e6c449;color:#000;padding:14px 28px;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;text-decoration:none;font-weight:700">MUGEN を見る →</a>
</div>"#, prize = prize_label, code = code2, ticket = ticket2.chars().take(8).collect::<String>());
                let _ = reqwest::Client::new()
                    .post("https://api.resend.com/emails")
                    .bearer_auth(&resend_key2)
                    .json(&serde_json::json!({
                        "from": "MU <noreply@wearmu.com>",
                        "to": [to],
                        "subject": format!("MU 抽選 当選: {} OFF クーポン", prize_label),
                        "html": html,
                    }))
                    .send().await;
            });
            won_count += 1;
        }
    }

    {
        let conn = db.lock().unwrap();
        for id in &losers {
            conn.execute("UPDATE exit_offers SET status='lost' WHERE id=?", params![id]).ok();
        }
    }

    Json(serde_json::json!({
        "ok": true, "drawn": pending.len(),
        "winners": won_count, "losers": losers.len(),
    })).into_response()
}

// ── Exit-intent funnel ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ExitSurveyBody {
    #[serde(default)] email: String,
    #[serde(default)] page: String,
    #[serde(default)] why_left: String,
    #[serde(default)] price_feel: String,
    #[serde(default)] would_buy_at: i64,
    #[serde(default)] comment: String,
}

/// Step 1: record the survey response. Always 200 OK so the modal flow
/// continues even if email is empty (anonymous insight is still useful).
async fn exit_survey(
    State(db): State<Db>,
    Json(body): Json<ExitSurveyBody>,
) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let _ = conn.execute(
        "INSERT INTO exit_surveys (email, page, why_left, price_feel, would_buy_at, comment, created_at)
         VALUES (?,?,?,?,?,?,?)",
        params![
            body.email.trim().to_lowercase(),
            body.page.chars().take(120).collect::<String>(),
            body.why_left.chars().take(80).collect::<String>(),
            body.price_feel.chars().take(80).collect::<String>(),
            body.would_buy_at,
            body.comment.chars().take(500).collect::<String>(),
            chrono_now(),
        ],
    );
    Json(serde_json::json!({"ok": true})).into_response()
}

#[derive(Deserialize)]
struct ExitDiscountBody {
    email: String,
}

/// Step 2: mint a Stripe one-time-use 50%-off coupon (≒ "原価レベル") for the
/// email and return the code. Idempotent within 24h: returns the same code if
/// the same email has already claimed today.
async fn exit_discount_claim(
    State(db): State<Db>,
    Json(body): Json<ExitDiscountBody>,
) -> impl IntoResponse {
    let email = body.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return (StatusCode::BAD_REQUEST, "invalid email").into_response();
    }

    // Reuse an existing un-used discount if one already exists today.
    let existing: Option<String> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT stripe_coupon FROM exit_offers
             WHERE email=? AND kind='discount_50' AND used_at IS NULL
               AND CAST(created_at AS INTEGER) > CAST(? AS INTEGER) - 86400
             ORDER BY id DESC LIMIT 1",
            params![email, chrono_now()],
            |r| r.get(0),
        ).ok().flatten()
    };
    if let Some(code) = existing {
        let pct: i64 = {
            let conn = db.lock().unwrap();
            cv_get(&conn, "coupon_percent_off", "50").parse().unwrap_or(50)
        };
        return Json(serde_json::json!({
            "ok": true, "coupon": code, "percent_off": pct,
            "valid_for": "MUGEN / MUON / MA / /you all", "reused": true,
        })).into_response();
    }

    // Mint a new Stripe coupon with a memorable code.
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    if stripe_key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "checkout disabled").into_response();
    }
    let token = uuid::Uuid::new_v4().to_string().replace('-', "");
    let code = format!("MU-COST-{}", token[..8].to_uppercase());
    // cv_pulse may have tuned the strength; default 50 (≒ "原価レベル").
    let pct = {
        let conn = db.lock().unwrap();
        cv_get(&conn, "coupon_percent_off", "50")
    };
    let pct_clamped = pct.parse::<i64>().unwrap_or(50).clamp(20, 80).to_string();
    let resp = reqwest::Client::new()
        .post("https://api.stripe.com/v1/coupons")
        .basic_auth(&stripe_key, None::<&str>)
        .form(&[
            ("id", code.as_str()),
            ("percent_off", pct_clamped.as_str()),
            ("duration", "once"),
            ("max_redemptions", "1"),
            ("name", &format!("MU 原価レベル ({}% OFF)", pct_clamped)),
        ])
        .send().await;
    let coupon_id = match resp {
        Ok(r) if r.status().is_success() => {
            let j: serde_json::Value = r.json().await.unwrap_or_default();
            j["id"].as_str().unwrap_or(&code).to_string()
        }
        Ok(r) => {
            let s = r.status();
            let t = r.text().await.unwrap_or_default();
            eprintln!("[exit] stripe coupon create {}: {}", s, t);
            return (StatusCode::BAD_GATEWAY, "could not mint coupon").into_response();
        }
        Err(e) => {
            eprintln!("[exit] stripe coupon network: {}", e);
            return (StatusCode::BAD_GATEWAY, "stripe network error").into_response();
        }
    };
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO exit_offers (email, kind, stripe_coupon, status, expires_at, created_at)
             VALUES (?, 'discount_50', ?, 'issued', ?, ?)",
            params![
                email, coupon_id,
                format!("{}", chrono_now().parse::<i64>().unwrap_or(0) + 30 * 86400),
                chrono_now(),
            ],
        ).ok();
    }

    // Email the code so it doesn't depend on the user keeping the tab open.
    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    if !resend_key.is_empty() {
        let to = email.clone();
        let code_for_mail = coupon_id.clone();
        tokio::spawn(async move {
            let html = format!(r#"
<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:48px;max-width:560px;margin:0 auto">
  <div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:32px">MU</div>
  <div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#e6c449;opacity:0.85;margin-bottom:8px">原価レベル クーポン</div>
  <div style="font-size:18px;font-weight:300;line-height:1.5;margin-bottom:24px">アンケートにお答えいただきありがとうございます。</div>
  <p style="font-size:12px;line-height:1.85;opacity:0.78;margin-bottom:18px">
    お試しいただきたいので、ほぼ製造原価で 1 着お渡しします。<br>
    Stripe チェックアウトで以下のクーポンコードを入力してください。<br>
    <strong>有効期限 30 日 · 1 回限り · 全カテゴリ対応</strong>
  </p>
  <div style="background:#1C1C1C;padding:18px;text-align:center;font-family:monospace;font-size:18px;letter-spacing:0.2em;color:#e6c449;margin:16px 0">
    {code}
  </div>
  <a href="https://wearmu.com/mugen" style="display:inline-block;background:#e6c449;color:#000;padding:14px 28px;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;text-decoration:none;font-weight:700">MUGEN を見る →</a>
  <p style="font-size:10px;opacity:0.5;margin-top:32px">MU が「合うかどうか」を体感してほしい。気に入ったら 2 着目から通常価格でどうぞ。</p>
</div>"#, code = code_for_mail);
            let _ = reqwest::Client::new()
                .post("https://api.resend.com/emails")
                .bearer_auth(&resend_key)
                .json(&serde_json::json!({
                    "from": "MU <noreply@wearmu.com>",
                    "to": [to],
                    "subject": format!("MU — 原価レベル クーポン ({}, 30 日有効)", code_for_mail),
                    "html": html,
                })).send().await;
        });
    }
    Json(serde_json::json!({
        "ok": true, "coupon": coupon_id,
        "percent_off": pct_clamped.parse::<i64>().unwrap_or(50),
        "valid_days": 30, "reused": false,
    })).into_response()
}

#[derive(Deserialize)]
struct ExitLotteryBody {
    email: String,
    #[serde(default)] referrer: String,
}

/// Step 3: open lottery entry (オープン懸賞 — purchase NOT required, no
/// statutory prize cap in Japan). Weekly draw selects winners for
/// ¥1,000–¥3,000 cashback coupons.
async fn exit_lottery_enter(
    State(db): State<Db>,
    Json(body): Json<ExitLotteryBody>,
) -> impl IntoResponse {
    let email = body.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return (StatusCode::BAD_REQUEST, "invalid email").into_response();
    }
    // 1 ticket per email per week.
    let existing: Option<String> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT ticket_id FROM exit_offers
             WHERE email=? AND kind='lottery_entry'
               AND CAST(created_at AS INTEGER) > CAST(? AS INTEGER) - 7 * 86400
             ORDER BY id DESC LIMIT 1",
            params![email, chrono_now()],
            |r| r.get(0),
        ).ok().flatten()
    };
    if let Some(t) = existing {
        return Json(serde_json::json!({
            "ok": true, "ticket": t, "prize_range_jpy": [1000, 3000],
            "draw_at": "weekly Monday 9:00 JST", "reused": true,
        })).into_response();
    }
    let ticket = uuid::Uuid::new_v4().to_string();
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO exit_offers (email, kind, ticket_id, status, created_at)
         VALUES (?, 'lottery_entry', ?, 'pending', ?)",
        params![email, ticket, chrono_now()],
    ).ok();
    if !body.referrer.is_empty() {
        // Optional: log the referrer slug if any
        conn.execute(
            "UPDATE exit_offers SET status=? WHERE ticket_id=?",
            params![format!("pending:ref={}", body.referrer.chars().take(40).collect::<String>()), ticket],
        ).ok();
    }
    Json(serde_json::json!({
        "ok": true, "ticket": ticket,
        "prize_range_jpy": [1000, 3000],
        "draw_at": "weekly Monday 9:00 JST",
        "reused": false,
    })).into_response()
}

/// Public purchase: anyone (not just the design's owner) can buy a /you
/// design they see on the share page. Buyer enters their email + shipping
/// address inside Stripe Checkout. Default price is ¥6,800; the design
/// owner does NOT have to pre-list.
async fn you_public_buy(
    State(db): State<Db>,
    Path(design_id): Path<i64>,
) -> impl IntoResponse {
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    if stripe_key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "checkout disabled").into_response();
    }
    // Look up the design + its owner's slug for branding.
    let row: Option<(i64, i64, String, String, Option<String>, String)> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT d.id, d.day_num, d.name, d.gen_status, u.slug, u.size
             FROM you_designs d JOIN you_users u ON u.id = d.user_id
             WHERE d.id=?",
            params![design_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        ).ok()
    };
    let (id, day_num, name, gen_status, slug_opt, default_size) = match row {
        Some(v) => v,
        None => return (StatusCode::NOT_FOUND, "design not found").into_response(),
    };
    if gen_status != "ready" {
        return (StatusCode::CONFLICT, "design not ready yet").into_response();
    }
    let serial = format!("YOU#{:04}", id);
    let owner_tag = slug_opt.as_deref().unwrap_or("anon");
    let display = format!("MU × YOU @{} — {} ({})", owner_tag, name, serial);
    let price_jpy: i64 = 6_800;
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    let cancel = match slug_opt.as_ref() {
        Some(s) if !s.is_empty() => format!("{}/{}", base_url.trim_end_matches('/'), s),
        _ => format!("{}/you", base_url.trim_end_matches('/')),
    };

    let resp = reqwest::Client::new()
        .post("https://api.stripe.com/v1/checkout/sessions")
        .basic_auth(&stripe_key, None::<&str>)
        .form(&[
            ("mode", "payment"),
            ("currency", "jpy"),
            ("line_items[0][price_data][currency]", "jpy"),
            ("line_items[0][price_data][product_data][name]", &display),
            ("line_items[0][price_data][unit_amount]", &price_jpy.to_string()),
            ("line_items[0][quantity]", "1"),
            ("success_url", &format!("{}/success?sid={{CHECKOUT_SESSION_ID}}", base_url)),
            ("cancel_url", &cancel),
            ("shipping_address_collection[allowed_countries][0]", "JP"),
            ("shipping_address_collection[allowed_countries][1]", "US"),
            ("shipping_address_collection[allowed_countries][2]", "GB"),
            ("shipping_address_collection[allowed_countries][3]", "FR"),
            ("shipping_address_collection[allowed_countries][4]", "DE"),
            ("shipping_address_collection[allowed_countries][5]", "AU"),
            ("shipping_address_collection[allowed_countries][6]", "KR"),
            ("shipping_address_collection[allowed_countries][7]", "TW"),
            ("shipping_address_collection[allowed_countries][8]", "HK"),
            // Stripe collects buyer email. Default size is the owner's; buyer
            // can change via shipping form (Stripe address has no size field
            // so size is determined by the owner-of-design's profile for now;
            // a follow-up will add a size selector on the slug page).
            ("allow_promotion_codes", "true"),
            ("metadata[you_design_id]", &id.to_string()),
            ("metadata[you_size]",      &default_size),
            ("metadata[you_serial]",    &serial),
            ("metadata[you_day_num]",   &day_num.to_string()),
            ("metadata[you_owner_slug]", owner_tag),
            ("metadata[you_public_buy]", "1"),
        ])
        .send().await;
    match resp {
        Ok(r) if r.status().is_success() => {
            let json: serde_json::Value = r.json().await.unwrap_or_default();
            let url = json["url"].as_str().unwrap_or("/").to_string();
            // We don't mark the design as claimed for public-buy until the
            // webhook confirms payment.
            Json(serde_json::json!({"url": url, "serial": serial})).into_response()
        }
        Ok(r) => {
            let status = r.status();
            let txt = r.text().await.unwrap_or_default();
            eprintln!("[you/public_buy] stripe {}: {}", status, txt);
            (StatusCode::BAD_GATEWAY, "stripe error").into_response()
        }
        Err(e) => {
            eprintln!("[you/public_buy] reqwest: {}", e);
            (StatusCode::BAD_GATEWAY, "stripe network error").into_response()
        }
    }
}

async fn you_claim(
    State(db): State<Db>,
    Json(body): Json<YouClaimBody>,
) -> impl IntoResponse {
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    if stripe_key.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "checkout disabled (STRIPE_SECRET_KEY not set)",
        ).into_response();
    }

    let (email, size, design_name, day_num) = {
        let conn = db.lock().unwrap();
        let row: Option<(i64, String, String)> = conn.query_row(
            "SELECT id, email, size FROM you_users
             WHERE token=? AND unsubscribed_at IS NULL",
            params![body.token],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).ok();
        let (uid, email, size) = match row {
            Some(v) => v,
            None => return (StatusCode::NOT_FOUND, "invalid token").into_response(),
        };
        let drow: Option<(i64, String, i64)> = conn.query_row(
            "SELECT user_id, name, day_num FROM you_designs WHERE id=?",
            params![body.design_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).ok();
        let (owner_id, name, day_num) = match drow {
            Some(v) => v,
            None => return (StatusCode::NOT_FOUND, "design not found").into_response(),
        };
        if owner_id != uid {
            return (StatusCode::FORBIDDEN, "not your design").into_response();
        }
        (email, size, name, day_num)
    };

    let serial = format!("YOU#{:04}", body.design_id);
    let display_name = format!("MU × YOU — {} ({}, {})", design_name, size, serial);
    let price_jpy: i64 = 6_800;
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.stripe.com/v1/checkout/sessions")
        .basic_auth(&stripe_key, None::<&str>)
        .form(&[
            ("mode", "payment"),
            ("currency", "jpy"),
            ("line_items[0][price_data][currency]", "jpy"),
            ("line_items[0][price_data][product_data][name]", &display_name),
            ("line_items[0][price_data][unit_amount]", &price_jpy.to_string()),
            ("line_items[0][quantity]", "1"),
            ("success_url", &format!("{}/success?sid={{CHECKOUT_SESSION_ID}}", base_url)),
            ("cancel_url", &format!("{}/you", base_url)),
            ("customer_email", &email),
            ("shipping_address_collection[allowed_countries][0]", "JP"),
            ("shipping_address_collection[allowed_countries][1]", "US"),
            ("shipping_address_collection[allowed_countries][2]", "GB"),
            ("shipping_address_collection[allowed_countries][3]", "FR"),
            ("shipping_address_collection[allowed_countries][4]", "DE"),
            ("shipping_address_collection[allowed_countries][5]", "AU"),
            ("shipping_address_collection[allowed_countries][6]", "KR"),
            ("shipping_address_collection[allowed_countries][7]", "TW"),
            ("shipping_address_collection[allowed_countries][8]", "HK"),
            ("allow_promotion_codes", "true"),
            ("metadata[you_design_id]", &body.design_id.to_string()),
            ("metadata[you_size]", &size),
            ("metadata[you_serial]", &serial),
            ("metadata[you_day_num]", &day_num.to_string()),
        ])
        .send().await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let json: serde_json::Value = r.json().await.unwrap_or_default();
            let url = json["url"].as_str().unwrap_or("/").to_string();
            // mark claimed
            let conn = db.lock().unwrap();
            conn.execute(
                "UPDATE you_designs SET status='claimed', updated_at=? WHERE id=?",
                params![chrono_now(), body.design_id],
            ).ok();
            Json(serde_json::json!({"url": url, "serial": serial})).into_response()
        }
        Ok(r) => {
            let status = r.status();
            let txt = r.text().await.unwrap_or_default();
            eprintln!("[you] stripe error {}: {}", status, txt);
            (StatusCode::INTERNAL_SERVER_ERROR,
                format!("stripe error: {}", &txt[..txt.len().min(200)])).into_response()
        }
        Err(e) => {
            eprintln!("[you] stripe request error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "stripe connection error").into_response()
        }
    }
}

async fn you_image(
    State(db): State<Db>,
    Path(id): Path<i64>,
) -> Response {
    let row: Option<(Option<Vec<u8>>, Option<String>, String)> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT image_bytes, image_mime, gen_status FROM you_designs WHERE id=?",
            params![id],
            |r| Ok((
                r.get::<_,Option<Vec<u8>>>(0)?,
                r.get::<_,Option<String>>(1)?,
                r.get::<_,String>(2)?,
            )),
        ).ok()
    };
    let (bytes, mime, status) = match row {
        Some(v) => v,
        None => return (StatusCode::NOT_FOUND, "design not found").into_response(),
    };

    if let (Some(b), m) = (bytes, mime) {
        let mime = m.unwrap_or_else(|| "image/png".into());
        let mut resp = b.into_response();
        let h = resp.headers_mut();
        if let Ok(v) = HeaderValue::from_str(&mime) {
            h.insert(header::CONTENT_TYPE, v);
        }
        h.insert(header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=2592000, immutable"));
        return resp;
    }

    // No bytes yet — return a 202 with a placeholder SVG so <img> still renders.
    let placeholder = r##"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 800 800'>
<defs><linearGradient id='g' x1='0%' y1='0%' x2='100%' y2='100%'>
  <stop offset='0%' stop-color='#1a1a1a'/><stop offset='100%' stop-color='#0a0a0a'/></linearGradient></defs>
<rect width='800' height='800' fill='url(#g)'/>
<text x='50%' y='48%' text-anchor='middle' fill='rgba(230,196,73,0.9)'
  font-family='Helvetica Neue,Arial' font-size='44' letter-spacing='10' font-weight='200'>GENERATING</text>
<text x='50%' y='56%' text-anchor='middle' fill='rgba(255,255,255,0.45)'
  font-family='Helvetica Neue,Arial' font-size='14' letter-spacing='6'>MU × YOU</text>
<text x='50%' y='62%' text-anchor='middle' fill='rgba(255,255,255,0.25)'
  font-family='monospace' font-size='10' letter-spacing='4'>Gemini 3 Pro · 30〜60s</text>
</svg>"##;
    let code = if status == "failed" { StatusCode::INTERNAL_SERVER_ERROR }
               else { StatusCode::ACCEPTED };
    let mut resp = (code, placeholder.to_string()).into_response();
    resp.headers_mut().insert(header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml"));
    resp.headers_mut().insert(header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"));
    resp
}

#[derive(Deserialize)]
struct YouTasteBody {
    token: String,
    #[serde(default)] mood: Vec<String>,
    #[serde(default)] palette: Vec<String>,
    #[serde(default)] scene: Vec<String>,
    #[serde(default)] size: String,
    #[serde(default)] bio: String,
    #[serde(default)] display_name: Option<String>,
}

#[derive(Deserialize)]
struct YouStyleBody {
    token: String,
    style_name: String,
}

/// Day-7 ritual: subscriber gives their personal "style name" (IKEA effect /
/// commitment). Stored on the user, used in milestone design prompts.
async fn you_style_set(
    State(db): State<Db>,
    Json(body): Json<YouStyleBody>,
) -> impl IntoResponse {
    let name = body.style_name.trim();
    if name.is_empty() || name.chars().count() > 32 {
        return (StatusCode::BAD_REQUEST, "1〜32 文字で").into_response();
    }
    let n = {
        let conn = db.lock().unwrap();
        conn.execute(
            "UPDATE you_users SET style_name=?, updated_at=?
             WHERE token=? AND unsubscribed_at IS NULL",
            params![name, chrono_now(), body.token],
        ).unwrap_or(0)
    };
    if n == 0 {
        return (StatusCode::NOT_FOUND, "invalid token").into_response();
    }
    Json(serde_json::json!({"ok": true, "style_name": name})).into_response()
}

async fn you_taste_update(
    State(db): State<Db>,
    Json(body): Json<YouTasteBody>,
) -> impl IntoResponse {
    let outcome: Result<(i64, String, String, Option<String>, Option<String>), Response> = {
        let conn = db.lock().unwrap();
        let row: Option<(i64, String, Option<String>, Option<String>)> = conn.query_row(
            "SELECT id, email, slug, display_name FROM you_users
             WHERE token=? AND unsubscribed_at IS NULL",
            params![body.token],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).ok();
        let (uid, email, slug, prev_display) = match row {
            Some(v) => v,
            None => return (StatusCode::NOT_FOUND, "invalid token").into_response(),
        };
        let size = if body.size.is_empty() { "S".to_string() } else { body.size.clone() };
        if !["XS","S","M","L","XL","XXL"].contains(&size.as_str()) {
            return (StatusCode::BAD_REQUEST, "invalid size").into_response();
        }
        let display_name = body.display_name.clone()
            .filter(|s| !s.trim().is_empty())
            .or(prev_display);
        let taste = serde_json::json!({
            "email": email,
            "mood": body.mood, "palette": body.palette, "scene": body.scene,
            "size": size, "bio": body.bio,
        });
        let now = chrono_now();
        if let Err(e) = conn.execute(
            "UPDATE you_users SET taste_json=?, size=?, display_name=?, updated_at=? WHERE id=?",
            params![taste.to_string(), size.clone(), display_name, now, uid],
        ) {
            eprintln!("[you/taste] update failed: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "could not save").into_response();
        }
        // Force regenerate today's design with the new taste
        let day = jst_today_str();
        let design_id = match ensure_design_for_day(&conn, uid, &day, &taste, true) {
            Ok((id, _)) => id,
            Err(e) => { eprintln!("[you/taste] regen failed: {}", e); 0 }
        };
        Ok((uid, email, taste.to_string(), slug, Some(design_id.to_string())))
    };
    let (uid, email, _taste_str, slug, design_id_s) = match outcome {
        Ok(v) => v,
        Err(r) => return r,
    };
    let design_id: i64 = design_id_s.and_then(|s| s.parse().ok()).unwrap_or(0);
    if design_id > 0 {
        spawn_gemini_for_design(db.clone(), design_id);
    }

    // Send confirmation email
    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    if !resend_key.is_empty() {
        let to = email.clone();
        let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
        let share = slug.clone().map(|s| format!("{}/{}", base_url.trim_end_matches('/'), s))
            .unwrap_or_else(|| format!("{}/you", base_url.trim_end_matches('/')));
        tokio::spawn(async move {
            let html = format!(r#"
<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:48px;max-width:560px;margin:0 auto">
  <div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:32px">MU × YOU</div>
  <div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#e6c449;opacity:0.85;margin-bottom:8px">Prompt updated</div>
  <div style="font-size:18px;font-weight:300;line-height:1.55;margin-bottom:24px">
    プロンプトを更新しました。<br>本日の案は新しい好みで再生成中です（30〜60秒）。
  </div>
  <p style="font-size:12px;line-height:1.85;opacity:0.65;margin-bottom:32px">
    明日以降の案も、この内容に沿って生成されます。<br>気が変わったら、いつでも下のリンクから書き直せます。
  </p>
  <a href="{share}" style="display:inline-block;background:#F5F5F0;color:#0A0A0A;padding:14px 28px;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;text-decoration:none;font-weight:700">本日の案を見る →</a>
  <p style="font-size:10px;opacity:0.4;margin-top:32px;line-height:1.7">
    退会は <code>STOP</code> 返信または /you ページの「退会」リンクから即時実行されます。
  </p>
</div>"#, share = share);
            let _ = reqwest::Client::new()
                .post("https://api.resend.com/emails")
                .bearer_auth(&resend_key)
                .json(&serde_json::json!({
                    "from": "MU × YOU <noreply@wearmu.com>",
                    "to": [to],
                    "subject": "MU × YOU — プロンプトを更新しました",
                    "html": html,
                }))
                .send().await;
        });
    }
    let _ = uid;
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    let share_url = slug.as_ref().map(|s|
        format!("{}/{}", base_url.trim_end_matches('/'), s));
    Json(serde_json::json!({
        "ok": true,
        "share_url": share_url,
        "slug": slug,
    })).into_response()
}

#[derive(Deserialize)]
struct YouAdminBackfillBody {
    admin_token: String,
    /// When true, re-generate today's design even if it already exists
    /// (useful after prompt-template changes, e.g. adding bio).
    #[serde(default)] force: bool,
}

/// Admin: list /you subscribers (read-only, no emails sent).
/// Use to verify count + addresses before triggering you-backfill.
async fn you_admin_list(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if let Err(r) = require_admin_token(q.get("token")) { return r; }
    let conn = db.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT id, email, slug, display_name, size, created_at, updated_at,
                CASE WHEN unsubscribed_at IS NULL THEN 0 ELSE 1 END
         FROM you_users
         ORDER BY id ASC"
    ) { Ok(s) => s, Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {}", e)).into_response() };
    let rows: Vec<serde_json::Value> = stmt.query_map([], |r| {
        Ok(serde_json::json!({
            "id":            r.get::<_, i64>(0)?,
            "email":         r.get::<_, String>(1)?,
            "slug":          r.get::<_, Option<String>>(2)?,
            "display_name":  r.get::<_, Option<String>>(3)?,
            "size":          r.get::<_, String>(4)?,
            "created_at":    r.get::<_, String>(5)?,
            "updated_at":    r.get::<_, String>(6)?,
            "unsubscribed":  r.get::<_, i64>(7)? == 1,
        }))
    }).map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default();
    let active = rows.iter().filter(|r| r["unsubscribed"] == false).count();
    Json(serde_json::json!({
        "total": rows.len(),
        "active": active,
        "subscribers": rows,
    })).into_response()
}

/// Day-7 commitment ritual: ask the subscriber to give their style a name.
/// IKEA effect — naming creates ownership of the design feed.
/// Idempotent on day7_email_sent_at.
fn send_day7_style_prompt_if_needed(db: Db, user_id: i64, email: String) {
    {
        let conn = db.lock().unwrap();
        let already: Option<String> = conn.query_row(
            "SELECT day7_email_sent_at FROM you_users WHERE id=?",
            params![user_id], |r| r.get(0),
        ).ok().flatten();
        if already.is_some() { return; }
        conn.execute(
            "UPDATE you_users SET day7_email_sent_at=? WHERE id=?",
            params![chrono_now(), user_id],
        ).ok();
    }
    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    if resend_key.is_empty() { return; }
    // Token for the deep-link is fetched in the spawned future to avoid
    // holding the DB lock across await.
    let db2 = db.clone();
    tokio::spawn(async move {
        let token: Option<String> = {
            let conn = db2.lock().unwrap();
            conn.query_row("SELECT token FROM you_users WHERE id=?",
                params![user_id], |r| r.get::<_, String>(0)).ok()
        };
        let link = match token {
            Some(t) => format!("https://wearmu.com/you?t={}#name-your-style", t),
            None => "https://wearmu.com/you".to_string(),
        };
        let html = format!(r#"
<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:48px;max-width:560px;margin:0 auto">
  <div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:32px">MU × YOU</div>
  <div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#e6c449;opacity:0.85;margin-bottom:8px">Day 7 / 30</div>
  <div style="font-size:18px;font-weight:300;line-height:1.5;margin-bottom:24px">7 日間で 7 案。あなたのスタイルが見えてきました。</div>
  <p style="font-size:12px;line-height:1.85;opacity:0.75;margin-bottom:24px">
    ここまでの選択は、あなただけのフィルター。<br>
    そのフィルターに <strong>名前</strong> を付けてください。<br>
    たとえば「霧と紙」「夜の引き算」「8 月の沈黙」など、3〜10 文字で。<br><br>
    付けた名前は、Day 14 と Day 30 の特別な一着に折り込まれます。
  </p>
  <a href="{link}" style="display:inline-block;background:#e6c449;color:#000;padding:14px 28px;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;text-decoration:none;font-weight:700">名前をつける →</a>
  <p style="font-size:10px;opacity:0.5;margin-top:32px;line-height:1.7">
    まだピンとこなければ、明日でも来週でも OK。/you ページからいつでも変えられます。
  </p>
</div>"#, link = link);
        let _ = reqwest::Client::new()
            .post("https://api.resend.com/emails")
            .bearer_auth(&resend_key)
            .json(&serde_json::json!({
                "from": "MU × YOU <noreply@wearmu.com>",
                "to": [email],
                "subject": "Day 7 — あなたのスタイルに名前を",
                "html": html,
            }))
            .send().await;
    });
}

/// Send a "5 days left in your free trial" email exactly once, when the
/// remaining trial window first dips under 6 days. Idempotent on the
/// trial_reminder_sent_at column.
fn send_trial_reminder_if_needed(
    db: Db, user_id: i64, email: String, trial_end_at: Option<&str>
) {
    let trial_end: u64 = match trial_end_at.and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => return,
    };
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    if trial_end <= now { return; }
    let days_left = (trial_end - now) / 86400;
    if days_left > 5 { return; }
    // Already sent? Bail.
    {
        let conn = db.lock().unwrap();
        let already: Option<String> = conn.query_row(
            "SELECT trial_reminder_sent_at FROM you_users WHERE id=?",
            params![user_id], |r| r.get(0),
        ).ok().flatten();
        if already.is_some() { return; }
        conn.execute(
            "UPDATE you_users SET trial_reminder_sent_at=? WHERE id=?",
            params![chrono_now(), user_id],
        ).ok();
    }
    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    if resend_key.is_empty() { return; }
    let subj_variant = you_subject_variant(&db);
    tokio::spawn(async move {
        let html = format!(r#"
<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:48px;max-width:560px;margin:0 auto">
  <div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:32px">MU × YOU</div>
  <div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#C8362C;opacity:0.95;margin-bottom:8px">あと {days} 日であなたの 30 案が消えます</div>
  <div style="font-size:20px;font-weight:300;line-height:1.4;margin-bottom:18px">{days} 日後、毎朝の一着は届かなくなります。</div>
  <p style="font-size:12px;line-height:1.85;opacity:0.78;margin-bottom:24px">
    ここまで {days_done} 案を見てきました。<br>
    その「あなただけの方向性」を <strong>失わない</strong> 方法はひとつ —<br>
    MU の T シャツを 1 着、手に入れること。1 着で MU × YOU は <strong>一生無料</strong>。<br>
    日割りすると 1 日 4 円以下。コーヒー 1 杯にも届きません。
  </p>
  <a href="https://wearmu.com/mugen" style="display:inline-block;background:#e6c449;color:#000;padding:14px 28px;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;text-decoration:none;font-weight:700;margin-right:8px">MUGEN を見る →</a>
  <a href="https://wearmu.com/ma" style="display:inline-block;border:1px solid rgba(255,255,255,0.2);color:#F5F5F0;padding:13px 22px;font-size:10px;letter-spacing:0.25em;text-transform:uppercase;text-decoration:none;opacity:0.8">週次 MA オークション</a>
  <p style="font-size:10px;opacity:0.5;margin-top:32px;line-height:1.7">
    トライアル終了後は、購入が無い限り毎日のデザイン配信は停止します。<br>退会は <code>STOP</code> 返信で即時。
  </p>
</div>"#, days = days_left.max(1), days_done = (30 - days_left as i64).max(0));
        let _ = reqwest::Client::new()
            .post("https://api.resend.com/emails")
            .bearer_auth(&resend_key)
            .json(&serde_json::json!({
                "from": "MU × YOU <noreply@wearmu.com>",
                "to": [email],
                "subject": you_email_subject(&subj_variant, "trial5d",
                    &serde_json::json!({"days_left": days_left as i64})),
                "html": html,
            }))
            .send().await;
    });
}

/// Send a "trial expired — buy a MU shirt to keep going" email once.
/// Idempotent on trial_expired_notice_sent_at.
fn send_trial_expired_notice_if_needed(
    db: Db, user_id: i64, email: String, _trial_end_at: Option<String>,
) {
    {
        let conn = db.lock().unwrap();
        let already: Option<String> = conn.query_row(
            "SELECT trial_expired_notice_sent_at FROM you_users WHERE id=?",
            params![user_id], |r| r.get(0),
        ).ok().flatten();
        if already.is_some() { return; }
        conn.execute(
            "UPDATE you_users SET trial_expired_notice_sent_at=? WHERE id=?",
            params![chrono_now(), user_id],
        ).ok();
    }
    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    if resend_key.is_empty() { return; }
    let subj_variant = you_subject_variant(&db);
    tokio::spawn(async move {
        let html = r#"
<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:48px;max-width:560px;margin:0 auto">
  <div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:32px">MU × YOU</div>
  <div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#e6c449;opacity:0.85;margin-bottom:8px">Trial Ended</div>
  <div style="font-size:18px;font-weight:300;line-height:1.5;margin-bottom:24px">30 日間のトライアル、ここまで届けてくれてありがとう。</div>
  <p style="font-size:12px;line-height:1.85;opacity:0.75;margin-bottom:24px">
    今日からは、毎朝 9 時のデザイン配信は一旦停止します。<br><br>
    <strong>もう一度 ON にする方法は 2 つ</strong>:<br>
    ① MU の T シャツを <strong>1 着</strong> 買う (¥6,800〜) → <strong>一生 ¥0</strong>。<br>
    ② サブスク <strong>¥980/月</strong> (いつでも解約)。<br><br>
    どちらでも、明日からまた毎朝、あなただけの一着が届きます。
  </p>
  <a href="https://wearmu.com/mugen" style="display:inline-block;background:#e6c449;color:#000;padding:14px 28px;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;text-decoration:none;font-weight:700;margin-right:8px;margin-bottom:8px">MU を買う →</a>
  <a href="https://wearmu.com/you?subscribe=1" style="display:inline-block;border:1px solid #e6c449;color:#e6c449;padding:13px 22px;font-size:10px;letter-spacing:0.25em;text-transform:uppercase;text-decoration:none;font-weight:700">¥980/月で続ける</a>
  <p style="font-size:10px;opacity:0.5;margin-top:32px;line-height:1.7">
    トライアル中の 30 案は <a href="https://wearmu.com/you" style="color:#e6c449">あなたのページ</a> でいつでも見返せます。
  </p>
</div>"#;
        let _ = reqwest::Client::new()
            .post("https://api.resend.com/emails")
            .bearer_auth(&resend_key)
            .json(&serde_json::json!({
                "from": "MU × YOU <noreply@wearmu.com>",
                "to": [email],
                "subject": you_email_subject(&subj_variant, "trial_end", &serde_json::json!({})),
                "html": html,
            }))
            .send().await;
    });
}

/// Admin: ensure today's design exists for every active subscriber and send
/// the daily email. Useful for manually verifying deliverability after deploy.
async fn you_admin_backfill(
    State(db): State<Db>,
    Json(body): Json<YouAdminBackfillBody>,
) -> impl IntoResponse {
    if let Err(r) = require_admin_token(Some(&body.admin_token)) { return r; }
    let day = jst_today_str();
    type UserRow = (
        i64, String, String, Option<String>, Option<String>, i64,
        String, Option<String>, Option<String>, Option<String>,
    );
    let users: Vec<UserRow> = {
        let conn = db.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, email, taste_json, slug, trial_end_at, COALESCE(lifetime_free,0),
                    created_at, style_name, subscription_status, subscription_until
             FROM you_users
             WHERE unsubscribed_at IS NULL"
        ) { Ok(s) => s, Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db").into_response() };
        stmt.query_map([], |r| Ok((
            r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?,
            r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?,
        ))).map(|it| it.filter_map(|r| r.ok()).collect())
           .unwrap_or_default()
    };

    let mut queued = 0;
    let mut skipped_expired = 0;
    for (uid, email, taste_json, _slug, trial_end_at, lifetime_free_int, created_at,
         style_name, _sub_status, sub_until) in &users {
        let lifetime_free = *lifetime_free_int != 0;
        // Skip non-active accounts (no daily email until they pay or buy MU).
        if !you_user_active_full(trial_end_at.as_deref(), lifetime_free, sub_until.as_deref()) {
            skipped_expired += 1;
            send_trial_expired_notice_if_needed(db.clone(), *uid, email.clone(), trial_end_at.clone());
            continue;
        }
        let created_secs: u64 = created_at.parse().unwrap_or(0);
        let day_n = days_since_signup_secs(created_secs);
        // Day-7 IKEA-effect ritual (asks the user to name their style),
        // sent once. Skips if they already set a style_name.
        if day_n >= 7 && day_n <= 9 && style_name.as_deref().unwrap_or("").is_empty() {
            send_day7_style_prompt_if_needed(db.clone(), *uid, email.clone());
        }
        // 5-days-left and trial-end reminders.
        send_trial_reminder_if_needed(db.clone(), *uid, email.clone(), trial_end_at.as_deref());
        let taste: serde_json::Value = serde_json::from_str(taste_json)
            .unwrap_or(serde_json::json!({}));
        let (did, needs) = {
            let conn = db.lock().unwrap();
            ensure_design_for_day(&conn, *uid, &day, &taste, body.force).unwrap_or((0, false))
        };
        // Without force we only spawn when the row actually needs work; otherwise
        // ready rows get re-sent each day at JST 9:00 unnecessarily.
        if did > 0 && (body.force || needs) {
            spawn_gemini_for_design(db.clone(), did);
            queued += 1;
        }
    }
    Json(serde_json::json!({
        "ok": true,
        "day": day,
        "users": users.len(),
        "queued": queued,
        "skipped_expired": skipped_expired,
    })).into_response()
}

/// Admin: synchronously sends "today's design ready" email to every
/// active subscriber whose today's design has image_bytes (gen_status=ready).
/// Unlike the fire-and-forget path inside spawn_gemini_for_design, this
/// awaits the Resend response and reports per-user success/failure so we
/// can debug missing deliveries.
async fn you_admin_email_today(
    State(db): State<Db>,
    Json(body): Json<YouAdminBackfillBody>,
) -> impl IntoResponse {
    if let Err(r) = require_admin_token(Some(&body.admin_token)) { return r; }
    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    if resend_key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "RESEND_API_KEY not set").into_response();
    }
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    let base = base_url.trim_end_matches('/').to_string();
    let day = jst_today_str();

    type Row = (i64, String, i64, String, String, Option<String>, String);
    let rows: Vec<Row> = {
        let conn = db.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT d.id, u.email, d.day_num, d.name, d.prompt, u.slug, u.token
             FROM you_designs d JOIN you_users u ON u.id = d.user_id
             WHERE d.day=? AND d.gen_status='ready'
               AND u.unsubscribed_at IS NULL
               AND length(coalesce(u.email,''))>3"
        ) { Ok(s)=>s, Err(_)=>return (StatusCode::INTERNAL_SERVER_ERROR, "db").into_response() };
        stmt.query_map(params![day], |r| Ok((
            r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?,
        ))).map(|it| it.filter_map(|r| r.ok()).collect())
           .unwrap_or_default()
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build().unwrap_or_default();

    let mut sent = 0;
    let mut failed: Vec<serde_json::Value> = vec![];
    let subj_variant = you_subject_variant(&db);
    for (design_id, email, day_num, name, prompt, slug, token) in &rows {
        let img_url = format!("{}/api/you/design/{}/image.png", base, design_id);
        let share = slug.as_ref()
            .map(|s| format!("{}/{}", base, s))
            .unwrap_or_else(|| format!("{}/you", base));
        let html = format!(r#"
<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:32px 0;margin:0">
  <div style="max-width:600px;margin:0 auto;padding:0 32px">
    <div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:32px">MU × YOU</div>
    <div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#e6c449;opacity:0.85;margin-bottom:8px">DAY {day_num:03}</div>
    <div style="font-size:24px;font-weight:200;line-height:1.4;margin-bottom:8px">{name}</div>
    <p style="font-size:12px;line-height:1.85;opacity:0.7;margin-bottom:24px;font-style:italic;border-left:2px solid #e6c449;padding-left:14px">{prompt}</p>
    <img src="{img}" alt="MU × YOU DAY {day_num}" style="width:100%;display:block;background:#1a1a1a;border-radius:2px;margin-bottom:24px">
    <a href="{share}" style="display:inline-block;background:#e6c449;color:#000;padding:16px 28px;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;text-decoration:none;font-weight:700;margin-right:8px">この一着を仕立てる →</a>
    <a href="{share}" style="display:inline-block;border:1px solid rgba(255,255,255,0.2);color:#F5F5F0;padding:15px 22px;font-size:10px;letter-spacing:0.25em;text-transform:uppercase;text-decoration:none;opacity:0.7">明日に期待 / Skip</a>
    {reactions}
    <p style="font-size:10px;opacity:0.45;margin-top:32px;line-height:1.7">
      気分が変わったら <a href="{share}" style="color:#e6c449">プロンプトを書き直す</a>こともできます。<br>
      退会は <code>STOP</code> 返信、または /you ページから即時。
    </p>
  </div>
</div>"#,
            day_num = day_num, name = name, prompt = prompt,
            img = img_url, share = share,
            reactions = email_reaction_row(*design_id, token));

        let resp = client
            .post("https://api.resend.com/emails")
            .bearer_auth(&resend_key)
            .json(&serde_json::json!({
                "from": "MU × YOU <noreply@wearmu.com>",
                "to": [email],
                "subject": you_email_subject(&subj_variant, "daily",
                    &serde_json::json!({"day_num": *day_num, "name": name})),
                "html": html,
            }))
            .send().await;
        match resp {
            Ok(r) if r.status().is_success() => {
                eprintln!("[you/email] sent design {} → {}", design_id, email);
                sent += 1;
            }
            Ok(r) => {
                let status = r.status();
                let txt = r.text().await.unwrap_or_default();
                eprintln!("[you/email] FAIL design {} → {}: {} {}",
                    design_id, email, status, &txt[..txt.len().min(200)]);
                failed.push(serde_json::json!({
                    "design_id": design_id, "email": email,
                    "status": status.as_u16(), "body": &txt[..txt.len().min(200)],
                }));
            }
            Err(e) => {
                eprintln!("[you/email] NET FAIL design {} → {}: {}", design_id, email, e);
                failed.push(serde_json::json!({
                    "design_id": design_id, "email": email, "error": e.to_string(),
                }));
            }
        }
        // gentle pacing to stay under Resend rate limits (free tier: 2/s)
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    }
    Json(serde_json::json!({
        "ok": true, "day": day,
        "candidates": rows.len(),
        "sent": sent,
        "failed_count": failed.len(),
        "failed": failed,
    })).into_response()
}

async fn you_unsubscribe(
    State(db): State<Db>,
    Json(body): Json<YouUnsubBody>,
) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let n = conn.execute(
        "UPDATE you_users SET unsubscribed_at=? WHERE token=?",
        params![chrono_now(), body.token],
    ).unwrap_or(0);
    if n == 0 {
        return (StatusCode::NOT_FOUND, "invalid token").into_response();
    }
    Json(serde_json::json!({"ok": true})).into_response()
}

// ── Slug / share page ────────────────────────────────────────────────────────

/// Reserved at root level: would clash with literal routes or static files.
/// Keep this list aligned with the router below + static/ directory.
const RESERVED_SLUGS: &[&str] = &[
    "ma", "muon", "mugen", "nouns", "you", "city", "tokushoho", "success",
    "wallet", "v", "products", "api", "static", "mockups", "u",
    "about", "press", "vision", "muer", "council", "sweep", "collab",
    "robots.txt", "sitemap.xml", "manifest.json",
    "favicon.ico", "favicon.svg", "favicon-16x16.png", "favicon-32x32.png",
    "apple-touch-icon.png", "icon-192.png", "icon-512.png", "og.jpg",
    "you.html", "tokushoho.html", "city.html", "about.html", "press.html",
    "nouns-proposal.html", "nouns-proposal", "index.html",
    // common reservations
    "admin", "auth", "login", "logout", "signup", "settings", "help",
    "support", "contact", "shop", "store", "cart", "checkout", "blog",
    "news", "press", "team", "jobs", "careers", "privacy", "terms",
    "legal", "twitter", "instagram", "facebook", "ig", "x", "linkedin",
    "discord", "github", "mail", "email", "rss", "feed", "search",
    "www", "ftp", "ssh", "root", "null", "undefined",
];

fn slug_valid(s: &str) -> bool {
    let len = s.chars().count();
    if !(3..=32).contains(&len) { return false; }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphanumeric() { return false; }
    for c in chars {
        let ok = c.is_ascii_alphanumeric() || c == '-' || c == '_';
        if !ok { return false; }
    }
    true
}

fn slug_reserved(s: &str) -> bool {
    let lo = s.to_ascii_lowercase();
    RESERVED_SLUGS.iter().any(|r| *r == lo.as_str())
}

fn random_slug() -> String {
    // 7-char base32-like slug from a UUID — short, lowercase, URL-safe.
    let raw = uuid::Uuid::new_v4().simple().to_string();
    let alphabet = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut out = String::with_capacity(7);
    for i in 0..7 {
        let byte = u8::from_str_radix(&raw[i*2..i*2+2], 16).unwrap_or(0);
        out.push(alphabet[(byte as usize) % alphabet.len()] as char);
    }
    out
}

#[derive(Deserialize)]
struct YouSlugBody {
    token: String,
    slug: String,
}

async fn you_slug_set(
    State(db): State<Db>,
    Json(body): Json<YouSlugBody>,
) -> impl IntoResponse {
    let slug = body.slug.trim().to_ascii_lowercase();
    if !slug_valid(&slug) {
        return (StatusCode::BAD_REQUEST,
            "invalid slug: 3-32 chars, a-z 0-9 - _ only, must start with alphanumeric")
            .into_response();
    }
    if slug_reserved(&slug) {
        return (StatusCode::CONFLICT, "this name is reserved").into_response();
    }
    let conn = db.lock().unwrap();
    // Check uniqueness vs other users
    let owner: Option<i64> = conn.query_row(
        "SELECT id FROM you_users WHERE slug=?", params![slug], |r| r.get(0),
    ).ok();
    let me: Option<i64> = conn.query_row(
        "SELECT id FROM you_users WHERE token=? AND unsubscribed_at IS NULL",
        params![body.token], |r| r.get(0),
    ).ok();
    let me = match me {
        Some(v) => v,
        None => return (StatusCode::NOT_FOUND, "invalid token").into_response(),
    };
    if let Some(other) = owner {
        if other != me {
            return (StatusCode::CONFLICT, "this name is taken").into_response();
        }
    }
    if let Err(e) = conn.execute(
        "UPDATE you_users SET slug=?, updated_at=? WHERE id=?",
        params![slug, chrono_now(), me],
    ) {
        eprintln!("[you] slug update failed: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR, "could not save").into_response();
    }
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    Json(serde_json::json!({
        "ok": true,
        "slug": slug,
        "share_url": format!("{}/{}", base_url.trim_end_matches('/'), slug),
    })).into_response()
}

async fn you_slug_check(
    State(db): State<Db>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    let slug = slug.trim().to_ascii_lowercase();
    if !slug_valid(&slug) {
        return Json(serde_json::json!({"available": false, "reason": "invalid"})).into_response();
    }
    if slug_reserved(&slug) {
        return Json(serde_json::json!({"available": false, "reason": "reserved"})).into_response();
    }
    let conn = db.lock().unwrap();
    let exists: bool = conn.query_row(
        "SELECT 1 FROM you_users WHERE slug=?", params![slug], |_| Ok(true),
    ).unwrap_or(false);
    Json(serde_json::json!({"available": !exists})).into_response()
}

/// Public per-user share page. Server-rendered HTML with full OGP/SEO.
/// Falls back to ServeDir behavior if the slug matches a static file at root
/// (e.g. /about.html), so we don't break the existing static asset access.
async fn slug_or_static(
    State(db): State<Db>,
    Path(slug): Path<String>,
) -> Response {
    let slug_lo = slug.to_ascii_lowercase();

    // Reserved names → never claim them. Try static fallback then 404.
    if slug_reserved(&slug_lo) {
        return serve_static_or_404(&slug);
    }
    // Invalid slug shape → static fallback then 404.
    if !slug_valid(&slug_lo) {
        return serve_static_or_404(&slug);
    }
    // Look up user
    let row: Option<(i64, String, Option<String>)> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT id, email, display_name FROM you_users
             WHERE slug=? AND unsubscribed_at IS NULL",
            params![slug_lo],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).ok()
    };
    let (uid, _email, display_name) = match row {
        Some(v) => v,
        None => return serve_static_or_404(&slug),
    };

    // Pull recent designs (history) for the share page + user bio
    let (designs, user_bio) = {
        let conn = db.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, day, day_num, name, prompt, status, gen_status, image_url
             FROM you_designs WHERE user_id=? ORDER BY day DESC LIMIT 24"
        ) {
            Ok(s) => s, Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
        };
        let v: Vec<(i64, String, i64, String, String, String, String, Option<String>)> =
            stmt.query_map(params![uid], |r| Ok((
                r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?,
            )))
            .map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        let taste_json: String = conn.query_row(
            "SELECT taste_json FROM you_users WHERE id=?",
            params![uid], |r| r.get(0),
        ).unwrap_or_else(|_| "{}".into());
        let taste: serde_json::Value =
            serde_json::from_str(&taste_json).unwrap_or(serde_json::json!({}));
        let bio = taste.get("bio").and_then(|v| v.as_str()).unwrap_or("").to_string();
        (v, bio)
    };

    let html = render_share_page(&slug_lo, display_name.as_deref(), &user_bio, &designs);
    let mut resp = Html(html).into_response();
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=300, s-maxage=300"),
    );
    resp
}

/// Serve a file from /static if it exists; otherwise 404.
/// This preserves wearmu.com/<asset>.<ext> access for legitimate static
/// files (about.html, robots.txt, og.jpg, etc.) when the slug doesn't
/// belong to a user.
fn serve_static_or_404(name: &str) -> Response {
    // Sanitize: no path traversal, no leading slash.
    if name.contains('/') || name.contains("..") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let path = std::path::Path::new("static").join(name);
    let bytes = std::fs::read(&path).ok();
    let bytes = match bytes {
        Some(b) => b,
        None => {
            // Try with .html appended for clean URLs (e.g. /about → about.html)
            let path2 = std::path::Path::new("static").join(format!("{}.html", name));
            match std::fs::read(&path2) {
                Ok(b) => return html_response(b),
                Err(_) => return (StatusCode::NOT_FOUND, "not found").into_response(),
            }
        }
    };
    let mime = mime_for(name);
    let mut resp = bytes.into_response();
    if let Ok(v) = HeaderValue::from_str(&mime) {
        resp.headers_mut().insert(header::CONTENT_TYPE, v);
    }
    resp
}

fn html_response(bytes: Vec<u8>) -> Response {
    let mut resp = bytes.into_response();
    resp.headers_mut().insert(header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"));
    resp
}

fn mime_for(name: &str) -> String {
    let lo = name.to_ascii_lowercase();
    let ext = lo.rsplit('.').next().unwrap_or("");
    match ext {
        "html" | "htm" => "text/html; charset=utf-8".into(),
        "css" => "text/css".into(),
        "js" => "application/javascript".into(),
        "json" => "application/json".into(),
        "svg" => "image/svg+xml".into(),
        "png" => "image/png".into(),
        "jpg" | "jpeg" => "image/jpeg".into(),
        "webp" => "image/webp".into(),
        "ico" => "image/x-icon".into(),
        "txt" => "text/plain; charset=utf-8".into(),
        "xml" => "application/xml".into(),
        "md" => "text/markdown; charset=utf-8".into(),
        _ => "application/octet-stream".into(),
    }
}

#[allow(clippy::type_complexity)]
fn render_share_page(
    slug: &str,
    display_name: Option<&str>,
    user_bio: &str,
    designs: &[(i64, String, i64, String, String, String, String, Option<String>)],
) -> String {
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    let canonical = format!("{}/{}", base_url.trim_end_matches('/'), slug);

    // Featured / primary image: first claimed, else first design with gen_status=ready, else first
    let primary = designs.iter().find(|d| d.5 == "claimed")
        .or_else(|| designs.iter().find(|d| d.6 == "ready"))
        .or_else(|| designs.first());
    let primary_img_path = primary
        .map(|d| format!("/api/you/design/{}/image.png", d.0))
        .unwrap_or_else(|| "/og.jpg".to_string());
    let og_image = format!("{}{}", base_url.trim_end_matches('/'), primary_img_path);
    let title_name = display_name.unwrap_or(slug);
    let n = designs.len();
    let claimed = designs.iter().filter(|d| d.5 == "claimed").count();

    let title = format!("@{} — MU × YOU コレクション | wearmu.com", html_escape(title_name));
    let description = if claimed > 0 {
        format!(
            "@{} さん専用の MU × YOU コラボTシャツ・コレクション。AI が毎日描く一着の案、これまで {} 案 / 仕立てたのは {} 着。あなたも始める →",
            html_escape(title_name), n, claimed,
        )
    } else if n > 0 {
        format!(
            "@{} さんは MU × YOU を始めました。AI が毎日描く一着の案、これまで {} 案。あなたも始める →",
            html_escape(title_name), n,
        )
    } else {
        format!("@{} さんは MU × YOU を始めたばかり。AI が毎日描くコラボTシャツの案。あなたも始める →",
            html_escape(title_name))
    };

    // Cards markup. Each card is buyable when status != claimed and the
    // image is already generated (gen_status=ready).
    let cards: String = designs.iter().map(|d| {
        let (id, day, day_num, name, prompt, status, gen_status, _img_url) = d;
        let img_src = format!("/api/you/design/{}/image.png", id);
        let buyable = status != "claimed" && gen_status == "ready";
        let label = if status == "claimed" { "CLAIMED · 仕立て済み" }
                    else if status == "skip" { "SKIPPED · あえて選ばれなかった" }
                    else if gen_status == "generating" { "GENERATING · 生成中" }
                    else if gen_status == "ready" { "AVAILABLE · ¥6,800" }
                    else if gen_status == "failed" { "FAILED · 再生成待ち" }
                    else { "PROPOSAL · 提案" };
        let class = if status == "claimed" { "card claimed" } else { "card" };
        let buy_btn = if buyable {
            format!(r##"<button class="buy-btn" data-buy-id="{id}" type="button">この一着を仕立てる · ¥6,800</button>"##, id = id)
        } else if status == &"claimed".to_string() {
            r##"<div class="buy-btn disabled" aria-disabled="true">SOLD · この一着は旅立ちました</div>"##.to_string()
        } else {
            String::new()
        };
        let rx_row = if buyable {
            format!(
                r##"<div class="rx-row">
      <button class="rx" data-rx-id="{id}" data-rx="love" type="button">🔥</button>
      <button class="rx" data-rx-id="{id}" data-rx="ok"   type="button">👍</button>
      <button class="rx" data-rx-id="{id}" data-rx="meh"  type="button">😐</button>
      <button class="rx" data-rx-id="{id}" data-rx="skip" type="button">👋</button>
    </div>"##, id = id)
        } else { String::new() };
        format!(
            r##"<div class="{class}" data-id="{id}">
  <div class="card-img" style="background-image:url('{img}')"></div>
  <div class="card-meta">
    <div class="day">DAY {day_num:03} · {day}</div>
    <div class="name">{name}</div>
    <div class="prompt">{prompt}</div>
    <div class="badge">{label}</div>
    {rx_row}
    {buy_btn}
  </div>
</div>"##,
            class = class,
            id = id,
            img = img_src,
            day_num = day_num,
            day = html_escape(day),
            name = html_escape(name),
            prompt = html_escape(prompt),
            label = label,
            buy_btn = buy_btn,
            rx_row = rx_row,
        )
    }).collect();

    let designs_jsonld: String = designs.iter().take(12).map(|d| {
        format!(
            r##"{{"@type":"Product","name":"{name}","image":"{base}/api/you/design/{id}/image.png","description":"{prompt}","brand":{{"@type":"Brand","name":"MU × YOU"}},"offers":{{"@type":"Offer","priceCurrency":"JPY","price":"6800","availability":"https://schema.org/InStock"}}}}"##,
            name = json_escape(&d.3),
            prompt = json_escape(&d.4),
            base = base_url.trim_end_matches('/'),
            id = d.0,
        )
    }).collect::<Vec<_>>().join(",");

    format!(r##"<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1.0,viewport-fit=cover">
<title>{title}</title>
<meta name="description" content="{description}">
<meta name="theme-color" content="#0A0A0A">
<link rel="canonical" href="{canonical}">
<link rel="icon" type="image/svg+xml" href="/favicon.svg">
<link rel="apple-touch-icon" sizes="180x180" href="/apple-touch-icon.png">
<link rel="manifest" href="/manifest.json">

<meta property="og:type" content="profile">
<meta property="og:url" content="{canonical}">
<meta property="og:title" content="@{slug} — MU × YOU コレクション">
<meta property="og:description" content="{description}">
<meta property="og:image" content="{og_image}">
<meta property="og:image:width" content="1200">
<meta property="og:image:height" content="1200">
<meta property="og:image:alt" content="@{slug} の MU × YOU コラボTシャツ">
<meta property="og:site_name" content="MU">
<meta property="og:locale" content="ja_JP">
<meta property="profile:username" content="{slug}">

<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="@{slug} — MU × YOU コレクション">
<meta name="twitter:description" content="{description}">
<meta name="twitter:image" content="{og_image}">

<script type="application/ld+json">
{{
  "@context": "https://schema.org",
  "@graph": [
    {{
      "@type": "Person",
      "@id": "{canonical}#person",
      "name": "@{slug}",
      "url": "{canonical}",
      "alternateName": "{title_name}"
    }},
    {{
      "@type": "ItemList",
      "@id": "{canonical}#list",
      "name": "@{slug} の MU × YOU コレクション",
      "numberOfItems": {n},
      "itemListElement": [{designs_jsonld}]
    }}
  ]
}}
</script>

<style>
:root{{--k:#0A0A0A;--w:#F5F5F0;--y:#e6c449;--r:#C8362C;--g:#1C1C1C}}
*,*::before,*::after{{margin:0;padding:0;box-sizing:border-box}}
body{{background:var(--k);color:var(--w);font-family:'Helvetica Neue',Arial,sans-serif;
  font-weight:200;-webkit-font-smoothing:antialiased;line-height:1.6}}
a{{color:inherit;text-decoration:none}}
nav{{position:fixed;top:0;left:0;right:0;z-index:50;display:flex;justify-content:space-between;
  align-items:center;padding:14px 24px;background:rgba(10,10,10,0.92);backdrop-filter:blur(12px);
  border-bottom:1px solid rgba(255,255,255,0.05)}}
.nav-logo{{font-size:14px;font-weight:700;letter-spacing:0.45em}}
.nav-cta{{background:var(--y);color:#000;font-size:9px;letter-spacing:0.3em;text-transform:uppercase;
  padding:10px 18px;font-weight:700}}
.nav-cta:hover{{opacity:0.9}}
header.hero{{padding:120px 24px 60px;text-align:center;position:relative}}
.hero-bg{{position:absolute;inset:0;background:
  radial-gradient(ellipse at 30% 20%,rgba(230,196,73,0.08),transparent 50%),
  radial-gradient(ellipse at 70% 80%,rgba(200,54,44,0.06),transparent 55%);pointer-events:none}}
.eyebrow{{font-size:9px;letter-spacing:0.45em;text-transform:uppercase;color:var(--y);opacity:0.85;margin-bottom:24px;display:flex;align-items:center;gap:14px;justify-content:center}}
.dot{{width:6px;height:6px;background:var(--y);border-radius:50%;animation:p 2s infinite}}
@keyframes p{{0%,100%{{opacity:1}}50%{{opacity:0.4}}}}
h1.handle{{font-size:clamp(48px,9vw,108px);font-weight:200;letter-spacing:0.04em;
  line-height:1.05;background:linear-gradient(135deg,var(--y) 0%,#fff 60%);
  -webkit-background-clip:text;background-clip:text;color:transparent;display:inline-block}}
.handle-prefix{{color:rgba(255,255,255,0.4);background:none;-webkit-text-fill-color:rgba(255,255,255,0.4)}}
.bio{{font-size:14px;opacity:0.65;line-height:1.95;max-width:520px;margin:24px auto 0;font-weight:300}}
.userbio{{font-size:14px;font-style:italic;opacity:0.8;line-height:1.85;max-width:520px;margin:18px auto 0;
  font-weight:300;letter-spacing:0.02em;color:#fff}}
.stats{{display:flex;gap:48px;justify-content:center;margin-top:48px;flex-wrap:wrap}}
.stat .v{{font-size:28px;font-weight:200;color:var(--y);letter-spacing:0.03em}}
.stat .l{{font-size:8px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.5;margin-top:4px}}
main{{padding:0 24px 80px;max-width:1280px;margin:0 auto}}
.section-h{{font-size:11px;letter-spacing:0.4em;text-transform:uppercase;opacity:0.55;margin:48px 0 24px}}
.grid{{display:grid;grid-template-columns:repeat(auto-fill,minmax(260px,1fr));gap:16px}}
.card{{background:var(--g);overflow:hidden;display:flex;flex-direction:column;
  border:1px solid rgba(255,255,255,0.04);transition:transform 0.2s,border-color 0.2s}}
.card:hover{{transform:translateY(-2px);border-color:rgba(230,196,73,0.25)}}
.card.claimed{{border-color:rgba(230,196,73,0.35)}}
.card-img{{aspect-ratio:1;background:#000 center/cover no-repeat}}
.card-meta{{padding:18px 18px 22px;display:flex;flex-direction:column;gap:10px}}
.card .day{{font-size:8px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.5}}
.card .name{{font-size:15px;font-weight:300;letter-spacing:0.03em;line-height:1.4}}
.card .prompt{{font-size:11px;opacity:0.55;line-height:1.7;
  display:-webkit-box;-webkit-line-clamp:3;-webkit-box-orient:vertical;overflow:hidden}}
.card .badge{{display:inline-block;align-self:flex-start;font-size:8px;letter-spacing:0.25em;
  text-transform:uppercase;background:rgba(230,196,73,0.1);color:var(--y);padding:4px 10px}}
.card.claimed .badge{{background:var(--y);color:#000}}
.buy-btn{{margin-top:6px;background:var(--y);color:#000;border:0;padding:11px 14px;
  font-size:10px;letter-spacing:0.18em;text-transform:uppercase;font-weight:700;cursor:pointer;
  font-family:'Helvetica Neue',Arial,sans-serif;transition:transform 0.15s, background 0.15s}}
.buy-btn:hover{{transform:translateY(-1px);background:#fff}}
.buy-btn:disabled,.buy-btn.disabled{{background:rgba(255,255,255,0.06);color:rgba(255,255,255,0.4);cursor:not-allowed;font-weight:500}}
.rx-row{{display:flex;gap:4px;margin-top:2px}}
.rx{{background:rgba(255,255,255,0.04);border:1px solid rgba(255,255,255,0.08);color:#F5F5F0;padding:6px 9px;font-size:13px;cursor:pointer;border-radius:2px;line-height:1;transition:background 0.12s, transform 0.12s}}
.rx:hover{{background:rgba(230,196,73,0.18);transform:translateY(-1px)}}
.rx.on{{background:#e6c449;color:#000;border-color:#e6c449}}
.cta-block{{margin:80px auto 0;max-width:680px;text-align:center;padding:48px 24px;
  background:linear-gradient(180deg,rgba(230,196,73,0.06),transparent);
  border-top:1px solid rgba(230,196,73,0.15)}}
.cta-h{{font-size:clamp(22px,4vw,36px);font-weight:200;letter-spacing:0.03em;line-height:1.3;margin-bottom:16px}}
.cta-sub{{font-size:13px;opacity:0.6;margin-bottom:28px;line-height:1.85}}
.cta-btn{{display:inline-flex;align-items:center;gap:12px;background:var(--w);color:var(--k);
  padding:18px 32px;font-size:10px;letter-spacing:0.35em;text-transform:uppercase;font-weight:700}}
.cta-btn:hover{{transform:translateY(-2px)}}
footer{{padding:40px 24px;border-top:1px solid rgba(255,255,255,0.05);
  display:flex;justify-content:space-between;align-items:center;gap:24px;flex-wrap:wrap;
  font-size:9px;letter-spacing:0.25em;text-transform:uppercase;opacity:0.45}}
.empty{{text-align:center;padding:80px 24px;font-size:12px;opacity:0.4;letter-spacing:0.1em}}
@media(max-width:600px){{
  .stats{{gap:28px}} h1.handle{{font-size:48px}}
  .grid{{grid-template-columns:1fr 1fr;gap:8px}}
  .card-meta{{padding:12px 12px 16px}}
  .card .name{{font-size:13px}}
}}
</style>
</head>
<body>
<nav>
  <a href="/" class="nav-logo">MU</a>
  <a href="/you" class="nav-cta">あなたも始める</a>
</nav>
<header class="hero">
  <div class="hero-bg"></div>
  <div class="eyebrow"><span class="dot"></span>MU × YOU · profile</div>
  <h1 class="handle"><span class="handle-prefix">@</span>{slug}</h1>
  {bio_block}
  <p class="bio">{description}</p>
  <div class="stats">
    <div class="stat"><div class="v">{n}</div><div class="l">designs</div></div>
    <div class="stat"><div class="v">{claimed}</div><div class="l">claimed</div></div>
    <div class="stat"><div class="v">¥6,800</div><div class="l">per tee</div></div>
  </div>
</header>
<main>
  <div class="section-h">Collection</div>
  {grid}
  <div class="cta-block">
    <div class="cta-h">あなたの "ほしい" も、毎朝AIが描く。</div>
    <div class="cta-sub">気分・色・着るシーンを 1 分で登録。<br>翌朝から毎日、あなた専用のTシャツ案が 1 枚届きます。</div>
    <a href="/you" class="cta-btn">MU × YOU を始める →</a>
  </div>
</main>
<footer>
  <div>MU × YOU © wearmu.com</div>
  <div style="display:flex;gap:24px"><a href="/">MU</a><a href="/you">/you</a><a href="/tokushoho">特商法</a></div>
</footer>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
<script src="/exit-funnel.js" defer></script>
<script>
// Anonymous reaction signal — anyone on the share page can tap an emoji
// and the design's owner sees their next-day prompt bend accordingly.
document.addEventListener('click', async (e) => {{
  const rx = e.target.closest('.rx[data-rx-id]');
  if (!rx) return;
  const id = rx.getAttribute('data-rx-id');
  const kind = rx.getAttribute('data-rx');
  rx.classList.add('on');
  try {{
    await fetch('/api/you/signal/' + id, {{
      method: 'POST', headers: {{'Content-Type': 'application/json'}},
      body: JSON.stringify({{kind: kind, source: 'slug'}}),
    }});
  }} catch (_) {{}}
}});

// Public buy: any visitor on the share page can buy a /you design.
document.addEventListener('click', async (e) => {{
  const btn = e.target.closest('.buy-btn[data-buy-id]');
  if (!btn) return;
  const id = btn.getAttribute('data-buy-id');
  if (!id || btn.disabled) return;
  btn.disabled = true;
  const orig = btn.textContent;
  btn.textContent = '読み込み中…';
  try {{
    const r = await fetch('/api/you/buy/' + id, {{method: 'POST'}});
    if (!r.ok) throw new Error('HTTP ' + r.status);
    const data = await r.json();
    if (data && data.url) {{
      window.location.href = data.url;
      return;
    }}
    throw new Error('no checkout url');
  }} catch (err) {{
    btn.disabled = false;
    btn.textContent = orig;
    alert('購入処理を起動できませんでした。少し待って再度お試しください。\n(' + err.message + ')');
  }}
}});
</script>
</body>
</html>"##,
        title = html_escape(&title),
        description = html_escape(&description),
        canonical = canonical,
        slug = html_escape(slug),
        og_image = og_image,
        title_name = html_escape(title_name),
        n = n,
        claimed = claimed,
        designs_jsonld = designs_jsonld,
        bio_block = if user_bio.trim().is_empty() {
            String::new()
        } else {
            format!(r#"<p class="userbio">"{}"</p>"#, html_escape(user_bio))
        },
        grid = if designs.is_empty() {
            r#"<div class="empty">まだデザインがありません</div>"#.to_string()
        } else {
            format!(r#"<div class="grid">{}</div>"#, cards)
        },
    )
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=info,axum::rejection=trace".into()),
        )
        .with_target(false)
        .compact()
        .init();

    let db_path = env::var("DB_PATH").unwrap_or_else(|_| "products.db".into());
    let conn = Connection::open(&db_path).expect("open db");
    conn.execute_batch("PRAGMA journal_mode=WAL;").ok();
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS products (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            brand        TEXT NOT NULL,
            drop_num     INTEGER NOT NULL,
            name         TEXT NOT NULL,
            design_url   TEXT,
            mockup_url   TEXT,
            price_jpy    INTEGER NOT NULL,
            inventory    INTEGER NOT NULL,
            sold         INTEGER DEFAULT 0,
            created_at   TEXT NOT NULL,
            active       INTEGER DEFAULT 1,
            weather_data TEXT,
            prompt_text  TEXT,
            prompt_hash  TEXT,
            seed_data    TEXT,
            auction_end  TEXT,
            current_bid  INTEGER DEFAULT 0,
            bid_count    INTEGER DEFAULT 0,
            nft_mint     TEXT,
            parent_design TEXT,
            sold_out_at  TEXT
        );
        CREATE TABLE IF NOT EXISTS bids (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            product_id INTEGER NOT NULL,
            amount     INTEGER NOT NULL,
            email      TEXT NOT NULL,
            wallet     TEXT,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS fragment_requests (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            email      TEXT NOT NULL,
            direction  TEXT NOT NULL,
            order_ids  TEXT NOT NULL,
            status     TEXT NOT NULL DEFAULT 'pending',
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS kyc_records (
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            product_id       INTEGER NOT NULL,
            email            TEXT NOT NULL,
            full_name        TEXT NOT NULL,
            dob              TEXT NOT NULL,
            nationality      TEXT NOT NULL,
            id_type          TEXT NOT NULL,
            id_last4         TEXT NOT NULL,
            address          TEXT NOT NULL,
            consent_at       TEXT NOT NULL,
            payment_method   TEXT NOT NULL,
            total_amount_jpy INTEGER NOT NULL,
            created_at       TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_kyc_records_email ON kyc_records(email);
        CREATE INDEX IF NOT EXISTS idx_kyc_records_created ON kyc_records(created_at DESC);
        CREATE TABLE IF NOT EXISTS pending_crypto_payments (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            reference       TEXT NOT NULL UNIQUE,
            product_id      INTEGER NOT NULL,
            email           TEXT NOT NULL,
            size            TEXT NOT NULL DEFAULT 'M',
            quantity        INTEGER NOT NULL DEFAULT 1,
            wallet          TEXT,
            payment_method  TEXT NOT NULL,
            amount_jpy      INTEGER NOT NULL,
            amount_crypto   TEXT NOT NULL,
            asset           TEXT NOT NULL,
            recipient       TEXT NOT NULL,
            pay_url         TEXT NOT NULL,
            status          TEXT NOT NULL DEFAULT 'pending',
            tx_signature    TEXT,
            confirmed_at    TEXT,
            expires_at      TEXT NOT NULL,
            created_at      TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_pcp_reference ON pending_crypto_payments(reference);
        CREATE INDEX IF NOT EXISTS idx_pcp_status ON pending_crypto_payments(status, created_at DESC);
        CREATE TABLE IF NOT EXISTS crypto_settings (
            key        TEXT PRIMARY KEY,
            value      TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS you_users (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            email           TEXT NOT NULL UNIQUE,
            token           TEXT NOT NULL UNIQUE,
            slug            TEXT UNIQUE,
            display_name    TEXT,
            taste_json      TEXT NOT NULL DEFAULT '{}',
            size            TEXT NOT NULL DEFAULT 'S',
            created_at      TEXT NOT NULL,
            updated_at      TEXT NOT NULL,
            unsubscribed_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_you_users_token ON you_users(token);
        CREATE TABLE IF NOT EXISTS you_designs (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id         INTEGER NOT NULL,
            day             TEXT NOT NULL,
            day_num         INTEGER NOT NULL,
            name            TEXT NOT NULL,
            prompt          TEXT NOT NULL,
            seed            TEXT NOT NULL,
            image_url       TEXT,
            image_bytes     BLOB,
            image_mime      TEXT,
            gen_status      TEXT NOT NULL DEFAULT 'pending',
            gen_error       TEXT,
            status          TEXT NOT NULL DEFAULT 'pending',
            size            TEXT,
            refresh_count   INTEGER NOT NULL DEFAULT 0,
            created_at      TEXT NOT NULL,
            updated_at      TEXT NOT NULL,
            UNIQUE(user_id, day)
        );
        CREATE INDEX IF NOT EXISTS idx_you_designs_user ON you_designs(user_id, day DESC);
        CREATE TABLE IF NOT EXISTS you_feedback (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id     INTEGER NOT NULL,
            design_id   INTEGER NOT NULL,
            action      TEXT NOT NULL,
            created_at  TEXT NOT NULL
        );
        -- 架空のサンプル ペルソナ (12-20名)。/you の gallery セクションに表示し、
        -- 各ペルソナが「今日もらったデザイン」として実在の MUGEN drop を購入
        -- 動線にリンクさせる。毎日 cron で picked_product_id を再ロールする。
        CREATE TABLE IF NOT EXISTS sample_personas (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            slug            TEXT NOT NULL UNIQUE,
            display_name    TEXT NOT NULL,
            bio             TEXT NOT NULL,
            taste_json      TEXT NOT NULL,
            avatar_glyph    TEXT,
            active          INTEGER NOT NULL DEFAULT 1,
            created_at      TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_sample_personas_active ON sample_personas(active);
        -- ペルソナの「今日もらった案」履歴。day=YYYY-MM-DD ごとに1行。
        -- picked_product_id は MUGEN drops のうち未売り切れのものから決定的に選ぶ。
        CREATE TABLE IF NOT EXISTS sample_designs (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            persona_id        INTEGER NOT NULL,
            day               TEXT NOT NULL,
            day_num           INTEGER NOT NULL,
            name              TEXT NOT NULL,
            prompt            TEXT NOT NULL,
            picked_product_id INTEGER,
            image_url         TEXT,
            created_at        TEXT NOT NULL,
            UNIQUE(persona_id, day)
        );
        CREATE INDEX IF NOT EXISTS idx_sample_designs_day ON sample_designs(day DESC);
        CREATE INDEX IF NOT EXISTS idx_sample_designs_persona ON sample_designs(persona_id, day DESC);
    ").expect("init schema");
    // Idempotent column additions for existing DBs
    for col in &[
        // Phase 3.1: shipping collection on crypto checkout
        "ALTER TABLE pending_crypto_payments ADD COLUMN ship_name TEXT",
        "ALTER TABLE pending_crypto_payments ADD COLUMN ship_line1 TEXT",
        "ALTER TABLE pending_crypto_payments ADD COLUMN ship_line2 TEXT",
        "ALTER TABLE pending_crypto_payments ADD COLUMN ship_city TEXT",
        "ALTER TABLE pending_crypto_payments ADD COLUMN ship_state TEXT",
        "ALTER TABLE pending_crypto_payments ADD COLUMN ship_zip TEXT",
        "ALTER TABLE pending_crypto_payments ADD COLUMN ship_country TEXT",
        "ALTER TABLE pending_crypto_payments ADD COLUMN ship_phone TEXT",
        "ALTER TABLE pending_crypto_payments ADD COLUMN printful_order_id TEXT",
        "ALTER TABLE pending_crypto_payments ADD COLUMN fulfilled_at TEXT",
        "ALTER TABLE products ADD COLUMN weather_data TEXT",
        "ALTER TABLE products ADD COLUMN prompt_text TEXT",
        "ALTER TABLE products ADD COLUMN prompt_hash TEXT",
        "ALTER TABLE products ADD COLUMN seed_data TEXT",
        "ALTER TABLE products ADD COLUMN auction_end TEXT",
        "ALTER TABLE products ADD COLUMN current_bid INTEGER DEFAULT 0",
        "ALTER TABLE products ADD COLUMN bid_count INTEGER DEFAULT 0",
        "ALTER TABLE products ADD COLUMN nft_mint TEXT",
        "ALTER TABLE products ADD COLUMN parent_design TEXT",
        "ALTER TABLE products ADD COLUMN sold_out_at TEXT",
        "ALTER TABLE bids ADD COLUMN wallet_token TEXT",
        // Soulbound NFT pilot opt-in (per-bid; carried to settle_auction).
        "ALTER TABLE bids ADD COLUMN nft_opt_in INTEGER DEFAULT 0",
        "ALTER TABLE bids ADD COLUMN nft_wallet TEXT",
        "ALTER TABLE you_designs ADD COLUMN image_bytes BLOB",
        "ALTER TABLE you_designs ADD COLUMN image_mime TEXT",
        "ALTER TABLE you_designs ADD COLUMN gen_status TEXT NOT NULL DEFAULT 'pending'",
        "ALTER TABLE you_designs ADD COLUMN gen_error TEXT",
        "ALTER TABLE you_designs ADD COLUMN daily_email_sent_at TEXT",
        "ALTER TABLE products ADD COLUMN serial_code TEXT",
        "ALTER TABLE you_users ADD COLUMN slug TEXT",
        "ALTER TABLE you_users ADD COLUMN display_name TEXT",
        // 30-day trial / lifetime-free for MU shirt owners
        "ALTER TABLE you_users ADD COLUMN trial_end_at TEXT",
        "ALTER TABLE you_users ADD COLUMN lifetime_free INTEGER DEFAULT 0",
        "ALTER TABLE you_users ADD COLUMN lifetime_reason TEXT",
        "ALTER TABLE you_users ADD COLUMN trial_reminder_sent_at TEXT",
        "ALTER TABLE you_users ADD COLUMN trial_expired_notice_sent_at TEXT",
        // Day-7 IKEA-effect commitment ritual: user names their style
        "ALTER TABLE you_users ADD COLUMN style_name TEXT",
        "ALTER TABLE you_users ADD COLUMN style_name_prompted_at TEXT",
        // Day-7 / Day-14 / Day-25 / Day-30 trigger guards (idempotent emails)
        "ALTER TABLE you_users ADD COLUMN day7_email_sent_at TEXT",
        "ALTER TABLE you_users ADD COLUMN day14_peak_sent_at TEXT",
        "ALTER TABLE you_users ADD COLUMN bonus_drops_sent INTEGER DEFAULT 0",
        // Mark a design as the bonus / milestone variant for UI badges
        "ALTER TABLE you_designs ADD COLUMN kind TEXT NOT NULL DEFAULT 'daily'",
        // ¥980/月 paid subscription tier (alternative to buying a MU shirt).
        "ALTER TABLE you_users ADD COLUMN stripe_customer_id TEXT",
        "ALTER TABLE you_users ADD COLUMN stripe_subscription_id TEXT",
        "ALTER TABLE you_users ADD COLUMN subscription_status TEXT",
        "ALTER TABLE you_users ADD COLUMN subscription_until TEXT",
        // Lifestyle photo (人着画) generated via Gemini image-to-image from
        // the design itself. R2 URL.
        "ALTER TABLE products ADD COLUMN lifestyle_url TEXT",
        // Referral: which you_user.slug brought this signup in?
        "ALTER TABLE you_users ADD COLUMN referred_by_slug TEXT",
        // Lifetime referral credit (¥). Spendable via auto-mint coupon.
        "ALTER TABLE you_users ADD COLUMN referral_credit_jpy INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE you_users ADD COLUMN referral_count INTEGER NOT NULL DEFAULT 0",
        // Thank-you outreach to real buyers (cs_live_*). Idempotent.
        "ALTER TABLE mu_purchases ADD COLUMN thank_you_sent_at TEXT",
        "ALTER TABLE mu_purchases ADD COLUMN thank_you_coupon TEXT",
        // X (Twitter) auto-post — set when twitter_post.py succeeds.
        "ALTER TABLE products ADD COLUMN x_posted_at TEXT",
        "ALTER TABLE products ADD COLUMN x_tweet_id TEXT",
    ] {
        conn.execute(col, []).ok();
    }

    // ── MU × Collab partner products (e.g. SWEEP, draft, password gated) ──
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS collab_products (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            slug        TEXT UNIQUE NOT NULL,
            partner     TEXT NOT NULL,
            category    TEXT NOT NULL,
            name        TEXT NOT NULL,
            description TEXT,
            image_url   TEXT,
            price_jpy   INTEGER NOT NULL,
            sizes_json  TEXT,
            active      INTEGER NOT NULL DEFAULT 1,
            draft       INTEGER NOT NULL DEFAULT 1,
            created_at  TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_collab_partner ON collab_products(partner, active);
        -- 注文記録 (Stripe webhook 経由)。production_route で
        --   'printful' = 自動発注 (printful_variant_id set 必須)
        --   'sweep_manual' = SWEEP社 手動生産 (Telegram 通知のみ)
        --   'pre_order' = 受注生産 (Gi など、SWEEP社 が個別対応)
        CREATE TABLE IF NOT EXISTS collab_orders (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            stripe_session  TEXT UNIQUE NOT NULL,
            slug            TEXT NOT NULL,
            size            TEXT,
            email           TEXT,
            ship_name       TEXT,
            ship_address    TEXT,
            ship_country    TEXT,
            amount_jpy      INTEGER,
            production_route TEXT,
            printful_order_id TEXT,
            status          TEXT NOT NULL DEFAULT 'received',
            created_at      TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_collab_orders_slug ON collab_orders(slug);
        -- 商品ごとの好き嫌い 1-clic シグナル + 自由記述 FB。
        --   kind: 'love' (👍) / 'meh' (👎) / 'comment' (自由記述同送)
        --   visitor_token は cookie 由来の匿名 ID。集計用、再投稿の弱め判定。
        CREATE TABLE IF NOT EXISTS sweep_signals (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            slug          TEXT NOT NULL,
            kind          TEXT NOT NULL,
            comment       TEXT,
            email         TEXT,
            visitor_token TEXT,
            user_agent    TEXT,
            created_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_sweep_signals_slug ON sweep_signals(slug, kind);
        CREATE INDEX IF NOT EXISTS idx_sweep_signals_at ON sweep_signals(created_at DESC);
    ").ok();
    // Idempotent extra columns on collab_products (run after CREATE so we can
    // add Printful integration columns without dropping the table).
    for col in &[
        "ALTER TABLE collab_products ADD COLUMN printful_product_id INTEGER",
        "ALTER TABLE collab_products ADD COLUMN printful_variant_id INTEGER",
        "ALTER TABLE collab_products ADD COLUMN production_route TEXT NOT NULL DEFAULT 'sweep_manual'",
        "ALTER TABLE collab_products ADD COLUMN lead_time_days INTEGER NOT NULL DEFAULT 21",
        // JSON: {\"S\":<id>,\"M\":<id>,\"L\":<id>,\"XL\":<id>} or {\"OS\":<id>} for one-size.
        "ALTER TABLE collab_products ADD COLUMN printful_variant_map TEXT",
        // JSON array of [{type, url}] — type='default' for DTG, 'embroidery_*' for stitching.
        // All-over-print and DTG use 'default'; specific embroidery placements per product.
        "ALTER TABLE collab_products ADD COLUMN printful_files TEXT",
        // JSON array of [{id, value}] options like thread_colors_front_large, stitch_color.
        "ALTER TABLE collab_products ADD COLUMN printful_options TEXT",
    ] {
        conn.execute(col, []).ok();
    }

    // Seed MU × SWEEP items (idempotent on slug).
    //
    // 13 商品が正式販売可能 (active=1):
    //   - 3 BJJ items (rashguard/shorts/spats) use Printful all-over print
    //     athletic apparel (rash guard 301, athletic long shorts 332, leggings 189).
    //   - 10 lifestyle items use Printful catalog with verified variant IDs.
    //
    // 4 商品は SWEEP社 サインオフ前のため非表示 (active=0):
    //   gi, belt, BJJ tape, mouthguard case — Printful カタログに無い。
    //   SWEEP社 と契約完了後に active=1 へ。
    //
    // variant_map JSON: {\"S\":id,\"M\":id,\"L\":id,\"XL\":id} or {\"OS\":id} for one-size.
    // ── 共通の Printful 印刷ファイル / オプション ──
    //
    // 印刷ファイル URL は SIIIEEP 公式ロゴ (PNG 透過、3000×474)。R2 lifestyle.wearmu.com/sweep/_logo.png に配置。
    // 商品ごとの placement (files[].type) + thread_colors key は Printful catalog からの実測。
    const LOGO_URL: &str = "https://lifestyle.wearmu.com/sweep/_logo.png";

    // 全面プリント (rashguard, shorts, spats) はライフスタイル写真を front に。
    // ステッチ色は黒 (生地白の場合) — Printful はラベル/ステッチで使用。
    let allover_options = r#"[{"id":"stitch_color","value":"black"}]"#;
    // DTG (tee, hoodie, longsleeve, tote, tee-classic) は default 配置で胸に SIIIEEP wordmark。
    // DTG は thread option 不要 (インクジェット印刷)。
    let dtg_no_options: Option<&str> = None;

    // Seed MU × SIIIEEP items (idempotent on slug).
    //
    // 13 商品が正式販売可能 (active=1):
    //   - 3 BJJ items (rashguard/shorts/spats) use Printful all-over print
    //     athletic apparel (rash guard 301, athletic long shorts 332, leggings 189).
    //   - 10 lifestyle items use Printful catalog with verified variant IDs +
    //     correct placement + thread_colors options for each embroidery product.
    //
    // 4 商品は SIIIEEP社 サインオフ前のため非表示 (active=0):
    //   gi, belt, BJJ tape, mouthguard case — Printful カタログに無い。
    type SweepRow = (
        &'static str,            // slug
        &'static str,            // category
        &'static str,            // name
        &'static str,            // description
        i64,                     // price_jpy
        &'static str,            // production_route
        Option<i64>,             // printful_product_id
        Option<i64>,             // printful_variant_id (M default; lookup uses map first)
        Option<&'static str>,    // printful_variant_map (JSON {S:id,...})
        Option<&'static str>,    // printful_files (JSON [{type,url},...])
        Option<&'static str>,    // printful_options (JSON [{id,value},...])
        i64,                     // lead_time_days
        i64,                     // active (0 = hidden)
    );
    // Pre-build JSON file/option blobs (avoid repeating long strings)
    let allover_files = format!(r#"[{{"type":"default","url":"{}"}}]"#, "https://lifestyle.wearmu.com/sweep/sweep-rashguard-ls.jpg");
    let _ = allover_files; // (each product references its own image below)

    let sweep_items: &[SweepRow] = &[
        // ── BJJ 専用品: Printful all-over print に置換 (今日 fulfillable) ──
        ("sweep-rashguard-ls",  "ラッシュガード",        "MU × SIIIEEP Long-Sleeve Rashguard",
         "Printful All-Over Print Men's Rash Guard (pid 301) ベース。全面プリントで MU の北海道気象データグラフィック + SIIIEEP サイドステッチ。圧縮ニット、UPF50+。",
         11_800, "printful", Some(301), Some(9328),
         Some(r#"{"S":9327,"M":9328,"L":9329,"XL":9330,"2XL":9331,"3XL":9332,"XS":9326}"#),
         Some(r#"[{"type":"default","url":"https://lifestyle.wearmu.com/sweep/sweep-rashguard-ls.jpg"}]"#),
         Some(allover_options), 14, 1),
        ("sweep-fight-shorts",  "ファイトショーツ",      "MU × SIIIEEP Athletic Long Shorts",
         "Printful All-Over Print Unisex Athletic Long Shorts (pid 332) ベース。MUON の温度パターンを全面プリント。ストレッチ + 内側ライナー。",
         9_800,  "printful", Some(332), Some(9813),
         Some(r#"{"S":9812,"M":9813,"L":9814,"XL":9815,"2XL":9816,"3XL":9817,"XS":9811,"2XS":16588}"#),
         Some(r#"[{"type":"default","url":"https://lifestyle.wearmu.com/sweep/sweep-fight-shorts.jpg"}]"#),
         Some(allover_options), 14, 1),
        ("sweep-spats",         "スパッツ / グラップリング タイツ",
         "MU × SIIIEEP Grappling Spats",
         "Printful All-Over Print Leggings (pid 189) ベース。MUGEN の連番が縦に流れるサイドライン入り。寒い日のアンダー / そのまま着用も可。",
         8_800,  "printful", Some(189), Some(7678),
         Some(r#"{"S":7677,"M":7678,"L":7679,"XL":7680,"XS":7676}"#),
         Some(r#"[{"type":"default","url":"https://lifestyle.wearmu.com/sweep/sweep-spats.jpg"}]"#),
         Some(allover_options), 14, 1),

        // ── 非表示 (SIIIEEP社 サインオフ前 — Printful カタログ外) ──
        ("sweep-gi-classic",    "柔術 Gi (道着)",        "MU × SIIIEEP Classic Gi",
         "綿100% 550gsm、SIIIEEP 標準カット。襟裏に MUGEN 連番を刺繍。SIIIEEP社 確認後に販売開始。",
         38_800, "pre_order", None, None, None, None, None, 56, 0),
        ("sweep-belt-promo",    "帯 (昇格用)",           "MU × SIIIEEP Promotion Belt",
         "白帯〜黒帯。先端に MU×SIIIEEP のラベル縫い込み。SIIIEEP社 確認後に販売開始。",
         6_800,  "pre_order", None, None, None, None, None, 21, 0),
        ("sweep-bjj-tape",      "BJJ フィンガーテープ",  "MU × SIIIEEP Finger Tape (3 rolls)",
         "10m × 3 ロール。ロール側面に MUGEN ロゴ。SIIIEEP社 確認後に販売開始。",
         2_400,  "sweep_manual", None, None, None, None, None, 14, 0),
        ("sweep-mouthguard",    "マウスガード ケース",   "MU × SIIIEEP Mouthguard Case",
         "アルマイトアルミ製、消臭穴、刻印 MU×SIIIEEP。SIIIEEP社 確認後に販売開始。",
         3_800,  "sweep_manual", None, None, None, None, None, 21, 0),

        // ── DTG 印刷系 (T / hoodie / longsleeve / tote) ──
        ("sweep-hoodie",        "ヘビーフーディ",          "MU × SIIIEEP Heavy Hoodie",
         "Printful Gildan 18500 (pid 146) ベース、ヘビーブレンド。胸に SIIIEEP wordmark を DTG プリント (Black/White)。",
         16_800, "printful", Some(146), Some(5531),
         Some(r#"{"S":5530,"M":5531,"L":5532,"XL":5533,"2XL":5534,"3XL":5535,"4XL":5536,"5XL":5537}"#),
         Some(r#"[{"type":"default","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         dtg_no_options, 14, 1),
        ("sweep-tee",           "コットン T",            "MU × SIIIEEP Heavy Cotton Tee",
         "Printful Bella+Canvas 3001 (pid 71) ベース、Black。胸に SIIIEEP wordmark を DTG プリント。",
         6_800,  "printful", Some(71), Some(4017),
         Some(r#"{"S":4016,"M":4017,"L":4018,"XL":4019,"2XL":4020,"XS":9527}"#),
         Some(r#"[{"type":"default","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         dtg_no_options, 10, 1),
        ("sweep-tee-classic",   "クラシック T",          "MU × SIIIEEP Classic Tee",
         "Printful Bella+Canvas 3001 (pid 71)、Black。胸に最小限の SIIIEEP wordmark のみ。",
         4_800,  "printful", Some(71), Some(4017),
         Some(r#"{"S":4016,"M":4017,"L":4018,"XL":4019,"2XL":4020,"XS":9527}"#),
         Some(r#"[{"type":"default","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         dtg_no_options, 10, 1),
        ("sweep-longsleeve",    "ロングスリーブ T",      "MU × SIIIEEP Long Sleeve Tee",
         "Printful Bella+Canvas 3501 (pid 356) ベース、Black。胸に SIIIEEP wordmark を DTG プリント。",
         7_800,  "printful", Some(356), Some(10095),
         Some(r#"{"S":10094,"M":10095,"L":10096,"XL":10097,"2XL":10098,"XS":10093}"#),
         Some(r#"[{"type":"default","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         dtg_no_options, 12, 1),
        ("sweep-tote",          "コットントート",        "MU × SIIIEEP Cotton Tote",
         "Printful AS Colour 1001 (pid 641) ベース、Black。前面に SIIIEEP wordmark DTG プリント。",
         7_800,  "printful", Some(641), Some(16287),
         Some(r#"{"OS":16287,"ONE SIZE":16287}"#),
         Some(r#"[{"type":"default","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         dtg_no_options, 10, 1),

        // ── Embroidery (cap / beanie / sweatpants / windbreaker / socks) ──
        ("sweep-sweatpants",    "スウェットパンツ",      "MU × SIIIEEP Garment-Dyed Sweatpants",
         "Printful Comfort Colors 1469 (pid 898)、Blue Jean。左大腿に SIIIEEP wordmark 刺繍 (白糸)。",
         12_800, "printful", Some(898), Some(22923),
         Some(r#"{"S":22916,"M":22923,"L":22930,"XL":22937,"2XL":22944}"#),
         Some(r#"[{"type":"embroidery_chest_left","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         Some(r##"[{"id":"thread_colors_chest_left","value":["#FFFFFF"]}]"##), 14, 1),
        ("sweep-cap",           "ダッドハット",          "MU × SIIIEEP Classic Dad Hat",
         "Printful Yupoong 6245CM (pid 206)、Black。フロントに SIIIEEP wordmark 刺繍 (白糸)。ワンサイズ。",
         5_800,  "printful", Some(206), Some(7854),
         Some(r#"{"OS":7854,"ONE SIZE":7854}"#),
         Some(r#"[{"type":"embroidery_front_large","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         Some(r##"[{"id":"thread_colors_front_large","value":["#FFFFFF"]}]"##), 10, 1),
        ("sweep-beanie",        "リブニットビーニー",    "MU × SIIIEEP Ribbed Knit Beanie",
         "Printful Atlantis Ribbed Knit Beanie (pid 519)、Black。フロントに SIIIEEP wordmark 刺繍 (白糸)。",
         5_800,  "printful", Some(519), Some(13238),
         Some(r#"{"OS":13238,"ONE SIZE":13238}"#),
         Some(r#"[{"type":"embroidery_front","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         Some(r##"[{"id":"thread_colors","value":["#FFFFFF"]}]"##), 10, 1),
        ("sweep-socks-3pack",   "刺繍クルーソックス",    "MU × SIIIEEP Embroidered Crew Socks (1 pair)",
         "Printful SOCCO SC200 (pid 502)、Black。1 ペア。カフ外側に SIIIEEP wordmark 刺繍 (白糸)。",
         5_800,  "printful", Some(502), Some(12674),
         Some(r#"{"S":12674,"M":12674,"S/M":12674,"L":12675,"XL":12675,"L/XL":12675}"#),
         Some(r#"[{"type":"embroidery_outside_left","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         Some(r##"[{"id":"thread_colors_outside_left","value":["#FFFFFF"]}]"##), 10, 1),
        ("sweep-windbreaker",   "ナイロン ウィンドブレーカー", "MU × SIIIEEP Basic Windbreaker",
         "Printful SOL'S 32000 (pid 661)、Black 撥水ナイロン。左胸に SIIIEEP wordmark 刺繍 (白糸)。",
         14_800, "printful", Some(661), Some(16425),
         Some(r#"{"S":16424,"M":16425,"L":16426,"XL":16427,"2XL":16428}"#),
         Some(r#"[{"type":"embroidery_chest_left","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         Some(r##"[{"id":"thread_colors_chest_left","value":["#FFFFFF"]}]"##), 14, 1),

        // ── 第二弾追加 (2026-05-11): DTG + 刺繍 + 全面プリント + 雑貨 ──
        ("sweep-tank-top",      "タンクトップ",          "MU × SIIIEEP Staple Tank Top",
         "Printful Bella+Canvas 3480 (pid 248)、Black。胸に SIIIEEP wordmark DTG プリント。ジムで快適。",
         5_800,  "printful", Some(248), Some(8630),
         Some(r#"{"S":8629,"M":8630,"L":8631,"XL":8632,"2XL":8633,"XS":8628}"#),
         Some(r#"[{"type":"default","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         None, 10, 1),
        ("sweep-zip-hoodie",    "ジップフーディ",        "MU × SIIIEEP Zip Hoodie",
         "Printful Gildan 18600 (pid 692)、Black。前胸に SIIIEEP wordmark DTG プリント。練習前後の温度調整に。",
         18_800, "printful", Some(692), Some(17296),
         Some(r#"{"S":17295,"M":17296,"L":17297,"XL":17298,"2XL":17299,"3XL":17300,"4XL":17301,"5XL":17302}"#),
         Some(r#"[{"type":"default","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         None, 14, 1),
        ("sweep-crewneck",      "クルーネック",          "MU × SIIIEEP Champion Crewneck",
         "Printful Champion S149 (pid 318)、Black。胸に SIIIEEP wordmark DTG プリント。厚手フリース。",
         13_800, "printful", Some(318), Some(9660),
         Some(r#"{"S":9659,"M":9660,"L":9661,"XL":9662,"2XL":9663}"#),
         Some(r#"[{"type":"default","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         None, 14, 1),
        ("sweep-snapback",      "スナップバック",        "MU × SIIIEEP Classic Snapback",
         "Printful Yupoong 6089M (pid 99)、Black フラットブリム。フロントに SIIIEEP wordmark 刺繍 (白糸)。",
         6_800,  "printful", Some(99), Some(4792),
         Some(r#"{"OS":4792,"ONE SIZE":4792}"#),
         Some(r#"[{"type":"embroidery_front_large","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         Some(r##"[{"id":"thread_colors_front_large","value":["#FFFFFF"]}]"##), 10, 1),
        ("sweep-mug",           "コーヒーマグ",          "MU × SIIIEEP Glossy Mug",
         "Printful Black Glossy Mug (pid 300)、11oz。両面に SIIIEEP wordmark サブリメーション印刷。",
         2_800,  "printful", Some(300), Some(9323),
         Some(r#"{"11 OZ":9323,"15 OZ":9324,"OS":9323,"ONE SIZE":9323,"M":9323,"L":9324}"#),
         Some(r#"[{"type":"default","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         None, 10, 1),
        ("sweep-bottle",        "ステンレスボトル",      "MU × SIIIEEP Stainless Bottle",
         "Printful Stainless Steel Water Bottle (pid 382)、17oz。側面に SIIIEEP wordmark サブリメーション印刷。",
         4_800,  "printful", Some(382), Some(16030),
         Some(r#"{"17 OZ":16030,"OS":16030,"ONE SIZE":16030,"M":16030}"#),
         Some(r#"[{"type":"default","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         None, 10, 1),
        ("sweep-stickers",      "ステッカーシート",      "MU × SIIIEEP Sticker Sheet",
         "Printful Kiss-Cut Sticker Sheet (pid 505)、5.83×8.27\"。複数の SIIIEEP wordmark / アイコンを kiss-cut。",
         1_200,  "printful", Some(505), Some(12917),
         Some(r#"{"OS":12917,"ONE SIZE":12917,"M":12917}"#),
         Some(r#"[{"type":"default","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         None, 7, 1),
        ("sweep-duffle",        "全面プリント ダッフル", "MU × SIIIEEP All-Over Duffle Bag",
         "Printful All-Over Print Duffle Bag (pid 465)。SIIIEEP のサインを全面に展開した、ジム/旅行兼用バッグ。",
         14_800, "printful", Some(465), Some(12021),
         Some(r#"{"OS":12021,"ONE SIZE":12021,"M":12021}"#),
         Some(r#"[{"type":"front","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         Some(r#"[{"id":"stitch_color","value":"black"}]"#), 14, 1),
        ("sweep-gym-bag",       "全面プリント ジム バッグ", "MU × SIIIEEP All-Over Gym Bag",
         "Printful All-Over Print Gym Bag (pid 594)。SIIIEEP パターンを全面プリント。Gi / 練習着収納に。",
         9_800,  "printful", Some(594), Some(15155),
         Some(r#"{"OS":15155,"ONE SIZE":15155,"M":15155}"#),
         Some(r#"[{"type":"front","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         Some(r#"[{"id":"stitch_color","value":"black"}]"#), 14, 1),
        ("sweep-cotton-shorts", "コットンショーツ",      "MU × SIIIEEP All-Over Cotton Shorts",
         "Printful All-Over Print Unisex Cotton Shorts (pid 1481)。全面プリント、ラフな普段着用。",
         6_800,  "printful", Some(1481), Some(46347),
         Some(r#"{"S":46346,"M":46347,"L":46348,"XL":46349,"2XL":46350,"3XL":46351,"XS":46345}"#),
         Some(r#"[{"type":"front_dtfabric","url":"https://lifestyle.wearmu.com/sweep/_logo.png"}]"#),
         Some(r#"[{"id":"stitch_color","value":"black"}]"#), 14, 1),
    ];
    let now = chrono_now();
    for (slug, cat, name, desc, price, route, pf_prod, pf_var, var_map, files, opts, lead, active) in sweep_items {
        conn.execute(
            "INSERT OR IGNORE INTO collab_products
                 (slug, partner, category, name, description, image_url, price_jpy,
                  sizes_json, active, draft, created_at,
                  printful_product_id, printful_variant_id, production_route,
                  lead_time_days, printful_variant_map,
                  printful_files, printful_options)
             VALUES (?, 'sweep', ?, ?, ?, NULL, ?,
                     '[\"XS\",\"S\",\"M\",\"L\",\"XL\"]', ?, 1, ?,
                     ?, ?, ?, ?, ?, ?, ?)",
            params![slug, cat, name, desc, price, active, now,
                    pf_prod, pf_var, route, lead, var_map, files, opts],
        ).ok();
        // For pre-existing rows (idempotent), sync every field that can change.
        conn.execute(
            "UPDATE collab_products
             SET production_route = ?, lead_time_days = ?,
                 printful_product_id = ?, printful_variant_id = ?,
                 printful_variant_map = ?,
                 printful_files = ?, printful_options = ?,
                 active = ?,
                 category = ?, name = ?, description = ?, price_jpy = ?
             WHERE slug = ?",
            params![route, lead, pf_prod, pf_var, var_map, files, opts, active,
                    cat, name, desc, price, slug],
        ).ok();
    }
    // Auto-blog posts table — every day the AI composes a "field log"
    // entry from /api/transparency, recent commits + cron health, and
    // it lands here. Rendered at /blog/auto/<slug>.
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS auto_blog_posts (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            slug        TEXT NOT NULL UNIQUE,
            title       TEXT NOT NULL,
            body_html   TEXT NOT NULL,
            body_md     TEXT,
            model       TEXT,
            stats_json  TEXT,
            published   INTEGER NOT NULL DEFAULT 1,
            created_at  TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_auto_blog_published ON auto_blog_posts(published, created_at DESC);
        -- blog_rate_limit: tracks /api/blog/stats_for_today fetches per IP per hour
        -- to prevent abuse + cost explosion (Gemini API key is published in
        -- the prompt field, so attacker could bypass our wrapper).
        CREATE TABLE IF NOT EXISTS blog_rate_limit (
            ip          TEXT NOT NULL,
            hour_bucket INTEGER NOT NULL,
            hits        INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY (ip, hour_bucket)
        );
        -- お客様 → AI フィードバック。MUer / MU Owner / MA Council でタグ。
        -- Gemini が即時返答、Telegram 通知、DB に永続記録。
        CREATE TABLE IF NOT EXISTS customer_feedback (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id       INTEGER,
            email         TEXT,
            message       TEXT NOT NULL,
            kind          TEXT NOT NULL DEFAULT 'request',
            is_lifetime   INTEGER NOT NULL DEFAULT 0,
            is_ma_council INTEGER NOT NULL DEFAULT 0,
            ai_reply      TEXT,
            ai_reply_at   TEXT,
            created_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_feedback_user ON customer_feedback(user_id);
        CREATE INDEX IF NOT EXISTS idx_feedback_council ON customer_feedback(is_ma_council, created_at DESC);
        -- MA Council weekly briefs: Gemini が customer_feedback (MA Council
        -- 投稿) を要約して N 件の議題を生成。MA owner だけが /api/council/vote
        -- で投票できる。集計は public で晒される。
        CREATE TABLE IF NOT EXISTS ma_council_briefs (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            slug         TEXT NOT NULL UNIQUE,
            week_start   TEXT NOT NULL,
            title        TEXT NOT NULL,
            body_md      TEXT NOT NULL,
            agendas_json TEXT NOT NULL,    -- [{id, q, options:[a,b,c]}, ...]
            model        TEXT,
            published    INTEGER NOT NULL DEFAULT 1,
            created_at   TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_council_briefs_pub ON ma_council_briefs(published, created_at DESC);
        CREATE TABLE IF NOT EXISTS ma_council_votes (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            brief_slug   TEXT NOT NULL,
            agenda_id    TEXT NOT NULL,
            voter_email  TEXT NOT NULL,
            choice       TEXT NOT NULL,
            created_at   TEXT NOT NULL,
            UNIQUE(brief_slug, agenda_id, voter_email)
        );
        CREATE INDEX IF NOT EXISTS idx_council_votes_brief ON ma_council_votes(brief_slug);
        -- MA Council members: auto-enrolled at bid time (tier='trial') and
        -- upgraded to tier='full' on auction win. Token is HMAC-SHA256 of the
        -- email + COUNCIL_TOKEN_SECRET env var, generated lazily.
        CREATE TABLE IF NOT EXISTS ma_council_members (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            email           TEXT NOT NULL UNIQUE,
            tier            TEXT NOT NULL DEFAULT 'trial'
                             CHECK (tier IN ('trial','full')),
            joined_at       TEXT NOT NULL,
            mu_piece_id     INTEGER,
            unsubscribed_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_council_members_tier
            ON ma_council_members(tier, joined_at DESC);
    ").ok();
    // Add option_index to ma_council_votes for the new HMAC-token flow
    // (older flow stored free-text `choice`). Both columns coexist.
    let _ = conn.execute(
        "ALTER TABLE ma_council_votes ADD COLUMN option_index INTEGER", []);
    // Add sent_at column for the weekly-brief cron to track delivery.
    let _ = conn.execute(
        "ALTER TABLE ma_council_briefs ADD COLUMN sent_at TEXT", []);
    // Audit columns for auto-blog: track which side (Fly compose / Actions
    // publish) produced the post, how many retries it took, and when we
    // notified subscribers / cross-posted to X.
    let _ = conn.execute("ALTER TABLE auto_blog_posts ADD COLUMN origin TEXT", []);
    let _ = conn.execute("ALTER TABLE auto_blog_posts ADD COLUMN retry_count INTEGER DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE auto_blog_posts ADD COLUMN digest_sent_at TEXT", []);
    let _ = conn.execute("ALTER TABLE auto_blog_posts ADD COLUMN cross_post_x_at TEXT", []);
    // One-shot migration: Gemini 2.5 Flash hallucinated the year in the
    // title (saw "2024 週11" on 2026-05-11 when week_label was "2026.W19").
    // Force-rebuild title from week_label for any existing brief whose title
    // doesn't already contain its own week_label. Idempotent.
    let _ = conn.execute(
        "UPDATE ma_council_briefs
            SET title = '今週の MA Council Brief — ' || week_start
            WHERE title NOT LIKE '%' || week_start || '%'",
        [],
    );
    // CV-pulse autonomous loop: every 30 min the cron POSTs to
    // /api/admin/cv_pulse, which writes a snapshot here + may update
    // cv_config (modal cooldown / coupon strength / email subject variant).
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS cv_pulses (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            at              TEXT NOT NULL,
            signups_30m     INTEGER DEFAULT 0,
            signups_24h     INTEGER DEFAULT 0,
            surveys_30m     INTEGER DEFAULT 0,
            surveys_24h     INTEGER DEFAULT 0,
            lottery_30m     INTEGER DEFAULT 0,
            lottery_24h     INTEGER DEFAULT 0,
            discounts_30m   INTEGER DEFAULT 0,
            discounts_24h   INTEGER DEFAULT 0,
            purchases_30m   INTEGER DEFAULT 0,
            purchases_24h   INTEGER DEFAULT 0,
            decision        TEXT,
            notes           TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_cv_pulses_at ON cv_pulses(at);

        CREATE TABLE IF NOT EXISTS cv_config (
            key         TEXT PRIMARY KEY,
            value       TEXT NOT NULL,
            updated_at  TEXT NOT NULL,
            reason      TEXT
        );
    ").ok();
    // Seed defaults if the cv_config table is empty
    conn.execute(
        "INSERT OR IGNORE INTO cv_config (key, value, updated_at, reason)
         VALUES (?, ?, ?, ?)",
        params!["modal_cooldown_hours", "24", chrono_now(), "default"],
    ).ok();
    conn.execute(
        "INSERT OR IGNORE INTO cv_config (key, value, updated_at, reason)
         VALUES (?, ?, ?, ?)",
        params!["coupon_percent_off", "50", chrono_now(), "default"],
    ).ok();
    conn.execute(
        "INSERT OR IGNORE INTO cv_config (key, value, updated_at, reason)
         VALUES (?, ?, ?, ?)",
        params!["email_subject_variant", "loss", chrono_now(), "default (loss-aversion)"],
    ).ok();
    conn.execute(
        "INSERT OR IGNORE INTO cv_config (key, value, updated_at, reason)
         VALUES (?, ?, ?, ?)",
        params!["modal_scroll_required", "1", chrono_now(), "default"],
    ).ok();
    // /you LP hero CTA variant. cv_pulse rotates: 'identity' / 'loss' / 'value'.
    conn.execute(
        "INSERT OR IGNORE INTO cv_config (key, value, updated_at, reason)
         VALUES (?, ?, ?, ?)",
        params!["hero_cta_variant", "value", chrono_now(), "default"],
    ).ok();
    // Monthly subscription price in JPY (¥). Editable from cv_config without
    // a redeploy. Bezos anchoring: ¥1,480 makes the ¥2,500 3-mo pack look
    // like a clear discount (¥1,480 × 3 = ¥4,440 vs ¥2,500 = 44% OFF).
    conn.execute(
        "INSERT OR IGNORE INTO cv_config (key, value, updated_at, reason)
         VALUES (?, ?, ?, ?)",
        params!["monthly_price_jpy", "1480", chrono_now(), "default"],
    ).ok();
    // Migrate prior default ¥980 → ¥1,480 (anchoring redesign).
    conn.execute(
        "UPDATE cv_config SET value='1480', updated_at=?, reason='anchor-rev-3'
         WHERE key='monthly_price_jpy' AND value='980' AND reason='default'",
        params![chrono_now()],
    ).ok();
    // 3-month prepaid pack (¥980 × 3 → 15% OFF = ¥2,500). One-time charge
    // that extends subscription_until by 90 days. Finite-duration option for
    // users uncomfortable with recurring billing.
    conn.execute(
        "INSERT OR IGNORE INTO cv_config (key, value, updated_at, reason)
         VALUES (?, ?, ?, ?)",
        params!["pack_3mo_price_jpy", "2500", chrono_now(), "default"],
    ).ok();
    // Migrate prior default ¥2,940 → ¥2,500 (15% OFF re-pricing). Only
    // touches rows we previously seeded as 'default'; operator-set values
    // are left alone.
    conn.execute(
        "UPDATE cv_config SET value='2500', updated_at=?, reason='default-rev-2'
         WHERE key='pack_3mo_price_jpy' AND value='2940' AND reason='default'",
        params![chrono_now()],
    ).ok();

    // Seed 15 サンプル ペルソナ (一度だけ、INSERT OR IGNORE on slug)。
    // /you ページ "他の人がもらっているデザイン" で表示される架空のキャラ。
    // 売れそうなクラスタを意図的に散らしてある (年代・地域・テイスト)。
    seed_sample_personas(&conn);

    // /you signal stream — emoji reactions + dwell time + email taps.
    // Drives the auto-tuning of compose_design so tomorrow's drop bends
    // toward "love" tokens and away from "meh" / "skip" tokens.
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS you_signals (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id     INTEGER NOT NULL,
            design_id   INTEGER NOT NULL,
            kind        TEXT NOT NULL,     -- love / ok / meh / skip / claim_intent / share / dwell
            weight      INTEGER NOT NULL DEFAULT 1,
            source      TEXT,              -- 'page' / 'email' / 'slug' / 'auto'
            created_at  TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_you_signals_user ON you_signals(user_id);
        CREATE INDEX IF NOT EXISTS idx_you_signals_design ON you_signals(design_id);
        CREATE INDEX IF NOT EXISTS idx_you_signals_kind ON you_signals(kind);
    ").ok();

    // Exit-intent funnel: survey → cost-price discount → no-purchase
    // open lottery (オープン懸賞 — Japan has no prize cap on these).
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS exit_surveys (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            email        TEXT,
            page         TEXT,
            why_left     TEXT,
            price_feel   TEXT,
            would_buy_at INTEGER,
            comment      TEXT,
            created_at   TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS exit_offers (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            email           TEXT NOT NULL,
            kind            TEXT NOT NULL,        -- 'discount_50' | 'lottery_entry'
            stripe_coupon   TEXT,                 -- Stripe coupon id once minted
            ticket_id       TEXT,                 -- lottery ticket UUID
            prize_jpy       INTEGER,              -- 0 if not yet drawn
            status          TEXT NOT NULL DEFAULT 'issued',
            expires_at      TEXT,
            used_at         TEXT,
            created_at      TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_exit_offers_email ON exit_offers(email);
        CREATE INDEX IF NOT EXISTS idx_exit_offers_ticket ON exit_offers(ticket_id);
        CREATE INDEX IF NOT EXISTS idx_exit_offers_status ON exit_offers(status);
    ").ok();
    // Per-Stripe-checkout purchase ledger so we can grant lifetime_free
    // retroactively when a returning buyer signs up for /you, AND so the
    // webhook can mark a /you account lifetime_free as soon as a purchase
    // settles.
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS mu_purchases (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            email       TEXT NOT NULL,
            product_id  INTEGER NOT NULL,
            brand       TEXT NOT NULL,
            drop_num    INTEGER,
            session_id  TEXT,
            created_at  TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_mu_purchases_email ON mu_purchases(email);
        CREATE INDEX IF NOT EXISTS idx_mu_purchases_session ON mu_purchases(session_id);
        CREATE INDEX IF NOT EXISTS idx_you_users_email ON you_users(email);
    ").ok();
    // Backfill trial_end_at for pre-existing /you users.
    // created_at is a unix-epoch-seconds string; trial_end_at is the same
    // format so we can compare without parsing each time.
    // 30 days = 2592000 seconds.
    conn.execute(
        "UPDATE you_users
         SET trial_end_at = CAST((CAST(created_at AS INTEGER) + 2592000) AS TEXT)
         WHERE trial_end_at IS NULL",
        [],
    ).ok();
    // Now that the slug column has been added (or was created on a fresh DB),
    // create the index on it. Doing this after the ALTER TABLE migrations
    // is what makes a redeploy onto an older DB safe.
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_you_users_slug ON you_users(slug)", []);
    let _ = conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_you_users_slug_unique \
         ON you_users(slug) WHERE slug IS NOT NULL", []);

    // Backfill: every existing you_user gets a random slug if missing
    {
        let mut stmt = conn.prepare("SELECT id FROM you_users WHERE slug IS NULL OR slug=''")
            .expect("prepare slug backfill");
        let ids: Vec<i64> = stmt.query_map([], |r| r.get::<_,i64>(0))
            .map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        for id in ids {
            let s = random_slug();
            // Try a few times in case of collision
            for _ in 0..5 {
                let r = conn.execute("UPDATE you_users SET slug=? WHERE id=?", params![s, id]);
                if r.is_ok() { break; }
            }
        }
    }
    // Backfill wallet_token for any pre-existing bid rows so old auctions can be settled
    {
        let mut stmt = conn.prepare("SELECT id FROM bids WHERE wallet_token IS NULL OR wallet_token=''")
            .expect("prepare bid backfill");
        let ids: Vec<i64> = stmt.query_map([], |r| r.get::<_,i64>(0))
            .map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        for id in ids {
            conn.execute(
                "UPDATE bids SET wallet_token=? WHERE id=?",
                params![uuid::Uuid::new_v4().to_string(), id]
            ).ok();
        }
    }
    // Ensure mockups dir exists for persisted images
    std::fs::create_dir_all(mockups_dir()).ok();
    let db: Db = Arc::new(Mutex::new(conn));

    // ── Phase 3.4 + 3.7: Pyth rate refresh + pending payment sweep ──
    payments::start_crons(db.clone());

    // ── Daily cron: JST 07:00, ensure today's design + send paced emails ──
    // Started before the router consumes `db` via with_state.
    let db_cron = db.clone();
    tokio::spawn(async move {
        loop {
            let sleep_secs = seconds_until_next_jst(7, 0);
            tracing::info!("[cron] you-daily: sleeping {}s until next JST 07:00", sleep_secs);
            tokio::time::sleep(std::time::Duration::from_secs(sleep_secs)).await;
            run_you_daily_cron(db_cron.clone()).await;
            // Belt-and-braces: avoid double-fire within the same minute.
            tokio::time::sleep(std::time::Duration::from_secs(120)).await;
        }
    });

    // ── Weekly cron: every Sunday 18:00 JST, generate + email the MA Council
    //    brief. Idempotent on iso_week_start_jst() — safe to redeploy across
    //    the firing window. The run-loop fires on Sun 18:00 and otherwise
    //    sleeps for ~1h between checks so a missed Sunday (e.g. Fly app
    //    asleep) still catches up on the next wake.
    let db_council = db.clone();
    tokio::spawn(async move {
        loop {
            let sleep_secs = seconds_until_next_jst_weekly_sunday(18, 0);
            tracing::info!("[cron] council-weekly: sleeping {}s until next JST Sunday 18:00",
                sleep_secs);
            tokio::time::sleep(std::time::Duration::from_secs(sleep_secs)).await;
            run_council_weekly_cron(db_council.clone()).await;
            tokio::time::sleep(std::time::Duration::from_secs(120)).await;
        }
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/success", get(success_page))
        // Brand SPA routes
        .route("/ma", get(index))
        .route("/muon", get(index))
        .route("/mugen", get(index))
        .route("/nouns", get(index))
        // Product detail SPA routes
        .route("/products/:brand/:id", get(index))
        // API routes
        .route("/api/products", get(list_brands))
        .route("/api/products/:brand", get(list_products))
        .route("/api/products/item/:id", get(get_product))
        .route("/api/weather", get(weather_handler))
        .route("/api/bid", post(place_bid))
        .route("/api/checkout", post(checkout))
        .route("/api/checkout/crypto", post(payments::checkout_crypto))
        .route("/api/checkout/crypto/status/:reference", get(payments::checkout_crypto_status))
        .route("/api/rates", get(payments::rates_handler))
        .route("/api/payment_methods", get(payments::payment_methods_handler))
        .route("/health", get(payments::health_handler))
        .route("/api/webhook/stripe", post(stripe_webhook))
        .route("/api/webhook/helius", post(payments::helius_webhook))
        .route("/api/webhook/alchemy", post(payments::alchemy_webhook))
        .route("/api/webhook/stripe-identity", post(payments::stripe_identity_webhook))
        .route("/api/kyc/identity-session", post(payments::create_stripe_identity_session))
        .route("/api/admin/exports/kyc.csv", get(payments::admin_export_kyc))
        .route("/api/admin/exports/crypto.csv", get(payments::admin_export_crypto))
        .route("/api/admin/import", post(import_product))
        .route("/api/admin/update-price", post(update_price))
        .route("/api/admin/update-nft", post(update_nft))
        .route("/api/admin/update-design", post(update_design))
        .route("/api/admin/update-sold", post(update_sold))
        .route("/api/admin/mockup", patch(update_mockup))
        .route("/api/admin/upload-mockup", post(upload_mockup))
        .route("/api/admin/recover_mugen", post(admin_recover_mugen))
        .route("/api/admin/lookup", get(admin_lookup))
        .route("/api/admin/deactivate", post(deactivate_product))
        .route("/api/admin/settle-auction", post(settle_auction))
        .route("/wallet/:token", get(wallet_page))
        .route("/api/wallet/:token", post(update_wallet))
        .route("/api/nft/:brand/:drop_num", get(nft_metadata))
        .route("/api/fragment", post(fragment_request))
        .route("/v/:brand/:drop_num", get(verify_page))
        .route("/tokushoho", get(tokushoho_page))
        .route("/tokushoho.html", get(tokushoho_page))
        .route("/city", get(city_page))
        .route("/city.html", get(city_page))
        // MU × YOU collab
        .route("/you", get(you_page))
        .route("/you.html", get(you_page))
        .route("/api/you/subscribe", post(you_subscribe))
        .route("/api/you/daily/:token", get(you_daily))
        .route("/api/you/feedback", post(you_feedback))
        .route("/api/you/claim", post(you_claim))
        .route("/api/you/unsubscribe", post(you_unsubscribe))
        .route("/api/you/design/:id/image.png", get(you_image))
        .route("/api/you/design/:id/image", get(you_image))
        .route("/api/you/slug", post(you_slug_set))
        .route("/api/you/slug/check/:slug", get(you_slug_check))
        .route("/api/you/taste", post(you_taste_update))
        .route("/api/you/admin/backfill_today", post(you_admin_backfill))
        .route("/api/you/admin/email_today", post(you_admin_email_today))
        .route("/api/you/admin/list", get(you_admin_list))
        .route("/api/you/style", post(you_style_set))
        .route("/api/you/stats", get(you_active_count))
        .route("/api/you/buy/:design_id", post(you_public_buy))
        // Exit-intent funnel
        .route("/api/exit/survey", post(exit_survey))
        .route("/api/exit/discount", post(exit_discount_claim))
        .route("/api/exit/lottery", post(exit_lottery_enter))
        .route("/api/admin/lottery_draw", post(admin_lottery_draw))
        .route("/api/admin/cv_pulse", post(cv_pulse))
        .route("/api/health/cron", get(cron_health_handler))
        .route("/api/transparency", get(public_transparency))
        .route("/api/sample_personas", get(list_sample_personas))
        .route("/api/admin/sample_grow", post(admin_sample_grow))
        .route("/api/admin/lifestyle", axum::routing::patch(admin_lifestyle))
        .route("/api/admin/collab_image", axum::routing::patch(admin_collab_image))
        .route("/api/admin/blog_compose", post(admin_blog_compose))
        .route("/api/blog/stats_for_today", get(blog_stats_for_today))
        .route("/api/admin/blog_publish", post(admin_blog_publish))
        .route("/api/blog/auto", get(list_auto_blog))
        .route("/blog/auto/:slug", get(show_auto_blog))
        .route("/api/you/referral", post(you_referral_status))
        .route("/api/feedback", post(submit_feedback))
        .route("/api/admin/thank_buyers", post(admin_thank_buyers))
        .route("/api/treasury", get(public_treasury))
        .route("/api/admin/x_queue", get(admin_x_queue))
        .route("/api/admin/x_mark_posted", post(admin_x_mark_posted))
        .route("/sweep", get(show_sweep_page))
        .route("/api/sweep/checkout", post(sweep_checkout))
        .route("/api/sweep/signal", post(sweep_signal))
        .route("/api/sweep/signals", get(sweep_signals_summary))
        .route("/api/admin/sweep_signals", get(admin_sweep_signals))
        .route("/api/admin/council_compose", post(admin_council_compose))
        .route("/api/council/briefs", get(list_council_briefs))
        .route("/api/council/vote", post(council_vote))
        // MA Council v2 (HMAC-token flow — 2026.07 roadmap)
        .route("/council", get(council_page))
        .route("/council.html", get(council_page))
        .route("/api/council/me", get(council_me))
        .route("/api/council/agenda", get(council_agenda))
        .route("/api/council/vote_token", post(council_vote_token))
        .route("/api/council/results/:brief_id", get(council_results))
        .route("/api/cv/config", get(cv_public_config))
        .route("/api/you/signal/:design_id", post(you_signal))
        .route("/api/you/preferences", get(you_preferences))
        .route("/r/:design_id/:kind", get(you_signal_email))
        .route("/api/admin/backfill_purchases", post(admin_backfill_purchases))
        .route("/api/you/subscribe-paid", post(you_subscribe_paid))
        .route("/api/you/subscribe-3mo", post(you_subscribe_3mo))
        .route("/api/you/portal", post(you_portal))
        // Blog (public ops notes). Clean URLs without .html extension.
        .route("/blog", get(blog_index))
        .route("/blog/", get(blog_index))
        .route("/blog/from-automation-to-autonomy", get(blog_post_001))
        .route("/sitemap.xml", get(dynamic_sitemap))
        // Per-user share page — REGISTER LAST so literal routes win
        .route("/:slug", get(slug_or_static))
        .nest_service("/static", ServeDir::new("static"))
        .nest_service("/mockups", ServeDir::new(mockups_dir()))
        .fallback_service(ServeDir::new("static"));
    let watcher_db = db.clone();
    let app = app
        .with_state(db)
        .layer(middleware::from_fn(security_headers))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        );

    // Background self-heal watcher — runs hourly inside the Fly app itself,
    // independent of m5 cron. Detects stale brands (MUGEN > 2h, MUON > 30h,
    // MA > 35d) and pings Telegram CRITICAL. De-dup: 1 alert per brand per
    // 24h to avoid alarm fatigue. (watcher_db cloned before with_state above.)
    tokio::spawn(async move {
        // Wait 60s on boot so deploy-time cron lag doesn't trigger.
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        interval.tick().await; // skip the first immediate tick
        loop {
            interval.tick().await;
            let warnings: Vec<String> = {
                let conn = watcher_db.lock().unwrap();
                cron_health_warnings(&conn)
            };
            if warnings.is_empty() { continue; }
            // De-dup by suppressing alerts when we've alerted on the same set
            // in the last 24h. Use cv_config as a tiny K-V store: key=last_cron_alert
            let now_s: i64 = chrono_now().parse().unwrap_or(0);
            let suppress = {
                let conn = watcher_db.lock().unwrap();
                let last: i64 = cv_get(&conn, "last_cron_alert", "0").parse().unwrap_or(0);
                now_s - last < 24 * 3600
            };
            if suppress { continue; }
            // Send Telegram CRITICAL
            let tg_token = env::var("TELEGRAM_BOT_TOKEN").unwrap_or_default();
            let tg_chat  = env::var("TELEGRAM_CHAT_ID").unwrap_or_else(|_| "1136442501".into());
            if tg_token.is_empty() { continue; }
            let msg = format!(
                "🚨 CRITICAL — MU cron self-heal watcher\n\
                 m5 cron が止まっている可能性。Fly 側 watcher が検知:\n{}\n\n\
                 → m5 Mac の crontab 確認 / `bash cron.sh install` 再実行",
                warnings.join("\n")
            );
            let _ = reqwest::Client::new()
                .post(format!("https://api.telegram.org/bot{}/sendMessage", tg_token))
                .json(&serde_json::json!({"chat_id": tg_chat, "text": msg, "disable_web_page_preview": true}))
                .send().await;
            // Mark suppression timestamp
            {
                let conn = watcher_db.lock().unwrap();
                cv_set(&conn, "last_cron_alert", &now_s.to_string(), "self-heal");
            }
        }
    });

    let port = env::var("PORT").unwrap_or_else(|_| "3000".into());
    let addr = format!("0.0.0.0:{}", port);
    println!("mu-store listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// Number of seconds from now until the next JST (hh:mm). Always positive.
fn seconds_until_next_jst(target_h: u32, target_m: u32) -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let now_jst = now_secs + 9 * 3600;
    let day = now_jst / 86400;
    let sec_of_day = now_jst - day * 86400;
    let target_sec = (target_h as i64) * 3600 + (target_m as i64) * 60;
    let mut delta = target_sec - sec_of_day;
    if delta <= 0 { delta += 86400; }
    delta as u64
}

/// Number of seconds from now until the next JST Sunday at (hh:mm). Always
/// positive. 1970-01-01 = Thursday → days_since_epoch % 7 == 3 is Sunday.
fn seconds_until_next_jst_weekly_sunday(target_h: u32, target_m: u32) -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let now_jst = now_secs + 9 * 3600;
    let day = now_jst / 86400;
    let sec_of_day = now_jst - day * 86400;
    // 1970-01-01 was Thursday; +3 mod 7 → 0=Mon, 6=Sun
    let dow = (day + 3).rem_euclid(7);
    let sun_offset = (6 - dow + 7) % 7; // days until next Sunday (0 if today)
    let target_sec = (target_h as i64) * 3600 + (target_m as i64) * 60;
    let mut delta = sun_offset * 86400 + target_sec - sec_of_day;
    if delta <= 0 { delta += 7 * 86400; }
    delta as u64
}

/// Body of the daily-email cron. Idempotent + safe to run more than once
/// per day (won't regenerate designs that are already ready; won't double-
/// send emails for the same day per user because of the cron_last_sent
/// column).
async fn run_you_daily_cron(db: Db) {
    let day = jst_today_str();
    tracing::info!("[cron] you-daily: starting for day={}", day);

    // 1. Ensure today's design exists for every active subscriber, kick off
    //    Gemini for the ones that don't have one yet.
    let pending: Vec<(i64, String)> = {
        let conn = db.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT u.id, u.taste_json FROM you_users u
             WHERE u.unsubscribed_at IS NULL
               AND you_user_active_sql(u.trial_end_at, COALESCE(u.lifetime_free,0))"
        ) {
            // Fallback to plain WHERE if the helper function isn't installed
            // — SQLite doesn't have user-defined functions registered here, so
            // this is the actual query we run:
            Err(_) => match conn.prepare(
                "SELECT u.id, u.taste_json FROM you_users u
                 WHERE u.unsubscribed_at IS NULL"
            ) {
                Ok(s) => s,
                Err(e) => { tracing::error!("[cron] db prepare: {}", e); return; }
            },
            Ok(s) => s,
        };
        stmt.query_map([], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,String>(1)?)))
            .map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    };

    let mut ensured = 0;
    for (uid, taste_json) in &pending {
        let taste: serde_json::Value =
            serde_json::from_str(taste_json).unwrap_or(serde_json::json!({}));
        let (did, needs) = {
            let conn = db.lock().unwrap();
            ensure_design_for_day(&conn, *uid, &day, &taste, false).unwrap_or((0, false))
        };
        if did > 0 && needs {
            spawn_gemini_for_design(db.clone(), did);
            ensured += 1;
            // Stagger Gemini calls so we don't blast 50 requests in parallel
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
    tracing::info!("[cron] you-daily: queued {} Gemini gen calls", ensured);

    // 2. Wait for Gemini calls to settle (~90s avg per design, but they
    //    run in parallel; 3 minutes is generous).
    tokio::time::sleep(std::time::Duration::from_secs(180)).await;

    // 3. Send paced emails to everyone whose today's design is now ready
    //    AND we haven't yet emailed for this day (tracked by daily_email_sent).
    let send_targets: Vec<(i64, String, i64, String, String, Option<String>, String)> = {
        let conn = db.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT d.id, u.email, d.day_num, d.name, d.prompt, u.slug, u.token
             FROM you_designs d JOIN you_users u ON u.id = d.user_id
             WHERE d.day=? AND d.gen_status='ready'
               AND u.unsubscribed_at IS NULL
               AND length(coalesce(u.email,''))>3
               AND COALESCE(d.daily_email_sent_at,'')=''"
        ) {
            Ok(s) => s,
            Err(_) => match conn.prepare(
                "SELECT d.id, u.email, d.day_num, d.name, d.prompt, u.slug, u.token
                 FROM you_designs d JOIN you_users u ON u.id = d.user_id
                 WHERE d.day=? AND d.gen_status='ready'
                   AND u.unsubscribed_at IS NULL
                   AND length(coalesce(u.email,''))>3"
            ) {
                Ok(s) => s,
                Err(e) => { tracing::error!("[cron] email prepare: {}", e); return; }
            },
        };
        stmt.query_map(params![day], |r| Ok((
            r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?,
        ))).map(|it| it.filter_map(|r| r.ok()).collect())
           .unwrap_or_default()
    };

    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    let base = base_url.trim_end_matches('/').to_string();
    if resend_key.is_empty() {
        tracing::warn!("[cron] RESEND_API_KEY not set — skipping email phase");
        return;
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build().unwrap_or_default();

    let mut sent = 0;
    let mut failed = 0;
    let subj_variant = you_subject_variant(&db);
    for (design_id, email, day_num, name, prompt, slug, token) in &send_targets {
        let img_url = format!("{}/api/you/design/{}/image.png", base, design_id);
        let share = slug.as_ref()
            .map(|s| format!("{}/{}", base, s))
            .unwrap_or_else(|| format!("{}/you", base));
        let reactions = email_reaction_row(*design_id, token);
        let html = format!(r#"<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:32px 0;margin:0"><div style="max-width:600px;margin:0 auto;padding:0 32px"><div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:32px">MU × YOU</div><div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#e6c449;opacity:0.85;margin-bottom:8px">DAY {day_num:03}</div><div style="font-size:24px;font-weight:200;line-height:1.4;margin-bottom:8px">{name}</div><p style="font-size:12px;line-height:1.85;opacity:0.7;margin-bottom:24px;font-style:italic;border-left:2px solid #e6c449;padding-left:14px">{prompt}</p><img src="{img}" alt="MU × YOU DAY {day_num}" style="width:100%;display:block;background:#1a1a1a;border-radius:2px;margin-bottom:24px"><a href="{share}" style="display:inline-block;background:#e6c449;color:#000;padding:16px 28px;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;text-decoration:none;font-weight:700;margin-right:8px">この一着を仕立てる →</a><a href="{share}" style="display:inline-block;border:1px solid rgba(255,255,255,0.2);color:#F5F5F0;padding:15px 22px;font-size:10px;letter-spacing:0.25em;text-transform:uppercase;text-decoration:none;opacity:0.7">明日に期待 / Skip</a>{reactions}<p style="font-size:10px;opacity:0.45;margin-top:32px;line-height:1.7">気分が変わったら <a href="{share}" style="color:#e6c449">プロンプトを書き直す</a>こともできます。<br>退会は <code>STOP</code> 返信、または /you ページから即時。</p></div></div>"#,
            day_num = day_num, name = name, prompt = prompt, img = img_url, share = share, reactions = reactions);
        let resp = client
            .post("https://api.resend.com/emails")
            .bearer_auth(&resend_key)
            .json(&serde_json::json!({
                "from": "MU × YOU <noreply@wearmu.com>",
                "to": [email],
                "subject": you_email_subject(&subj_variant, "daily",
                    &serde_json::json!({"day_num": *day_num, "name": name})),
                "html": html,
            }))
            .send().await;
        match resp {
            Ok(r) if r.status().is_success() => {
                tracing::info!("[cron] sent design {} → {}", design_id, email);
                sent += 1;
                let conn = db.lock().unwrap();
                let _ = conn.execute(
                    "UPDATE you_designs SET daily_email_sent_at=? WHERE id=?",
                    params![chrono_now(), design_id],
                );
            }
            Ok(r) => {
                let s = r.status();
                let txt = r.text().await.unwrap_or_default();
                tracing::warn!("[cron] FAIL design {} → {}: {} {}",
                    design_id, email, s, &txt[..txt.len().min(200)]);
                failed += 1;
            }
            Err(e) => {
                tracing::warn!("[cron] NET FAIL design {} → {}: {}", design_id, email, e);
                failed += 1;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    }
    tracing::info!("[cron] you-daily: done day={} sent={} failed={}", day, sent, failed);
}
