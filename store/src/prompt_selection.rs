//! Purchase-backed design prompt selection. Attribution lives in the existing
//! product metadata and is frozen by checkout_catalog_line in the order snapshot.
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::HashSet;

pub const EXPERIMENT: &str = "design-prompts-v1";
pub const MIN_PURCHASE_UU: usize = 4;

pub struct Pattern {
    pub id: &'static str,
    pub name_ja: &'static str,
    pub name_en: &'static str,
    pub prompt: &'static str,
}

// A change to these instructions requires a new experiment version: previous
// purchases must never be attributed to an instruction that did not create them.
pub const PATTERNS: [Pattern; 10] = [
    Pattern { id: "minimal", name_ja: "ミニマル", name_en: "Minimal",
        prompt: "Distill the requested idea into one unmistakable symbol. Remove decorative noise, use confident simple shapes and a restrained palette, and keep the silhouette recognizable at a glance." },
    Pattern { id: "line_art", name_ja: "線画", name_en: "Line art",
        prompt: "Express the requested subject with deliberate flowing contour lines. Use a distinctive silhouette, consistent printable stroke weight and a clear focal point, avoiding fragile hairline detail." },
    Pattern { id: "ink", name_ja: "墨・筆致", name_en: "Expressive ink",
        prompt: "Interpret the requested idea with expressive Japanese ink-inspired gestures. Balance bold brush shapes with controlled empty space and preserve a legible silhouette; do not invent lettering." },
    Pattern { id: "geometric", name_ja: "幾何学", name_en: "Geometric",
        prompt: "Build the requested motif from a small vocabulary of circles, angular forms and rhythmic lines. Use precise alignment, clear hierarchy and an intentional limited palette." },
    Pattern { id: "vintage", name_ja: "ヴィンテージ", name_en: "Vintage",
        prompt: "Give the requested subject an original vintage sports-print character through bold simplified illustration and restrained screen-print colors. Avoid fine distressed noise, borrowed logos and fabricated dates or heritage claims." },
    Pattern { id: "type_symbol", name_ja: "文字とシンボル", name_en: "Type and symbol",
        prompt: "If the brief explicitly supplies text, make that exact text the readable visual anchor with a supporting symbol. Otherwise use only a strong symbolic composition. Never add slogans, names or extra lettering." },
    Pattern { id: "organic", name_ja: "有機的な曲線", name_en: "Organic forms",
        prompt: "Translate the requested motif into graceful organic shapes and flowing curves. Keep the composition calm but distinctive, with a cohesive limited palette and a recognizable central idea." },
    Pattern { id: "dynamic", name_ja: "躍動感", name_en: "Dynamic motion",
        prompt: "Communicate the requested subject through movement, directional rhythm and decisive diagonals. Keep silhouettes clear and energetic, with strong contrast and no unnecessary background scenery." },
    Pattern { id: "playful", name_ja: "遊び心", name_en: "Playful",
        prompt: "Find a warm, playful visual twist within the requested idea. Use original friendly shapes, a memorable expressive silhouette and a few harmonious colors, avoiding generic clip-art or existing characters." },
    Pattern { id: "abstract", name_ja: "抽象グラフィック", name_en: "Abstract graphic",
        prompt: "Abstract the requested idea into a distinctive arrangement of bold shapes and rhythmic marks while retaining its essential meaning. Use a disciplined palette and intentional hierarchy rather than random visual noise." },
];

#[derive(Default)]
struct Metrics {
    generated: [u64; 10],
    buyers: [HashSet<String>; 10],
}

fn index(meta: &Value) -> Option<usize> {
    let attribution = &meta["prompt_selection"];
    if attribution["experiment"] != EXPERIMENT { return None; }
    PATTERNS.iter().position(|p| attribution["pattern_id"] == p.id)
}

fn parse(raw: Option<String>) -> Result<Value, String> {
    serde_json::from_str(raw.as_deref().unwrap_or("{}"))
        .map_err(|e| format!("prompt selection metadata: {e}"))
}

fn metrics(conn: &Connection) -> Result<Metrics, String> {
    let mut metrics = Metrics::default();
    // Only experiment products are scanned. Invalid legacy metadata is ignored;
    // invalid metadata claiming this experiment is an error, not a zero count.
    let mut products = conn.prepare("SELECT meta_json FROM catalog_products WHERE meta_json LIKE ?")
        .map_err(|e| e.to_string())?;
    let rows = products.query_map([format!("%{EXPERIMENT}%")], |r| r.get::<_, Option<String>>(0))
        .map_err(|e| e.to_string())?;
    for row in rows {
        if let Some(i) = index(&parse(row.map_err(|e| e.to_string())?)?) {
            metrics.generated[i] += 1;
        }
    }
    let mut orders = conn.prepare("SELECT stripe_session_id, payment_status, session_json,
        checkout_spec_json, amount_jpy FROM catalog_orders
        WHERE payment_status IN ('paid','crypto_confirmed') AND checkout_spec_json LIKE ?
        AND COALESCE(status,'') NOT IN ('refunded','partially_refunded','voided','payment_failed','cancelled','canceled')")
        .map_err(|e| e.to_string())?;
    let rows = orders.query_map([format!("%{EXPERIMENT}%")], |r| Ok((
        r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?,
        r.get::<_, Option<String>>(3)?, r.get::<_, Option<i64>>(4)?)))
        .map_err(|e| e.to_string())?;
    for row in rows {
        let (sid, payment, session, spec, amount) = row.map_err(|e| e.to_string())?;
        let spec = parse(spec)?;
        let session = parse(session)?;
        let email = if payment == "paid" {
            // Require real, captured payment evidence, not a checkout or a
            // fulfillment status. A paid session alone also survives refunds.
            if session["id"] != sid || session["livemode"] != true
                || session["amount_total"].as_i64().unwrap_or(0) <= 0
                || !crate::order_contract::payment_clear(&session) { continue; }
            session["customer_details"]["email"].as_str()
                .or_else(|| session["customer_email"].as_str())
        } else {
            if !sid.starts_with("crypto:") || spec["kind"] != "crypto_catalog"
                || spec["reference"].as_str() != sid.strip_prefix("crypto:")
                || amount.unwrap_or(0) <= 0 { continue; }
            // crypto_confirmed is set only by claim_crypto_catalog after its
            // receipt and immutable checkout snapshot have been validated.
            spec["shipping"]["email"].as_str()
        };
        let Some(email) = email.map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()) else { continue; };
        let Some(lines) = spec["lines"].as_array() else { continue; };
        for line in lines {
            if line["qty"].as_i64().unwrap_or(0) <= 0
                || line["unit_amount"].as_i64().unwrap_or(0) <= 0 { continue; }
            // Use the purchased snapshot, never today's mutable product record.
            let meta = match &line["meta_json"] {
                Value::String(raw) => parse(Some(raw.clone()))?,
                Value::Object(_) => line["meta_json"].clone(),
                _ => continue,
            };
            if let Some(i) = index(&meta) { metrics.buyers[i].insert(email.clone()); }
        }
    }
    Ok(metrics)
}

fn choose(metrics: &Metrics, seed: &str) -> usize {
    let adopted: Vec<usize> = (0..PATTERNS.len())
        .filter(|&i| metrics.buyers[i].len() >= MIN_PURCHASE_UU).collect();
    let candidates: Vec<usize> = if adopted.is_empty() {
        let min = *metrics.generated.iter().min().unwrap();
        (0..PATTERNS.len()).filter(|&i| metrics.generated[i] == min).collect()
    } else { adopted };
    // Stable seed tie-breaker distributes simultaneous requests without another
    // table or a paid model call. Exploration balances completed SKU counts.
    let hash = seed.bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64));
    candidates[(hash % candidates.len() as u64) as usize]
}

pub struct Selection {
    pattern: usize,
    adopted: bool,
}

impl Selection {
    pub fn apply(&self, base: &str) -> String {
        format!("{base}\nDesign direction (subordinate to the requested subject, explicit style, colors, text, and ALL production constraints above; do not override them): {}", PATTERNS[self.pattern].prompt)
    }

    pub fn attribution(&self) -> Value {
        json!({"experiment": EXPERIMENT, "pattern_id": PATTERNS[self.pattern].id,
            "phase": if self.adopted { "adopted" } else { "trial" }})
    }

    pub fn metadata(&self) -> String {
        json!({"prompt_selection": self.attribution()}).to_string()
    }
}

pub fn select(conn: &Connection, seed: &str) -> Result<Selection, String> {
    let metrics = metrics(conn)?;
    let pattern = choose(&metrics, seed);
    Ok(Selection { pattern, adopted: metrics.buyers[pattern].len() >= MIN_PURCHASE_UU })
}

pub fn status(conn: &Connection) -> Result<Value, String> {
    let metrics = metrics(conn)?;
    let patterns: Vec<Value> = PATTERNS.iter().enumerate().map(|(i, p)| json!({
        "id": p.id, "name_ja": p.name_ja, "name_en": p.name_en, "prompt": p.prompt,
        "generated_skus": metrics.generated[i], "purchase_uu": metrics.buyers[i].len(),
        "adopted": metrics.buyers[i].len() >= MIN_PURCHASE_UU,
    })).collect();
    Ok(json!({"experiment": EXPERIMENT, "minimum_purchase_uu": MIN_PURCHASE_UU,
        "identity": "normalized_buyer_email", "patterns": patterns}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE catalog_products(sku TEXT PRIMARY KEY,meta_json TEXT);
            CREATE TABLE catalog_orders(stripe_session_id TEXT PRIMARY KEY,payment_status TEXT,
                session_json TEXT,checkout_spec_json TEXT,amount_jpy INTEGER,status TEXT);").unwrap();
        conn
    }

    fn line(pattern: usize, sku: &str) -> Value {
        json!({"sku":sku,"qty":1,"unit_amount":4900,"meta_json":Selection {
            pattern, adopted: false,
        }.metadata()})
    }

    fn session(id: &str, email: &str) -> Value {
        json!({"id":id,"livemode":true,"amount_total":4900,"payment_status":"paid",
            "customer_details":{"email":email},"payment_intent":{"status":"succeeded",
            "latest_charge":{"paid":true,"captured":true,"status":"succeeded",
                "amount_refunded":0,"refunded":false}}})
    }

    fn order(conn: &Connection, id: &str, email: &str, pattern: usize) {
        conn.execute("INSERT INTO catalog_orders VALUES (?,'paid',?,?,4900,'submitted')",
            params![id,session(id,email).to_string(),json!({"lines":[line(pattern,id)]}).to_string()]).unwrap();
    }

    #[test]
    fn ten_trials_then_four_distinct_buyers_adopt_on_next_generation() {
        let conn = db();
        let mut seen = HashSet::new();
        for n in 0..10 {
            let selection = select(&conn,"same seed").unwrap();
            assert!(!selection.adopted);
            assert!(seen.insert(selection.pattern));
            conn.execute("INSERT INTO catalog_products VALUES (?,?)",
                params![format!("sku-{n}"),selection.metadata()]).unwrap();
        }
        for n in 0..3 { order(&conn,&format!("cs_{n}"),&format!("buyer{n}@example.test"),2); }
        let s = status(&conn).unwrap();
        assert_eq!(s["patterns"][2]["purchase_uu"],3);
        assert_eq!(s["patterns"][2]["adopted"],false);
        // Multiple purchases / products / email case / surrounding whitespace
        // cannot turn 3 real buyers into 4.
        order(&conn,"cs_repeat"," BUYER0@example.test ",2);
        assert_eq!(status(&conn).unwrap()["patterns"][2]["purchase_uu"],3);
        order(&conn,"cs_fourth","buyer3@example.test",2);
        for seed in ["a","b","c"] {
            let selection = select(&conn,seed).unwrap();
            assert_eq!(selection.pattern,2);
            assert!(selection.adopted);
        }
        // A refund reduces eligible UU immediately; adoption is not sticky.
        conn.execute("UPDATE catalog_orders SET payment_status='refunded' WHERE stripe_session_id='cs_fourth'",[]).unwrap();
        assert_eq!(status(&conn).unwrap()["patterns"][2]["adopted"],false);
    }

    #[test]
    fn excludes_unpaid_test_free_uncaptured_refunded_and_unknown_buyers() {
        let conn = db();
        for (n,change) in ["test","free","uncaptured","refunded","partial","unpaid","no_identity","wrong_session"].iter().enumerate() {
            let id = format!("cs_{n}");
            let mut s = session(&id,&format!("buyer{n}@example.test"));
            match *change {
                "test" => s["livemode"] = json!(false),
                "free" => s["amount_total"] = json!(0),
                "uncaptured" => s["payment_intent"]["latest_charge"]["captured"] = json!(false),
                "refunded" => s["payment_intent"]["latest_charge"]["refunded"] = json!(true),
                "partial" => s["payment_intent"]["latest_charge"]["amount_refunded"] = json!(1),
                "unpaid" => s["payment_status"] = json!("unpaid"),
                "no_identity" => s["customer_details"]["email"] = json!("  "),
                "wrong_session" => s["id"] = json!("unrelated"),
                _ => unreachable!(),
            }
            conn.execute("INSERT INTO catalog_orders VALUES (?,'paid',?,?,4900,'submitted')",
                params![id,s.to_string(),json!({"lines":[line(0,"A")]}).to_string()]).unwrap();
        }
        for (n,payment) in ["unpaid","refunded","partially_refunded","voided","no_payment_required"].iter().enumerate() {
            let id = format!("blocked-{n}");
            order(&conn,&id,&format!("blocked{n}@example.test"),0);
            conn.execute("UPDATE catalog_orders SET payment_status=? WHERE stripe_session_id=?",params![payment,id]).unwrap();
        }
        assert_eq!(status(&conn).unwrap()["patterns"][0]["purchase_uu"],0);
        // Fulfillment failure is NOT a payment failure: captured revenue counts.
        order(&conn,"paid_vendor_failed","paid@example.test",0);
        conn.execute("UPDATE catalog_orders SET status='failed_network' WHERE stripe_session_id='paid_vendor_failed'",[]).unwrap();
        assert_eq!(status(&conn).unwrap()["patterns"][0]["purchase_uu"],1);
    }

    #[test]
    fn uses_frozen_line_attribution_counts_addons_and_deduplicates_cart() {
        let conn = db();
        order(&conn,"cs_cart","buyer@example.test",0);
        conn.execute("UPDATE catalog_orders SET checkout_spec_json=? WHERE stripe_session_id='cs_cart'",
            [json!({"lines":[line(0,"A"),line(0,"B"),line(1,"addon")]}).to_string()]).unwrap();
        // A product changed after checkout must not rewrite experiment history.
        conn.execute("INSERT INTO catalog_products VALUES ('A',?)",
            [Selection { pattern:9,adopted:false }.metadata()]).unwrap();
        let s = status(&conn).unwrap();
        assert_eq!(s["patterns"][0]["purchase_uu"],1);
        assert_eq!(s["patterns"][1]["purchase_uu"],1);
        assert_eq!(s["patterns"][9]["purchase_uu"],0);
        assert_eq!(s["patterns"][9]["generated_skus"],1);
        // Missing/old attribution and free add-ons are not guessed from SKU.
        let mut old = line(2,"old");
        old["meta_json"] = json!({"prompt_selection":{"experiment":"old","pattern_id":"ink"}});
        let mut free = line(3,"free"); free["unit_amount"] = json!(0);
        conn.execute("UPDATE catalog_orders SET checkout_spec_json=?",
            [json!({"lines":[old,free,{"sku":"A","qty":1,"unit_amount":4900}]}).to_string()]).unwrap();
        for pattern in status(&conn).unwrap()["patterns"].as_array().unwrap() {
            assert_eq!(pattern["purchase_uu"],0);
        }
    }

    #[test]
    fn multiple_adopted_patterns_are_used_and_unqualified_patterns_are_not() {
        let conn = db();
        for i in [0,4] {
            for n in 0..4 { order(&conn,&format!("cs_{i}_{n}"),&format!("buyer{n}@example.test"),i); }
        }
        let seen: HashSet<usize> = (0..20).map(|n| select(&conn,&n.to_string()).unwrap().pattern).collect();
        assert_eq!(seen,HashSet::from([0,4]));
    }

    #[test]
    fn confirmed_crypto_and_stripe_deduplicate_the_same_buyer() {
        let conn = db();
        let spec = json!({"kind":"crypto_catalog","reference":"abc",
            "shipping":{"email":"BUYER@example.test"},"lines":[line(5,"crypto-sku")]});
        conn.execute("INSERT INTO catalog_orders VALUES ('crypto:abc','crypto_confirmed',NULL,?,4900,'submitting')",
            [spec.to_string()]).unwrap();
        order(&conn,"cs_card","buyer@example.test",5);
        assert_eq!(status(&conn).unwrap()["patterns"][5]["purchase_uu"],1);
        conn.execute("UPDATE catalog_orders SET stripe_session_id='crypto:wrong' WHERE stripe_session_id='crypto:abc'",[]).unwrap();
        conn.execute("DELETE FROM catalog_orders WHERE stripe_session_id='cs_card'",[]).unwrap();
        assert_eq!(status(&conn).unwrap()["patterns"][5]["purchase_uu"],0);
    }

    #[test]
    fn restart_keeps_attribution_and_database_errors_do_not_fabricate_winners() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let conn = db();
        conn.execute("VACUUM INTO ?",[file.path().to_str().unwrap()]).unwrap();
        let conn = Connection::open(file.path()).unwrap();
        for n in 0..4 { order(&conn,&format!("cs_{n}"),&format!("buyer{n}@example.test"),7); }
        drop(conn);
        let conn = Connection::open(file.path()).unwrap();
        assert_eq!(select(&conn,"restart").unwrap().pattern,7);
        conn.execute("DROP TABLE catalog_orders",[]).unwrap();
        assert!(select(&conn,"db error").is_err());
    }

    #[test]
    fn production_constraints_remain_in_every_prompt() {
        let base = "Requested: exact blue belt text. PURE WHITE background. NO frame. Thick embroidery strokes.";
        for pattern in 0..10 {
            let prompt = Selection { pattern,adopted:false }.apply(base);
            assert!(prompt.starts_with(base));
            assert!(prompt.contains("do not override them"));
            assert!(prompt.ends_with(PATTERNS[pattern].prompt));
        }
    }
}
