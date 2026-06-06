// store/src/creators.rs — クリエイターの「人間の顔」レイヤー。
//
// MU をメルカリ/Etsy 型の「誰でも作って売れる」場にする 3 点セット:
//   /start  — メール magic-link でクリエイター登録(既存 collab_users 認証を流用)
//   /studio — 自分の作品・売上・作者報酬・紹介リンクが 1 画面のダッシュボード
//   /kpi    — 北極星 KPI「初売上クリエイター数/週」の公開ページ (+ /api/kpi JSON)
//
// 新テーブルなし。collab_users.display_name 列のみ追加(main.rs migration)。
// 作者帰属は catalog_products.meta_json.$.maker_email (既存の /make 認証で刻印
// 済みのキー) と catalog_brands.config_json.$.owner_email (agent ストア) を使う。
// 作者報酬の付与は catalog.rs::apply_maker_commission (注文 fulfill 時・冪等)。

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};

use crate::Db;

/// 作者(maker)を解決する共通 SQL 断片: /make 認証で刻まれた meta_json の
/// maker_email が先勝ち、無ければ agent ストアのオーナー。p = catalog_products。
pub(crate) const MAKER_SQL: &str = "LOWER(COALESCE(NULLIF(json_extract(p.meta_json,'$.maker_email'),''), \
     (SELECT json_extract(b.config_json,'$.owner_email') FROM catalog_brands b WHERE b.slug=p.brand), ''))";

fn fmt_jpy(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 { out.push(','); }
        out.push(c);
    }
    if n < 0 { format!("-{}", out) } else { out }
}

// ════════════════════════════════════════════════════════════════════
// GET /start — クリエイター登録 (メール → 6桁コード → /studio)
// ════════════════════════════════════════════════════════════════════

const START_HTML: &str = r##"<!doctype html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>MU STUDIO — 30秒で、自分のブランドを持つ</title>
<meta name="description" content="ことば1行で商品が生まれ、売れたら10%があなたに。メール1本でクリエイター登録。">
<meta property="og:title" content="MU STUDIO — 30秒で、自分のブランドを持つ">
<meta property="og:description" content="ことば1行で商品が生まれ、売れたら10%があなたに。">
<style>
body{background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,'Helvetica Neue',Arial,sans-serif;margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;padding:24px}
.box{max-width:460px;width:100%}
.logo{font-size:20px;font-weight:700;letter-spacing:.45em;margin-bottom:6px}
.kicker{font-size:11px;letter-spacing:.3em;text-transform:uppercase;color:#ffd700;margin-bottom:22px}
h1{font-size:24px;font-weight:600;line-height:1.5;margin:0 0 10px}
p.lead{font-size:13.5px;opacity:.78;line-height:1.9;margin:0 0 22px}
input{width:100%;box-sizing:border-box;padding:13px 14px;border-radius:8px;border:1px solid #333;background:#111;color:#f5f5f0;font-size:15px;margin-bottom:10px}
button{width:100%;background:#ffd700;color:#0a0a0a;border:0;border-radius:8px;font-weight:800;padding:14px;font-size:15px;cursor:pointer;letter-spacing:.02em}
button:disabled{opacity:.5;cursor:wait}
.msg{font-size:13px;margin-top:12px;min-height:18px}
.msg.err{color:#ff6b6b}.msg.ok{color:#7ad58a}
.steps{display:flex;gap:14px;margin:26px 0 0;font-size:12px;opacity:.6;line-height:1.7}
.steps div{flex:1}
.steps b{color:#ffd700;display:block;font-size:15px}
a{color:#ffd700}
.fine{font-size:11px;opacity:.45;margin-top:26px;line-height:1.8}
#step2{display:none}
code.big{font-family:'SF Mono',monospace;letter-spacing:.3em}
</style></head><body>
<div class="box">
<div class="logo">━◯━ MU</div>
<div class="kicker">STUDIO — designed by you</div>
<div id="step1">
<h1>30秒で、自分のブランドを持つ。</h1>
<p class="lead">ことば1行で商品が生まれて、世界中に届く。<br><b style="color:#ffd700">売れたら10%があなたに</b>(MUクレジット)。在庫ゼロ・費用ゼロ・受注生産。</p>
<input id="email" type="email" placeholder="you@example.com" autocomplete="email" autofocus>
<button id="send" data-funnel="cta_click" data-funnel-cta="start_send_code">メールで始める(無料)</button>
<div class="msg" id="m1"></div>
<div class="steps">
<div><b>1</b>メールにコードが届く</div>
<div><b>2</b>ことば1行で商品が生まれる</div>
<div><b>3</b>売れたら10%があなたに</div>
</div>
</div>
<div id="step2">
<h1>メールのコードを入力</h1>
<p class="lead"><span id="sentTo"></span> に6桁の確認コードを送りました(15分有効)。届かない時は迷惑メールも確認してください。</p>
<input id="code" inputmode="numeric" pattern="[0-9]*" maxlength="6" placeholder="123456" class="big">
<button id="verify" data-funnel="cta_click" data-funnel-cta="start_verify_code">ログインしてスタジオへ →</button>
<div class="msg" id="m2"></div>
<p style="font-size:12px;opacity:.6"><a href="#" id="back">← メールアドレスを入れ直す</a></p>
</div>
<div class="fine">登録はメールアドレスのみ。<a href="/privacy">プライバシー</a> · <a href="/tokushoho">特商法</a> · すでに登録済みでも同じ手順でログインできます · <a href="/kpi">みんなの数字(公開KPI)</a></div>
</div>
<script defer src="/mu-funnel.js"></script>
<script>
var $=function(id){return document.getElementById(id)};
function msg(el,t,err){el.textContent=t;el.className='msg '+(err?'err':'ok')}
$('send').onclick=async function(){
  var e=$('email').value.trim();
  if(!/.+@.+\..+/.test(e)){msg($('m1'),'メールアドレスを確認してください',true);return}
  this.disabled=true;msg($('m1'),'送信中…');
  try{
    var r=await fetch('/api/collab/auth/start',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({email:e,next:'/studio'})});
    var j=await r.json().catch(function(){return{}});
    if(r.ok){$('sentTo').textContent=e;$('step1').style.display='none';$('step2').style.display='';$('code').focus();window._em=e}
    else msg($('m1'),j.error||'送信に失敗しました。少し待って再試行してください',true);
  }catch(_){msg($('m1'),'通信エラー',true)}
  this.disabled=false;
};
$('verify').onclick=async function(){
  var c=$('code').value.trim();
  if(!/^[0-9]{6}$/.test(c)){msg($('m2'),'6桁の数字を入力してください',true);return}
  this.disabled=true;msg($('m2'),'確認中…');
  try{
    var r=await fetch('/api/collab/auth/verify',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({email:window._em,code:c})});
    var j=await r.json().catch(function(){return{}});
    if(r.ok&&j.ok){msg($('m2'),'ログインしました。スタジオへ…');location.href='/studio'}
    else{msg($('m2'),j.error==='code expired'?'コードの期限が切れました。入れ直してください':'コードが一致しません',true);this.disabled=false}
  }catch(_){msg($('m2'),'通信エラー',true);this.disabled=false}
};
$('code').addEventListener('keydown',function(e){if(e.key==='Enter')$('verify').click()});
$('email').addEventListener('keydown',function(e){if(e.key==='Enter')$('send').click()});
$('back').onclick=function(e){e.preventDefault();$('step2').style.display='none';$('step1').style.display=''};
</script>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
</body></html>"##;

/// GET /start — クリエイター登録ページ。ログイン済みなら /studio へ。
pub async fn start_page(State(db): State<Db>, headers: HeaderMap) -> Response {
    if crate::collab_session_email(&db, &headers).is_some() {
        return Redirect::temporary("/studio").into_response();
    }
    Html(START_HTML).into_response()
}

// ════════════════════════════════════════════════════════════════════
// GET /studio — クリエイターダッシュボード
// ════════════════════════════════════════════════════════════════════

const STUDIO_HTML: &str = r##"<!doctype html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="robots" content="noindex">
<title>MU STUDIO — __EMAIL__</title>
<style>
body{background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,'Helvetica Neue',Arial,sans-serif;margin:0;padding:0}
.wrap{max-width:920px;margin:0 auto;padding:28px 22px 60px}
.logo{font-size:18px;font-weight:700;letter-spacing:.45em}
.top{display:flex;justify-content:space-between;align-items:baseline;flex-wrap:wrap;gap:8px;margin-bottom:4px}
.top .who{font-size:12px;opacity:.6}
.top a{color:#888;font-size:12px;text-decoration:none}
.kicker{font-size:11px;letter-spacing:.3em;text-transform:uppercase;color:#ffd700;margin:2px 0 20px}
.stats{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:10px;margin:0 0 26px}
.stat{background:#111;border:1px solid #222;border-radius:12px;padding:16px 14px}
.stat .v{font-size:24px;font-weight:700;color:#ffd700}
.stat .k{font-size:11px;opacity:.6;margin-top:4px;letter-spacing:.05em}
h2{font-size:15px;font-weight:700;letter-spacing:.08em;margin:30px 0 12px;color:#ffd700}
.make{background:linear-gradient(90deg,rgba(255,215,0,.12),rgba(255,215,0,.04));border:1px solid rgba(255,215,0,.4);border-radius:14px;padding:18px}
.make textarea{width:100%;box-sizing:border-box;background:#0d0d0d;border:1px solid #333;border-radius:8px;color:#f5f5f0;font-size:15px;padding:12px;min-height:64px;resize:vertical;font-family:inherit}
.make button{background:#ffd700;color:#0a0a0a;border:0;border-radius:8px;font-weight:800;padding:12px 26px;font-size:14px;cursor:pointer;margin-top:10px}
.make button:disabled{opacity:.5;cursor:wait}
.make .hint{font-size:11.5px;opacity:.55;margin-top:8px;line-height:1.7}
#mko{font-size:14px;margin-top:12px;line-height:1.7}
#mko a{color:#ffd700}
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(160px,1fr));gap:12px}
.card{background:#111;border:1px solid #222;border-radius:12px;overflow:hidden;text-decoration:none;color:#f5f5f0;display:block}
.card img{width:100%;aspect-ratio:1;object-fit:cover;background:#0d0d0d;display:block}
.card .b{padding:10px 12px}
.card .t{font-size:12.5px;line-height:1.5;max-height:3em;overflow:hidden}
.card .p{font-size:13px;color:#ffd700;font-weight:700;margin-top:4px}
.chip{display:inline-block;font-size:10px;border-radius:99px;padding:2px 9px;margin-top:6px;letter-spacing:.05em}
.chip.live{background:rgba(122,213,138,.15);color:#7ad58a}
.chip.review{background:rgba(255,200,80,.13);color:#fc8}
.chip.retired{background:#222;color:#888}
table{width:100%;border-collapse:collapse;font-size:13px}
td,th{padding:8px 10px;border-bottom:1px solid #1c1c1c;text-align:left}
th{font-size:11px;opacity:.55;letter-spacing:.08em}
td.amt{color:#7ad58a;font-weight:700;white-space:nowrap}
td.amt.neg{color:#ff8a8a}
.empty{font-size:13px;opacity:.55;line-height:1.9;background:#101010;border:1px dashed #2a2a2a;border-radius:12px;padding:18px}
.row2{display:grid;grid-template-columns:1fr;gap:10px}
@media(min-width:720px){.row2{grid-template-columns:1fr 1fr}}
.panel{background:#111;border:1px solid #222;border-radius:12px;padding:16px}
.panel .code{font-family:'SF Mono',monospace;color:#ffd700;background:#0d0d0d;border:1px solid #2a2a2a;border-radius:8px;padding:10px 12px;font-size:13px;word-break:break-all;margin:8px 0}
.panel p{font-size:12px;opacity:.65;line-height:1.8;margin:8px 0 0}
.panel input{box-sizing:border-box;width:100%;background:#0d0d0d;border:1px solid #333;border-radius:8px;color:#f5f5f0;font-size:14px;padding:10px 12px}
.panel button{background:#222;color:#f5f5f0;border:1px solid #444;border-radius:8px;padding:9px 18px;font-size:13px;cursor:pointer;margin-top:8px}
a{color:#ffd700}
footer{font-size:11px;opacity:.45;margin-top:46px;line-height:1.9}
</style></head><body>
<div class="wrap">
<div class="top"><span class="logo">━◯━ MU</span><span><span class="who">__EMAIL__</span> · <a href="/api/collab/auth/logout">ログアウト</a></span></div>
<div class="kicker">STUDIO</div>

<div class="stats">
<div class="stat"><div class="v">¥__BALANCE__</div><div class="k">MUクレジット残高</div></div>
<div class="stat"><div class="v">__NPROD__</div><div class="k">あなたの作品</div></div>
<div class="stat"><div class="v">__NSALES__</div><div class="k">売れた数</div></div>
<div class="stat"><div class="v">¥__EARNED__</div><div class="k">作者報酬 累計</div></div>
</div>

<div class="make">
<div style="font-size:15px;font-weight:700;margin-bottom:10px">✦ ことば1行で、次の一枚をつくる</div>
<textarea id="mkp" placeholder="例: 朝焼けの富士山と「継続」の二文字、墨絵タッチで"></textarea>
<button id="mkb" data-funnel="cta_click" data-funnel-cta="studio_make">つくる(無料)</button>
<div id="mko"></div>
<div class="hint">AIが約30秒でデザイン → そのまま棚に並びます(内容によっては人の確認後)。<b>売れるたびに売上の10%があなたのMUクレジットに</b>。じっくり作るなら <a href="/make?ref=studio">/make</a> へ。</div>
</div>

<h2>あなたの作品 (__NPROD__)</h2>
__PRODUCTS__

<h2>収益</h2>
<div class="row2">
<div class="panel">
<div style="font-size:13px;font-weight:700">作者報酬 — 売れたら10%</div>
<p>あなたの作品が売れるたび、売上の10%がMUクレジットで自動的に入ります。クレジットは次の購入で使えます。<b>現金振込は申請制</b>: <a href="mailto:info@enablerdao.com?subject=MU%20%E6%8C%AF%E8%BE%BC%E7%94%B3%E8%AB%8B">info@enablerdao.com</a> へ(残高¥3,000以上・運営が手動対応・通常5営業日)。</p>
__LEDGER__
</div>
<div class="panel">
<div style="font-size:13px;font-weight:700">紹介リンク — 広めても10%</div>
<div class="code">__REFLINK__</div>
<p>このリンク経由で<b>誰の商品でも</b>30日以内に売れると、売上の10%があなたに。SNSのプロフィールにどうぞ。<a href="/affiliate/__REFCODE__">実績を見る →</a></p>
</div>
</div>

<h2>公開名(作品ページに「つくった人」として表示)</h2>
<div class="panel">
<input id="dn" maxlength="24" placeholder="例: yuki / 道場の名前 / ニックネーム" value="__DISPLAY_NAME__">
<button id="dnb">保存</button> <span id="dnm" style="font-size:12px;margin-left:8px"></span>
<p>空のままなら「MU クリエイター」と匿名表示。メールアドレスが公開されることはありません。</p>
</div>

<footer>━◯━ MU · <a href="/shop">SHOP</a> · <a href="/make">作る</a> · <a href="/kpi">みんなの数字(公開KPI)</a> · <a href="/transparency">透明性レポート</a> · <a href="/returns">返品</a> · 株式会社イネブラ</footer>
</div>
<script defer src="/mu-funnel.js"></script>
<script>
var $=function(id){return document.getElementById(id)};
$('mkb').onclick=async function(){
  var p=$('mkp').value.trim();var o=$('mko');
  if(!p){$('mkp').focus();return}
  this.disabled=true;this.textContent='生成中… (約30秒)';o.textContent='';
  try{
    var r=await fetch('/api/make?prompt='+encodeURIComponent(p),{method:'POST'});
    var j=await r.json().catch(function(){return{}});
    if(j.ok){o.innerHTML='✓ できました — <a href="/shop/'+encodeURIComponent(j.sku)+'">'+(j.display||'作品')+' を見る →</a>'+(j.auto_approved?'':'<div style="opacity:.7;font-size:12px;margin-top:4px">内容確認のため、人の目を通してから公開されます</div>');}
    else{o.textContent=j.error||'生成に失敗しました。言い換えてお試しください。';}
  }catch(_){o.textContent='通信エラー。もう一度どうぞ。'}
  this.disabled=false;this.textContent='つくる(無料)';
};
$('dnb').onclick=async function(){
  var m=$('dnm');m.textContent='保存中…';
  try{
    var r=await fetch('/api/studio/profile',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({display_name:$('dn').value.trim()})});
    var j=await r.json().catch(function(){return{}});
    m.textContent=(r.ok&&j.ok)?'✓ 保存しました':(j.error||'保存できませんでした');
  }catch(_){m.textContent='通信エラー'}
};
</script>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
</body></html>"##;

/// GET /studio — 要ログイン。未ログインは /start へ。
pub async fn studio_page(State(db): State<Db>, headers: HeaderMap) -> Response {
    let Some((email, _collabs, _sub, _verified)) = crate::collab_session_email(&db, &headers) else {
        return Redirect::temporary("/start").into_response();
    };
    let email_lc = email.to_lowercase();
    let ref_code = crate::referral_code_for(&email_lc);

    struct Prod { sku: String, label: String, price: i64, status: String, img: String }
    let (balance, display_name, products, ledger, n_sales, earned): (i64, String, Vec<Prod>, Vec<(i64, String, String)>, i64, i64) = {
        let conn = db.lock().unwrap();
        // 紹介コードを常備(自己申請不要に) — /affiliate と同じ upsert。
        let _ = conn.execute(
            "INSERT INTO mu_referrals (code, owner_email, clicks, created_at)
             VALUES (?, ?, 0, ?)
             ON CONFLICT(code) DO UPDATE SET owner_email = excluded.owner_email",
            rusqlite::params![ref_code, email_lc, crate::chrono_now().parse::<i64>().unwrap_or(0)],
        );
        let balance = crate::mu_credit_balance(&conn, &email_lc);
        let display_name: String = conn.query_row(
            "SELECT COALESCE(display_name,'') FROM collab_users WHERE email=?",
            rusqlite::params![email_lc], |r| r.get(0)).unwrap_or_default();
        let products: Vec<Prod> = conn.prepare(
            &format!(
                "SELECT p.sku, p.label, p.retail_price_jpy, p.status,
                        COALESCE(p.mockup_url_external, p.mockup_main_file, p.design_file, '')
                 FROM catalog_products p
                 WHERE {maker} = ?1
                 ORDER BY p.created_at DESC LIMIT 60", maker = MAKER_SQL))
            .ok()
            .and_then(|mut s| s.query_map(rusqlite::params![email_lc], |r| Ok(Prod {
                sku: r.get(0)?, label: r.get(1)?, price: r.get(2)?, status: r.get(3)?, img: r.get(4)?,
            })).map(|it| it.filter_map(|x| x.ok()).collect()).ok())
            .unwrap_or_default();
        let ledger: Vec<(i64, String, String)> = conn.prepare(
            "SELECT delta_jpy, reason, datetime(created_at,'unixepoch') FROM mu_credit_ledger
             WHERE email=? ORDER BY id DESC LIMIT 12")
            .ok()
            .and_then(|mut s| s.query_map(rusqlite::params![email_lc], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map(|it| it.filter_map(|x| x.ok()).collect()).ok())
            .unwrap_or_default();
        let n_sales: i64 = conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM catalog_orders co JOIN catalog_products p ON p.sku = co.sku
                 WHERE co.amount_jpy > 0 AND co.status <> 'submitting' AND {maker} = ?1", maker = MAKER_SQL),
            rusqlite::params![email_lc], |r| r.get(0)).unwrap_or(0);
        let earned: i64 = conn.query_row(
            "SELECT COALESCE(SUM(delta_jpy),0) FROM mu_credit_ledger WHERE email=? AND reason LIKE 'creator:%'",
            rusqlite::params![email_lc], |r| r.get(0)).unwrap_or(0);
        (balance, display_name, products, ledger, n_sales, earned)
    };

    let products_html = if products.is_empty() {
        r#"<div class="empty">まだ作品がありません。上のフォームに「作りたいもの」を1行 — それだけで最初の1枚が生まれます。<br>例: 「黒帯への道、筆文字で」「コーヒーとプログラミング、線画で」</div>"#.to_string()
    } else {
        let cards: String = products.iter().map(|p| {
            let chip = match p.status.as_str() {
                "live" => r#"<span class="chip live">公開中</span>"#,
                "review" => r#"<span class="chip review">確認待ち</span>"#,
                s => return format!(
                    r#"<a class="card" href="/shop/{sku}"><img src="{img}" alt="" loading="lazy"><div class="b"><div class="t">{label}</div><div class="p">¥{price}</div><span class="chip retired">{st}</span></div></a>"#,
                    sku = crate::html_escape(&p.sku), img = crate::html_escape(&p.img),
                    label = crate::html_escape(p.label.chars().take(40).collect::<String>().as_str()),
                    price = fmt_jpy(p.price), st = crate::html_escape(s)),
            };
            format!(
                r#"<a class="card" href="/shop/{sku}"><img src="{img}" alt="" loading="lazy"><div class="b"><div class="t">{label}</div><div class="p">¥{price}</div>{chip}</div></a>"#,
                sku = crate::html_escape(&p.sku), img = crate::html_escape(&p.img),
                label = crate::html_escape(p.label.chars().take(40).collect::<String>().as_str()),
                price = fmt_jpy(p.price), chip = chip)
        }).collect();
        format!(r#"<div class="grid">{}</div>"#, cards)
    };

    let ledger_html = if ledger.is_empty() {
        r#"<p style="opacity:.5">まだ入出金はありません。最初の1枚が売れるとここに現れます。</p>"#.to_string()
    } else {
        let rows: String = ledger.iter().map(|(d, reason, at)| {
            let label = if reason.starts_with("creator:") { "作者報酬" }
                else if reason.starts_with("affiliate:") { "紹介報酬" }
                else if reason.starts_with("purchase:") { "購入特典" }
                else if reason.starts_with("agent_welcome") { "ようこそ特典" }
                else { reason.as_str() };
            format!(
                r#"<tr><td>{at}</td><td>{label}</td><td class="amt{neg}">{sign}¥{amt}</td></tr>"#,
                at = crate::html_escape(&at[..at.len().min(10)]),
                label = crate::html_escape(label),
                neg = if *d < 0 { " neg" } else { "" },
                sign = if *d < 0 { "-" } else { "+" },
                amt = fmt_jpy(d.abs()))
        }).collect();
        format!(r#"<table><tr><th>日付</th><th>内容</th><th>金額</th></tr>{}</table>"#, rows)
    };

    let base = std::env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    let ref_link = format!("{}/r/{}", base.trim_end_matches('/'), ref_code);

    let html = STUDIO_HTML
        .replace("__EMAIL__", &crate::html_escape(&email_lc))
        .replace("__BALANCE__", &fmt_jpy(balance))
        .replace("__NPROD__", &products.len().to_string())
        .replace("__NSALES__", &n_sales.to_string())
        .replace("__EARNED__", &fmt_jpy(earned))
        .replace("__PRODUCTS__", &products_html)
        .replace("__LEDGER__", &ledger_html)
        .replace("__REFLINK__", &crate::html_escape(&ref_link))
        .replace("__REFCODE__", &crate::html_escape(&ref_code))
        .replace("__DISPLAY_NAME__", &crate::html_escape(&display_name));
    Html(html).into_response()
}

// ════════════════════════════════════════════════════════════════════
// POST /api/studio/profile — 公開名の設定
// ════════════════════════════════════════════════════════════════════

#[derive(serde::Deserialize)]
pub struct ProfileBody { pub display_name: String }

pub async fn studio_profile(
    State(db): State<Db>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ProfileBody>,
) -> Response {
    let Some((email, ..)) = crate::collab_session_email(&db, &headers) else {
        return (StatusCode::UNAUTHORIZED, axum::Json(serde_json::json!({"ok":false,"error":"ログインしてください"}))).into_response();
    };
    let dn: String = body.display_name.trim().chars()
        .filter(|c| !c.is_control())
        .take(24)
        .collect();
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "UPDATE collab_users SET display_name=? WHERE email=?",
            rusqlite::params![if dn.is_empty() { None } else { Some(dn.as_str()) }, email.to_lowercase()],
        );
    }
    axum::Json(serde_json::json!({"ok": true, "display_name": dn})).into_response()
}

// ════════════════════════════════════════════════════════════════════
// 北極星 KPI — 「初売上を経験したクリエイター数 / 週」
// GET /api/kpi (JSON) · GET /kpi (公開ページ)
// ════════════════════════════════════════════════════════════════════

fn kpi_snapshot(db: &Db) -> serde_json::Value {
    let conn = db.lock().unwrap();
    let q1 = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };

    let creators_verified = q1("SELECT COUNT(*) FROM collab_users WHERE verified=1");
    let products_made = q1("SELECT COUNT(*) FROM catalog_products WHERE legacy_source IN ('public_make','agent_api')");
    let makers_attributed = q1(
        "SELECT COUNT(DISTINCT LOWER(json_extract(meta_json,'$.maker_email')))
         FROM catalog_products
         WHERE COALESCE(json_extract(meta_json,'$.maker_email'),'') LIKE '%@%'");
    let orders_total = q1("SELECT COUNT(*) FROM catalog_orders WHERE amount_jpy>0 AND status<>'submitting'");
    let revenue_total = q1("SELECT COALESCE(SUM(amount_jpy),0) FROM catalog_orders WHERE amount_jpy>0 AND status<>'submitting'");
    let makers_with_sale = q1(&format!(
        "SELECT COUNT(*) FROM (
            SELECT {maker} AS maker FROM catalog_orders co JOIN catalog_products p ON p.sku=co.sku
            WHERE co.amount_jpy>0 AND co.status<>'submitting'
            GROUP BY maker HAVING maker LIKE '%@%')", maker = MAKER_SQL));

    // 直近12週のキー(古→新)。
    let weeks: Vec<String> = conn.prepare(
        "WITH RECURSIVE w(i) AS (SELECT 11 UNION ALL SELECT i-1 FROM w WHERE i>0)
         SELECT strftime('%Y-%W', datetime('now', (-7*i)||' days')) FROM w")
        .ok()
        .and_then(|mut s| s.query_map([], |r| r.get::<_, String>(0))
            .map(|it| it.filter_map(|x| x.ok()).collect()).ok())
        .unwrap_or_default();

    let map_of = |sql: &str| -> std::collections::HashMap<String, i64> {
        conn.prepare(sql).ok()
            .and_then(|mut s| s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                .map(|it| it.filter_map(|x| x.ok()).collect()).ok())
            .unwrap_or_default()
    };
    let wk_creators = map_of(
        "SELECT strftime('%Y-%W', datetime(verified_at,'unixepoch')) wk, COUNT(*)
         FROM collab_users WHERE verified=1 AND verified_at IS NOT NULL GROUP BY wk");
    let wk_products = map_of(
        "SELECT strftime('%Y-%W', created_at) wk, COUNT(*)
         FROM catalog_products WHERE legacy_source IN ('public_make','agent_api') GROUP BY wk");
    let wk_orders = map_of(
        "SELECT strftime('%Y-%W', created_at) wk, COUNT(*)
         FROM catalog_orders WHERE amount_jpy>0 AND status<>'submitting' GROUP BY wk");
    let wk_revenue = map_of(
        "SELECT strftime('%Y-%W', created_at) wk, COALESCE(SUM(amount_jpy),0)
         FROM catalog_orders WHERE amount_jpy>0 AND status<>'submitting' GROUP BY wk");
    // 北極星: その週に「人生初の売上」を迎えた作者の数。
    let wk_first_sale = map_of(&format!(
        "SELECT strftime('%Y-%W', first_at) wk, COUNT(*) FROM (
            SELECT {maker} AS maker, MIN(co.created_at) AS first_at
            FROM catalog_orders co JOIN catalog_products p ON p.sku=co.sku
            WHERE co.amount_jpy>0 AND co.status<>'submitting'
            GROUP BY maker HAVING maker LIKE '%@%') GROUP BY wk", maker = MAKER_SQL));

    let this_week: String = conn.query_row(
        "SELECT strftime('%Y-%W','now')", [], |r| r.get(0)).unwrap_or_default();

    let series: Vec<serde_json::Value> = weeks.iter().map(|w| serde_json::json!({
        "week": w,
        "new_creators": wk_creators.get(w).copied().unwrap_or(0),
        "products_created": wk_products.get(w).copied().unwrap_or(0),
        "first_sale_creators": wk_first_sale.get(w).copied().unwrap_or(0),
        "orders": wk_orders.get(w).copied().unwrap_or(0),
        "revenue_jpy": wk_revenue.get(w).copied().unwrap_or(0),
    })).collect();

    serde_json::json!({
        "north_star": {
            "name": "first_sale_creators_per_week",
            "name_ja": "初売上を経験したクリエイター数/週",
            "this_week": wk_first_sale.get(&this_week).copied().unwrap_or(0),
            "why": "クリエイターが最初の1枚を売れた週 = MUが約束を果たした週。この数が伸びない施策は捨てる。",
        },
        "totals": {
            "creators_verified": creators_verified,
            "products_made": products_made,
            "makers_attributed": makers_attributed,
            "makers_with_sale": makers_with_sale,
            "orders": orders_total,
            "revenue_jpy": revenue_total,
        },
        "weeks": series,
        "honest_note": "ゼロもそのまま出します。数字は catalog_orders / collab_users / mu_credit_ledger の実数。",
    })
}

pub async fn api_kpi(State(db): State<Db>) -> Response {
    let snap = kpi_snapshot(&db);
    ([(axum::http::header::CACHE_CONTROL, "public, max-age=300")], axum::Json(snap)).into_response()
}

const KPI_HTML: &str = r##"<!doctype html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>MU — みんなの数字 (公開KPI)</title>
<meta name="description" content="MUの北極星KPI「初売上を経験したクリエイター数/週」。ゼロもそのまま公開。">
<style>
body{background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,'Helvetica Neue',Arial,sans-serif;margin:0}
.wrap{max-width:760px;margin:0 auto;padding:32px 22px 60px}
.logo{font-size:18px;font-weight:700;letter-spacing:.45em}
.kicker{font-size:11px;letter-spacing:.3em;text-transform:uppercase;color:#ffd700;margin:4px 0 22px}
h1{font-size:22px;font-weight:600;margin:0 0 6px}
p.lead{font-size:13px;opacity:.7;line-height:1.9;margin:0 0 24px}
.north{background:linear-gradient(135deg,rgba(255,215,0,.14),rgba(255,215,0,.03));border:1px solid rgba(255,215,0,.45);border-radius:16px;padding:22px;margin-bottom:26px}
.north .v{font-size:46px;font-weight:800;color:#ffd700;line-height:1}
.north .k{font-size:13px;margin-top:8px;font-weight:700}
.north .why{font-size:12px;opacity:.6;margin-top:8px;line-height:1.8}
.tot{display:grid;grid-template-columns:repeat(auto-fit,minmax(110px,1fr));gap:8px;margin-bottom:28px}
.tot div{background:#111;border:1px solid #222;border-radius:10px;padding:12px}
.tot .v{font-size:18px;font-weight:700;color:#ffd700}
.tot .k{font-size:10px;opacity:.55;margin-top:3px}
table{width:100%;border-collapse:collapse;font-size:12.5px}
td,th{padding:8px 8px;border-bottom:1px solid #1b1b1b;text-align:right}
th{font-size:10px;opacity:.5;letter-spacing:.06em}
td:first-child,th:first-child{text-align:left}
td.star{color:#ffd700;font-weight:800}
.fine{font-size:11px;opacity:.45;margin-top:26px;line-height:1.9}
a{color:#ffd700}
</style></head><body>
<div class="wrap">
<div class="logo">━◯━ MU</div>
<div class="kicker">みんなの数字 — public KPI</div>
<h1>北極星: 初売上を経験したクリエイター数/週</h1>
<p class="lead">MUは「誰でも作って、売れる」場。だから一番大事な数字は売上総額ではなく、<b>初めて自分の作品が売れたクリエイターが今週何人いたか</b>。ゼロもそのまま出します(<a href="/transparency">透明性レポート</a>と同じ流儀)。</p>
<div class="north"><div class="v">__NS__</div><div class="k">今週、初売上を経験したクリエイター</div><div class="why">この数が伸びない施策は捨てる。データは実テーブル(注文・登録・台帳)の生集計。<a href="/api/kpi">JSON</a></div></div>
<div class="tot">
<div><div class="v">__T_CREATORS__</div><div class="k">登録クリエイター</div></div>
<div><div class="v">__T_PRODUCTS__</div><div class="k">生まれた作品</div></div>
<div><div class="v">__T_MAKERS_SALE__</div><div class="k">売上経験ありの作者</div></div>
<div><div class="v">__T_ORDERS__</div><div class="k">累計注文</div></div>
<div><div class="v">¥__T_REVENUE__</div><div class="k">累計売上</div></div>
</div>
<table>
<tr><th>週</th><th>新規クリエイター</th><th>作品</th><th>★初売上作者</th><th>注文</th><th>売上</th></tr>
__ROWS__
</table>
<div class="fine">毎週この表が1行ずつ増えていきます。あなたの行になるかもしれません → <a href="/start">30秒でクリエイター登録</a><br>━◯━ MU · <a href="/shop">SHOP</a> · <a href="/make">作る</a> · <a href="/transparency">透明性</a> · 株式会社イネブラ</div>
</div>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
</body></html>"##;

pub async fn kpi_page(State(db): State<Db>) -> Response {
    let snap = kpi_snapshot(&db);
    let rows: String = snap["weeks"].as_array().map(|ws| ws.iter().rev().map(|w| format!(
        "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"star\">{}</td><td>{}</td><td>¥{}</td></tr>",
        w["week"].as_str().unwrap_or(""),
        w["new_creators"].as_i64().unwrap_or(0),
        w["products_created"].as_i64().unwrap_or(0),
        w["first_sale_creators"].as_i64().unwrap_or(0),
        w["orders"].as_i64().unwrap_or(0),
        fmt_jpy(w["revenue_jpy"].as_i64().unwrap_or(0)),
    )).collect::<String>()).unwrap_or_default();
    let html = KPI_HTML
        .replace("__NS__", &snap["north_star"]["this_week"].as_i64().unwrap_or(0).to_string())
        .replace("__T_CREATORS__", &snap["totals"]["creators_verified"].as_i64().unwrap_or(0).to_string())
        .replace("__T_PRODUCTS__", &snap["totals"]["products_made"].as_i64().unwrap_or(0).to_string())
        .replace("__T_MAKERS_SALE__", &snap["totals"]["makers_with_sale"].as_i64().unwrap_or(0).to_string())
        .replace("__T_ORDERS__", &snap["totals"]["orders"].as_i64().unwrap_or(0).to_string())
        .replace("__T_REVENUE__", &fmt_jpy(snap["totals"]["revenue_jpy"].as_i64().unwrap_or(0)))
        .replace("__ROWS__", &rows);
    ([(axum::http::header::CACHE_CONTROL, "public, max-age=300")], Html(html)).into_response()
}
