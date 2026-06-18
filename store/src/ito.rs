//! 糸 (ITO) — 服が財布になる、出会いで編まれるポイント。
//!
//! 設計原則 (2026-06-07 優貴さん指示「世界で問題なく・不正なく・儲けは後」):
//! - **無償付与のみ・購入不可・譲渡不可・円換金不可** — 前払式支払手段/暗号資産/
//!   証券のどの定義からも構造的に外す。台帳は非チェーン (SQLite append-only)。
//! - **読取時指数減価** — 30日で×0.9 (ASH と同じゲゼル設計・cron 不要)。
//!   指数減価は一様乗法なので「残高 = Σ delta×0.9^(経過日/30)」が
//!   spend (負イベント) 込みで常に非負・一貫する。
//! - **採掘 = 出会い** — 他人の服 (シリアル) をスキャンすると両者に +1糸。
//!   sybil 耐性は物理 (実物の服 + 別人 + ペア7日クールダウン + 日次上限3)。
//! - **購入でも編まれる** — +2糸/注文。ただし景表法の総付景品 20% キャップを
//!   mu_credit_grant_for_purchase と同じ式で併算 (1糸 = ¥490 参考価値)。
//! - **使う = 品目交換のみ** — 10糸 = 1着。金額建てにしない。交換は
//!   redemption キュー (人間承認) → 既存の Printful 発注フローで履行。
//!
//! テーブル: ito_serials (服=ウォレット) / ito_events (append-only 台帳) /
//! ito_redemptions (交換キュー)。全イベントが監査ログを兼ねる。

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::Db;

/// スキャン1回で両者に編まれる量 (milli糸 = 1/1000糸)。
const SCAN_MINT_MILLI: i64 = 1_000;
/// 購入1件で編まれる量 (milli糸)。景表法キャップと min() を取る。
const PURCHASE_MINT_MILLI: i64 = 2_000;
/// 1糸の参考価値 (円)。景表法 20% キャップ計算にのみ使用 — 表示・換金には使わない。
/// 10糸=1着 (¥4,900 基準) → 1糸 ≒ ¥490。
const ITO_VALUE_JPY: i64 = 490;
/// 1着との交換に必要な量 (milli糸)。
pub(crate) const REDEEM_TEE_MILLI: i64 = 10_000;
/// 減価: 30日ごとに ×0.9 (読取時計算)。
const DECAY_PER_30D: f64 = 0.9;
const DECAY_WINDOW_SECS: f64 = 30.0 * 86400.0;
/// 同一ペア (email × serial) の再採掘クールダウン。
const PAIR_COOLDOWN_SECS: i64 = 7 * 86400;
/// 1日にスキャンで編める上限 (スキャンする側・される側それぞれ・メール単位)。
const DAILY_SCAN_CAP: i64 = 3;
/// 1日に同一 IP から編める上限 (捨てメール量産での farm を抑える物理的な蓋)。
/// メール単位の上限は捨てメールでバイパスできるため、IP 単位の蓋を併用する。
const DAILY_IP_SCAN_CAP: i64 = 6;

pub(crate) fn ensure_tables(conn: &Connection) {
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS ito_serials (
            serial      TEXT PRIMARY KEY,
            sku         TEXT NOT NULL DEFAULT '',
            order_session TEXT,
            owner_email TEXT NOT NULL,
            note        TEXT,
            scan_count  INTEGER NOT NULL DEFAULT 0,
            issued_at   INTEGER NOT NULL,
            last_scan_at INTEGER
        );
        CREATE TABLE IF NOT EXISTS ito_events (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            email       TEXT NOT NULL,
            serial      TEXT,
            delta_milli INTEGER NOT NULL,
            reason      TEXT NOT NULL,
            ref_id      TEXT,
            ip          TEXT,
            created_at  INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_ito_events_email ON ito_events(email);
        CREATE INDEX IF NOT EXISTS idx_ito_events_ref ON ito_events(ref_id);
        CREATE INDEX IF NOT EXISTS idx_ito_events_ip ON ito_events(ip, created_at);
        CREATE TABLE IF NOT EXISTS ito_redemptions (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            email       TEXT NOT NULL,
            sku         TEXT NOT NULL,
            size        TEXT NOT NULL DEFAULT '',
            name        TEXT NOT NULL DEFAULT '',
            address_json TEXT NOT NULL DEFAULT '{}',
            cost_milli  INTEGER NOT NULL,
            status      TEXT NOT NULL DEFAULT 'pending',
            created_at  INTEGER NOT NULL,
            done_at     INTEGER
        );",
    );
}

fn now_s() -> i64 {
    crate::chrono_now().parse().unwrap_or(0)
}

/// 読取時減価つき残高 (milli糸)。残高 = Σ delta×0.9^(経過秒/30日)。
/// spend は負イベントとして同率で減価するため、spend 時に残高 ≥ 額を
/// 強制していれば以後も恒久的に非負 (指数減価の一様乗法性)。
pub(crate) fn balance_milli(conn: &Connection, email: &str, now: i64) -> i64 {
    let email_lc = email.trim().to_lowercase();
    let mut stmt = match conn.prepare(
        "SELECT delta_milli, created_at FROM ito_events WHERE email=?1",
    ) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let rows = stmt
        .query_map(params![email_lc], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
        .map(|it| it.flatten().collect::<Vec<_>>())
        .unwrap_or_default();
    let mut sum = 0.0_f64;
    for (delta, at) in rows {
        let age = (now - at).max(0) as f64;
        sum += (delta as f64) * DECAY_PER_30D.powf(age / DECAY_WINDOW_SECS);
    }
    sum.floor() as i64
}

#[allow(clippy::too_many_arguments)]
fn append_event(
    conn: &Connection,
    email: &str,
    serial: Option<&str>,
    delta_milli: i64,
    reason: &str,
    ref_id: Option<&str>,
    ip: Option<&str>,
    now: i64,
) {
    let _ = conn.execute(
        "INSERT INTO ito_events (email, serial, delta_milli, reason, ref_id, ip, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![email.trim().to_lowercase(), serial, delta_milli, reason, ref_id, ip, now],
    );
}

fn valid_email(e: &str) -> bool {
    let e = e.trim();
    e.len() >= 6 && e.len() <= 120 && e.contains('@') && e.contains('.') && !e.contains(char::is_whitespace)
}

/// 注文 session_id から決定的・推測不能なシリアルを生成 (ticket_code と同型)。
/// 同じ session → 同じ serial なので at-least-once webhook でも二重発行しない。
pub(crate) fn serial_for_session(session_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"ito:");
    h.update(session_id.as_bytes());
    let hex: String = h.finalize().iter().take(6).map(|b| format!("{:02x}", b)).collect();
    format!("it{}", hex)
}

/// 購入採掘 + シリアル発行。record_order_full (catalog.rs) から呼ばれる。
/// - +2糸/注文。ただし景表法 総付景品 20% キャップ:
///   grant = min(2糸, floor(amount_jpy×0.2 / ¥490) 糸) — ¥4,900 の服でちょうど 2糸。
/// - 物件 (digital 以外) には服シリアルを発行し owner_email に紐付け。
/// - ref_id = "order:<session>" で冪等。
pub(crate) fn grant_for_order(
    conn: &Connection,
    session_id: &str,
    sku: &str,
    amount_jpy: i64,
    email: &str,
    status: &str,
) {
    if email.is_empty() || !valid_email(email) || session_id.is_empty() { return; }
    if status == "failed" || amount_jpy <= 0 { return; }
    ensure_tables(conn);
    let now = now_s();
    let ref_id = format!("order:{}", session_id);
    let already: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ito_events WHERE ref_id=?1",
            params![ref_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if already == 0 {
        // 景表法 20% キャップ (milli糸): amount×0.2 円 ÷ ¥490/糸 × 1000
        let cap_milli = amount_jpy * 200 / ITO_VALUE_JPY; // = amount×0.2×1000/490
        let grant = PURCHASE_MINT_MILLI.min(cap_milli);
        if grant > 0 {
            append_event(conn, email, None, grant, "purchase", Some(&ref_id), None, now);
        }
    }
    // 服シリアル (digital チケットには発行しない)
    let route: String = conn
        .query_row(
            "SELECT COALESCE(fulfillment_route,'') FROM catalog_products WHERE sku=?1",
            params![sku],
            |r| r.get(0),
        )
        .unwrap_or_default();
    if route != "digital" {
        let serial = serial_for_session(session_id);
        let _ = conn.execute(
            "INSERT OR IGNORE INTO ito_serials (serial, sku, order_session, owner_email, issued_at)
             VALUES (?1,?2,?3,?4,?5)",
            params![serial, sku, session_id, email.trim().to_lowercase(), now],
        );
    }
}

// ───────────────────────── scan (採掘 = 出会い) ─────────────────────────

#[derive(serde::Deserialize)]
pub(crate) struct ScanBody {
    serial: String,
    email: String,
}

/// POST /api/ito/scan — 他人の服をスキャンして両者に +1糸。
/// 防御: 自己スキャン不可 / 同一ペア 7日 / 双方 日次上限 3 (メール) /
///       同一 IP 日次 6 (捨てメール対策) / 実在シリアルのみ。
/// ⚠ v1 の限界: シリアルは URL に現れるため「物理的に会った」ことは証明できない
///    (NFC 動的コードは将来)。本物の不正コストゲートは交換側 (人間承認 + 住所) に置く。
pub(crate) async fn api_scan(State(db): State<Db>, headers: HeaderMap, Json(body): Json<ScanBody>) -> Response {
    let email = body.email.trim().to_lowercase();
    if !valid_email(&email) {
        return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "error": "メールアドレスを確認してください"}))).into_response();
    }
    let serial = body.serial.trim().to_lowercase();
    let ip = crate::client_ip(&headers);
    let now = now_s();
    let conn = db.lock().unwrap();
    ensure_tables(&conn);
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT owner_email, sku FROM ito_serials WHERE serial=?1",
            params![serial],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    let Some((owner, _sku)) = row else {
        return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "error": "このシリアルは登録されていません"}))).into_response();
    };
    if owner == email {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "ok": false,
            "error": "自分の服では編めません — 誰かと会って、お互いの服をスキャンしよう"
        }))).into_response();
    }
    // ペア (このメール × この服) クールダウン 7日
    let pair = format!("pair:{}:{}", email, serial);
    let last_pair: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(created_at),0) FROM ito_events WHERE ref_id=?1",
            params![pair],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if now - last_pair < PAIR_COOLDOWN_SECS {
        return (StatusCode::TOO_MANY_REQUESTS, Json(json!({
            "ok": false,
            "error": "この服とはもう編んだばかり (同じ服とは7日に1回)。別の誰かと会おう"
        }))).into_response();
    }
    // 日次上限 (スキャンする側 / 服のオーナー側 それぞれ 3)
    let day_ago = now - 86400;
    let scans_today = |who: &str, reason: &str| -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM ito_events WHERE email=?1 AND reason=?2 AND created_at>?3",
            params![who, reason, day_ago],
            |r| r.get(0),
        ).unwrap_or(0)
    };
    // 同一 IP の日次上限 (捨てメール量産対策の物理的な蓋)。
    if !ip.is_empty() && ip != "unknown" {
        let ip_scans: i64 = conn.query_row(
            "SELECT COUNT(*) FROM ito_events WHERE ip=?1 AND reason='scan:met' AND created_at>?2",
            params![ip, day_ago], |r| r.get(0),
        ).unwrap_or(0);
        if ip_scans >= DAILY_IP_SCAN_CAP {
            return (StatusCode::TOO_MANY_REQUESTS, Json(json!({
                "ok": false, "error": "この端末からは本日の上限です。また明日"
            }))).into_response();
        }
    }
    let scanner_capped = scans_today(&email, "scan:met") >= DAILY_SCAN_CAP;
    let owner_capped = scans_today(&owner, "scan:worn") >= DAILY_SCAN_CAP;
    if scanner_capped && owner_capped {
        return (StatusCode::TOO_MANY_REQUESTS, Json(json!({
            "ok": false, "error": "今日はもう編み切りました (1日3回まで)。また明日"
        }))).into_response();
    }
    if !scanner_capped {
        append_event(&conn, &email, Some(&serial), SCAN_MINT_MILLI, "scan:met", Some(&pair), Some(&ip), now);
    }
    if !owner_capped {
        append_event(&conn, &owner, Some(&serial), SCAN_MINT_MILLI, "scan:worn", Some(&pair), Some(&ip), now);
    }
    let _ = conn.execute(
        "UPDATE ito_serials SET scan_count=scan_count+1, last_scan_at=?1 WHERE serial=?2",
        params![now, serial],
    );
    let bal = balance_milli(&conn, &email, now);
    Json(json!({
        "ok": true,
        "minted_milli": if scanner_capped { 0 } else { SCAN_MINT_MILLI },
        "balance_milli": bal,
        "balance": format_ito(bal),
        "note": if scanner_capped { "あなたは本日上限。服のオーナーには編まれました" } else { "+1糸 編まれました" },
    })).into_response()
}

// ───────────────────────── balance / redeem ─────────────────────────

pub(crate) fn format_ito(milli: i64) -> String {
    format!("{}.{:01}", milli / 1000, (milli % 1000).abs() / 100)
}

/// GET /api/ito/balance?email=
pub(crate) async fn api_balance(
    State(db): State<Db>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let email = q.get("email").cloned().unwrap_or_default().trim().to_lowercase();
    if !valid_email(&email) {
        return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "error": "email required"}))).into_response();
    }
    let now = now_s();
    let conn = db.lock().unwrap();
    ensure_tables(&conn);
    let bal = balance_milli(&conn, &email, now);
    // 自分の服 (シリアル) 一覧 — それぞれが /ito/:serial の QR ウォレット
    let mut st = conn
        .prepare("SELECT serial, sku, scan_count, issued_at FROM ito_serials WHERE owner_email=?1 ORDER BY issued_at DESC LIMIT 50")
        .unwrap();
    let serials: Vec<Value> = st
        .query_map(params![email], |r| {
            Ok(json!({
                "serial": r.get::<_, String>(0)?,
                "sku": r.get::<_, String>(1)?,
                "scan_count": r.get::<_, i64>(2)?,
                "url": format!("/ito/{}", r.get::<_, String>(0)?),
            }))
        })
        .map(|it| it.flatten().collect())
        .unwrap_or_default();
    let mut st2 = conn
        .prepare("SELECT delta_milli, reason, created_at FROM ito_events WHERE email=?1 ORDER BY id DESC LIMIT 20")
        .unwrap();
    let recent: Vec<Value> = st2
        .query_map(params![email], |r| {
            Ok(json!({
                "delta_milli": r.get::<_, i64>(0)?,
                "reason": r.get::<_, String>(1)?,
                "at": r.get::<_, i64>(2)?,
            }))
        })
        .map(|it| it.flatten().collect())
        .unwrap_or_default();
    Json(json!({
        "ok": true,
        "balance_milli": bal,
        "balance": format_ito(bal),
        "redeem_tee_milli": REDEEM_TEE_MILLI,
        "can_redeem_tee": bal >= REDEEM_TEE_MILLI,
        "garments": serials,
        "recent": recent,
        "decay": "30日で×0.9 にほつれます。貯めずに、会って、編んで、使う。",
    })).into_response()
}

#[derive(serde::Deserialize)]
pub(crate) struct RedeemBody {
    email: String,
    sku: String,
    #[serde(default)] size: String,
    #[serde(default)] name: String,
    #[serde(default)] address: Value,
}

/// POST /api/ito/redeem — 10糸 = 1着。負イベントで即時控除し、交換キューへ。
/// 履行は人間承認 → 既存 Printful 発注 (救済発注と同フロー)。¥0 注文なので
/// Stripe を経由しない。
pub(crate) async fn api_redeem(State(db): State<Db>, Json(body): Json<RedeemBody>) -> Response {
    // 蛇口を閉じておく: 交換は ITO_REDEEM_LIVE=1 のときだけ受け付ける。
    // 既定 off の理由 — earn 側 (スキャン/購入) の不正耐性が固まるまで、
    // 原価のかかる現物交換を開けない。残高表示・体験は off でも動く。
    if std::env::var("ITO_REDEEM_LIVE").map(|v| v != "1").unwrap_or(true) {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({
            "ok": false,
            "error": "糸の交換は近日公開です (now in pilot — redemption opens soon)。今は会って・買って糸を編めます。",
        }))).into_response();
    }
    let email = body.email.trim().to_lowercase();
    if !valid_email(&email) {
        return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "error": "メールアドレスを確認してください"}))).into_response();
    }
    let sku = body.sku.trim().to_string();
    if sku.is_empty() || sku.len() > 80 {
        return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "error": "sku を指定してください"}))).into_response();
    }
    let now = now_s();
    let conn = db.lock().unwrap();
    ensure_tables(&conn);
    // 実在 SKU のみ (hardcode しない — 実在 verify)
    let exists: i64 = conn
        .query_row("SELECT COUNT(*) FROM catalog_products WHERE sku=?1", params![sku], |r| r.get(0))
        .unwrap_or(0);
    if exists == 0 {
        return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "error": "その SKU は見つかりません"}))).into_response();
    }
    let bal = balance_milli(&conn, &email, now);
    if bal < REDEEM_TEE_MILLI {
        return (StatusCode::PAYMENT_REQUIRED, Json(json!({
            "ok": false,
            "error": format!("糸が足りません (残 {} / 必要 10)。会って編むか、買って編もう", format_ito(bal)),
            "balance_milli": bal,
        }))).into_response();
    }
    let addr = serde_json::to_string(&body.address).unwrap_or_else(|_| "{}".into());
    let _ = conn.execute(
        "INSERT INTO ito_redemptions (email, sku, size, name, address_json, cost_milli, status, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,'pending',?7)",
        params![email, sku, body.size.chars().take(20).collect::<String>(),
                body.name.chars().take(80).collect::<String>(), addr, REDEEM_TEE_MILLI, now],
    );
    let rid = conn.last_insert_rowid();
    append_event(&conn, &email, None, -REDEEM_TEE_MILLI, "redeem:tee", Some(&format!("redeem:{}", rid)), None, now);
    let bal2 = balance_milli(&conn, &email, now);
    Json(json!({
        "ok": true,
        "redemption_id": rid,
        "status": "pending",
        "balance_milli": bal2,
        "balance": format_ito(bal2),
        "note": "交換を受け付けました。確認のうえ発送します (発送時にメールでお知らせ)。",
    })).into_response()
}

// ───────────────────────── admin ─────────────────────────

#[derive(serde::Deserialize)]
pub(crate) struct AdminIssueBody {
    owner_email: String,
    #[serde(default)] sku: String,
    #[serde(default)] note: String,
}

/// POST /api/admin/ito/issue — genesis 配布用: 注文を介さず服シリアルを発行。
pub(crate) async fn api_admin_issue(
    State(db): State<Db>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<AdminIssueBody>,
) -> Response {
    if let Err(resp) = crate::admin_auth(&headers, &q, db.clone(), "/api/admin/ito/issue").await {
        return resp;
    }
    let email = body.owner_email.trim().to_lowercase();
    if !valid_email(&email) {
        return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "error": "owner_email invalid"}))).into_response();
    }
    let now = now_s();
    // ランダムシリアル (genesis): 時刻+メール からの決定的生成だと再発行で衝突するため
    // ここは乱数。推測困難 12 hex。
    let serial = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(b"ito-genesis:");
        h.update(email.as_bytes());
        h.update(now.to_le_bytes());
        h.update(std::process::id().to_le_bytes());
        let hex: String = h.finalize().iter().take(6).map(|b| format!("{:02x}", b)).collect();
        format!("it{}", hex)
    };
    let conn = db.lock().unwrap();
    ensure_tables(&conn);
    let _ = conn.execute(
        "INSERT OR IGNORE INTO ito_serials (serial, sku, owner_email, note, issued_at)
         VALUES (?1,?2,?3,?4,?5)",
        params![serial, body.sku.trim(), email, body.note.chars().take(200).collect::<String>(), now],
    );
    Json(json!({
        "ok": true,
        "serial": serial,
        "url": format!("https://wearmu.com/ito/{}", serial),
        "qr_png": format!("https://wearmu.com/ito/{}/qr.png", serial),
    })).into_response()
}

/// GET /api/admin/ito/redemptions — 交換キュー一覧 (履行は救済発注フローで)。
pub(crate) async fn api_admin_redemptions(
    State(db): State<Db>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if let Err(resp) = crate::admin_auth(&headers, &q, db.clone(), "/api/admin/ito/redemptions").await {
        return resp;
    }
    let conn = db.lock().unwrap();
    ensure_tables(&conn);
    let mut st = conn
        .prepare("SELECT id, email, sku, size, name, address_json, cost_milli, status, created_at FROM ito_redemptions ORDER BY id DESC LIMIT 200")
        .unwrap();
    let rows: Vec<Value> = st
        .query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?, "email": r.get::<_, String>(1)?,
                "sku": r.get::<_, String>(2)?, "size": r.get::<_, String>(3)?,
                "name": r.get::<_, String>(4)?, "address": r.get::<_, String>(5)?,
                "cost_milli": r.get::<_, i64>(6)?, "status": r.get::<_, String>(7)?,
                "created_at": r.get::<_, i64>(8)?,
            }))
        })
        .map(|it| it.flatten().collect())
        .unwrap_or_default();
    Json(json!({"ok": true, "redemptions": rows})).into_response()
}

#[derive(serde::Deserialize)]
pub(crate) struct RedeemDoneBody { id: i64 }

/// POST /api/admin/ito/redeem-done — 発送済みマーク。
pub(crate) async fn api_admin_redeem_done(
    State(db): State<Db>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<RedeemDoneBody>,
) -> Response {
    if let Err(resp) = crate::admin_auth(&headers, &q, db.clone(), "/api/admin/ito/redeem-done").await {
        return resp;
    }
    let conn = db.lock().unwrap();
    let n = conn
        .execute(
            "UPDATE ito_redemptions SET status='done', done_at=?1 WHERE id=?2 AND status='pending'",
            params![now_s(), body.id],
        )
        .unwrap_or(0);
    Json(json!({"ok": n > 0, "id": body.id})).into_response()
}

// ───────────────────────── pages ─────────────────────────

/// GET /ito/:serial/qr.png — 服ウォレットの QR (スキャン先 = /ito/:serial)。
pub(crate) async fn serial_qr(State(db): State<Db>, Path(serial): Path<String>) -> Response {
    let serial = serial.trim().to_lowercase();
    let exists: i64 = {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        conn.query_row("SELECT COUNT(*) FROM ito_serials WHERE serial=?1", params![serial], |r| r.get(0))
            .unwrap_or(0)
    };
    if exists == 0 {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let url = format!("https://wearmu.com/ito/{}", serial);
    match crate::catalog::ticket_qr_png(&url) {
        Some(png) => ([("content-type", "image/png"), ("cache-control", "public, max-age=86400")], png).into_response(),
        None => (StatusCode::INTERNAL_SERVER_ERROR, "qr failed").into_response(),
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// GET /ito/:serial — 服のページ。出会った人がここでメールを入れてスキャン完了。
pub(crate) async fn serial_page(State(db): State<Db>, Path(serial): Path<String>) -> Response {
    let serial = serial.trim().to_lowercase();
    let row: Option<(String, String, i64, i64)> = {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        conn.query_row(
            "SELECT sku, owner_email, scan_count, issued_at FROM ito_serials WHERE serial=?1",
            params![serial],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok()
    };
    let Some((sku, owner, scans, _issued)) = row else {
        return (StatusCode::NOT_FOUND, Html("<h1>not found</h1>".to_string())).into_response();
    };
    // オーナーはマスク表示 (PII を公開面に出さない)
    let owner_masked = {
        let head: String = owner.chars().take(2).collect();
        format!("{}***", head)
    };
    let sku_link = if sku.is_empty() { String::new() } else {
        format!("<a href=\"/shop/{0}\" style=\"color:#5cf\">{0}</a> · ", esc(&sku))
    };
    let html = format!(r##"<!doctype html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>糸 {serial} — wearmu</title>
<meta name="robots" content="noindex">
<style>
*{{box-sizing:border-box;margin:0;padding:0}}
body{{background:#0a0a0a;color:#e8ecf2;font-family:-apple-system,'Hiragino Sans',sans-serif;line-height:1.7;min-height:100dvh;display:flex;flex-direction:column;align-items:center;padding:40px 20px}}
.card{{max-width:420px;width:100%;background:#0d1117;border:1px solid #233;border-radius:16px;padding:28px}}
h1{{font-size:22px;font-weight:600;margin-bottom:6px}}
.sub{{color:#8aa;font-size:13px;margin-bottom:18px}}
input{{width:100%;background:#141a22;border:1px solid #2a3340;border-radius:8px;color:#eee;padding:12px;font-size:15px;margin-bottom:10px}}
button{{width:100%;background:#5cf;color:#04121a;border:0;border-radius:8px;padding:13px;font-size:15px;font-weight:800;cursor:pointer}}
button:disabled{{opacity:.5}}
.meta{{font-size:12px;color:#6a8;margin-top:14px}}
#out{{margin-top:14px;font-size:14px;white-space:pre-wrap}}
.ok{{color:#7fdba0}}.err{{color:#ff8a7a}}
.en{{display:block;color:#6a8;font-size:11px;margin-top:8px;line-height:1.5}}
a.shop{{display:block;text-align:center;margin-top:16px;color:#5cf;text-decoration:none;font-size:13px}}
</style></head><body>
<div class="card">
<h1>🧵 この服と、糸を編む</h1>
<div class="sub">これは {owner_masked} さんの一着 ({sku_link}これまで {scans} 回 編まれた)。<br>
出会いの証として、あなたと持ち主の両方に <b>+1糸</b> が編まれます。10糸 = 1着と交換。<br>
糸は買えない・送れない・換金できない。30日で×0.9 にほつれる — 会って編むだけ。<br>
<span class="en">EN — Meet the owner of this MU garment: both of you mint <b>+1 ito</b>. 10 ito = 1 tee. Ito can’t be bought, sent, or cashed out, and decays ×0.9 every 30 days.</span></div>
<input id="em" type="email" placeholder="あなたのメール / your email" autocomplete="email">
<button id="go">糸を編む / Mint (+1)</button>
<div id="out"></div>
<a class="shop" href="/shop">自分の一着を持つ → /shop (購入でも +2糸)</a>
<div class="meta">serial: {serial} · 台帳は append-only 監査ログ · <a href="/ito" style="color:#6a8">糸とは?</a></div>
</div>
<script>
var em=document.getElementById('em');try{{em.value=localStorage.getItem('ito_email')||'';}}catch(e){{}}
document.getElementById('go').onclick=function(){{
  var b=document.getElementById('go'),o=document.getElementById('out');
  var v=em.value.trim(); if(!v){{em.focus();return;}}
  try{{localStorage.setItem('ito_email',v);}}catch(e){{}}
  b.disabled=true;o.textContent='編んでいます…';o.className='';
  fetch('/api/ito/scan',{{method:'POST',headers:{{'content-type':'application/json'}},body:JSON.stringify({{serial:'{serial}',email:v}})}})
  .then(function(r){{return r.json();}})
  .then(function(d){{
    if(d.ok){{o.className='ok';o.textContent='🧵 '+(d.note||'+1糸')+'\n残高: '+d.balance+' 糸';}}
    else{{o.className='err';o.textContent=d.error||'エラー';}}
  }})
  .catch(function(){{o.className='err';o.textContent='通信エラー';}})
  .finally(function(){{b.disabled=false;}});
}};
</script></body></html>"##,
        serial = esc(&serial), owner_masked = esc(&owner_masked), sku_link = sku_link, scans = scans);
    Html(html).into_response()
}

/// GET /ito — 糸の説明 + 残高/自分の服の確認。
pub(crate) async fn ito_page() -> Response {
    let html = r##"<!doctype html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>糸 (ITO) — 服が財布になる | wearmu</title>
<meta name="description" content="MUの糸は買えない・送れない・換金できないポイント。人と会って服をスキャンすると編まれ、30日でほつれる。10糸で1着と交換。">
<style>
*{box-sizing:border-box;margin:0;padding:0}
body{background:#0a0a0a;color:#e8ecf2;font-family:-apple-system,'Hiragino Sans',sans-serif;line-height:1.8}
.wrap{max-width:640px;margin:0 auto;padding:48px 22px 100px}
h1{font-size:30px;font-weight:700;margin-bottom:8px}
h2{font-size:16px;color:#9ad;margin:32px 0 8px;border-bottom:1px solid #1a3340;padding-bottom:6px}
.sub{color:#8aa;font-size:14px;margin-bottom:24px}
.big{font-size:18px;color:#fff;margin:18px 0}
ul{padding-left:20px;color:#bcd;font-size:14px}
li{margin:6px 0}
input{width:100%;background:#141a22;border:1px solid #2a3340;border-radius:8px;color:#eee;padding:12px;font-size:15px;margin-bottom:10px}
button{width:100%;background:#5cf;color:#04121a;border:0;border-radius:8px;padding:13px;font-size:15px;font-weight:800;cursor:pointer}
#bal{margin-top:16px;font-size:14px;white-space:pre-wrap}
.g{background:#0d1117;border:1px solid #233;border-radius:10px;padding:12px 14px;margin:8px 0;font-size:13px}
.g a{color:#5cf;text-decoration:none}
a{color:#5cf}
.en{color:#6a8;font-size:12px;margin-top:4px}
</style></head><body><div class="wrap">
<h1>🧵 糸 (ITO)</h1>
<div class="sub">服が財布になる。出会いで編まれる。<span class="en"> / Your garment is the wallet. Minted only by meeting people.</span></div>

<div class="big">MUのトークンは口座に入らない。体に着る。</div>

<h2>編み方 (earn)</h2>
<ul>
<li>🤝 <b>会う</b> — 誰かの MU の服 (QRページ) をスキャン → <b>両方に +1糸</b>。同じ服とは7日に1回・1日3回まで</li>
<li>🛍 <b>買う</b> — <a href="/shop">/shop</a> で1着買うと <b>+2糸</b>、服にはシリアル (QRウォレット) が付く</li>
</ul>

<h2>ほつれ (decay)</h2>
<ul><li>糸は30日で ×0.9 にほつれます。貯め込む価値はない — 会って、編んで、使う</li></ul>

<h2>使い方 (spend)</h2>
<ul><li>👕 <b>10糸 = 1着</b> と交換 (送料込み・現金不要)。それ以外の使い道はありません</li></ul>

<h2>ルール (なぜ世界中で使えるか)</h2>
<ul>
<li>糸は<b>買えない・人に送れない・換金できない</b> — 金融商品ではなくただの感謝の記録</li>
<li>台帳は append-only。集計は透明性ページで公開予定</li>
</ul>

<h2>残高をみる</h2>
<input id="em" type="email" placeholder="メールアドレス" autocomplete="email">
<button id="go">残高と自分の服</button>
<div id="bal"></div>
<div id="gs"></div>
<script>
var em=document.getElementById('em');try{em.value=localStorage.getItem('ito_email')||'';}catch(e){}
document.getElementById('go').onclick=function(){
  var v=em.value.trim(); if(!v){em.focus();return;}
  try{localStorage.setItem('ito_email',v);}catch(e){}
  fetch('/api/ito/balance?email='+encodeURIComponent(v)).then(function(r){return r.json();}).then(function(d){
    var b=document.getElementById('bal'),g=document.getElementById('gs');g.innerHTML='';
    if(!d.ok){b.textContent=d.error||'エラー';return;}
    b.innerHTML='🧵 残高: <b>'+d.balance+'</b> 糸'+(d.can_redeem_tee?' — <a href="#" onclick="alert(\'交換は服のページ、または /shop の各商品から (近日UI)。今すぐは api/ito/redeem へ\');return false">1着と交換できます</a>':' (10糸で1着)');
    (d.garments||[]).forEach(function(s){
      g.insertAdjacentHTML('beforeend','<div class="g">👕 '+(s.sku||'(genesis)')+' — 編まれた回数 '+s.scan_count+' · <a href="'+s.url+'">服のページ</a> · <a href="'+s.url+'/qr.png">QR</a></div>');
    });
  }).catch(function(){document.getElementById('bal').textContent='通信エラー';});
};
</script>
</div></body></html>"##;
    Html(html.to_string()).into_response()
}

// ───────────────────────── tests ─────────────────────────

#[cfg(test)]
mod ito_tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_tables(&conn);
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS catalog_products (sku TEXT PRIMARY KEY, fulfillment_route TEXT);
             INSERT INTO catalog_products VALUES ('TEE-1','printful_dtg'),('TICKET-1','digital');",
        );
        conn
    }

    #[test]
    fn decay_30d_is_090() {
        let conn = test_conn();
        let t0 = 1_700_000_000_i64;
        append_event(&conn, "a@example.com", None, 1000, "scan:met", None, None, t0);
        // 直後は満額
        assert_eq!(balance_milli(&conn, "a@example.com", t0), 1000);
        // 30日後は ×0.9
        let b = balance_milli(&conn, "a@example.com", t0 + 30 * 86400);
        assert_eq!(b, 900);
        // 60日後は ×0.81
        let b2 = balance_milli(&conn, "a@example.com", t0 + 60 * 86400);
        assert_eq!(b2, 810);
    }

    #[test]
    fn spend_keeps_balance_nonnegative_forever() {
        let conn = test_conn();
        let t0 = 1_700_000_000_i64;
        append_event(&conn, "a@example.com", None, 10_000, "scan:met", None, None, t0);
        // 10日後に全額 (減価後) を spend
        let t1 = t0 + 10 * 86400;
        let bal = balance_milli(&conn, "a@example.com", t1);
        append_event(&conn, "a@example.com", None, -bal, "redeem:tee", None, None, t1);
        assert_eq!(balance_milli(&conn, "a@example.com", t1), 0);
        // 指数減価の一様乗法性: その後いつ見ても非負 (丸めで ±1milli 許容)
        for d in [1_i64, 30, 90, 365] {
            let b = balance_milli(&conn, "a@example.com", t1 + d * 86400);
            assert!(b >= -1, "balance went negative: {} at +{}d", b, d);
        }
    }

    #[test]
    fn purchase_grant_capped_at_20pct_and_idempotent() {
        let conn = test_conn();
        // ¥4,900 → 20% = ¥980 = 2000 milli (ちょうど満額)
        grant_for_order(&conn, "sess_1", "TEE-1", 4900, "b@example.com", "pending");
        let b = balance_milli(&conn, "b@example.com", now_s());
        assert_eq!(b, 2000);
        // 同じ session 再実行 → 増えない (冪等)
        grant_for_order(&conn, "sess_1", "TEE-1", 4900, "b@example.com", "pending");
        assert_eq!(balance_milli(&conn, "b@example.com", now_s()), b);
        // ¥1,000 の安い商品 → cap = ¥200 = 408 milli < 2000
        grant_for_order(&conn, "sess_2", "TEE-1", 1000, "c@example.com", "pending");
        let c = balance_milli(&conn, "c@example.com", now_s());
        assert_eq!(c, 1000 * 200 / ITO_VALUE_JPY);
        assert!(c < PURCHASE_MINT_MILLI);
    }

    #[test]
    fn serial_issued_for_physical_not_digital() {
        let conn = test_conn();
        grant_for_order(&conn, "sess_p", "TEE-1", 4900, "d@example.com", "pending");
        grant_for_order(&conn, "sess_d", "TICKET-1", 3000, "d@example.com", "pending");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM ito_serials WHERE owner_email='d@example.com'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "physical のみシリアル発行");
        let serial: String = conn
            .query_row("SELECT serial FROM ito_serials WHERE owner_email='d@example.com'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(serial, serial_for_session("sess_p"));
    }

    #[test]
    fn failed_or_free_orders_grant_nothing() {
        let conn = test_conn();
        grant_for_order(&conn, "sess_f", "TEE-1", 4900, "e@example.com", "failed");
        grant_for_order(&conn, "sess_0", "TEE-1", 0, "e@example.com", "pending");
        assert_eq!(balance_milli(&conn, "e@example.com", now_s()), 0);
    }
}
