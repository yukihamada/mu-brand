//! Durable checkout specifications and payment/submission fences. No product rereads on replay.
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    for (name, ty) in [
        ("checkout_spec_json", "TEXT"), ("fulfillment_request_json", "TEXT"),
        ("retry_count", "INTEGER NOT NULL DEFAULT 0"),
        ("order_updated_at", "TEXT"), ("payment_status", "TEXT"),
        ("order_error", "TEXT"),
        ("paid_items_json", "TEXT"),
        ("effects_started", "INTEGER NOT NULL DEFAULT 0"),
        ("sample_credit_applied", "INTEGER NOT NULL DEFAULT 0"),
        ("session_json", "TEXT"),
        ("payment_intent_id", "TEXT"),
        ("rewards_completed", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('catalog_orders') WHERE name=?)",
            [name], |r| r.get(0))?;
        if !exists { conn.execute(&format!("ALTER TABLE catalog_orders ADD COLUMN {name} {ty}"), [])?; }
    }
    // Payment blocks dominate even legacy writers of order status.
    conn.execute_batch("CREATE TRIGGER IF NOT EXISTS catalog_payment_block_guard
        BEFORE UPDATE OF status,payment_status ON catalog_orders
        WHEN OLD.payment_status IN ('refunded','partially_refunded','voided')
          AND NOT (NEW.status IS NEW.payment_status AND
            (NEW.payment_status IS OLD.payment_status OR (OLD.payment_status='partially_refunded' AND NEW.payment_status IS 'refunded')))
        BEGIN SELECT RAISE(ABORT,'payment blocked'); END;")?;
    Ok(())
}

fn object_id(v: &Value) -> Option<&str> { v.as_str().or_else(||v["id"].as_str()) }

pub fn payment_block(s: &Value) -> Option<&'static str> {
    let pi = &s["payment_intent"];
    let charge = &pi["latest_charge"];
    if charge["refunded"] == true { return Some("refunded"); }
    if charge["amount_refunded"].as_i64().unwrap_or(0) > 0 { return Some("partially_refunded"); }
    if pi["status"] == "canceled" || charge["status"] == "failed" { return Some("voided"); }
    None
}

/// Session `paid` stays true after a Dashboard refund. Only a current expanded
/// PaymentIntent + captured charge authorizes fulfillment.
pub fn payment_clear(s: &Value) -> bool {
    if !paid(s) || payment_block(s).is_some() { return false; }
    if s["payment_status"] == "no_payment_required" && s["amount_total"].as_i64() == Some(0) { return true; }
    let pi=&s["payment_intent"]; let c=&pi["latest_charge"];
    pi["status"] == "succeeded" && c["paid"] == true && c["captured"] == true
        && c["status"] == "succeeded" && c["amount_refunded"].as_i64() == Some(0)
        && c["refunded"] == false
}

pub fn persist_payment(conn: &Connection, s: &Value) -> Result<bool,String> {
    let sid=s["id"].as_str().ok_or("session id missing")?;
    if let Some(status)=payment_block(s) {
        block_payment(conn,object_id(&s["payment_intent"]),Some(sid),status)?;
        return Ok(false);
    }
    let blocked: bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM catalog_orders WHERE
        (stripe_session_id=? OR (payment_intent_id IS NOT NULL AND payment_intent_id=?))
        AND payment_status IN ('refunded','partially_refunded','voided'))",
        params![sid,object_id(&s["payment_intent"])],|r|r.get(0)).map_err(|e|e.to_string())?;
    if blocked { return Ok(false); }
    conn.execute("UPDATE catalog_orders SET session_json=?,payment_intent_id=COALESCE(?,payment_intent_id)
        WHERE stripe_session_id=?",params![s.to_string(),object_id(&s["payment_intent"]),sid]).map_err(|e|e.to_string())?;
    Ok(payment_clear(s))
}

pub fn block_payment(conn: &Connection, pi: Option<&str>, sid: Option<&str>, status: &str) -> Result<(),String> {
    if !matches!(status,"refunded"|"partially_refunded"|"voided") { return Err("invalid payment block".into()); }
    let tx=conn.unchecked_transaction().map_err(|e|e.to_string())?;
    // A payment tombstone in the canonical order store also fences a refund
    // delivered before checkout.session.completed / draft attachment.
    if let Some(pi)=pi {
        tx.execute("INSERT OR IGNORE INTO catalog_orders(stripe_session_id,payment_intent_id,status,payment_status)
            VALUES (?,?,?,?)",params![format!("payment:{pi}"),pi,status,status]).map_err(|e|e.to_string())?;
    }
    tx.execute("UPDATE catalog_orders SET status=?,payment_status=?,order_error='payment refunded or voided',order_updated_at=datetime('now')
        WHERE (stripe_session_id=? OR payment_intent_id=?)
        AND (COALESCE(payment_status,'') NOT IN ('refunded','partially_refunded','voided')
            OR (payment_status='partially_refunded' AND ?='refunded'))",
        params![status,status,sid,pi,status]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e|e.to_string())
}

/// Called synchronously before webhook ACK. No worker must exist for this row
/// to survive: cron can resume payment_ready using the saved session + snapshot.
pub fn enqueue(conn: &Connection, s: &Value) -> Result<(),String> {
    if !paid(s) { return Err("session not paid".into()); }
    let tx=conn.unchecked_transaction().map_err(|e|e.to_string())?;
    let sid=s["id"].as_str().ok_or("session id missing")?;
    if let Some(d)=s["metadata"]["order_draft"].as_str() { attach(&tx,d,s)?; }
    let raw: Option<String>=tx.query_row("SELECT checkout_spec_json FROM catalog_orders WHERE stripe_session_id=?",
        [sid],|r|r.get(0)).optional().map_err(|e|e.to_string())?.flatten();
    let has_spec=raw.as_deref().and_then(|r|serde_json::from_str::<Value>(r).ok()).is_some();
    tx.execute("INSERT OR IGNORE INTO catalog_orders(stripe_session_id,sku,status,session_json,order_error)
        VALUES (?,?,'blocked_legacy_snapshot',?,'checkout snapshot unavailable')",
        params![sid,s["metadata"]["catalog_sku"].as_str(),s.to_string()]).map_err(|e|e.to_string())?;
    let blocked: Option<String>=tx.query_row("SELECT payment_status FROM catalog_orders WHERE payment_intent_id=?
        AND payment_status IN ('refunded','partially_refunded','voided') LIMIT 1",
        [object_id(&s["payment_intent"])],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
    if let Some(status)=blocked {
        tx.execute("UPDATE catalog_orders SET status=?,payment_status=? WHERE stripe_session_id=?
            AND COALESCE(payment_status,'') NOT IN ('refunded','partially_refunded','voided')",params![status,status,sid]).map_err(|e|e.to_string())?;
    } else if has_spec {
        tx.execute("UPDATE catalog_orders SET status='payment_ready',session_json=?,payment_intent_id=?,
            payment_status=?,order_updated_at=datetime('now') WHERE stripe_session_id=?
            AND status IN ('checkout_pending','payment_pending','payment_failed','payment_ready')",
            params![s.to_string(),object_id(&s["payment_intent"]),s["payment_status"].as_str(),sid]).map_err(|e|e.to_string())?;
    }
    tx.commit().map_err(|e|e.to_string())
}

pub fn paid(s: &Value) -> bool {
    !s["id"].as_str().unwrap_or("").is_empty()
        && matches!(s["payment_status"].as_str(), Some("paid" | "no_payment_required"))
}

pub fn files(v: &Value) -> Result<(), String> {
    let a = v.as_array().filter(|a| !a.is_empty()).ok_or("print files missing")?;
    let mut placements = std::collections::HashSet::new();
    for f in a {
        let url = f["url"].as_str().unwrap_or("");
        let placement = f["type"].as_str().unwrap_or("");
        if !url.starts_with("https://") || url.len() <= 8 || placement.is_empty()
            || !placements.insert(placement) { return Err("invalid/duplicate print file placement".into()); }
    }
    Ok(())
}

pub fn size_variant(map: &Value, size: &str) -> Result<i64, String> {
    let m = map.as_object().filter(|m| !m.is_empty()).ok_or("size map missing")?;
    let key = size.trim().to_uppercase();
    let value = m.get(&key).or_else(|| {
        if m.len() == 1 && matches!(key.as_str(), "OS" | "ONE SIZE" | "ONE_SIZE") {
            m.get("OS").or_else(|| m.get("ONE SIZE")).or_else(|| m.get("ONE_SIZE"))
        } else { None }
    });
    value.and_then(Value::as_i64).filter(|v| *v > 0).ok_or_else(|| format!("unsupported size: {key}"))
}

pub fn draft(conn: &Connection, sku: &str, spec: &Value) -> Result<String, String> {
    let id = format!("checkout:{}", uuid::Uuid::new_v4().simple());
    conn.execute("INSERT INTO catalog_orders (stripe_session_id,sku,status,checkout_spec_json,order_updated_at)
        VALUES (?,?,'checkout_pending',?,datetime('now'))", params![id, sku, spec.to_string()])
        .map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn attach(conn: &Connection, draft: &str, session: &Value) -> Result<(), String> {
    let sid = session["id"].as_str().filter(|s| !s.is_empty()).ok_or("Stripe session missing id")?;
    let changed = conn.execute("UPDATE catalog_orders SET stripe_session_id=? WHERE stripe_session_id=? AND status='checkout_pending'",
        params![sid, draft]).map_err(|e| e.to_string())?;
    if changed == 0 {
        let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM catalog_orders WHERE stripe_session_id=?)", [sid], |r| r.get(0)).map_err(|e| e.to_string())?;
        if !exists { return Err("checkout snapshot binding failed".into()); }
    }
    Ok(())
}

pub fn load(conn: &Connection, sid: &str) -> Result<Value, String> {
    let raw: Option<String> = conn.query_row("SELECT checkout_spec_json FROM catalog_orders WHERE stripe_session_id=?", [sid], |r| r.get(0))
        .optional().map_err(|e| e.to_string())?.flatten();
    serde_json::from_str(&raw.ok_or("legacy order: checkout specification unavailable; manual review required")?).map_err(|e| e.to_string())
}

/// UPDATE predicate is the cross-process fence; a webhook never retries a failed submission.
pub fn claim(conn: &Connection, session: &Value) -> Result<Option<Value>, String> {
    if !paid(session) { return Ok(None); }
    let sid = session["id"].as_str().unwrap();
    if let Some(d) = session["metadata"]["order_draft"].as_str() { attach(conn, d, session)?; }
    let spec = match load(conn, sid) {
        Ok(s) => s,
        Err(e) => {
            conn.execute("INSERT OR IGNORE INTO catalog_orders (stripe_session_id,sku,status,order_error,amount_jpy)
                VALUES (?,?,'blocked_legacy_snapshot',?,?)", params![sid, session["metadata"]["catalog_sku"].as_str(), e, session["amount_total"].as_i64()]).map_err(|e| e.to_string())?;
            conn.execute("UPDATE catalog_orders SET status='blocked_legacy_snapshot',order_error=? WHERE stripe_session_id=?
                AND status IN ('failed','failed_network','failed_no_key','retry_ready','submitting')", params![e,sid]).map_err(|e| e.to_string())?;
            return Err(e);
        }
    };
    let n = conn.execute("UPDATE catalog_orders SET status='submitting', payment_status=?, amount_jpy=?, order_updated_at=datetime('now')
        WHERE stripe_session_id=? AND status IN ('checkout_pending','payment_pending','payment_failed','payment_ready','retry_ready')
        AND COALESCE(payment_status,'') NOT IN ('refunded','partially_refunded','voided')",
        params![session["payment_status"].as_str(), session["amount_total"].as_i64(), sid]).map_err(|e| e.to_string())?;
    Ok((n == 1).then_some(spec))
}

pub fn mark(conn: &Connection, sid: &str, status: &str, error: &str) {
    if let Err(e) = conn.execute("UPDATE catalog_orders SET status=?,order_error=?,order_updated_at=datetime('now')
        WHERE stripe_session_id=? AND status NOT IN ('submitted','refunded','partially_refunded','voided','ticket_delivered','manual_pending','collab_complete','blocked_vendor_preflight')
        AND COALESCE(payment_status,'') NOT IN ('refunded','partially_refunded','voided')",
        params![status,error,sid]) { tracing::error!("order state update failed: {e}"); }
}

pub fn queue_retry(conn: &Connection, id: i64) -> Result<bool, String> {
    // Deliberately no stale submitting/sending/gift_building takeover: without
    // an epoch a paused worker could resume into a newer attempt. Only workers
    // that have finished their supplier operation may publish a retryable state.
    conn.execute("UPDATE catalog_orders SET status='retry_ready',retry_count=retry_count+1,order_updated_at=datetime('now')
        WHERE id=? AND checkout_spec_json IS NOT NULL AND retry_count<3
        AND COALESCE(payment_status,'') NOT IN ('refunded','partially_refunded','voided')
        AND status IN ('failed','failed_network','failed_no_key','failed_line_items','submission_uncertain','blocked_vendor_preflight')", [id])
        .map(|n| n == 1).map_err(|e| e.to_string())
}

pub fn freeze_items(conn: &Connection, sid: &str, items: &[Value]) -> Result<Vec<Value>, String> {
    let n=conn.execute("UPDATE catalog_orders SET paid_items_json=COALESCE(paid_items_json,?) WHERE stripe_session_id=?
        AND COALESCE(payment_status,'') NOT IN ('refunded','partially_refunded','voided')",
        params![serde_json::to_string(items).map_err(|e| e.to_string())?,sid]).map_err(|e| e.to_string())?;
    if n!=1 { return Err("payment blocked or order missing".into()); }
    saved_items(conn,sid)
}

pub fn saved_items(conn: &Connection, sid: &str) -> Result<Vec<Value>, String> {
    let raw: String = conn.query_row("SELECT paid_items_json FROM catalog_orders WHERE stripe_session_id=?", [sid], |r| r.get(0)).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

pub fn effects_once(conn: &Connection, sid: &str) -> Result<bool, String> {
    conn.execute("UPDATE catalog_orders SET effects_started=1 WHERE stripe_session_id=? AND effects_started=0", [sid])
        .map(|n| n == 1).map_err(|e| e.to_string())
}

/// A reward receipt and balance mutation commit together. BEGIN IMMEDIATE
/// serializes independent SQLite connections; an interrupted transaction leaves
/// neither a receipt nor a balance change. Extra accounting shares the commit.
pub fn credit_once<F>(conn: &Connection, sid: &str, email: &str, amount: i64, reason: &str, extra: F) -> Result<bool,String>
where F: FnOnce(&Connection) -> rusqlite::Result<()> {
    if amount<=0 { return Ok(false); }
    let tx=rusqlite::Transaction::new_unchecked(conn,rusqlite::TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    let exists: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM mu_credit_ledger WHERE ref_id=? AND reason=?)",
        params![sid,reason],|r|r.get(0)).map_err(|e|e.to_string())?;
    if exists { return Ok(false); }
    let email=email.to_lowercase();
    tx.execute("INSERT INTO mu_credits(email,balance_jpy,total_earned_jpy,total_spent_jpy,updated_at)
        VALUES (?, ?, ?, 0, strftime('%s','now')) ON CONFLICT(email) DO UPDATE SET
        balance_jpy=balance_jpy+excluded.balance_jpy,total_earned_jpy=total_earned_jpy+excluded.total_earned_jpy,
        updated_at=excluded.updated_at",params![email,amount,amount]).map_err(|e|e.to_string())?;
    tx.execute("INSERT INTO mu_credit_ledger(email,delta_jpy,reason,ref_id,created_at) VALUES (?,?,?,?,strftime('%s','now'))",
        params![email,amount,reason,sid]).map_err(|e|e.to_string())?;
    extra(&tx).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e|e.to_string())?;
    Ok(true)
}

pub async fn full_session(session: &Value) -> Result<Value, String> {
    let key = std::env::var("STRIPE_SECRET_KEY").map_err(|_| "STRIPE_SECRET_KEY unset")?;
    full_session_at(session,&key,"https://api.stripe.com/v1").await
}

async fn full_session_at(session: &Value, key: &str, root: &str) -> Result<Value,String> {
    let client=reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).build().map_err(|e|e.to_string())?;
    let response = client.get(format!("{root}/checkout/sessions/{}", session["id"].as_str().ok_or("missing session id")?))
        .query(&[("expand[]", "line_items"),("expand[]","payment_intent.latest_charge")]).basic_auth(key, None::<&str>).send().await.map_err(|e| e.to_string())?
        .error_for_status().map_err(|e| e.to_string())?;
    let mut full: Value = response.json().await.map_err(|e| e.to_string())?;
    if full["id"] != session["id"] { return Err("Stripe session mismatch".into()); }
    if full["line_items"]["has_more"].as_bool() == Some(true) {
        full["line_items"] = client.get(format!("{root}/checkout/sessions/{}/line_items", session["id"].as_str().unwrap()))
            .query(&[("limit","100")]).basic_auth(key,None::<&str>).send().await.map_err(|e|e.to_string())?
            .error_for_status().map_err(|e|e.to_string())?.json().await.map_err(|e|e.to_string())?;
    }
    Ok(full)
}

pub async fn verified_session(db: &crate::Db, session: &Value) -> Result<Value,String> {
    let full=full_session(session).await?;
    if !persist_payment(&db.lock().unwrap(),&full)? { return Err("payment refunded, voided, or not captured".into()); }
    Ok(full)
}

/// Validate every charged line before any supplier request. Adjustable quantities are disabled.
pub fn purchased_items(spec: &Value, session: &Value) -> Result<Vec<Value>, String> {
    if session["currency"].as_str() != Some("jpy") { return Err("currency mismatch".into()); }
    let lines = spec["lines"].as_array().ok_or("snapshot lines missing")?;
    let paid = session["line_items"]["data"].as_array().ok_or("Stripe line_items missing")?;
    if lines.is_empty() || paid.len() != lines.len() || session["line_items"]["has_more"].as_bool() == Some(true) { return Err("incomplete Stripe line_items".into()); }
    let mut result = Vec::new();
    for (line, payment) in lines.iter().zip(paid) {
        let qty = payment["quantity"].as_i64().filter(|q| *q > 0 && *q <= 50).ok_or("invalid quantity")?;
        if Some(qty) != line["qty"].as_i64() || payment["price"]["unit_amount"] != line["unit_amount"]
            || payment["price"]["currency"].as_str() != Some("jpy") { return Err("purchased line differs from checkout specification".into()); }
        if !line["variants"].is_object() {
            if matches!(line["route"].as_str(),Some("manual" | "digital" | "sweep_manual" | "pre_order" | "mu_drop")) { continue; }
            return Err("printful line has no frozen variants".into());
        }
        let mut size = line["size"].as_str().unwrap_or("FIXED").to_string();
        if let Some(field) = line["size_field"].as_str() {
            size = session["custom_fields"].as_array().and_then(|a| a.iter().find(|f| f["key"].as_str() == Some(field)))
                .and_then(|f| f["dropdown"]["value"].as_str()).ok_or("selected size missing")?.to_string();
        }
        let mut item = line["variants"].get(&size).cloned().ok_or("selected size not in checkout snapshot")?;
        files(&item["files"])?;
        if item["variant_id"].as_i64().filter(|v| *v > 0).is_none() { return Err("explicit print variant missing".into()); }
        item["quantity"] = json!(qty);
        item["retail_price"] = json!(format!("{:.2}", line["unit_amount"].as_i64().ok_or("price missing")? as f64));
        result.push(item);
    }
    Ok(result)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderPurpose {
    Sale,
    /// Only the authenticated partner sample-checkout handler may select this.
    PartnerSample,
}

pub fn size_choices(map: &Value) -> Result<Vec<String>, String> {
    let m = map.as_object().filter(|m| !m.is_empty()).ok_or("size map missing")?;
    let mut sizes = Vec::new();
    for (size, id) in m {
        if size.trim().is_empty() || size != &size.trim().to_uppercase()
            || id.as_i64().filter(|id| *id > 0).is_none() { return Err("invalid size map".into()); }
        sizes.push(size.clone());
    }
    const ORDER: &[&str] = &["XS","S","M","L","XL","2XL","3XL","4XL","5XL"];
    sizes.sort_by_key(|s| (ORDER.iter().position(|v| *v == s).unwrap_or(ORDER.len()),s.clone()));
    Ok(sizes)
}

/// Complete persisted spec is authoritative even when invalid; never fall back to design_file.
pub fn persisted_print_spec(meta: &Value) -> Result<Option<Value>, String> {
    if !["printful_files","printful_options","printful_variant_map","collab_migration_version"]
        .iter().any(|k| meta.get(k).is_some()) { return Ok(None); }
    files(&meta["printful_files"])?;
    size_choices(&meta["printful_variant_map"])?;
    let options = if meta["printful_options"].is_null() { json!([]) } else { meta["printful_options"].clone() };
    if !options.is_array() { return Err("invalid print options".into()); }
    Ok(Some(json!({"files":meta["printful_files"],"options":options,"variant_map":meta["printful_variant_map"]})))
}

pub fn print_spec_for_route(route: &str, meta: &Value) -> Result<Option<Value>,String> {
    if route.starts_with("printful_") { persisted_print_spec(meta) } else { Ok(None) }
}

pub fn line_availability(line: &Value, country: Option<&str>, stock: Option<bool>) -> Result<(),String> {
    let meta: Value=match &line["meta_json"] {
        Value::String(raw)=>serde_json::from_str(raw).map_err(|_|"invalid frozen metadata")?,
        value=>value.clone(),
    };
    let sku=line["sku"].as_str().unwrap_or("");
    let product=line["product_id"].as_i64().unwrap_or(0);
    let kinds=[line["product_kind"].as_str(),meta["kind"].as_str(),meta["product_kind"].as_str(),
        crate::catalog::order_kind_from_sku(sku),crate::catalog::order_product_restriction(product)];
    for kind in kinds.into_iter().flatten() {
        let a=crate::catalog::kind_availability_with_stock(kind,country,stock);
        if !a.available { return Err(format!("vendor preflight: {sku}: {}",a.unavailable_reason.unwrap_or("unavailable"))); }
    }
    Ok(())
}

/// Read-only vendor preflight. None checks that checkout has available options;
/// checkout callers must use prepare_checkout to receive the filtered snapshot.
/// Some(items) checks exactly the purchased/frozen supplier lines. No estimated
/// orders, stock cache, or availability_regions-as-destination inference.
pub async fn preflight(spec: &Value, items: Option<&[Value]>, country: &str) -> Result<(),String> {
    let key=std::env::var("PRINTFUL_API_KEY").unwrap_or_default();
    preflight_at(spec,items,country,&key,"https://api.printful.com").await
}

async fn vendor_get(client: &reqwest::Client, key: &str, url: &str) -> Result<Value,String> {
    let mut request=client.get(url);
    if !key.is_empty() { request=request.bearer_auth(key); }
    if let Ok(store)=std::env::var("PRINTFUL_STORE_ID") {
        if !store.is_empty() { request=request.header("X-PF-Store-Id",store); }
    }
    request.send().await.map_err(|e|format!("vendor GET: {e}"))?
        .error_for_status().map_err(|e|format!("vendor GET: {e}"))?
        .json().await.map_err(|e|format!("vendor JSON: {e}"))
}

fn vendor_variant<'a>(result: &'a Value, product: i64, variant: i64) -> Result<&'a Value,String> {
    if result["product"]["id"].as_i64()!=Some(product) || result["product"]["is_discontinued"].as_bool()!=Some(false) {
        return Err("vendor preflight: product missing, discontinued, or unknown".into());
    }
    let v=result["variants"].as_array().and_then(|vs|vs.iter().find(|v|v["id"].as_i64()==Some(variant)))
        .ok_or("vendor preflight: variant does not belong to product")?;
    if v["product_id"].as_i64()!=Some(product) { return Err("vendor preflight: variant product mismatch".into()); }
    Ok(v)
}

/// Only the documented primary file ID is an alias. Never interpret arbitrary
/// file IDs as placement types (e.g. embroidery versus DTG).
fn canonical_file_type<'a>(types: &'a [Value], placement: &str) -> Result<&'a str,String> {
    if placement=="default" {
        let primary:Vec<_>=types.iter().filter(|f|f["id"]=="default").collect();
        if primary.len()==1 {
            return primary[0]["type"].as_str().filter(|t|!t.is_empty() && *t!="mockup")
                .ok_or_else(||"vendor preflight: invalid primary file".into());
        }
        if primary.len()>1 { return Err("vendor preflight: ambiguous primary file".into()); }
    }
    types.iter().find(|t|t["type"].as_str()==Some(placement) && placement!="mockup")
        .and_then(|t|t["type"].as_str()).ok_or_else(||format!("vendor preflight: unsupported placement {placement}"))
}

fn normalize_item_files(item: &mut Value, result: &Value) -> Result<(),String> {
    files(&item["files"])?;
    let types=result["product"]["files"].as_array().ok_or("vendor preflight: file types unknown")?;
    for f in item["files"].as_array_mut().unwrap() {
        f["type"]=json!(canonical_file_type(types,f["type"].as_str().unwrap_or(""))?);
    }
    // Alias collapse must not produce two front files.
    files(&item["files"])
}

/// Prepare only a NEW checkout snapshot. Never call this on a purchased order.
/// Explicit/fixed selections fail on missing stock. An unselected dropdown may
/// discard unavailable options and choose an available initial UI default.
pub async fn prepare_checkout(spec: &Value, country: &str) -> Result<Value,String> {
    let key=std::env::var("PRINTFUL_API_KEY").unwrap_or_default();
    prepare_checkout_at(spec,country,&key,"https://api.printful.com").await
}

pub(crate) async fn prepare_checkout_at(spec: &Value, country: &str, key: &str, root: &str) -> Result<Value,String> {
    let country=country.trim().to_uppercase();
    if country.len()!=2 || !country.bytes().all(|b|b.is_ascii_uppercase()) { return Err("vendor preflight: ISO-2 destination required".into()); }
    let client=reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).build().map_err(|e|e.to_string())?;
    let mut result=spec.clone();
    let mut cache=std::collections::HashMap::<i64,Value>::new();
    for line in result["lines"].as_array_mut().ok_or("snapshot lines missing")? {
        line_availability(line,Some(&country),Some(true))?;
        if !line["route"].as_str().unwrap_or("").starts_with("printful") { continue; }
        let product=line["product_id"].as_i64().filter(|p|*p>0).ok_or("frozen product ID missing")?;
        if !cache.contains_key(&product) {
            cache.insert(product,vendor_get(&client,key,&format!("{root}/products/{product}")).await?);
        }
        let vendor=&cache[&product]["result"];
        let mut available=serde_json::Map::new();
        let selectable=line["size_field"].as_str().is_some();
        for (size,item) in line["variants"].as_object().filter(|v|!v.is_empty()).ok_or("variants missing")? {
            let vid=item["variant_id"].as_i64().filter(|v|*v>0).ok_or("variant missing")?;
            let v=vendor_variant(vendor,product,vid)?;
            let stock=v["in_stock"].as_bool().ok_or("vendor preflight: selected variant stock unknown")?;
            if !stock {
                if selectable { continue; }
                return Err("vendor preflight: selected variant out of stock".into());
            }
            let mut item=item.clone();normalize_item_files(&mut item,vendor)?;
            available.insert(size.clone(),item);
        }
        if available.is_empty() { return Err("vendor preflight: all variants unavailable".into()); }
        if selectable {
            let previous=line["size"].as_str().unwrap_or("M");
            let default=if available.contains_key(previous) {previous.to_string()}
                else if available.contains_key("M") {"M".into()} else {available.keys().next().unwrap().clone()};
            line["size"]=json!(default);
        } else if !available.contains_key(line["size"].as_str().unwrap_or("FIXED")) {
            return Err("vendor preflight: explicit selection missing".into());
        }
        line["variant_map"]=json!(available.iter().map(|(s,i)|(s.clone(),i["variant_id"].clone())).collect::<serde_json::Map<_,_>>());
        line["variants"]=Value::Object(available);
    }
    Ok(result)
}

/// Official Products API: GET /store/variants/{id}, SyncVariant response.
/// https://developers.printful.com/docs/#tag/Products-API/operation/getSyncVariantById
/// Flatten the sync configuration into explicit immutable supplier files; never
/// send a mutable sync_variant_id or substitute a different base/color/size.
pub async fn resolve_sync_line(line: Value, sync_id: i64, expected_variant: i64) -> Result<Value,String> {
    let key=std::env::var("PRINTFUL_API_KEY").map_err(|_|"PRINTFUL_API_KEY required for sync variant")?;
    resolve_sync_line_at(line,sync_id,expected_variant,&key,"https://api.printful.com").await
}

pub(crate) async fn resolve_sync_line_at(mut line: Value, sync_id: i64, expected_variant: i64, key: &str, root: &str) -> Result<Value,String> {
    if key.is_empty() || sync_id<=0 { return Err("authenticated sync variant required".into()); }
    let client=reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).build().map_err(|e|e.to_string())?;
    let body=vendor_get(&client,key,&format!("{root}/store/variants/{sync_id}")).await?;
    let sync=&body["result"];
    if sync["id"].as_i64()!=Some(sync_id) || sync["synced"]!=true || sync["is_ignored"]!=false
        || sync["availability_status"]!="active" { return Err("sync variant unavailable or unsynced".into()); }
    let variant=sync["variant_id"].as_i64().filter(|v|*v>0).ok_or("sync base variant missing")?;
    let product=sync["product"]["product_id"].as_i64().filter(|v|*v>0).ok_or("sync product missing")?;
    if sync["product"]["variant_id"].as_i64()!=Some(variant)
        || (expected_variant>0 && expected_variant!=variant)
        || line["product_id"].as_i64().is_some_and(|p|p>0 && p!=product) { return Err("sync product/base variant changed; refusing color or size substitution".into()); }
    let mut fs=Vec::new();
    for f in sync["files"].as_array().ok_or("sync print files missing")? {
        if matches!(f["type"].as_str(),Some("preview"|"mockup")) { continue; }
        if f["status"]!="ok" { return Err("sync print file is not ready".into()); }
        let mut file=json!({"type":f["type"],"url":f["url"]});
        for field in ["position","options"] {if let Some(v)=f.get(field) {file[field]=v.clone();}}
        fs.push(file);
    }
    let options=sync["options"].as_array().ok_or("sync options missing")?;
    let mut item=json!({"variant_id":variant,"files":fs,"options":options,
        "quantity":line["qty"],"retail_price":format!("{:.2}",line["unit_amount"].as_i64().ok_or("price missing")? as f64)});
    let vendor=vendor_get(&client,key,&format!("{root}/products/{product}")).await?;
    let v=vendor_variant(&vendor["result"],product,variant)?;
    if v["in_stock"]!=true {return Err("sync selected variant stock unavailable".into());}
    normalize_item_files(&mut item,&vendor["result"])?;
    line["product_id"]=json!(product);line["sync_source_id"]=json!(sync_id);
    line["sync_size"]=sync["size"].clone();line["sync_color"]=sync["color"].clone();
    line["size"]=json!("FIXED");line["variants"]=json!({"FIXED":item});
    line.as_object_mut().unwrap().remove("size_field");
    line_availability(&line,None,Some(true))?;
    Ok(line)
}

pub async fn preflight_paid(db: &crate::Db, spec: &Value, session: &Value, items: &[Value]) -> Result<(),String> {
    // Gift recipient country is resolved and checked in submit, not the sender's.
    if session["metadata"]["gift_to"].as_str().is_some_and(|s|!s.is_empty())
        || session["metadata"]["gift_email"].as_str().is_some_and(|s|!s.is_empty()) { return Ok(()); }
    if items.is_empty() { return Ok(()); }
    let country=session["collected_information"]["shipping_details"]["address"]["country"].as_str()
        .or_else(||session["shipping_details"]["address"]["country"].as_str()).unwrap_or("");
    if let Err(e)=preflight(spec,Some(items),country).await {
        mark(&db.lock().unwrap(),session["id"].as_str().unwrap_or(""),"blocked_vendor_preflight",&e);
        return Err(e);
    }
    Ok(())
}

pub(crate) async fn preflight_at(spec: &Value, items: Option<&[Value]>, country: &str, key: &str, root: &str) -> Result<(),String> {
    if items.is_none() { return prepare_checkout_at(spec,country,key,root).await.map(|_|()); }
    let country=country.trim().to_uppercase();
    if country.len()!=2 || !country.bytes().all(|b|b.is_ascii_uppercase()) { return Err("vendor preflight: ISO-2 destination required".into()); }
    let lines=spec["lines"].as_array().ok_or("vendor preflight: snapshot lines missing")?;
    let mut selected=Vec::new();
    for line in lines {
        line_availability(line,Some(&country),Some(true))?;
        let route=line["route"].as_str().unwrap_or("");
        if !route.starts_with("printful") { continue; }
        let variants=line["variants"].as_object().filter(|v|!v.is_empty()).ok_or("vendor preflight: variants missing")?;
        if items.is_none() { for item in variants.values() { selected.push((line,item)); } }
    }
    if let Some(items)=items {
        for item in items {
            let matches: Vec<_>=lines.iter().filter(|l|l["route"].as_str().unwrap_or("").starts_with("printful"))
                .filter(|l|l["variants"].as_object().is_some_and(|v|v.values().any(|i|
                    i["variant_id"]==item["variant_id"] && i["files"]==item["files"] && i["options"]==item["options"])))
                .collect();
            if matches.is_empty() { return Err("vendor preflight: supplier item differs from frozen variants".into()); }
            for line in matches { selected.push((line,item)); }
        }
    }
    let client=reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).build().map_err(|e|e.to_string())?;
    let mut products=std::collections::HashMap::<i64,Value>::new();
    for (line,item) in selected {
        let product=line["product_id"].as_i64().filter(|p|*p>0).ok_or("vendor preflight: frozen product ID missing; manual review required")?;
        let variant=item["variant_id"].as_i64().filter(|v|*v>0).ok_or("vendor preflight: variant missing")?;
        if !products.contains_key(&product) {
            let mut request=client.get(format!("{root}/products/{product}"));
            if !key.is_empty() { request=request.bearer_auth(key); }
            let body: Value=request.send().await.map_err(|e|format!("vendor preflight GET: {e}"))?
                .error_for_status().map_err(|e|format!("vendor preflight GET: {e}"))?
                .json().await.map_err(|e|format!("vendor preflight JSON: {e}"))?;
            products.insert(product,body);
        }
        let result=&products[&product]["result"];
        if result["product"]["id"].as_i64()!=Some(product) || result["product"]["is_discontinued"].as_bool()!=Some(false) {
            return Err("vendor preflight: product missing, discontinued, or unknown".into());
        }
        let v=result["variants"].as_array().and_then(|vs|vs.iter().find(|v|v["id"].as_i64()==Some(variant)))
            .ok_or("vendor preflight: variant does not belong to product")?;
        if v["product_id"].as_i64()!=Some(product) { return Err("vendor preflight: variant product mismatch".into()); }
        let stock=v["in_stock"].as_bool().ok_or("vendor preflight: selected variant stock unknown")?;
        if !stock { return Err("vendor preflight: selected variant out of stock".into()); }
        line_availability(line,Some(&country),Some(stock))?;
        // Validate alias equivalence and duplicates on a scratch copy only.
        // Purchased snapshots and the frozen supplier request remain untouched.
        normalize_item_files(&mut item.clone(),result)?;
    }
    Ok(())
}

/// Commit the partner decision and canonical lifecycle together, or neither.
pub fn collab_approval(conn: &mut Connection, partner: &str, id: i64, action: &str, now: &str) -> Result<(), String> {
    let approved = match action { "approve" => 1, "hold" => -1, "reset" => 0, _ => return Err("bad action".into()) };
    let tx = conn.transaction().map_err(|e|e.to_string())?;
    let (slug, active, draft): (String,i64,i64) = tx.query_row(
        "SELECT slug,active,draft FROM collab_products WHERE id=? AND partner=?",params![id,partner],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
    let status = if approved != 1 { "review" } else if draft == 1 { "draft" }
        else if active == 1 { "live" } else { "approved" };
    let canonical = format!("COLLAB-{}-{}",partner.to_uppercase(),slug.to_uppercase());
    let n = tx.execute("UPDATE catalog_products SET status=?,is_active=?,updated_at=?
        WHERE sku=? AND brand=? AND legacy_source='collab_products' AND status NOT IN ('dead','retired')
        AND (json_extract(meta_json,'$.legacy_collab.slug') IS NULL OR json_extract(meta_json,'$.legacy_collab.slug')=?)
        AND (json_extract(meta_json,'$.legacy_collab.partner') IS NULL OR json_extract(meta_json,'$.legacy_collab.partner')=?)",
        params![status,i64::from(status=="live"),now,canonical,partner,slug,partner]).map_err(|e|e.to_string())?;
    if n != 1 { return Err("canonical product missing, retired, or provenance mismatch".into()); }
    tx.execute("UPDATE collab_products SET partner_approved=?,partner_updated_at=? WHERE id=? AND partner=?",
        params![approved,now,id,partner]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e|e.to_string())
}

pub fn collab_line(conn: &Connection, partner: &str, slug: &str, size: &str, qty: i64, unit: i64, purpose: OrderPurpose) -> Result<Value, String> {
    let canonical = format!("COLLAB-{}-{}",partner.to_uppercase(),slug.to_uppercase());
    let status: String = conn.query_row("SELECT status FROM catalog_products WHERE sku=?", [&canonical], |r|r.get(0)).map_err(|e|e.to_string())?;
    let allowed = match purpose {
        OrderPurpose::Sale => status == "live",
        OrderPurpose::PartnerSample => matches!(status.as_str(),"live" | "review" | "draft" | "approved"),
    };
    if !allowed { return Err("canonical catalog product is not available for this order purpose".into()); }
    let (name, route, map, files_raw, options): (String,String,Option<String>,Option<String>,Option<String>) = conn.query_row(
        "SELECT name,COALESCE(production_route,'sweep_manual'),printful_variant_map,printful_files,printful_options
         FROM collab_products WHERE slug=? AND partner=? AND active=1", params![slug,partner],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e| e.to_string())?;
    let mut line = json!({"sku":slug,"name":name,"route":route,"size":size.trim().to_uppercase(),"qty":qty,"unit_amount":unit});
    if route == "printful" {
        let (product,meta):(i64,Option<String>)=conn.query_row("SELECT COALESCE(printful_product_id,0),meta_json FROM catalog_products WHERE sku=?",
            [&canonical],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
        line["product_id"]=json!(product); line["meta_json"]=json!(meta);
        line_availability(&line,None,Some(true))?;
        let map: Value = serde_json::from_str(map.as_deref().ok_or("size map missing")?).map_err(|e| e.to_string())?;
        size_choices(&map)?;
        let vid = size_variant(&map,size)?;
        let f: Value = serde_json::from_str(files_raw.as_deref().ok_or("print files missing")?).map_err(|e| e.to_string())?;
        files(&f)?;
        let opts: Value = serde_json::from_str(options.as_deref().unwrap_or("[]")).map_err(|e| e.to_string())?;
        if !opts.is_array() { return Err("invalid print options".into()); }
        line["variants"] = json!({size.trim().to_uppercase(): {"variant_id":vid,"files":f,"options":opts}});
        line["variant_map"] = map;
    } else if !matches!(route.as_str(), "sweep_manual" | "pre_order") { return Err("unsupported collab route".into()); }
    Ok(line)
}

/// A durable request is frozen before the first POST. On every recovery, resolve external_id first.
pub async fn submit(db: &crate::Db, sid: &str, body: &Value, key: &str, endpoint: &str) -> Result<(reqwest::StatusCode,String), String> {
    let stripe_key=if sid.starts_with("crypto:") { None } else {
        Some(std::env::var("STRIPE_SECRET_KEY").map_err(|_|"STRIPE_SECRET_KEY unset")?)
    };
    submit_checked(db,sid,body,key,endpoint,stripe_key.as_deref().map(|key|(key,"https://api.stripe.com/v1"))).await
}

async fn submit_checked(db: &crate::Db, sid: &str, body: &Value, key: &str, endpoint: &str, stripe: Option<(&str,&str)>) -> Result<(reqwest::StatusCode,String), String> {
    if let Some((key,root))=stripe {
        let full=full_session_at(&json!({"id":sid}),key,root).await?;
        if !persist_payment(&db.lock().unwrap(),&full)? { return Err("payment blocked before supplier lookup".into()); }
    }
    let request: Value = {
        let conn = db.lock().unwrap();
        conn.execute("UPDATE catalog_orders SET fulfillment_request_json=COALESCE(fulfillment_request_json,?) WHERE stripe_session_id=?
            AND status IN ('submitting','gift_building') AND COALESCE(payment_status,'') NOT IN ('refunded','partially_refunded','voided')",
            params![body.to_string(),sid]).map_err(|e| e.to_string())?;
        let raw: String = conn.query_row("SELECT fulfillment_request_json FROM catalog_orders WHERE stripe_session_id=?", [sid], |r| r.get(0)).map_err(|e| e.to_string())?;
        let n = conn.execute("UPDATE catalog_orders SET status='sending',order_updated_at=datetime('now') WHERE stripe_session_id=? AND status IN ('submitting','gift_building')", [sid]).map_err(|e| e.to_string())?;
        if n != 1 { return Err("submission already claimed".into()); }
        serde_json::from_str(&raw).map_err(|e| e.to_string())?
    };
    for field in ["name","address1","city","country_code","zip"] {
        if request["recipient"][field].as_str().unwrap_or("").trim().is_empty() { return Err(format!("recipient {field} missing")); }
    }
    let items = request["items"].as_array().filter(|a| !a.is_empty()).ok_or("order items missing")?;
    for item in items { files(&item["files"])?; }
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(60)).build().map_err(|e| e.to_string())?;
    let ext = request["external_id"].as_str().filter(|s| !s.is_empty()).ok_or("external_id missing")?;
    let root = endpoint.split('?').next().unwrap_or(endpoint);
    let lookup = client.get(format!("{root}/@{}", urlencoding::encode(ext))).bearer_auth(key).send().await.map_err(|e| e.to_string())?;
    let status = lookup.status();
    let text = lookup.text().await.map_err(|e| e.to_string())?;
    if status.is_success() {
        let found: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if found["result"]["id"].is_null() || found["result"]["external_id"].as_str() != Some(ext) { return Err("invalid recovery response".into()); }
        if matches!(found["result"]["status"].as_str(),Some("canceled" | "cancelled" | "failed")) {
            return Err("existing vendor order requires manual review; never recreate it".into());
        }
        return Ok((status,text));
    }
    if status != reqwest::StatusCode::NOT_FOUND { return Err(format!("external_id lookup failed: {status}")); }
    let spec=load(&db.lock().unwrap(),sid)?;
    let vendor_root=root.strip_suffix("/orders").ok_or("invalid supplier endpoint")?;
    if let Err(e)=preflight_at(&spec,Some(items),request["recipient"]["country_code"].as_str().unwrap_or(""),key,vendor_root).await {
        mark(&db.lock().unwrap(),sid,"blocked_vendor_preflight",&e);
        return Err(e);
    }
    // Lookup itself may have taken up to 60s. Re-read current Stripe state
    // immediately before a new supplier POST, including Dashboard-only refunds.
    if let Some((key,root))=stripe {
        let full=full_session_at(&json!({"id":sid}),key,root).await?;
        if !persist_payment(&db.lock().unwrap(),&full)? { return Err("payment blocked before supplier send".into()); }
    }
    let sending: bool=db.lock().unwrap().query_row("SELECT EXISTS(SELECT 1 FROM catalog_orders WHERE stripe_session_id=?
        AND status='sending' AND COALESCE(payment_status,'') NOT IN ('refunded','partially_refunded','voided'))",[sid],|r|r.get(0)).map_err(|e|e.to_string())?;
    if !sending { return Err("submission blocked".into()); }
    let r = client.post(endpoint).bearer_auth(key).json(&request).send().await.map_err(|e| e.to_string())?;
    let status = r.status();
    Ok((status,r.text().await.map_err(|e| e.to_string())?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn database() -> crate::Db {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE catalog_orders(id INTEGER PRIMARY KEY, stripe_session_id TEXT UNIQUE NOT NULL,
            sku TEXT,amount_jpy INTEGER,status TEXT,created_at TEXT DEFAULT 'original',gift_json TEXT);
            CREATE TABLE catalog_products(sku TEXT,status TEXT,printful_product_id INTEGER DEFAULT 71,meta_json TEXT);
            CREATE TABLE collab_products(slug TEXT,partner TEXT,name TEXT,production_route TEXT,active INTEGER,
            printful_variant_map TEXT,printful_files TEXT,printful_options TEXT);").unwrap();
        migrate(&c).unwrap();
        Arc::new(Mutex::new(c))
    }
    fn item() -> Value { json!({"variant_id":123,"files":[{"type":"front","url":"https://example.test/immutable.png","position":{"width":100}}]}) }
    fn spec() -> Value { json!({"version":1,"kind":"catalog","route":"printful_dtg","lines":[{
        "sku":"A","product_id":71,"route":"printful_dtg","qty":3,"unit_amount":1200,"variants":{"FIXED":item()}}]}) }
    fn session() -> Value { json!({"id":"cs_test_123","currency":"jpy","payment_status":"paid","amount_total":3600,
        "line_items":{"has_more":false,"data":[{"quantity":3,"price":{"currency":"jpy","unit_amount":1200}}]}}) }
    fn reserve(db: &crate::Db) -> Value {
        let c = db.lock().unwrap();
        let d = draft(&c,"A",&spec()).unwrap();
        let mut s = session(); s["metadata"] = json!({"order_draft":d});
        attach(&c,&d,&s).unwrap(); s
    }

    #[test]
    fn strict_print_files_and_size_maps() {
        assert!(paid(&json!({"id":"free","payment_status":"no_payment_required"})));
        for status in ["unpaid","processing",""] { assert!(!paid(&json!({"id":"s","payment_status":status}))); }
        for f in [Value::Null,json!([]),json!({}),json!([{"url":"https://example.test/a"}]),
            json!([{"url":"","type":"front"}]),json!([{"url":"https://example.test/a","type":"front"},{"url":"https://example.test/b","type":"front"}])] {
            assert!(files(&f).is_err());
        }
        assert!(files(&item()["files"]).is_ok());
        assert_eq!(size_variant(&json!({"S":1,"M":2,"OS":3}),"M").unwrap(),2);
        assert!(size_variant(&json!({"S":1,"M":2,"OS":3}),"XL").is_err());
        assert_eq!(size_variant(&json!({"ONE SIZE":4}),"OS").unwrap(),4);
        assert!(size_variant(&json!({"ONE SIZE":4,"M":2}),"OS").is_err());
        assert!(size_variant(&json!({"M":0}),"M").is_err());
    }

    #[test]
    fn partner_sample_purpose_allows_review_and_draft_but_sale_does_not() {
        let db=database(); let c=db.lock().unwrap();
        c.execute("INSERT INTO catalog_products(sku,status) VALUES ('COLLAB-SWEEP-SHIRT','review')",[]).unwrap();
        c.execute("INSERT INTO collab_products VALUES ('shirt','sweep','sample','printful',1,?,?, '[]')",
            params![json!({"M":123}).to_string(),item()["files"].to_string()]).unwrap();
        for status in ["review","draft","approved","live","retired","dead"] {
            c.execute("UPDATE catalog_products SET status=?",[status]).unwrap();
            assert_eq!(collab_line(&c,"sweep","shirt","M",1,500,OrderPurpose::Sale).is_ok(),status=="live");
            assert_eq!(collab_line(&c,"sweep","shirt","M",1,500,OrderPurpose::PartnerSample).is_ok(),
                matches!(status,"review"|"draft"|"approved"|"live"));
        }
        c.execute("UPDATE catalog_products SET status='draft'",[]).unwrap();
        assert!(collab_line(&c,"other","shirt","M",1,500,OrderPurpose::PartnerSample).is_err());
        assert!(collab_line(&c,"sweep","shirt","XL",1,500,OrderPurpose::PartnerSample).is_err());
        c.execute("UPDATE collab_products SET printful_files='[]'",[]).unwrap();
        assert!(collab_line(&c,"sweep","shirt","M",1,500,OrderPurpose::PartnerSample).is_err());
    }

    #[test]
    fn partner_approval_syncs_only_matching_product_and_rolls_back_on_failure() {
        let mut c=Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE collab_products(id INTEGER PRIMARY KEY,slug TEXT,partner TEXT,active INTEGER,draft INTEGER,partner_approved INTEGER,partner_updated_at TEXT);
            CREATE TABLE catalog_products(sku TEXT,brand TEXT,status TEXT,is_active INTEGER,legacy_source TEXT,meta_json TEXT,updated_at TEXT);
            INSERT INTO collab_products VALUES(1,'shirt','sweep',1,0,0,'old'),(2,'shirt','other',1,0,0,'old');
            INSERT INTO catalog_products VALUES('COLLAB-SWEEP-SHIRT','sweep','review',0,'collab_products','{}','old'),
                ('COLLAB-OTHER-SHIRT','other','review',0,'collab_products','{}','old');").unwrap();
        for (action,status,active,approved) in [("approve","live",1,1),("hold","review",0,-1),("reset","review",0,0)] {
            collab_approval(&mut c,"sweep",1,action,"now").unwrap();
            let actual:(String,i64,i64)=c.query_row("SELECT c.status,c.is_active,p.partner_approved FROM catalog_products c JOIN collab_products p ON p.id=1 WHERE c.brand='sweep'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
            assert_eq!(actual,(status.into(),active,approved));
            assert_eq!(c.query_row("SELECT status FROM catalog_products WHERE brand='other'",[],|r|r.get::<_,String>(0)).unwrap(),"review");
        }
        for edit in ["UPDATE catalog_products SET legacy_source='make' WHERE brand='sweep'",
            "UPDATE catalog_products SET legacy_source='collab_products',meta_json='{\"legacy_collab\":{\"slug\":\"wrong\"}}' WHERE brand='sweep'"] {
            c.execute(edit,[]).unwrap();
            assert!(collab_approval(&mut c,"sweep",1,"approve","later").is_err());
            assert_eq!(c.query_row("SELECT partner_approved FROM collab_products WHERE id=1",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        }
        c.execute("UPDATE catalog_products SET meta_json='{}' WHERE brand='sweep'",[]).unwrap();
        c.execute_batch("CREATE TRIGGER fail_legacy BEFORE UPDATE ON collab_products BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
        assert!(collab_approval(&mut c,"sweep",1,"approve","later").is_err());
        assert_eq!(c.query_row("SELECT status FROM catalog_products WHERE brand='sweep'",[],|r|r.get::<_,String>(0)).unwrap(),"review");
    }

    #[test]
    fn paid_lines_fail_closed_and_unit_price_is_not_total() {
        let s = session();
        let i = purchased_items(&spec(),&s).unwrap();
        assert_eq!(i[0]["quantity"],3); assert_eq!(i[0]["retail_price"],"1200.00");
        for bad in [json!(null),json!({"data":[]}),json!({"data":[{"quantity":1,"price":{"unit_amount":1200,"currency":"jpy"}}]})] {
            let mut s = s.clone(); s["line_items"] = bad;
            assert!(purchased_items(&spec(),&s).is_err());
        }
        let mut s = s.clone(); s["line_items"]["has_more"] = json!(true);
        assert!(purchased_items(&spec(),&s).is_err());
        let mut p = spec(); p["lines"][0]["size_field"] = json!("size");
        assert!(purchased_items(&p,&session()).is_err());
        let mut mixed = spec();
        mixed["lines"].as_array_mut().unwrap().push(json!({"sku":"manual","route":"sweep_manual","qty":2,"unit_amount":500}));
        let mut payment = session();
        payment["line_items"]["data"].as_array_mut().unwrap().push(json!({"quantity":2,"price":{"unit_amount":500,"currency":"jpy"}}));
        assert_eq!(purchased_items(&mixed,&payment).unwrap().len(),1);
        mixed["lines"][1]["route"] = json!("printful");
        assert!(purchased_items(&mixed,&payment).is_err());
    }

    #[test]
    fn atomic_paid_claim_and_retry_preserve_history() {
        let db = database(); let s = reserve(&db);
        let mut unpaid = s.clone(); unpaid["payment_status"] = json!("unpaid");
        assert!(claim(&db.lock().unwrap(),&unpaid).unwrap().is_none());
        let workers: Vec<_> = (0..12).map(|_| { let db = db.clone(); let s = s.clone();
            std::thread::spawn(move || claim(&db.lock().unwrap(),&s).unwrap().is_some()) }).collect();
        assert_eq!(workers.into_iter().filter_map(|w| w.join().ok()).filter(|v| *v).count(),1);
        let c = db.lock().unwrap();
        c.execute("UPDATE catalog_orders SET gift_json='keep'",[]).unwrap();
        let id: i64 = c.query_row("SELECT id FROM catalog_orders",[],|r| r.get(0)).unwrap();
        assert!(effects_once(&c,"cs_test_123").unwrap());
        for attempt in 1..=3 {
            mark(&c,"cs_test_123","submission_uncertain","");
            assert!(queue_retry(&c,id).unwrap()); assert!(!queue_retry(&c,id).unwrap());
            assert!(claim(&c,&s).unwrap().is_some());
            assert!(!effects_once(&c,"cs_test_123").unwrap());
            let row: (i64,i64,String,String) = c.query_row("SELECT id,retry_count,created_at,gift_json FROM catalog_orders",[],|r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
            assert_eq!(row,(id,attempt,"original".into(),"keep".into()));
        }
        mark(&c,"cs_test_123","submission_uncertain",""); assert!(!queue_retry(&c,id).unwrap());
        for terminal in ["submitted","refunded","ticket_delivered","manual_pending","collab_complete"] {
            c.execute("UPDATE catalog_orders SET status=?,retry_count=0",[terminal]).unwrap();
            assert!(!queue_retry(&c,id).unwrap()); assert!(claim(&c,&s).unwrap().is_none());
        }
    }

    #[test]
    fn snapshot_survives_product_edit_and_missing_legacy_is_held() {
        let db = database(); let c = db.lock().unwrap();
        c.execute("INSERT INTO catalog_products(sku,status) VALUES ('COLLAB-NAKAMURA-SHIRT','live')",[]).unwrap();
        c.execute("INSERT INTO collab_products VALUES ('shirt','nakamura','original','printful',1,?,?, '[]')",
            params![json!({"M":123}).to_string(),item()["files"].to_string()]).unwrap();
        let line = collab_line(&c,"nakamura","shirt","M",3,1200,OrderPurpose::Sale).unwrap();
        c.execute("UPDATE catalog_products SET status='retired'",[]).unwrap();
        assert!(collab_line(&c,"nakamura","shirt","M",3,1200,OrderPurpose::Sale).is_err());
        let p = json!({"kind":"collab","lines":[line]});
        let d = draft(&c,"shirt",&p).unwrap(); attach(&c,&d,&session()).unwrap();
        c.execute("UPDATE collab_products SET printful_files='[]',printful_variant_map='{}',production_route='pre_order',active=0",[]).unwrap();
        let i = purchased_items(&load(&c,"cs_test_123").unwrap(),&session()).unwrap();
        assert_eq!(i[0]["variant_id"],123); assert_eq!(i[0]["files"],item()["files"]);
        let saved = freeze_items(&c,"cs_test_123",&i).unwrap();
        assert_eq!(freeze_items(&c,"cs_test_123",&[json!({"variant_id":999})]).unwrap(),saved);
        let old = json!({"id":"old","payment_status":"paid"});
        assert!(claim(&c,&old).is_err());
        assert_eq!(c.query_row("SELECT status FROM catalog_orders WHERE stripe_session_id='old'",[],|r|r.get::<_,String>(0)).unwrap(),"blocked_legacy_snapshot");
        migrate(&c).unwrap();
        assert_eq!(load(&c,"cs_test_123").unwrap(),p);
    }

    async fn fake_gateway(found: Arc<Mutex<bool>>, posts: Arc<Mutex<Vec<Value>>>, lookup_status: u16) -> (String,tokio::task::JoinHandle<()>) {
        use axum::{routing::{get,post}, Json, http::StatusCode};
        let f = found.clone();
        let router = axum::Router::new().route("/products/71",get(|| async { Json(vendor_product(71,123,Some(true))) }))
        .route("/orders/@ext",get(move || { let f=f.clone(); async move {
            if lookup_status != 200 { return (StatusCode::from_u16(lookup_status).unwrap(),Json(json!({}))); }
            if *f.lock().unwrap() { (StatusCode::OK,Json(json!({"result":{"id":42,"external_id":"ext"}}))) }
            else { (StatusCode::NOT_FOUND,Json(json!({}))) }
        }})).route("/orders",post(move |Json(body): Json<Value>| { let found=found.clone(); let posts=posts.clone(); async move {
            posts.lock().unwrap().push(body); *found.lock().unwrap()=true;
            // Vendor accepted, but the success response was lost/returned as an error.
            (StatusCode::BAD_GATEWAY,Json(json!({"error":"response lost"})))
        }}));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr=listener.local_addr().unwrap();
        let task=tokio::spawn(async move { axum::serve(listener,router).await.unwrap(); });
        (format!("http://{addr}/orders"),task)
    }
    fn request() -> Value { json!({"external_id":"ext","recipient":{"name":"Test","address1":"1 Test","city":"Test","zip":"000","country_code":"JP"},"items":[item()]}) }

    fn vendor_product(product: i64, variant: i64, stock: Option<bool>) -> Value {
        json!({"result":{"product":{"id":product,"is_discontinued":false,"files":[{"type":"front"}]},
            "variants":[{"id":variant,"product_id":product,"in_stock":stock,"availability_regions":{"AU":"Australia"}}]}})
    }

    #[tokio::test]
    async fn checkout_filters_stock_normalizes_only_primary_alias_and_freezes_sync() {
        use axum::{routing::get,Json};
        let mut vendor=vendor_product(71,123,Some(false));
        // Exact product 71 file descriptor shape from the GET audit.
        vendor["result"]["product"]["files"]=json!([
            {"id":"embroidery_front","type":"embroidery_front","additional_price":"2.95"},
            {"id":"default","type":"front","title":"Front print","additional_price":null,"options":[]},
            {"id":"back_alias","type":"back"}]);
        vendor["result"]["variants"].as_array_mut().unwrap().push(json!({"id":124,"product_id":71,"in_stock":true}));
        let current=Arc::new(Mutex::new(vendor));let v=current.clone();
        // Official SyncVariant response: product is NOT sync_product_id.
        let sync=Arc::new(Mutex::new(json!({"result":{"id":10,"sync_product_id":999,"synced":true,
            "is_ignored":false,"availability_status":"active","variant_id":124,
            "product":{"variant_id":124,"product_id":71},"size":"XL","color":"White",
            "files":[{"id":40,"type":"default","url":"https://example.test/sync.png","status":"ok",
                "options":[{"id":"template_type","value":"native"}]},
                {"id":41,"type":"preview","url":"https://example.test/preview.png","status":"ok"}],
            "options":[{"id":"embroidery_type","value":"flat"}]}})));
        let s=sync.clone();
        let app=axum::Router::new().route("/products/:id",get(move || {let v=v.clone();async move {Json(v.lock().unwrap().clone())}}))
            .route("/store/variants/10",get(move || {let s=s.clone();async move {Json(s.lock().unwrap().clone())}}));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let root=format!("http://{}",listener.local_addr().unwrap());
        let server=tokio::spawn(async move {axum::serve(listener,app).await.unwrap();});
        let mut p=spec();let mut xl=item();xl["variant_id"]=json!(124);xl["files"][0]["type"]=json!("default");
        p["lines"][0]["variants"]=json!({"M":item(),"XL":xl});p["lines"][0]["size_field"]=json!("size");p["lines"][0]["size"]=json!("M");
        let prepared=prepare_checkout_at(&p,"JP","",&root).await.unwrap();
        assert_eq!(prepared["lines"][0]["size"],"XL");
        assert_eq!(prepared["lines"][0]["variant_map"],json!({"XL":124}));
        assert_eq!(prepared["lines"][0]["variants"]["XL"]["files"][0]["type"],"front");
        assert_eq!(p["lines"][0]["variants"]["XL"]["files"][0]["type"],"default");
        assert!(preflight_at(&p,Some(&[xl.clone()]),"JP","",&root).await.is_ok());
        let mut bad=xl.clone();bad["files"][0]["type"]=json!("back_alias");
        assert!(normalize_item_files(&mut bad,&current.lock().unwrap()["result"]).is_err());
        let mut fixed=p.clone();fixed["lines"][0].as_object_mut().unwrap().remove("size_field");
        fixed["lines"][0]["variants"]=json!({"M":item()});
        assert!(prepare_checkout_at(&fixed,"JP","",&root).await.is_err());
        let mut paid=session();paid["custom_fields"]=json!([{"key":"size","dropdown":{"value":"M"}}]);
        assert!(purchased_items(&prepared,&paid).is_err());
        current.lock().unwrap()["result"]["variants"][1]["in_stock"]=json!(false);
        assert!(prepare_checkout_at(&p,"JP","",&root).await.is_err());
        current.lock().unwrap()["result"]["variants"][1]["in_stock"]=json!(true);
        let base=json!({"sku":"SYNC-TEE","product_id":71,"qty":1,"unit_amount":5000,"route":"printful_dtg"});
        let resolved=resolve_sync_line_at(base.clone(),10,124,"fake",&root).await.unwrap();
        assert_eq!(resolved["variants"]["FIXED"]["variant_id"],124);
        assert_eq!(resolved["variants"]["FIXED"]["files"].as_array().unwrap().len(),1);
        assert_eq!(resolved["variants"]["FIXED"]["files"][0]["type"],"front");
        assert_eq!(resolved["sync_color"],"White");
        assert!(resolve_sync_line_at(base.clone(),10,123,"fake",&root).await.is_err());
        assert!(resolve_sync_line_at(base.clone(),10,124,"",&root).await.is_err());
        let db=database();let snapshot=json!({"lines":[resolved]});
        let id=draft(&db.lock().unwrap(),"SYNC-TEE",&snapshot).unwrap();
        sync.lock().unwrap()["result"]["files"][0]["url"]=json!("https://example.test/edited.png");
        assert_eq!(load(&db.lock().unwrap(),&id).unwrap(),snapshot);
        sync.lock().unwrap()["result"]["files"][0]["status"]=json!("waiting");
        assert!(resolve_sync_line_at(base,10,124,"fake",&root).await.is_err());
        // Real collar product scenario: XL restocked, the old M default isn't.
        let mut collar=p.clone();collar["lines"][0]["product_id"]=json!(902);collar["lines"][0]["product_kind"]=json!("pet_collar");
        {
            let mut v=current.lock().unwrap();v["result"]["product"]["id"]=json!(902);
            for variant in v["result"]["variants"].as_array_mut().unwrap() {variant["product_id"]=json!(902);}
        }
        let collar=prepare_checkout_at(&collar,"JP","",&root).await.unwrap();
        assert_eq!(collar["lines"][0]["variant_map"],json!({"XL":124}));
        assert_eq!(collar["lines"][0]["size"],"XL");
        server.abort();
    }

    #[tokio::test]
    async fn vendor_preflight_membership_stock_placement_country_and_disabled_kinds_never_post() {
        use axum::{routing::{get,post},Json,http::StatusCode};
        let current=Arc::new(Mutex::new(vendor_product(71,123,Some(true))));
        let state=current.clone();
        let posts=Arc::new(std::sync::atomic::AtomicUsize::new(0)); let count=posts.clone();
        let app=axum::Router::new().route("/products/:id",get(move || {let s=state.clone(); async move {
            let body=s.lock().unwrap().clone();
            let status=if body["rate_limited"]==true {StatusCode::TOO_MANY_REQUESTS} else {StatusCode::OK};
            (status,Json(body))
        }}))
            .route("/orders/@ext",get(||async {(StatusCode::NOT_FOUND,Json(json!({})))}))
            .route("/orders",post(move || {let n=count.clone(); async move {n.fetch_add(1,std::sync::atomic::Ordering::SeqCst);StatusCode::OK}}));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let root=format!("http://{}",listener.local_addr().unwrap());
        let server=tokio::spawn(async move {axum::serve(listener,app).await.unwrap();});
        for case in ["membership","missing_variant","stock","unknown","discontinued","placement","country","dog","photo","429","missing_product"] {
            let db=database(); let s=reserve(&db); claim(&db.lock().unwrap(),&s).unwrap();
            let mut p=spec(); let mut v=vendor_product(71,123,Some(true));
            match case {
                "membership"=>v["result"]["variants"][0]["product_id"]=json!(99),
                "missing_variant"=>v["result"]["variants"][0]["id"]=json!(999),
                "stock"=>v["result"]["variants"][0]["in_stock"]=json!(false),
                "unknown"=>v["result"]["variants"][0]["in_stock"]=Value::Null,
                "discontinued"=>v["result"]["product"]["is_discontinued"]=json!(true),
                "placement"=>v["result"]["product"]["files"]=json!([{"type":"back"}]),
                "country"=>{p["lines"][0]["product_id"]=json!(539);v=vendor_product(539,123,Some(true));},
                "dog"=>p["lines"][0]["sku"]=json!("MAKE-DOG-TEE-old"),
                "photo"=>{p["lines"][0]["product_id"]=json!(1564);v=vendor_product(1564,123,Some(true));},
                "429"=>v["rate_limited"]=json!(true),
                "missing_product"=>p["lines"][0]["product_id"]=Value::Null,
                _=>unreachable!(),
            }
            *current.lock().unwrap()=v;
            db.lock().unwrap().execute("UPDATE catalog_orders SET checkout_spec_json=?",[p.to_string()]).unwrap();
            assert!(submit_checked(&db,"cs_test_123",&request(),"",&format!("{root}/orders"),None).await.is_err(),"{case}");
            let c=db.lock().unwrap(); mark(&c,"cs_test_123","submission_uncertain","late caller");
            assert_eq!(c.query_row("SELECT status FROM catalog_orders",[],|r|r.get::<_,String>(0)).unwrap(),"blocked_vendor_preflight","{case}");
        }
        assert_eq!(posts.load(std::sync::atomic::Ordering::SeqCst),0);
        // Fresh stock releases the dated pet_collar hold, and AU production
        // location does not prohibit shipping an unrestricted item to JP.
        *current.lock().unwrap()=vendor_product(902,123,Some(true));
        let mut p=spec(); p["lines"][0]["product_id"]=json!(902);p["lines"][0]["product_kind"]=json!("pet_collar");
        assert!(preflight_at(&p,None,"JP","",&root).await.is_ok());
        current.lock().unwrap()["result"]["variants"][0]["in_stock"]=json!(false);
        assert!(preflight_at(&p,None,"JP","",&root).await.is_err());
        for (product,kind,country,ok) in [(539,"tank","AU",true),(539,"tank","JP",false),
            (678,"pet_bowl","US",true),(678,"pet_bowl","JP",false),(786,"notepad","US",true),
            (786,"notepad","NZ",false),(635,"towel","JP",true),(635,"towel","US",false)] {
            p["lines"][0]["product_id"]=json!(product);p["lines"][0]["product_kind"]=json!(kind);
            *current.lock().unwrap()=vendor_product(product,123,Some(true));
            assert_eq!(preflight_at(&p,None,country,"",&root).await.is_ok(),ok,"{kind}/{country}");
        }
        // Stripe-selected L must be checked, not the stocked default M.
        let mut p=spec(); let mut large=item();large["variant_id"]=json!(124);
        p["lines"][0]["variants"]=json!({"M":item(),"L":large});p["lines"][0]["size_field"]=json!("size");
        let mut v=vendor_product(71,123,Some(true));
        v["result"]["variants"].as_array_mut().unwrap().push(json!({"id":124,"product_id":71,"in_stock":false}));
        *current.lock().unwrap()=v;
        let mut paid=session();paid["custom_fields"]=json!([{"key":"size","dropdown":{"value":"L"}}]);
        let selected=purchased_items(&p,&paid).unwrap();
        assert!(preflight_at(&p,Some(&selected),"JP","",&root).await.is_err());
        paid["custom_fields"][0]["dropdown"]["value"]=json!("M");
        assert!(preflight_at(&p,Some(&purchased_items(&p,&paid).unwrap()),"JP","",&root).await.is_ok());
        let filtered=prepare_checkout_at(&p,"JP","",&root).await.unwrap();
        assert_eq!(filtered["lines"][0]["variant_map"],json!({"M":123}));
        // Two paid supplier lines: an in-stock first line cannot hide an
        // unavailable second one. No positional/default-variant fallback.
        let mut two=spec();
        let mut second=two["lines"][0].clone();second["sku"]=json!("B");
        second["variants"]=json!({"FIXED":large});
        two["lines"].as_array_mut().unwrap().push(second);
        assert!(preflight_at(&two,Some(&[item(),large]),"JP","",&root).await.is_err());
        server.abort();
    }

    fn captured() -> Value {
        let mut s=session();
        s["payment_intent"]=json!({"id":"pi_test","status":"succeeded","latest_charge":{
            "id":"ch_test","status":"succeeded","paid":true,"captured":true,"refunded":false,"amount_refunded":0}});
        s
    }

    #[tokio::test]
    async fn current_charge_overrides_cached_paid_and_blocks_supplier_and_retry() {
        use axum::{routing::get,Json};
        let current=Arc::new(Mutex::new(captured()));
        let state=current.clone();
        let app=axum::Router::new().route("/checkout/sessions/cs_test_123",get(move || {
            let state=state.clone(); async move { Json(state.lock().unwrap().clone()) }
        }));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let root=format!("http://{}",listener.local_addr().unwrap());
        let server=tokio::spawn(async move { axum::serve(listener,app).await.unwrap(); });
        for (status,amount,refunded,canceled) in [("partially_refunded",100,false,false),("refunded",3600,true,false),("voided",0,false,true)] {
            let db=database(); let s=reserve(&db); enqueue(&db.lock().unwrap(),&s).unwrap();
            claim(&db.lock().unwrap(),&s).unwrap();
            let mut changed=captured();
            changed["payment_intent"]["latest_charge"]["amount_refunded"]=json!(amount);
            changed["payment_intent"]["latest_charge"]["refunded"]=json!(refunded);
            if canceled { changed["payment_intent"]["status"]=json!("canceled"); }
            *current.lock().unwrap()=changed;
            // Even a complete cached session must result in an HTTP fetch.
            let full=full_session_at(&captured(),"local",&root).await.unwrap();
            assert!(paid(&full)); assert!(!payment_clear(&full));
            assert!(!persist_payment(&db.lock().unwrap(),&full).unwrap());
            assert!(submit_checked(&db,"cs_test_123",&request(),"local","http://127.0.0.1:1/orders",Some(("local",&root))).await.is_err());
            let c=db.lock().unwrap();
            mark(&c,"cs_test_123","submission_uncertain","late worker");
            assert!(!queue_retry(&c,1).unwrap());
            assert!(claim(&c,&captured()).unwrap().is_none());
            assert!(freeze_items(&c,"cs_test_123",&[item()]).is_err());
            assert!(c.execute("UPDATE catalog_orders SET status='submitted' WHERE stripe_session_id='cs_test_123'",[]).is_err());
            assert_eq!(c.query_row("SELECT status FROM catalog_orders WHERE stripe_session_id='cs_test_123'",[],|r|r.get::<_,String>(0)).unwrap(),status);
        }
        server.abort();
    }

    #[tokio::test]
    async fn refund_during_supplier_lookup_is_rechecked_before_post() {
        use axum::{routing::{get,post},Json,http::StatusCode};
        let current=Arc::new(Mutex::new(captured()));
        let stripe=current.clone(); let supplier=current.clone();
        let posts=Arc::new(std::sync::atomic::AtomicUsize::new(0)); let count=posts.clone();
        let app=axum::Router::new()
            .route("/products/71",get(|| async {Json(vendor_product(71,123,Some(true)))}))
            .route("/checkout/sessions/cs_test_123",get(move || {let s=stripe.clone(); async move {Json(s.lock().unwrap().clone())}}))
            .route("/orders/@ext",get(move || {let s=supplier.clone(); async move {
                s.lock().unwrap()["payment_intent"]["latest_charge"]["amount_refunded"]=json!(100);
                (StatusCode::NOT_FOUND,Json(json!({})))
            }}))
            .route("/orders",post(move || {let n=count.clone(); async move {n.fetch_add(1,std::sync::atomic::Ordering::SeqCst);StatusCode::OK}}));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let root=format!("http://{}",listener.local_addr().unwrap());
        let server=tokio::spawn(async move {axum::serve(listener,app).await.unwrap();});
        let db=database(); let s=reserve(&db); claim(&db.lock().unwrap(),&s).unwrap();
        let error=submit_checked(&db,"cs_test_123",&request(),"test",&format!("{root}/orders"),Some(("test",&root))).await.unwrap_err();
        assert!(error.contains("before supplier send"));
        assert_eq!(posts.load(std::sync::atomic::Ordering::SeqCst),0);
        assert_eq!(db.lock().unwrap().query_row("SELECT status FROM catalog_orders WHERE stripe_session_id='cs_test_123'",[],|r|r.get::<_,String>(0)).unwrap(),"partially_refunded");
        server.abort();
    }

    #[test]
    fn enqueue_survives_missing_worker_and_refund_before_payment_event() {
        for kind in ["catalog","collab","collab_sample"] {
            let db=database(); let c=db.lock().unwrap();
            let mut p=spec(); p["kind"]=json!(kind);
            let draft=draft(&c,"A",&p).unwrap(); let mut s=captured(); s["metadata"]=json!({"order_draft":draft});
            enqueue(&c,&s).unwrap(); enqueue(&c,&s).unwrap();
            let raw:String=c.query_row("SELECT session_json FROM catalog_orders WHERE status='payment_ready'",[],|r|r.get(0)).unwrap();
            let recovered:Value=serde_json::from_str(&raw).unwrap();
            assert_eq!(claim(&c,&recovered).unwrap().unwrap()["kind"],kind);
            assert!(claim(&c,&recovered).unwrap().is_none());
        }
        let db=database(); let s=reserve(&db); let c=db.lock().unwrap();
        c.execute_batch("CREATE TRIGGER fail_enqueue BEFORE UPDATE OF session_json ON catalog_orders BEGIN SELECT RAISE(ABORT,'disk full'); END;").unwrap();
        assert!(enqueue(&c,&s).is_err());
        assert_eq!(c.query_row("SELECT status FROM catalog_orders",[],|r|r.get::<_,String>(0)).unwrap(),"checkout_pending");
        c.execute_batch("DROP TRIGGER fail_enqueue").unwrap();
        block_payment(&c,Some("pi_test"),None,"partially_refunded").unwrap();
        enqueue(&c,&captured()).unwrap();
        assert!(claim(&c,&captured()).unwrap().is_none());
        block_payment(&c,Some("pi_test"),Some("cs_test_123"),"refunded").unwrap();
        assert_eq!(c.query_row("SELECT status FROM catalog_orders WHERE stripe_session_id='cs_test_123'",[],|r|r.get::<_,String>(0)).unwrap(),"refunded");
    }

    #[test]
    fn elapsed_time_never_steals_an_active_worker() {
        let db=database(); let s=reserve(&db); let c=db.lock().unwrap();
        claim(&c,&s).unwrap();
        for status in ["submitting","sending","gift_building","retry_ready"] {
            c.execute("UPDATE catalog_orders SET status=?,order_updated_at='2000-01-01'",[status]).unwrap();
            assert!(!queue_retry(&c,1).unwrap(),"{status}");
        }
    }

    #[test]
    fn reward_crash_rolls_back_and_replay_does_not_duplicate_balance() {
        let db=database(); let c=db.lock().unwrap();
        c.execute_batch("CREATE TABLE mu_credits(email TEXT PRIMARY KEY,balance_jpy INTEGER,total_earned_jpy INTEGER,total_spent_jpy INTEGER,updated_at TEXT);
            CREATE TABLE mu_credit_ledger(email TEXT,delta_jpy INTEGER,reason TEXT,ref_id TEXT,created_at TEXT);").unwrap();
        assert!(credit_once(&c,"cs_rewards","maker@test",100,"creator:A",|_|Err(rusqlite::Error::InvalidQuery)).is_err());
        assert_eq!(c.query_row("SELECT COUNT(*) FROM mu_credits",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        assert_eq!(c.query_row("SELECT COUNT(*) FROM mu_credit_ledger",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        assert!(credit_once(&c,"cs_rewards","maker@test",100,"creator:A",|_|Ok(())).unwrap());
        // Simulated crash after one commission, before the remaining commissions.
        assert!(!credit_once(&c,"cs_rewards","maker@test",100,"creator:A",|_|panic!("duplicate accounting")).unwrap());
        assert!(credit_once(&c,"cs_rewards","maker@test",50,"remix_royalty:A",|_|Ok(())).unwrap());
        assert_eq!(c.query_row("SELECT balance_jpy FROM mu_credits",[],|r|r.get::<_,i64>(0)).unwrap(),150);
        assert_eq!(c.query_row("SELECT SUM(delta_jpy) FROM mu_credit_ledger",[],|r|r.get::<_,i64>(0)).unwrap(),150);
    }

    #[tokio::test]
    async fn uncertain_submission_recovers_without_duplicate_post_or_mutated_request() {
        let db=database(); let s=reserve(&db); claim(&db.lock().unwrap(),&s).unwrap();
        let posts=Arc::new(Mutex::new(Vec::new()));
        let (url,task)=fake_gateway(Arc::new(Mutex::new(false)),posts.clone(),200).await;
        assert_eq!(submit_checked(&db,"cs_test_123",&request(),"test",&url,None).await.unwrap().0,reqwest::StatusCode::BAD_GATEWAY);
        { let c=db.lock().unwrap(); mark(&c,"cs_test_123","submission_uncertain",""); assert!(queue_retry(&c,1).unwrap()); claim(&c,&s).unwrap(); }
        let mut edited=request(); edited["items"][0]["variant_id"]=json!(999);
        assert!(submit_checked(&db,"cs_test_123",&edited,"test",&url,None).await.unwrap().0.is_success());
        assert_eq!(posts.lock().unwrap().as_slice(),&[request()]);
        assert!(submit_checked(&db,"cs_test_123",&edited,"test",&url,None).await.is_err());
        task.abort();
    }

    #[tokio::test]
    async fn rate_limited_lookup_never_posts_or_refunds() {
        let db=database(); let s=reserve(&db); claim(&db.lock().unwrap(),&s).unwrap();
        let posts=Arc::new(Mutex::new(Vec::new()));
        let (url,task)=fake_gateway(Arc::new(Mutex::new(false)),posts.clone(),429).await;
        assert!(submit_checked(&db,"cs_test_123",&request(),"test",&url,None).await.unwrap_err().contains("429"));
        assert!(posts.lock().unwrap().is_empty());
        assert_eq!(db.lock().unwrap().query_row("SELECT status FROM catalog_orders",[],|r|r.get::<_,String>(0)).unwrap(),"sending");
        task.abort();
    }
}
