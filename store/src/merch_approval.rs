// merch_approval.rs — SOLUNA×ZAMNA DJコラボT の承認ページ（MU内・鍵URL）。
//
// 目的: ZAMNA / SOLUNA / 各DJ の担当者が、自分宛の商品をグループ別リストで見て
// 承認/却下できるようにする。承認の証跡は catalog_brands.config_json.approvals に
// 追記する（CATALOG_CONTRACT: 新テーブル・新カラム禁止 = config_json を使う）。
//
// 認証: MERCH_APPROVAL_KEY（無ければ ADMIN_TOKEN）。未設定環境では 404 で非公開。
// 販売開始(live化)は承認状況を見て人が行う。ここでは記録と表示まで。

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use std::collections::HashMap;

use crate::Db;

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
fn html_attr_escape(s: &str) -> String {
    html_escape(s).replace('"', "&quot;").replace('\'', "&#39;")
}

const LABEL_PREFIX: &str = "SOLUNA FEST HAWAII";
const MERCH_BRAND: &str = "g-cb0e8c21";

/// 承認/却下ページの鍵。MERCH_APPROVAL_KEY 優先、無ければ ADMIN_TOKEN。
fn merch_key() -> Option<String> {
    std::env::var("MERCH_APPROVAL_KEY").ok().filter(|s| !s.is_empty())
        .or_else(|| std::env::var("ADMIN_TOKEN").ok().filter(|s| !s.is_empty()))
}

fn key_ok(q: &HashMap<String, String>, headers: &HeaderMap) -> bool {
    let Some(expected) = merch_key() else { return false };
    let mut cand = q.get("key").cloned();
    if cand.is_none() {
        cand = headers.get("x-merch-key").and_then(|v| v.to_str().ok()).map(String::from);
    }
    match cand {
        Some(t) => t.len() == expected.len()
            && t.bytes().zip(expected.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0,
        None => false,
    }
}

/// ラベルからグループ（DJ/ブランド）を切り出す。"SOLUNA FEST HAWAII — ARODES — motif"
/// → "ARODES"。区切りは em dash / en dash / hyphen を許容。
fn group_of(label: &str) -> String {
    let rest = label.strip_prefix(LABEL_PREFIX).unwrap_or(label).trim_start();
    let rest = rest.trim_start_matches(['—', '–', '-', ':', ' ']).trim();
    rest.split(['—', '–', '-', ':']).next().unwrap_or("").trim().to_string()
}

fn motif_of(label: &str) -> String {
    let parts: Vec<&str> = label.split(['—', '–']).map(|s| s.trim()).collect();
    if parts.len() >= 3 { parts[2].to_string() } else { String::new() }
}

const GROUP_ORDER: &[&str] = &["ZAMNA", "WHOMADEWHO", "ARODES", "STUDIO APARTMENT", "FIYAH"];
const GROUP_LABEL: &[(&str, &str)] = &[
    ("ZAMNA", "ZAMNA (brand approval §4.6)"),
    ("WHOMADEWHO", "WhoMadeWho / management"),
    ("ARODES", "Arodes / management"),
    ("STUDIO APARTMENT", "Studio Apartment"),
    ("FIYAH", "Fiyah / management"),
];

/// GET /merch-approval?key=… — グループ別リスト（鍵必須・noindex）。
pub async fn merch_approval_page(
    State(db): State<Db>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if !key_ok(&q, &headers) {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    // 決定済み(map: sku -> {decision,name,at})
    let decisions: HashMap<String, (String, String, String)> = {
        let conn = db.lock().unwrap();
        read_decisions(&conn)
    };
    let key = q.get("key").cloned().unwrap_or_default();
    let key_q = urlencoding::encode(&key);
    let rows: Vec<(String, String, String, String)> = {
        let conn = db.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT sku, label, COALESCE(mockup_url_external,''), status
             FROM catalog_products WHERE label LIKE ?1 ORDER BY label",
        ) {
            Ok(s) => s,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")).into_response(),
        };
        let like = format!("{}%", LABEL_PREFIX);
        stmt.query_map([like], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map(|it| it.filter_map(|x| x.ok()).collect()).unwrap_or_default()
    };

    // グループ別に束ねる
    let mut by_group: HashMap<String, Vec<(String, String, String, String)>> = HashMap::new();
    for row in rows {
        by_group.entry(group_of(&row.1)).or_default().push(row);
    }
    let mut known: Vec<String> = GROUP_ORDER.iter().map(|s| s.to_string()).collect();
    for g in by_group.keys() { if !known.contains(g) { known.push(g.clone()); } }

    let mut body = String::new();
    for g in &known {
        let Some(items) = by_group.get(g) else { continue };
        let display = GROUP_LABEL.iter().find(|(k, _)| k == g).map(|(_, v)| *v).unwrap_or(g.as_str());
        let approved = items.iter().filter(|r| decisions.get(&r.0).map(|d| d.0 == "approve").unwrap_or(false)).count();
        body.push_str(&format!(
            r#"<h2 class="sec">{disp} <span class="cnt">{n}案 · 承認 {ap}</span></h2><div class="grid">"#,
            disp = html_escape(display), n = items.len(), ap = approved));
        for (sku, label, mock, status) in items {
            let motif = motif_of(label);
            let dec = decisions.get(sku);
            let (badge, cls) = match dec {
                Some((d, name, at)) if d == "approve" => (format!("✓ 承認{} / {}", name_html(name), at), "ok"),
                Some((d, name, at)) if d == "reject" => (format!("却下{} / {}", name_html(name), at), "no"),
                _ => (format!("未回答 · status={}", html_escape(status)), "pending"),
            };
            let img = if mock.is_empty() {
                String::new()
            } else {
                format!(r#"<div class="mock"><img loading="lazy" src="{}" alt="" title="クリックで拡大"></div>"#, html_attr_escape(mock))
            };
            body.push_str(&format!(
                r#"<figure class="card">{img}
<figcaption><b>{motif}</b><div class="sku">{sku}</div>
<div class="st {cls}">{badge}</div>
<div class="acts">
<button class="ok" data-sku="{sku}" data-decide="approve">承認</button>
<button class="no" data-sku="{sku}" data-decide="reject">却下</button>
</div></figcaption></figure>"#,
                img = img, motif = html_escape(&motif), sku = html_escape(sku), cls = cls, badge = badge));
        }
        body.push_str("</div>");
    }
    if known.is_empty() || body.is_empty() {
        body.push_str("<p>承認対象の商品がありません。</p>");
    }

    let html = format!(
        r##"<!doctype html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="robots" content="noindex,nofollow,noarchive">
<title>SOLUNA × ZAMNA — 商品承認</title>
<style>
:root{{--bg:#0b0b0d;--card:#141418;--fg:#eee5ce;--dim:#8b8677;--ok:#7ee0a0;--no:#ff8a6a}}
*{{box-sizing:border-box}}body{{margin:0;background:var(--bg);color:var(--fg);
font-family:-apple-system,BlinkMacSystemFont,"Hiragino Sans",sans-serif}}
header{{padding:24px 22px 4px;max-width:1500px;margin:0 auto}}
h1{{font-size:19px;margin:0 0 6px;letter-spacing:.04em}}
.sub{{color:var(--dim);font-size:13px;line-height:1.7}}
.who{{max-width:1500px;margin:14px auto 0;padding:0 22px}}
.who input{{background:#141418;border:1px solid #232329;color:var(--fg);border-radius:8px;padding:9px 12px;font:inherit;width:260px;max-width:100%}}
.sec{{font-size:20px;margin:40px 0 2px;max-width:1600px;padding:14px 22px 10px;
border-top:1px solid #232329;letter-spacing:.06em;background:linear-gradient(#17171c,transparent)}}
.cnt{{color:var(--dim);font-size:13px;font-weight:400;letter-spacing:0}}
main{{max-width:1600px;margin:0 auto;padding:4px 22px 90px}}
.grid{{display:grid;grid-template-columns:repeat(auto-fill,minmax(300px,1fr));gap:20px;margin-top:12px}}
.card{{margin:0;background:var(--card);border:1px solid #2a2a31;border-radius:14px;padding:10px;
transition:border-color .15s,transform .15s}}
.card:hover{{border-color:#3a3a44;transform:translateY(-2px)}}
.mock{{background:#0e0e10;border-radius:10px;overflow:hidden;position:relative}}
.mock img{{width:100%;display:block;cursor:zoom-in;aspect-ratio:1/1;object-fit:cover}}
.mock::after{{content:"⤢ クリックで拡大";position:absolute;right:8px;bottom:8px;
background:rgba(0,0,0,.55);color:#eee;font-size:11px;padding:3px 8px;border-radius:20px;
opacity:0;transition:opacity .15s;pointer-events:none}}
.mock:hover::after{{opacity:1}}
figcaption{{font-size:14px;color:var(--fg);margin-top:10px}}
.sku{{font-family:ui-monospace,monospace;font-size:11px;color:var(--dim);margin:3px 0 8px;word-break:break-all}}
.st{{font-size:12px;margin-bottom:8px}}
.st.ok{{color:var(--ok)}} .st.no{{color:var(--no)}} .st.pending{{color:var(--dim)}}
.acts{{display:flex;gap:8px}}
.acts button{{flex:1;cursor:pointer;border:1px solid #232329;border-radius:10px;padding:11px 0;font:inherit;font-size:15px;font-weight:700}}
.acts .ok{{background:#123a22;color:#bdf5cf;border-color:#2f7d46}}
.acts .no{{background:#3a1712;color:#f5cfc0;border-color:#7d3a2f}}
.acts button:disabled{{opacity:.5;cursor:default}}
#lb{{position:fixed;inset:0;background:rgba(0,0,0,.92);display:none;z-index:99;
align-items:center;justify-content:center;padding:3vmin;cursor:zoom-out}}
#lb.on{{display:flex}}
#lb img{{max-width:96vw;max-height:96vh;border-radius:10px;box-shadow:0 10px 60px rgba(0,0,0,.6)}}
#lb .x{{position:fixed;top:16px;right:22px;color:#eee;font-size:26px;font-weight:700;pointer-events:none}}
.foot{{color:var(--dim);font-size:12px;margin-top:26px;border-top:1px solid #232329;padding-top:14px;line-height:1.7}}
</style></head><body>
<header>
<h1>SOLUNA × ZAMNA ／ DJコラボT — 商品承認</h1>
<div class="sub">2026.11.13 · Oʻahu　｜　グループ別に承認/却下を記録できます。<br>
承認が揃うまで公開・販売しません。無回答は承認ではありません。</div>
</header>
<div class="who"><input id="name" placeholder="お名前 / your name（任意）" autocomplete="name"></div>
<main>{body}
<div class="foot">Approval is limited to this product, media, term and the final artwork. Changes require renewed approval. ／ お問い合わせ: info@solun.art</div>
</main>
<div id="lb"><span class="x">×</span><img alt=""></div>
<script>
(function(){{
 var KEY="__KEY__";
 // クリックで拡大（ライトボックス）。
 var lb=document.getElementById('lb'), lbi=lb.querySelector('img');
 document.querySelectorAll('.mock img').forEach(function(im){{
  im.addEventListener('click', function(){{ lbi.src=im.src; lb.classList.add('on'); }});
 }});
 lb.addEventListener('click', function(){{ lb.classList.remove('on'); lbi.src=''; }});
 document.addEventListener('keydown', function(e){{ if(e.key==='Escape'){{ lb.classList.remove('on'); lbi.src=''; }} }});
 document.querySelectorAll('.acts button').forEach(function(b){{
  b.addEventListener('click', async function(){{
   var sku=b.getAttribute('data-sku'), decide=b.getAttribute('data-decide');
   var name=(document.getElementById('name').value||'').trim();
   var acts=b.parentElement; acts.querySelectorAll('button').forEach(function(x){{x.disabled=true;}});
   try{{
    var r=await fetch('/api/merch/approval?key='+encodeURIComponent(KEY),{{method:'POST',
     headers:{{'Content-Type':'application/json'}},
     body:JSON.stringify({{sku:sku,decision:decide,name:name}})}});
    var j=await r.json();
    if(!(r.ok&&j.ok)) throw new Error(j.error||('HTTP '+r.status));
    var st=b.closest('figcaption').querySelector('.st');
    st.className='st '+(decide==='approve'?'ok':'no');
    st.textContent=(decide==='approve'?'✓ 承認':'却下')+(name?(' / '+name):'');
   }}catch(e){{ acts.querySelectorAll('button').forEach(function(x){{x.disabled=false;}});
    alert('記録できませんでした: '+e.message); }}
  }});
 }});
}})();
</script>
</body></html>"##,
        body = body,
    );
    let html = html.replace("__KEY__", &key_q_js(&key));
    (StatusCode::OK, Html(html)).into_response()
}

fn name_html(n: &str) -> String {
    if n.is_empty() { String::new() } else { format!(" / {}", html_escape(n)) }
}

/// JS 文字列リテラル用エスケープ（key はサーバ発行の英数記号なので最小限）。
fn key_q_js(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('<', "")
}

#[derive(Deserialize)]
pub struct DecideBody {
    pub sku: String,
    pub decision: String,
    pub name: Option<String>,
}

/// POST /api/merch/approval?key=… — 承認/却下を config_json.approvals へ追記。
pub async fn merch_approval_decide(
    State(db): State<Db>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(b): Json<DecideBody>,
) -> Response {
    if !key_ok(&q, &headers) {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"ok":false,"error":"key required"}))).into_response();
    }
    let expect = merch_key().unwrap_or_default();
    let is_admin = std::env::var("ADMIN_TOKEN").ok().map(|t| t == expect).unwrap_or(false);
    let _ = is_admin;
    let sku = b.sku.trim().to_string();
    let decision = match b.decision.as_str() { "approve" => "approve", "reject" => "reject", _ => {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"ok":false,"error":"decision must be approve|reject"}))).into_response();
    }};
    let name = b.name.unwrap_or_default().trim().chars().take(120).collect::<String>();
    if sku.is_empty() || sku.len() > 200 {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"ok":false,"error":"sku required"}))).into_response();
    }
    // 対象SKUの存在とラベル（グループ判定用）を確認。
    let label: Option<String> = {
        let conn = db.lock().unwrap();
        conn.query_row("SELECT label FROM catalog_products WHERE sku=?1", [&sku], |r| r.get(0)).ok()
    };
    let Some(label) = label else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"ok":false,"error":"unknown sku"}))).into_response();
    };
    let group = group_of(&label);
    let now = crate::chrono_now_jst_datetime_str();
    {
        let conn = db.lock().unwrap();
        let cfg: Option<String> = conn.query_row(
            "SELECT config_json FROM catalog_brands WHERE slug=?1", [MERCH_BRAND], |r| r.get(0),
        ).ok().flatten();
        let mut v: serde_json::Value = cfg.as_deref().and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if !v.is_object() { v = serde_json::json!({}); }
        let arr = v.as_object_mut().unwrap().entry("approvals").or_insert_with(|| serde_json::json!([]));
        if let Some(a) = arr.as_array_mut() {
            a.push(serde_json::json!({"sku": sku, "group": group, "decision": decision,
                "name": name, "at": now}));
        }
        let _ = conn.execute("UPDATE catalog_brands SET config_json=?1 WHERE slug=?2",
            rusqlite::params![v.to_string(), MERCH_BRAND]);
    }
    Json(serde_json::json!({"ok":true, "sku":sku, "group":group, "decision":decision, "at":now})).into_response()
}

/// config_json.approvals を sku -> (decision,name,at) に畳む（後勝ち＝最新）。
fn read_decisions(conn: &rusqlite::Connection) -> HashMap<String, (String, String, String)> {
    let mut out = HashMap::new();
    let cfg: Option<String> = conn.query_row(
        "SELECT config_json FROM catalog_brands WHERE slug=?1", [MERCH_BRAND], |r| r.get(0),
    ).ok().flatten();
    let Some(cfg) = cfg else { return out };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&cfg) else { return out };
    let Some(arr) = v.get("approvals").and_then(|a| a.as_array()) else { return out };
    for e in arr {
        let sku = e.get("sku").and_then(|s| s.as_str()).unwrap_or("");
        if sku.is_empty() { continue; }
        let d = e.get("decision").and_then(|s| s.as_str()).unwrap_or("");
        let n = e.get("name").and_then(|s| s.as_str()).unwrap_or("");
        let at = e.get("at").and_then(|s| s.as_str()).unwrap_or("");
        out.insert(sku.to_string(), (d.to_string(), n.to_string(), short_time(at)));
    }
    out
}

fn short_time(s: &str) -> String {
    // "2026-10-09 12:34:56 JST" → "2026-10-09 12:34"。それ以外はそのまま。
    if s.len() >= 16 && s.as_bytes().get(10) == Some(&b' ') {
        s[..16].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_and_motif_parse_from_label() {
        // 実際に登録している形式。
        let l = "SOLUNA FEST HAWAII — ZAMNA — cenote — PENDING APPROVAL";
        assert_eq!(group_of(l), "ZAMNA");
        assert_eq!(motif_of(l), "cenote");

        // 2語のグループ（STUDIO APARTMENT）も正しく切れる。
        let l2 = "SOLUNA FEST HAWAII — STUDIO APARTMENT — nightwave — PENDING APPROVAL";
        assert_eq!(group_of(l2), "STUDIO APARTMENT");
        assert_eq!(motif_of(l2), "nightwave");

        // 接頭辞が無い/区切りが違っても落ちない。
        assert_eq!(group_of("OTHER — X — y"), "OTHER");
        assert_eq!(motif_of("no-dashes"), "");
    }

    #[test]
    fn key_check_is_exact_and_fail_closed() {
        std::env::set_var("MERCH_APPROVAL_KEY", "secret-key");
        let mut q = HashMap::new();
        q.insert("key".to_string(), "secret-key".to_string());
        assert!(key_ok(&q, &HeaderMap::new()));
        let mut bad = HashMap::new();
        bad.insert("key".to_string(), "wrong".to_string());
        assert!(!key_ok(&bad, &HeaderMap::new()));
        // 鍵なしは false（公開しない）。
        assert!(!key_ok(&HashMap::new(), &HeaderMap::new()));
        std::env::remove_var("MERCH_APPROVAL_KEY");
    }

    #[test]
    fn short_time_trims_to_minutes() {
        assert_eq!(short_time("2026-10-09 12:34:56 JST"), "2026-10-09 12:34");
        assert_eq!(short_time("1791600447"), "1791600447");
    }
}
