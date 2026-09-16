//! Purchase-first storefront. Prices and availability come from the live catalog.
use axum::{extract::State, response::{Html, IntoResponse, Response}};
use rusqlite::Connection;
use crate::{Db, html_attr_escape, format_jpy, mockup_thumb_url};

pub const APP_STORE_URL: &str = "https://apps.apple.com/app/id6781269252";

pub fn app_banner(lang: &str) -> String {
    let (note, cta) = if lang == "en" {
        ("Create your own in the MU app · iPhone / iPad", "Download on the App Store")
    } else {
        ("自分の一着は、MUアプリで。iPhone / iPad 対応", "App Storeからダウンロード")
    };
    format!(r#"<aside class="shop-create"><span>{note}</span><a href="{APP_STORE_URL}" data-funnel="cta_click" data-funnel-cta="renewal_catalog_app">{cta} ↗</a></aside>"#)
}

struct Pick {
    sku: String,
    label: String,
    brand: String,
    image: String,
    price: i64,
}

fn picks(conn: &Connection) -> rusqlite::Result<Vec<Pick>> {
    // Stable editorial order, not a claim about best sellers. One design per
    // brand/label avoids filling the first screen with colour variants.
    let mut stmt = conn.prepare(
        "WITH candidates AS (
           SELECT p.sku, p.label, b.name, p.mockup_url_external, p.retail_price_jpy,
                  p.brand, p.sort_order,
                  ROW_NUMBER() OVER (PARTITION BY p.brand, p.label ORDER BY
                    (UPPER(p.sku) LIKE '%TEE%') DESC,
                    (UPPER(p.sku) LIKE '%TEE-BLACK') DESC, p.sort_order, p.sku) AS variant
           FROM catalog_products p JOIN catalog_brands b ON b.slug=p.brand
           WHERE p.status='live' AND p.is_active=1 AND b.is_active=1
             AND p.retail_price_jpy>0 AND p.sku NOT LIKE 'PROPOSAL-%'
             AND (UPPER(p.sku) LIKE '%TEE%' OR UPPER(p.sku) LIKE '%-RASH%'
                  OR UPPER(p.sku) LIKE '%HOODIE%' OR UPPER(p.sku) LIKE '%CREWNECK%')
             AND p.mockup_url_external LIKE 'https://%'
             AND p.mockup_url_external NOT LIKE 'https://printful-upload.s3%'
             AND p.mockup_url_external NOT LIKE '%/tmp/%'
         ) SELECT sku,label,name,mockup_url_external,retail_price_jpy
           FROM candidates WHERE variant=1
           ORDER BY (brand IN ('bjj','jiuflow','tatami')) DESC,
                    (UPPER(sku) LIKE '%TEE%') DESC, sort_order, sku LIMIT 8"
    )?;
    let rows = stmt.query_map([], |r| Ok(Pick {
        sku: r.get(0)?, label: r.get(1)?, brand: r.get(2)?, image: r.get(3)?, price: r.get(4)?,
    }))?.collect();
    rows
}

fn product_link(p: &Pick, lang: &str) -> String {
    format!("/shop/{}?lang={}", urlencoding::encode(&p.sku), lang)
}

fn render(items: &[Pick], en: bool) -> String {
    let lang = if en { "en" } else { "ja" };
    let t = |ja, english| if en { english } else { ja };
    let mut html = include_str!("../static/storefront.html").to_string();
    for (key, ja, english) in [
        ("title", "MU — 好きなことを、着ていこう。", "MU — Wear what you love."),
        ("description", "柔術のある毎日に、好きな一着を。Tシャツ、ラッシュガード、コラボウェアを1着から受注生産。自分のアイデアから作ることもできます。", "Tees, rashguards and collaborations for life on and off the mats. Made to order from one piece. Or create something of your own."),
        ("skip", "商品を見る", "Skip to products"),
        ("shop", "商品を探す", "Shop"),
        ("make", "アプリでつくる", "Create in app"),
        ("help", "ご利用ガイド", "Buying guide"),
        ("eyebrow", "ON THE MAT. OFF THE MAT.", "ON THE MAT. OFF THE MAT."),
        ("headline", "好きなことを、<br>着ていこう。", "Wear what<br>you love."),
        ("intro", "道場で交わす「それ、いいね」。<br>柔術のある毎日に、あなたらしい一着を。", "A nod from someone who gets it.<br>Find your next favourite, on and off the mats."),
        ("primary", "柔術のコレクションを見る", "Explore the BJJ collection"),
        ("all", "すべての商品を見る", "Shop all products"),
        ("made", "1着から、受注生産", "Made to order, from one piece"),
        ("hero_caption", "PRODUCT PREVIEW / 商品イメージ", "PRODUCT PREVIEW"),
        ("detail", "商品を見る", "View product"),
        ("trust1", "注文を受けてからつくります", "Made when you order"),
        ("trust2", "送料・合計はお支払い前に確認", "Review shipping & total before payment"),
        ("trust3", "サイズ・返品条件を事前に確認", "Check sizing & returns before you buy"),
        ("categories", "今日は、何を着よう。", "Find your everyday favourite."),
        ("tee", "Tシャツ", "T-shirts"),
        ("tee_note", "練習のあとも、休日も。", "After training. All weekend."),
        ("rash", "ラッシュガード", "Rashguards"),
        ("rash_note", "マットの上で、自分らしく。", "Make the mats your own."),
        ("hoodie", "パーカー・スウェット", "Hoodies & sweatshirts"),
        ("hoodie_note", "道場までの道も、心地よく。", "Comfort for the way there."),
        ("selection", "まずは、この一着から。", "A good place to start."),
        ("selection_note", "MUのコレクションから。気になるデザインを選んで、仕様・サイズを確認。", "Discover the collection. Pick a design, then check its details and sizing."),
        ("tax", "税込・送料別", "JPY · tax included, shipping extra"),
        ("create_head", "そのアイデアも、<br>一着になる。", "Your idea.<br>Your next favourite."),
        ("create_note", "道場の名前、得意技、大切な仲間。<br>MUアプリで、言葉や声からオリジナルのデザインへ。<br>デザインと価格を確認してから、購入できます。", "Your dojo. Your signature move. Your people.<br>Start with words or your voice in the MU app.<br>Review the design and price before buying."),
        ("create_cta", "App Storeからダウンロード", "Download on the App Store"),
        ("app_devices", "iPhone / iPad 対応", "For iPhone and iPad"),
        ("guide_head", "はじめてのMU。", "Your first MU."),
        ("guide_note", "買う前に、知っておきたいこと。", "A few things to know before you order."),
        ("q1", "いつ届きますか？ 送料は？", "When will it arrive? What does shipping cost?"),
        ("a1", "受注生産のため、製造と配送にお時間をいただきます。日数は商品・お届け先で異なります。送料と合計金額はお支払い前の画面でご確認ください。お急ぎの際は、ご注文前にお問い合わせください。", "Production and shipping take time. Timing varies by product and destination. Review the shipping charge and total before payment. Please contact us before ordering if you have a deadline."),
        ("q2", "サイズはどう選べばいいですか？", "How do I choose a size?"),
        ("a2", "商品詳細のサイズ表を、お手持ちの服の実寸と比べてお選びください。サイズ感を理由にした返品・交換は対象外のため、ご注文前の確認をお願いします。", "Compare the size chart on the product page with a garment you own. Returns or exchanges for fit preference are not covered, so please check before ordering."),
        ("q3", "返品・交換はできますか？", "Can I return or exchange my order?"),
        ("a3", "印刷不良・破損・注文と異なる商品が届いた場合は、到着後30日以内にご連絡ください。条件を確認のうえ、交換または返金で対応します。お客様都合の返品は対象外です。", "For print defects, damage or an item different from your order, contact us within 30 days of delivery. Eligible issues are resolved with an exchange or refund. Change-of-mind returns are not covered."),
        ("q4", "自分のデザインでもつくれますか？", "Can I create my own design?"),
        ("a4", "MUアプリをApp Storeからダウンロードして、言葉や声でデザインを始められます。iPhone・iPadに対応。商品を購入する前に、デザイン・価格・仕様をご確認いただけます。", "Download the MU app from the App Store to start designing with words or your voice. Available for iPhone and iPad. Review the design, price and specifications before purchasing."),
        ("shipping", "配送について", "Shipping details"),
        ("returns", "返品条件", "Returns policy"),
        ("contact", "お問い合わせ", "Contact"),
        ("story", "MUのものづくり", "The story of MU"),
        ("story_note", "アイデアを、日常で使うものへ。注文が生まれてから、ひとつずつ。", "Ideas become things you live in. Made one by one, when you order."),
        ("drops", "毎日生まれるデザイン", "Discover the daily drops"),
        ("legal", "特定商取引法に基づく表記", "Seller information"),
        ("privacy", "プライバシー", "Privacy"),
        ("empty", "コレクションは商品一覧からご覧いただけます。", "Explore the collection in our shop."),
    ] {
        html = html.replace(&format!("{{{{{key}}}}}"), t(ja, english));
    }
    let cards = items.iter().enumerate().map(|(i, p)| format!(
        r#"<a class="product" href="{href}" data-funnel="cta_click" data-funnel-cta="renewal_product" data-funnel-view="renewal_product" data-funnel-pos="{i}"><div class="product-image"><img src="{image}" data-fallback="{fallback}" alt="{name}" width="480" height="480" loading="lazy" decoding="async"></div><div class="product-info"><span class="brand-name">{brand}</span><h3>{name}</h3><div class="product-price">¥{price}<span>↗</span></div></div></a>"#,
        href=html_attr_escape(&product_link(p, lang)), image=html_attr_escape(&mockup_thumb_url(&p.image, 480)),
        fallback=html_attr_escape(&p.image), name=html_attr_escape(&p.label), brand=html_attr_escape(&p.brand), price=format_jpy(p.price),
    )).collect::<String>();
    let hero = items.first().map(|p| format!(
        r#"<a class="hero-product" href="{href}" data-funnel="cta_click" data-funnel-cta="renewal_hero_product"><img src="{image}" data-fallback="{fallback}" alt="{label}" width="640" height="640" fetchpriority="high"><div class="hero-product-label"><span>{label}</span><strong>¥{price} ↗</strong></div></a>"#,
        href=html_attr_escape(&product_link(p, lang)), image=html_attr_escape(&mockup_thumb_url(&p.image, 960)),
        fallback=html_attr_escape(&p.image), label=html_attr_escape(&p.label), price=format_jpy(p.price),
    )).unwrap_or_default();
    html.replace("{{lang}}", lang)
        .replace("{{app_url}}", APP_STORE_URL)
        .replace("{{other_lang}}", if en { "ja" } else { "en" })
        .replace("{{other_label}}", if en { "日本語" } else { "EN" })
        .replace("{{canonical}}", if en { "https://wearmu.com/?lang=en" } else { "https://wearmu.com/" })
        .replace("{{hero_product}}", &hero)
        .replace("{{cards}}", &cards)
        .replace("{{empty_hidden}}", if items.is_empty() { "" } else { "hidden" })
}

pub async fn home(State(db): State<Db>, en: bool) -> Response {
    let items = match picks(&db.lock().unwrap()) {
        Ok(items) => items,
        Err(error) => { tracing::error!(%error, "storefront catalog unavailable"); Vec::new() }
    };
    let mut response = Html(render(&items, en)).into_response();
    response.headers_mut().insert("Content-Security-Policy", axum::http::HeaderValue::from_static(
        "default-src 'self'; base-uri 'self'; object-src 'none'; script-src 'self' https://enabler-analytics.fly.dev; style-src 'self'; img-src 'self' data: https:; connect-src 'self' https:; frame-ancestors 'self'; form-action 'self'"
    ));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_is_live_stable_and_deduplicates_variants() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE catalog_brands(slug TEXT,name TEXT,is_active INTEGER);
            CREATE TABLE catalog_products(sku TEXT,label TEXT,brand TEXT,mockup_url_external TEXT,
                retail_price_jpy INTEGER,sort_order INTEGER,status TEXT,is_active INTEGER);
            INSERT INTO catalog_brands VALUES('bjj','BJJ',1),('hidden','Hidden',0);
            INSERT INTO catalog_products VALUES
              ('BJJ-TEE-1','Tap','bjj','https://wearmu.com/tee.jpg',4800,1,'live',1),
              ('BJJ-TEE-2','Tap','bjj','https://wearmu.com/tee-white.jpg',4800,2,'live',1),
              ('BJJ-TEE-DRAFT','Draft','bjj','https://wearmu.com/draft.jpg',4800,0,'draft',1),
              ('BJJ-TEE-INACTIVE','Inactive','bjj','https://wearmu.com/inactive.jpg',4800,0,'live',0),
              ('HIDDEN-TEE','Hidden','hidden','https://wearmu.com/hidden.jpg',4800,0,'live',1),
              ('BJJ-TEE-TEMP','Temporary','bjj','https://printful-upload.s3.com/a.jpg',4800,0,'live',1),
              ('BJJ-TEE-FREE','Zero','bjj','https://wearmu.com/free.jpg',0,0,'live',1),
              ('PROPOSAL-TEE','Proposal','bjj','https://wearmu.com/proposal.jpg',4800,0,'live',1),
              ('BJJ-HOUSE','House','bjj','https://wearmu.com/house.jpg',900000,0,'live',1);").unwrap();
        let selected = picks(&conn).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].sku, "BJJ-TEE-1");
        assert_eq!(picks(&conn).unwrap()[0].sku, selected[0].sku);
    }

    #[test]
    fn render_escapes_catalog_content_and_preserves_locale() {
        let p = Pick { sku: "TEE-<bad>".into(), label: "<img onerror=alert(1)>".into(),
            brand: "A & B".into(), image: "https://wearmu.com/a.jpg\" onerror=\"alert(1)".into(), price: 4800 };
        let html = render(&[p], true);
        assert!(html.contains("<html lang=\"en\">"));
        assert!(html.contains("/shop/TEE-%3Cbad%3E?lang=en"));
        assert!(html.contains("¥4,800"));
        assert!(!html.contains("<img onerror=alert(1)>"));
        assert!(!html.contains("{{"));
        assert!(render(&[], false).contains("コレクションは商品一覧から"));
        assert!(!render(&[], false).contains("class=\"hero-product\""));
    }
}
