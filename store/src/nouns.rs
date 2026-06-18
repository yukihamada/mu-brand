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
    extract::{Query, State},
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

#[derive(serde::Deserialize)]
pub struct NounsPageQuery {
    /// `?noun=372` — dynamic OG card + input prefill, so a shared link
    /// previews the sharer's own Noun instead of a hardcoded Noun 0
    /// (persona FB: OGP固定はシェア拡散の自殺).
    #[serde(default)]
    pub noun: Option<u32>,
}

pub async fn nouns_page(Query(q): Query<NounsPageQuery>) -> Html<String> {
    let base = include_str!("../static/nouns.html");
    let Some(id) = q.noun.filter(|n| *n <= 999_999) else {
        return Html(base.to_string());
    };
    // Swap every Noun-0 reference (og:image + inline preview) for the shared
    // Noun, point og:url at the parameterized URL, and hand the id to the
    // page JS for input prefill. `id` is a validated u32 — no escaping needed.
    let html = base
        .replace(
            "https://noun.pics/0.png",
            &format!("https://noun.pics/{}.png", id),
        )
        .replace(
            "content=\"https://wearmu.com/nouns\"",
            &format!("content=\"https://wearmu.com/nouns?noun={}\"", id),
        )
        .replace(
            "</head>",
            &format!("<script>window.__noun={};</script></head>", id),
        );
    Html(html)
}

#[derive(serde::Deserialize)]
pub struct NounsCreateBody {
    pub noun_id: u32,
    pub kind: String,
    /// Knock out the Noun's flat background color → the character floats
    /// on the black garment instead of sitting in a colored square.
    #[serde(default)]
    pub transparent: bool,
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

    let sku = format!(
        "NOUNS-{}-{}{}",
        sku_kind,
        body.noun_id,
        if body.transparent { "-NB" } else { "" },
    );

    let kind_cap = if kind == "tee" { "Tee" } else { "Hoodie" };
    let style_tag = if body.transparent { " · Floating" } else { "" };
    let label = format!("Noun {} ⌐◨-◨ {}{}", body.noun_id, kind_cap, style_tag);
    // Short, human title — the PDP renders description_ja as its headline,
    // so a spec dump here reads as machine output (persona FB). Specs live
    // in the PDP's own SPEC section.
    let desc = format!(
        "Noun {} {}{} — あなたのNounを、着る。CC0 · printed on demand",
        body.noun_id, kind_cap, style_tag,
    );

    // Idempotency: existing SKU → return it as-is (no refetch, no R2 write).
    // Self-heal: refresh label/description so already-minted rows pick up
    // copy fixes on their next touch instead of needing a manual migration.
    let existing: Option<(String, String)> = {
        let conn = db.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT COALESCE(design_file,''), COALESCE(mockup_url_external, mockup_main_file, '')
                 FROM catalog_products WHERE sku=?",
                params![&sku],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();
        if row.is_some() {
            let _ = conn.execute(
                "UPDATE catalog_products SET label=?, description_ja=?
                 WHERE sku=? AND legacy_source='nouns_page'",
                params![&label, &desc, &sku],
            );
        }
        row
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
    let mut rgba = img.to_rgba8();
    if body.transparent {
        // Knock out the flat background. Nouns renders use a single solid
        // bg color (cool #d5d7e1 / warm #e1d7d5) — sample the corner pixel
        // and clear everything within a small tolerance (pixel art is flat,
        // so this is exact in practice; tolerance absorbs PNG quantization).
        let bg = *rgba.get_pixel(0, 0);
        const TOL: i16 = 8;
        for p in rgba.pixels_mut() {
            let d = (p.0[0] as i16 - bg.0[0] as i16)
                .abs()
                .max((p.0[1] as i16 - bg.0[1] as i16).abs())
                .max((p.0[2] as i16 - bg.0[2] as i16).abs());
            if d <= TOL {
                p.0 = [0, 0, 0, 0];
            }
        }
    }
    let up = image::DynamicImage::ImageRgba8(rgba)
        .resize_exact(1280, 1280, image::imageops::FilterType::Nearest);
    let mut png_buf: Vec<u8> = Vec::new();
    if let Err(e) = up.write_to(
        &mut std::io::Cursor::new(&mut png_buf),
        image::ImageFormat::Png,
    ) {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("encode: {}", e)).into_response();
    }

    // Persist to R2 — one design per (Noun, style), shared across kinds.
    let r2_key = format!(
        "catalog/nouns/noun-{}{}-1280.png",
        body.noun_id,
        if body.transparent { "-nb" } else { "" },
    );
    let Some(design_url) = crate::store_r2_bytes(&r2_key, &png_buf, "image/png").await else {
        return (StatusCode::SERVICE_UNAVAILABLE, "image storage unavailable").into_response();
    };

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
                format!("{{\"noun_id\":{},\"transparent\":{}}}", body.noun_id, body.transparent),
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
