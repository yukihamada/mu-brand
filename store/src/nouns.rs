// nouns.rs — /nouns: あなたのNounを、着る。 Wear your Noun.
//
// Nouns DAO art is CC0. An owner (or anyone — it's public domain) enters a
// Noun ID; we fetch the art from noun.pics, upscale it pixel-perfect
// (NEAREST ×4, pixel art must never be smoothed), persist it to R2, and
// insert a live catalog SKU under brand 'nouns'. Checkout then reuses the
// two existing rails unchanged:
//
//   card   → GET /api/shop/checkout?sku=NOUNS-…&qty=N   (Stripe; bulk via
//            adjustable_quantity, see shop_checkout's nouns branch)
//   crypto → POST /api/checkout/crypto {catalog_sku: "NOUNS-…", …}
//            (Solana Pay USDC/SOL + ETH EIP-681, payments.rs)
//
// Contract compliance (docs/CATALOG_CONTRACT.md): brand = one
// catalog_brands row, products = catalog_products rows, images =
// design_file/mockup columns. No new tables; no new catalog columns.
// 10% of nouns-brand revenue is pledged to the Nouns Treasury
// (catalog_brands.revenue_share_pct=10; settled manually for now).

use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    Json,
};
use rusqlite::params;

use crate::Db;

/// kind → (SKU token, Printful product, base variant = Black/M, retail ¥).
/// Black bodies only: resolve_size_variant() maps sizes to BLACK variants
/// for 71/146, so a white-body kind here would silently ship black once a
/// buyer picks a size. Add white only together with a white-aware resolver.
const NOUNS_KINDS: &[(&str, &str, i64, i64, i64)] = &[
    ("tee", "TEE", 71, 4017, 4_900),
    ("hoodie", "HOODIE", 146, 5531, 8_800),
];

/// Global creation cap per rolling hour — /api/nouns/create is
/// unauthenticated (CC0 art, no login), so bound the R2/mockup burn.
const NOUNS_CREATES_PER_HOUR: i64 = 60;

pub async fn nouns_page() -> Html<&'static str> {
    Html(include_str!("../static/nouns.html"))
}

#[derive(serde::Deserialize)]
pub struct NounsCreateBody {
    pub noun_id: u32,
    pub kind: String,
}

/// POST /api/nouns/create {noun_id, kind} → {sku, design_url, shop_url, …}
///
/// Idempotent per (noun_id, kind): the SKU is deterministic
/// (NOUNS-<KIND>-<id>), so re-submitting returns the existing product
/// instead of minting duplicates.
pub async fn nouns_create(
    State(db): State<Db>,
    Json(body): Json<NounsCreateBody>,
) -> Response {
    let Some(&(kind, sku_kind, pp_id, pv_id, retail_jpy)) =
        NOUNS_KINDS.iter().find(|k| k.0 == body.kind)
    else {
        let allowed: Vec<&str> = NOUNS_KINDS.iter().map(|k| k.0).collect();
        return (
            StatusCode::BAD_REQUEST,
            format!("unknown kind; allowed: {}", allowed.join("/")),
        )
            .into_response();
    };
    // Nouns mints one per day since 2021 — six digits is decades of headroom,
    // and it keeps junk IDs from burning noun.pics fetches.
    if body.noun_id > 999_999 {
        return (StatusCode::BAD_REQUEST, "noun_id out of range").into_response();
    }

    let sku = format!("NOUNS-{}-{}", sku_kind, body.noun_id);

    // Idempotency: existing SKU → return it as-is (no refetch, no R2 write).
    let existing: Option<(String, String)> = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(design_file,''), COALESCE(mockup_url_external, mockup_main_file, '')
             FROM catalog_products WHERE sku=?",
            params![&sku],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()
    };
    if let Some((design_url, mockup_url)) = existing {
        return Json(serde_json::json!({
            "ok": true,
            "sku": sku,
            "existing": true,
            "design_url": design_url,
            "mockup_url": mockup_url,
            "shop_url": format!("/shop/{}", sku),
            "price_jpy": retail_jpy,
        }))
        .into_response();
    }

    // Rate limit (global, rolling hour) — checked only on the create path.
    let recent: i64 = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM catalog_products
             WHERE legacy_source='nouns_page'
               AND created_at > datetime('now','-1 hour')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };
    if recent >= NOUNS_CREATES_PER_HOUR {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "too many new Nouns this hour — try again soon",
        )
            .into_response();
    }

    // Fetch the Noun art. noun.pics serves the canonical on-chain render as
    // a 320×320 PNG; an unknown ID returns a non-200 (observed: 500).
    let src_url = format!("https://noun.pics/{}.png", body.noun_id);
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, format!("client: {}", e)).into_response()
        }
    };
    let resp = match client.get(&src_url).send().await {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            return (
                StatusCode::NOT_FOUND,
                format!("Noun {} not found upstream ({})", body.noun_id, r.status()),
            )
                .into_response();
        }
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, format!("noun.pics: {}", e)).into_response()
        }
    };
    let bytes = match resp.bytes().await {
        Ok(b) if b.len() <= 1_000_000 => b,
        Ok(_) => return (StatusCode::BAD_GATEWAY, "noun image too large").into_response(),
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, format!("noun.pics body: {}", e)).into_response()
        }
    };

    // Pixel-perfect upscale: 320 → 1280 with NEAREST (crisp pixel art; any
    // smoothing filter would destroy it). 1280px front print matches the
    // resolution the Gemini design pipeline already ships at.
    let img = match image::load_from_memory(&bytes) {
        Ok(i) => i,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, format!("decode: {}", e)).into_response()
        }
    };
    let up = img.resize_exact(1280, 1280, image::imageops::FilterType::Nearest);
    let mut png_buf: Vec<u8> = Vec::new();
    if let Err(e) = up.write_to(
        &mut std::io::Cursor::new(&mut png_buf),
        image::ImageFormat::Png,
    ) {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("encode: {}", e)).into_response();
    }

    // Persist to R2 — one design per Noun, shared across kinds.
    let r2_key = format!("catalog/nouns/noun-{}-1280.png", body.noun_id);
    let Some(design_url) = crate::store_r2_bytes(&r2_key, &png_buf, "image/png").await else {
        return (StatusCode::SERVICE_UNAVAILABLE, "image storage unavailable").into_response();
    };

    let label = format!("Noun {} ⌐◨-◨ {}", body.noun_id, kind);
    let desc = format!(
        "Noun {} を、着る。CC0 on-chain art, printed on demand — \
         pixel-perfect ⌐◨-◨ on a black {}. 10% pledged to the Nouns Treasury.",
        body.noun_id,
        if kind == "tee" { "Bella+Canvas 3001 tee" } else { "Gildan 18500 hoodie" },
    );
    {
        let conn = db.lock().unwrap();
        // Brand row (single INSERT per the catalog contract; idempotent).
        let _ = conn.execute(
            "INSERT OR IGNORE INTO catalog_brands (slug, name, emoji, color_primary, tagline, is_active, revenue_share_pct)
             VALUES ('nouns', 'NOUNS × MU', '⌐◨-◨', '#d53c5e', 'あなたのNounを、着る。 Wear your Noun — CC0, one of one.', 1, 10)",
            [],
        );
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO catalog_products (
                sku, brand, label, description_ja, retail_price_jpy,
                printful_product_id, printful_variant_id, printful_placement,
                printful_print_w, printful_print_h,
                design_file, mockup_main_file, mockup_url_external,
                is_active, sort_order, status, fulfillment_route, legacy_source, meta_json
             ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                &sku, "nouns", &label, &desc, retail_jpy,
                pp_id, pv_id, "front",
                0, 0,
                &design_url, &design_url, &design_url,
                1, 60, "live", "printful_dtg", "nouns_page",
                format!("{{\"noun_id\":{}}}", body.noun_id),
            ],
        );
        if let Err(e) = inserted {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("insert failed: {}", e),
            )
                .into_response();
        }
    }

    // Real Printful on-body mockup, in the background (it polls up to ~4 min).
    // The page shows the flat design immediately and the PDP upgrades itself
    // once mockup_url_external lands.
    {
        let db_c = db.clone();
        let sku_c = sku.clone();
        let design_c = design_url.clone();
        tokio::spawn(async move {
            if let Err(e) =
                crate::catalog::generate_onbody_mockup(db_c, sku_c.clone(), pp_id, pv_id, design_c)
                    .await
            {
                tracing::warn!("[nouns] mockup failed for {}: {}", sku_c, e);
            }
        });
    }

    Json(serde_json::json!({
        "ok": true,
        "sku": sku,
        "existing": false,
        "design_url": design_url,
        "mockup_url": "",
        "shop_url": format!("/shop/{}", sku),
        "price_jpy": retail_jpy,
    }))
    .into_response()
}
