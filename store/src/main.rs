mod gemini;

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
}

#[derive(Deserialize)]
struct CheckoutBody {
    product_id: i64,
    quantity: u32,
    email: String,
    size: Option<String>,
    wallet: Option<String>,
}

#[derive(Deserialize)]
struct BidBody {
    product_id: i64,
    amount: i64,
    email: String,
    wallet: Option<String>,
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
/// Special cases: MA = ¥120,000, MUGEN #108 = ¥30,000 fixed.
fn dynamic_price(brand: &str, drop_num: i64, sold: i64, name: &str) -> i64 {
    if brand == "ma" {
        return 120_000;
    }
    if brand == "nouns" {
        let nm = name.to_uppercase();
        if nm.contains("間") || nm.contains(" MA ") || nm.starts_with("MA ") || nm.ends_with(" MA") {
            return 120_000;
        }
    }
    if brand == "mugen" && drop_num == 108 {
        return 30_000;
    }
    let base: i64 = 5_000;
    let step: i64 = 250;
    let max:  i64 = 30_000;
    (base + sold.max(0) * step).min(max)
}

fn read_product(row: &rusqlite::Row) -> rusqlite::Result<Product> {
    let brand:    String = row.get(1)?;
    let drop_num: i64    = row.get(2)?;
    let name:     String = row.get(3)?;
    let sold:     i64    = row.get(7)?;
    let dynamic = dynamic_price(&brand, drop_num, sold, &name);
    Ok(Product {
        id:           row.get(0)?,
        brand,
        drop_num,
        name,
        mockup_url:   row.get(4)?,
        price_jpy:    dynamic,
        inventory:    row.get(6)?,
        sold,
        created_at:   row.get(8)?,
        weather_data: row.get(9)?,
        prompt_hash:  row.get(10)?,
        seed_data:    row.get(11)?,
        nft_mint:     row.get(12)?,
        auction_end:  row.get(13)?,
        current_bid:  row.get(14).unwrap_or(0),
        bid_count:    row.get(15).unwrap_or(0),
        sold_out_at:  row.get(16).unwrap_or(None),
    })
}

async fn list_products(
    Path(brand): Path<String>,
    State(db): State<Db>,
) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, brand, drop_num, name, mockup_url, price_jpy, inventory, sold, created_at,
                weather_data, prompt_hash, seed_data, nft_mint, auction_end,
                COALESCE(current_bid,0), COALESCE(bid_count,0), sold_out_at
         FROM products WHERE brand=? AND active=1 ORDER BY created_at DESC LIMIT 24"
    ).unwrap();
    let products: Vec<Product> = stmt.query_map(params![brand], |row| read_product(row))
        .unwrap().filter_map(|r| r.ok()).collect();
    Json(products)
}

async fn list_brands(State(db): State<Db>) -> impl IntoResponse {
    let counts: Vec<(String, i64, String)> = {
        let conn = db.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT brand, COUNT(*) AS active_count, MAX(created_at) AS latest
             FROM products WHERE active=1 GROUP BY brand ORDER BY brand"
        ).unwrap();
        stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
        }).unwrap().filter_map(|r| r.ok()).collect()
    };

    let brands_json: Vec<serde_json::Value> = counts.into_iter().map(|(b, c, latest)| {
        let (description, cycle) = match b.as_str() {
            "mugen" => ("108 pieces per hour, weather-driven design", "hourly"),
            "muon"  => ("daily drop, quantity from temperature", "daily"),
            "ma"    => ("monthly auction, single piece", "monthly"),
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
        "SELECT id, brand, drop_num, name, mockup_url, price_jpy, inventory, sold, created_at,
                weather_data, prompt_hash, seed_data, nft_mint, auction_end,
                COALESCE(current_bid,0), COALESCE(bid_count,0), sold_out_at
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
    let now = chrono_now();
    let wallet_token = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO bids (product_id, amount, email, wallet, wallet_token, created_at) VALUES (?,?,?,?,?,?)",
        params![body.product_id, body.amount, body.email, body.wallet, wallet_token, now]
    ).unwrap();
    conn.execute(
        "UPDATE products SET current_bid=?, bid_count=bid_count+1 WHERE id=?",
        params![body.amount, body.product_id]
    ).unwrap();
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    Json(serde_json::json!({
        "ok": true,
        "wallet_token": wallet_token,
        "wallet_url": format!("{}/wallet/{}", base_url, wallet_token),
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
    let price_jpy = dynamic_price(&brand_str, drop_num, sold, &product_name);

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
            ("metadata[product_id]",   &body.product_id.to_string()),
            ("metadata[size]",         &size_label),
            ("metadata[wallet]",       &wallet),
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
    if event["type"] == "checkout.session.completed" {
        let session = &event["data"]["object"];
        let meta = session["metadata"].clone();
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
                            "from": "MU <noreply@enablerdao.com>",
                            "to": [&buyer_email2],
                            "subject": "あなたがこのドロップを閉じた — MU LAST PIECE",
                            "html": html,
                        }))
                        .send().await.ok();
                });
            }
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
            "SELECT b.id, b.amount, b.email, b.wallet, b.wallet_token, p.name, p.price_jpy FROM bids b
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
            ))
        )
    };
    let (bid_id, amount, email, current_wallet, wallet_token_opt, product_name, _base_price) = match bid {
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
    );

    client.post("https://api.resend.com/emails")
        .bearer_auth(&resend_key)
        .json(&serde_json::json!({
            "from": "MU <noreply@enablerdao.com>",
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
/// Active when:
///   - lifetime_free is set (purchased a MU shirt → forever), OR
///   - trial_end_at is in the future
fn you_user_active(trial_end_at: Option<&str>, lifetime_free: bool) -> bool {
    if lifetime_free { return true; }
    let trial_end: u64 = match trial_end_at.and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => return false,
    };
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    now < trial_end
}

/// Subscription state shown to the client (and stamped on emails).
fn you_user_state(trial_end_at: Option<&str>, lifetime_free: bool) -> serde_json::Value {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let trial_end: u64 = trial_end_at.and_then(|s| s.parse().ok()).unwrap_or(0);
    let days_left: i64 = if lifetime_free {
        -1   // sentinel: ∞
    } else if trial_end > now {
        ((trial_end - now) / 86400) as i64
    } else {
        0
    };
    let status = if lifetime_free {
        "lifetime"
    } else if trial_end > now {
        "trial"
    } else {
        "expired"
    };
    serde_json::json!({
        "status": status,
        "trial_end_at": trial_end_at,
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
            "from": "MU <noreply@enablerdao.com>",
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
            "from": "MU Fragment <noreply@enablerdao.com>",
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

    let prompt = format!(
        "{date}・{mood}な{noun}を、{pal}の階調で。{sc}に着られる、身体の延長としてのコットンTシャツ。\
         胸ポケット位置に小さなモチーフ、背中に余白。10oz Bella+Canvas、DTG。",
        date = day, mood = m1, noun = noun, pal = pal, sc = sc,
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

    // Merge the user's style_name (set on Day 7) into the taste so milestone
    // prompts can reference it.
    let mut taste_with_style = taste.clone();
    if let Some(obj) = taste_with_style.as_object_mut() {
        let style_name: Option<String> = conn.query_row(
            "SELECT style_name FROM you_users WHERE id=?",
            params![user_id], |r| r.get(0),
        ).ok().flatten();
        if let Some(sn) = style_name {
            obj.insert("style_name".into(), serde_json::Value::String(sn));
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
fn spawn_gemini_for_design(db: Db, design_id: i64) {
    tokio::spawn(async move {
        let row = {
            let conn = db.lock().unwrap();
            conn.query_row(
                "SELECT d.name, d.prompt, d.seed, d.day_num, u.taste_json,
                        u.email, u.slug
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
                )),
            ).ok()
        };
        let (name, prompt, seed, day_num, taste_json, email, slug) = match row {
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
    <p style="font-size:10px;opacity:0.45;margin-top:32px;line-height:1.7">
      気分が変わったら <a href="{share}" style="color:#e6c449">プロンプトを書き直す</a>こともできます。<br>
      退会は <code>STOP</code> 返信、または /you ページから即時。
    </p>
  </div>
</div>"#,
                        day_num = day_num, name = name, prompt = prompt,
                        img = img_url, share = share);
                    let _ = reqwest::Client::new()
                        .post("https://api.resend.com/emails")
                        .bearer_auth(&resend_key)
                        .json(&serde_json::json!({
                            "from": "MU × YOU <noreply@enablerdao.com>",
                            "to": [email],
                            "subject": format!("MU × YOU DAY {:03} — {}", day_num, name),
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
                    "from": "MU × YOU <noreply@enablerdao.com>",
                    "to": [to],
                    "subject": "MU × YOU — 明朝9時から毎日デザインが届きます",
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

    let (trial_end_at, lifetime_free): (Option<String>, i64) = conn.query_row(
        "SELECT trial_end_at, COALESCE(lifetime_free,0) FROM you_users WHERE id=?",
        params![user_id], |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap_or((None, 0));
    let subscription = you_user_state(trial_end_at.as_deref(), lifetime_free != 0);
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
                    "from": "MU × YOU <noreply@enablerdao.com>",
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
                "from": "MU × YOU <noreply@enablerdao.com>",
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
  <a href="https://wearmu.com/ma" style="display:inline-block;border:1px solid rgba(255,255,255,0.2);color:#F5F5F0;padding:13px 22px;font-size:10px;letter-spacing:0.25em;text-transform:uppercase;text-decoration:none;opacity:0.8">月次 MA オークション</a>
  <p style="font-size:10px;opacity:0.5;margin-top:32px;line-height:1.7">
    トライアル終了後は、購入が無い限り毎日のデザイン配信は停止します。<br>退会は <code>STOP</code> 返信で即時。
  </p>
</div>"#, days = days_left.max(1), days_done = (30 - days_left as i64).max(0));
        let _ = reqwest::Client::new()
            .post("https://api.resend.com/emails")
            .bearer_auth(&resend_key)
            .json(&serde_json::json!({
                "from": "MU × YOU <noreply@enablerdao.com>",
                "to": [email],
                "subject": format!("MU × YOU — トライアル残り {} 日", days_left.max(1)),
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
    tokio::spawn(async move {
        let html = r#"
<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:48px;max-width:560px;margin:0 auto">
  <div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:32px">MU × YOU</div>
  <div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#e6c449;opacity:0.85;margin-bottom:8px">Trial Ended</div>
  <div style="font-size:18px;font-weight:300;line-height:1.5;margin-bottom:24px">30 日間のトライアル、ここまで届けてくれてありがとう。</div>
  <p style="font-size:12px;line-height:1.85;opacity:0.75;margin-bottom:24px">
    今日からは、毎朝 9 時のデザイン配信は一旦停止します。<br><br>
    <strong>もう一度 ON にする方法はひとつだけ</strong> — MU の T シャツを 1 着、手に入れてください。<br>
    1 着でも所有すれば、MU × YOU は <strong>一生無料</strong>。明日からまた毎朝、あなただけの一着が届きます。
  </p>
  <a href="https://wearmu.com/mugen" style="display:inline-block;background:#e6c449;color:#000;padding:14px 28px;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;text-decoration:none;font-weight:700;margin-right:8px">MUGEN を見る →</a>
  <a href="https://wearmu.com/ma" style="display:inline-block;border:1px solid rgba(255,255,255,0.2);color:#F5F5F0;padding:13px 22px;font-size:10px;letter-spacing:0.25em;text-transform:uppercase;text-decoration:none;opacity:0.8">MA オークション</a>
  <p style="font-size:10px;opacity:0.5;margin-top:32px;line-height:1.7">
    トライアル中の 30 案は <a href="https://wearmu.com/you" style="color:#e6c449">あなたのページ</a> でいつでも見返せます。
  </p>
</div>"#;
        let _ = reqwest::Client::new()
            .post("https://api.resend.com/emails")
            .bearer_auth(&resend_key)
            .json(&serde_json::json!({
                "from": "MU × YOU <noreply@enablerdao.com>",
                "to": [email],
                "subject": "MU × YOU — トライアル終了。続けるには MU を 1 着。",
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
    type UserRow = (i64, String, String, Option<String>, Option<String>, i64, String, Option<String>);
    let users: Vec<UserRow> = {
        let conn = db.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, email, taste_json, slug, trial_end_at, COALESCE(lifetime_free,0),
                    created_at, style_name
             FROM you_users
             WHERE unsubscribed_at IS NULL"
        ) { Ok(s) => s, Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db").into_response() };
        stmt.query_map([], |r| Ok((
            r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?,
        ))).map(|it| it.filter_map(|r| r.ok()).collect())
           .unwrap_or_default()
    };

    let mut queued = 0;
    let mut skipped_expired = 0;
    for (uid, email, taste_json, _slug, trial_end_at, lifetime_free_int, created_at, style_name) in &users {
        let lifetime_free = *lifetime_free_int != 0;
        // Skip expired trials (no daily email until they buy a MU shirt).
        if !you_user_active(trial_end_at.as_deref(), lifetime_free) {
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
    "about", "press", "robots.txt", "sitemap.xml", "manifest.json",
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

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
        .replace('"', "&quot;").replace('\'', "&#39;")
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

    // Cards markup
    let cards: String = designs.iter().map(|d| {
        let (id, day, day_num, name, prompt, status, gen_status, _img_url) = d;
        let img_src = format!("/api/you/design/{}/image.png", id);
        let label = if status == "claimed" { "CLAIMED · 仕立てた一着" }
                    else if status == "skip" { "SKIPPED · 明日に期待" }
                    else if gen_status == "generating" { "GENERATING · 生成中" }
                    else if gen_status == "ready" { "TODAY'S CANDIDATE · 候補" }
                    else if gen_status == "failed" { "FAILED · 再生成待ち" }
                    else { "PROPOSAL · 提案" };
        let class = if status == "claimed" { "card claimed" } else { "card" };
        format!(
            r##"<a class="{class}" href="#" data-id="{id}">
  <div class="card-img" style="background-image:url('{img}')"></div>
  <div class="card-meta">
    <div class="day">DAY {day_num:03} · {day}</div>
    <div class="name">{name}</div>
    <div class="prompt">{prompt}</div>
    <div class="badge">{label}</div>
  </div>
</a>"##,
            class = class,
            id = id,
            img = img_src,
            day_num = day_num,
            day = html_escape(day),
            name = html_escape(name),
            prompt = html_escape(prompt),
            label = label,
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
    ").expect("init schema");
    // Idempotent column additions for existing DBs
    for col in &[
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
        "ALTER TABLE you_designs ADD COLUMN image_bytes BLOB",
        "ALTER TABLE you_designs ADD COLUMN image_mime TEXT",
        "ALTER TABLE you_designs ADD COLUMN gen_status TEXT NOT NULL DEFAULT 'pending'",
        "ALTER TABLE you_designs ADD COLUMN gen_error TEXT",
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
    ] {
        conn.execute(col, []).ok();
    }
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
        .route("/api/webhook/stripe", post(stripe_webhook))
        .route("/api/admin/import", post(import_product))
        .route("/api/admin/update-price", post(update_price))
        .route("/api/admin/update-nft", post(update_nft))
        .route("/api/admin/update-design", post(update_design))
        .route("/api/admin/update-sold", post(update_sold))
        .route("/api/admin/mockup", patch(update_mockup))
        .route("/api/admin/upload-mockup", post(upload_mockup))
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
        .route("/api/you/admin/list", get(you_admin_list))
        .route("/api/you/style", post(you_style_set))
        .route("/api/you/stats", get(you_active_count))
        // Per-user share page — REGISTER LAST so literal routes win
        .route("/:slug", get(slug_or_static))
        .nest_service("/static", ServeDir::new("static"))
        .nest_service("/mockups", ServeDir::new(mockups_dir()))
        .fallback_service(ServeDir::new("static"))
        .with_state(db)
        .layer(middleware::from_fn(security_headers))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        );

    let port = env::var("PORT").unwrap_or_else(|_| "3000".into());
    let addr = format!("0.0.0.0:{}", port);
    println!("mu-store listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
