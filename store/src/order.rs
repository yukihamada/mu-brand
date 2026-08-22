//! 製造オーケストレーション層 Phase3: 受注状態機械 + ロットロック。
//!
//! RFQ received（見積受領）から受注を起票し、
//! `ordered → production → shipped → completed` の状態機械で追跡する。
//!
//! ## ロットロック（Heritage 15着未満自動返金）
//! MOQ 未満の注文は `lot_lock=1` でロット（lot_id）に積み増しされ、
//! ロット合計が MOQ に達するまで発注保留。`lock_deadline`（既定30日）を
//! 超えても埋まらなければ `refund_pending` に遷移（refund_flag=1）。
//! **返金実行自体は人間ゲート**（フラグ + 一覧API のみ。Stripe 等は触らない）。
//!
//! ## 流儀（CATALOG_CONTRACT / rfq.rs 準拠）
//! - 新テーブルは manufacturing_schema.rs の `manufacturing_orders`。本モジュールは CREATE TABLE を書かない。
//! - 不正遷移は `order_advance` で弾く（後退・スキップ禁止）。
//! - `updated_at` は UPDATE 側で `datetime('now')` を手書きする。
//! - 対外送信・実返金はしない。人間が見て動く。

use rusqlite::{params, Connection};
use serde_json::{json, Value};

use crate::catalog::SUPPLIER_REGISTRY;

/// 状態機械の許可状態（CHECK 制約と一致）。
const ALLOWED_STATUS: &[&str] = &[
    "ordered",
    "production",
    "shipped",
    "completed",
    "refund_pending",
    "refunded",
];

/// 通常系の直線遷移（ordered→production→shipped→completed）。
const LINEAR_FLOW: &[&str] = &["ordered", "production", "shipped", "completed"];

/// ロットロックの期限（日）。期限内にロットが埋まらなければ自動返金フラグ。
pub const LOT_LOCK_DAYS: i64 = 30;

fn supplier_by_id(supplier_id: &str) -> Option<&'static crate::catalog::SupplierCapability> {
    SUPPLIER_REGISTRY.iter().find(|s| s.id == supplier_id)
}

fn supplier_name_of(supplier_id: &str) -> String {
    supplier_by_id(supplier_id)
        .map(|s| s.name.to_string())
        .unwrap_or_else(|| supplier_id.to_string())
}

/// ロット id（supplier × kind で1ロット。ロットは「同じ条件の注文を束ねて MOQ を埋める」単位）。
fn lot_key(supplier_id: &str, kind: &str) -> String {
    format!("{}:{}", supplier_id, kind)
}

/// ロット内の有効注文（refund_pending/refunded 以外）の合計数量。
fn lot_total_qty(conn: &Connection, lot_id: &str) -> i64 {
    conn.query_row(
        "SELECT COALESCE(SUM(qty),0) FROM manufacturing_orders \
         WHERE lot_id=?1 AND status NOT IN ('refund_pending','refunded')",
        params![lot_id],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

/// ロットが埋まったか（MOQ 到達）。supplier の MOQ 未設定/1以下なら常に埋まっている扱い。
fn lot_filled(conn: &Connection, supplier_id: &str, lot_id: &str) -> bool {
    let moq = supplier_by_id(supplier_id).map(|s| s.moq).unwrap_or(1);
    if moq <= 1 {
        return true;
    }
    lot_total_qty(conn, lot_id) >= moq
}

/// 期限切れのロック注文を `refund_pending` にスイープする（冪等・読み込み時に呼ぶ）。
/// 返金は実行しない。refund_flag=1 + status='refund_pending' を立てるだけ。
pub fn sweep_expired_locks(conn: &Connection) -> usize {
    let mut stmt = match conn.prepare(
        "SELECT id FROM manufacturing_orders \
         WHERE lot_lock=1 AND status='ordered' AND lock_deadline IS NOT NULL \
           AND lock_deadline < datetime('now')",
    ) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let ids: Vec<i64> = stmt
        .query_map([], |r| r.get(0))
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default();
    let n = ids.len();
    for id in ids {
        let _ = conn.execute(
            "UPDATE manufacturing_orders \
             SET status='refund_pending', refund_flag=1, \
                 refund_reason='ロットが期限内に埋まりませんでした（MOQ未達・自動返金対象）', \
                 updated_at=datetime('now') \
             WHERE id=?1",
            params![id],
        );
    }
    n
}

/// 受注を起票する。RFQ received からの発注（rfq_id 指定）か、直接起票。
///
/// - `rfq_id` 指定時: その RFQ が `received` であること（見積確定が発注の前提）。
///   supplier/kind/qty/単価は RFQ から引き継ぐ（明示指定があればそちら優先）。
/// - supplier の MOQ 未満なら `lot_lock=1`（ロットが埋まるまで発注保留）。
///   ロット合計が MOQ に達していればロックしない。
pub fn order_create(
    conn: &Connection,
    rfq_id: Option<i64>,
    supplier_id: Option<&str>,
    kind: Option<&str>,
    qty: i64,
    unit_jpy: Option<i64>,
    note: Option<&str>,
    owner_email: Option<&str>,
) -> Result<Value, String> {
    sweep_expired_locks(conn);

    // RFQ からの引き継ぎ。
    let (mut r_supplier, mut r_kind, mut r_qty, mut r_unit) =
        (None::<String>, None::<String>, None::<i64>, None::<i64>);
    if let Some(rid) = rfq_id {
        let row = conn
            .query_row(
                "SELECT supplier_id, kind, qty, quoted_unit_jpy, status FROM quote_requests WHERE id=?1",
                params![rid],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )
            .map_err(|_| format!("rfq id={} が見つかりません", rid))?;
        if row.4 != "received" {
            return Err(format!(
                "rfq id={} は status='{}' です。発注は見積受領(received)後に行ってください",
                rid, row.4
            ));
        }
        r_supplier = Some(row.0);
        r_kind = Some(row.1);
        r_qty = Some(row.2);
        r_unit = row.3;
    }

    let supplier = supplier_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or(r_supplier)
        .ok_or_else(|| "supplier_id を指定してください（または received 済み rfq_id）".to_string())?;
    let kind = kind
        .map(|k| k.trim().to_lowercase())
        .filter(|k| !k.is_empty())
        .or(r_kind)
        .ok_or_else(|| "kind を指定してください".to_string())?;
    let qty = if qty > 0 { qty } else { r_qty.unwrap_or(1) }.max(1);
    let unit = unit_jpy.or(r_unit);

    // ロットロック判定: MOQ>1 の supplier で、ロット合計（本注文込み）が MOQ 未満ならロック。
    let moq = supplier_by_id(&supplier).map(|s| s.moq).unwrap_or(1);
    let lot = lot_key(&supplier, &kind);
    let needs_lock = moq > 1 && (lot_total_qty(conn, &lot) + qty) < moq;
    // ロット (supplier×kind) は「埋まり具合の集計」を単位とする。locked/unlocked
    // いずれの注文も同じ lot_id を共有し、lot_total_qty が合計を集計する。
    // unlocked は deadline だけ NULL にして期限スイープを避ける。
    let (lot_lock, lot_id, lock_deadline): (i64, Option<String>, Option<String>) = if needs_lock {
        (
            1,
            Some(lot.clone()),
            Some(format!("datetime('now', '+{} days')", LOT_LOCK_DAYS)),
        )
    } else {
        (0, Some(lot), None)
    };

    let sql = format!(
        "INSERT INTO manufacturing_orders \
            (rfq_id, supplier_id, kind, qty, unit_jpy, status, lot_lock, lot_id, lock_deadline, note, owner_email) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'ordered', ?6, ?7, {}, ?8, ?9)",
        lock_deadline.clone().unwrap_or_else(|| "NULL".to_string())
    );
    conn.execute(
        &sql,
        params![rfq_id, supplier, kind, qty, unit, lot_lock, lot_id, note, owner_email],
    )
    .map_err(|e| format!("insert manufacturing_orders: {}", e))?;

    let id = conn.last_insert_rowid();
    let order = fetch_order_json(conn, id).unwrap_or(Value::Null);

    let next = if lot_lock == 1 {
        format!(
            "数量が MOQ({}着)未満のためロットロック中です。ロットが埋まるまで発注保留。{}日以内に埋まらなければ自動返金対象(refund_pending)になります。",
            moq, LOT_LOCK_DAYS
        )
    } else {
        "発注を記録しました。生産開始は order_advance で status='production' に。対外発注(PO送信)は人間ゲートです。".to_string()
    };

    Ok(json!({ "ok": true, "order": order, "next": next }))
}

/// 1行 manufacturing_orders を JSON 化。
#[allow(clippy::too_many_arguments)]
fn row_to_json(
    id: i64,
    rfq_id: Option<i64>,
    supplier_id: &str,
    kind: &str,
    qty: i64,
    unit_jpy: Option<i64>,
    status: &str,
    lot_lock: i64,
    lot_id: Option<String>,
    lock_deadline: Option<String>,
    refund_flag: i64,
    refund_reason: Option<String>,
    note: Option<String>,
    owner_email: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
) -> Value {
    json!({
        "id": id,
        "rfq_id": rfq_id,
        "supplier_id": supplier_id,
        "supplier_name": supplier_name_of(supplier_id),
        "kind": kind,
        "qty": qty,
        "unit_jpy": unit_jpy,
        "status": status,
        "lot_lock": lot_lock == 1,
        "lot_id": lot_id,
        "lock_deadline": lock_deadline,
        "refund_flag": refund_flag == 1,
        "refund_reason": refund_reason,
        "note": note,
        "owner_email": owner_email,
        "created_at": created_at,
        "updated_at": updated_at,
    })
}

const ORDER_COLS: &str = "id, rfq_id, supplier_id, kind, qty, unit_jpy, status, lot_lock, lot_id, \
     lock_deadline, refund_flag, refund_reason, note, owner_email, created_at, updated_at";

fn read_order_row(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(row_to_json(
        r.get(0)?,
        r.get(1)?,
        &r.get::<_, String>(2)?,
        &r.get::<_, String>(3)?,
        r.get(4)?,
        r.get(5)?,
        &r.get::<_, String>(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
        r.get(10)?,
        r.get(11)?,
        r.get(12)?,
        r.get(13)?,
        r.get(14)?,
        r.get(15)?,
    ))
}

/// id 指定で1行を JSON 取得（無ければ None）。
fn fetch_order_json(conn: &Connection, id: i64) -> Option<Value> {
    conn.query_row(
        &format!("SELECT {} FROM manufacturing_orders WHERE id=?1", ORDER_COLS),
        params![id],
        read_order_row,
    )
    .ok()
}

/// 受注の所有者メール（per-agent 認可チェック用）。
pub fn order_owner_email(conn: &Connection, id: i64) -> Option<String> {
    conn.query_row(
        "SELECT owner_email FROM manufacturing_orders WHERE id=?1",
        params![id],
        |r| r.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
}

/// 状態遷移の検証。許可されるのは:
/// - 直線: ordered→production→shipped→completed（1歩ずつ。スキップ・後退禁止）
/// - ordered → refund_pending（ロット期限切れ or 人間判断）
/// - refund_pending → refunded（返金完了の記録。人間が実行後に記録）
fn transition_allowed(from: &str, to: &str) -> bool {
    if from == to {
        return true; // 冪等（同状態への再指定は no-op として許す）
    }
    if from == "ordered" && to == "refund_pending" {
        return true;
    }
    if from == "refund_pending" && to == "refunded" {
        return true;
    }
    let fi = LINEAR_FLOW.iter().position(|s| *s == from);
    let ti = LINEAR_FLOW.iter().position(|s| *s == to);
    matches!((fi, ti), (Some(f), Some(t)) if t == f + 1)
}

/// 受注の状態を1歩進める（または refund 系を記録する）。
///
/// - 不正遷移（後退・スキップ・完了後の変更）はエラー。
/// - ロットロック中（lot_lock=1 かつロット未達）の ordered→production は弾く。
/// - `to='refunded'` は `refund_pending` からのみ（返金実行は人間。ここは記録のみ）。
pub fn order_advance(
    conn: &Connection,
    id: i64,
    to: &str,
    note: Option<&str>,
) -> Result<Value, String> {
    sweep_expired_locks(conn);

    if !ALLOWED_STATUS.contains(&to) {
        return Err(format!("status は {:?} のいずれか", ALLOWED_STATUS));
    }

    let (from, lot_lock, lot_id, supplier): (String, i64, Option<String>, String) = conn
        .query_row(
            "SELECT status, lot_lock, lot_id, supplier_id FROM manufacturing_orders WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|_| format!("order id={} が見つかりません", id))?;

    if !transition_allowed(&from, to) {
        return Err(format!(
            "不正な状態遷移です: {} → {}（許可: ordered→production→shipped→completed、ordered→refund_pending、refund_pending→refunded）",
            from, to
        ));
    }

    // ロットロック中の生産開始は、ロットが埋まっていればロックを解放して進める。
    if from == "ordered" && to == "production" && lot_lock == 1 {
        let filled = lot_id
            .as_deref()
            .map(|l| {
                let total = lot_total_qty(conn, l);
                #[cfg(test)]
                eprintln!("DEBUG total={} moq={} filled={}", total, supplier_by_id(&supplier).map(|s| s.moq).unwrap_or(1), total >= supplier_by_id(&supplier).map(|s| s.moq).unwrap_or(1));
                total >= supplier_by_id(&supplier).map(|s| s.moq).unwrap_or(1)
            })
            .unwrap_or(true);
        #[cfg(test)]
        eprintln!(
            "DEBUG advance id={} supplier={} lot_lock={} lot_id={:?} filled={}",
            id, supplier, lot_lock, lot_id, filled
        );
        if !filled {
            return Err(
                "ロットロック中です（MOQ未達）。ロットが埋まるまで生産開始できません".to_string(),
            );
        }
        // ロットが埋まった → この注文のロックを解放して進める。
        let _ = conn.execute(
            "UPDATE manufacturing_orders SET lot_lock=0, updated_at=datetime('now') WHERE id=?1",
            params![id],
        );
    }

    let refund_flag = if to == "refund_pending" { 1 } else { 0 };
    if let Some(n) = note {
        conn.execute(
            "UPDATE manufacturing_orders \
             SET status=?1, refund_flag=?2, note=?3, updated_at=datetime('now') WHERE id=?4",
            params![to, refund_flag, n, id],
        )
    } else {
        conn.execute(
            "UPDATE manufacturing_orders \
             SET status=?1, refund_flag=?2, updated_at=datetime('now') WHERE id=?3",
            params![to, refund_flag, id],
        )
    }
    .map_err(|e| format!("update manufacturing_orders: {}", e))?;

    let order = fetch_order_json(conn, id).unwrap_or(Value::Null);
    Ok(json!({ "ok": true, "order": order }))
}

/// 受注一覧（フィルタ付き）。refund_pending=1 で自動返金対象のみ。
pub fn order_list(
    conn: &Connection,
    supplier_id: Option<&str>,
    status: Option<&str>,
    refund_pending_only: bool,
    owner_email: Option<&str>,
) -> Value {
    sweep_expired_locks(conn);

    let mut where_clauses: Vec<String> = Vec::new();
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(o) = owner_email.filter(|x| !x.trim().is_empty()) {
        where_clauses.push(format!("owner_email=?{}", binds.len() + 1));
        binds.push(Box::new(o.to_string()));
    }
    if let Some(s) = supplier_id.filter(|x| !x.trim().is_empty()) {
        where_clauses.push(format!("supplier_id=?{}", binds.len() + 1));
        binds.push(Box::new(s.to_string()));
    }
    if let Some(st) = status.filter(|x| !x.trim().is_empty()) {
        where_clauses.push(format!("status=?{}", binds.len() + 1));
        binds.push(Box::new(st.to_string()));
    }
    if refund_pending_only {
        where_clauses.push("refund_flag=1 AND status='refund_pending'".to_string());
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", where_clauses.join(" AND "))
    };

    let sql = format!(
        "SELECT {} FROM manufacturing_orders{} ORDER BY id DESC",
        ORDER_COLS, where_sql
    );

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => return json!({ "ok": false, "error": format!("prepare: {}", e), "orders": [] }),
    };
    let bind_refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    let rows: Vec<Value> = stmt
        .query_map(bind_refs.as_slice(), read_order_row)
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default();

    json!({ "ok": true, "count": rows.len(), "orders": rows })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manufacturing_schema::ensure_manufacturing_schema;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_manufacturing_schema(&conn);
        conn
    }

    /// received 済み RFQ を1件作って id を返す。
    fn seed_received_rfq(conn: &Connection, supplier: &str, kind: &str, qty: i64) -> i64 {
        let created = crate::rfq::rfq_create(
            conn,
            Some(supplier),
            Some(kind),
            None,
            qty,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let id = created["rfq"]["id"].as_i64().unwrap();
        crate::rfq::rfq_record(conn, id, Some("received"), Some(35000), None, None, None, None)
            .unwrap();
        id
    }

    #[test]
    fn create_from_received_rfq_and_linear_advance_to_completed() {
        let conn = setup();
        let rid = seed_received_rfq(&conn, "isami_gi", "gi", 10);
        let created = order_create(&conn, Some(rid), None, None, 0, None, None, None).unwrap();
        let oid = created["order"]["id"].as_i64().unwrap();
        assert_eq!(created["order"]["status"], "ordered");
        assert_eq!(created["order"]["supplier_id"], "isami_gi");
        assert_eq!(created["order"]["unit_jpy"], 35000);
        assert_eq!(created["order"]["lot_lock"], false); // isami MOQ=10, qty=10 → ロック無し

        order_advance(&conn, oid, "production", None).unwrap();
        order_advance(&conn, oid, "shipped", None).unwrap();
        let done = order_advance(&conn, oid, "completed", Some("納品確認")).unwrap();
        assert_eq!(done["order"]["status"], "completed");
    }

    #[test]
    fn create_rejects_rfq_not_received() {
        let conn = setup();
        // drafted のままの RFQ からは発注できない。
        let created = crate::rfq::rfq_create(
            &conn,
            Some("isami_gi"),
            Some("gi"),
            None,
            10,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let rid = created["rfq"]["id"].as_i64().unwrap();
        let err = order_create(&conn, Some(rid), None, None, 0, None, None, None);
        assert!(err.is_err(), "drafted RFQ からの発注はエラーであるべき");
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        let conn = setup();
        let rid = seed_received_rfq(&conn, "isami_gi", "gi", 10);
        let created = order_create(&conn, Some(rid), None, None, 0, None, None, None).unwrap();
        let oid = created["order"]["id"].as_i64().unwrap();

        // スキップ禁止（ordered→shipped）。
        assert!(order_advance(&conn, oid, "shipped", None).is_err());
        // 存在しない状態。
        assert!(order_advance(&conn, oid, "cancelled", None).is_err());
        // 正常に進めた後の後退禁止。
        order_advance(&conn, oid, "production", None).unwrap();
        assert!(order_advance(&conn, oid, "ordered", None).is_err());
        // 完了後の変更禁止。
        order_advance(&conn, oid, "shipped", None).unwrap();
        order_advance(&conn, oid, "completed", None).unwrap();
        assert!(order_advance(&conn, oid, "refunded", None).is_err());
    }

    #[test]
    fn heritage_below_moq_is_lot_locked_until_lot_fills() {
        let conn = setup();
        // heritage_loopwheel MOQ=15。5着の注文はロットロックされる。
        let c1 = order_create(
            &conn,
            None,
            Some("heritage_loopwheel"),
            Some("loopwheel_sweat"),
            5,
            Some(35000),
            None,
            None,
        )
        .unwrap();
        let o1 = c1["order"]["id"].as_i64().unwrap();
        assert_eq!(c1["order"]["lot_lock"], true);
        assert!(c1["order"]["lot_id"].as_str().is_some());
        assert!(c1["order"]["lock_deadline"].as_str().is_some());

        // ロック中は production に進めない。
        let err = order_advance(&conn, o1, "production", None);
        assert!(err.is_err(), "ロット未達で生産開始はエラーであるべき");

        // 同じロットに10着追加 → 合計15で MOQ 到達。2件目はロック無し。
        let c2 = order_create(
            &conn,
            None,
            Some("heritage_loopwheel"),
            Some("loopwheel_sweat"),
            10,
            Some(35000),
            None,
            None,
        )
        .unwrap();
        let o2 = c2["order"]["id"].as_i64().unwrap();
        assert_eq!(c2["order"]["lot_lock"], false);

        let lot = c1["order"]["lot_id"].as_str().unwrap_or("");
        eprintln!("DEBUG lot={} o1_lock={} o2_lock={} total={}",
            lot,
            c1["order"]["lot_lock"],
            c2["order"]["lot_lock"],
            lot_total_qty(&conn, lot));
        // ロットが埋まったので1件目も production に進める（ロック自動解放）。
        let adv = order_advance(&conn, o1, "production", None).unwrap();
        assert_eq!(adv["order"]["status"], "production");
        assert_eq!(adv["order"]["lot_lock"], false);

        // 2件目も普通に進む。
        order_advance(&conn, o2, "production", None).unwrap();
    }

    #[test]
    fn expired_lot_lock_sweeps_to_refund_pending() {
        let conn = setup();
        let c = order_create(
            &conn,
            None,
            Some("heritage_loopwheel"),
            Some("loopwheel_sweat"),
            5,
            Some(35000),
            None,
            None,
        )
        .unwrap();
        let oid = c["order"]["id"].as_i64().unwrap();
        // 期限を過去に書き換えてスイープを発火させる。
        conn.execute(
            "UPDATE manufacturing_orders SET lock_deadline=datetime('now','-1 day') WHERE id=?1",
            params![oid],
        )
        .unwrap();
        let n = sweep_expired_locks(&conn);
        assert_eq!(n, 1);
        let o = fetch_order_json(&conn, oid).unwrap();
        assert_eq!(o["status"], "refund_pending");
        assert_eq!(o["refund_flag"], true);
        assert!(o["refund_reason"].as_str().unwrap().contains("MOQ未達"));

        // 返金実行（人間）後の記録: refund_pending → refunded は許可。
        let done = order_advance(&conn, oid, "refunded", Some("Stripe 払い戻し済")).unwrap();
        assert_eq!(done["order"]["status"], "refunded");
        // refunded からはどこにも行けない。
        assert!(order_advance(&conn, oid, "ordered", None).is_err());
    }

    #[test]
    fn order_list_filters_and_refund_pending_view() {
        let conn = setup();
        order_create(
            &conn,
            None,
            Some("heritage_loopwheel"),
            Some("loopwheel_sweat"),
            5,
            None,
            None,
            Some("a@example.com"),
        )
        .unwrap();
        order_create(
            &conn,
            None,
            Some("heritage_loopwheel"),
            Some("loopwheel_sweat"),
            3,
            None,
            None,
            Some("b@example.com"),
        )
        .unwrap();

        let all = order_list(&conn, None, None, false, None);
        assert_eq!(all["count"].as_i64().unwrap(), 2);

        let mine = order_list(&conn, None, None, false, Some("a@example.com"));
        assert_eq!(mine["count"].as_i64().unwrap(), 1);

        // 両方 refund_pending にして一覧APIで拾えるか。
        conn.execute(
            "UPDATE manufacturing_orders SET status='refund_pending', refund_flag=1", [],
        )
        .unwrap();
        let refunds = order_list(&conn, None, None, true, None);
        assert_eq!(refunds["count"].as_i64().unwrap(), 2);
        let locked = order_list(&conn, None, Some("ordered"), false, None);
        assert_eq!(locked["count"].as_i64().unwrap(), 0);
    }
}
