//! MU 出品代行エンジン (consignment / 買取・委託・お預かり)
//!
//! 「写真を撮るだけ → AI査定 → ① いますぐ買取 / ② 待って高く委託 / ③ 預ける」を
//! 1モジュールに集約する。main.rs への変更は (a) `mod consign;` (b) ルート登録
//! (c) 起動時 `consign::init_db(&conn)` の3点だけ。既存テーブル/ルート/関数には触れない。
//!
//! ── 法務の不変条件（コードで強制。緩めない） ──────────────────────────
//! 1. サイト内残高 (consign_balance_ledger) は付与時に expires_at = 付与日 + 6ヶ月。
//!    出金/換金APIは存在しない（資金移動業を回避）。残高は MU 内購入にのみ使える。
//! 2. 売り系 intent (sell=即時買取 / consign=委託) は、active な consign_partners 行
//!    (古物商許可番号 license_no を持つ提携先) が無い限り全て gated。ユーザーには開放せず
//!    「準備中（提携古物商の登録待ち）」を返す。store(預ける) のみ常時受理。
//! 3. worker/利用者の資金を MU がプール/運用/分配するロジックは作らない
//!    (集団投資スキーム/出資法を回避)。残高は「MU が出品者に付与する債務」であって
//!    第三者の資金の保管・運用ではない。
//! 4. 取引DPF法対応ページ(販売業者情報開示・苦情窓口)と特商法表記ページを提供する。
//! 5. AI査定額の表示は景表法配慮: 「査定額は確約ではない / 即時買取は減額前提 / 手数料明示」。

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::Db;

// ── バンドルする HTML テンプレ (include_str! / Docker build context は store/) ──
const CONSIGN_HTML: &str = include_str!("../static/consign/consign.html");
const BALANCE_HTML: &str = include_str!("../static/consign/balance.html");
const SELLERS_HTML: &str = include_str!("../static/consign/sellers.html");
const TOKUSHOHO_HTML: &str = include_str!("../static/consign/tokushoho.html");

// ── 法務定数 ──────────────────────────────────────────────────────────
/// 残高の有効期限: 付与日 + 6ヶ月。
const BALANCE_TTL_SECS: i64 = 182 * 86400; // ≒ 6ヶ月
/// 即時買取は早さと引き換えに減額 (査定額の 70%)。景表法: 減額前提を明示。
const SELL_NOW_RATE: f64 = 0.70;
/// 委託手数料 (販売価格の 20%)。委託で残高に乗るのは査定額 × (1 - これ)。
const CONSIGN_FEE_RATE: f64 = 0.20;
/// 査定レンジ幅 (estimate に対する下限/上限係数)。
const RANGE_LOW: f64 = 0.75;
const RANGE_HIGH: f64 = 1.30;

// ── スキーマ ──────────────────────────────────────────────────────────
pub(crate) fn init_db(conn: &Connection) {
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS consign_partners (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL,
            license_no  TEXT NOT NULL,           -- 古物商許可番号 (空不可)
            authority   TEXT NOT NULL DEFAULT '',-- 公安委員会名
            address     TEXT NOT NULL DEFAULT '',
            contact     TEXT NOT NULL DEFAULT '',
            active      INTEGER NOT NULL DEFAULT 1,
            created_at  INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS consign_items (
            id          TEXT PRIMARY KEY,        -- 受付ID (公開可・推測不能)
            email       TEXT NOT NULL,
            category    TEXT NOT NULL DEFAULT '',
            memo        TEXT NOT NULL DEFAULT '',
            image_ref   TEXT NOT NULL DEFAULT '',-- 保存先 (R2/volume key 等。生base64は保持しない)
            estimate_jpy INTEGER NOT NULL DEFAULT 0,
            reason      TEXT NOT NULL DEFAULT '',
            status      TEXT NOT NULL DEFAULT 'new', -- new|stored|bought|listed|sold|cancelled
            created_at  INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS consign_intents (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            item_id     TEXT NOT NULL,
            intent      TEXT NOT NULL,           -- sell|consign|store
            partner_id  INTEGER,                 -- 売り系で受理時の担当古物商
            gated       INTEGER NOT NULL DEFAULT 0,
            amount_jpy  INTEGER NOT NULL DEFAULT 0, -- sell=付与額 / consign=到達目安 / store=0
            created_at  INTEGER NOT NULL
        );
        -- 残高は付与ロット単位。expires_at で 6ヶ月失効。spent_jpy で消し込み。
        -- 出金/換金カラムは意図的に持たない (資金移動業回避)。
        CREATE TABLE IF NOT EXISTS consign_balance_ledger (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            email       TEXT NOT NULL,
            item_id     TEXT,
            amount_jpy  INTEGER NOT NULL,        -- 付与額 (>0)
            spent_jpy   INTEGER NOT NULL DEFAULT 0,
            source      TEXT NOT NULL,           -- 'sell' | 'consign_sold'
            granted_at  INTEGER NOT NULL,
            expires_at  INTEGER NOT NULL         -- granted_at + 6ヶ月 (必須)
        );
        -- 査定の一時保存 (submit が quote_id を引くため。submit は再計算せずこれを信頼)
        CREATE TABLE IF NOT EXISTS consign_quotes (
            id          TEXT PRIMARY KEY,
            email       TEXT NOT NULL DEFAULT '',
            category    TEXT NOT NULL DEFAULT '',
            memo        TEXT NOT NULL DEFAULT '',
            image_ref   TEXT NOT NULL DEFAULT '',
            estimate_jpy INTEGER NOT NULL,
            reason      TEXT NOT NULL DEFAULT '',
            created_at  INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_consign_ledger_email ON consign_balance_ledger(email, expires_at);
        CREATE INDEX IF NOT EXISTS idx_consign_intents_item ON consign_intents(item_id);"
    );
}

fn now_s() -> i64 {
    crate::chrono_now().parse().unwrap_or(0)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
     .replace('"', "&quot;").replace('\'', "&#39;")
}

fn rand_id(prefix: &str) -> String {
    // 推測不能な受付ID。実装者は既存の id 生成ヘルパ(あれば)に差し替え可。
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{}{:x}", prefix, nanos)
}

/// 売り系が開放されているか = active かつ license_no を持つ提携先が1件以上。
/// 不変条件 #2 の唯一の門。sell/consign はここを通らない限り gated。
fn partner_gate_open(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM consign_partners WHERE active=1 AND TRIM(license_no) <> ''",
        [], |r| r.get::<_, i64>(0),
    ).unwrap_or(0) > 0
}

fn active_partner_id(conn: &Connection) -> Option<i64> {
    conn.query_row(
        "SELECT id FROM consign_partners WHERE active=1 AND TRIM(license_no) <> '' ORDER BY id LIMIT 1",
        [], |r| r.get::<_, i64>(0),
    ).ok()
}

fn yen(n: i64) -> String {
    let s = n.abs().to_string();
    let b = s.as_bytes();
    let mut out = String::new();
    for (i, c) in b.iter().enumerate() {
        if i > 0 && (b.len() - i) % 3 == 0 { out.push(','); }
        out.push(*c as char);
    }
    format!("¥{}", out)
}

// ── GET /consign ──────────────────────────────────────────────────────
pub(crate) async fn consign_page(State(db): State<Db>) -> Response {
    let open = { let c = db.lock().unwrap(); partner_gate_open(&c) };
    let gate_banner = if open {
        String::new()
    } else {
        r##"<div class="gatewall">🛠 <b>買取・委託は準備中です。</b>提携する古物商（古物営業法の許可業者）の登録が完了するまで、いまは「③ 預ける」のみご利用いただけます。査定はいつでもお試しいただけます。<div class="small">古物の買取・販売は許可業者が行います。準備が整い次第ここでお知らせします。</div></div>"##.to_string()
    };
    let html = CONSIGN_HTML
        .replace("__GATE_BANNER__", &gate_banner)
        .replace("__PARTNER_OPEN__", if open { "1" } else { "0" });
    Html(html).into_response()
}

// ── POST /api/consign/quote ───────────────────────────────────────────
#[derive(serde::Deserialize)]
pub(crate) struct QuoteBody {
    image: String,            // data:image/...;base64,xxxx
    #[serde(default)] category: String,
    #[serde(default)] memo: String,
    #[serde(default)] email: String,
}

pub(crate) async fn api_quote(State(db): State<Db>, Json(b): Json<QuoteBody>) -> Response {
    if b.image.len() < 64 {
        return Json(json!({"error":"画像が必要です"})).into_response();
    }
    // 1) 画像を保存 (実装者: 既存の store_mockup_bytes 等で R2/volume に。生base64はDBに入れない)。
    //    ここでは image_ref はプレースホルダ。
    let image_ref = format!("consign/{}", rand_id(""));

    // 2) AI査定。gemini.rs を使う。重ければ簡易ロジック fallback。理由(reason)は必ず返す。
    let (estimate, reason) = ai_quote(&b.category, &b.memo, &b.image).await;

    // 3) quote を一時保存 (submit が引く)。
    let qid = rand_id("q_");
    let now = now_s();
    {
        let c = db.lock().unwrap();
        let _ = c.execute(
            "INSERT INTO consign_quotes (id,email,category,memo,image_ref,estimate_jpy,reason,created_at)
             VALUES (?,?,?,?,?,?,?,?)",
            params![qid, b.email, b.category, b.memo, image_ref, estimate, reason, now],
        );
    }
    let low = (estimate as f64 * RANGE_LOW) as i64;
    let high = (estimate as f64 * RANGE_HIGH) as i64;
    let sell_now = (estimate as f64 * SELL_NOW_RATE) as i64;          // 即時買取 = 減額後
    let consign_target = (estimate as f64 * (1.0 - CONSIGN_FEE_RATE)) as i64; // 委託 = 手数料後の到達目安
    Json(json!({
        "quote_id": qid,
        "estimate_jpy": estimate,
        "low_jpy": low,
        "high_jpy": high,
        "sell_now_jpy": sell_now,
        "consign_target_jpy": consign_target,
        "reason": reason,
    })).into_response()
}

/// AI査定。gemini::call_gemini_with_image で画像から推定。失敗時は簡易ヒューリスティック。
/// 戻り: (estimate_jpy, reason)。reason は景表法・UX両面で必須。
async fn ai_quote(category: &str, memo: &str, _image_data_url: &str) -> (i64, String) {
    // 実装者向け:
    //   let prompt = format!("この画像の中古品を日本の二次流通相場で査定。カテゴリ={category} メモ={memo}。\
    //       JSONのみ: {{\"estimate_jpy\":<整数>,\"reason\":\"<状態と相場の根拠を1-2文>\"}}");
    //   gemini::call_gemini_text や画像対応関数に投げ、JSONをparse。
    //   ここでは外部依存なしで動くカテゴリ別の簡易ロジックを置く (重い/未設定でも落ちない)。
    let base = match category {
        "watch" => 18000,
        "bag" => 9000,
        "shoes" => 5000,
        "gadget" => 7000,
        "hobby" => 4000,
        "apparel" => 2500,
        _ => 3000,
    };
    let reason = format!(
        "カテゴリ「{}」の中古相場とご記入内容（{}）から算出した参考値です。状態確認後に最終額が決まります。",
        if category.is_empty() { "その他" } else { category },
        if memo.trim().is_empty() { "記載なし" } else { memo.trim() }
    );
    (base, reason)
}

// ── POST /api/consign/submit ──────────────────────────────────────────
#[derive(serde::Deserialize)]
pub(crate) struct SubmitBody {
    quote_id: String,
    intent: String,           // sell | consign | store
    #[serde(default)] email: String,
}

pub(crate) async fn api_submit(State(db): State<Db>, Json(b): Json<SubmitBody>) -> Response {
    let intent = b.intent.as_str();
    if !matches!(intent, "sell" | "consign" | "store") {
        return Json(json!({"error":"不正な選択です"})).into_response();
    }
    let c = db.lock().unwrap();

    // quote を引く (submit は再計算しない。改ざん防止のためサーバ保存値を信頼)。
    let q: Option<(String, String, String, i64, String)> = c.query_row(
        "SELECT category,memo,image_ref,estimate_jpy,reason FROM consign_quotes WHERE id=?",
        params![b.quote_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    ).ok();
    let (category, memo, image_ref, estimate, reason) = match q {
        Some(x) => x,
        None => return Json(json!({"error":"査定が見つかりません。もう一度査定してください"})).into_response(),
    };

    let now = now_s();
    let item_id = rand_id("i_");

    // 不変条件 #2: 売り系は partner gate を通る場合のみ受理。閉じていれば store にダウングレード。
    let gate_open = partner_gate_open(&c);
    let is_sell_side = matches!(intent, "sell" | "consign");

    if is_sell_side && !gate_open {
        // gated: お預かりとして受け付け、準備中を返す (買取/委託は受理しない)。
        let _ = c.execute(
            "INSERT INTO consign_items (id,email,category,memo,image_ref,estimate_jpy,reason,status,created_at)
             VALUES (?,?,?,?,?,?,?,'stored',?)",
            params![item_id, b.email, category, memo, image_ref, estimate, reason, now],
        );
        let _ = c.execute(
            "INSERT INTO consign_intents (item_id,intent,partner_id,gated,amount_jpy,created_at)
             VALUES (?,?,?,1,0,?)",
            params![item_id, intent, Option::<i64>::None, now],
        );
        return Json(json!({
            "gated": true,
            "item_id": item_id,
            "message": "提携古物商の登録が完了するまで、買取・委託はお受けできません。お預かりとして受け付けました。"
        })).into_response();
    }

    match intent {
        "sell" => {
            // 即時買取: 減額後の額をサイト内残高に即時付与 (6ヶ月失効)。
            let credited = (estimate as f64 * SELL_NOW_RATE) as i64;
            let pid = active_partner_id(&c);
            let _ = c.execute(
                "INSERT INTO consign_items (id,email,category,memo,image_ref,estimate_jpy,reason,status,created_at)
                 VALUES (?,?,?,?,?,?,?,'bought',?)",
                params![item_id, b.email, category, memo, image_ref, estimate, reason, now],
            );
            let _ = c.execute(
                "INSERT INTO consign_intents (item_id,intent,partner_id,gated,amount_jpy,created_at)
                 VALUES (?,?,?,0,?,?)",
                params![item_id, intent, pid, credited, now],
            );
            let expires = now + BALANCE_TTL_SECS;
            let _ = c.execute(
                "INSERT INTO consign_balance_ledger (email,item_id,amount_jpy,spent_jpy,source,granted_at,expires_at)
                 VALUES (?,?,?,0,'sell',?,?)",
                params![b.email, item_id, credited, now, expires],
            );
            Json(json!({
                "ok": true, "item_id": item_id, "credited_jpy": credited,
                "expires_at": fmt_date(expires)
            })).into_response()
        }
        "consign" => {
            // 委託: 受付のみ。残高は「売れたとき」に別途付与 (集団投資/資金プールにしない)。
            let pid = active_partner_id(&c);
            let target = (estimate as f64 * (1.0 - CONSIGN_FEE_RATE)) as i64;
            let _ = c.execute(
                "INSERT INTO consign_items (id,email,category,memo,image_ref,estimate_jpy,reason,status,created_at)
                 VALUES (?,?,?,?,?,?,?,'listed',?)",
                params![item_id, b.email, category, memo, image_ref, estimate, reason, now],
            );
            let _ = c.execute(
                "INSERT INTO consign_intents (item_id,intent,partner_id,gated,amount_jpy,created_at)
                 VALUES (?,?,?,0,?,?)",
                params![item_id, intent, pid, target, now],
            );
            Json(json!({"ok": true, "item_id": item_id, "consign_target_jpy": target})).into_response()
        }
        _ => {
            // 預ける: 保管のみ。残高付与なし。常時受理。
            let _ = c.execute(
                "INSERT INTO consign_items (id,email,category,memo,image_ref,estimate_jpy,reason,status,created_at)
                 VALUES (?,?,?,?,?,?,?,'stored',?)",
                params![item_id, b.email, category, memo, image_ref, estimate, reason, now],
            );
            let _ = c.execute(
                "INSERT INTO consign_intents (item_id,intent,partner_id,gated,amount_jpy,created_at)
                 VALUES (?,?,?,0,0,?)",
                params![item_id, intent, Option::<i64>::None, now],
            );
            Json(json!({"ok": true, "item_id": item_id})).into_response()
        }
    }
}

fn fmt_date(epoch: i64) -> String {
    // JST の YYYY-MM-DD。main.rs の civil_from_days (crate-private) を流用。
    let days = (epoch + 9 * 3600) / 86400;
    let (y, m, d) = crate::civil_from_days(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

// ── GET /consign/balance ──────────────────────────────────────────────
#[derive(serde::Deserialize)]
pub(crate) struct BalanceQuery {
    #[serde(default)] email: String,
}

pub(crate) async fn balance_page(State(db): State<Db>, Query(q): Query<BalanceQuery>) -> Response {
    let email = q.email.trim().to_string();
    if email.is_empty() {
        // 未照会: メール入力フォームのみ。
        let html = BALANCE_HTML
            .replace("__LOOKUP_DISPLAY__", "block")
            .replace("__BALANCE_BLOCK__", "");
        return Html(html).into_response();
    }
    let now = now_s();
    let c = db.lock().unwrap();
    let mut stmt = c.prepare(
        "SELECT amount_jpy, spent_jpy, source, granted_at, expires_at
         FROM consign_balance_ledger WHERE email=? ORDER BY expires_at ASC"
    ).unwrap();
    let rows = stmt.query_map(params![email], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?, r.get::<_, i64>(4)?))
    }).unwrap();

    let mut total_live = 0i64;
    let mut lots = String::new();
    for row in rows.flatten() {
        let (amount, spent, source, granted, expires) = row;
        let remain = amount - spent;
        if remain <= 0 { continue; }
        let expired = expires <= now;
        let soon = !expired && (expires - now) < 30 * 86400;
        if !expired { total_live += remain; }
        let cls = if expired { "lot expired" } else if soon { "lot soon" } else { "lot" };
        let src_label = match source.as_str() {
            "sell" => "買取",
            "consign_sold" => "委託の売上",
            _ => "出品代行",
        };
        lots.push_str(&format!(
            r##"<div class="{cls}"><div class="li">{src}<div class="src">付与 {g}</div></div><div><div class="ramt">{amt}</div><div class="exp">{e}</div></div></div>"##,
            cls = cls, src = esc(src_label), g = esc(&fmt_date(granted)),
            amt = yen(remain),
            e = if expired { "失効".to_string() } else { format!("{} まで", fmt_date(expires)) }
        ));
    }
    if lots.is_empty() {
        lots = r##"<div class="empty">有効な残高はありません。<a href="/consign" style="color:#ffd700">出品する →</a></div>"##.to_string();
    }
    let block = format!(
        r##"<div class="bigbox"><div class="lbl">利用できる残高</div><div class="amt">{total}</div><a class="use" href="/shop">MUで使う →</a></div><h2>内訳（有効期限つき）</h2>{lots}"##,
        total = yen(total_live), lots = lots
    );
    let html = BALANCE_HTML
        .replace("__LOOKUP_DISPLAY__", "none")
        .replace("__BALANCE_BLOCK__", &block);
    Html(html).into_response()
}

// ── GET /consign/sellers (取引DPF法: 販売業者情報開示・苦情窓口) ────────
pub(crate) async fn sellers_page(State(db): State<Db>) -> Response {
    let c = db.lock().unwrap();
    let mut stmt = c.prepare(
        "SELECT name, license_no, authority, address, contact
         FROM consign_partners WHERE active=1 AND TRIM(license_no) <> '' ORDER BY id"
    ).unwrap();
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?,
            r.get::<_, String>(3)?, r.get::<_, String>(4)?))
    }).unwrap();
    let mut block = String::new();
    let mut any = false;
    for row in rows.flatten() {
        any = true;
        let (name, lic, auth, addr, contact) = row;
        block.push_str(&format!(
            r##"<div class="partner"><div class="pn">{n}</div><div class="pd">古物商許可番号: {l}{a}{ad}{ct}</div></div>"##,
            n = esc(&name), l = esc(&lic),
            a = if auth.is_empty() { String::new() } else { format!("（{}）", esc(&auth)) },
            ad = if addr.is_empty() { String::new() } else { format!("<br>所在地: {}", esc(&addr)) },
            ct = if contact.is_empty() { String::new() } else { format!("<br>連絡: {}", esc(&contact)) },
        ));
    }
    if !any {
        block = r##"<div class="box"><b>準備中</b>：現在、提携する古物商（販売業者）の登録手続き中です。登録が完了するまで買取・委託の受付を停止しています（お預かりのみ可能）。</div>"##.to_string();
    }
    Html(SELLERS_HTML.replace("__PARTNERS_BLOCK__", &block)).into_response()
}

// ── GET /consign/tokushoho (特商法表記) ───────────────────────────────
pub(crate) async fn tokushoho_page() -> Response {
    let updated = crate::chrono_now_jst_date_str();
    Html(TOKUSHOHO_HTML.replace("__UPDATED_AT__", &updated)).into_response()
}

// ── admin: 提携古物商の登録 / intent 確認 (admin_auth 流用) ─────────────
pub(crate) async fn admin_partner_add(
    State(db): State<Db>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if let Err(r) = crate::admin_auth(&headers, &q, db.clone(), "/api/admin/consign/partner").await {
        return r;
    }
    let name = q.get("name").map(|s| s.trim()).unwrap_or("");
    let license = q.get("license_no").map(|s| s.trim()).unwrap_or("");
    if name.is_empty() || license.is_empty() {
        return (StatusCode::BAD_REQUEST, "name と license_no は必須 (license_no が無いと売り系は開放されません)").into_response();
    }
    let c = db.lock().unwrap();
    let _ = c.execute(
        "INSERT INTO consign_partners (name,license_no,authority,address,contact,active,created_at)
         VALUES (?,?,?,?,?,1,?)",
        params![
            name, license,
            q.get("authority").cloned().unwrap_or_default(),
            q.get("address").cloned().unwrap_or_default(),
            q.get("contact").cloned().unwrap_or_default(),
            now_s()
        ],
    );
    Json(json!({"ok": true, "gate_open": partner_gate_open(&c)})).into_response()
}

pub(crate) async fn admin_intents(
    State(db): State<Db>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if let Err(r) = crate::admin_auth(&headers, &q, db.clone(), "/api/admin/consign/intents").await {
        return r;
    }
    let c = db.lock().unwrap();
    let mut stmt = c.prepare(
        "SELECT ci.id, it.intent, it.gated, it.amount_jpy, ci.email, ci.category, ci.estimate_jpy, ci.status, ci.created_at
         FROM consign_items ci JOIN consign_intents it ON it.item_id = ci.id
         ORDER BY ci.created_at DESC LIMIT 200"
    ).unwrap();
    let rows = stmt.query_map([], |r| {
        Ok(json!({
            "item_id": r.get::<_, String>(0)?,
            "intent": r.get::<_, String>(1)?,
            "gated": r.get::<_, i64>(2)? == 1,
            "amount_jpy": r.get::<_, i64>(3)?,
            "email": r.get::<_, String>(4)?,
            "category": r.get::<_, String>(5)?,
            "estimate_jpy": r.get::<_, i64>(6)?,
            "status": r.get::<_, String>(7)?,
            "created_at": r.get::<_, i64>(8)?,
        }))
    }).unwrap();
    let items: Vec<Value> = rows.flatten().collect();
    Json(json!({"gate_open": partner_gate_open(&c), "count": items.len(), "items": items})).into_response()
}
