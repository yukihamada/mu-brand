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
        -- 付与時に同額を mu_credits にも積む (= MU 内購入で実際に使える本物の残高)。
        -- 失効は consign_balance_lazy_expire() が expires_at 経過ロットの未使用分を
        -- mu_credits から差し引き、swept_at を立てる (6ヶ月失効をコードで強制)。
        CREATE TABLE IF NOT EXISTS consign_balance_ledger (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            email       TEXT NOT NULL,
            item_id     TEXT,
            amount_jpy  INTEGER NOT NULL,        -- 付与額 (>0)
            spent_jpy   INTEGER NOT NULL DEFAULT 0,
            source      TEXT NOT NULL,           -- 'sell' | 'consign_sold'
            granted_at  INTEGER NOT NULL,
            expires_at  INTEGER NOT NULL,        -- granted_at + 6ヶ月 (必須)
            swept_at    INTEGER                  -- 失効処理済の時刻 (NULL=未失効)
        );
        -- 査定の一時保存 (submit が quote_id を引くため。submit は再計算せずこれを信頼)。
        -- consumed_at: submit で使用済みになると立つ。二重 submit (= 残高の無限 mint) を防ぐ。
        CREATE TABLE IF NOT EXISTS consign_quotes (
            id          TEXT PRIMARY KEY,
            email       TEXT NOT NULL DEFAULT '',
            category    TEXT NOT NULL DEFAULT '',
            memo        TEXT NOT NULL DEFAULT '',
            image_ref   TEXT NOT NULL DEFAULT '',
            estimate_jpy INTEGER NOT NULL,
            reason      TEXT NOT NULL DEFAULT '',
            created_at  INTEGER NOT NULL,
            consumed_at INTEGER                  -- submit で消費済 (NULL=未使用)
        );
        -- メール所有確認 (マジックリンク)。残高付与 (sell) は verified_at が
        -- 立っている email にのみ行う = なりすまし付与を塞ぐ。token は推測不能・
        -- 1回踏めば verified_at が立ち、以降そのメールは検証済み扱い。
        -- 出金/換金は無いので token に強い PII は乗らない (email のみ)。
        CREATE TABLE IF NOT EXISTS consign_email_verifications (
            email       TEXT PRIMARY KEY,        -- 小文字正規化。1メール1行 (upsert)
            token       TEXT NOT NULL,           -- 推測不能なマジックリンク token (踏むと検証)
            created_at  INTEGER NOT NULL,        -- token 発行時刻
            verified_at INTEGER                  -- 検証完了時刻 (NULL=未検証)
        );
        CREATE INDEX IF NOT EXISTS idx_consign_emailverif_token ON consign_email_verifications(token);
        CREATE INDEX IF NOT EXISTS idx_consign_ledger_email ON consign_balance_ledger(email, expires_at);
        CREATE INDEX IF NOT EXISTS idx_consign_intents_item ON consign_intents(item_id);"
    );
    // 既存DBへの後方互換マイグレーション (列が無ければ足す。ある場合のエラーは無視)。
    let _ = conn.execute("ALTER TABLE consign_balance_ledger ADD COLUMN swept_at INTEGER", []);
    let _ = conn.execute("ALTER TABLE consign_quotes ADD COLUMN consumed_at INTEGER", []);
}

/// 失効処理 (遅延): expires_at を過ぎた未失効ロットの「まだ使われていない分」を
/// mu_credits から差し引き、swept_at を立てる。残高表示・付与・submit のたびに呼ぶ。
///
/// 不変条件 #1 (6ヶ月失効) をコードで強制する本体。付与は mu_credit_apply(+) で
/// 即 MU 内購入に使える本物の残高にしてあるので、失効は「未使用の付与分を回収」する形。
/// clawback は現在の mu_credits 残高を下限 0 でキャップ (他で稼いだ残高は削らない)。
fn consign_balance_lazy_expire(conn: &Connection, email: &str) {
    let now = now_s();
    let email_lc = email.to_lowercase();
    // 失効すべきロット: 期限切れ かつ 未失効 かつ 未使用残 (amount - spent) > 0。
    let mut stmt = match conn.prepare(
        "SELECT id, amount_jpy, spent_jpy FROM consign_balance_ledger
         WHERE LOWER(email)=? AND expires_at <= ? AND swept_at IS NULL"
    ) { Ok(s) => s, Err(_) => return };
    let lots: Vec<(i64, i64, i64)> = stmt.query_map(params![email_lc, now], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
    }).map(|it| it.flatten().collect()).unwrap_or_default();
    drop(stmt);
    for (id, amount, spent) in lots {
        let unused = (amount - spent).max(0);
        // mu_credits から未使用分を回収 (現残高を超えない = 他収入は守る)。
        if unused > 0 {
            let bal = crate::mu_credit_balance(conn, &email_lc);
            let claw = unused.min(bal.max(0));
            if claw > 0 {
                let _ = crate::mu_credit_apply(
                    conn, &email_lc, -claw,
                    &format!("consign_expire:{}", id), Some(&format!("consign-expire-{}", id)));
            }
        }
        // ロットを失効済に: spent=amount にして残ゼロ、swept_at を記録。
        let _ = conn.execute(
            "UPDATE consign_balance_ledger SET spent_jpy = amount_jpy, swept_at = ? WHERE id = ?",
            params![now, id]);
    }
}

fn now_s() -> i64 {
    crate::chrono_now().parse().unwrap_or(0)
}

/// 最低限のメール形式チェック (空・@無し・ドメイン無しを弾く)。所有確認ではない。
fn valid_email(s: &str) -> bool {
    let s = s.trim();
    if s.len() < 5 || s.len() > 254 || s.contains(char::is_whitespace) { return false; }
    let mut it = s.splitn(2, '@');
    let (local, domain) = match (it.next(), it.next()) { (Some(l), Some(d)) => (l, d), _ => return false };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

/// 当該 email がメール所有確認済みか (verified_at が立っているか)。
/// 残高付与 (sell) の前提条件。未検証なら付与しない (なりすまし防止)。
fn email_verified(conn: &Connection, email: &str) -> bool {
    let email_lc = email.trim().to_lowercase();
    conn.query_row(
        "SELECT verified_at FROM consign_email_verifications WHERE email=?",
        params![email_lc],
        |r| r.get::<_, Option<i64>>(0),
    ).ok().flatten().is_some()
}

/// 推測不能な検証トークン (32 hex)。SystemTime 由来のエントロピー + rand。
fn rand_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let a: u64 = rng.gen();
    let b: u64 = rng.gen();
    format!("{:016x}{:016x}", a, b)
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
    // must-fix 3: メール必須。誰の残高か紐づけ、submit で一致を要求するため quote 時に確定。
    // TODO(所有確認): 本番ではマジックリンク等でメール所有を検証してから付与すること
    //   (現状は形式チェックのみ = なりすまし可能。MVP として明示)。
    let email = b.email.trim().to_lowercase();
    if !valid_email(&email) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"メールアドレスを入力してください"}))).into_response();
    }
    // 1) 画像を保存 (実装者: 既存の store_mockup_bytes 等で R2/volume に。生base64はDBに入れない)。
    //    ここでは image_ref はプレースホルダ。
    let image_ref = format!("consign/{}", rand_id(""));

    // 2) 自動概算 (参考値)。現状は画像を見ず、カテゴリ+メモから決定論で算出する。
    //    Gemini 画像査定は未接続 (gemini.rs に画像→テキスト査定関数が無く、
    //    既存 call_gemini_with_image は画像 URL を受け画像を返す = 査定用途に不適)。
    //    景表法配慮: 「AI査定」と謳わず「自動概算(参考値)」と表示し、表示と実装を一致させる。
    let (estimate, reason) = auto_estimate(&b.category, &b.memo);

    // 3) quote を一時保存 (submit が引く)。
    let qid = rand_id("q_");
    let now = now_s();
    {
        let c = db.lock().unwrap();
        let _ = c.execute(
            "INSERT INTO consign_quotes (id,email,category,memo,image_ref,estimate_jpy,reason,created_at,consumed_at)
             VALUES (?,?,?,?,?,?,?,?,NULL)",
            params![qid, email, b.category, b.memo, image_ref, estimate, reason, now],
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

/// 自動概算 (参考値)。カテゴリ+メモから決定論で算出。画像は見ない。
/// 戻り: (estimate_jpy, reason)。reason は景表法・UX両面で必須。
/// NOTE: 実画像査定 (Gemini) を入れる場合は表示コピーも「AI査定」に戻すこと
///       (表示と実装の一致を必ず維持する = 景表法)。
fn auto_estimate(category: &str, memo: &str) -> (i64, String) {
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
        "カテゴリ「{}」の中古相場とご記入内容（{}）から自動算出した概算（参考値）です。画像は確認していません。状態確認後に最終額が決まります。",
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
    // must-fix 3: メール必須 + quote のメールと一致を要求 (他人のメールへの付与を防ぐ)。
    // 所有確認: マジックリンク (consign_email_verifications.verified_at) で
    // メール所有を検証済みの場合のみ、残高付与 (sell) を許可する。検証は
    // POST /api/consign/verify/request → メール内リンク GET /consign/verify で完了。
    let email = b.email.trim().to_lowercase();
    if !valid_email(&email) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"メールアドレスを入力してください"}))).into_response();
    }
    let c = db.lock().unwrap();
    // 付与の前に失効処理を走らせ、期限切れ残高が温存されないようにする (不変条件 #1)。
    consign_balance_lazy_expire(&c, &email);

    // quote を引く (submit は再計算しない。改ざん防止のためサーバ保存値を信頼)。
    // must-fix 3: quote 作成時の email と submit の email が一致しないと拒否。
    let q: Option<(String, String, String, i64, String, String, Option<i64>)> = c.query_row(
        "SELECT category,memo,image_ref,estimate_jpy,reason,email,consumed_at FROM consign_quotes WHERE id=?",
        params![b.quote_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
    ).ok();
    let (category, memo, image_ref, estimate, reason, quote_email, consumed_at) = match q {
        Some(x) => x,
        None => return Json(json!({"error":"査定が見つかりません。もう一度査定してください"})).into_response(),
    };
    // must-fix 2: 既に消費済みの quote は二度と使えない (残高の無限 mint を防ぐ)。
    if consumed_at.is_some() {
        return Json(json!({"error":"この査定は既に使用済みです。もう一度査定してください"})).into_response();
    }
    // must-fix 3: メール不一致は拒否。
    if quote_email.trim().to_lowercase() != email {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"査定時とメールアドレスが一致しません"}))).into_response();
    }

    // 所有確認: 残高付与に到達しうる sell は、検証済みメールに限定する。
    // 未検証なら 403 を返し、quote は消費しない (検証後にそのまま再提出できる)。
    // consign/store は残高を即時付与しないため検証不要 (sell が唯一の mint 経路)。
    if intent == "sell" && !email_verified(&c, &email) {
        return (StatusCode::FORBIDDEN, Json(json!({
            "error":"メール確認が必要です",
            "need_verify": true,
            "message":"買取で残高を受け取るには、メールアドレスの確認が必要です。確認メールのリンクを押してから、もう一度「いますぐ買取」を選んでください。",
            "verify_endpoint":"/api/consign/verify/request"
        }))).into_response();
    }

    let now = now_s();
    let item_id = rand_id("i_");

    // must-fix 2: quote を原子的に消費済みに。consumed_at IS NULL 条件付き UPDATE が
    // 1 行だけ成功する = この submit だけが付与を行える。0 行なら別 submit が先行した。
    let consumed = c.execute(
        "UPDATE consign_quotes SET consumed_at=? WHERE id=? AND consumed_at IS NULL",
        params![now, b.quote_id],
    ).unwrap_or(0);
    if consumed != 1 {
        return Json(json!({"error":"この査定は既に使用済みです。もう一度査定してください"})).into_response();
    }

    // 不変条件 #2: 売り系は partner gate を通る場合のみ受理。閉じていれば store にダウングレード。
    let gate_open = partner_gate_open(&c);
    let is_sell_side = matches!(intent, "sell" | "consign");

    if is_sell_side && !gate_open {
        // gated: お預かりとして受け付け、準備中を返す (買取/委託は受理しない)。
        let _ = c.execute(
            "INSERT INTO consign_items (id,email,category,memo,image_ref,estimate_jpy,reason,status,created_at)
             VALUES (?,?,?,?,?,?,?,'stored',?)",
            params![item_id, email, category, memo, image_ref, estimate, reason, now],
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
                params![item_id, email, category, memo, image_ref, estimate, reason, now],
            );
            let _ = c.execute(
                "INSERT INTO consign_intents (item_id,intent,partner_id,gated,amount_jpy,created_at)
                 VALUES (?,?,?,0,?,?)",
                params![item_id, intent, pid, credited, now],
            );
            let expires = now + BALANCE_TTL_SECS;
            // 失効監査用ロットを記録 (expires_at が 6ヶ月失効の真実)。
            let _ = c.execute(
                "INSERT INTO consign_balance_ledger (email,item_id,amount_jpy,spent_jpy,source,granted_at,expires_at,swept_at)
                 VALUES (?,?,?,0,'sell',?,?,NULL)",
                params![email, item_id, credited, now, expires],
            );
            // must-fix 1: 同額を mu_credits に積む = MU 内購入 (MU PAY redeem / collab 等)
            // で実際に減る本物の残高。これが無いと特商法ページの「MU内で使える残高」が嘘になる。
            // 失効は consign_balance_lazy_expire() が expires_at 経過時に未使用分を回収する。
            if credited > 0 {
                let _ = crate::mu_credit_apply(
                    &c, &email, credited, "consign_sell", Some(&item_id));
            }
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
                params![item_id, email, category, memo, image_ref, estimate, reason, now],
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
                params![item_id, email, category, memo, image_ref, estimate, reason, now],
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

// ── POST /api/consign/verify/request ──────────────────────────────────
// メール所有確認の起点。email を受け、推測不能 token を発行 (DB保存) し、
// /consign/verify?token=... へのマジックリンクをメール送信する。
// 既存の Resend (RESEND_API_KEY) を使う。鍵未設定なら 503 (黙って通さない)。
#[derive(serde::Deserialize)]
pub(crate) struct VerifyReqBody {
    #[serde(default)] email: String,
}

pub(crate) async fn api_verify_request(State(db): State<Db>, Json(b): Json<VerifyReqBody>) -> Response {
    let email = b.email.trim().to_lowercase();
    if !valid_email(&email) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"メールアドレスを入力してください"}))).into_response();
    }
    let now = now_s();
    let token = rand_token();
    {
        let c = db.lock().unwrap();
        // レート制限: 1メール 5/h + 全体 300/h (inbox爆撃・Resendコスト/レピュ防御)。
        // 既存の rate_limit_hit_ok (blog_rate_limit) を流用。
        if !crate::rate_limit_hit_ok(&c, &format!("consignverify:{}", email), 5) {
            return (StatusCode::TOO_MANY_REQUESTS, Json(json!({
                "error":"確認メールの送信が多すぎます。1時間ほどおいて再度お試しください"
            }))).into_response();
        }
        if !crate::rate_limit_hit_ok(&c, "consignverify:_global", 300) {
            return (StatusCode::TOO_MANY_REQUESTS, Json(json!({"error":"混み合っています。少し待って再度お試しください"}))).into_response();
        }
        // 既に検証済みなら再送せず ok を返す (冪等)。未検証 or 別tokenなら upsert。
        let already = email_verified(&c, &email);
        if already {
            return Json(json!({"ok": true, "verified": true,
                "message":"このメールアドレスは確認済みです。そのまま出品できます。"})).into_response();
        }
        // 新しい token を保存 (毎回ローテ = 古いリンクは無効化)。verified_at はクリア。
        let _ = c.execute(
            "INSERT INTO consign_email_verifications (email, token, created_at, verified_at)
             VALUES (?,?,?,NULL)
             ON CONFLICT(email) DO UPDATE SET token=excluded.token, created_at=excluded.created_at, verified_at=NULL",
            params![email, token, now],
        );
    }
    // メール送信 (既存 Resend パターンを流用)。鍵未設定なら 503。
    let key = std::env::var("RESEND_API_KEY").unwrap_or_default();
    if key.is_empty() {
        // TODO: 本番では RESEND_API_KEY を必ず設定する。鍵が無い間は検証を完了できない。
        eprintln!("[consign/verify] RESEND_API_KEY 未設定 — token は保存したがメール未送信 (email={})", email);
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({
            "error":"メール送信が設定されていません。運営にお問い合わせください"
        }))).into_response();
    }
    let base = std::env::var("MU_BASE").unwrap_or_else(|_| "https://wearmu.com".into());
    let verify_url = format!("{}/consign/verify?token={}",
        base.trim_end_matches('/'), urlencoding::encode(&token));
    let body_html = format!(r#"<div style="background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,'Helvetica Neue',Arial,sans-serif;padding:32px 0;margin:0">
<div style="max-width:540px;margin:0 auto;padding:0 32px">
<div style="font-size:22px;font-weight:700;letter-spacing:0.45em;margin-bottom:22px">━◯━ MU</div>
<div style="font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:#ffd700;opacity:0.85;margin-bottom:8px">出品代行 メール確認</div>
<h2 style="font-size:19px;font-weight:600;line-height:1.5;margin:0 0 14px">このメールアドレスの確認をお願いします</h2>
<p style="font-size:13px;line-height:1.9;opacity:0.78;margin:0 0 22px">下のボタンを押すと確認が完了し、買取の残高をこのアドレスで受け取れるようになります。心当たりがない場合はこのメールを無視してください。</p>
<div style="text-align:center;margin:26px 0">
<a href="{url}" style="display:inline-block;background:#ffd700;color:#0a0a0a;text-decoration:none;font-weight:700;font-size:15px;padding:14px 30px;border-radius:99px">メールアドレスを確認する →</a></div>
<p style="font-size:11px;color:#666;word-break:break-all;margin-top:18px">{url}</p>
<p style="font-size:11px;line-height:1.85;opacity:0.55;margin:24px 0 0;border-top:1px solid #222;padding-top:18px">MU · wearmu.com · 株式会社イネブラ<br>お問い合わせ: <a href="mailto:info@enablerdao.com" style="color:#ffd700">info@enablerdao.com</a></p>
</div></div>"#, url = verify_url);
    let payload = json!({
        "from": "━◯━ MU 出品代行 <info@enablerdao.com>",
        "to": [email],
        "subject": "MU 出品代行 — メールアドレスの確認",
        "html": body_html,
    });
    match reqwest::Client::new()
        .post("https://api.resend.com/emails")
        .bearer_auth(&key)
        .json(&payload)
        .send().await
    {
        Ok(r) if r.status().is_success() => {
            Json(json!({"ok": true, "verified": false,
                "message":"確認メールを送りました。メール内のリンクを押すと出品できます。"})).into_response()
        }
        Ok(r) => {
            let st = r.status();
            let txt = r.text().await.unwrap_or_default();
            eprintln!("[consign/verify] resend {} email={} → {}", st, email, txt.chars().take(300).collect::<String>());
            (StatusCode::BAD_GATEWAY, Json(json!({"error":"確認メールの送信に失敗しました。少し待って再度お試しください"}))).into_response()
        }
        Err(e) => {
            eprintln!("[consign/verify] resend http error email={}: {}", email, e);
            (StatusCode::BAD_GATEWAY, Json(json!({"error":"確認メールの送信に失敗しました。少し待って再度お試しください"}))).into_response()
        }
    }
}

// ── GET /consign/verify?token=... ─────────────────────────────────────
// マジックリンクの着地点。token を引き、当該行の verified_at を立てる。
// 成功で「確認できました」、不正/期限切れで案内を返す (HTMLページ)。
#[derive(serde::Deserialize)]
pub(crate) struct VerifyQuery {
    #[serde(default)] token: String,
}

pub(crate) async fn verify_page(State(db): State<Db>, Query(q): Query<VerifyQuery>) -> Response {
    let token = q.token.trim();
    let now = now_s();
    let ok_email: Option<String> = if token.len() >= 16 {
        let c = db.lock().unwrap();
        // token から email を引く (verified_at 未設定でも済でも対象 = 冪等)。
        let email: Option<String> = c.query_row(
            "SELECT email FROM consign_email_verifications WHERE token=?",
            params![token], |r| r.get::<_, String>(0),
        ).ok();
        if email.is_some() {
            let _ = c.execute(
                "UPDATE consign_email_verifications SET verified_at=COALESCE(verified_at,?) WHERE token=?",
                params![now, token],
            );
        }
        email
    } else { None };

    let (title, msg, color) = match ok_email {
        Some(e) => (
            "確認できました ✓",
            format!("<b>{}</b> のメールアドレスを確認しました。<br>出品ページに戻って「いますぐ買取」を選ぶと、このアドレスの残高に反映されます。", esc(&e)),
            "#9fdf9f",
        ),
        None => (
            "リンクが無効です",
            "このリンクは無効か、期限切れです。出品ページからもう一度「確認メールを送る」をお試しください。".to_string(),
            "#e89a9a",
        ),
    };
    let html = format!(r##"<!doctype html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} — MU 出品代行</title>
<style>body{{margin:0;background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,'Helvetica Neue',Arial,sans-serif;display:flex;min-height:100vh;align-items:center;justify-content:center;padding:24px}}
.card{{max-width:480px;text-align:center}}
.brand{{font-size:20px;font-weight:700;letter-spacing:0.42em;margin-bottom:26px}}
h1{{font-size:22px;font-weight:600;margin:0 0 14px;color:{color}}}
p{{font-size:14px;line-height:1.9;opacity:0.82;margin:0 0 26px}}
a.cta{{display:inline-block;background:#ffd700;color:#0a0a0a;text-decoration:none;font-weight:700;font-size:15px;padding:13px 28px;border-radius:99px}}</style></head>
<body><div class="card"><div class="brand">━◯━ MU</div><h1>{title}</h1><p>{msg}</p>
<a class="cta" href="/consign">出品ページに戻る →</a></div></body></html>"##,
        title = esc(title), color = color, msg = msg);
    Html(html).into_response()
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
    let email = q.email.trim().to_lowercase();
    if email.is_empty() {
        // 未照会: メール入力フォームのみ。
        let html = BALANCE_HTML
            .replace("__LOOKUP_DISPLAY__", "block")
            .replace("__BALANCE_BLOCK__", "");
        return Html(html).into_response();
    }
    let now = now_s();
    let c = db.lock().unwrap();
    // 失効処理を先に走らせる (期限切れロットを mu_credits から回収)。表示を真実に。
    consign_balance_lazy_expire(&c, &email);
    // 実際に使える残高 = mu_credits の残高 (= 購入時に減る本物の値)。
    let spendable = crate::mu_credit_balance(&c, &email);
    let mut stmt = c.prepare(
        "SELECT amount_jpy, spent_jpy, source, granted_at, expires_at
         FROM consign_balance_ledger WHERE LOWER(email)=? ORDER BY expires_at ASC"
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
    // 「利用できる残高」= mu_credits の実残高 (= 購入時に実際に減る本物の値)。
    // 出品代行で付与した分はこの中に含まれ、MU 内購入 (MU PAY redeem 等) で使える。
    let _ = total_live; // 内訳は下のロット明細で表示 (失効分は回収済)。
    let block = format!(
        r##"<div class="bigbox"><div class="lbl">利用できる残高（MU内購入で使えます）</div><div class="amt">{total}</div><a class="use" href="/shop">MUで使う →</a></div><h2>出品代行ぶんの内訳（有効期限つき）</h2>{lots}"##,
        total = yen(spendable), lots = lots
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
