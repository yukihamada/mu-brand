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
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<title>MU STUDIO — 30秒で、自分のブランドを持つ</title>
<meta name="description" content="ことば1行で商品が生まれ、売れたら10%があなたに。メール1本でクリエイター登録。">
<meta property="og:title" content="MU STUDIO — 30秒で、自分のブランドを持つ">
<meta property="og:description" content="ことば1行で商品が生まれ、売れたら10%があなたに。">
<meta property="og:image" content="https://wearmu.com/static/og.jpg">
<meta property="og:url" content="https://wearmu.com/start">
<meta name="twitter:card" content="summary_large_image">
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
<h1 id="h1main">1行で、自分のブランドが生まれる。</h1>
<p class="lead" id="leadmain">ことば1行で商品が生まれて、世界中に届く。<br><b style="color:#ffd700">売れたら販売価格の10%があなたに</b>(<a href="/credit">MUクレジットとは — 1cr=¥1・¥3,000以上で振込可</a>)。在庫ゼロ・費用ゼロ・受注生産。</p>
<a href="/make?ref=start" id="tryMake" data-funnel="cta_click" data-funnel-cta="start_try_make" style="display:block;text-align:center;background:#ffd700;color:#0a0a0a;border-radius:8px;font-weight:800;padding:14px;font-size:15px;text-decoration:none;margin-bottom:10px">① まず作ってみる(登録不要・30秒) → /make</a>
<div style="font-size:11.5px;opacity:.55;text-align:center;margin-bottom:14px">↓ もう作った人・ログインしたい人はこちら</div>
<input id="email" type="email" placeholder="you@example.com" autocomplete="email" autofocus value="__PREFILL__">
<button id="send" data-funnel="cta_click" data-funnel-cta="start_send_code" style="background:none;border:1px solid #ffd700;color:#ffd700">② メールで名義をつくる(無料・ログイン兼用・登録後すぐ1行で作れます)</button>
<div class="msg" id="m1"></div>
<div class="steps">
<div><b>1</b>ことば1行で作る(登録不要)</div>
<div><b>2</b>メール1つで<b style="display:inline;font-size:12px">あなたの名義</b>に</div>
<div><b>3</b>売れたら10% · <a href="/studio" style="color:inherit">/studio</a> で管理</div>
</div>
</div>
<div id="step2">
<h1>メールのコードを入力</h1>
<p class="lead"><span id="sentTo"></span> に6桁の確認コードを送りました(15分有効)。届かない時は迷惑メールも確認してください。</p>
<input id="code" inputmode="numeric" pattern="[0-9]*" maxlength="6" placeholder="123456" class="big">
<button id="verify" data-funnel="cta_click" data-funnel-cta="start_verify_code">ログインしてスタジオへ →</button>
<div class="msg" id="m2"></div>
<p style="font-size:12px;opacity:.6"><a href="#" id="resend">コードを再送</a> · <a href="#" id="back">← メールアドレスを入れ直す</a></p>
</div>
<div class="fine">登録はメールアドレスのみ。すでに登録済みでも同じ手順でログインできます。報酬の現金化分は課税所得になる場合があります(<a href="/credit">詳細</a>)。<a href="/privacy">プライバシー</a> · <a href="/tokushoho">特商法</a> · <a href="/kpi">みんなの数字(公開KPI)</a></div>
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
    else if(r.status===429){msg($('m1'),'送信が混み合っています。60秒ほど待ってから再試行してください',true)}else{msg($('m1'),j.error||'送信に失敗しました。少し待って再試行してください',true)}
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
var _rs=0;
$('resend').onclick=async function(e){
  e.preventDefault();
  var now=Date.now();
  if(now-_rs<60000){msg($('m2'),'再送は60秒に1回までです',true);return}
  _rs=now;msg($('m2'),'再送中…');
  try{await fetch('/api/collab/auth/start',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({email:window._em,next:'/studio'})});msg($('m2'),'再送しました。迷惑メールもご確認ください')}catch(_){msg($('m2'),'通信エラー',true)}
};
// 再ログイン文脈(?login=1 / /studioからの307)はオンボーディングコピーを畳む
if(/[?&]login=1/.test(location.search)){
  $('h1main').textContent='おかえりなさい。メールでログイン';
  $('leadmain').innerHTML='登録済みのメールアドレスに6桁コードを送ります。';
  $('tryMake').style.display='none';
  $('send').textContent='ログインコードを送る';
}
</script>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
</body></html>"##;

/// GET /start — クリエイター登録ページ。ログイン済みなら /studio へ。
/// /make のメール認証を済ませた端末(mu_make_email cookie)はメールを
/// プレフィルし、make→start の「もう一度メール入力」を消す。
pub async fn start_page(State(db): State<Db>, headers: HeaderMap) -> Response {
    if crate::collab_session_email(&db, &headers).is_some() {
        return Redirect::temporary("/studio").into_response();
    }
    let prefill = headers.get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|c| c.split(';').find_map(|p| p.trim().strip_prefix("mu_make_email=")))
        .and_then(|v| urlencoding::decode(v).ok())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| s.contains('@') && s.len() <= 254)
        .unwrap_or_default();
    Html(START_HTML.replace("__PREFILL__", &crate::html_escape(&prefill))).into_response()
}

// ════════════════════════════════════════════════════════════════════
// GET /studio — クリエイターダッシュボード
// ════════════════════════════════════════════════════════════════════

const STUDIO_HTML: &str = r##"<!doctype html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
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
<div style="font-size:13px;font-weight:700">作者報酬 — あなたの還元率: <span style="color:#ffd700">販売価格の10%</span></div>
<p style="margin:6px 0 0">今年の受取累計(作者+紹介): <b style="color:#ffd700">¥__YTD__</b> / 確定申告の一般的な目安ライン ¥200,000(<a href="/credit">詳細</a>)</p>
<p>あなたの作品が売れるたび、<b>販売価格の10%</b>(¥4,900のTシャツなら¥490)がMUクレジットで自動的に入ります。全クリエイター一律10%・実績に応じてストア単位で引き上げあり(<a href="/credit">仕組みと根拠</a>)。クレジットは次の購入で使えます。</p>
<button id="po" data-funnel="cta_click" data-funnel-cta="studio_payout">¥3,000以上で銀行振込を申請する</button> <span id="pom" style="font-size:12px;margin-left:6px"></span>
<p>申請後、運営が確認して通常<b>5営業日</b>以内に振込(手数料は当社負担)。台帳に「振込申請」の行が立ち、進捗はメールでお知らせします。現金化分は課税所得になる場合があります(<a href="/credit">詳細</a>)。</p>
__LEDGER__
</div>
<div class="panel">
<div style="font-size:13px;font-weight:700">紹介リンク — 広めても10%</div>
<div class="code" id="refl">__REFLINK__</div>
<button id="refc" data-funnel="cta_click" data-funnel-cta="studio_referral_copy" style="margin-top:2px">リンクをコピー</button> <span id="refm" style="font-size:12px;margin-left:6px"></span>
<p>このリンク経由で<b>誰の商品でも</b>30日以内に売れると、売上の10%があなたに。SNSのプロフィールにどうぞ。<a href="/affiliate/__REFCODE__">実績を見る →</a></p>
</div>
</div>

<h2>公開名(作品ページに「つくった人」として表示)</h2>
<div class="panel">
<input id="dn" maxlength="24" placeholder="例: yuki / 道場の名前 / ニックネーム" value="__DISPLAY_NAME__">
<button id="dnb" data-funnel="cta_click" data-funnel-cta="studio_name_save">保存</button> <span id="dnm" style="font-size:12px;margin-left:8px"></span>
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
$('refc').onclick=function(){var t=$('refl').textContent.trim();navigator.clipboard.writeText(t).then(function(){$('refm').textContent='✓ コピーしました'});};
$('po').onclick=async function(){
  var m=$('pom');this.disabled=true;m.textContent='申請中…';
  try{
    var r=await fetch('/api/studio/payout',{method:'POST',headers:{'content-type':'application/json'},body:'{}'});
    var j=await r.json().catch(function(){return{}});
    m.textContent=(r.ok&&j.ok)?'✓ 受け付けました。5営業日以内に振込します(確認メール送信済み)':(j.error||'申請できませんでした');
  }catch(_){m.textContent='通信エラー'}
  this.disabled=false;
};
</script>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
</body></html>"##;

/// GET /studio — 要ログイン。未ログインは /start へ。
pub async fn studio_page(State(db): State<Db>, headers: HeaderMap) -> Response {
    let Some((email, _collabs, _sub, _verified)) = crate::collab_session_email(&db, &headers) else {
        return Redirect::temporary("/start?login=1").into_response();
    };
    let email_lc = email.to_lowercase();
    let ref_code = crate::referral_code_for(&email_lc);

    struct Prod { sku: String, label: String, price: i64, status: String, img: String }
    let (balance, display_name, products, ledger, n_sales, earned, ytd): (i64, String, Vec<Prod>, Vec<(i64, String, String)>, i64, i64, i64) = {
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
        // 今年の受取累計(作者+紹介の正の付与) — 確定申告の自己評価用。
        let ytd: i64 = conn.query_row(
            "SELECT COALESCE(SUM(delta_jpy),0) FROM mu_credit_ledger
             WHERE email=? AND delta_jpy>0 AND (reason LIKE 'creator:%' OR reason LIKE 'affiliate:%')
             AND created_at >= CAST(strftime('%s', date('now','start of year')) AS INTEGER)",
            rusqlite::params![email_lc], |r| r.get(0)).unwrap_or(0);
        (balance, display_name, products, ledger, n_sales, earned, ytd)
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
        .replace("__YTD__", &fmt_jpy(ytd))
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
// POST /api/studio/payout — 振込申請(アプリ内導線)
// 残高¥3,000以上で受理 → 台帳に申請行(delta=0・監査用) + 運営/本人へメール。
// 実際の振込・残高減算は運営の手動処理(現状の正直な運用のまま、入口だけUI化)。
// ════════════════════════════════════════════════════════════════════

pub async fn studio_payout(State(db): State<Db>, headers: HeaderMap) -> Response {
    let Some((email, ..)) = crate::collab_session_email(&db, &headers) else {
        return (StatusCode::UNAUTHORIZED, axum::Json(serde_json::json!({"ok":false,"error":"ログインしてください"}))).into_response();
    };
    let email_lc = email.to_lowercase();
    let (balance, pending): (i64, i64) = {
        let conn = db.lock().unwrap();
        let bal = crate::mu_credit_balance(&conn, &email_lc);
        // 直近7日の未処理申請があれば二重申請を断る(運営が処理したら台帳で分かる)。
        let pend: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mu_credit_ledger WHERE email=? AND reason='payout_request'
             AND created_at > CAST(strftime('%s','now','-7 days') AS INTEGER)",
            rusqlite::params![email_lc], |r| r.get(0)).unwrap_or(0);
        (bal, pend)
    };
    if balance < 3000 {
        return (StatusCode::BAD_REQUEST, axum::Json(serde_json::json!({
            "ok": false,
            "error": format!("残高¥{}。振込申請は¥3,000以上から(あと¥{})", fmt_jpy(balance), fmt_jpy(3000 - balance)),
        }))).into_response();
    }
    if pending > 0 {
        return (StatusCode::CONFLICT, axum::Json(serde_json::json!({
            "ok": false, "error": "申請済みです(処理中・通常5営業日)。完了メールをお待ちください",
        }))).into_response();
    }
    {
        let conn = db.lock().unwrap();
        crate::mu_credit_apply(&conn, &email_lc, 0, "payout_request", None);
    }
    // 運営+本人への通知(Resend)。鍵が無い環境でも申請自体は台帳に残る。
    let key = std::env::var("RESEND_API_KEY").unwrap_or_default();
    if !key.is_empty() {
        let payload = serde_json::json!({
            "from": "━◯━ MU <info@enablerdao.com>",
            "to": ["info@enablerdao.com"],
            "reply_to": [email_lc.clone()],
            "subject": format!("【MU振込申請】{} 残高¥{}", email_lc, fmt_jpy(balance)),
            "html": format!("<p>振込申請を受け付けました。</p><p>申請者: {}<br>残高: ¥{}</p><p>処理: 台帳確認 → 振込 → mu_credit_ledger に負の行で精算。</p>", email_lc, fmt_jpy(balance)),
        });
        let confirm = serde_json::json!({
            "from": "━◯━ MU <info@enablerdao.com>",
            "to": [email_lc.clone()],
            "subject": "MU — 振込申請を受け付けました",
            "html": format!("<div style=\"font-family:-apple-system,sans-serif;line-height:1.9\"><p>振込申請を受け付けました(残高 ¥{})。</p><p>運営が確認のうえ、通常<b>5営業日</b>以内にお振込みします。振込先口座は折り返しのメールでお伺いします。</p><p>━◯━ MU · wearmu.com/credit</p></div>", fmt_jpy(balance)),
        });
        let k2 = key.clone();
        tokio::spawn(async move {
            let c = reqwest::Client::new();
            let _ = c.post("https://api.resend.com/emails").bearer_auth(&key).json(&payload).send().await;
            let _ = c.post("https://api.resend.com/emails").bearer_auth(&k2).json(&confirm).send().await;
        });
    }
    axum::Json(serde_json::json!({"ok": true, "balance_jpy": balance})).into_response()
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
    let makers_with_attributed_product = q1(
        "SELECT COUNT(DISTINCT LOWER(json_extract(meta_json,'$.maker_email')))
         FROM catalog_products
         WHERE COALESCE(json_extract(meta_json,'$.maker_email'),'') LIKE '%@%'");
    let orders_total = q1("SELECT COUNT(*) FROM catalog_orders WHERE amount_jpy>0 AND status<>'submitting'");
    let revenue_total = q1("SELECT COALESCE(SUM(amount_jpy),0) FROM catalog_orders WHERE amount_jpy>0 AND status<>'submitting'");
    let makers_with_first_sale = q1(&format!(
        "SELECT COUNT(*) FROM (
            SELECT {maker} AS maker FROM catalog_orders co JOIN catalog_products p ON p.sku=co.sku
            WHERE co.amount_jpy>0 AND co.status<>'submitting'
            GROUP BY maker HAVING maker LIKE '%@%')", maker = MAKER_SQL));

    // ── 週バケット: 「今週の月曜」を起点に12週ぶんの [start, end) 日付範囲。
    // strftime('%Y-%W') の週番号ラベルは ISO週とズレて誤読を生むため使わない —
    // 週は常に開始日(月曜・JSTでなくUTC日付)で名指しする。進行中の週も必ず含む。
    let monday: String = conn.query_row(
        "SELECT date('now','weekday 0','-6 days')", [], |r| r.get(0)).unwrap_or_default();
    let as_of: String = conn.query_row(
        "SELECT datetime('now')", [], |r| r.get(0)).unwrap_or_default();
    let bounds: Vec<(String, String)> = (0..12).rev().map(|i| {
        let s: String = conn.query_row(
            "SELECT date(?1, (?2)||' days')", rusqlite::params![monday, -7 * i], |r| r.get(0)).unwrap_or_default();
        let e: String = conn.query_row(
            "SELECT date(?1, '+7 days')", rusqlite::params![s], |r| r.get(0)).unwrap_or_default();
        (s, e)
    }).collect();

    let count_between = |sql: &str, s: &str, e: &str| -> i64 {
        conn.query_row(sql, rusqlite::params![s, e], |r| r.get(0)).unwrap_or(0)
    };
    // 北極星の素材: 作者ごとの「人生初の売上」日時(1クエリ→Rustでバケット)。
    let first_sales: Vec<String> = conn.prepare(&format!(
        "SELECT MIN(co.created_at) FROM catalog_orders co JOIN catalog_products p ON p.sku=co.sku
         WHERE co.amount_jpy>0 AND co.status<>'submitting'
         GROUP BY {maker} HAVING {maker} LIKE '%@%'", maker = MAKER_SQL))
        .ok()
        .and_then(|mut st| st.query_map([], |r| r.get::<_, String>(0))
            .map(|it| it.filter_map(|x| x.ok()).collect()).ok())
        .unwrap_or_default();

    let mut series: Vec<serde_json::Value> = Vec::with_capacity(12);
    let mut ns_this_week: i64 = 0;
    for (s, e) in &bounds {
        let is_current = *s == monday;
        let first_sale_creators = first_sales.iter()
            .filter(|t| t.as_str() >= s.as_str() && t.as_str() < e.as_str()).count() as i64;
        if is_current { ns_this_week = first_sale_creators; }
        series.push(serde_json::json!({
            "week_start": s,
            "week_end_exclusive": e,
            "current": is_current,
            "new_creators": count_between(
                "SELECT COUNT(*) FROM collab_users WHERE verified=1 AND verified_at IS NOT NULL
                 AND datetime(verified_at,'unixepoch') >= ?1 AND datetime(verified_at,'unixepoch') < ?2", s, e),
            "products_created": count_between(
                "SELECT COUNT(*) FROM catalog_products WHERE legacy_source IN ('public_make','agent_api')
                 AND created_at >= ?1 AND created_at < ?2", s, e),
            "first_sale_creators": first_sale_creators,
            "orders": count_between(
                "SELECT COUNT(*) FROM catalog_orders WHERE amount_jpy>0 AND status<>'submitting'
                 AND created_at >= ?1 AND created_at < ?2", s, e),
            "revenue_jpy": count_between(
                "SELECT COALESCE(SUM(amount_jpy),0) FROM catalog_orders WHERE amount_jpy>0 AND status<>'submitting'
                 AND created_at >= ?1 AND created_at < ?2", s, e),
            // 北極星の手前のリーディング指標(mu-funnel.js 実イベント)。
            // funnel_events.created_at は unix epoch の TEXT。
            "visitors": count_between(
                "SELECT COUNT(DISTINCT visitor_id) FROM funnel_events WHERE event='pageview'
                 AND datetime(CAST(created_at AS INTEGER),'unixepoch') >= ?1
                 AND datetime(CAST(created_at AS INTEGER),'unixepoch') < ?2", s, e),
            "registrations": count_between(
                "SELECT COUNT(*) FROM funnel_events WHERE event='you_register'
                 AND datetime(CAST(created_at AS INTEGER),'unixepoch') >= ?1
                 AND datetime(CAST(created_at AS INTEGER),'unixepoch') < ?2", s, e),
            "shares": count_between(
                "SELECT COUNT(*) FROM funnel_events WHERE event='share'
                 AND datetime(CAST(created_at AS INTEGER),'unixepoch') >= ?1
                 AND datetime(CAST(created_at AS INTEGER),'unixepoch') < ?2", s, e),
            "checkout_attempt": count_between(
                "SELECT COUNT(*) FROM funnel_events WHERE event='checkout_attempt'
                 AND datetime(CAST(created_at AS INTEGER),'unixepoch') >= ?1
                 AND datetime(CAST(created_at AS INTEGER),'unixepoch') < ?2", s, e),
            "checkout_start": count_between(
                "SELECT COUNT(*) FROM funnel_events WHERE event='checkout_start'
                 AND datetime(CAST(created_at AS INTEGER),'unixepoch') >= ?1
                 AND datetime(CAST(created_at AS INTEGER),'unixepoch') < ?2", s, e),
        }));
    }

    // 確定済み直近週(進行中の1つ前)の北極星値 — 暫定値との誤読を防ぐ。
    let ns_last_confirmed: i64 = series.iter().rev().nth(1)
        .and_then(|w| w["first_sale_creators"].as_i64()).unwrap_or(0);
    let ns_last_week_start: String = series.iter().rev().nth(1)
        .and_then(|w| w["week_start"].as_str()).unwrap_or("").to_string();
    serde_json::json!({
        "north_star": {
            "name": "first_sale_creators_per_week",
            "name_ja": "初売上を経験したクリエイター数/週",
            "this_week": ns_this_week,
            "this_week_is_partial": true,
            "last_confirmed_week": ns_last_confirmed,
            "last_confirmed_week_start": ns_last_week_start,
            "week_start": monday,
            "as_of": as_of,
            "why": "クリエイターが最初の1枚を売れた週 = MUが約束を果たした週。この数が伸びない施策は捨てる。",
        },
        "totals": {
            "creators_verified": creators_verified,
            "products_made": products_made,
            "makers_with_attributed_product": makers_with_attributed_product,
            "makers_with_first_sale": makers_with_first_sale,
            "orders": orders_total,
            "revenue_jpy": revenue_total,
        },
        "definitions": {
            "creators_verified": "現在メール認証済み(verified=1)のクリエイター総数。スナップショット値なので、認証解除があると weeks.new_creators の合計と乖離しうる",
            "new_creators": "その週に初めてメール認証を完了した(verified_at がその週に入る)クリエイター数",
            "products_made": "クリエイター/エージェント発の作品数 (legacy_source: public_make + agent_api)",
            "makers_with_attributed_product": "作者刻印(meta_json.maker_email)付き作品を持つ作者数。agentストアのオーナー帰属はここに含まれない(売上集計には含む)ため makers_with_first_sale より小さくなりうる",
            "makers_with_first_sale": "1件以上売れた作者数(刻印 or agentストアオーナー)。各作者は生涯最初の売上(MIN(created_at))が属する週に一度だけ first_sale_creators として数えられるため、Σ weeks.first_sale_creators = この値 が定義上常に成立する(週次は『その週が生涯初売上だった作者数』であり、週次アクティブ作者数ではない)",
            "orders": "カタログ注文数 (amount>0・予約中除く)",
            "revenue_jpy": "クリエイターループ(カタログ注文)の累計売上。MU全体の売上(オークション/MUGEN等含む)は /transparency 参照 — 本数値はその部分集合",
            "weeks": "週は月曜開始のUTC日付範囲 [week_start, week_end_exclusive)。current=true は進行中の週(数字はまだ増える)",
            "visitors": "その週のユニーク訪問者数 (funnel_events pageview の distinct visitor_id)",
            "registrations": "その週の登録完了イベント数 (you_register — 6桁コード認証成功時にクライアント発火。広告ブロッカー等で実登録より少なく出ることがある: 真実源は new_creators)",
            "shares": "その週のシェア操作数 (PDP/作者ページの share イベント)",
            "checkout_attempt_vs_start": "attempt=クライアントの購入ボタン押下 / start=サーバーのStripeセッション作成。差分が大きい週はチェックアウト導線の故障シグナル",
        },
        "weeks": series,
        "honest_note": "ゼロもそのまま出します。数字は catalog_orders / collab_users / mu_credit_ledger の実数。",
    })
}

pub async fn api_kpi(State(db): State<Db>, headers: HeaderMap) -> Response {
    let snap = kpi_snapshot(&db);
    // 内容ハッシュ ETag — 週次集計は変化が遅いので 304 で帯域を節約する。
    // as_of(秒precision)はハッシュから除外しないと毎回変わるため、bodyの
    // ハッシュは as_of を抜いた安定部分で取る。
    let mut stable = snap.clone();
    if let Some(ns) = stable.get_mut("north_star").and_then(|v| v.as_object_mut()) { ns.remove("as_of"); }
    let etag = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(stable.to_string().as_bytes());
        format!("\"{}\"", hex::encode(&h.finalize()[..16]))
    };
    if headers.get(axum::http::header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(|v| v == etag)
        .unwrap_or(false)
    {
        return (StatusCode::NOT_MODIFIED,
                [(axum::http::header::ETAG, etag),
                 (axum::http::header::CACHE_CONTROL, "public, max-age=60, stale-while-revalidate=300".to_string())]).into_response();
    }
    ([(axum::http::header::ETAG, etag),
      (axum::http::header::CACHE_CONTROL, "public, max-age=60, stale-while-revalidate=300".to_string())],
     axum::Json(snap)).into_response()
}

const KPI_HTML: &str = r##"<!doctype html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
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
<div class="north"><div class="v">__NS__<span style="font-size:14px;font-weight:600;opacity:.6;margin-left:8px;vertical-align:middle">(暫定・集計中)</span></div><div class="k">今週 (__NSWEEK__ 月曜〜・進行中)、初売上を経験したクリエイター <span style="opacity:.6">— 先週(__NSLASTWEEK__〜・確定): __NSLAST__人</span></div><div class="why">この数が伸びない施策は捨てる。データは実テーブル(注文・登録・台帳)の生集計 (__ASOF__ UTC 時点)。<a href="/api/kpi">JSON</a> に全定義あり。</div></div>
<div class="tot">
<div><div class="v">__T_CREATORS__</div><div class="k">登録クリエイター</div></div>
<div><div class="v">__T_PRODUCTS__</div><div class="k">生まれた作品</div></div>
<div><div class="v">__T_MAKERS_SALE__</div><div class="k">売上経験ありの作者</div></div>
<div><div class="v">__T_ORDERS__</div><div class="k">累計注文</div></div>
<div><div class="v">¥__T_REVENUE__</div><div class="k">累計売上 (ループ分)</div></div>
</div>
<p style="font-size:11.5px;opacity:.55;line-height:1.8;margin:-14px 0 24px">※ ここの売上は<b>クリエイターループ(カタログ注文)のみ</b>。オークション・MUGEN 等を含む MU 全体の売上は <a href="/transparency">/transparency</a> に別掲(本数値はその部分集合)。週は月曜はじまり・最下行は進行中の週。</p>
<details style="margin:0 0 18px;font-size:12px;opacity:.75;line-height:1.9"><summary style="cursor:pointer;color:#ffd700">この数字の読み方(定義と注意)</summary>
<ul style="margin:8px 0 0;padding-left:18px">
<li><b>★初売上作者</b> = その週に「人生初」の売上を記録した作者数。各作者は一度だけ数えられるので、全週合計=「売上経験ありの作者」と常に一致します。</li>
<li><b>進行中の週</b>(最下行・ヒーローの数字)はまだ増えます。確定値と区別してください。</li>
<li><b>イベント計測</b>(訪問・登録・シェア等のファネル値)は広告ブロッカー等で実数より少なく出ることがあります。登録の真実源は認証テーブル(新規クリエイター列)です。</li>
<li>売上はクリエイターループ(カタログ注文)のみ。MU全体は <a href="/transparency">/transparency</a>(本数値はその部分集合)。</li>
</ul></details>
<table>
<tr><th>週 (月曜〜)</th><th>新規クリエイター</th><th>作品</th><th>★初売上作者</th><th>注文</th><th>売上</th></tr>
__ROWS__
</table>
<div class="fine">毎週この表が1行ずつ増えていきます。あなたの行になるかもしれません → <a href="/start?ref=kpi" data-funnel="cta_click" data-funnel-cta="kpi_start">クリエイター登録(無料)</a> · <a href="/make?ref=kpi" data-funnel="cta_click" data-funnel-cta="kpi_make">まず1行で作ってみる</a><br>━◯━ MU · <a href="/shop">SHOP</a> · <a href="/make">作る</a> · <a href="/credit">MUクレジット</a> · <a href="/transparency">透明性</a> · <a href="/returns">返品</a> · <a href="/tokushoho">特商法</a> · <a href="/privacy">プライバシー</a> · 株式会社イネブラ</div>
</div>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
</body></html>"##;

pub async fn kpi_page(State(db): State<Db>) -> Response {
    let snap = kpi_snapshot(&db);
    let rows: String = snap["weeks"].as_array().map(|ws| ws.iter().rev().map(|w| format!(
        "<tr><td>{}{}</td><td>{}</td><td>{}</td><td class=\"star\">{}</td><td>{}</td><td>¥{}</td></tr>",
        w["week_start"].as_str().unwrap_or(""),
        if w["current"].as_bool().unwrap_or(false) { " <span style=\"color:#ffd700;font-size:10px\">(進行中)</span>" } else { "" },
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
        .replace("__T_MAKERS_SALE__", &snap["totals"]["makers_with_first_sale"].as_i64().unwrap_or(0).to_string())
        .replace("__T_ORDERS__", &snap["totals"]["orders"].as_i64().unwrap_or(0).to_string())
        .replace("__T_REVENUE__", &fmt_jpy(snap["totals"]["revenue_jpy"].as_i64().unwrap_or(0)))
        .replace("__NSWEEK__", snap["north_star"]["week_start"].as_str().unwrap_or("?"))
        .replace("__NSLAST__", &snap["north_star"]["last_confirmed_week"].as_i64().unwrap_or(0).to_string())
        .replace("__NSLASTWEEK__", snap["north_star"]["last_confirmed_week_start"].as_str().unwrap_or("?"))
        .replace("__ASOF__", snap["north_star"]["as_of"].as_str().unwrap_or("?"))
        .replace("__ROWS__", &rows);
    ([(axum::http::header::CACHE_CONTROL, "public, max-age=60, stale-while-revalidate=300")], Html(html)).into_response()
}

// ════════════════════════════════════════════════════════════════════
// GET /credit — MUクレジットの定義(公開・ログイン不要)
// 「10%って何でもらえるの?」に登録前に答える1枚。法的位置づけも正直に。
// ════════════════════════════════════════════════════════════════════

const CREDIT_HTML: &str = r##"<!doctype html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<title>MUクレジットとは — 売れたら10%、の中身</title>
<meta name="description" content="MUクレジット: 1クレジット=¥1としてMUの決済で使える残高。¥3,000以上で銀行振込に交換可・有効期限なし。仕組みを全部公開。">
<meta property="og:title" content="MUクレジットとは — 売れたら10%、の中身">
<meta property="og:description" content="1クレジット=¥1。決済で使える・¥3,000以上で振込申請可・期限なし。">
<style>
body{background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,'Helvetica Neue',Arial,sans-serif;margin:0}
.wrap{max-width:680px;margin:0 auto;padding:32px 22px 60px}
.logo{font-size:18px;font-weight:700;letter-spacing:.45em}
.kicker{font-size:11px;letter-spacing:.3em;text-transform:uppercase;color:#ffd700;margin:4px 0 22px}
h1{font-size:23px;font-weight:600;margin:0 0 8px}
h2{font-size:15px;font-weight:700;color:#ffd700;margin:28px 0 8px}
p,li{font-size:13.5px;line-height:1.95;opacity:.85}
table{width:100%;border-collapse:collapse;font-size:13px;margin:10px 0}
td,th{padding:10px;border-bottom:1px solid #1c1c1c;text-align:left;vertical-align:top}
th{width:160px;color:#ffd700;font-weight:700;font-size:12px}
.cta{display:inline-block;background:#ffd700;color:#0a0a0a;border-radius:8px;font-weight:800;padding:13px 28px;font-size:14px;text-decoration:none;margin-top:18px}
a{color:#ffd700}
.fine{font-size:11px;opacity:.45;margin-top:30px;line-height:1.9}
</style></head><body>
<div class="wrap">
<div class="logo">━◯━ MU</div>
<div class="kicker">MU CREDIT — 売れたら10%、の中身</div>
<h1>MUクレジットとは</h1>
<p>あなたの作品が売れるたび<b>売上の10%</b>、紹介リンク(<a href="/affiliate">/affiliate</a>)経由で誰かの作品が売れても<b>売上の10%</b>が、自動であなたの「MUクレジット」になります。<b>2つの10%は別枠</b>です: 他人があなたの紹介リンク経由で「あなたの作品」を買うと作者10%+紹介10%の両方が入ります(上限なし)。自分で自分の作品を買った場合はどちらも付きません(自己購入除外)。</p>
<table>
<tr><th>価値</th><td>1クレジット = ¥1 相当。</td></tr>
<tr><th>使い道</th><td>① MU の決済でそのまま値引きに使える ② 残高 <b>¥3,000以上</b>で<b>銀行振込(現金)</b>への交換を申請できる。</td></tr>
<tr><th>振込の手順</th><td><a href="/studio">/studio</a> の<b>「振込申請」ボタン</b>を押すだけ(残高¥3,000以上で有効・不足時は「残高¥X。あと¥Y」と表示・7日以内の二重申請は不可)。受付すると運営とあなたの両方にメールが届き、通常<b>5営業日</b>以内に振込(手数料は当社負担)。/studio が使えない場合のみ <a href="mailto:info@enablerdao.com?subject=MU%20%E6%8C%AF%E8%BE%BC%E7%94%B3%E8%AB%8B">info@enablerdao.com</a> へ。</td></tr>
<tr><th>有効期限</th><td>なし。</td></tr>
<tr><th>確認方法</th><td><a href="/studio">/studio</a> に残高と入出金の台帳(いつ・どの作品で・いくら)が出ます。</td></tr>
<tr><th>法的な位置づけ</th><td>MUクレジットは当社が役務の対価として無償付与する社内ポイントで、<b>販売はしていません</b>(購入できるポイントではないため、資金決済法上の前払式支払手段に該当しない運用)。報酬の付与率(10%)は実装コード・台帳とも公開で、誇大表示をしない方針です(<a href="/transparency">/transparency</a>)。</td></tr>
<tr><th>対象外</th><td>自分の作品を自分で買った分には付きません(自己購入除外)。不正・規約違反時は取り消すことがあります。</td></tr>
<tr><th>税金</th><td>クレジットの付与・現金化は、受け取った方の<b>課税所得になる場合があります</b>。一般に給与所得者は副収入が年20万円を超えると確定申告が必要になる場合があります — 正確な判断は税務署・税理士にご確認ください(ログインすると <a href="/studio">/studio</a> に今年の受取累計と申告目安ラインが常時表示されます)。</td></tr>
</table>
<h2>10%の計算基準と根拠</h2>
<p><b>付与額 = 販売価格(税込)の10%</b>です。¥4,900のTシャツなら¥490 — PDPに表示される実額と同じ計算で、粗利の10%ではありません。受注生産(Printful)の原価・送料・決済手数料はMU側の取り分から負担し、作者10%・紹介者10%を<b>売上から先取り</b>で配る設計です。現在の料率は全クリエイター一律10%(あなたの実効率は <a href="/studio">/studio</a> にも表示)。ストア単位で引き上げる仕組み(maker_pct)があり、実績に応じて見直します。変更時はこのページと <a href="/kpi">/kpi</a> で告知します。</p>
<a class="cta" href="/start?ref=credit">30秒でクリエイター登録 →</a>
<div class="fine">━◯━ MU · <a href="/start">作って売る</a> · <a href="/kpi">公開KPI</a> · <a href="/tokushoho">特定商取引法</a> · <a href="/returns">返品</a> · <a href="/privacy">プライバシー</a> · 株式会社イネブラ</div>
</div>
<script defer src="/mu-funnel.js"></script>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
</body></html>"##;

pub async fn credit_page() -> Response {
    ([(axum::http::header::CACHE_CONTROL, "public, max-age=600")], Html(CREDIT_HTML)).into_response()
}

// ════════════════════════════════════════════════════════════════════
// GET /u/:code — 作者の公開ポートフォリオ。code は referral_code_for(email)
// (安定・非PII)。PDP byline からここに飛び、作者の全作品が並ぶ =
// 「1作者1ブランド」のハブ。メールアドレスは一切出さない。
// ════════════════════════════════════════════════════════════════════

pub async fn maker_page(
    State(db): State<Db>,
    axum::extract::Path(code): axum::extract::Path<String>,
) -> Response {
    let code_clean: String = code.chars().filter(|c| c.is_ascii_alphanumeric())
        .take(8).collect::<String>().to_uppercase();
    if code_clean.len() < 4 {
        return (StatusCode::NOT_FOUND, "maker not found").into_response();
    }
    struct Prod { sku: String, label: String, price: i64, img: String }
    let found: Option<(String, Vec<Prod>, i64)> = {
        let conn = db.lock().unwrap();
        let email: Option<String> = conn.query_row(
            "SELECT owner_email FROM mu_referrals WHERE code=?",
            rusqlite::params![code_clean], |r| r.get(0)).ok().flatten();
        email.map(|em| {
            let em = em.to_lowercase();
            let dn: String = conn.query_row(
                "SELECT COALESCE(display_name,'') FROM collab_users WHERE email=?",
                rusqlite::params![em], |r| r.get(0)).unwrap_or_default();
            let products: Vec<Prod> = conn.prepare(&format!(
                "SELECT p.sku, p.label, p.retail_price_jpy,
                        COALESCE(p.mockup_url_external, p.mockup_main_file, p.design_file, '')
                 FROM catalog_products p
                 WHERE {maker} = ?1 AND p.is_active=1 AND p.status='live'
                 ORDER BY p.created_at DESC LIMIT 60", maker = MAKER_SQL))
                .ok()
                .and_then(|mut s| s.query_map(rusqlite::params![em], |r| Ok(Prod {
                    sku: r.get(0)?, label: r.get(1)?, price: r.get(2)?, img: r.get(3)?,
                })).map(|it| it.filter_map(|x| x.ok()).collect()).ok())
                .unwrap_or_default();
            let sales: i64 = conn.query_row(&format!(
                "SELECT COUNT(*) FROM catalog_orders co JOIN catalog_products p ON p.sku=co.sku
                 WHERE co.amount_jpy>0 AND co.status<>'submitting' AND {maker} = ?1", maker = MAKER_SQL),
                rusqlite::params![em], |r| r.get(0)).unwrap_or(0);
            (dn, products, sales)
        })
    };
    let Some((dn, products, sales)) = found else {
        return (StatusCode::NOT_FOUND, Html(
            r#"<!doctype html><html lang="ja"><body style="background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,sans-serif;text-align:center;padding:80px 24px"><div style="letter-spacing:.45em;font-weight:700">━◯━ MU</div><p style="opacity:.7;margin-top:20px">この作者ページはまだありません。</p><p><a href="/start" style="color:#ffd700">あなたが最初の1ページを作りますか? →</a></p></body></html>"#,
        )).into_response();
    };
    let who = if dn.trim().is_empty() { "MU クリエイター".to_string() } else { crate::html_escape(dn.trim()) };
    let cards: String = products.iter().map(|p| format!(
        r#"<a class="card" href="/shop/{sku}?ref={code}" data-funnel="cta_click" data-funnel-cta="portfolio_pdp"><img src="{img}" alt="" loading="lazy"><div class="b"><div class="t">{label}</div><div class="p">¥{price}</div></div></a>"#,
        sku = crate::html_escape(&p.sku), img = crate::html_escape(&p.img),
        label = crate::html_escape(p.label.chars().take(40).collect::<String>().as_str()),
        price = fmt_jpy(p.price), code = crate::html_escape(&code_clean))).collect();
    let grid = if products.is_empty() {
        r#"<p style="opacity:.6;font-size:13px">公開中の作品はまだありません。</p>"#.to_string()
    } else { format!(r#"<div class="grid">{}</div>"#, cards) };
    let html = format!(r##"<!doctype html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<title>{who} — MU クリエイター</title>
<meta name="description" content="{who} の作品 {n} 点。ことば1行から生まれた一点もの。">
<meta property="og:title" content="{who} — MU クリエイター">
<meta property="og:description" content="作品 {n} 点・売れた数 {sales}。ことば1行から30秒、あなたもブランドを持てる。">
<meta property="og:image" content="{og_img}">
<meta property="og:url" content="https://wearmu.com/u/{code}">
<meta name="twitter:card" content="summary_large_image">
<style>
body{{background:#0a0a0a;color:#f5f5f0;font-family:-apple-system,'Helvetica Neue',Arial,sans-serif;margin:0}}
.wrap{{max-width:920px;margin:0 auto;padding:32px 22px 60px}}
.logo{{font-size:18px;font-weight:700;letter-spacing:.45em}}
h1{{font-size:24px;font-weight:600;margin:18px 0 4px}}
.sub{{font-size:12.5px;opacity:.6;margin-bottom:24px}}
.grid{{display:grid;grid-template-columns:repeat(auto-fill,minmax(160px,1fr));gap:12px}}
.card{{background:#111;border:1px solid #222;border-radius:12px;overflow:hidden;text-decoration:none;color:#f5f5f0;display:block}}
.card img{{width:100%;aspect-ratio:1;object-fit:cover;background:#0d0d0d;display:block}}
.card .b{{padding:10px 12px}} .card .t{{font-size:12.5px;line-height:1.5;max-height:3em;overflow:hidden}}
.card .p{{font-size:13px;color:#ffd700;font-weight:700;margin-top:4px}}
.cta{{display:inline-block;background:#ffd700;color:#0a0a0a;border-radius:8px;font-weight:800;padding:12px 26px;font-size:14px;text-decoration:none;margin-top:30px}}
a{{color:#ffd700}} footer{{font-size:11px;opacity:.45;margin-top:40px;line-height:1.9}}
</style></head><body><div class="wrap">
<div class="logo">━◯━ MU</div>
<h1>{who}</h1>
<div class="sub">作品 {n} 点 · 売れた数 {sales} · <b>全作品 AI画像生成</b>(ことば1行 → AIが描く・人が選ぶ)。</div>
<div style="display:flex;gap:8px;align-items:center;margin:0 0 18px;font-size:12.5px;flex-wrap:wrap">
<span style="opacity:.55">このブランドを広める:</span>
<a href="https://x.com/intent/tweet?text={share_text}&url={share_url}" target="_blank" rel="noopener" data-funnel="share" data-funnel-cta="portfolio_share_x" style="color:#f5f5f0;text-decoration:none;border:1px solid #3a3a3a;border-radius:99px;padding:6px 14px">𝕏 ポスト</a>
<a href="https://social-plugins.line.me/lineit/share?url={share_url}" target="_blank" rel="noopener" data-funnel="share" data-funnel-cta="portfolio_share_line" style="color:#f5f5f0;text-decoration:none;border:1px solid #3a3a3a;border-radius:99px;padding:6px 14px">LINE</a>
<button id="shareBtn" data-funnel="share" data-funnel-cta="portfolio_share_native" style="background:none;color:#f5f5f0;border:1px solid #3a3a3a;border-radius:99px;padding:6px 14px;cursor:pointer;font-size:12.5px;font-family:inherit">リンクをコピー</button>
</div>
{grid}
<script>(function(){{var b=document.getElementById('shareBtn');if(!b)return;b.addEventListener('click',function(){{
if(navigator.share){{navigator.share({{url:location.href}}).catch(function(){{}});}}
else{{navigator.clipboard.writeText(location.href).then(function(){{b.textContent='✓ コピーしました';}});}}
}});}})();</script>
<a class="cta" href="/start?ref=maker_page" data-funnel="cta_click" data-funnel-cta="maker_page_start">あなたも30秒で作って、売れたら10%受け取る →</a>
<div style="font-size:12px;opacity:.65;margin-top:10px">売上の10%があなたのMUクレジットに(1cr=¥1・¥3,000以上で振込可 — <a href="/credit">仕組み</a>)</div>
<footer>━◯━ MU · <a href="/shop">SHOP</a> · <a href="/make">作る</a> · <a href="/credit">MUクレジットとは</a> · <a href="/kpi">公開KPI</a> · 株式会社イネブラ</footer>
</div>
<script defer src="/mu-funnel.js"></script>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
</body></html>"##,
        who = who, n = products.len(), sales = sales, grid = grid,
        code = crate::html_escape(&code_clean),
        // シェアは作者の一人称: 「私のブランドできた」をそのまま貼れる形に。
        share_text = urlencoding::encode(&format!("{} のMUブランド(作品{}点) — ことば1行から30秒 #MU #wearmu", who, products.len())),
        share_url = urlencoding::encode(&format!("https://wearmu.com/u/{}?ref=share_portfolio", code_clean)),
        // OG画像 = 代表作のモックアップ。無ければブランド既定にフォールバック。
        og_img = crate::html_escape(
            products.iter().map(|p| p.img.as_str()).find(|u| u.starts_with("https://"))
                .unwrap_or("https://wearmu.com/static/og-default.png")));
    Html(html).into_response()
}
