//! 製造要件KB（bim.house の houki 検査の「製造版」）。
//!
//! 「与えられた spec が、その kind / 地域 / 供給先の要件を満たすか」を **決定論で**
//! 返す層。bim.house が建築基準法をその場で判定するのと同じ思想で、MU は
//! 製品仕様 floor・法的要件・メーカー発注条件を3種の要件として持ち、出荷前に
//! 「足りない属性」「対応が要る法令」を機械可読に列挙する。
//!
//! ## データソースは2系統（マージ）
//!   1. **コード既定**（本ファイルの const 3表）— ビルドに焼かれた最低ライン。
//!      conn=None でも動くので決定論テストの単位になる。
//!   2. **DB**（`manufacturing_requirements`, active=1）— 日々アップデート（版上げ）
//!      される可変要件。`seed_requirements` で const を初期投入し、
//!      `upsert_requirement` / `refresh_requirements_via_gemini` で育てる。
//!
//! ## 流儀（CATALOG_CONTRACT / manufacturing_schema 準拠）
//! - 新テーブルは作らない（schema は `manufacturing_schema::ensure_manufacturing_schema`
//!   が確定済み）。本ファイルに CREATE TABLE は無い。
//! - UPDATE 時は `updated_at=datetime('now')` を手書きする（自動更新トリガ無し）。
//! - 対外送信は一切しない。Gemini 取り込みは既定 OFF（環境変数ゲート）＋予算ガード。

use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;

// ─────────────────────────────────────────────────────────────────────────
//  公開レポート型
// ─────────────────────────────────────────────────────────────────────────

/// `check_requirements` の決定論結果。
/// - `ok`   : required 違反がゼロ（= そのまま発注/RFQ 起票へ進める）。
/// - `gaps` : spec に足りない / 違反している点の人間可読文。
/// - `actions`: 法令該当など「人手対応が要る」アクション文。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RequirementReport {
    pub ok: bool,
    pub gaps: Vec<String>,
    pub actions: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────
//  (1) コード既定の3表（出典コメント付き）
// ─────────────────────────────────────────────────────────────────────────

/// (A) SPEC_FLOORS: kind 毎の「これが無いと発注できない」必須属性キー。
/// 出典: store/src/catalog.rs の PRODUCT_SPECS（POD 各 kind の入稿要件）＋
/// docs/gi-isami-2026-05-12（道着の刺繍指定）＋
/// docs/heritage-fulfillment-workflow.md（受注生産の素材/サイズ展開）＋
/// docs/seamless_knit/tech_pack.json（無縫製ニットのゲージ/糸）。
/// 値は spec(JSON) のトップレベルキー。保守的に「無いと作れない」最小集合のみ。
const SPEC_FLOORS: &[(&str, &[&str])] = &[
    // POD（Printful）: デザイン URL と配置が無いと印刷できない。
    ("tee", &["material", "placement", "size_range"]),
    ("hoodie", &["material", "placement", "size_range"]),
    ("crewneck", &["material", "placement", "size_range"]),
    ("tank", &["material", "placement", "size_range"]),
    ("rashguard_ls", &["material", "placement", "size_range"]),
    ("rashguard_black", &["material", "placement", "size_range"]),
    ("rashguard_premium", &["material", "placement", "size_range"]),
    ("tote", &["material", "dimensions"]),
    ("mug", &["material", "dimensions"]),
    ("sticker", &["material", "dimensions"]),
    ("cap", &["material", "embroidery_spec"]),
    ("phone_case", &["model", "placement"]),
    // 道着（ISAMI・刺繍17箇所・IBJJF 準拠）。
    ("gi", &["material", "size_range", "embroidery_spec"]),
    ("gi_embroidery", &["material", "size_range", "embroidery_spec"]),
    // 受注生産（Heritage ループウィール）。素材/サイズ展開/縫製仕様が要る。
    ("loopwheel_sweat", &["material", "size_range", "construction"]),
    ("cut_and_sew", &["material", "size_range", "construction"]),
    ("sweatshirt_mto", &["material", "size_range", "construction"]),
    // 無縫製ニット（島精機 WHOLEGARMENT）。ゲージ/糸/サイズ展開。
    ("seamless_knit", &["material", "gauge", "size_range"]),
    ("wholegarment", &["material", "gauge", "size_range"]),
];

/// (B) LEGAL_REQUIREMENTS: (kind 系統 | region) 毎の法令該当。
/// `kind_match` は kind の **前方一致グループ**（"apparel" は布製品全般）か個別 kind、
/// `region` は該当地域（"" = 全地域）。誤りは出荷リスクなので保守的（疑わしきは出す）。
/// 出典: 各法令の主管（消費者庁/経産省/総務省/厚労省）の一般公開情報。
/// `severity`: "required"（出荷前に必須）/ "recommended" / "info"。
struct LegalRule {
    /// "apparel" / "electronics" / "food" のグループ名、または個別 kind。
    group: &'static str,
    /// "" = 全地域、"jp" など。
    region: &'static str,
    key: &'static str,
    severity: &'static str,
    /// 人手対応の指示文（actions に出る）。
    action: &'static str,
    source_url: &'static str,
}

const LEGAL_REQUIREMENTS: &[LegalRule] = &[
    // 日本国内の布製品 → 家庭用品品質表示法（組成表示・取扱絵表示）。
    LegalRule {
        group: "apparel", region: "jp",
        key: "household_goods_quality_labeling",
        severity: "required",
        action: "家庭用品品質表示法: 繊維組成（混用率）と取扱い絵表示（JIS L0001）を製品/下げ札に表示すること。spec に fiber_composition と care_label を含める。",
        source_url: "https://www.caa.go.jp/policies/policy/representation/household_goods/",
    },
    // 日本国内アパレル一般 → 表示の根拠となる原産国（景表法・原産国表示）。
    LegalRule {
        group: "apparel", region: "jp",
        key: "country_of_origin_labeling",
        severity: "recommended",
        action: "原産国表示: 製造国を明示し、消費者の誤認を招く表示をしないこと（景品表示法）。",
        source_url: "https://www.caa.go.jp/policies/policy/representation/fair_labeling/",
    },
    // 電子機器（device 等）×日本 → 技適（電波法）と PSE（電気用品安全法）。
    LegalRule {
        group: "electronics", region: "jp",
        key: "giteki_radio_law",
        severity: "required",
        action: "技適（電波法）: 無線を内蔵する機器は工事設計認証（技適マーク）が必要。未取得のまま国内頒布しないこと。",
        source_url: "https://www.tele.soumu.go.jp/j/sys/equ/tech/",
    },
    LegalRule {
        group: "electronics", region: "jp",
        key: "pse_electrical_safety",
        severity: "required",
        action: "PSE（電気用品安全法）: 電源/充電する電気用品は PSE マーク（菱形/丸形）の対象か確認し、対象なら適合表示を付すこと。",
        source_url: "https://www.meti.go.jp/policy/consumer/seian/denan/",
    },
    // 飲食品系 → 食品衛生法。
    LegalRule {
        group: "food", region: "jp",
        key: "food_sanitation_act",
        severity: "required",
        action: "食品衛生法: 飲食物・添加物・器具/容器包装は食品衛生法の規格基準に適合し、必要な営業許可/届出を行うこと。",
        source_url: "https://www.mhlw.go.jp/stf/seisakunitsuite/bunya/kenkou_iryou/shokuhin/",
    },
];

/// kind → 法令グループ。前方一致でなく明示マップ（誤分類を避ける）。
/// 未知 kind は None（= 法令 floor 無し。DB 側で個別追加可）。
fn legal_group_for_kind(kind: &str) -> Option<&'static str> {
    match kind {
        // 布製品（POD / 道着 / 受注生産 / ニット）全般 → apparel。
        "tee" | "hoodie" | "crewneck" | "tank" | "tote" | "cap" | "beanie"
        | "rashguard_ls" | "rashguard_black" | "rashguard_premium"
        | "gi" | "gi_embroidery"
        | "loopwheel_sweat" | "cut_and_sew" | "sweatshirt_mto"
        | "seamless_knit" | "wholegarment" => Some("apparel"),
        "device" | "electronics" | "nfc_coin" => Some("electronics"),
        "food" | "beverage" | "snack" => Some("food"),
        _ => None,
    }
}

/// (C) SUPPLIER_ORDER_TERMS: supplier_id 毎の発注条件（MOQ/入稿形式/素材制約/リードタイム）。
/// SUPPLIER_REGISTRY（catalog.rs）と矛盾しない範囲の **補足**（入稿形式など、
/// レジストリに無い運用上の必須情報）。MOQ/lead は参考再掲（齟齬時は SUPPLIER_REGISTRY が正）。
/// 出典: 各供給先の docs（gi-isami / heritage-fulfillment-workflow / seamless_knit /
/// CONTRADO_SALES_OUTREACH）。
struct SupplierTerm {
    supplier_id: &'static str,
    /// 受理する入稿フォーマット（順序＝優先）。
    file_formats: &'static [&'static str],
    /// 素材制約・運用注意（自由文）。
    material_note: &'static str,
    moq: i64,
    lead_time_days: i64,
    source_url: &'static str,
}

const SUPPLIER_ORDER_TERMS: &[SupplierTerm] = &[
    SupplierTerm {
        supplier_id: "printful",
        file_formats: &["PNG"],
        material_note: "300DPI PNG・透過背景・配置(placement)必須。AOP は全パネルに同一 URL を展開。",
        moq: 1, lead_time_days: 10,
        source_url: "https://www.printful.com/",
    },
    SupplierTerm {
        supplier_id: "isami_gi",
        file_formats: &["PDF", "PNG"],
        material_note: "刺繍17箇所のベクター/位置指定・パール綿/コットン・試作1着必須・IBJJF 規格。",
        moq: 10, lead_time_days: 45,
        source_url: "docs/gi-isami-2026-05-12",
    },
    SupplierTerm {
        supplier_id: "heritage_loopwheel",
        file_formats: &["PDF", "PNG"],
        material_note: "和歌山ループウィール×弟子屈鉱物染×兵庫縫製・縫製仕様(construction)必須・30着限定。",
        moq: 15, lead_time_days: 90,
        source_url: "docs/heritage-fulfillment-workflow.md",
    },
    SupplierTerm {
        supplier_id: "shima_seamless",
        file_formats: &["DXF", "PDF"],
        material_note: "無縫製ニット・ゲージ(gauge)/糸指定必須・型代¥1-2M（複数ドロップで償却）・要見積。",
        moq: -1, lead_time_days: -1,
        source_url: "docs/seamless_knit/tech_pack.json",
    },
    SupplierTerm {
        supplier_id: "contrado_uk",
        file_formats: &["PNG", "PDF"],
        material_note: "縁まで全面サブリメーション・原価は Printful の2-3倍・プレミアム線(¥19,800+)。",
        moq: 1, lead_time_days: 14,
        source_url: "docs/CONTRADO_SALES_OUTREACH.md",
    },
];

// ─────────────────────────────────────────────────────────────────────────
//  (4) 公開: kind の必須属性キー
// ─────────────────────────────────────────────────────────────────────────

/// SPEC_FLOORS から kind の必須属性キーを返す（spec.rs が不足判定に第一呼びする IF）。
/// 未知 kind は空 Vec（= floor 無し＝任意属性で可）。
pub fn kind_required_attrs(kind: &str) -> Vec<&'static str> {
    SPEC_FLOORS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, attrs)| attrs.to_vec())
        .unwrap_or_default()
}

// ─────────────────────────────────────────────────────────────────────────
//  (2) seed: const → DB 初期投入（冪等）
// ─────────────────────────────────────────────────────────────────────────

/// const 3表を `manufacturing_requirements` に INSERT OR IGNORE で初期投入する。
/// UNIQUE(req_type,kind,region,supplier_id,key) で衝突＝無視するので何度呼んでも安全。
/// NULL を避け、空文字で正規化して UNIQUE/UPSERT を決定論にする（SQLite の NULL!=NULL 回避）。
pub fn seed_requirements(conn: &Connection) {
    // (A) spec_floor。kind 毎の必須属性キー集合を1行で持つ。
    for (kind, attrs) in SPEC_FLOORS {
        let value = serde_json::json!({ "required_attrs": attrs }).to_string();
        let _ = conn.execute(
            "INSERT OR IGNORE INTO manufacturing_requirements
                (req_type, kind, region, supplier_id, key, value_json, severity, source_url, version, active)
             VALUES ('spec_floor', ?, '', '', 'required_attrs', ?, 'required', ?, 1, 1)",
            rusqlite::params![
                kind,
                value,
                "store/src/catalog.rs#PRODUCT_SPECS"
            ],
        );
    }

    // (B) legal。(group|region|key) 毎に1行。
    for r in LEGAL_REQUIREMENTS {
        let value = serde_json::json!({
            "action": r.action,
            "group": r.group,
        })
        .to_string();
        let _ = conn.execute(
            "INSERT OR IGNORE INTO manufacturing_requirements
                (req_type, kind, region, supplier_id, key, value_json, severity, source_url, version, active)
             VALUES ('legal', ?, ?, '', ?, ?, ?, ?, 1, 1)",
            rusqlite::params![
                r.group, // legal は kind 列に group を入れる（legal_group_for_kind で解決）
                r.region,
                r.key,
                value,
                r.severity,
                r.source_url
            ],
        );
    }

    // (C) supplier_term。supplier_id 毎に1行。
    for t in SUPPLIER_ORDER_TERMS {
        let value = serde_json::json!({
            "file_formats": t.file_formats,
            "material_note": t.material_note,
            "moq": (t.moq >= 0).then_some(t.moq),
            "lead_time_days": (t.lead_time_days >= 0).then_some(t.lead_time_days),
        })
        .to_string();
        let _ = conn.execute(
            "INSERT OR IGNORE INTO manufacturing_requirements
                (req_type, kind, region, supplier_id, key, value_json, severity, source_url, version, active)
             VALUES ('supplier_term', '', '', ?, 'order_terms', ?, 'required', ?, 1, 1)",
            rusqlite::params![t.supplier_id, value, t.source_url],
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  (3) check_requirements: spec が要件を満たすか（決定論）
// ─────────────────────────────────────────────────────────────────────────

/// spec(JSON) が kind / region / supplier_id の要件を満たすか判定する。
/// - conn=Some → DB(active=1) の要件と const 既定をマージ（DB 優先で上書き）。
/// - conn=None → const 既定のみ（決定論テスト可能）。
/// 戻り `ok` は required 違反ゼロ。recommended/info は actions に出るが ok は落とさない。
pub fn check_requirements(
    conn: Option<&Connection>,
    kind: &str,
    region: Option<&str>,
    supplier_id: Option<&str>,
    spec: &Value,
) -> RequirementReport {
    let mut gaps: Vec<String> = Vec::new();
    let mut actions: Vec<String> = Vec::new();
    let mut ok = true;

    // ── (A) spec_floor: 必須属性の存在チェック ──
    // DB に kind 個別の required_attrs があればそれを、無ければ const SPEC_FLOORS を使う。
    let required_attrs: Vec<String> = db_required_attrs(conn, kind)
        .unwrap_or_else(|| kind_required_attrs(kind).iter().map(|s| s.to_string()).collect());

    for attr in &required_attrs {
        if !spec_has_attr(spec, attr) {
            ok = false;
            gaps.push(format!(
                "必須属性 `{}` が spec にありません（kind={}）。",
                attr, kind
            ));
        }
    }

    // ── (B) legal: kind 系統 × region の法令該当 ──
    // DB の legal 行（active=1）と const をマージ。region は「対象地域 or 全地域('')」。
    let region_lc = region.map(|r| r.to_lowercase());
    let group = legal_group_for_kind(kind);
    let legal_hits = collect_legal(conn, group, region_lc.as_deref());
    for hit in legal_hits {
        match hit.severity.as_str() {
            "required" => {
                ok = false;
                gaps.push(format!("法令要対応（必須）: {}", hit.key));
                actions.push(hit.action);
            }
            _ => {
                // recommended / info は ok を落とさず actions に助言として出す。
                actions.push(hit.action);
            }
        }
    }

    // ── (C) supplier_term: 供給先固有の入稿/素材制約（情報提供。ok は落とさない）──
    if let Some(sid) = supplier_id {
        if let Some(term_action) = supplier_term_action(conn, sid) {
            actions.push(term_action);
        }
    }

    // 重複アクションを除去（DB と const が同じ法令を返した場合）。
    actions.sort();
    actions.dedup();

    RequirementReport { ok, gaps, actions }
}

/// 見積段階（route_request・spec 未入力）向けの軽量チェック。
/// **spec_floor は見ない**（spec 未入力での floor 不足は『違反』でなく『未入力』なので、
/// 毎回 ok=false になる誤信号を避ける）。法令(legal)＋供給先条件(supplier_term)の助言のみ返す。
/// 返り `(compliance_ok, notices)`: compliance_ok=false は『必須法令の対応が要る』。
pub fn compliance_actions(
    conn: Option<&Connection>,
    kind: &str,
    region: Option<&str>,
    supplier_id: Option<&str>,
) -> (bool, Vec<String>) {
    let mut ok = true;
    let mut notices: Vec<String> = Vec::new();

    let region_lc = region.map(|r| r.to_lowercase());
    let group = legal_group_for_kind(kind);
    for hit in collect_legal(conn, group, region_lc.as_deref()) {
        if hit.severity == "required" {
            ok = false;
        }
        notices.push(hit.action);
    }
    if let Some(sid) = supplier_id {
        if let Some(term_action) = supplier_term_action(conn, sid) {
            notices.push(term_action);
        }
    }
    notices.sort();
    notices.dedup();
    (ok, notices)
}

/// spec(JSON) がトップレベルに非空の属性 `attr` を持つか。
/// 存在し、かつ null / 空文字 / 空配列 / 空オブジェクトでないこと。
fn spec_has_attr(spec: &Value, attr: &str) -> bool {
    match spec.get(attr) {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => !s.trim().is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(_) => true, // number / bool は値ありとみなす
    }
}

/// DB に kind 個別の spec_floor required_attrs があれば取り出す（active=1, region='', supplier_id='')。
fn db_required_attrs(conn: Option<&Connection>, kind: &str) -> Option<Vec<String>> {
    let conn = conn?;
    let row: Option<String> = conn
        .query_row(
            "SELECT value_json FROM manufacturing_requirements
             WHERE req_type='spec_floor' AND kind=? AND active=1
             ORDER BY version DESC LIMIT 1",
            rusqlite::params![kind],
            |r| r.get(0),
        )
        .ok();
    let json = row?;
    let v: Value = serde_json::from_str(&json).ok()?;
    let arr = v.get("required_attrs")?.as_array()?;
    let attrs: Vec<String> = arr
        .iter()
        .filter_map(|x| x.as_str().map(|s| s.to_string()))
        .collect();
    if attrs.is_empty() {
        None
    } else {
        Some(attrs)
    }
}

/// 1件の法令該当（マージ後）。
struct LegalHit {
    key: String,
    severity: String,
    action: String,
}

/// const + DB の legal 要件を group×region で集約。重複キーは DB 優先。
fn collect_legal(
    conn: Option<&Connection>,
    group: Option<&str>,
    region_lc: Option<&str>,
) -> Vec<LegalHit> {
    use std::collections::BTreeMap;
    let mut by_key: BTreeMap<String, LegalHit> = BTreeMap::new();

    let region_matches = |rule_region: &str| -> bool {
        // rule_region="" は全地域。指定 region が無ければ全地域ルールのみ。
        if rule_region.is_empty() {
            return true;
        }
        match region_lc {
            Some(r) => r == rule_region,
            None => false,
        }
    };

    // const 既定（group が解決できる時のみ）。
    if let Some(g) = group {
        for r in LEGAL_REQUIREMENTS {
            if r.group == g && region_matches(r.region) {
                by_key.insert(
                    r.key.to_string(),
                    LegalHit {
                        key: r.key.to_string(),
                        severity: r.severity.to_string(),
                        action: r.action.to_string(),
                    },
                );
            }
        }
    }

    // DB（active=1）。legal は kind 列に group を格納している。
    if let (Some(conn), Some(g)) = (conn, group) {
        let mut stmt = match conn.prepare(
            "SELECT key, severity, value_json, region FROM manufacturing_requirements
             WHERE req_type='legal' AND kind=? AND active=1",
        ) {
            Ok(s) => s,
            Err(_) => return by_key.into_values().collect(),
        };
        let rows = stmt.query_map(rusqlite::params![g], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        });
        if let Ok(rows) = rows {
            for r in rows.flatten() {
                let (key, severity, value_json, rule_region) = r;
                if !region_matches(&rule_region) {
                    continue;
                }
                let action = serde_json::from_str::<Value>(&value_json)
                    .ok()
                    .and_then(|v| v.get("action").and_then(|a| a.as_str()).map(|s| s.to_string()))
                    .unwrap_or_else(|| format!("法令要対応: {}", key));
                by_key.insert(key.clone(), LegalHit { key, severity, action });
            }
        }
    }

    by_key.into_values().collect()
}

/// supplier_term の入稿/素材制約を action 文として返す（DB 優先、無ければ const）。
fn supplier_term_action(conn: Option<&Connection>, supplier_id: &str) -> Option<String> {
    // DB 優先。
    if let Some(conn) = conn {
        let row: Option<String> = conn
            .query_row(
                "SELECT value_json FROM manufacturing_requirements
                 WHERE req_type='supplier_term' AND supplier_id=? AND active=1
                 ORDER BY version DESC LIMIT 1",
                rusqlite::params![supplier_id],
                |r| r.get(0),
            )
            .ok();
        if let Some(json) = row {
            if let Ok(v) = serde_json::from_str::<Value>(&json) {
                let formats = v
                    .get("file_formats")
                    .and_then(|f| f.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str())
                            .collect::<Vec<_>>()
                            .join("/")
                    })
                    .unwrap_or_default();
                let note = v.get("material_note").and_then(|x| x.as_str()).unwrap_or("");
                return Some(format!(
                    "供給先 {} の発注条件: 入稿形式={} / {}",
                    supplier_id, formats, note
                ));
            }
        }
    }
    // const fallback。
    SUPPLIER_ORDER_TERMS
        .iter()
        .find(|t| t.supplier_id == supplier_id)
        .map(|t| {
            format!(
                "供給先 {} の発注条件: 入稿形式={} / {}",
                supplier_id,
                t.file_formats.join("/"),
                t.material_note
            )
        })
}

// ─────────────────────────────────────────────────────────────────────────
//  (5) upsert_requirement: 版上げ UPSERT
// ─────────────────────────────────────────────────────────────────────────

/// 1件の要件を UNIQUE(req_type,kind,region,supplier_id,key) で UPSERT する。
/// 既存があれば value/severity/source を更新し version+1・updated_at=datetime('now')・active=1。
/// NULL を避け空文字で正規化（UNIQUE/ON CONFLICT が NULL で発火しない問題を回避）。
#[allow(clippy::too_many_arguments)]
pub fn upsert_requirement(
    conn: &Connection,
    req_type: &str,
    kind: Option<&str>,
    region: Option<&str>,
    supplier_id: Option<&str>,
    key: &str,
    value_json: &str,
    severity: &str,
    source_url: Option<&str>,
) {
    let kind = kind.unwrap_or("");
    let region = region.unwrap_or("");
    let supplier_id = supplier_id.unwrap_or("");
    let _ = conn.execute(
        "INSERT INTO manufacturing_requirements
            (req_type, kind, region, supplier_id, key, value_json, severity, source_url, version, active, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, 1, datetime('now'))
         ON CONFLICT(req_type, kind, region, supplier_id, key) DO UPDATE SET
            value_json = excluded.value_json,
            severity   = excluded.severity,
            source_url = excluded.source_url,
            version    = manufacturing_requirements.version + 1,
            active     = 1,
            updated_at = datetime('now')",
        rusqlite::params![
            req_type, kind, region, supplier_id, key, value_json, severity, source_url
        ],
    );
}

// ─────────────────────────────────────────────────────────────────────────
//  (6) Gemini 取り込み（既定 OFF・予算ガード前提）
// ─────────────────────────────────────────────────────────────────────────

/// kind の最新法令/メーカー要件を Gemini で構造化取得し upsert する。
/// **既定 OFF**（`MU_REQ_INGEST_ENABLED=1` でのみ稼働）＝¥0。
/// 呼ぶ前に予算ガード（spend_or_refuse）を通し、false なら 0 件で中止。
/// 戻り = upsert した件数。
pub async fn refresh_requirements_via_gemini(db: &crate::Db, kind: &str) -> Result<usize, String> {
    // ① 環境変数ゲート（既定 OFF）。
    if std::env::var("MU_REQ_INGEST_ENABLED").ok().as_deref() != Some("1") {
        return Ok(0);
    }
    // ② 予算ガード（短くロック→drop。await を跨いで MutexGuard を保持しない＝Send 維持）。
    const INGEST_COST_JPY: i64 = 6;
    {
        let guard = db.lock().unwrap();
        let conn: &Connection = &guard;
        let charged = crate::catalog::spend_or_refuse(
            conn,
            "requirements_ingest",
            INGEST_COST_JPY,
            &format!("requirements ingest kind={}", kind),
            Some(kind),
        );
        if !charged {
            return Err("budget refused (requirements_ingest)".to_string());
        }
    }

    // ③ 取得。JSON 配列で「要件」を返させる。
    let prompt = format!(
        "あなたは製造コンプライアンスの専門家です。アパレル/雑貨の製品種別 \"{kind}\" を\n\
         日本国内で製造・販売する際に必須/推奨の **法令・表示・メーカー発注上の要件** を、\n\
         次の JSON 配列形式のみで出力してください（前後の説明文なし）:\n\
         [{{\"req_type\":\"legal|spec_floor|supplier_term\",\"key\":\"識別子(英数_)\",\
         \"region\":\"jp|''\",\"severity\":\"required|recommended|info\",\
         \"action\":\"人手対応の指示文(日本語)\",\"source\":\"根拠URL or 法令名\"}}]\n\
         最大8件。確実なものだけ。"
    );
    let raw = crate::gemini::call_gemini_text(&prompt).await?;

    // ④ パース（```json フェンス除去 → 配列抽出）。
    let cleaned = strip_json_fence(&raw);
    let arr: Vec<Value> = serde_json::from_str(&cleaned)
        .map_err(|e| format!("gemini ingest parse error: {e}; raw={}", truncate(&raw, 280)))?;

    // ⑤ upsert（再ロック）。legal は kind 列に group(=本 kind の legal_group か kind 自体) を格納。
    let guard = db.lock().unwrap();
    let conn: &Connection = &guard;
    let group = legal_group_for_kind(kind).unwrap_or(kind);
    let mut n = 0usize;
    for item in arr {
        let req_type = item.get("req_type").and_then(|x| x.as_str()).unwrap_or("legal");
        if !matches!(req_type, "legal" | "spec_floor" | "supplier_term") {
            continue;
        }
        let key = match item.get("key").and_then(|x| x.as_str()) {
            Some(k) if !k.trim().is_empty() => k.trim(),
            _ => continue,
        };
        let region = item.get("region").and_then(|x| x.as_str()).unwrap_or("");
        let severity = match item.get("severity").and_then(|x| x.as_str()) {
            Some(s @ ("required" | "recommended" | "info")) => s,
            _ => "recommended",
        };
        let action = item.get("action").and_then(|x| x.as_str()).unwrap_or("");
        let source = item.get("source").and_then(|x| x.as_str());

        // kind 列: legal は group、それ以外は本 kind。supplier_term は kind 無し。
        let stored_kind = match req_type {
            "legal" => Some(group),
            "supplier_term" => None,
            _ => Some(kind),
        };
        let value = serde_json::json!({ "action": action, "ingested": true }).to_string();
        upsert_requirement(
            conn,
            req_type,
            stored_kind,
            Some(region),
            None,
            key,
            &value,
            severity,
            source,
        );
        n += 1;
    }
    Ok(n)
}

/// ```json フェンスや前後ノイズを剥がして配列本体を取り出す。
fn strip_json_fence(s: &str) -> String {
    let t = s.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    // 最初の '[' と最後の ']' で配列だけを切り出す（保険）。
    if let (Some(a), Some(b)) = (t.find('['), t.rfind(']')) {
        if b >= a {
            return t[a..=b].to_string();
        }
    }
    t.to_string()
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  (7) テスト（決定論・conn=None / in-memory）
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kind_required_attrs_known_and_unknown() {
        assert_eq!(kind_required_attrs("tote"), vec!["material", "dimensions"]);
        let gi = kind_required_attrs("gi");
        assert!(gi.contains(&"embroidery_spec"));
        assert!(gi.contains(&"size_range"));
        assert!(kind_required_attrs("totally_unknown_kind").is_empty());
    }

    #[test]
    fn check_none_satisfied_tote() {
        // tote の必須を満たす spec → ok。
        let spec = json!({
            "material": "オーガニックコットン",
            "dimensions": "38x42cm"
        });
        let rep = check_requirements(None, "tote", None, None, &spec);
        assert!(rep.ok, "gaps={:?}", rep.gaps);
        assert!(rep.gaps.is_empty());
    }

    #[test]
    fn check_none_missing_attr() {
        // dimensions 欠落 → ok=false・gap に dimensions。
        let spec = json!({ "material": "コットン" });
        let rep = check_requirements(None, "tote", None, None, &spec);
        assert!(!rep.ok);
        assert!(rep.gaps.iter().any(|g| g.contains("dimensions")));
    }

    #[test]
    fn check_none_empty_string_attr_counts_as_missing() {
        // 空文字は「無い」扱い。
        let spec = json!({ "material": "  ", "dimensions": "38x42cm" });
        let rep = check_requirements(None, "tote", None, None, &spec);
        assert!(!rep.ok);
        assert!(rep.gaps.iter().any(|g| g.contains("material")));
    }

    #[test]
    fn check_none_legal_apparel_jp_action() {
        // 布製品×jp は家庭用品品質表示法(required) → ok=false・action 出る。
        let spec = json!({
            "material": "コットン",
            "placement": "front",
            "size_range": "S-XL"
        });
        let rep = check_requirements(None, "tee", Some("jp"), None, &spec);
        assert!(!rep.ok, "required legal should fail ok; actions={:?}", rep.actions);
        assert!(rep
            .actions
            .iter()
            .any(|a| a.contains("家庭用品品質表示法")));
    }

    #[test]
    fn check_none_legal_not_triggered_without_region() {
        // region 未指定なら jp 限定法令は出ない（spec floor は満たす）。
        let spec = json!({
            "material": "コットン",
            "placement": "front",
            "size_range": "S-XL"
        });
        let rep = check_requirements(None, "tee", None, None, &spec);
        assert!(rep.ok, "gaps={:?} actions={:?}", rep.gaps, rep.actions);
    }

    #[test]
    fn check_none_electronics_jp_required() {
        // device×jp は技適/PSE(required) → ok=false。
        let spec = json!({ "model": "x" });
        let rep = check_requirements(None, "device", Some("jp"), None, &spec);
        assert!(!rep.ok);
        assert!(rep.actions.iter().any(|a| a.contains("技適")));
    }

    #[test]
    fn check_supplier_term_action_const() {
        // supplier_id を渡すと発注条件が action に出る（ok は落とさない）。
        let spec = json!({
            "material": "パール綿",
            "size_range": "A0-A5",
            "embroidery_spec": "17箇所"
        });
        let rep = check_requirements(None, "gi", None, Some("isami_gi"), &spec);
        assert!(rep.ok, "gaps={:?}", rep.gaps);
        assert!(rep.actions.iter().any(|a| a.contains("isami_gi")));
        assert!(rep.actions.iter().any(|a| a.contains("PDF")));
    }

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::manufacturing_schema::ensure_manufacturing_schema(&conn);
        conn
    }

    #[test]
    fn seed_is_idempotent() {
        let conn = mem();
        seed_requirements(&conn);
        let after_first: i64 = conn
            .query_row("SELECT COUNT(*) FROM manufacturing_requirements", [], |r| r.get(0))
            .unwrap();
        seed_requirements(&conn);
        let after_second: i64 = conn
            .query_row("SELECT COUNT(*) FROM manufacturing_requirements", [], |r| r.get(0))
            .unwrap();
        assert!(after_first > 0);
        assert_eq!(after_first, after_second, "seed must be idempotent");
    }

    #[test]
    fn check_with_db_matches_const() {
        // seed 後の DB 経路でも const と同じ結論になる（tote 満たす）。
        let conn = mem();
        seed_requirements(&conn);
        let spec = json!({ "material": "コットン", "dimensions": "38x42cm" });
        let rep = check_requirements(Some(&conn), "tote", None, None, &spec);
        assert!(rep.ok, "gaps={:?}", rep.gaps);
    }

    #[test]
    fn upsert_bumps_version_idempotently() {
        let conn = mem();
        upsert_requirement(
            &conn,
            "legal",
            Some("apparel"),
            Some("jp"),
            None,
            "test_rule",
            &json!({"action":"v1"}).to_string(),
            "required",
            Some("src"),
        );
        let v1: i64 = conn
            .query_row(
                "SELECT version FROM manufacturing_requirements WHERE key='test_rule'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v1, 1);
        upsert_requirement(
            &conn,
            "legal",
            Some("apparel"),
            Some("jp"),
            None,
            "test_rule",
            &json!({"action":"v2"}).to_string(),
            "recommended",
            Some("src2"),
        );
        let (v2, sev): (i64, String) = conn
            .query_row(
                "SELECT version, severity FROM manufacturing_requirements WHERE key='test_rule'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(v2, 2, "version must bump on conflict");
        assert_eq!(sev, "recommended", "severity must update");
        // 行は1本のまま（UNIQUE で増えない）。
        let cnt: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM manufacturing_requirements WHERE key='test_rule'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cnt, 1);
    }
}
