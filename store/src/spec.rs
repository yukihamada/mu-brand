//! 製造オーケストレーション層 (B): 仕様生成エンジン（言う→完璧な仕様）。
//!
//! 自然文リクエスト（「弟子屈ロゴの黒Tを30枚」）を構造化 `ManufacturingSpec` に
//! 落とし、**不足している必須属性を逆質問で埋める**。bim.house が「言葉が、建つ」で
//! 設計を引き出すのと同じ思想で、MU は「言えば、作れる仕様になる」を担う。
//!
//! ## 必須属性の判定は manufacturing_req に第一委譲する（A 依存）
//! 「その kind を発注するのに最低限要る属性キー」は製造要件KB（A）の
//! `crate::manufacturing_req::kind_required_attrs` が決定論で返す。本モジュールは
//! それを第一に呼び、未知 kind（floor 無し）の場合のみ素朴フォールバックで最小限の
//! 不足判定を行う（A 未配線でも単体で意味のある挙動を保つ）。
//!
//! ## 流儀・ゲート（CATALOG_CONTRACT / manufacturing_schema 準拠）
//! - 新テーブルは作らない。ドラフトは `manufacturing_specs`（schema 確定済）。本モジュールに
//!   CREATE TABLE は無い。
//! - **Gemini 課金は呼び出し側ガード前提**。`draft_spec` は冒頭で必ず `spend_or_refuse` を
//!   通し、予算超過なら Gemini を一切呼ばずに `Err("budget")` で止める。
//! - `updated_at` は自動更新トリガが無いので UPSERT 側で必ず `datetime('now')` を手書きする。
//! - `spec_id` は **prompt 由来の決定論ハッシュ**で生成する（乱数/時刻を使わない＝再実行安全）。
//!   同一 prompt は同一 spec_id に正規化され、UPSERT（spec_id UNIQUE）で冪等になる。
//! - kind 推論は `crate::catalog::infer_kind`（T-SHARED で pub(crate) 化前提）。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ─────────────────────────────────────────────────────────────────────────
//  構造化仕様型
// ─────────────────────────────────────────────────────────────────────────

/// 製造仕様の構造化表現。全フィールド Option（=「まだ埋まっていない」を表す）。
/// `extra` は kind 固有の追加属性（embroidery_spec / size_range / gauge 等）を
/// 自由に格納する逃し弁。`build_spec_response` / 不足判定はトップレベル + `extra` の
/// 両方を spec(JSON) に平坦化して扱う。
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct ManufacturingSpec {
    pub kind: Option<String>,
    pub material: Option<String>,
    pub dimensions: Option<String>,
    pub colors: Option<String>,
    pub print_method: Option<String>,
    pub placement: Option<String>,
    pub qty: Option<i64>,
    pub region: Option<String>,
    #[serde(default)]
    pub extra: Value,
}

impl ManufacturingSpec {
    /// トップレベルの確定属性キー → spec(JSON) に平坦化したオブジェクトを作る。
    /// `extra` がオブジェクトならそのキーもマージ（トップレベルが優先）。
    /// 不足判定（`kind_required_attrs` の各キーが埋まっているか）はこの平坦化 JSON で行う。
    fn to_flat_value(&self) -> Value {
        let mut map = serde_json::Map::new();

        // extra を先に展開し、その上にトップレベルの確定値をかぶせる（トップレベル優先）。
        if let Value::Object(obj) = &self.extra {
            for (k, v) in obj {
                map.insert(k.clone(), v.clone());
            }
        }

        let mut put_str = |k: &str, v: &Option<String>| {
            if let Some(s) = v {
                if !s.trim().is_empty() {
                    map.insert(k.to_string(), Value::String(s.clone()));
                }
            }
        };
        put_str("kind", &self.kind);
        put_str("material", &self.material);
        put_str("dimensions", &self.dimensions);
        put_str("colors", &self.colors);
        put_str("print_method", &self.print_method);
        put_str("placement", &self.placement);
        put_str("region", &self.region);
        if let Some(q) = self.qty {
            map.insert("qty".to_string(), Value::from(q));
        }

        Value::Object(map)
    }

    /// この kind の必須属性のうち、まだ埋まっていないキー一覧を返す。
    /// 第一に `manufacturing_req::kind_required_attrs(kind)` に委譲し、未知 kind で
    /// floor が空のときのみ素朴フォールバック（POD 系の最小集合）を使う。
    fn missing_attrs(&self) -> Vec<String> {
        let kind = self.kind.as_deref().unwrap_or("");
        let flat = self.to_flat_value();

        // A（製造要件KB）に第一委譲。
        let mut required: Vec<&'static str> = crate::manufacturing_req::kind_required_attrs(kind);

        // A が floor を持たない（未知 kind / 未配線）場合の素朴フォールバック。
        // 「何か作る」のに最低限要る属性のみ。kind 自体が未定なら kind を最初に要求する。
        if required.is_empty() {
            required = fallback_required_attrs(kind);
        }

        let mut missing: Vec<String> = Vec::new();
        for attr in required {
            if !flat_has_attr(&flat, attr) {
                missing.push(attr.to_string());
            }
        }
        missing
    }
}

/// A が floor を返さないときの素朴フォールバック必須集合。
/// kind 未定なら「kind」を先頭に要求（まず何を作るかを聞く）。
/// kind 既定だが floor 未登録なら、最低限 material を要求する保守的既定。
fn fallback_required_attrs(kind: &str) -> Vec<&'static str> {
    if kind.trim().is_empty() {
        vec!["kind", "material"]
    } else {
        vec!["material"]
    }
}

/// 平坦化 spec(JSON) がトップレベルに非空の属性 `attr` を持つか。
/// manufacturing_req::spec_has_attr と同じ判定基準（null / 空文字 / 空配列 / 空オブジェクトは「無い」）。
fn flat_has_attr(flat: &Value, attr: &str) -> bool {
    match flat.get(attr) {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => !s.trim().is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(_) => true,
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  spec_id: prompt 由来の決定論ハッシュ（乱数/時刻不使用＝resume 安全）
// ─────────────────────────────────────────────────────────────────────────

/// prompt から決定論の spec_id を生成する。同一 prompt → 同一 spec_id。
/// 形式: `SPEC-<16桁hex>`。`DefaultHasher` は環境内で安定（乱数 seed を使わない）ため、
/// 同一プロセス系での冪等性に十分（UPSERT の UNIQUE 衝突キーとして機能）。
pub fn spec_id_for(prompt: &str) -> String {
    let mut h = DefaultHasher::new();
    prompt.trim().hash(&mut h);
    format!("SPEC-{:016x}", h.finish())
}

// ─────────────────────────────────────────────────────────────────────────
//  build_spec_response: 同期・テスト可能な応答合成器
// ─────────────────────────────────────────────────────────────────────────

/// spec と不足キーから、API 応答の中核 JSON（spec / missing / next_question / status）を合成する。
/// ネット非依存・決定論。`draft_spec` はこの合成器を使って最終応答を組む。
pub fn build_spec_response(spec: &ManufacturingSpec, missing: &[&str]) -> Value {
    let status = if missing.is_empty() { "complete" } else { "draft" };
    let next_question = next_question_for(spec, missing);
    json!({
        "spec": spec,
        "missing": missing,
        "next_question": next_question,
        "status": status,
    })
}

/// 不足キーの先頭に対する日本語の逆質問を作る。不足が無ければ null。
fn next_question_for(spec: &ManufacturingSpec, missing: &[&str]) -> Value {
    let head = match missing.first() {
        Some(k) => *k,
        None => return Value::Null,
    };
    let kind_label = spec.kind.as_deref().unwrap_or("");
    let q = question_text(head, kind_label);
    Value::String(q)
}

/// 属性キー → 日本語逆質問文。未知キーは汎用文。
fn question_text(attr: &str, kind: &str) -> String {
    let suffix = if kind.is_empty() {
        String::new()
    } else {
        format!("（{kind}）")
    };
    let body = match attr {
        "kind" => "何を作りますか？（例: Tシャツ / パーカー / 道着 / トート / ステッカー）".to_string(),
        "material" => "素材は何にしますか？（例: コットン100% / ポリエステル / パール綿）".to_string(),
        "dimensions" => "サイズ・寸法を教えてください（例: トート 38×42cm / マグ 350ml）".to_string(),
        "colors" => "色は何色にしますか？（例: 黒 / 白 / ネイビー）".to_string(),
        "print_method" => "プリント方法はどうしますか？（例: DTG / 刺繍 / 昇華転写）".to_string(),
        "placement" => "デザインの配置はどこですか？（例: 前面中央 / 背面 / 左胸）".to_string(),
        "size_range" => "展開サイズを教えてください（例: S〜XL / A0〜A5）".to_string(),
        "embroidery_spec" => "刺繍の指定を教えてください（位置・色・ロゴデータの有無）".to_string(),
        "construction" => "縫製仕様を教えてください（例: 吊り編み / カットソー / 起毛）".to_string(),
        "gauge" => "編みゲージ・使用糸を教えてください（例: 12G / メリノウール）".to_string(),
        "model" => "対応機種を教えてください（例: iPhone 15 Pro）".to_string(),
        "qty" => "数量は何枚（何個）にしますか？".to_string(),
        "region" => "販売・配送の地域はどこですか？（例: 日本 / グローバル）".to_string(),
        _ => format!("`{attr}` を教えてください。"),
    };
    format!("{body}{suffix}")
}

// ─────────────────────────────────────────────────────────────────────────
//  persist_spec: manufacturing_specs への UPSERT（spec_id UNIQUE）
// ─────────────────────────────────────────────────────────────────────────

/// 仕様ドラフトを `manufacturing_specs` に UPSERT する（spec_id UNIQUE で冪等）。
/// - `status` は missing 空なら 'complete'、不足ありなら 'draft'。
/// - `updated_at=datetime('now')` を手書き（自動更新トリガ無し）。
/// - 既存 spec_id があれば prompt / kind / spec_json / missing_json / status / email を更新。
pub fn persist_spec(
    conn: &rusqlite::Connection,
    spec_id: &str,
    prompt: &str,
    spec: &ManufacturingSpec,
    missing: &[String],
    email: Option<&str>,
) {
    let spec_json = serde_json::to_string(spec).unwrap_or_else(|_| "{}".to_string());
    let missing_json = serde_json::to_string(missing).unwrap_or_else(|_| "[]".to_string());
    let status = if missing.is_empty() { "complete" } else { "draft" };
    let kind = spec.kind.as_deref();

    let _ = conn.execute(
        "INSERT INTO manufacturing_specs
            (spec_id, prompt, kind, spec_json, missing_json, status, email, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'), datetime('now'))
         ON CONFLICT(spec_id) DO UPDATE SET
            prompt       = excluded.prompt,
            kind         = excluded.kind,
            spec_json    = excluded.spec_json,
            missing_json = excluded.missing_json,
            status       = excluded.status,
            email        = excluded.email,
            updated_at   = datetime('now')",
        rusqlite::params![spec_id, prompt, kind, spec_json, missing_json, status, email],
    );
}

// ─────────────────────────────────────────────────────────────────────────
//  draft_spec: 自然文 → 構造化 spec（Gemini）＋ 逆質問 ＋ 永続化
// ─────────────────────────────────────────────────────────────────────────

/// 自然文 `prompt` を構造化仕様にし、不足を逆質問で埋め、`manufacturing_specs` に保存する。
///
/// 1. **予算ガード**: 先に `spend_or_refuse("spec_draft", 6, …)` を通し、false なら `Err("budget")`。
///    Gemini はガード通過後にのみ呼ぶ（無条件 call はしない）。
/// 2. **構造化**: `call_gemini_text` に「製造仕様 JSON に構造化（kind/material/…・不明は null）」を投げ、
///    JSON 部を緩くパース。失敗時は `infer_kind_from_text` のみで最小 spec にフォールバック。
/// 3. **kind 補完**: kind 未定なら `crate::catalog::infer_kind(prompt)`。
/// 4. **不足判定**: `kind_required_attrs(kind)`（A）に第一委譲し、未配線時は素朴フォールバック。
/// 5. **永続化**: 決定論 spec_id で UPSERT（冪等）。
///
/// 返り JSON: `{ ok, spec_id, spec, missing, next_question, status }`。
pub async fn draft_spec(
    db: &crate::Db,
    prompt: &str,
    email: Option<&str>,
) -> Result<Value, String> {
    // ── ① 予算ガード（Gemini を呼ぶ前に・課金は呼び出し側ガード前提）──
    // MutexGuard は await をまたがない（ガード判定だけで即 drop）。
    {
        let conn = db.lock().unwrap();
        if !crate::catalog::spend_or_refuse(&conn, "spec_draft", 6, "spec gen", None) {
            return Err("budget".to_string());
        }
    }

    // ── ② Gemini で構造化（ロック外・await）──
    let gemini_prompt = build_gemini_prompt(prompt);
    let raw = crate::gemini::call_gemini_text(&gemini_prompt).await;

    // ── ③ パース（失敗時は infer_kind のみの最小 spec へフォールバック）──
    let mut spec = match raw {
        Ok(text) => parse_spec_from_gemini(&text).unwrap_or_else(|| minimal_spec(prompt)),
        Err(_) => minimal_spec(prompt),
    };

    // ── ④ kind 補完/補正 ──
    // dog_gi / gi_patch は「犬の道着」「パッチ」等 description で一義に判別できるが、
    // Gemini は「道着→gi」のように一般化しがち。これらは infer_kind を優先して上書きする。
    // それ以外は kind 未定のときだけ infer で補完する。
    let inferred = crate::catalog::infer_kind(prompt);
    if matches!(inferred.as_str(), "dog_gi" | "gi_patch") {
        spec.kind = Some(inferred);
    } else if spec.kind.as_deref().map(|k| k.trim().is_empty()).unwrap_or(true)
        && !inferred.trim().is_empty()
    {
        spec.kind = Some(inferred);
    }

    // ── ⑤ 不足判定（A 委譲・フォールバック込み）──
    let missing: Vec<String> = spec.missing_attrs();

    // ── ⑥ 永続化（決定論 spec_id・冪等 UPSERT）──
    let spec_id = spec_id_for(prompt);
    {
        let conn = db.lock().unwrap();
        persist_spec(&conn, &spec_id, prompt, &spec, &missing, email);
    }

    // ── ⑦ 応答合成 ──
    let missing_refs: Vec<&str> = missing.iter().map(|s| s.as_str()).collect();
    let mut resp = build_spec_response(&spec, &missing_refs);
    if let Value::Object(map) = &mut resp {
        map.insert("ok".to_string(), Value::Bool(true));
        map.insert("spec_id".to_string(), Value::String(spec_id));
    }
    Ok(resp)
}

/// kind 推論のみで作る最小 spec（Gemini 失敗 / パース不能時のフォールバック）。
fn minimal_spec(prompt: &str) -> ManufacturingSpec {
    let inferred = crate::catalog::infer_kind(prompt);
    ManufacturingSpec {
        kind: (!inferred.trim().is_empty()).then_some(inferred),
        extra: Value::Object(serde_json::Map::new()),
        ..Default::default()
    }
}

/// Gemini に投げる構造化プロンプトを作る。
fn build_gemini_prompt(prompt: &str) -> String {
    format!(
        "あなたはアパレル/雑貨の製造仕様アシスタントです。次の依頼を製造仕様 JSON に構造化してください。\n\
         kind / material / dimensions / colors / print_method / placement / qty / region を抽出し、\n\
         不明な項目は null にしてください。前後の説明文なしで、次の形の JSON オブジェクトのみを出力:\n\
         {{\"kind\":null,\"material\":null,\"dimensions\":null,\"colors\":null,\
         \"print_method\":null,\"placement\":null,\"qty\":null,\"region\":null}}\n\
         （qty は整数。kind は tee/hoodie/crewneck/tank/tote/mug/sticker/cap/phone_case/gi/\
         loopwheel_sweat/seamless_knit/rashguard_ls 等の英小文字 1 語。）\n\n\
         依頼: {prompt}"
    )
}

/// Gemini の生応答から `ManufacturingSpec` を緩くパースする。
/// ```json フェンス除去 → 最初の `{`〜最後の `}` を抽出 → serde で読む。
/// null / 欠落は Option=None に落ちる。失敗時 None（呼び出し側がフォールバック）。
fn parse_spec_from_gemini(raw: &str) -> Option<ManufacturingSpec> {
    let cleaned = strip_json_fence(raw);
    let v: Value = serde_json::from_str(&cleaned).ok()?;
    let obj = v.as_object()?;

    let get_str = |k: &str| -> Option<String> {
        obj.get(k)
            .and_then(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s.to_lowercase() != "null")
    };
    // qty は number / 数字文字列の両方を許容。
    let qty: Option<i64> = obj.get("qty").and_then(|x| {
        x.as_i64()
            .or_else(|| x.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
    });

    // 既知キー以外（embroidery_spec / size_range / gauge 等）は extra に温存。
    let known = [
        "kind",
        "material",
        "dimensions",
        "colors",
        "print_method",
        "placement",
        "qty",
        "region",
    ];
    let mut extra = serde_json::Map::new();
    for (k, val) in obj {
        if known.contains(&k.as_str()) {
            continue;
        }
        if val.is_null() {
            continue;
        }
        extra.insert(k.clone(), val.clone());
    }

    Some(ManufacturingSpec {
        kind: get_str("kind").map(|s| s.to_lowercase()),
        material: get_str("material"),
        dimensions: get_str("dimensions"),
        colors: get_str("colors"),
        print_method: get_str("print_method"),
        placement: get_str("placement"),
        qty: qty.filter(|q| *q > 0),
        region: get_str("region").map(|s| s.to_lowercase()),
        extra: Value::Object(extra),
    })
}

/// ```json フェンスや前後ノイズを剥がして JSON オブジェクト本体を取り出す。
/// （manufacturing_req::strip_json_fence の object 版。`{`〜`}` を切り出す。）
fn strip_json_fence(s: &str) -> String {
    let t = s.trim();
    let t = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```"))
        .unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    if let (Some(a), Some(b)) = (t.find('{'), t.rfind('}')) {
        if b >= a {
            return t[a..=b].to_string();
        }
    }
    t.to_string()
}

// ─────────────────────────────────────────────────────────────────────────
//  テスト（決定論・ネット非依存部のみ）
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manufacturing_schema::ensure_manufacturing_schema;
    use rusqlite::Connection;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_manufacturing_schema(&conn);
        conn
    }

    #[test]
    fn spec_id_is_deterministic_for_same_prompt() {
        let a = spec_id_for("弟子屈ロゴの黒Tを30枚");
        let b = spec_id_for("  弟子屈ロゴの黒Tを30枚  "); // trim 後同一
        assert_eq!(a, b);
        assert!(a.starts_with("SPEC-"));
        // 異なる prompt は別 id。
        assert_ne!(a, spec_id_for("別の依頼"));
    }

    #[test]
    fn build_spec_response_complete_when_no_missing() {
        let spec = ManufacturingSpec {
            kind: Some("tote".to_string()),
            material: Some("コットン".to_string()),
            extra: Value::Object(serde_json::Map::new()),
            ..Default::default()
        };
        let resp = build_spec_response(&spec, &[]);
        assert_eq!(resp["status"], "complete");
        assert!(resp["next_question"].is_null());
        assert_eq!(resp["missing"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn build_spec_response_draft_with_next_question() {
        let spec = ManufacturingSpec {
            kind: Some("tote".to_string()),
            ..Default::default()
        };
        let resp = build_spec_response(&spec, &["dimensions"]);
        assert_eq!(resp["status"], "draft");
        // 先頭 missing に対する日本語逆質問が出る。
        let q = resp["next_question"].as_str().unwrap();
        assert!(q.contains("寸法") || q.contains("サイズ"), "q={}", q);
    }

    #[test]
    fn missing_attrs_uses_req_floor_for_known_kind() {
        // tote の floor は manufacturing_req 由来（material, dimensions）。
        let spec = ManufacturingSpec {
            kind: Some("tote".to_string()),
            material: Some("コットン".to_string()),
            // dimensions 欠落
            ..Default::default()
        };
        let missing = spec.missing_attrs();
        assert!(missing.contains(&"dimensions".to_string()), "missing={:?}", missing);
        assert!(!missing.contains(&"material".to_string()), "missing={:?}", missing);
    }

    #[test]
    fn missing_attrs_satisfied_known_kind_is_empty() {
        let spec = ManufacturingSpec {
            kind: Some("tote".to_string()),
            material: Some("コットン".to_string()),
            dimensions: Some("38x42cm".to_string()),
            ..Default::default()
        };
        assert!(spec.missing_attrs().is_empty(), "{:?}", spec.missing_attrs());
    }

    #[test]
    fn missing_attrs_fallback_when_kind_unknown_requires_kind() {
        // kind 未定 → フォールバックで kind を最優先で要求。
        let spec = ManufacturingSpec::default();
        let missing = spec.missing_attrs();
        assert_eq!(missing.first().map(|s| s.as_str()), Some("kind"), "missing={:?}", missing);
    }

    #[test]
    fn missing_attrs_reads_from_extra() {
        // floor の属性が extra に入っていても「埋まっている」と判定される。
        let mut extra = serde_json::Map::new();
        extra.insert("dimensions".to_string(), Value::String("38x42cm".to_string()));
        let spec = ManufacturingSpec {
            kind: Some("tote".to_string()),
            material: Some("コットン".to_string()),
            extra: Value::Object(extra),
            ..Default::default()
        };
        assert!(spec.missing_attrs().is_empty(), "{:?}", spec.missing_attrs());
    }

    #[test]
    fn persist_spec_is_idempotent_upsert() {
        let conn = mem();
        let spec = ManufacturingSpec {
            kind: Some("tote".to_string()),
            material: Some("コットン".to_string()),
            ..Default::default()
        };
        let sid = spec_id_for("toteを作る");
        persist_spec(&conn, &sid, "toteを作る", &spec, &["dimensions".to_string()], None);
        persist_spec(&conn, &sid, "toteを作る", &spec, &["dimensions".to_string()], None);
        let cnt: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM manufacturing_specs WHERE spec_id=?1",
                rusqlite::params![sid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cnt, 1, "spec_id UNIQUE で1行のまま");
        // missing ありなので status=draft。
        let status: String = conn
            .query_row(
                "SELECT status FROM manufacturing_specs WHERE spec_id=?1",
                rusqlite::params![sid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "draft");
    }

    #[test]
    fn persist_spec_complete_status_when_no_missing() {
        let conn = mem();
        let spec = ManufacturingSpec {
            kind: Some("tote".to_string()),
            material: Some("コットン".to_string()),
            dimensions: Some("38x42cm".to_string()),
            ..Default::default()
        };
        let sid = spec_id_for("tote完全版");
        persist_spec(&conn, &sid, "tote完全版", &spec, &[], None);
        let (status, kind): (String, Option<String>) = conn
            .query_row(
                "SELECT status, kind FROM manufacturing_specs WHERE spec_id=?1",
                rusqlite::params![sid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "complete");
        assert_eq!(kind.as_deref(), Some("tote"));
    }

    #[test]
    fn persist_then_update_changes_status_and_missing() {
        // 同 spec_id で「不足あり draft」→「不足なし complete」へ UPSERT 更新できる。
        let conn = mem();
        let sid = spec_id_for("段階的に埋める");
        let draft = ManufacturingSpec {
            kind: Some("tote".to_string()),
            ..Default::default()
        };
        persist_spec(&conn, &sid, "段階的に埋める", &draft, &draft.missing_attrs(), None);
        let complete = ManufacturingSpec {
            kind: Some("tote".to_string()),
            material: Some("コットン".to_string()),
            dimensions: Some("38x42cm".to_string()),
            ..Default::default()
        };
        persist_spec(&conn, &sid, "段階的に埋める", &complete, &[], None);
        let status: String = conn
            .query_row(
                "SELECT status FROM manufacturing_specs WHERE spec_id=?1",
                rusqlite::params![sid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "complete");
    }

    #[test]
    fn parse_spec_from_gemini_strips_fence_and_reads_fields() {
        let raw = "```json\n{\"kind\":\"TEE\",\"material\":\"コットン\",\"qty\":30,\
                   \"colors\":null,\"placement\":\"前面中央\",\"dimensions\":null,\
                   \"print_method\":null,\"region\":\"jp\",\"size_range\":\"S-XL\"}\n```";
        let spec = parse_spec_from_gemini(raw).expect("should parse");
        assert_eq!(spec.kind.as_deref(), Some("tee")); // 小文字化
        assert_eq!(spec.material.as_deref(), Some("コットン"));
        assert_eq!(spec.qty, Some(30));
        assert_eq!(spec.placement.as_deref(), Some("前面中央"));
        assert!(spec.colors.is_none()); // "null" は None
        // 未知キーは extra に温存。
        assert_eq!(spec.extra.get("size_range").and_then(|v| v.as_str()), Some("S-XL"));
    }

    #[test]
    fn parse_spec_from_gemini_handles_string_qty_and_garbage() {
        let raw = "前置き {\"kind\":\"tote\",\"qty\":\"12\"} 後置き";
        let spec = parse_spec_from_gemini(raw).expect("should parse object slice");
        assert_eq!(spec.qty, Some(12));
        // 完全に壊れた入力は None。
        assert!(parse_spec_from_gemini("not json at all").is_none());
    }

    #[test]
    fn next_question_unknown_kind_asks_kind_first() {
        let spec = ManufacturingSpec::default();
        let missing = spec.missing_attrs();
        let refs: Vec<&str> = missing.iter().map(|s| s.as_str()).collect();
        let resp = build_spec_response(&spec, &refs);
        let q = resp["next_question"].as_str().unwrap();
        assert!(q.contains("何を作りますか"), "q={}", q);
    }
}
