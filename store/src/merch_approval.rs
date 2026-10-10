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
        let rejected = items.iter().filter(|r| decisions.get(&r.0).map(|d| d.0 == "reject").unwrap_or(false)).count();
        let pending = items.len() - approved - rejected;
        let group_id = format!("approval-group-{}", known.iter().position(|x| x == g).unwrap_or(0));
        body.push_str(&format!(
            r#"<h2 class="sec" id="{group_id}" data-group-heading="{group_attr}">{disp} <span class="cnt" aria-live="polite">承認 {ap} ・却下 {re} ・未回答 {pending}（計 {n}）</span></h2><div class="grid" data-group="{group_attr}">"#,
            group_id = group_id,
            group_attr = html_attr_escape(g),
            disp = html_escape(display), n = items.len(), ap = approved, re = rejected, pending = pending));
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
                r#"<figure class="card" data-group="{group_attr}" data-search="{search}">{img}
<figcaption><b>{motif}</b><div class="sku">{sku}</div>
<div class="st {cls}" aria-live="polite">{badge}</div>
<div class="acts">
<button class="ok" type="button" data-sku="{sku}" data-decide="approve" aria-label="{motif} を承認">承認</button>
<button class="no" type="button" data-sku="{sku}" data-decide="reject" aria-label="{motif} を却下">却下</button>
</div></figcaption></figure>"#,
                img = img, group_attr = html_attr_escape(g),
                search = html_attr_escape(&format!("{} {}", motif, sku).to_lowercase()),
                motif = html_escape(&motif), sku = html_escape(sku), cls = cls, badge = badge));
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
:root{{--bg:#0b0b0d;--card:#141418;--fg:#eee5ce;--dim:#aaa596;--ok:#7ee0a0;--no:#ff9a83;--line:#303039}}
*{{box-sizing:border-box}}body{{margin:0;background:var(--bg);color:var(--fg);
font-family:-apple-system,BlinkMacSystemFont,"Hiragino Sans",sans-serif;line-height:1.55}}
header{{padding:28px 22px 8px;max-width:1500px;margin:0 auto}}
h1{{font-size:clamp(22px,2.5vw,30px);line-height:1.35;margin:0 0 8px;letter-spacing:.02em}}
.sub{{color:var(--dim);font-size:14px;line-height:1.8;max-width:900px}}
.who{{max-width:1500px;margin:12px auto 18px;padding:0 22px}}
.who label{{display:block;color:var(--dim);font-size:12px;margin-bottom:5px}}
.who input{{background:#141418;border:1px solid #393942;color:var(--fg);border-radius:9px;padding:10px 12px;font:inherit;width:min(100%,360px);min-height:44px}}
.who input::placeholder{{color:#878476}}
.who input:focus-visible,.tool button:focus-visible,.tool input:focus-visible,.tool select:focus-visible,.acts button:focus-visible,.mock img:focus-visible,.jump a:focus-visible{{outline:3px solid #e7c56b;outline-offset:3px}}
.sec{{font-size:clamp(19px,2vw,24px);line-height:1.35;margin:32px 0 0;max-width:1600px;padding:17px 22px 8px;
border-top:1px solid var(--line);letter-spacing:.02em;background:linear-gradient(#17171c,transparent);scroll-margin-top:150px}}
.cnt{{color:var(--dim);font-size:13px;font-weight:400;letter-spacing:0;white-space:nowrap}}
main{{max-width:1480px;margin:0 auto;padding:0 22px 90px}}
.tool{{position:sticky;top:0;z-index:20;margin:0 -22px 18px;padding:12px 22px;background:rgba(11,11,13,.97);border-top:1px solid #292930;border-bottom:1px solid #383840;box-shadow:0 8px 24px rgba(0,0,0,.28);backdrop-filter:blur(12px)}}
.tool-top{{display:flex;align-items:center;justify-content:space-between;gap:12px;flex-wrap:wrap;margin-bottom:9px}}
.progress{{font-size:14px;font-weight:700}}.progress small{{display:block;color:var(--dim);font-size:12px;font-weight:400}}
.controls{{display:flex;align-items:center;gap:7px;flex-wrap:wrap}}
.tool button,.tool input,.tool select{{font:inherit;font-size:13px;color:var(--fg);background:#17171c;border:1px solid #393942;border-radius:9px;min-height:40px;padding:8px 11px}}
.tool button{{cursor:pointer}}.tool button[aria-pressed="true"]{{color:#101014;background:#e7c56b;border-color:#e7c56b;font-weight:700}}
.tool input{{flex:1 1 190px;max-width:300px}}.tool input::placeholder{{color:#aaa596}}
.jump{{display:flex;gap:7px;overflow-x:auto;padding:2px 0 3px;scrollbar-width:thin}}
.jump a{{flex:0 0 auto;border:1px solid #393942;color:var(--fg);background:#17171c;padding:7px 12px;border-radius:999px;font-size:12px;text-decoration:none}}
.jump a:hover{{border-color:#e7c56b}}
.grid{{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,260px),1fr));gap:16px;margin-top:10px}}
.card{{margin:0;background:var(--card);border:1px solid #303039;border-radius:14px;padding:12px;transition:border-color .15s}}
.card:hover{{border-color:#555560}}.card[hidden],.sec[hidden]{{display:none!important}}
.mock{{background:#101014;border-radius:10px;overflow:hidden;position:relative}}
.mock img{{width:100%;display:block;cursor:zoom-in;aspect-ratio:4/3;object-fit:contain;background:#101014}}
.mock::after{{content:"⤢ 画像を拡大";position:absolute;right:8px;bottom:8px;background:rgba(0,0,0,.75);color:#fff;font-size:11px;padding:4px 8px;border-radius:20px;pointer-events:none}}
figcaption{{font-size:14px;color:var(--fg);margin-top:10px}}
.sku{{font-family:ui-monospace,monospace;font-size:11px;color:var(--dim);margin:3px 0 8px;word-break:break-all}}
.st{{font-size:12px;line-height:1.6;min-height:19px;margin-bottom:8px}}
.st.ok{{color:var(--ok)}} .st.no{{color:var(--no)}} .st.pending{{color:var(--dim)}}
.acts{{display:flex;gap:8px}}
.acts button{{flex:1;cursor:pointer;border:1px solid #393942;border-radius:10px;min-height:46px;padding:10px 8px;font:inherit;font-size:15px;font-weight:700}}
.acts .ok{{background:#123a22;color:#c9f4d5;border-color:#378451}}
.acts .no{{background:#3a1712;color:#ffd1c5;border-color:#945043}}
.acts button:disabled{{opacity:.55;cursor:default}}
#lb{{position:fixed;inset:0;background:rgba(0,0,0,.92);display:none;z-index:99;
align-items:center;justify-content:center;padding:3vmin;cursor:zoom-out}}
#lb.on{{display:flex}}
#lb img{{max-width:96vw;max-height:96vh;border-radius:10px;box-shadow:0 10px 60px rgba(0,0,0,.6)}}
#lb .x{{position:fixed;top:16px;right:22px;color:#fff;background:#24242a;border:1px solid #666;border-radius:999px;width:44px;height:44px;font-size:26px;font-weight:700;cursor:pointer}}
.foot{{color:var(--dim);font-size:12px;margin:26px auto 0;max-width:900px;border-top:1px solid #303039;padding-top:14px;line-height:1.8}}
@media(max-width:640px){{header{{padding:22px 16px 8px}}.who{{padding:0 16px}}main{{padding-left:16px;padding-right:16px}}.tool{{margin-left:-16px;margin-right:-16px;padding:10px 16px}}.tool-top{{align-items:flex-start}}.controls{{width:100%}}.tool input{{max-width:none}}.tool select{{flex:1 1 100%}}.sec{{padding-left:16px;padding-right:16px;margin-top:24px}}.grid{{grid-template-columns:1fr;gap:14px}}.mock img{{aspect-ratio:1/1}}}}
@media(prefers-reduced-motion:reduce){{*,*::before,*::after{{scroll-behavior:auto!important;transition:none!important}}}}
</style></head><body>
<header>
<h1>SOLUNA × ZAMNA ／ DJコラボT — 商品承認</h1>
<div class="sub">2026.11.13 · Oʻahu　｜　グループ別に承認/却下を記録できます。<br>
承認が揃うまで公開・販売しません。無回答は承認ではありません。</div>
</header>
<div class="who"><label for="name">記録に表示するお名前（任意）／ Name</label><input id="name" placeholder="お名前 / your name（任意）" autocomplete="name"></div>
<main>
<section class="tool" aria-label="商品一覧の検索と絞り込み">
 <div class="tool-top"><div class="progress" id="approval-progress" aria-live="polite"></div>
  <div class="controls" role="group" aria-label="承認状況で絞り込み">
   <button type="button" data-filter="all" aria-pressed="true">すべて</button>
   <button type="button" data-filter="pending" aria-pressed="false">未回答</button>
   <button type="button" data-filter="approve" aria-pressed="false">承認済み</button>
   <button type="button" data-filter="reject" aria-pressed="false">却下</button>
   <input type="search" id="approval-search" placeholder="デザイン名・SKUを検索" aria-label="デザイン名またはSKUで検索">
   <select id="approval-group-filter" aria-label="グループで絞り込み"><option value="all">すべてのグループ</option></select>
  </div>
 </div>
 <nav class="jump" aria-label="グループへ移動"></nav>
</section>
{body}
<div class="foot">Approval is limited to this product, media, term and the final artwork. Changes require renewed approval. ／ お問い合わせ: info@solun.art</div>
</main>
<div id="lb" role="dialog" aria-modal="true" aria-label="商品画像の拡大表示"><button class="x" type="button" aria-label="閉じる" tabindex="-1">×</button><img alt=""></div>
<script>
(function(){{
  var KEY="__KEY__";
  // クリックで拡大（ライトボックス）。
  var lb=document.getElementById('lb'), lbi=lb.querySelector('img');
  document.querySelectorAll('.mock img').forEach(function(im){{
   var caption=im.closest('.card').querySelector('figcaption b').textContent;
   im.alt=caption+' のTシャツモックアップ。クリックで拡大';
   im.tabIndex=0; im.setAttribute('role','button');
   im.addEventListener('click', function(){{ lastImage=im; lbi.src=im.src; lbi.alt=caption+' の拡大画像'; lb.classList.add('on'); document.body.style.overflow='hidden'; closeButton.focus(); }});
   im.addEventListener('keydown', function(e){{if(e.key==='Enter'||e.key===' '){{e.preventDefault();im.click();}}}});
  }});
  var closeButton=lb.querySelector('.x'), lastImage=null;
  function closeLightbox(){{lb.classList.remove('on');lbi.src='';lbi.alt='';document.body.style.overflow='';if(lastImage)lastImage.focus();}}
  closeButton.addEventListener('click',function(e){{e.stopPropagation();closeLightbox();}});
  lb.addEventListener('click', function(){{ closeLightbox(); }});
  document.addEventListener('keydown', function(e){{ if(e.key==='Escape'&&lb.classList.contains('on')){{ closeLightbox(); }} }});

  // 商品承認そのものは既存APIへ送り、絞り込みは画面内だけで処理する。
  var cards=Array.from(document.querySelectorAll('.card'));
  var headings=Array.from(document.querySelectorAll('.sec'));
  var groupSelect=document.getElementById('approval-group-filter');
  var jump=document.querySelector('.jump');
  headings.forEach(function(h){{
   var group=h.dataset.groupHeading, label=h.childNodes[0].textContent.trim();
   var option=document.createElement('option');option.value=group;option.textContent=label;groupSelect.appendChild(option);
   var link=document.createElement('a');link.href='#'+h.id;link.textContent=label;jump.appendChild(link);
  }});
  var activeFilter='all';
  function decisionOf(card){{var st=card.querySelector('.st');return st.classList.contains('ok')?'approve':st.classList.contains('no')?'reject':'pending';}}
  function refreshList(){{
   var query=document.getElementById('approval-search').value.trim().toLowerCase(), group=groupSelect.value;
   var counts={{approve:0,reject:0,pending:0}}, shown=0;
   cards.forEach(function(card){{
    var state=decisionOf(card);counts[state]++;
    var visible=(activeFilter==='all'||activeFilter===state)&&(group==='all'||card.dataset.group===group)&&(!query||card.dataset.search.indexOf(query)!==-1);
    card.hidden=!visible;if(visible)shown++;
   }});
   headings.forEach(function(h){{
    var grid=h.nextElementSibling, groupCards=Array.from(grid.querySelectorAll('.card'));
    h.hidden=!groupCards.some(function(card){{return !card.hidden;}});
    var groupCounts={{approve:0,reject:0,pending:0}};
    groupCards.forEach(function(card){{groupCounts[decisionOf(card)]++;}});
    h.querySelector('.cnt').textContent='承認 '+groupCounts.approve+' ・却下 '+groupCounts.reject+' ・未回答 '+groupCounts.pending+'（計 '+groupCards.length+'）';
   }});
   var total=cards.length, done=counts.approve+counts.reject;
   document.getElementById('approval-progress').innerHTML='承認の進捗　'+done+' / '+total+' 件回答済み<small>承認 '+counts.approve+' ・却下 '+counts.reject+' ・未回答 '+counts.pending+'　｜　表示 '+shown+' 件</small>';
  }}
  document.querySelectorAll('[data-filter]').forEach(function(btn){{btn.addEventListener('click',function(){{activeFilter=btn.dataset.filter;document.querySelectorAll('[data-filter]').forEach(function(other){{other.setAttribute('aria-pressed',String(other===btn));}});refreshList();}});}});
  document.getElementById('approval-search').addEventListener('input',refreshList);
  groupSelect.addEventListener('change',refreshList);
  refreshList();
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
     refreshList();
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
