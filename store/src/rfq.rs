//! 製造オーケストレーション層 (C): RFQ 見積ライフサイクル。
//!
//! 非POD(mode="quote")サプライヤ(isami_gi / heritage_loopwheel /
//! shima_seamless / contrado_uk)の「要見積」を、DB駆動の状態機械
//! `drafted → sent → received → expired` に昇格する。
//!
//! ## 流儀・ゲート（CATALOG_CONTRACT 準拠）
//! - 新テーブルは作らない。状態は `quote_requests`（manufacturing_schema.rs で確定済）。
//!   本モジュールは CREATE TABLE を一切書かない。
//! - **対外送信(メール/PO)は絶対にしない**。ドラフト生成 + DB 保存まで。送信は人間ゲート。
//! - メール下書きにはPII・本物の連絡先を書かない（プレースホルダのみ）。実宛先の確認は人間。
//! - `updated_at` は自動更新トリガが無いので UPDATE 側で必ず `datetime('now')` を手書きする。
//! - サプライヤ正準は `crate::catalog::SUPPLIER_REGISTRY`（T-SHARED で pub(crate) 化前提）。
//!   kind 推論は `crate::catalog::infer_kind`（同・pub(crate) 化前提）。

use rusqlite::{params, Connection};
use serde_json::{json, Value};

use crate::catalog::{infer_kind, SupplierCapability, SUPPLIER_REGISTRY};

/// 状態機械の許可状態（CHECK 制約と一致）。
const ALLOWED_STATUS: &[&str] = &["drafted", "sent", "received", "expired"];

/// レジストリから supplier を引く（id 一致）。
fn supplier_by_id(supplier_id: &str) -> Option<&'static SupplierCapability> {
    SUPPLIER_REGISTRY.iter().find(|s| s.id == supplier_id)
}

/// supplier_id → 表示名（不明なら id をそのまま返す）。
fn supplier_name_of(supplier_id: &str) -> String {
    supplier_by_id(supplier_id)
        .map(|s| s.name.to_string())
        .unwrap_or_else(|| supplier_id.to_string())
}

/// kind / description から quote モードのサプライヤを解決する。
/// 1) kind 一致（quote モードのみ）を優先 2) なければ description→kind 推論で再試行。
fn resolve_quote_supplier(kind: Option<&str>, description: Option<&str>) -> Option<&'static str> {
    // kind 明示があれば、その kind を扱える quote サプライヤを探す。
    let resolved_kind: Option<String> = kind
        .map(|k| k.trim().to_lowercase())
        .filter(|k| !k.is_empty())
        .or_else(|| description.map(infer_kind).filter(|k| !k.is_empty()));

    let k = resolved_kind?;
    SUPPLIER_REGISTRY
        .iter()
        .find(|s| s.mode == "quote" && s.kinds.iter().any(|kk| *kk == k.as_str()))
        .map(|s| s.id)
}

/// 既存メール下書き本文（プレースホルダのみ・**送信しない**）を生成する。
///
/// `mu_gi_request_logos` 流の固定テンプレ:
/// 社名 / 数量 / kind / MOQ 確認 / 入稿(入稿形式)の質問を含む。
/// 実宛先・実連絡先・住所/電話などPIIは一切書かない（人間が確認して差し込む）。
///
/// 返り値: `(件名, 本文)`。
pub fn draft_email_for(supplier_id: &str, kind: &str, qty: i64) -> (String, String) {
    let supplier = supplier_by_id(supplier_id);
    let supplier_label = supplier.map(|s| s.name).unwrap_or(supplier_id);
    let moq = supplier.map(|s| s.moq).filter(|m| *m > 0);
    let lead = supplier.map(|s| s.lead_time_days).filter(|l| *l > 0);

    let subject = format!(
        "【お見積りのご相談】{kind}（{qty}着）— 株式会社イネブラ 濱田",
        kind = kind,
        qty = qty,
    );

    // MOQ / 納期は分かっていれば「想定」として確認質問に織り込む（プレースホルダ）。
    let moq_line = match moq {
        Some(m) => format!(
            "・最小ロット(MOQ): まず {qty}着で検討しています（御社想定 MOQ {m}着 との認識で相違ないかご確認ください）\n",
            qty = qty,
            m = m,
        ),
        None => format!(
            "・最小ロット(MOQ): まず {qty}着で検討しています。御社の最小ロットをお教えください\n",
            qty = qty,
        ),
    };
    let lead_line = match lead {
        Some(l) => format!("・希望納期: 御社想定リードタイム約 {l}日 を前提に進められますでしょうか\n", l = l),
        None => "・希望納期: 御社の標準リードタイムをお教えください\n".to_string(),
    };

    let body = format!(
        "{supplier_label} ご担当者 様\n\n\
         突然のご連絡失礼いたします。株式会社イネブラ（Enabler Inc.）の濱田と申します。\n\
         弊社で展開しているアパレルブランド MU（wearmu.com）にて、下記の製造をご相談したく\n\
         お見積りのお願いを差し上げます。\n\n\
         【ご相談内容（想定値・確定前）】\n\
         ・品目(kind): {kind}\n\
         ・数量: 約 {qty}着\n\
         {moq_line}\
         {lead_line}\
         ・入稿形式: 御社が受け付けられる入稿データ形式（AI / EPS / SVG / PDF / 配置仕様書 等）をお教えください\n\
         ・単価/型代/初期費用: 上記数量での概算お見積りをお願いいたします\n\n\
         技術資料（テックパック・配置仕様書）はご返信後に共有いたします。\n\
         まずは可否と概算感だけでも頂けますと幸いです。\n\n\
         ──────────\n\
         株式会社イネブラ（Enabler Inc.）／ MU（wearmu.com）\n\
         担当: 濱田\n\
         ※本メールは下書きです。実際の送信前に担当者名・宛先・連絡先を確認してください。\n",
        supplier_label = supplier_label,
        kind = kind,
        qty = qty,
        moq_line = moq_line,
        lead_line = lead_line,
    );

    (subject, body)
}

/// 既存 docs のメール下書きを `status='drafted'` 行として移植する（冪等）。
///
/// docs（heritage / seamless / gi / contrado）由来の各 quote サプライヤについて
/// 1行ずつ `INSERT OR IGNORE`。本文は `draft_email_for` で生成（プレースホルダのみ）。
/// 既に drafted 行があれば二重投入しない（同一 supplier/kind の drafted を確認）。
pub fn seed_rfq_drafts(conn: &Connection) {
    // docs に裏取りのある quote サプライヤ × 代表 kind × 既定数量。
    // 数量は各 docs の「想定ロット」を反映（無ければ MOQ ベース）。
    let seeds: &[(&str, &str, i64)] = &[
        ("isami_gi", "gi", 10),                 // docs/gi-isami-2026-05-12（試作→10-30着量産）
        ("heritage_loopwheel", "loopwheel_sweat", 30), // docs/heritage-supplier-inquiries.md（30着限定）
        ("shima_seamless", "seamless_knit", 30), // docs/seamless_knit（型代償却・要見積）
        ("contrado_uk", "rashguard_premium", 50), // docs/CONTRADO_SALES_OUTREACH.md（30-100/月）
    ];

    for (supplier_id, kind, qty) in seeds {
        // 既存 drafted があればスキップ（冪等・本文の上書きはしない）。
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM quote_requests \
                 WHERE supplier_id=?1 AND kind=?2 AND status='drafted'",
                params![supplier_id, kind],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if exists > 0 {
            continue;
        }

        let (subject, body) = draft_email_for(supplier_id, kind, *qty);
        let _ = conn.execute(
            "INSERT OR IGNORE INTO quote_requests \
                (supplier_id, kind, qty, status, note, draft_subject, draft_body) \
             VALUES (?1, ?2, ?3, 'drafted', ?4, ?5, ?6)",
            params![
                supplier_id,
                kind,
                qty,
                "seed: docs 由来の見積下書き。送信は人間ゲート。",
                subject,
                body
            ],
        );
    }
}

/// 1行 quote_requests を JSON 化（supplier_name を補完）。
fn row_to_json(
    id: i64,
    supplier_id: &str,
    kind: &str,
    spec_id: Option<String>,
    product_ref: Option<String>,
    qty: i64,
    spec_pack_url: Option<String>,
    status: &str,
    quoted_unit_jpy: Option<i64>,
    moq: Option<i64>,
    lead_time_days: Option<i64>,
    valid_until: Option<String>,
    note: Option<String>,
    draft_subject: Option<String>,
    draft_body: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
) -> Value {
    json!({
        "id": id,
        "supplier_id": supplier_id,
        "supplier_name": supplier_name_of(supplier_id),
        "kind": kind,
        "spec_id": spec_id,
        "product_ref": product_ref,
        "qty": qty,
        "spec_pack_url": spec_pack_url,
        "status": status,
        "quoted_unit_jpy": quoted_unit_jpy,
        "moq": moq,
        "lead_time_days": lead_time_days,
        "valid_until": valid_until,
        "note": note,
        "draft_subject": draft_subject,
        "draft_body": draft_body,
        "created_at": created_at,
        "updated_at": updated_at,
    })
}

/// id 指定で1行を JSON 取得（無ければ None）。
fn fetch_rfq_json(conn: &Connection, id: i64) -> Option<Value> {
    conn.query_row(
        "SELECT id, supplier_id, kind, spec_id, product_ref, qty, spec_pack_url, status, \
                quoted_unit_jpy, moq, lead_time_days, valid_until, note, \
                draft_subject, draft_body, created_at, updated_at \
         FROM quote_requests WHERE id=?1",
        params![id],
        |r| {
            Ok(row_to_json(
                r.get(0)?,
                &r.get::<_, String>(1)?,
                &r.get::<_, String>(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                &r.get::<_, String>(7)?,
                r.get(8)?,
                r.get(9)?,
                r.get(10)?,
                r.get(11)?,
                r.get(12)?,
                r.get(13)?,
                r.get(14)?,
                r.get(15)?,
                r.get(16)?,
            ))
        },
    )
    .ok()
}

/// RFQ を `status='drafted'` で新規作成する。
///
/// - `supplier_id` 省略時: `kind`/`description` から quote モード候補を解決。
/// - `kind` 省略時: `description` を `infer_kind` で推論。
/// - `draft_subject`/`draft_body` は `draft_email_for` で埋める（送信はしない）。
#[allow(clippy::too_many_arguments)]
pub fn rfq_create(
    conn: &Connection,
    supplier_id: Option<&str>,
    kind: Option<&str>,
    description: Option<&str>,
    qty: i64,
    spec_id: Option<&str>,
    product_ref: Option<&str>,
    spec_pack_url: Option<&str>,
    note: Option<&str>,
    owner_email: Option<&str>,
) -> Result<Value, String> {
    let qty = qty.max(1);

    // supplier 解決: 明示 > kind/description からの quote 候補。
    let resolved_supplier: String = match supplier_id.filter(|s| !s.trim().is_empty()) {
        Some(s) => {
            // 明示指定は実在チェック（レジストリ外も許すが warn 用に確認）。
            s.trim().to_string()
        }
        None => resolve_quote_supplier(kind, description)
            .ok_or_else(|| {
                "supplier_id を解決できませんでした。kind か description を具体化するか supplier_id を指定してください（要見積サプライヤ: isami_gi / heritage_loopwheel / shima_seamless / contrado_uk）".to_string()
            })?
            .to_string(),
    };

    // kind 解決: 明示 > description 推論 > supplier の代表 kind。
    let resolved_kind: String = kind
        .map(|k| k.trim().to_lowercase())
        .filter(|k| !k.is_empty())
        .or_else(|| description.map(infer_kind).filter(|k| !k.is_empty()))
        .or_else(|| {
            supplier_by_id(&resolved_supplier)
                .and_then(|s| s.kinds.first())
                .map(|k| k.to_string())
        })
        .ok_or_else(|| {
            "kind を特定できませんでした。kind か description を渡してください".to_string()
        })?;

    let (subject, body) = draft_email_for(&resolved_supplier, &resolved_kind, qty);

    conn.execute(
        "INSERT INTO quote_requests \
            (supplier_id, kind, spec_id, product_ref, qty, spec_pack_url, status, note, draft_subject, draft_body, owner_email) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'drafted', ?7, ?8, ?9, ?10)",
        params![
            resolved_supplier,
            resolved_kind,
            spec_id,
            product_ref,
            qty,
            spec_pack_url,
            note,
            subject,
            body,
            owner_email
        ],
    )
    .map_err(|e| format!("insert quote_requests: {}", e))?;

    let id = conn.last_insert_rowid();
    let rfq = fetch_rfq_json(conn, id).unwrap_or(Value::Null);

    Ok(json!({
        "ok": true,
        "rfq": rfq,
        "next": "下書き(draft_subject/draft_body)を人間が確認・送信してください。送信後は rfq_record で status='sent' に。対外送信はエージェントが行いません。",
    }))
}

/// RFQ の状態を更新する（状態機械の遷移を記録）。
///
/// - `status` は ALLOWED_STATUS のみ。
/// - `status='received'` の場合 `quoted_unit_jpy` は必須。
/// - UPDATE では `updated_at=datetime('now')` を手書きする。
#[allow(clippy::too_many_arguments)]
/// ゼロ詰め ISO 日付 `YYYY-MM-DD` か。received_quote_for の辞書順足切りの前提を守る。
/// 月日の範囲までは厳密判定しない（辞書順比較が壊れない形式の保証が目的）。
fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[8..10].iter().all(u8::is_ascii_digit)
}

/// RFQ の所有者メール（per-agent 認可チェック用）。行が無い/未設定なら None。
pub fn rfq_owner_email(conn: &Connection, id: i64) -> Option<String> {
    conn.query_row(
        "SELECT owner_email FROM quote_requests WHERE id=?1",
        params![id],
        |r| r.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
}

pub fn rfq_record(
    conn: &Connection,
    id: i64,
    status: Option<&str>,
    quoted_unit_jpy: Option<i64>,
    moq: Option<i64>,
    lead_time_days: Option<i64>,
    valid_until: Option<&str>,
    note: Option<&str>,
) -> Result<Value, String> {
    // 対象存在チェック。
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM quote_requests WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if exists == 0 {
        return Err(format!("rfq id={} が見つかりません", id));
    }

    if let Some(s) = status {
        if !ALLOWED_STATUS.contains(&s) {
            return Err(format!(
                "status は {:?} のいずれか（received は quoted_unit_jpy 必須）",
                ALLOWED_STATUS
            ));
        }
        if s == "received" && quoted_unit_jpy.is_none() {
            return Err("status='received' には quoted_unit_jpy が必須です".to_string());
        }
    }

    // valid_until は received_quote_for が文字列辞書順で足切り（>=date('now')）するため、
    // ゼロ詰め ISO `YYYY-MM-DD` のみ許可。非ISO/ゼロ詰め欠落は順序比較が破綻するので弾く。
    if let Some(vu) = valid_until {
        if !is_iso_date(vu) {
            return Err(format!(
                "valid_until は YYYY-MM-DD（ゼロ詰め ISO 日付）で渡してください: '{}'",
                vu
            ));
        }
    }

    // 動的に SET 句を組み立てる（指定された列のみ更新）。updated_at は常に手書き。
    let mut sets: Vec<String> = vec!["updated_at=datetime('now')".to_string()];
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(s) = status {
        sets.push(format!("status=?{}", binds.len() + 1));
        binds.push(Box::new(s.to_string()));
    }
    if let Some(v) = quoted_unit_jpy {
        sets.push(format!("quoted_unit_jpy=?{}", binds.len() + 1));
        binds.push(Box::new(v));
    }
    if let Some(v) = moq {
        sets.push(format!("moq=?{}", binds.len() + 1));
        binds.push(Box::new(v));
    }
    if let Some(v) = lead_time_days {
        sets.push(format!("lead_time_days=?{}", binds.len() + 1));
        binds.push(Box::new(v));
    }
    if let Some(v) = valid_until {
        sets.push(format!("valid_until=?{}", binds.len() + 1));
        binds.push(Box::new(v.to_string()));
    }
    if let Some(v) = note {
        sets.push(format!("note=?{}", binds.len() + 1));
        binds.push(Box::new(v.to_string()));
    }

    let id_placeholder = binds.len() + 1;
    binds.push(Box::new(id));

    let sql = format!(
        "UPDATE quote_requests SET {} WHERE id=?{}",
        sets.join(", "),
        id_placeholder
    );
    let bind_refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    conn.execute(&sql, bind_refs.as_slice())
        .map_err(|e| format!("update quote_requests: {}", e))?;

    let rfq = fetch_rfq_json(conn, id).unwrap_or(Value::Null);
    Ok(json!({ "ok": true, "rfq": rfq }))
}

/// フィルタ付き RFQ 一覧（supplier_name は SUPPLIER_REGISTRY から補完）。
pub fn rfq_list(
    conn: &Connection,
    supplier_id: Option<&str>,
    kind: Option<&str>,
    status: Option<&str>,
    // Some(email)=その所有者のRFQのみ（ユーザーページ）。None=全件（管理者）。
    owner_email: Option<&str>,
) -> Value {
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
    if let Some(k) = kind.filter(|x| !x.trim().is_empty()) {
        where_clauses.push(format!("kind=?{}", binds.len() + 1));
        binds.push(Box::new(k.to_string()));
    }
    if let Some(st) = status.filter(|x| !x.trim().is_empty()) {
        where_clauses.push(format!("status=?{}", binds.len() + 1));
        binds.push(Box::new(st.to_string()));
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", where_clauses.join(" AND "))
    };

    let sql = format!(
        "SELECT id, supplier_id, kind, spec_id, product_ref, qty, spec_pack_url, status, \
                quoted_unit_jpy, moq, lead_time_days, valid_until, note, \
                draft_subject, draft_body, created_at, updated_at \
         FROM quote_requests{} ORDER BY id DESC",
        where_sql
    );

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => return json!({ "ok": false, "error": format!("prepare: {}", e), "rfqs": [] }),
    };
    let bind_refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    let rows: Vec<Value> = stmt
        .query_map(bind_refs.as_slice(), |r| {
            Ok(row_to_json(
                r.get(0)?,
                &r.get::<_, String>(1)?,
                &r.get::<_, String>(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                &r.get::<_, String>(7)?,
                r.get(8)?,
                r.get(9)?,
                r.get(10)?,
                r.get(11)?,
                r.get(12)?,
                r.get(13)?,
                r.get(14)?,
                r.get(15)?,
                r.get(16)?,
            ))
        })
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default();

    json!({
        "ok": true,
        "count": rows.len(),
        "rfqs": rows,
    })
}

/// 共有契約: 最新の有効な確定見積単価を返す。
///
/// `status='received'` かつ（`valid_until IS NULL` または `valid_until>=date('now')`）の
/// 中で最新（id 降順）の `quoted_unit_jpy` を返す。無ければ None。
/// 仕様ルーター（route_request 等）が「実見積があれば est より優先」するための入口。
pub fn received_quote_for(conn: &Connection, supplier_id: &str, kind: &str) -> Option<i64> {
    conn.query_row(
        "SELECT quoted_unit_jpy FROM quote_requests \
         WHERE supplier_id=?1 AND kind=?2 AND status='received' \
           AND quoted_unit_jpy IS NOT NULL \
           AND (valid_until IS NULL OR valid_until>=date('now')) \
         ORDER BY id DESC LIMIT 1",
        params![supplier_id, kind],
        |r| r.get::<_, Option<i64>>(0),
    )
    .ok()
    .flatten()
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

    #[test]
    fn create_record_received_then_received_quote_for_returns_value() {
        let conn = setup();
        let created = rfq_create(
            &conn,
            Some("isami_gi"),
            Some("gi"),
            None,
            10,
            None,
            None,
            None,
            Some("test"),
            None,
        )
        .unwrap();
        let id = created["rfq"]["id"].as_i64().unwrap();
        assert_eq!(created["rfq"]["status"], "drafted");

        // received 時は quoted_unit_jpy 必須 → 無いとエラー。
        let err = rfq_record(&conn, id, Some("received"), None, None, None, None, None);
        assert!(err.is_err(), "received without quoted_unit_jpy must error");

        // 正しく received へ。
        rfq_record(
            &conn,
            id,
            Some("received"),
            Some(18000),
            Some(10),
            Some(45),
            None, // valid_until NULL → 有効
            Some("見積到着"),
        )
        .unwrap();

        let q = received_quote_for(&conn, "isami_gi", "gi");
        assert_eq!(q, Some(18000));
    }

    #[test]
    fn received_quote_with_past_valid_until_returns_none() {
        let conn = setup();
        let created =
            rfq_create(&conn, Some("isami_gi"), Some("gi"), None, 10, None, None, None, None, None)
                .unwrap();
        let id = created["rfq"]["id"].as_i64().unwrap();
        rfq_record(
            &conn,
            id,
            Some("received"),
            Some(20000),
            None,
            None,
            Some("2000-01-01"), // 過去 → 無効
            None,
        )
        .unwrap();
        assert_eq!(received_quote_for(&conn, "isami_gi", "gi"), None);
    }

    #[test]
    fn list_filters_by_supplier_kind_status() {
        let conn = setup();
        rfq_create(&conn, Some("isami_gi"), Some("gi"), None, 10, None, None, None, None, None).unwrap();
        rfq_create(
            &conn,
            Some("heritage_loopwheel"),
            Some("loopwheel_sweat"),
            None,
            30,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let all = rfq_list(&conn, None, None, None, None);
        assert_eq!(all["count"].as_i64().unwrap(), 2);

        let only_gi = rfq_list(&conn, Some("isami_gi"), None, None, None);
        assert_eq!(only_gi["count"].as_i64().unwrap(), 1);
        assert_eq!(only_gi["rfqs"][0]["supplier_id"], "isami_gi");
        // supplier_name 補完。
        assert!(only_gi["rfqs"][0]["supplier_name"]
            .as_str()
            .unwrap()
            .contains("ISAMI"));

        let drafted = rfq_list(&conn, None, None, Some("drafted"), None);
        assert_eq!(drafted["count"].as_i64().unwrap(), 2);
        let received = rfq_list(&conn, None, None, Some("received"), None);
        assert_eq!(received["count"].as_i64().unwrap(), 0);
    }

    #[test]
    fn draft_email_for_has_no_send_fields_and_includes_required_points() {
        let (subject, body) = draft_email_for("isami_gi", "gi", 10);
        // 件名・本文に kind/数量/MOQ確認/入稿形式が含まれる。
        assert!(subject.contains("gi"));
        assert!(body.contains("10着") || body.contains("10"));
        assert!(body.contains("MOQ") || body.contains("最小ロット"));
        assert!(body.contains("入稿"));
        // 送信用フィールド（宛先/連絡先/PII）は含まない。
        let lower = body.to_lowercase();
        assert!(!lower.contains("@"), "draft body must not embed any email address");
        assert!(!body.contains("電話"), "draft body must not embed phone");
        assert!(!body.contains("〒"), "draft body must not embed postal address");
    }

    #[test]
    fn resolve_supplier_from_description_when_supplier_omitted() {
        let conn = setup();
        // supplier_id 省略 + description のみ → quote 候補解決。
        let created = rfq_create(
            &conn,
            None,
            None,
            Some("和歌山のループウィールのスウェットを作りたい"),
            30,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(created["rfq"]["supplier_id"], "heritage_loopwheel");
        assert_eq!(created["rfq"]["kind"], "loopwheel_sweat");
    }

    #[test]
    fn seed_rfq_drafts_is_idempotent() {
        let conn = setup();
        seed_rfq_drafts(&conn);
        let first = rfq_list(&conn, None, None, Some("drafted"), None);
        let n1 = first["count"].as_i64().unwrap();
        assert!(n1 >= 4, "expected >=4 seeded drafts, got {}", n1);
        // 2回目で増えない。
        seed_rfq_drafts(&conn);
        let second = rfq_list(&conn, None, None, Some("drafted"), None);
        assert_eq!(second["count"].as_i64().unwrap(), n1);
    }
}
