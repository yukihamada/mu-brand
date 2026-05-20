//! Crypto checkout (Solana Pay USDC/SOL, ETH via EIP-681), Helius webhook,
//! Stripe Identity, and admin CSV exports for KYC + crypto reconciliation.
//!
//! Routes registered from main.rs:
//!   POST /api/checkout/crypto              → checkout_crypto
//!   GET  /api/checkout/crypto/status/:ref  → checkout_crypto_status
//!   POST /api/webhook/helius               → helius_webhook
//!   POST /api/kyc/identity-session         → create_stripe_identity_session
//!   GET  /api/admin/exports/kyc.csv        → admin_export_kyc
//!   GET  /api/admin/exports/crypto.csv     → admin_export_crypto
//!
//! All amounts denominated in JPY at the API boundary; conversion to crypto
//! happens here using env-var rates (JPY_PER_USD / JPY_PER_SOL / JPY_PER_ETH).
//! KYC records are written by `/api/checkout` and `/api/bid` in main.rs;
//! this module additionally writes from `checkout_crypto` for crypto orders
//! crossing the ¥300,000 threshold.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use rusqlite::params;
use std::env;
use std::sync::{Arc, Mutex};

pub type Db = Arc<Mutex<rusqlite::Connection>>;

// Surcharge layer (kept in sync with main.rs constants)
pub const PRICE_CAP_JPY: i64 = 300_000;
pub const KYC_THRESHOLD_JPY: i64 = 300_000;

pub fn payment_surcharge_bps(method: &str) -> i64 {
    match method.to_ascii_lowercase().as_str() {
        "eth" => 500,
        "usdc" | "sol" | "solana" | "crypto" => 300,
        _ => 0,
    }
}

pub fn apply_payment_surcharge(price_jpy: i64, method: &str) -> i64 {
    let bps = payment_surcharge_bps(method);
    if bps == 0 {
        return price_jpy.min(PRICE_CAP_JPY);
    }
    let surcharged = ((price_jpy as i128) * (10_000 + bps as i128) / 10_000) as i64;
    surcharged.min(PRICE_CAP_JPY)
}

// Crypto helpers
pub const SOLANA_USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
pub const JPY_PER_USD_DEFAULT: f64 = 150.0;
pub const JPY_PER_SOL_DEFAULT: f64 = 25_000.0;
pub const JPY_PER_ETH_DEFAULT: f64 = 600_000.0;
pub const CRYPTO_PAYMENT_TTL_MIN: i64 = 15;

/// Read a rate. Priority:
///   1. SQLite crypto_settings (set by Pyth cron, see `start_crons`)
///   2. process env var
///   3. compile-time default
/// Falls back through the chain so the server always has a usable rate
/// even when the oracle is unreachable.
fn env_rate(key: &str, default: f64) -> f64 {
    if let Some(db) = CRON_DB.get() {
        if let Ok(conn) = db.lock() {
            let v: Result<String, _> = conn.query_row(
                "SELECT value FROM crypto_settings WHERE key=?",
                params![key], |r| r.get(0)
            );
            if let Ok(s) = v {
                if let Ok(n) = s.parse::<f64>() {
                    if n.is_finite() && n > 0.0 { return n; }
                }
            }
        }
    }
    env::var(key).ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|r| r.is_finite() && *r > 0.0)
        .unwrap_or(default)
}

/// Process-global handle to the DB, populated by `start_crons` at startup.
/// `env_rate` uses this to peek at `crypto_settings` without taking a Db
/// argument (most call sites are deep inside synchronous helpers).
static CRON_DB: std::sync::OnceLock<Db> = std::sync::OnceLock::new();

/// Public accessor for main.rs (X poster needs DB without taking it as arg).
pub fn cron_db_ref() -> Option<Db> {
    CRON_DB.get().cloned()
}

/// True if the named env var is a placeholder value (unset, empty, or
/// contains "PLACEHOLDER" / "REPLACE"). Used by /api/payment_methods and
/// /health to gate crypto checkout until real secrets are deployed.
fn is_placeholder(key: &str) -> bool {
    let v = env::var(key).unwrap_or_default();
    if v.is_empty() { return true; }
    let up = v.to_ascii_uppercase();
    up.contains("PLACEHOLDER") || up.contains("REPLACE_ME") || up.contains("REPLACE_VIA")
        || v == "0x0000000000000000000000000000000000000000"
}

/// Crypto checkout is enabled per-asset only when both the receiver
/// wallet AND the corresponding webhook auth are real (non-placeholder).
fn usdc_enabled() -> bool { !is_placeholder("MU_SOL_RECIPIENT") && !is_placeholder("HELIUS_WEBHOOK_AUTH") }
fn sol_enabled()  -> bool { usdc_enabled() }
fn eth_enabled()  -> bool { !is_placeholder("MU_ETH_RECIPIENT") && !is_placeholder("ALCHEMY_WEBHOOK_SIGNING_KEY") }

/// GET /api/payment_methods — UI calls this on init to know which payment
/// methods to render. JPY is always on; crypto methods only on when both
/// the receiver wallet AND its webhook auth secret are real.
///
/// Security note (H039 fix, 2026-05-20): the previous version surfaced the
/// literal Fly secret variable names ("MU_ETH_RECIPIENT or
/// ALCHEMY_WEBHOOK_SIGNING_KEY is placeholder") in `_reason_unavailable`.
/// That gave anonymous callers `fly secrets list` -tier reconnaissance.
/// We now return a generic disabled reason — the per-secret detail still
/// surfaces on /health, which is operator-facing.
pub async fn payment_methods_handler(State(_db): State<Db>) -> impl IntoResponse {
    Json(serde_json::json!({
        "jpy":  true,
        "usdc": usdc_enabled(),
        "sol":  sol_enabled(),
        "eth":  eth_enabled(),
        "_reason_unavailable": {
            "usdc_sol": if !usdc_enabled() { Some("crypto temporarily unavailable") } else { None },
            "eth":      if !eth_enabled()  { Some("crypto temporarily unavailable") } else { None },
        }
    })).into_response()
}

/// Fire a Telegram message via the configured TELEGRAM_BOT_TOKEN /
/// TELEGRAM_CHAT_ID. Non-blocking, swallows errors (the bot failing
/// shouldn't break user-facing requests).
fn tg_alert(text: &str) {
    let token = env::var("TELEGRAM_BOT_TOKEN").unwrap_or_default();
    let chat_id = env::var("TELEGRAM_CHAT_ID").unwrap_or_default();
    if token.is_empty() || chat_id.is_empty() { return; }
    let text = format!("🚨 mu-store: {}", text);
    let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
    tokio::spawn(async move {
        let _ = reqwest::Client::new().post(&url)
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "text": text,
                "parse_mode": "HTML",
                "disable_web_page_preview": true,
            }))
            .timeout(std::time::Duration::from_secs(10))
            .send().await;
    });
}

/// GET /health — degraded-state report. Always returns 200 so Fly's HTTP
/// health check stays green; the JSON payload surfaces what's wrong.
pub async fn health_handler(State(db): State<Db>) -> impl IntoResponse {
    let placeholders: Vec<&str> = ["MU_SOL_RECIPIENT", "MU_ETH_RECIPIENT",
        "HELIUS_WEBHOOK_AUTH", "ALCHEMY_WEBHOOK_SIGNING_KEY",
        "STRIPE_IDENTITY_WEBHOOK_SECRET"]
        .iter().filter(|k| is_placeholder(k)).copied().collect();
    let pending_count: i64 = {
        if let Ok(conn) = db.lock() {
            conn.query_row(
                "SELECT COUNT(*) FROM pending_crypto_payments WHERE status='pending'",
                [], |r| r.get(0)
            ).unwrap_or(0)
        } else { 0 }
    };
    let last_rate_update: String = {
        if let Ok(conn) = db.lock() {
            conn.query_row(
                "SELECT MAX(updated_at) FROM crypto_settings",
                [], |r| r.get::<_,Option<String>>(0)
            ).unwrap_or(None).unwrap_or_default()
        } else { String::new() }
    };
    Json(serde_json::json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "degraded": !placeholders.is_empty(),
        "placeholder_secrets": placeholders,
        "crypto_enabled": {
            "usdc": usdc_enabled(),
            "sol":  sol_enabled(),
            "eth":  eth_enabled(),
        },
        "pending_crypto_payments": pending_count,
        "last_rate_refresh": last_rate_update,
    })).into_response()
}

pub fn jpy_to_usdc_amount(jpy: i64) -> String {
    format!("{:.2}", jpy as f64 / env_rate("JPY_PER_USD", JPY_PER_USD_DEFAULT))
}
pub fn jpy_to_sol_amount(jpy: i64) -> String {
    format!("{:.4}", jpy as f64 / env_rate("JPY_PER_SOL", JPY_PER_SOL_DEFAULT))
}
pub fn jpy_to_eth_amount(jpy: i64) -> String {
    format!("{:.6}", jpy as f64 / env_rate("JPY_PER_ETH", JPY_PER_ETH_DEFAULT))
}

/// Solana Pay URL — spec: https://docs.solanapay.com/spec
pub fn build_solana_pay_url(
    recipient: &str, amount: &str, spl_token: Option<&str>,
    reference: &str, label: &str, message: &str,
) -> String {
    let mut url = format!("solana:{}?amount={}", recipient, amount);
    if let Some(mint) = spl_token {
        url.push_str(&format!("&spl-token={}", mint));
    }
    url.push_str(&format!("&reference={}", reference));
    url.push_str(&format!("&label={}", urlencoding::encode(label)));
    url.push_str(&format!("&message={}", urlencoding::encode(message)));
    url
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    secs.to_string()
}

#[derive(serde::Deserialize)]
pub struct KycInfo {
    pub full_name: String,
    pub date_of_birth: String,
    pub nationality: String,
    pub id_type: String,
    pub id_last4: String,
    pub address: String,
    pub consent_at: String,
}

#[derive(serde::Deserialize, Default, Clone)]
pub struct ShippingInfo {
    pub name: String,
    pub line1: String,
    #[serde(default)]
    pub line2: String,
    pub city: String,
    #[serde(default)]
    pub state: String,
    pub zip: String,
    /// ISO 3166-1 alpha-2 (e.g. "JP", "US")
    pub country: String,
    #[serde(default)]
    pub phone: String,
}

impl ShippingInfo {
    fn is_complete(&self) -> bool {
        !self.name.trim().is_empty()
            && !self.line1.trim().is_empty()
            && !self.city.trim().is_empty()
            && !self.zip.trim().is_empty()
            && self.country.trim().len() == 2
    }
}

#[derive(serde::Deserialize)]
pub struct CryptoCheckoutBody {
    pub product_id: i64,
    pub quantity: u32,
    pub email: String,
    pub size: Option<String>,
    pub wallet: Option<String>,
    pub payment_method: String,
    pub kyc: Option<KycInfo>,
    /// Required (the Helius webhook needs this to fire Printful auto-order
    /// without a second user round-trip). Validated as complete on submit;
    /// see `ShippingInfo::is_complete`.
    pub shipping: Option<ShippingInfo>,
}

// MUGEN-cycle dynamic price (mirrors dynamic_price() in main.rs). We re-derive
// rather than reach into main.rs to avoid the cross-module coupling that was
// causing concurrent-edit races.
fn dynamic_price(brand: &str, drop_num: i64, sold: i64, name: &str) -> i64 {
    // MA starting bid lowered from ¥120,000 → ¥30,000 on 2026-05-11 when
    // MA cadence changed from monthly to weekly 7-day auctions.
    if brand == "ma" { return 30_000; }
    if brand == "nouns" {
        let nm = name.to_uppercase();
        if nm.contains("間") || nm.contains(" MA ") || nm.starts_with("MA ") || nm.ends_with(" MA") {
            return 30_000;
        }
    }
    if brand == "mugen" && drop_num == 108 { return 30_000; }
    (5_000 + sold.max(0) * 250).min(PRICE_CAP_JPY)
}

pub async fn checkout_crypto(
    State(db): State<Db>,
    Json(body): Json<CryptoCheckoutBody>,
) -> impl IntoResponse {
    let pm = body.payment_method.to_ascii_lowercase();
    if !matches!(pm.as_str(), "usdc" | "sol" | "solana" | "eth" | "crypto") {
        return (StatusCode::BAD_REQUEST, "payment_method must be one of: usdc, sol, eth").into_response();
    }
    // Server-side gate: never accept a crypto checkout if its on-chain
    // settlement webhook isn't fully configured. Otherwise we'd take a
    // payment we can't detect → customer pays, order never ships.
    let asset_enabled = match pm.as_str() {
        "usdc" | "sol" | "solana" | "crypto" => usdc_enabled(),
        "eth" => eth_enabled(),
        _ => false,
    };
    if !asset_enabled {
        return (StatusCode::SERVICE_UNAVAILABLE,
            "Crypto checkout for this method is temporarily disabled. \
             Webhook secret or recipient wallet is not yet configured. \
             Use payment_method=\"jpy\" or try again later.").into_response();
    }

    let check = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT brand, drop_num, inventory, sold, name FROM products WHERE id=? AND active=1",
            params![body.product_id],
            |row| Ok((row.get::<_,String>(0)?, row.get::<_,i64>(1)?,
                      row.get::<_,i64>(2)?, row.get::<_,i64>(3)?,
                      row.get::<_,String>(4)?))
        )
    };
    let (brand_str, drop_num, inventory, sold, product_name) = match check {
        Ok(r) => r,
        Err(_) => return (StatusCode::NOT_FOUND, "product not found").into_response(),
    };
    if inventory - sold < body.quantity as i64 {
        return (StatusCode::CONFLICT, "sold out").into_response();
    }

    // Phase 3.1: shipping is required so that Helius confirmation can
    // trigger Printful auto-fulfillment without a second user round-trip.
    let shipping = body.shipping.clone().unwrap_or_default();
    if !shipping.is_complete() {
        return (StatusCode::BAD_REQUEST,
            "shipping required: name, line1, city, zip, country (ISO-2)").into_response();
    }

    let base_price_jpy = dynamic_price(&brand_str, drop_num, sold, &product_name);
    let unit_price_jpy = apply_payment_surcharge(base_price_jpy, &pm);
    let total_jpy = unit_price_jpy.saturating_mul(body.quantity as i64);

    let mut kyc_record_id: i64 = 0;
    let mut kyc_token: String = String::new();
    if total_jpy >= KYC_THRESHOLD_JPY {
        let Some(kyc) = body.kyc.as_ref() else {
            return (StatusCode::BAD_REQUEST,
                "KYC required for purchases at or above ¥300,000").into_response();
        };
        if kyc.full_name.trim().is_empty() || kyc.id_last4.trim().is_empty() {
            return (StatusCode::BAD_REQUEST, "KYC required (incomplete fields)").into_response();
        }
        kyc_token = new_kyc_verification_token();
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO kyc_records
             (product_id, email, full_name, dob, nationality, id_type, id_last4,
              address, consent_at, payment_method, total_amount_jpy, created_at,
              verification_token)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                body.product_id, body.email,
                kyc.full_name.trim(), kyc.date_of_birth.trim(),
                kyc.nationality.trim(), kyc.id_type.trim(), kyc.id_last4.trim(),
                kyc.address.trim(), kyc.consent_at.trim(),
                pm, total_jpy, now_iso(),
                kyc_token,
            ]
        );
        kyc_record_id = conn.last_insert_rowid();
    }

    // R2 fix: Solana Pay `reference` must be a base58-encoded 32-byte
    // pubkey-format value. The Helius / @solana/pay client iterates txs that
    // include this pubkey as a signer/non-signer reference. A UUID string
    // (hex+hyphens) cannot be decoded as a pubkey → no tx ever confirms.
    // For ETH this is also fine — it's just an opaque order identifier there.
    let reference = new_solana_pay_reference();
    let size_label = body.size.clone().unwrap_or_else(|| "M".into());

    let (amount_crypto, asset, recipient, pay_url): (String, &str, String, String) = match pm.as_str() {
        "usdc" | "crypto" => {
            let recipient = env::var("MU_SOL_RECIPIENT")
                .unwrap_or_else(|_| "REPLACE_ME_WITH_MU_SOL_ADDRESS".into());
            let amt = jpy_to_usdc_amount(total_jpy);
            let url = build_solana_pay_url(
                &recipient, &amt, Some(SOLANA_USDC_MINT), &reference,
                &format!("MU — {} ({})", product_name, size_label),
                &format!("¥{} (USDC). Order #{}", total_jpy, body.product_id),
            );
            (amt, "USDC", recipient, url)
        }
        "sol" | "solana" => {
            let recipient = env::var("MU_SOL_RECIPIENT")
                .unwrap_or_else(|_| "REPLACE_ME_WITH_MU_SOL_ADDRESS".into());
            let amt = jpy_to_sol_amount(total_jpy);
            let url = build_solana_pay_url(
                &recipient, &amt, None, &reference,
                &format!("MU — {} ({})", product_name, size_label),
                &format!("¥{} (SOL). Order #{}", total_jpy, body.product_id),
            );
            (amt, "SOL", recipient, url)
        }
        "eth" => {
            let recipient = env::var("MU_ETH_RECIPIENT")
                .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".into());
            let amt = jpy_to_eth_amount(total_jpy);
            let wei = ((amt.parse::<f64>().unwrap_or(0.0)) * 1e18) as u128;
            let url = format!("ethereum:{}?value={}", recipient, wei);
            (amt, "ETH", recipient, url)
        }
        _ => unreachable!(),
    };

    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO pending_crypto_payments
             (reference, product_id, email, size, quantity, wallet, payment_method,
              amount_jpy, amount_crypto, asset, recipient, pay_url,
              status, expires_at, created_at,
              ship_name, ship_line1, ship_line2, ship_city, ship_state,
              ship_zip, ship_country, ship_phone)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,'pending',?,?,?,?,?,?,?,?,?,?)",
            params![
                reference, body.product_id, body.email, size_label, body.quantity,
                body.wallet, pm,
                total_jpy, amount_crypto, asset, recipient, pay_url,
                now_iso(), now_iso(),
                shipping.name.trim(), shipping.line1.trim(), shipping.line2.trim(),
                shipping.city.trim(), shipping.state.trim(),
                shipping.zip.trim(), shipping.country.trim().to_uppercase(),
                shipping.phone.trim()
            ]
        );
    }

    Json(serde_json::json!({
        "ok": true,
        "reference": reference,
        "asset": asset,
        "amount_crypto": amount_crypto,
        "amount_jpy": total_jpy,
        "unit_price_jpy": unit_price_jpy,
        "base_price_jpy": base_price_jpy,
        "surcharge_bps": payment_surcharge_bps(&pm),
        "recipient": recipient,
        "pay_url": pay_url,
        "expires_in_min": CRYPTO_PAYMENT_TTL_MIN,
        "status_url": format!("/api/checkout/crypto/status/{}", reference),
        // Returned only when KYC was triggered (total_jpy ≥ ¥300,000).
        // Pass these to /api/kyc/identity-session — the token closes R3 (IDOR).
        "kyc_record_id": if kyc_record_id > 0 { serde_json::Value::from(kyc_record_id) } else { serde_json::Value::Null },
        "kyc_verification_token": if !kyc_token.is_empty() { serde_json::Value::from(kyc_token) } else { serde_json::Value::Null },
    })).into_response()
}

/// 256-bit random token used to gate Stripe Identity session creation for
/// a specific kyc_records row. Two UUIDv4 concatenated → ~256 bits, 64 hex chars.
/// Single-use, scoped to one row.
pub fn new_kyc_verification_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple(),
    )
}

/// 32 random bytes encoded as base58 — a Solana Pay `reference` shaped like
/// an ed25519 pubkey (the spec requires this format so the wallet client
/// can attach it as a tx non-signer key for later lookup). Bytes are not
/// derived from any keypair; this is just an opaque identifier with the
/// right shape. ~44 chars typical. R2 fix.
pub fn new_solana_pay_reference() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bs58::encode(bytes).into_string()
}

/// GET /api/rates — exposes the current JPY/USD, JPY/SOL, JPY/ETH rates the
/// server is using. Pyth-refreshed by the rate cron; falls back to env vars,
/// then defaults. Used by the UI to show "1 USDC = ¥X" next to each
/// payment-method button.
pub async fn rates_handler(State(_db): State<Db>) -> impl IntoResponse {
    let jpy_per_usd = env_rate("JPY_PER_USD", JPY_PER_USD_DEFAULT);
    let jpy_per_sol = env_rate("JPY_PER_SOL", JPY_PER_SOL_DEFAULT);
    let jpy_per_eth = env_rate("JPY_PER_ETH", JPY_PER_ETH_DEFAULT);
    Json(serde_json::json!({
        "jpy_per_usd": jpy_per_usd,
        "jpy_per_sol": jpy_per_sol,
        "jpy_per_eth": jpy_per_eth,
        "usdc_per_jpy": 1.0 / jpy_per_usd,
        "sol_per_jpy":  1.0 / jpy_per_sol,
        "eth_per_jpy":  1.0 / jpy_per_eth,
    })).into_response()
}

pub async fn checkout_crypto_status(
    State(db): State<Db>,
    Path(reference): Path<String>,
) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    let row = conn.query_row(
        "SELECT status, tx_signature, confirmed_at, amount_jpy, asset
         FROM pending_crypto_payments WHERE reference=?",
        params![reference],
        |r| Ok((r.get::<_,String>(0)?, r.get::<_,Option<String>>(1)?,
                r.get::<_,Option<String>>(2)?, r.get::<_,i64>(3)?, r.get::<_,String>(4)?))
    );
    match row {
        Ok((status, tx, confirmed_at, amount_jpy, asset)) => Json(serde_json::json!({
            "reference": reference, "status": status, "tx_signature": tx,
            "confirmed_at": confirmed_at, "amount_jpy": amount_jpy, "asset": asset,
        })).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

/// Constant-time string comparison. Prevents timing-attack disclosure of
/// the secret length / content via response-time differences.
fn ct_eq(a: &str, b: &str) -> bool {
    let aa = a.as_bytes();
    let bb = b.as_bytes();
    if aa.len() != bb.len() { return false; }
    let mut diff: u8 = 0;
    for (x, y) in aa.iter().zip(bb.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Helius enhanced-webhook handler.
///
/// Security model:
///   1. Auth: shared secret in Authorization header, compared in constant time.
///   2. Recipient check: every event must include our `MU_SOL_RECIPIENT` (or
///      `MU_ETH_RECIPIENT`) in `accountData[].account`. Without this an
///      attacker who learns the webhook secret could forge "payments" with
///      arbitrary reference keys.
///   3. Amount tolerance: when the event carries `nativeBalanceChange` or
///      `tokenBalanceChanges` for our recipient, we require it to be at
///      least 95% of the expected amount_crypto (5% slip for fee netting /
///      rate drift within the TTL window).
///   4. Idempotency: status='pending' guard in the UPDATE — replaying the
///      same event won't double-confirm or double-increment sold count.
pub async fn helius_webhook(
    State(db): State<Db>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    let expected = env::var("HELIUS_WEBHOOK_AUTH").unwrap_or_default();
    let got = headers.get("authorization")
        .and_then(|h| h.to_str().ok()).unwrap_or("");
    if expected.is_empty() || !ct_eq(got, &expected) {
        return (StatusCode::UNAUTHORIZED, "auth").into_response();
    }
    let mu_sol = env::var("MU_SOL_RECIPIENT").unwrap_or_default();
    let mu_eth = env::var("MU_ETH_RECIPIENT").unwrap_or_default();

    let v: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "json").into_response(),
    };
    let events = v.as_array().cloned().unwrap_or_else(|| vec![v]);
    let mut matched = 0usize;
    let mut skipped_no_recipient = 0usize;
    let mut skipped_amount = 0usize;

    for ev in events {
        let signature = ev["signature"].as_str().unwrap_or("").to_string();
        let account_keys: Vec<String> = ev["accountData"].as_array()
            .map(|arr| arr.iter()
                .filter_map(|a| a["account"].as_str().map(|s| s.to_string()))
                .collect())
            .unwrap_or_default();
        if signature.is_empty() || account_keys.is_empty() { continue; }

        // Recipient must appear in this event. Otherwise it's not a tx
        // routed at us and we should not consume it.
        let recipient_present = (!mu_sol.is_empty() && account_keys.iter().any(|k| k == &mu_sol))
            || (!mu_eth.is_empty() && account_keys.iter().any(|k| k.eq_ignore_ascii_case(&mu_eth)));
        if !recipient_present {
            skipped_no_recipient += 1;
            continue;
        }

        // Build a quick lookup of net balance changes credited to our
        // recipient (positive deltas only). Both native lamports and SPL
        // token transfers are considered.
        let mut credited_lamports: u64 = 0;
        let mut credited_token_units: u128 = 0;
        if let Some(arr) = ev["accountData"].as_array() {
            for a in arr {
                let acct = a["account"].as_str().unwrap_or("");
                if acct != mu_sol && !acct.eq_ignore_ascii_case(&mu_eth) { continue; }
                if let Some(d) = a["nativeBalanceChange"].as_i64() {
                    if d > 0 { credited_lamports = credited_lamports.saturating_add(d as u64); }
                }
                if let Some(tb) = a["tokenBalanceChanges"].as_array() {
                    for t in tb {
                        let amt_str = t["rawTokenAmount"]["tokenAmount"]
                            .as_str().unwrap_or("0");
                        if let Ok(n) = amt_str.parse::<i128>() {
                            if n > 0 { credited_token_units =
                                credited_token_units.saturating_add(n as u128); }
                        }
                    }
                }
            }
        }

        // Iterate reference keys; only confirm the row if the credited
        // amount is at least 95% of the expected crypto amount.
        for key in &account_keys {
            // Look up expected payment row first; skip if not a reference of ours.
            let row = {
                let conn = db.lock().unwrap();
                conn.query_row(
                    "SELECT product_id, amount_crypto, asset, payment_method
                     FROM pending_crypto_payments
                     WHERE reference=? AND status='pending'",
                    params![key],
                    |r| Ok((r.get::<_,i64>(0)?, r.get::<_,String>(1)?,
                            r.get::<_,String>(2)?, r.get::<_,String>(3)?))
                ).ok()
            };
            let Some((product_id, expected_amt_str, asset, _pm)) = row else { continue; };

            let expected_amt: f64 = expected_amt_str.parse().unwrap_or(0.0);
            // Convert expected_amt to the same unit as the on-chain credit.
            // USDC has 6 decimals; SOL native is lamports (9 dec → use lamports).
            let (expected_units, credited_units) = match asset.as_str() {
                "USDC" => (
                    (expected_amt * 1_000_000.0) as u128,
                    credited_token_units,
                ),
                "SOL" => (
                    (expected_amt * 1_000_000_000.0) as u128,
                    credited_lamports as u128,
                ),
                _ => (0u128, 0u128), // ETH is reconciled separately; webhook
                                    // only treats Solana for now.
            };
            if expected_units == 0 || credited_units == 0
                || credited_units * 100 < expected_units * 95
            {
                skipped_amount += 1;
                continue;
            }

            // Confirm + bump sold count in a scoped lock.
            let fulfill_now: bool = {
                let conn = db.lock().unwrap();
                let upd = conn.execute(
                    "UPDATE pending_crypto_payments
                     SET status='confirmed', tx_signature=?, confirmed_at=?
                     WHERE reference=? AND status='pending'",
                    params![signature, now_iso(), key]
                ).unwrap_or(0);
                if upd > 0 {
                    let _ = conn.execute(
                        "UPDATE products SET sold = sold + 1 WHERE id=?",
                        params![product_id]
                    );
                    matched += 1;
                    tracing::info!(
                        "[helius] confirmed ref={} product_id={} sig={} asset={} credited={} expected={}",
                        key, product_id, signature, asset, credited_units, expected_units
                    );
                    true
                } else { false }
            };
            if fulfill_now {
                // Phase 3.2 + 3.3: fire Printful auto-order + Resend
                // confirmation email asynchronously. Lock has been released
                // above so this spawn doesn't pin the DB mutex.
                let db_clone = db.clone();
                let key_clone = key.clone();
                tokio::spawn(async move {
                    fulfill_crypto_order(db_clone, key_clone).await;
                });
            }
        }
    }
    Json(serde_json::json!({
        "ok": true,
        "matched": matched,
        "skipped_no_recipient": skipped_no_recipient,
        "skipped_amount_too_low": skipped_amount,
    })).into_response()
}

// ── Phase 3.2 + 3.3: post-confirmation fulfillment pipeline ────────────
//
// Triggered from helius_webhook on a successful confirm. Reads the
// pending_crypto_payments row (now status='confirmed'), pulls the product
// design / size / shipping, fires:
//
//   (1) Printful order  — auto-fulfillment via PRINTFUL_API_KEY
//   (2) Resend email    — confirmation to the buyer + tx receipt
//   (3) Stamps printful_order_id and fulfilled_at back on the row
//
// All failures are logged. Each side-effect is independent: an email
// failure does not unwind a Printful order, and vice versa. Operator
// can manually retry via the admin endpoint (TODO follow-up).

async fn fulfill_crypto_order(db: Db, reference: String) {
    // 1. Load all needed data in a single scoped lock.
    let load: Option<(i64, String, String, String, i64, String, i64, String, String, String, String, String, String, String, String, String, String)> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT pcp.product_id, pcp.email, pcp.size, pcp.tx_signature,
                    pcp.amount_jpy, pcp.asset, pcp.quantity,
                    pcp.ship_name, pcp.ship_line1, COALESCE(pcp.ship_line2,''),
                    pcp.ship_city, COALESCE(pcp.ship_state,''),
                    pcp.ship_zip, pcp.ship_country, COALESCE(pcp.ship_phone,''),
                    p.name, COALESCE(p.design_url, p.mockup_url, '')
             FROM pending_crypto_payments pcp
             JOIN products p ON p.id = pcp.product_id
             WHERE pcp.reference=? AND pcp.status='confirmed'",
            params![reference],
            |r| Ok((
                r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_,Option<String>>(3)?.unwrap_or_default(),
                r.get(4)?, r.get(5)?, r.get(6)?,
                r.get(7)?, r.get(8)?, r.get(9)?,
                r.get(10)?, r.get(11)?,
                r.get(12)?, r.get(13)?, r.get(14)?,
                r.get(15)?, r.get(16)?,
            ))
        ).ok()
    };
    let Some((product_id, email, size, tx_sig, amount_jpy, asset, quantity,
             ship_name, ship_line1, ship_line2, ship_city, ship_state,
             ship_zip, ship_country, ship_phone,
             product_name, design_url)) = load else {
        tracing::warn!("[fulfill] no confirmed row for reference {}", reference);
        return;
    };

    if design_url.is_empty() {
        tracing::warn!("[fulfill] product {} has no design_url; skipping Printful", product_id);
    }

    // 2. Printful order (only if key is configured and design_url present).
    let printful_key = env::var("PRINTFUL_API_KEY").unwrap_or_default();
    let mut printful_order_id: Option<String> = None;
    if !printful_key.is_empty() && !design_url.is_empty() {
        let variant_id: u64 = match size.as_str() {
            "S" => 4016, "M" => 4017, "L" => 4018, "XL" => 4019, _ => 4017,
        };
        let order = serde_json::json!({
            "recipient": {
                "name": ship_name,
                "address1": ship_line1,
                "address2": ship_line2,
                "city": ship_city,
                "state_code": ship_state,
                "country_code": ship_country.to_uppercase(),
                "zip": ship_zip,
                "phone": ship_phone,
                "email": email,
            },
            "items": [{
                "variant_id": variant_id,
                "quantity": quantity,
                "files": [{"url": design_url, "placement": "front"}],
            }],
            "confirm": true,
        });
        match reqwest::Client::new()
            .post("https://api.printful.com/orders")
            .bearer_auth(&printful_key)
            .json(&order).send().await
        {
            Ok(r) if r.status().is_success() => {
                let j: serde_json::Value = r.json().await.unwrap_or_default();
                let oid = j["result"]["id"].as_i64()
                    .map(|n| n.to_string())
                    .or_else(|| j["result"]["external_id"].as_str().map(|s| s.to_string()));
                if let Some(ref oid) = oid {
                    let conn = db.lock().unwrap();
                    let _ = conn.execute(
                        "UPDATE pending_crypto_payments
                         SET printful_order_id=?, fulfilled_at=?
                         WHERE reference=?",
                        params![oid, now_iso(), reference]
                    );
                }
                printful_order_id = oid;
                tracing::info!("[fulfill] Printful OK ref={} order_id={:?}", reference, printful_order_id);
            }
            Ok(r) => {
                let s = r.status();
                let body = r.text().await.unwrap_or_default();
                tracing::warn!("[fulfill] Printful {} ref={}: {}", s, reference, &body[..body.len().min(300)]);
                tg_alert(&format!(
                    "Printful order FAILED for confirmed crypto payment.\n\
                     ref={}\nproduct={}\nstatus={}\nbody={}",
                    reference, product_id, s, &body[..body.len().min(400)]));
            }
            Err(e) => {
                tracing::warn!("[fulfill] Printful net err ref={}: {}", reference, e);
                tg_alert(&format!(
                    "Printful network error for confirmed crypto payment.\n\
                     ref={}\nproduct={}\nerror={}",
                    reference, product_id, e));
            }
        }
    } else {
        tracing::info!("[fulfill] Printful skipped (no key or no design_url) ref={}", reference);
    }

    // 3. Confirmation email via Resend (independent of Printful outcome).
    let resend_key = env::var("RESEND_API_KEY").unwrap_or_default();
    if !resend_key.is_empty() {
        let order_id_html = printful_order_id.as_ref()
            .map(|o| format!("Order #{}", o))
            .unwrap_or_else(|| "Pending fulfillment ID".to_string());
        let html = format!(
            r#"<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:32px 0;margin:0"><div style="max-width:600px;margin:0 auto;padding:0 32px"><div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:24px">MU</div><div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#5cf;opacity:0.85;margin-bottom:8px">PAYMENT CONFIRMED</div><h2 style="font-size:18px;font-weight:300;line-height:1.4;margin:0 0 18px">{name} ({size}) — fulfillment started</h2><table style="width:100%;font-size:12px;line-height:1.8;border-collapse:collapse;margin-bottom:24px"><tr><td style="opacity:0.55;padding:4px 0;width:40%">Asset</td><td style="padding:4px 0">{asset}</td></tr><tr><td style="opacity:0.55;padding:4px 0">Reference</td><td style="padding:4px 0;font-family:monospace">{ref_id}</td></tr><tr><td style="opacity:0.55;padding:4px 0">Tx signature</td><td style="padding:4px 0;font-family:monospace;word-break:break-all">{tx}</td></tr><tr><td style="opacity:0.55;padding:4px 0">Amount (JPY)</td><td style="padding:4px 0">¥{amt}</td></tr><tr><td style="opacity:0.55;padding:4px 0">Quantity</td><td style="padding:4px 0">{qty}</td></tr><tr><td style="opacity:0.55;padding:4px 0">Order</td><td style="padding:4px 0">{oid}</td></tr></table><p style="font-size:12px;line-height:1.85;opacity:0.7;margin:0 0 18px">Your garment will be printed on-demand and shipped to:<br><br><b>{sn}</b><br>{s1}{s2br}<br>{sc}{ssp}{sz} {scn}<br>{sph}</p><p style="font-size:11px;line-height:1.85;opacity:0.55;margin:24px 0 0">Typically 7-10 business days for international shipping (DHL/FedEx). Tracking link will follow when Printful hands off to the carrier.<br><br>Reply to this email if anything looks wrong, or contact <a href="mailto:info@enablerdao.com" style="color:#5cf">info@enablerdao.com</a>.</p></div></div>"#,
            name = product_name, size = size, asset = asset, ref_id = reference,
            tx = if tx_sig.is_empty() { "—".to_string() } else { tx_sig.clone() },
            amt = amount_jpy.to_string(), qty = quantity, oid = order_id_html,
            sn = ship_name, s1 = ship_line1,
            s2br = if ship_line2.is_empty() { String::new() } else { format!(", {}", ship_line2) },
            sc = ship_city, ssp = if ship_state.is_empty() { ", ".to_string() } else { format!(", {} ", ship_state) },
            sz = ship_zip, scn = ship_country,
            sph = if ship_phone.is_empty() { String::new() } else { format!("Tel: {}", ship_phone) },
        );
        let subject = format!("MU — Payment confirmed for {} ({})", product_name, size);
        let payload = serde_json::json!({
            "from": "MU <noreply@wearmu.com>",
            "to": [email.clone()],
            "subject": subject,
            "html": html,
        });
        match reqwest::Client::new()
            .post("https://api.resend.com/emails")
            .bearer_auth(&resend_key)
            .json(&payload).send().await
        {
            Ok(r) if r.status().is_success() => {
                tracing::info!("[fulfill] Resend OK ref={} → {}", reference, email);
            }
            Ok(r) => {
                let s = r.status();
                let b = r.text().await.unwrap_or_default();
                tracing::warn!("[fulfill] Resend {} ref={}: {}", s, reference, &b[..b.len().min(300)]);
                tg_alert(&format!(
                    "Resend confirmation-email FAILED.\nref={}\nemail={}\nstatus={}",
                    reference, email, s));
            }
            Err(e) => {
                tracing::warn!("[fulfill] Resend net err ref={}: {}", reference, e);
                tg_alert(&format!(
                    "Resend network error.\nref={}\nemail={}\nerror={}",
                    reference, email, e));
            }
        }
    } else {
        tracing::info!("[fulfill] Resend skipped (no key) ref={}", reference);
    }

    // ── Soulbound NFT pilot trigger (crypto-settled path) ──
    // The buyer's Solana wallet is stored on the pending_crypto_payments row.
    // If present, dispatch the cNFT mint in the background. Dry-run by default;
    // see store/src/nft.rs and `MU_NFT_MINT_LIVE`.
    let buyer_wallet: String = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(wallet,'') FROM pending_crypto_payments WHERE reference=?",
            params![reference],
            |r| r.get::<_, String>(0),
        ).unwrap_or_default()
    };
    if !buyer_wallet.trim().is_empty() {
        crate::nft::mint_soulbound_bg(db.clone(), product_id, buyer_wallet, "helius_webhook");
    } else {
        tracing::info!("[nft] crypto-settle skipped product_id={} ref={}: no wallet", product_id, reference);
    }
}

// ── Admin CSV exports ─────────────────────────────────────────────────
fn require_admin(headers: &HeaderMap) -> Result<(), Response> {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "ADMIN_TOKEN not set").into_response());
    }
    let got = headers.get("x-admin-token")
        .and_then(|h| h.to_str().ok()).unwrap_or("");
    if !ct_eq(got, &expected) {
        return Err((StatusCode::UNAUTHORIZED, "admin token mismatch").into_response());
    }
    Ok(())
}

/// CSV cell escaping. Quotes any cell that contains `, " \r \n` and any
/// cell starting with the Excel/Sheets formula-injection sentinels
/// `= + - @` so opening the CSV does not silently execute as a formula.
fn csv_escape(s: &str) -> String {
    let needs_quote = s.contains(',') || s.contains('"')
        || s.contains('\n') || s.contains('\r');
    let needs_prefix = s.starts_with('=') || s.starts_with('+')
        || s.starts_with('-') || s.starts_with('@');
    let core: String = if needs_prefix { format!("'{}", s) } else { s.to_string() };
    if needs_quote || needs_prefix {
        format!("\"{}\"", core.replace('"', "\"\""))
    } else { core }
}

pub async fn admin_export_kyc(
    State(db): State<Db>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&headers) { return e; }
    let conn = db.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT id, product_id, email, full_name, dob, nationality, id_type,
                id_last4, address, consent_at, payment_method, total_amount_jpy,
                created_at FROM kyc_records ORDER BY created_at DESC"
    ) {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("prep: {}", e)).into_response(),
    };
    let rows = stmt.query_map([], |r| Ok((
        r.get::<_,i64>(0)?, r.get::<_,i64>(1)?, r.get::<_,String>(2)?,
        r.get::<_,String>(3)?, r.get::<_,String>(4)?, r.get::<_,String>(5)?,
        r.get::<_,String>(6)?, r.get::<_,String>(7)?, r.get::<_,String>(8)?,
        r.get::<_,String>(9)?, r.get::<_,String>(10)?, r.get::<_,i64>(11)?,
        r.get::<_,String>(12)?,
    )));
    let mut out = String::from(
        "id,product_id,email,full_name,dob,nationality,id_type,id_last4,address,consent_at,payment_method,total_amount_jpy,created_at\n"
    );
    if let Ok(iter) = rows {
        for row in iter.flatten() {
            out.push_str(&format!("{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                row.0, row.1, csv_escape(&row.2), csv_escape(&row.3), csv_escape(&row.4),
                csv_escape(&row.5), csv_escape(&row.6), csv_escape(&row.7),
                csv_escape(&row.8), csv_escape(&row.9), csv_escape(&row.10),
                row.11, csv_escape(&row.12)));
        }
    }
    ([
        (axum::http::header::CONTENT_TYPE, "text/csv; charset=utf-8"),
        (axum::http::header::CONTENT_DISPOSITION, "attachment; filename=\"kyc_records.csv\""),
    ], out).into_response()
}

pub async fn admin_export_crypto(
    State(db): State<Db>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&headers) { return e; }
    let conn = db.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT id, reference, product_id, email, payment_method, amount_jpy,
                amount_crypto, asset, status, COALESCE(tx_signature,''),
                COALESCE(confirmed_at,''), created_at
         FROM pending_crypto_payments ORDER BY created_at DESC"
    ) {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("prep: {}", e)).into_response(),
    };
    let rows = stmt.query_map([], |r| Ok((
        r.get::<_,i64>(0)?, r.get::<_,String>(1)?, r.get::<_,i64>(2)?,
        r.get::<_,String>(3)?, r.get::<_,String>(4)?, r.get::<_,i64>(5)?,
        r.get::<_,String>(6)?, r.get::<_,String>(7)?, r.get::<_,String>(8)?,
        r.get::<_,String>(9)?, r.get::<_,String>(10)?, r.get::<_,String>(11)?,
    )));
    let mut out = String::from(
        "id,reference,product_id,email,payment_method,amount_jpy,amount_crypto,asset,status,tx_signature,confirmed_at,created_at\n"
    );
    if let Ok(iter) = rows {
        for row in iter.flatten() {
            out.push_str(&format!("{},{},{},{},{},{},{},{},{},{},{},{}\n",
                row.0, csv_escape(&row.1), row.2, csv_escape(&row.3),
                csv_escape(&row.4), row.5, csv_escape(&row.6), csv_escape(&row.7),
                csv_escape(&row.8), csv_escape(&row.9), csv_escape(&row.10), csv_escape(&row.11)));
        }
    }
    ([
        (axum::http::header::CONTENT_TYPE, "text/csv; charset=utf-8"),
        (axum::http::header::CONTENT_DISPOSITION, "attachment; filename=\"crypto_payments.csv\""),
    ], out).into_response()
}

/// Generate a Stripe Identity verification session URL for high-value KYC.
/// POST /api/kyc/identity-session — create a Stripe Identity verification
/// session for a previously-created kyc_records row.
///
/// HARDENED for R3 (Stripe Identity IDOR + open redirect):
/// 1. Caller MUST present the per-record `verification_token` returned at
///    checkout time. Without it the request is rejected (was: unauthenticated).
/// 2. `kyc_record_id` is still accepted but only the (id, token) tuple is
///    trusted — the row's stored `email` is forwarded to Stripe, ignoring any
///    client-supplied email (was: attacker could swap email).
/// 3. `return_url` is validated against an allowlist of wearmu.com / teai.io
///    hosts (was: open redirect via Stripe success page).
/// 4. Refuses to create a duplicate session if one already exists for the row.
pub async fn create_stripe_identity_session(
    State(db): State<Db>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    if stripe_key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "stripe key not configured").into_response();
    }

    let kyc_record_id = body["kyc_record_id"].as_i64().unwrap_or(0);
    let token_in = body["verification_token"].as_str().unwrap_or("").trim().to_string();
    let return_url_in = body["return_url"].as_str().unwrap_or("").trim().to_string();
    if kyc_record_id <= 0 || token_in.len() < 16 || token_in.len() > 128 {
        return (StatusCode::BAD_REQUEST,
            "kyc_record_id + verification_token required").into_response();
    }

    // 1. Verify (id, token) pair + load stored email + existing session id.
    let row: Result<(String, Option<String>, Option<String>), rusqlite::Error> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT email, verification_token, stripe_identity_session_id
             FROM kyc_records WHERE id=?",
            params![kyc_record_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
    };
    let (stored_email, stored_token, existing_session_id) = match row {
        Ok(r) => r,
        Err(_) => {
            // Constant-ish response to not leak existence.
            return (StatusCode::FORBIDDEN, "verification_token mismatch").into_response();
        }
    };
    let expected = stored_token.unwrap_or_default();
    if expected.is_empty() || !ct_eq(&expected, &token_in) {
        return (StatusCode::FORBIDDEN, "verification_token mismatch").into_response();
    }

    // 2. Refuse to re-create a session for the same record. Idempotency +
    //    prevents an attacker who later acquires the token from re-binding.
    if let Some(sid) = existing_session_id.as_ref().filter(|s| !s.is_empty()) {
        return Json(serde_json::json!({
            "ok": true,
            "id": sid,
            "already_created": true,
            "message": "identity session already exists for this kyc_record",
        })).into_response();
    }

    // 3. return_url allowlist. Only wearmu.com / teai.io (+ www.) over https.
    //    Empty / invalid → safe default.
    let return_url = sanitize_kyc_return_url(&return_url_in);

    let resp = reqwest::Client::new()
        .post("https://api.stripe.com/v1/identity/verification_sessions")
        .basic_auth(&stripe_key, None::<&str>)
        .form(&[
            ("type", "document"),
            ("metadata[kyc_record_id]", &kyc_record_id.to_string()),
            ("metadata[email]", &stored_email),  // server-side email, NOT client-supplied
            ("return_url", &return_url),
        ])
        .send().await;
    match resp {
        Ok(r) if r.status().is_success() => {
            let json: serde_json::Value = r.json().await.unwrap_or(serde_json::json!({}));
            let session_id = json["id"].as_str().unwrap_or("").to_string();
            // Persist session id immediately so re-requests are short-circuited.
            if !session_id.is_empty() {
                let conn = db.lock().unwrap();
                let _ = conn.execute(
                    "UPDATE kyc_records SET stripe_identity_session_id=?, stripe_identity_status='pending'
                     WHERE id=? AND (stripe_identity_session_id IS NULL OR stripe_identity_session_id='')",
                    params![session_id, kyc_record_id],
                );
            }
            Json(serde_json::json!({"url": json["url"], "id": json["id"]})).into_response()
        }
        Ok(r) => {
            let status = r.status();
            let txt = r.text().await.unwrap_or_default();
            (StatusCode::INTERNAL_SERVER_ERROR,
             format!("stripe identity error {}: {}", status, &txt[..txt.len().min(200)])).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("stripe http: {}", e)).into_response(),
    }
}

/// Allowlist + scheme + length validation for the Stripe Identity return_url.
/// Falls back to https://wearmu.com/ on any rejection. Closes the open-redirect
/// half of R3.
pub fn sanitize_kyc_return_url(input: &str) -> String {
    const FALLBACK: &str = "https://wearmu.com/";
    const ALLOWED_HOSTS: &[&str] = &[
        "wearmu.com", "www.wearmu.com",
        "teai.io", "www.teai.io",
        "chatweb.ai", "www.chatweb.ai",
    ];
    let s = input.trim();
    if s.is_empty() || s.len() > 500 { return FALLBACK.into(); }
    // Reject anything that isn't https.
    let Some(rest) = s.strip_prefix("https://") else { return FALLBACK.into(); };
    // Host is everything up to the first /, ?, or #
    let host_end = rest.find(|c: char| matches!(c, '/' | '?' | '#')).unwrap_or(rest.len());
    let host_part = &rest[..host_end];
    // Strip optional :port, basic auth not allowed.
    if host_part.contains('@') { return FALLBACK.into(); }
    let host = host_part.split(':').next().unwrap_or("").to_ascii_lowercase();
    if !ALLOWED_HOSTS.contains(&host.as_str()) { return FALLBACK.into(); }
    s.to_string()
}

// ── Tests ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surcharge_three_percent_for_crypto() {
        assert_eq!(apply_payment_surcharge(5_000, "usdc"), 5_150);
        assert_eq!(apply_payment_surcharge(5_000, "sol"), 5_150);
    }

    #[test]
    fn surcharge_five_percent_for_eth() {
        assert_eq!(apply_payment_surcharge(5_000, "eth"), 5_250);
    }

    #[test]
    fn surcharge_clamps_to_cap() {
        assert_eq!(apply_payment_surcharge(PRICE_CAP_JPY, "eth"), PRICE_CAP_JPY);
        assert_eq!(apply_payment_surcharge(295_000, "eth"), PRICE_CAP_JPY);
    }

    #[test]
    fn solana_pay_url_contains_expected_params() {
        let url = build_solana_pay_url(
            "MURecipient11111111111111111111111111111111",
            "10.50", Some(SOLANA_USDC_MINT),
            "ref123", "MU MUGEN #42", "thanks",
        );
        assert!(url.starts_with("solana:MURecipient11111111111111111111111111111111?"));
        assert!(url.contains("amount=10.50"));
        assert!(url.contains(&format!("spl-token={}", SOLANA_USDC_MINT)));
        assert!(url.contains("reference=ref123"));
    }

    #[test]
    fn jpy_to_usdc_default_rate() {
        std::env::remove_var("JPY_PER_USD");
        assert_eq!(jpy_to_usdc_amount(150_000), "1000.00");
    }

    #[test]
    fn ct_eq_handles_unequal_lengths() {
        assert!(!ct_eq("abc", "abcd"));
        assert!(!ct_eq("abcd", "abc"));
        assert!(ct_eq("", ""));
        assert!(ct_eq("token", "token"));
        assert!(!ct_eq("token", "TOKEN"));
    }

    #[test]
    fn csv_escape_blocks_formula_injection() {
        // Excel/Sheets formula sentinels must be prefixed with ' and quoted.
        assert_eq!(csv_escape("=SUM(A1:A10)"), "\"'=SUM(A1:A10)\"");
        assert_eq!(csv_escape("+1234"), "\"'+1234\"");
        assert_eq!(csv_escape("-x"), "\"'-x\"");
        assert_eq!(csv_escape("@cmd"), "\"'@cmd\"");
        // Newline + carriage return both trigger quoting.
        assert_eq!(csv_escape("a\rb"), "\"a\rb\"");
        // Plain text passes through.
        assert_eq!(csv_escape("hello"), "hello");
    }
}

// ──────────────────────────────────────────────────────────────────────
// Phase 3.4 — Pyth rate refresh cron + Phase 3.7 — payment expiration sweep
// ──────────────────────────────────────────────────────────────────────
//
// Pyth REST endpoint:   https://hermes.pyth.network/api/latest_price_feeds
// Feed IDs (mainnet):
//   USD/JPY: 0xef2c98c804ba503c6a707e38be4dfbb16683775f195b091252bf24693042fd52
//   SOL/USD: 0xef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d
//   ETH/USD: 0xff61491a931112ddf1bd8147cd1b641375f79f5825126d665480874634fd0ace
//
// We compute:
//   JPY_PER_USD = 1 / (USD/JPY ↑ this is inverted in Pyth — see code)
//   JPY_PER_SOL = SOL/USD × JPY_PER_USD
//   JPY_PER_ETH = ETH/USD × JPY_PER_USD
//
// On any fetch / parse failure we don't write — the prior cached value
// (or env, or default) continues to be served.

const PYTH_USD_JPY_ID: &str = "ef2c98c804ba503c6a707e38be4dfbb16683775f195b091252bf24693042fd52";
const PYTH_SOL_USD_ID: &str = "ef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d";
const PYTH_ETH_USD_ID: &str = "ff61491a931112ddf1bd8147cd1b641375f79f5825126d665480874634fd0ace";

fn write_setting(db: &Db, key: &str, value: &str) {
    if let Ok(conn) = db.lock() {
        let _ = conn.execute(
            "INSERT INTO crypto_settings (key, value, updated_at) VALUES (?,?,?)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
            params![key, value, now_iso()],
        );
    }
}

// Pyth fetch failure noise filter — alert only after N consecutive fails so
// transient outages don't spam Telegram.
static PYTH_FAIL_STREAK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

async fn fetch_pyth_rates(db: Db) -> Result<(), String> {
    let ids = format!(
        "ids[]={}&ids[]={}&ids[]={}",
        PYTH_USD_JPY_ID, PYTH_SOL_USD_ID, PYTH_ETH_USD_ID
    );
    let url = format!("https://hermes.pyth.network/api/latest_price_feeds?{}", ids);
    let resp = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send().await.map_err(|e| format!("net: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("status {}", resp.status()));
    }
    let arr: serde_json::Value = resp.json().await.map_err(|e| format!("parse: {}", e))?;
    let arr = arr.as_array().ok_or("not array")?;

    let mut usd_jpy: Option<f64> = None;
    let mut sol_usd: Option<f64> = None;
    let mut eth_usd: Option<f64> = None;

    for feed in arr {
        let id = feed["id"].as_str().unwrap_or("");
        let price_s = feed["price"]["price"].as_str().unwrap_or("0");
        let expo: i32 = feed["price"]["expo"].as_i64().unwrap_or(0) as i32;
        let raw: f64 = price_s.parse().unwrap_or(0.0);
        let price = raw * 10f64.powi(expo);
        if !price.is_finite() || price <= 0.0 { continue; }
        if id.eq_ignore_ascii_case(PYTH_USD_JPY_ID) { usd_jpy = Some(price); }
        else if id.eq_ignore_ascii_case(PYTH_SOL_USD_ID) { sol_usd = Some(price); }
        else if id.eq_ignore_ascii_case(PYTH_ETH_USD_ID) { eth_usd = Some(price); }
    }

    let jpy_per_usd = usd_jpy.unwrap_or(JPY_PER_USD_DEFAULT);
    write_setting(&db, "JPY_PER_USD", &format!("{:.4}", jpy_per_usd));
    if let Some(s) = sol_usd {
        write_setting(&db, "JPY_PER_SOL", &format!("{:.4}", s * jpy_per_usd));
    }
    if let Some(e) = eth_usd {
        write_setting(&db, "JPY_PER_ETH", &format!("{:.4}", e * jpy_per_usd));
    }
    tracing::info!(
        "[rates] refreshed: JPY/USD={:.2} JPY/SOL={:?} JPY/ETH={:?}",
        jpy_per_usd,
        sol_usd.map(|s| s * jpy_per_usd),
        eth_usd.map(|e| e * jpy_per_usd),
    );
    Ok(())
}

async fn sweep_expired_pending(db: Db, ttl_min: i64) {
    let cutoff_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64).unwrap_or(0)
        - ttl_min * 60;
    let cutoff = cutoff_secs.to_string();
    if let Ok(conn) = db.lock() {
        let n = conn.execute(
            "UPDATE pending_crypto_payments
             SET status='expired'
             WHERE status='pending' AND CAST(created_at AS INTEGER) < ?",
            params![cutoff],
        ).unwrap_or(0);
        if n > 0 {
            tracing::info!("[sweep] expired {} pending crypto payment(s)", n);
        }
    }
}

/// Start the background cron tasks. Called once from main.rs after the
/// router DB is initialised. Idempotent on re-call (returns early).
pub fn start_crons(db: Db) {
    if CRON_DB.set(db.clone()).is_err() {
        // already started
        return;
    }
    // Pyth rate refresh — every 5 min. Fire once immediately at startup.
    let db1 = db.clone();
    tokio::spawn(async move {
        loop {
            match fetch_pyth_rates(db1.clone()).await {
                Ok(_) => {
                    PYTH_FAIL_STREAK.store(0, std::sync::atomic::Ordering::Relaxed);
                }
                Err(e) => {
                    let n = PYTH_FAIL_STREAK
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    tracing::warn!("[rates] fetch failed (streak={}): {}", n, e);
                    if n == 3 {
                        tg_alert(&format!(
                            "Pyth rate fetch failed 3 times in a row. Last error: {}. \
                             Rates will fall through to env vars / defaults.", e));
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        }
    });
    // Pending payment expiration sweep — every 5 min, TTL = CRYPTO_PAYMENT_TTL_MIN
    let db2 = db.clone();
    tokio::spawn(async move {
        // First tick after a short delay so server boot isn't bottlenecked.
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        loop {
            sweep_expired_pending(db2.clone(), CRYPTO_PAYMENT_TTL_MIN).await;
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        }
    });
}

// ──────────────────────────────────────────────────────────────────────
// Phase 3.5 — Alchemy ADDRESS_ACTIVITY webhook for ETH settlement
// ──────────────────────────────────────────────────────────────────────
//
// Auth: ALCHEMY_WEBHOOK_SIGNING_KEY → HMAC-SHA256 of body, sent in
//       `X-Alchemy-Signature` header. Constant-time compare.
// Match: Alchemy ADDRESS_ACTIVITY events carry { toAddress, value, hash }.
// We match the OLDEST pending ETH payment with our recipient where the
// credited ETH value is at least 95% of the expected amount_crypto.

pub async fn alchemy_webhook(
    State(db): State<Db>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    let key = env::var("ALCHEMY_WEBHOOK_SIGNING_KEY").unwrap_or_default();
    if key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "alchemy webhook key not set").into_response();
    }
    let sig = headers.get("x-alchemy-signature")
        .and_then(|h| h.to_str().ok()).unwrap_or("");
    // HMAC-SHA256(body)
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = match HmacSha256::new_from_slice(key.as_bytes()) {
        Ok(m) => m,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "key").into_response(),
    };
    mac.update(body.as_bytes());
    let expected_hex = hex::encode(mac.finalize().into_bytes());
    if !ct_eq(sig, &expected_hex) {
        return (StatusCode::UNAUTHORIZED, "sig").into_response();
    }

    let v: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "json").into_response(),
    };
    let mu_eth = env::var("MU_ETH_RECIPIENT").unwrap_or_default().to_ascii_lowercase();
    let activity = v["event"]["activity"].as_array().cloned().unwrap_or_default();

    let mut matched = 0usize;
    let mut skipped_no_recipient = 0usize;
    let mut skipped_no_match = 0usize;

    for ev in activity {
        let to = ev["toAddress"].as_str().unwrap_or("").to_ascii_lowercase();
        let from = ev["fromAddress"].as_str().unwrap_or("").to_ascii_lowercase();
        let value: f64 = ev["value"].as_f64().unwrap_or_else(|| {
            ev["value"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0)
        });
        let hash = ev["hash"].as_str().unwrap_or("").to_string();
        if to.is_empty() || hash.is_empty() || value <= 0.0 { continue; }
        if mu_eth.is_empty() || to != mu_eth {
            skipped_no_recipient += 1;
            continue;
        }

        // HARDENED for R4 (ETH order hijack via greedy oldest-pending match):
        //
        // 1. Match by `wallet` (sender address) WHEN KNOWN — closes the
        //    "attacker creates an older pending order at the same amount"
        //    attack. The buyer's wallet is captured at checkout_crypto time
        //    in pending_crypto_payments.wallet.
        // 2. Tolerance tightened from 95% to 99.5%. Gas-rounding + price-slip
        //    rarely costs >0.5%; a 5% slack made distinct-amount orders
        //    collide trivially.
        // 3. Reject when MULTIPLE candidate rows match the same amount — we
        //    refuse to choose rather than silently steal from the oldest.
        //    Operator can resolve manually via /admin/crypto-recon.
        let candidate: Option<(String, i64)> = {
            let conn = db.lock().unwrap();
            // First pass: wallet-bound match. Strictly the user who placed
            // this specific order from this specific sender address.
            let by_wallet: Option<(String, i64)> = if !from.is_empty() {
                conn.query_row(
                    "SELECT reference, product_id, amount_crypto
                     FROM pending_crypto_payments
                     WHERE status='pending' AND asset='ETH'
                       AND lower(recipient)=lower(?)
                       AND lower(COALESCE(wallet,''))=?
                     ORDER BY created_at ASC LIMIT 1",
                    params![mu_eth, from],
                    |r| Ok((
                        r.get::<_,String>(0)?, r.get::<_,i64>(1)?, r.get::<_,String>(2)?,
                    ))
                ).ok().and_then(|(reference, pid, exp)| {
                    let expected: f64 = exp.parse().unwrap_or(0.0);
                    if expected > 0.0 && value >= expected * 0.995 {
                        Some((reference, pid))
                    } else { None }
                })
            } else { None };

            if by_wallet.is_some() {
                by_wallet
            } else {
                // Fallback: amount-only match — but ONLY when exactly ONE
                // pending order is plausibly the recipient. If the amount
                // matches multiple orders we abstain (operator triage).
                let mut stmt = match conn.prepare(
                    "SELECT reference, product_id, amount_crypto
                     FROM pending_crypto_payments
                     WHERE status='pending' AND asset='ETH'
                       AND lower(recipient)=lower(?)
                     ORDER BY created_at ASC"
                ) { Ok(s) => s, Err(_) => continue };
                let mut matches: Vec<(String, i64)> = Vec::new();
                let rows = stmt.query_map(params![mu_eth], |r| Ok((
                    r.get::<_,String>(0)?, r.get::<_,i64>(1)?, r.get::<_,String>(2)?,
                )));
                if let Ok(it) = rows {
                    for row in it.flatten() {
                        let expected: f64 = row.2.parse().unwrap_or(0.0);
                        if expected > 0.0 && value >= expected * 0.995 {
                            matches.push((row.0, row.1));
                            if matches.len() > 1 { break; }
                        }
                    }
                }
                match matches.len() {
                    1 => Some(matches.into_iter().next().unwrap()),
                    n if n > 1 => {
                        tracing::warn!(
                            "[alchemy] ambiguous match: {} pending orders match value={} hash={} — abstaining",
                            n, value, hash);
                        None
                    }
                    _ => None,
                }
            }
        };

        let Some((reference, product_id)) = candidate else {
            skipped_no_match += 1;
            continue;
        };

        let confirmed = {
            let conn = db.lock().unwrap();
            let upd = conn.execute(
                "UPDATE pending_crypto_payments
                 SET status='confirmed', tx_signature=?, confirmed_at=?
                 WHERE reference=? AND status='pending'",
                params![hash, now_iso(), reference],
            ).unwrap_or(0);
            if upd > 0 {
                let _ = conn.execute(
                    "UPDATE products SET sold = sold + 1 WHERE id=?",
                    params![product_id]
                );
                matched += 1;
                true
            } else { false }
        };
        if confirmed {
            let db_clone = db.clone();
            let r2 = reference.clone();
            tokio::spawn(async move { fulfill_crypto_order(db_clone, r2).await; });
            tracing::info!(
                "[alchemy] confirmed ref={} product_id={} value={} hash={}",
                reference, product_id, value, hash
            );
        }
    }
    Json(serde_json::json!({
        "ok": true,
        "matched": matched,
        "skipped_no_recipient": skipped_no_recipient,
        "skipped_no_match": skipped_no_match,
    })).into_response()
}

// ──────────────────────────────────────────────────────────────────────
// Phase 3.6 — Stripe Identity webhook
// ──────────────────────────────────────────────────────────────────────
//
// Receives identity.verification_session.{verified,requires_input,canceled}
// events. The verification session was created by
// `create_stripe_identity_session` with metadata.kyc_record_id pointing at
// the kyc_records row, so we update that row's stripe_identity_* columns.

pub async fn stripe_identity_webhook(
    State(db): State<Db>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    let secret = env::var("STRIPE_IDENTITY_WEBHOOK_SECRET")
        .or_else(|_| env::var("STRIPE_WEBHOOK_SECRET"))
        .unwrap_or_default();
    if secret.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE,
            "STRIPE_IDENTITY_WEBHOOK_SECRET not set").into_response();
    }
    let sig_header = headers.get("stripe-signature")
        .and_then(|h| h.to_str().ok()).unwrap_or("");
    // Stripe signature: t=<unix>,v1=<hex>. Verify v1 against
    // HMAC-SHA256(timestamp.body) with the webhook secret.
    let mut ts: Option<&str> = None;
    let mut v1: Option<&str> = None;
    for kv in sig_header.split(',') {
        if let Some(rest) = kv.strip_prefix("t=") { ts = Some(rest); }
        else if let Some(rest) = kv.strip_prefix("v1=") { v1 = Some(rest); }
    }
    let (Some(ts), Some(v1)) = (ts, v1) else {
        return (StatusCode::BAD_REQUEST, "bad signature header").into_response();
    };
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "key").into_response(),
    };
    mac.update(ts.as_bytes());
    mac.update(b".");
    mac.update(body.as_bytes());
    let expected_hex = hex::encode(mac.finalize().into_bytes());
    if !ct_eq(v1, &expected_hex) {
        return (StatusCode::UNAUTHORIZED, "sig").into_response();
    }

    let event: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "json").into_response(),
    };
    let kind = event["type"].as_str().unwrap_or("");
    if !kind.starts_with("identity.verification_session.") {
        return Json(serde_json::json!({"ok": true, "skipped": kind})).into_response();
    }

    let session = &event["data"]["object"];
    let session_id = session["id"].as_str().unwrap_or("").to_string();
    let status = session["status"].as_str().unwrap_or("").to_string();
    let kyc_record_id: i64 = session["metadata"]["kyc_record_id"].as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| session["metadata"]["kyc_record_id"].as_i64())
        .unwrap_or(0);

    if kyc_record_id <= 0 {
        return (StatusCode::BAD_REQUEST, "metadata.kyc_record_id missing").into_response();
    }
    let conn = db.lock().unwrap();
    let _ = conn.execute(
        "UPDATE kyc_records
         SET stripe_identity_session_id=?, stripe_identity_status=?
         WHERE id=?",
        params![session_id, status, kyc_record_id],
    );
    tracing::info!(
        "[stripe-identity] kyc_record={} session={} status={}",
        kyc_record_id, session_id, status
    );
    Json(serde_json::json!({"ok": true})).into_response()
}
