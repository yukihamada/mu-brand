use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Json},
    routing::{get, patch, post},
    Router,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::{sync::{Arc, Mutex}, env};
use tower_http::services::ServeDir;

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
    let (base_price, current_bid, auction_end) = match row {
        Ok(r) => r,
        Err(_) => return (StatusCode::NOT_FOUND, "not found").into_response(),
    };
    let min_bid = current_bid.max(base_price) + 1000;
    if body.amount < min_bid {
        return (StatusCode::BAD_REQUEST,
            format!("最低入札額は¥{}です", min_bid)).into_response();
    }
    let now = chrono_now();
    conn.execute(
        "INSERT INTO bids (product_id, amount, email, wallet, created_at) VALUES (?,?,?,?,?)",
        params![body.product_id, body.amount, body.email, body.wallet, now]
    ).unwrap();
    conn.execute(
        "UPDATE products SET current_bid=?, bid_count=bid_count+1 WHERE id=?",
        params![body.amount, body.product_id]
    ).unwrap();
    Json(serde_json::json!({"ok": true})).into_response()
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
    // ── Signature verification ──
    if let Ok(secret) = env::var("STRIPE_WEBHOOK_SECRET") {
        let sig_header = headers
            .get("stripe-signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let timestamp = sig_header.split(',')
            .find(|s| s.starts_with("t="))
            .and_then(|s| s.strip_prefix("t="))
            .unwrap_or("");
        let expected = sig_header.split(',')
            .find(|s| s.starts_with("v1="))
            .and_then(|s| s.strip_prefix("v1="))
            .unwrap_or("");
        let signed_payload = format!("{}.{}", timestamp, body);
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .expect("HMAC init");
        mac.update(signed_payload.as_bytes());
        let computed = hex::encode(mac.finalize().into_bytes());
        if computed != expected {
            eprintln!("Stripe webhook signature mismatch");
            return StatusCode::UNAUTHORIZED.into_response();
        }
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
  <div style="font-size:9px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.4;margin-bottom:12px">LAST PIECE</div>
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
    let expected = env::var("ADMIN_TOKEN").unwrap_or_else(|_| "mu-admin".into());
    if q.get("token").map(|s| s.as_str()) != Some(expected.as_str()) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let product_id: i64 = match q.get("product_id").and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => return (StatusCode::BAD_REQUEST, "missing product_id").into_response(),
    };

    // Find highest bid
    let bid = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT b.amount, b.email, p.name, p.price_jpy FROM bids b
             JOIN products p ON p.id = b.product_id
             WHERE b.product_id=? ORDER BY b.amount DESC LIMIT 1",
            params![product_id],
            |row| Ok((row.get::<_,i64>(0)?, row.get::<_,String>(1)?,
                       row.get::<_,String>(2)?, row.get::<_,i64>(3)?))
        )
    };
    let (amount, email, product_name, _base_price) = match bid {
        Ok(r) => r,
        Err(_) => return (StatusCode::NOT_FOUND, "no bids found").into_response(),
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
    let html = format!(r#"
<div style="background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',Arial,sans-serif;padding:48px;max-width:560px;margin:0 auto">
  <div style="font-size:22px;font-weight:700;letter-spacing:0.4em;margin-bottom:32px">MU</div>
  <div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.4;margin-bottom:8px">間 MA — 落札のお知らせ</div>
  <div style="font-size:18px;font-weight:300;margin-bottom:24px">おめでとうございます。落札されました。</div>
  <div style="background:#1C1C1C;padding:24px;margin-bottom:24px">
    <div style="font-size:9px;opacity:0.4;letter-spacing:0.2em;text-transform:uppercase;margin-bottom:8px">落札金額</div>
    <div style="font-size:28px;font-weight:200">¥{amount}</div>
    <div style="font-size:10px;opacity:0.4;margin-top:8px">{product_name}</div>
  </div>
  <p style="font-size:12px;line-height:1.85;opacity:0.5;margin-bottom:24px">
    下記のボタンから決済をお願いします。<br>
    決済確認後、Printfulよりご自宅に発送します（約10〜14営業日）。<br>
    Soulbound NFT証明書は発送後にSolanaウォレットへ送付します。
  </p>
  <a href="{payment_url}" style="display:inline-block;background:#F5F5F0;color:#0A0A0A;padding:16px 32px;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;text-decoration:none;font-weight:600">決済する →</a>
  <div style="margin-top:48px;border-top:1px solid #1C1C1C;padding-top:20px;font-size:9px;opacity:0.25;letter-spacing:0.1em">
    MU — wearmu.com | mail@yukihamada.jp
  </div>
</div>
"#,
        amount = amount.to_string().chars().rev().collect::<Vec<_>>().chunks(3)
            .map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(",")
            .chars().rev().collect::<String>(),
        product_name = product_name,
        payment_url = payment_url,
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

    Json(serde_json::json!({"ok": true, "winner": email, "amount": amount, "payment_url": payment_url})).into_response()
}

async fn deactivate_product(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>,
) -> impl IntoResponse {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_else(|_| "mu-admin".into());
    if q.get("token").map(|s| s.as_str()) != Some(expected.as_str()) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let id: i64 = match q.get("product_id").and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => return (StatusCode::BAD_REQUEST, "missing product_id").into_response(),
    };
    let conn = db.lock().unwrap();
    conn.execute("UPDATE products SET active=0 WHERE id=?", params![id]).unwrap();
    Json(serde_json::json!({"ok": true, "id": id})).into_response()
}

async fn update_mockup(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>,
    Json(body): Json<UpdateMockupBody>,
) -> impl IntoResponse {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_else(|_| "mu-admin".into());
    if q.get("token").map(|s| s.as_str()) != Some(expected.as_str()) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let conn = db.lock().unwrap();
    conn.execute(
        "UPDATE products SET mockup_url=? WHERE id=?",
        params![body.mockup_url, body.product_id]
    ).unwrap();
    Json(serde_json::json!({"ok": true})).into_response()
}

async fn import_product(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>,
    Json(body): Json<ImportProductBody>,
) -> impl IntoResponse {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_else(|_| "mu-admin".into());
    if q.get("token").map(|s| s.as_str()) != Some(expected.as_str()) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let conn = db.lock().unwrap();
    let now = chrono_now();
    conn.execute(
        "INSERT OR IGNORE INTO products
         (brand, drop_num, name, design_url, mockup_url, price_jpy, inventory,
          created_at, weather_data, prompt_hash, seed_data, auction_end, nft_mint)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
        params![body.brand, body.drop_num, body.name, body.design_url, body.mockup_url,
                body.price_jpy, body.inventory, now, body.weather_data,
                body.prompt_hash, body.seed_data, body.auction_end, body.nft_mint]
    ).unwrap();
    Json(serde_json::json!({"ok": true})).into_response()
}

async fn update_price(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>,
    Json(body): Json<UpdatePriceBody>,
) -> impl IntoResponse {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_else(|_| "mu-admin".into());
    if q.get("token").map(|s| s.as_str()) != Some(expected.as_str()) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
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
    let expected = env::var("ADMIN_TOKEN").unwrap_or_else(|_| "mu-admin".into());
    if q.get("token").map(|s| s.as_str()) != Some(expected.as_str()) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
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
    let expected = env::var("ADMIN_TOKEN").unwrap_or_else(|_| "mu-admin".into());
    if q.get("token").map(|s| s.as_str()) != Some(expected.as_str()) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
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
    let expected = env::var("ADMIN_TOKEN").unwrap_or_else(|_| "mu-admin".into());
    if q.get("token").map(|s| s.as_str()) != Some(expected.as_str()) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
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
.brand-tag{{font-size:9px;letter-spacing:0.35em;text-transform:uppercase;opacity:0.4;margin-bottom:6px}}
.name{{font-size:22px;font-weight:300;letter-spacing:0.02em;margin-bottom:4px}}
.drop{{font-size:12px;opacity:0.4;letter-spacing:0.15em;margin-bottom:28px}}
.section-label{{font-size:8px;letter-spacing:0.35em;text-transform:uppercase;opacity:0.3;margin-bottom:12px;margin-top:28px}}
.row{{display:flex;justify-content:space-between;align-items:baseline;padding:10px 0;border-bottom:1px solid #1C1C1C;font-size:12px}}
.label{{opacity:0.4;letter-spacing:0.05em}}
.val{{font-weight:300;text-align:right;max-width:60%}}
.mono{{font-family:monospace;font-size:11px}}
.inventory{{display:flex;align-items:center;gap:6px}}
.inv-bar{{flex:1;height:2px;background:#1C1C1C;border-radius:1px;overflow:hidden}}
.inv-fill{{height:100%;background:#F5F5F0;border-radius:1px}}
.cta{{margin-top:32px;text-align:center}}
.cta a{{display:inline-block;border:1px solid #333;color:#F5F5F0;font-size:9px;letter-spacing:0.35em;text-transform:uppercase;padding:14px 28px;text-decoration:none}}
.cta a:hover{{background:#1C1C1C}}
.hokkaido{{margin-top:28px;font-size:10px;opacity:0.3;line-height:1.8;letter-spacing:0.05em}}
</style>
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

async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

async fn tokushoho_page() -> Html<&'static str> {
    Html(include_str!("../static/tokushoho.html"))
}

async fn city_page() -> Html<&'static str> {
    Html(include_str!("../static/city.html"))
}

async fn success_page() -> Html<&'static str> {
    Html(r#"<!DOCTYPE html><html><head><meta charset=UTF-8><style>
    body{background:#0A0A0A;color:#F5F5F0;font-family:'Helvetica Neue',sans-serif;
    display:flex;align-items:center;justify-content:center;height:100vh;flex-direction:column;gap:20px}
    h1{font-size:14px;letter-spacing:0.4em;text-transform:uppercase;font-weight:300;opacity:0.6}
    p{font-size:11px;opacity:0.3;letter-spacing:0.1em}
    a{color:inherit;font-size:9px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.3;margin-top:40px}
    </style></head><body>
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
  <div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.4;margin-bottom:8px">Fragment System</div>
  <div style="font-size:18px;font-weight:300;letter-spacing:0.05em;margin-bottom:24px">申請を受け付けました</div>
  <div style="background:#1C1C1C;padding:24px;margin-bottom:24px">
    <div style="font-size:9px;letter-spacing:0.25em;text-transform:uppercase;opacity:0.4;margin-bottom:8px">Exchange</div>
    <div style="font-size:14px">{direction_ja}</div>
    <div style="font-size:9px;opacity:0.4;margin-top:8px">注文番号: {order_ids}</div>
  </div>
  <p style="font-size:12px;line-height:1.85;opacity:0.5">
    担当者が注文を確認し、2営業日以内に返送先住所をこのメールにご返信します。<br>
    着払いで3着を返送してください。確認後、{result_item}をお送りします。<br><br>
    交換品の送料はMU負担です。申請から発送まで約2週間を予定しています。
  </p>
  <div style="margin-top:32px;padding-top:20px;border-top:1px solid #1C1C1C;font-size:9px;opacity:0.25;letter-spacing:0.1em">
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

#[tokio::main]
async fn main() {
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
    ] {
        conn.execute(col, []).ok();
    }
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
        .route("/api/admin/deactivate", post(deactivate_product))
        .route("/api/admin/settle-auction", post(settle_auction))
        .route("/api/nft/:brand/:drop_num", get(nft_metadata))
        .route("/api/fragment", post(fragment_request))
        .route("/v/:brand/:drop_num", get(verify_page))
        .route("/tokushoho", get(tokushoho_page))
        .route("/tokushoho.html", get(tokushoho_page))
        .route("/city", get(city_page))
        .route("/city.html", get(city_page))
        .nest_service("/static", ServeDir::new("static"))
        .fallback_service(ServeDir::new("static"))
        .with_state(db);

    let port = env::var("PORT").unwrap_or_else(|_| "3000".into());
    let addr = format!("0.0.0.0:{}", port);
    println!("mu-store listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
