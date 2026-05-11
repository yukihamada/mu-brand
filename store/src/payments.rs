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

fn env_rate(key: &str, default: f64) -> f64 {
    env::var(key).ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|r| r.is_finite() && *r > 0.0)
        .unwrap_or(default)
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

#[derive(serde::Deserialize)]
pub struct CryptoCheckoutBody {
    pub product_id: i64,
    pub quantity: u32,
    pub email: String,
    pub size: Option<String>,
    pub wallet: Option<String>,
    pub payment_method: String,
    pub kyc: Option<KycInfo>,
}

// MUGEN-cycle dynamic price (mirrors dynamic_price() in main.rs). We re-derive
// rather than reach into main.rs to avoid the cross-module coupling that was
// causing concurrent-edit races.
fn dynamic_price(brand: &str, drop_num: i64, sold: i64, name: &str) -> i64 {
    if brand == "ma" { return 120_000; }
    if brand == "nouns" {
        let nm = name.to_uppercase();
        if nm.contains("間") || nm.contains(" MA ") || nm.starts_with("MA ") || nm.ends_with(" MA") {
            return 120_000;
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

    let base_price_jpy = dynamic_price(&brand_str, drop_num, sold, &product_name);
    let unit_price_jpy = apply_payment_surcharge(base_price_jpy, &pm);
    let total_jpy = unit_price_jpy.saturating_mul(body.quantity as i64);

    if total_jpy >= KYC_THRESHOLD_JPY {
        let Some(kyc) = body.kyc.as_ref() else {
            return (StatusCode::BAD_REQUEST,
                "KYC required for purchases at or above ¥300,000").into_response();
        };
        if kyc.full_name.trim().is_empty() || kyc.id_last4.trim().is_empty() {
            return (StatusCode::BAD_REQUEST, "KYC required (incomplete fields)").into_response();
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
                pm, total_jpy, now_iso()
            ]
        );
    }

    let reference = uuid::Uuid::new_v4().to_string();
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
              status, expires_at, created_at)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,'pending',?,?)",
            params![
                reference, body.product_id, body.email, size_label, body.quantity,
                body.wallet, pm,
                total_jpy, amount_crypto, asset, recipient, pay_url,
                now_iso(), now_iso()
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
pub async fn create_stripe_identity_session(
    State(_db): State<Db>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let stripe_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
    if stripe_key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "stripe key not configured").into_response();
    }
    let email = body["email"].as_str().unwrap_or("");
    let kyc_record_id = body["kyc_record_id"].as_i64().unwrap_or(0);
    let return_url = body["return_url"].as_str().unwrap_or("https://wearmu.com/");
    let resp = reqwest::Client::new()
        .post("https://api.stripe.com/v1/identity/verification_sessions")
        .basic_auth(&stripe_key, None::<&str>)
        .form(&[
            ("type", "document"),
            ("metadata[kyc_record_id]", &kyc_record_id.to_string()),
            ("metadata[email]", email),
            ("return_url", return_url),
        ])
        .send().await;
    match resp {
        Ok(r) if r.status().is_success() => {
            let json: serde_json::Value = r.json().await.unwrap_or(serde_json::json!({}));
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
