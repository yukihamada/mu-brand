// work.rs — 在宅ワーカー向けフルフィルメント・ジョブ基盤。
//
// 2系統のジョブ:
//   job_kind='oto'   … NFC音コイン。書込→検品→梱包→発送(従来)。NFC対応端末が必要。
//   job_kind='unbox' … 開封体験パック。Printful直送品を受け取り→検品→薄紙+MU封緘
//                        +手書きカード(任意)→完成写真→投函。NFC不要・手書きOK。
//
//   /work               … 求人LP(公開)。2つの入口(音コイン/仕上げて届ける人)
//   POST /api/work/apply … 応募(承認待ち) → Telegramで運営に通知
//   GET  /admin/work/approve?token=&id= … 運営承認 → worker_token発行+メール
//   GET  /work/queue?token= … ワーカー専用キュー(着手/証跡提出)
//   POST /api/work/claim … 仕事を引き受ける(原子的: manual_pending→manual_assigned)
//   POST /api/work/proof … 完成写真を提出(EXIF/GPS除去・R2保存) → review_state='proof_submitted'
//   POST /api/work/ship  … 追跡番号で提出完了(oto) → review_state='proof_submitted'(報酬は未確定)
//   GET  /admin/work/review?token= … 運営が証跡を承認 → 報酬released(現金+糸仮計上)
//   GET  /work/payouts?token= … ワーカーの報酬明細(現金/糸を分離・確定申告用)
//   GET  /admin/work/payout_sheet?token= … 運営用・work_cash集計(実振込は人間)
//
// 注文ステータスは catalog_orders.status を単一ソースにする(契約準拠):
//   manual_pending → manual_assigned → manual_shipped
// ワーカー帰属・報酬は work_assignments(注文1行=1ジョブ)に持つ。
//
// ── 安全の土台(ブラインド配送) ──────────────────────────────────────────
// ・住所の単一ソースは catalog_orders.shipping_address_json のみ。
//   表示は常に render_addr() 経由で trust_tier ゲートする。
//   work_proofs/work_ito_grants/work_audit/work_assignments に住所・氏名は持たない。
// ・trust_tier 0(新人)=ハブ経由・full住所を一切見せない(箱に住所が物理的に無い)。
//   trust_tier 1+ かつ claim者本人のみ full住所(直送・宛名印字用)。
// ・proof写真は受信時に image crateで decode→PNG 再エンコードしEXIF/GPSを必ず除去
//   (剥がせなければ受理拒否=fail-closed)。
// ・Telegram通知は worker名でなく worker_id(双方の身元保護)。
// ・現金は mu_credit_ledger(reason='work_cash', ref_id='order:{id}' 冪等)に記帳。
//   糸(ITO)は別台帳 work_ito_grants に粒数で記帳(delta_jpy には絶対入れない)。

use axum::{
    extract::{Form, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use std::env;

use crate::Db;

fn fee_jpy() -> i64 {
    // 既定 ¥400/件: retail¥1,800・送料当社負担の収支(貢献利益≈¥1,410)で粗利56%を残しつつ、
    // 現実5〜10分/件→時給¥2,400〜4,800で最賃を確実に超える均衡点。env で上書き可。
    env::var("WORK_FEE_JPY").ok().and_then(|v| v.parse().ok()).unwrap_or(400)
}

/// unbox(開封体験パック)の報酬。手書きカード等で手間が増えるため oto より高め。
fn unbox_fee_jpy() -> i64 {
    env::var("WORK_UNBOX_FEE_JPY").ok().and_then(|v| v.parse().ok()).unwrap_or(700)
}

/// job_kind に応じた報酬単価。
fn fee_for(job_kind: &str) -> i64 {
    if job_kind == "unbox" { unbox_fee_jpy() } else { fee_jpy() }
}

/// unbox 1件で付与する糸の粒数(10粒=1着)。糸本体PR#109マージまで仮計上。
fn ito_grains_for(job_kind: &str) -> i64 {
    if job_kind == "unbox" { 3 } else { 0 }
}

fn ensure_tables(conn: &rusqlite::Connection) {
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS work_workers (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            email       TEXT UNIQUE NOT NULL,
            name        TEXT NOT NULL,
            region      TEXT,
            token       TEXT UNIQUE,
            status      TEXT NOT NULL DEFAULT 'pending',
            created_at  TEXT DEFAULT (datetime('now')),
            approved_at TEXT,
            about       TEXT
         );
         CREATE TABLE IF NOT EXISTS work_assignments (
            order_id   INTEGER PRIMARY KEY,
            worker_id  INTEGER NOT NULL,
            fee_jpy    INTEGER NOT NULL,
            claimed_at TEXT DEFAULT (datetime('now')),
            shipped_at TEXT,
            tracking   TEXT
         );",
    );
    // 既存の work_workers に about 列を追加（無ければ）。冪等・既にあればエラーは無視。
    let _ = conn.execute("ALTER TABLE work_workers ADD COLUMN about TEXT", []);
    // 価格A/B: 応募時に見た提示単価(¥200/¥300)。Tシャツ等の汎用ジョブ単価に使う(音コインは fee_jpy() 固定)。
    let _ = conn.execute("ALTER TABLE work_workers ADD COLUMN rate_jpy INTEGER", []);
    // 完成写真URL(LPの「写真で承認」の実装)と月次振込の二重払い防止マーカー。
    let _ = conn.execute("ALTER TABLE work_assignments ADD COLUMN photo_url TEXT", []);
    let _ = conn.execute("ALTER TABLE work_assignments ADD COLUMN paid_at TEXT", []);
    // ── additive 拡張(20260609_1_work_unbox.sql と同内容)。SQLite は重複ALTERで
    //    err を返すが戻り値を swallow するため起動毎の再適用は冪等。PII列は足さない。
    for stmt in [
        "ALTER TABLE work_workers ADD COLUMN nfc_capable  INTEGER NOT NULL DEFAULT 1",
        "ALTER TABLE work_workers ADD COLUMN trust_tier   INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE work_workers ADD COLUMN kyc_state    TEXT NOT NULL DEFAULT 'none'",
        "ALTER TABLE work_workers ADD COLUMN payout_hash  TEXT",
        "ALTER TABLE work_workers ADD COLUMN flagged      INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE work_assignments ADD COLUMN job_kind     TEXT NOT NULL DEFAULT 'oto'",
        "ALTER TABLE work_assignments ADD COLUMN review_state TEXT NOT NULL DEFAULT 'claimed'",
        "ALTER TABLE work_assignments ADD COLUMN ito_grains   INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE work_assignments ADD COLUMN approved_at  TEXT",
    ] {
        let _ = conn.execute(stmt, []);
    }
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS work_proofs (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            order_id      INTEGER NOT NULL,
            worker_id     INTEGER NOT NULL,
            stage         TEXT NOT NULL,
            object_key    TEXT NOT NULL,
            sha256        TEXT,
            exif_stripped INTEGER NOT NULL DEFAULT 0,
            pii_clear     INTEGER NOT NULL DEFAULT 0,
            is_public     INTEGER NOT NULL DEFAULT 0,
            created_at    INTEGER NOT NULL DEFAULT (strftime('%s','now'))
         );
         CREATE INDEX IF NOT EXISTS idx_work_proofs_order ON work_proofs(order_id);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_work_proofs_sha ON work_proofs(sha256) WHERE sha256 IS NOT NULL;
         CREATE TABLE IF NOT EXISTS work_ito_grants (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            worker_id  INTEGER NOT NULL,
            order_id   INTEGER NOT NULL,
            grains     INTEGER NOT NULL,
            ref_id     TEXT NOT NULL,
            settled    INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
         );
         CREATE UNIQUE INDEX IF NOT EXISTS idx_work_ito_ref ON work_ito_grants(ref_id);
         CREATE TABLE IF NOT EXISTS work_audit (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            worker_id   INTEGER,
            order_id    INTEGER,
            event       TEXT NOT NULL,
            client_ip   TEXT,
            detail_json TEXT,
            created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
         );",
    );
}

/// 監査ログを1行記録(PIIは入れない)。
fn audit(conn: &rusqlite::Connection, worker_id: Option<i64>, order_id: Option<i64>, event: &str, client_ip: &str, detail: &str) {
    let _ = conn.execute(
        "INSERT INTO work_audit (worker_id, order_id, event, client_ip, detail_json) VALUES (?,?,?,?,?)",
        rusqlite::params![worker_id, order_id, event, client_ip, detail],
    );
}

/// description_ja の "oto.html?s=KEY" 規約から NFC 書込URLを得る
/// (catalog.rs manual ルートと同じ規約)。
fn encode_url_of(desc: &str) -> Option<String> {
    desc.find("oto.html?s=").map(|p| &desc[p + "oto.html?s=".len()..]).and_then(|rest| {
        let k: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if k.is_empty() { None } else { Some(format!("https://mu.koe.live/oto.html?s={}", k)) }
    })
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Google Ads グローバルタグ (キャンペーン MU_WORK_Recruit のCV計測)。
/// /work 系LPと応募完了ページの head に入れる。tracking.js は env設定時のみ
/// 動的ロードのため、you.html と同じ「静的に必ず入れる」パターンで確実に計測する
/// (gtag.js 側は同一IDの二重 config を無害に扱う・you.html で実績あり)。
const GTAG_HEAD: &str = r#"<script async src="https://www.googletagmanager.com/gtag/js?id=AW-17814724474"></script>
<script>window.dataLayer=window.dataLayer||[];function gtag(){dataLayer.push(arguments);}gtag('js',new Date());gtag('config','AW-17814724474');</script>"#;

fn page(title: &str, body: &str) -> Response {
    page_with_head(title, "", body)
}

fn page_with_head(title: &str, head_extra: &str, body: &str) -> Response {
    let html = format!(
        r#"<!doctype html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="robots" content="noindex">
<title>{title}｜MU</title>{head_extra}
<style>
:root{{--ink:#111;--sub:#666;--line:#e5e5e5;--accent:#1f8a4c}}
body{{font-family:-apple-system,"Hiragino Sans",sans-serif;color:var(--ink);max-width:640px;margin:0 auto;padding:32px 20px 80px;line-height:1.9}}
h1{{font-size:24px;margin:0 0 4px}} h2{{font-size:17px;margin:36px 0 8px}}
.eyebrow{{font-size:12px;letter-spacing:.18em;color:var(--sub)}}
.card{{border:1px solid var(--line);border-radius:12px;padding:18px 20px;margin:14px 0}}
.muted{{color:var(--sub);font-size:13px}}
.btn{{display:inline-block;background:var(--ink);color:#fff;border:0;border-radius:8px;padding:11px 20px;font-size:14px;font-weight:700;cursor:pointer;text-decoration:none}}
.btn.green{{background:var(--accent)}}
input,select,textarea{{font-size:16px;padding:10px 12px;border:1px solid #ccc;border-radius:8px;width:100%;box-sizing:border-box;margin:4px 0 12px;font-family:inherit}}
.sticky-cta{{position:fixed;left:0;right:0;bottom:0;padding:9px 12px;background:rgba(255,255,255,.96);backdrop-filter:blur(8px);border-top:1px solid var(--line);text-align:center;z-index:20}}
.sticky-cta .btn{{width:auto;padding:11px 26px}}
table{{border-collapse:collapse;font-size:13.5px}} td{{padding:2px 12px 2px 0;vertical-align:top}} td:first-child{{color:var(--sub);white-space:nowrap}}
ol li{{margin:6px 0}}
.tag{{display:inline-block;font-size:11px;border:1px solid var(--line);border-radius:99px;padding:1px 10px;color:var(--sub)}}
.tag.mine{{border-color:var(--accent);color:var(--accent);font-weight:700}}
pre{{white-space:pre-wrap;font-family:inherit;margin:0}}
.hero-img{{width:100%;border-radius:14px;display:block;margin:18px 0;aspect-ratio:16/9;object-fit:cover}}
.steps{{list-style:none;padding:0;margin:0}}
.steps li{{display:flex;gap:14px;align-items:center;padding:10px 0;border-bottom:1px solid var(--line)}}
.steps li:last-child{{border-bottom:0}}
.steps img{{width:120px;height:80px;object-fit:cover;border-radius:10px;flex:0 0 auto}}
.steps .n{{font-weight:800;color:var(--accent);font-size:13px}}
.brand{{background:#0a0a0a;color:#f5f5f5;border-radius:14px;padding:22px 22px 18px;margin:18px 0}}
.brand h2{{color:#fff;margin-top:0}} .brand a{{color:#ffb37a}}
.brand .muted{{color:#aaa}}
.bignote{{font-size:16px;background:#f6f9f6;border:1px solid #d7e8dc;border-radius:12px;padding:14px 16px;margin:12px 0}}
@media(max-width:480px){{.steps img{{width:88px;height:64px}}}}
</style></head><body>{body}
<script defer src="/tracking.js"></script>
<script defer src="/mu-funnel.js"></script>
<script defer src="https://enabler-analytics.fly.dev/t.js"></script>
</body></html>"#,
    );
    ([(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}

async fn send_resend(to: &str, subject: &str, html: String) -> bool {
    let Ok(key) = env::var("RESEND_API_KEY") else { return false };
    let payload = serde_json::json!({
        "from": "MU おしごと <noreply@wearmu.com>",
        "to": [to],
        "subject": subject,
        "html": html,
    });
    reqwest::Client::new()
        .post("https://api.resend.com/emails")
        .bearer_auth(&key)
        .json(&payload)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

// ── GET /work — 募集LP（6パターンA/Bテスト） ───────────────────────────────
// 「作って届ける人」の募集トップ。?v=1..6 で訴求パターンを切替（広告6本=各vに着地
// →CVR比較）。指定なしはリクエストごとに分散。CVRは mu-funnel.js で計測
// (work_view_v{n} / work_apply_v{n})。応募は既存 /api/work/apply を流用。
#[derive(Deserialize)]
pub struct RecruitQuery {
    #[serde(default)]
    pub v: Option<String>,
}

// (eyebrow, h1, lead, cta) の6訴求パターン。
const RECRUIT_VARIANTS: &[(&str, &str, &str, &str)] = &[
    ("MU — おうちでできる仕事",
     "MUのTシャツを、家で<br>“きれいに包んで”送る仕事。",
     "AIがデザインしたMUのTシャツを、受け取って・検品して・きれいに包んで送る在宅ワーク。1件あたり数分、特別なスキルは不要。やった分だけ報酬、ノルマなし・いつでも辞められます。",
     "応募する（30秒）"),
    ("MU — センスを活かす仕事",
     "あなたのセンスで、<br>「開けた瞬間」をつくる。",
     "届いた品をMUの箱・薄紙・手書きカードで<b>包み直す“開封体験パック”</b>。梱包や手書き、写真が好きな人にぴったり。一つひとつ、あなたの手で。",
     "やってみる（30秒）"),
    ("MU — 安心して稼げる",
     "住所は見えない。前払いプール。<br>だから、安心。",
     "お客様の住所は<b>あなたには表示しません</b>（ブラインド配送）。報酬は先にプールされ、写真で承認されたら支払い。<b>立替なし・送料は当社負担</b>。",
     "安心して始める（30秒）"),
    ("MU — 一緒に育てる",
     "作る人の隣で、<br>「届ける人」になる。",
     "MUはAIが毎時ものづくりするブランド。その温度を最後に乗せるのが、あなた。<b>みんなで使って、みんなで育てる</b>仲間を募集中。",
     "仲間になる（30秒）"),
    ("MU — やった分だけ、すぐ報酬",
     "単価は着手前に必ず表示。<br>やった分だけ、翌月振込。",
     "完全出来高・<b>ノルマなし</b>。1件いくらかは引き受ける前に必ず表示。月末締め翌月振込（<b>振込手数料は当社負担</b>）。いまは立ち上げ期で件数は少なめ＝スキマ時間のおこづかい向き。収入は件数次第で保証はありませんが、評価が上がると単価もUP。MUで使えるポイント「糸(ITO)」ももらえます。",
     "いくら稼げるか見る"),
    ("MU — あなたの街のMU",
     "近所に、手で届ける。",
     "同じエリアの注文を、近所のあなたが<b>受け取り→仕上げ→お届け</b>（基本はポスト投函）。「MUの人が届けてくれた」を、あなたの街で。お客様の住所はMU側で管理し、あなたには見せません。",
     "街で始める（30秒）"),
];

pub async fn work_recruit(Query(q): Query<RecruitQuery>) -> Response {
    let n = q.v.as_deref().and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|n| *n >= 1 && *n <= RECRUIT_VARIANTS.len())
        .map(|n| n - 1)
        .unwrap_or_else(|| (rand::random::<u32>() as usize) % RECRUIT_VARIANTS.len());
    let (eyebrow, h1, lead, cta) = RECRUIT_VARIANTS[n];
    let v = n + 1;
    // 価格A/Bテスト (本人指示 2026-06-11): 提示単価をパターンの偶奇で出し分け。
    // v奇数=¥200(現行) / v偶数=¥300。v は応募フォームの hidden で送られるため、
    // スキーマ変更なしで「どの単価を見て応募したか」が追える (telegram通知に単価併記)。
    // ⚠提示した単価は約束 — v偶数経由の応募者へのジョブ単価は¥300前後にすること。
    let price = if v % 2 == 0 { 300 } else { 200 };
    let fee = fee_jpy();
    let img = "https://raw.githubusercontent.com/yukihamada/mu-mockups/main/work";
    let body = format!(
        r##"<div class="eyebrow">{eyebrow}</div>
<h1>{h1}</h1>
<img class="hero-img" src="{img}/step3_pack.png" alt="MUの梱包・仕上げの仕事" loading="lazy">
<p>{lead}</p>
<p style="font-size:15px;font-weight:700;margin:8px 0">👕 Tシャツを包んで送る ＝ <b>目安¥{price}前後/件・1件数分</b>。やった分だけ・ノルマなし・初期費用0。</p>
<div style="display:flex;gap:6px;flex-wrap:wrap;margin:6px 0 10px;font-size:12px">
<span class="tag">👕 仕事＝Tシャツを包んで送る</span><span class="tag">💴 目安¥{price}前後/件</span><span class="tag">📱 スマホだけ・初期費用0</span></div>
<p><a class="btn green" href="#apply" data-funnel="cta_click" data-funnel-cta="work_cta_v{v}">{cta}</a></p>

<p class="muted" style="margin:14px 0 4px;font-weight:700">▶ まずは30秒の紙芝居でどんな仕事か見てみてください（タップで再生・本人の声つき）</p>
<div style="position:relative;aspect-ratio:16/9;border-radius:14px;overflow:hidden;border:1px solid var(--line);margin:4px 0 18px">
<iframe src="https://kamishibai.tv/k/3vkvsih5" title="紙芝居「おうちで、MUを届ける人。」" loading="lazy" allow="autoplay; fullscreen" style="position:absolute;inset:0;width:100%;height:100%;border:0"></iframe>
</div>

<div class="brand" style="background:#14110a;border:1px solid #2a2418">
<p style="font-size:17px;font-weight:800;margin:0 0 6px;color:#fff">あなたが包んだ箱を、誰かが開けて笑う。</p>
<p style="margin:0;opacity:.88;font-size:14px">ただの「梱包作業」ではありません。AIが生み出した一着に、<b>最後の“温度”を乗せる</b>のがあなたの手。薄紙の包み方、封緘シール、手書きの一言——その小さな丁寧さで、受け取った人の<b>「箱を開けた瞬間」</b>が決まります。<b>退屈はAIに、温度は人に。</b></p>
<p class="muted" style="margin:8px 0 0">ちなみにAIは梱包がドヘタです。だから、あなたが要る。</p>
</div>

<h2>たぶん、こういう人。</h2>
<ul style="list-style:none;padding:0;margin:8px 0">
<li style="padding:7px 0;border-bottom:1px solid var(--line)">🤲 <b>きれいに包めたとき、ちょっと気持ちいい人</b> — その丁寧さが、そのまま誰かの「うれしい」になります。</li>
<li style="padding:7px 0;border-bottom:1px solid var(--line)">✍️ <b>手書きで一言添えるの、嫌いじゃない人</b> — あなたの一言が、その箱で一番記憶に残る部分です。</li>
<li style="padding:7px 0;border-bottom:1px solid var(--line)">⏳ <b>コーヒー淹れてから始めたい派の、時間に少し余裕がある人</b>（子育ての合間・学生・会社員の副業・リタイア後、どれでも）。</li>
<li style="padding:7px 0;border-bottom:1px solid var(--line)">📱 <b>スマホだけ・初期費用ゼロ</b>で、家でできる仕事を探している人。</li>
<li style="padding:7px 0">🌱 ものづくりブランドを<b>仲間と一緒に育てたい</b>人（ひとりじゃない）。</li>
</ul>
<p class="muted">ひとつでも「私かも」と思ったら、たぶん向いています。</p>

<h2>メインのお仕事：Tシャツの仕上げ・梱包・発送 👕</h2>
<p>MUのTシャツ（在庫を持たず、注文が入るたびに刷られます）を、受け取って・検品して・きれいに包んで・送る。<b>「箱を開けた瞬間」の体験をつくる</b>お仕事です。音コインなど他の品もありますが、<b>メインはTシャツ</b>です。</p>
<p class="muted" style="margin:-4px 0 6px">＝申し込む前に、やることはこれで全部わかります 👇</p>
<ol style="padding-left:1.1em;margin:8px 0 4px;line-height:1.85">
<li><b>受け取る</b> — MUからTシャツがまとめて届く（お客様の住所は<b>あなたには見えません</b>。宛名は封をした状態、またはMUの集約センター宛で届きます）</li>
<li><b>検品する</b> — プリントのかすれ・汚れ・サイズ違いがないか確認。スマホで写真を1枚</li>
<li><b>たたんで包む</b> — きれいにたたみ、薄紙で包んで <b>MUの封緘シール</b>を貼る</li>
<li><b>手書きの一言カード</b>を添える（テンプレあり・一言でOK）</li>
<li><b>封筒/箱に入れて投函</b> — クリックポスト等。<b>送料は当社負担</b>・立替なし</li>
<li><b>完成写真をアップ</b>して報告 → 運営が確認したら<b>報酬が確定</b></li>
</ol>
<p class="muted">所要：1件あたり数分（慣れたら流れ作業）。梱包資材キット（薄紙・封緘シール・カード）は当社からお送りします。</p>
<div class="card" style="border-color:var(--accent)">
<p style="margin:0 0 4px"><b>✨ NFCタグも入れられます（オプション・「タップする箱」）</b></p>
<p class="muted" style="margin:0">ご希望の注文に、スマホで<b>タップすると <a href="https://wearmu.com/make">wearmu.com/make</a></b>（やその一着のための音・真贋ページ）が開く<b>NFCタグ</b>を同梱します。書き込みは<b>NFC対応の方</b>が担当（音コインと同じ仕組み・無料アプリで約30秒）。受け取った人が箱をタップ→次の“作る”へ。応募フォームで「NFC対応」にチェックすると、この仕事も回ってきます。</p>
</div>

<h2>そのほかのお仕事</h2>
<ul class="steps">
<li><img src="{img}/step2_write.png" alt="" loading="lazy"><div><span class="n">届ける</span><br><b>🔔 音コイン(NFC)を作って発送</b><br><span class="muted">かざすと鳴るコインに書込→検品→投函。<a href="/work/oto">→ 詳しく</a></span></div></li>
<li><img src="{img}/step1_kit.png" alt="" loading="lazy"><div><span class="n">磨く</span><br><b>🔍 検品 / 📸 実着フォト</b><br><span class="muted">発送前チェック・実際に着て撮影して商品ページへ（順次開放）</span></div></li>
</ul>

<h2>安心して働ける仕組み</h2>
<div class="card">
<p>🔒 <b>お客様の住所はあなたに見せません</b>（ブラインド配送）。宛名は封をした状態、またはMUの集約センター経由でお届けします。</p>
<p>💴 <b>報酬は先にプール（エスクロー）</b>。写真で承認されたら支払い。立替なし・送料は当社負担。</p>
<p>🕊 <b>ノルマなし・いつでも辞められます</b>。引き受けた分だけ・自分のペースで。</p>
<p>⭐ <b>段階的に単価UP</b>。完了数と評価で、できる仕事と報酬が増えます。MU内で使えるポイント「糸(ITO)」も貯まります（MUの服やグッズに使えます）。</p>
</div>

<h2>報酬とお金のこと 💴</h2>
<table style="margin-top:6px">
<tr><td>単価</td><td><b>出来高制・着手する前に「1件いくら」を必ず表示</b>します。Tシャツの仕上げは目安 <b>¥{price}前後/件</b>（1件数分）、音コインは ¥{fee}/件。1件あたりが数分で終わる軽作業の単価です。</td></tr>
<tr><td>支払い</td><td>月末締め・<b>翌月に銀行振込</b>。<b>振込手数料は当社負担</b>。報酬は写真の承認で確定（先にプールするエスクロー方式）。</td></tr>
<tr><td>どのくらい稼げる?</td><td><b>正直に言うと、いまは立ち上げ期で件数はまだ少ないです。</b>「本業」ではなく<b>スキマ時間のおこづかい</b>くらいに考えてください。注文が増えれば回ってくる件数も増えます。収入は引き受けた件数しだいで、<b>金額は保証しません</b>。ノルマなし・やった分だけ。</td></tr>
<tr><td>初期費用</td><td><b>ゼロ</b>。梱包資材（薄紙・封緘シール・カード）も<b>当社が支給</b>・送料も当社負担。<b>立替なし</b>。</td></tr>
</table>

<h2>応募から、はじめるまでの流れ</h2>
<ol style="padding-left:1.1em;margin:8px 0;line-height:1.85">
<li><b>応募（30秒）</b> — 下のフォームに名前・メール・都道府県だけ。</li>
<li><b>承認（最短当日〜数日）</b> — あなた専用の「仕事ページ」リンクをメールでお送りします。</li>
<li><b>梱包キットが届く</b> — 薄紙・封緘シール・カード（すべて当社負担）。</li>
<li><b>最初の1件</b> — 仕事ページから引き受け→仕上げ→スマホで写真を1枚アップ。</li>
<li><b>報酬</b> — 月末締め翌月振込。お振込先はこのタイミングで登録します。</li>
</ol>

<h2>必要なもの / 要らないもの</h2>
<div class="card">
<p>✅ <b>要るもの</b>：スマホ（写真用）・ポストに行ける環境・メールアドレス。</p>
<p>🚫 <b>要らないもの</b>：プリンタ・特別な道具・初期費用・在庫を抱えること。</p>
</div>

<h2>よくある不安</h2>
<div class="card">
<p><b>Q. 怪しくないですか？</b><br>運営は <b>株式会社イネブラ（実在・東京／<a href="https://enablerdao.com">会社概要</a>）</b>。業務委託で、<b>初期費用・ノルマ・違約金はすべてゼロ</b>。辞めるのもメール一本でOK。MUは売上などの数字を <a href="https://wearmu.com/transparency">/transparency</a> で全部公開しています（口だけにしない、が方針です）。</p>
<p><b>Q. お客様の住所は見えますか？</b><br>見えません。宛名は封をした状態、またはMUの集約センター経由で届きます（ブラインド配送）。あなたは中身を仕上げるだけです。</p>
<p><b>Q. 口座番号やマイナンバーは？</b><br>お振込先は<b>承認後</b>に登録。報酬は業務委託の雑所得/事業所得です（年20万円超などで確定申告が必要な場合があります）。</p>
<p><b>Q. 不良品・配送事故のときは？</b><br>再送の品・送料は<b>当社が負担</b>します。あなたの自己負担はありません。</p>
<p><b>Q. 写真の承認が落ちることはある？</b><br>たたみ方やカードの入れ忘れなど、まれにやり直しをお願いすることがあります。その場合は<b>どこを直すか具体的にお伝えし、直してもらえれば報酬は支払われます</b>。理由なく落とすことはありません。ミスでお客様への再送が必要になっても、費用をあなたに請求することはありません。</p>
</div>
<p style="text-align:center"><a class="btn green" href="#apply" data-funnel="cta_click" data-funnel-cta="work_cta_mid_v{v}">この内容で応募する（30秒）</a></p>
<p class="muted" style="text-align:center;margin-top:-4px">いまは立ち上げ期。最初の仲間として一緒に始めてくれる方を募集中です。</p>

<div class="brand">
<div class="eyebrow" style="color:#888">一緒に育てるブランド</div>
<h2>MU ／ wearmu</h2>
<p style="margin:6px 0">MU(ムー)は、<b>AIが毎時ものづくりする</b>新しいブランド。在庫を持たず、注文が入ってから作る。数字は<a href="https://wearmu.com/transparency">/transparency</a>で全部公開。<b>退屈はAIに、温度は人に。</b>その“温度”を最後に乗せるのが、あなたの仕事です。</p>
</div>

<h2 id="apply">応募する</h2>
<form method="POST" action="/api/work/apply" class="card">
<input type="hidden" name="v" value="{v}">
<p class="muted" style="margin:0 0 8px">ご入力の氏名・メール・都道府県・自己紹介は、選考とご連絡のためだけに使い、第三者には渡しません。</p>
<label>お名前<input name="name" required maxlength="60" autocomplete="name" placeholder="山田 はなこ"></label>
<label>メールアドレス<input name="email" type="email" required maxlength="120" autocomplete="email" inputmode="email" placeholder="you@example.com"></label>
<label>お住まいの都道府県（任意）<input name="region" maxlength="20" autocomplete="address-level1" placeholder="北海道"></label>
<label>あなたについて・やってみたい理由（任意・空欄のままでもOK）<textarea name="about" maxlength="400" rows="2" placeholder="例: 丁寧な作業が好きです（任意・あとからでOK）"></textarea></label>
<label style="display:flex;gap:8px;align-items:flex-start;font-size:13px;font-weight:400">
<input type="checkbox" name="agree" required style="width:auto;margin-top:3px;flex:0 0 auto">
<span>お客様の配送情報を<b>発送目的のみ</b>に使い、第三者に渡さず、<b>発送後すみやかに破棄</b>することに同意します。</span></label>
<button class="btn" type="submit" data-funnel="cta_click" data-funnel-cta="work_apply_v{v}">30秒で応募する</button>
<p class="muted">無料。合わなければ、辞めるのも一言でOK。承認されると、仕事ページのリンクをメールでお送りします。<br>あなたの“ひと手間”を、待っている人がいます。🌱</p>
</form>
<p class="muted">運営: <b><a href="https://enablerdao.com">株式会社イネブラ</a></b>(Enabler Inc.)／〒102-0074 東京都千代田区九段南1-5-6 りそな九段ビル5階KSフロア／代表取締役 濱田優貴／業務委託／お問い合わせ info@enablerdao.com</p>
<div class="sticky-cta"><a class="btn green" href="#apply" data-funnel="cta_click" data-funnel-cta="work_cta_sticky_v{v}">30秒で応募する</a></div>
<script>try{{(window.MU_FUNNEL&&window.MU_FUNNEL.send||function(){{}})('work_view',{{variant:'{v}',price:'{price}'}})}}catch(e){{}}</script>"##,
    );
    page_with_head("MUで、作って届ける仕事", GTAG_HEAD, &body)
}

// ── GET /work/oto — 音コイン専用LP ─────────────────────────────────────────
pub async fn work_page() -> Response {
    let fee = fee_jpy();
    let ufee = unbox_fee_jpy();
    let body = format!(
        r#"<div class="eyebrow">MU — おうちでできる仕事</div>
<h1>つくる人と、仕上げて届ける人。</h1>
<p>MU(ムー)の小さなプロダクトを、あなたの手で<b>仕上げて、お客様に届ける</b>お仕事です。完全に自分のペース・出来高制・<b>ノルマなし・いつでも辞められます</b>。やり方は2つ、どちらか得意な方を選べます。</p>

<div class="card">
<span class="tag">A</span> <b>音コインをつくって届ける</b>(スマホでNFC書込が必要)
<p class="muted" style="margin:6px 0">¥{fee}/件。スマホをかざすと音が鳴る黒いコインに、無料アプリで音を書き込み→検品→封筒→投函。<b>NFC対応スマホ(iPhone 7以降・大半のAndroid)</b>が要ります。</p>
<a class="btn green" href='#apply'>このお仕事に応募する</a>
</div>

<div class="card">
<span class="tag">B</span> <b>仕上げて届ける(NFC不要・手書きOK)</b>
<p class="muted" style="margin:6px 0">¥{ufee}/件。届いた品を<b>きれいに包み直して、ひとことカードを添えて</b>お届けします。薄紙でくるみ、MUのシールで封をして、手書きのカード(任意)を入れる——それだけ。<b>特別な道具もNFCもいりません。</b>はじめての方も、文字が小さいと見づらい方も、無理なくできます。</p>
<a class="btn green" href='#apply'>このお仕事に応募する</a>
</div>

<div class="brand">
<div class="eyebrow" style="color:#888">作っているブランドのこと</div>
<h2>MU ／ AIが毎時つくるものづくり</h2>
<p style="margin:6px 0">MU(ムー)は、<b>AIが毎時1着、服やプロダクトを生み出す</b>新しいものづくりブランドです。在庫を持たず、注文が入ってから作る。数字は<a href="https://wearmu.com/transparency">/transparency</a>で全部公開しています。</p>
<p style="margin:6px 0">届いた箱を開けるその一瞬まで、ていねいにしたい。あなたが包んで届けた品から、誰かの毎日に小さな灯りがともります。運営: 株式会社イネブラ。</p>
</div>

<h2>「仕上げて届ける」の流れ</h2>
<ul class="steps">
<li><img src="{img}/step1_kit.png" alt="" loading="lazy"><div><span class="n">STEP 1</span><br><b>キットと品物を受け取る</b><br><span class="muted">薄紙・MUシール・カード台紙をまとめてお送りします(立替¥0)</span></div></li>
<li><img src="{img}/step3_pack.png" alt="" loading="lazy"><div><span class="n">STEP 2</span><br><b>検品して、包み直す</b><br><span class="muted">薄紙でくるみ、MUのシールで封をする。ひとことカードは任意</span></div></li>
<li><img src="{img}/step3_pack.png" alt="" loading="lazy"><div><span class="n">STEP 3</span><br><b>完成写真をとる</b><br><span class="muted">包んだ品の写真を1枚アップ(宛名・住所は写さないでください)</span></div></li>
<li><img src="{img}/step4_mail.png" alt="" loading="lazy"><div><span class="n">STEP 4</span><br><b>ポストに投函・完了報告</b><br><span class="muted">投函→追跡番号を入力。運営が写真を確認したら報酬が確定します</span></div></li>
</ul>

<table style="margin-top:18px">
<tr><td>報酬</td><td>音コイン <b>¥{fee}/件</b> ／ 仕上げて届ける <b>¥{ufee}/件</b>(月末締め・翌月銀行振込・<b>振込手数料は当社負担</b>)</td></tr>
<tr><td>送料</td><td><b>当社負担</b>。予納分・返送ラベルはキットに同梱します(立替不要)</td></tr>
<tr><td>必要なもの</td><td>A=NFC対応スマホ ／ B=スマホで写真がとれる環境・ポストに行ける環境だけ</td></tr>
<tr><td>はじめの方へ</td><td>まずは<b>ハブ(運営)経由</b>で品を受け取り、包んで送り返すだけ。<b>お客様の住所は一切見ません</b>。慣れてきたら直送もお選びいただけます。</td></tr>
<tr><td>時間</td><td>完全に自分のペース。引き受けた分だけ。<b>ノルマなし・いつでも辞められます</b></td></tr>
<tr><td>場所</td><td>日本国内どこでも</td></tr>
</table>

<h2>よくある質問</h2>
<div class="card">
<p><b>Q. お客様の住所はどう扱う？</b><br>はじめのうちは<b>住所を一切お見せしません</b>。運営のハブ宛に送り返していただくだけです。慣れて直送をお願いする段階になっても、宛名を書いたらすみやかに破棄してください(応募時に同意いただきます)。第三者への提供は禁止です。</p>
<p><b>Q. 写真に住所が写ったら？</b><br>アップされた写真は、サーバ側で位置情報(GPS)を自動で消します。万一読み取れない写真はお受けできません(もう一度撮り直してください)。宛名や追跡番号は写さないでください。</p>
<p><b>Q. 不良品・配送事故のときは？</b><br>再発送の品・送料は<b>当社が負担</b>します。あなたの自己負担はありません。報酬も減りません。</p>
<p><b>Q. ノルマや納期は？</b><br>ありません。引き受けた分だけ・自分のペースで。ゼロ件でもペナルティはありません。</p>
<p><b>Q. 報酬はいつ確定？</b><br>完成写真を運営が確認したあとに確定します(やり直しのときは理由をお伝えします・当社負担で再作業、過剰なペナルティはありません)。</p>
<p><b>Q. 税金は？</b><br>業務委託のため、報酬は雑所得/事業所得になります。年間の合計額によっては確定申告が必要です(目安: 給与所得者で年20万円超など)。明細は仕事ページからご確認いただけます。</p>
<p><b>Q. 辞めたいときは？</b><br>メール一本でOK。違約金などはありません。</p>
</div>

<h2 id="apply">応募する</h2>
<form method="POST" action="/api/work/apply" class="card">
<label>お名前<input name="name" required maxlength="60" placeholder="山田 はなこ"></label>
<label>メールアドレス<input name="email" type="email" required maxlength="120" placeholder="you@example.com"></label>
<label>お住まいの都道府県<input name="region" maxlength="20" placeholder="北海道"></label>
<label>やってみたいお仕事
<select name="want_job">
<option value="unbox">仕上げて届ける(NFC不要・手書きOK)</option>
<option value="oto">音コインをつくって届ける(NFCが必要)</option>
<option value="both">どちらでも</option>
</select></label>
<label style="display:flex;gap:8px;align-items:flex-start;font-size:13px;font-weight:400">
<input type="checkbox" name="nfc_ok" value="1" style="width:auto;margin-top:3px;flex:0 0 auto">
<span>スマホで<b>NFCの書き込み</b>ができます(音コインのお仕事に必要・分からなければチェックなしでOK)</span></label>
<label style="display:flex;gap:8px;align-items:flex-start;font-size:13px;font-weight:400">
<input type="checkbox" name="agree" required style="width:auto;margin-top:3px;flex:0 0 auto">
<span>お客様の配送情報を<b>発送目的のみ</b>に使い、第三者に渡さず、<b>発送後すみやかに破棄</b>することに同意します。</span></label>
<button class="btn" type="submit">応募する</button>
<p class="muted">承認されると、仕事ページのリンクをメールでお送りします。</p>
</form>
<p class="muted">運営: <b>株式会社イネブラ</b>(Enabler Inc.)／〒102-0074 東京都千代田区九段南1-5-6 りそな九段ビル5階KSフロア・業務委託。<br>質問は info@enablerdao.com へ。商品ページ: <a href="/shop?brand=oto">音コインを見る</a></p>"#,
        img = "https://raw.githubusercontent.com/yukihamada/mu-mockups/main/work",
    );
    page_with_head("おうちでできる仕事 — 音コイン", GTAG_HEAD, &body)
}

// ── POST /api/work/apply ────────────────────────────────────────────────
#[derive(Deserialize)]
pub struct ApplyForm {
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub region: String,
    /// 自己紹介・やってみたい理由（どんな人か）。任意。
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub want_job: String,
    #[serde(default)]
    pub nfc_ok: Option<String>,
    #[serde(default)]
    pub agree: Option<String>,
    /// 着地した募集パターン(1..6)。CVR帰属用。
    #[serde(default)]
    pub v: Option<String>,
}

pub async fn work_apply(State(db): State<Db>, headers: HeaderMap, Form(f): Form<ApplyForm>) -> Response {
    let name = f.name.trim().to_string();
    let email = f.email.trim().to_lowercase();
    let region = f.region.trim().to_string();
    let about: String = f.about.trim().chars().take(400).collect();
    let nfc_capable: i64 = if f.nfc_ok.as_deref().unwrap_or("").is_empty() { 0 } else { 1 };
    // 希望ジョブは want_job(unbox/oto/both)。oto には NFC が要る。
    let want_job = match f.want_job.trim() {
        "oto" => "oto",
        "both" => "both",
        _ => "unbox",
    };
    if name.is_empty() || !email.contains('@') {
        return page("入力エラー", "<h1>お名前とメールアドレスを入力してください</h1><p><a href=\"/work\">戻る</a></p>");
    }
    // 個人情報(客の配送情報)の取扱い同意は必須(HTMLのrequiredに加えサーバ側でも検証)
    if f.agree.as_deref().unwrap_or("").is_empty() {
        return page("同意が必要です", "<h1>配送情報の取扱いへの同意が必要です</h1><p>取扱いへの同意にチェックをお願いします。</p><p><a href=\"/work#apply\">戻る</a></p>");
    }
    let via = f.v.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()).unwrap_or("-");
    // 価格A/Bテスト: 提示単価はパターンの偶奇 (work_recruit と同一規則)。
    // ワーカーに保存し、汎用ジョブ(Tシャツ等)の単価として使う。見せた単価は約束。
    let rate_jpy: i64 = match via.parse::<usize>() {
        Ok(n) if n % 2 == 0 => 300,
        _ => 200,
    };
    let ip = crate::client_ip(&headers);
    let worker_id: i64 = {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        // 新規はデフォルト trust_tier 0(新人=ハブ経由・住所非開示)。nfc_capable を記録。
        let _ = conn.execute(
            "INSERT OR IGNORE INTO work_workers (email, name, region, about, rate_jpy, nfc_capable) VALUES (?,?,?,?,?,?)",
            rusqlite::params![email, name, region, about, rate_jpy, nfc_capable],
        );
        // 既存ワーカーの再応募でも name/region/about/rate_jpy/nfc_capable を最新で更新(承認状態は触らない)
        let _ = conn.execute(
            "UPDATE work_workers SET name=?, region=?, about=?, rate_jpy=?, nfc_capable=? WHERE email=?",
            rusqlite::params![name, region, about, rate_jpy, nfc_capable, email],
        );
        let id = conn
            .query_row("SELECT id FROM work_workers WHERE email=?", rusqlite::params![email], |r| r.get(0))
            .unwrap_or(0);
        audit(&conn, Some(id), None, "apply", &ip, &format!("{{\"want_job\":\"{}\",\"nfc\":{}}}", want_job, nfc_capable));
        id
    };
    let admin = env::var("ADMIN_TOKEN").unwrap_or_default();
    // Telegram は運営承認に必要な最小情報のみ。お客様の住所は無し。
    let shown_price = format!("提示単価¥{}", rate_jpy);
    let about_line = if about.is_empty() { String::new() } else { format!("\n「{}」", about) };
    let _ = crate::send_telegram_message(&format!(
        "🧵 *work応募* (パターンv{}・{}・希望: {} / NFC: {})\nworker#{} <{}> {}{}\n承認→ https://wearmu.com/admin/work/approve?id={}&token={}",
        via, shown_price, want_job, if nfc_capable == 1 { "可" } else { "不可" },
        worker_id, email, region, about_line, worker_id, admin
    ))
    .await;
    // 応募成功 = Google Ads コンバージョン発火 (MU_WORK_Recruit が応募で最適化できる
    // シグナル)。成功時のみこのページが返るので、ここで一度だけ発火する。
    // enabler-analytics の work_apply_v{n} (応募ボタンの data-funnel click) は
    // 別系統のままそのまま並存。
    page_with_head(
        "応募ありがとうございます",
        GTAG_HEAD,
        "<h1>応募を受け付けました。</h1><p>内容を確認して、承認されると<b>仕事ページのリンクをメール</b>でお送りします。少しお待ちください。</p>\
<script>try{gtag('event','conversion',{'send_to':'AW-17814724474/Sba2CLXm9rwcEPq-3K5C'})}catch(e){}</script>",
    )
}

// ── GET /admin/work/approve?token=&id= ──────────────────────────────────
#[derive(Deserialize)]
pub struct ApproveQuery {
    pub token: String,
    pub id: i64,
}

/// GET /admin/work/pending?token= — 承認待ちワーカー一覧(JSON)。
/// ローカルの通知/自動処理watcher(音を鳴らす→自動承認)がpollする検知口。
pub async fn admin_pending(State(db): State<Db>, Query(q): Query<QueueQuery>) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let conn = db.lock().unwrap();
    ensure_tables(&conn);
    let rows: Vec<serde_json::Value> = {
        let mut stmt = conn
            .prepare("SELECT id, name, COALESCE(region,''), COALESCE(created_at,''), email, COALESCE(about,'') FROM work_workers WHERE status='pending' ORDER BY id DESC LIMIT 50")
            .unwrap();
        stmt.query_map([], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)?,
                "region": r.get::<_, String>(2)?, "created_at": r.get::<_, String>(3)?,
                "email": r.get::<_, String>(4)?, "about": r.get::<_, String>(5)?
            }))
        }).unwrap().filter_map(|x| x.ok()).collect()
    };
    let body = serde_json::json!({"count": rows.len(), "pending": rows}).to_string();
    ([(axum::http::header::CONTENT_TYPE, "application/json")], body).into_response()
}

pub async fn admin_approve(State(db): State<Db>, Query(q): Query<ApproveQuery>) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let (email, name, token): (String, String, String) = {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        let row: Option<(String, String, Option<String>)> = conn
            .query_row(
                "SELECT email, name, token FROM work_workers WHERE id=?",
                rusqlite::params![q.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        let Some((email, name, existing)) = row else {
            return (StatusCode::NOT_FOUND, "worker not found").into_response();
        };
        // 冪等: 既に承認済みなら既存トークンを再利用(再メールのみ)
        let token = existing.unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        let _ = conn.execute(
            "UPDATE work_workers SET status='active', token=?, approved_at=datetime('now') WHERE id=?",
            rusqlite::params![token, q.id],
        );
        (email, name, token)
    };
    let queue_url = format!("https://wearmu.com/work/queue?token={}", token);
    let emailed = send_resend(
        &email,
        "【MU おしごと】承認されました — 仕事ページのご案内",
        format!(
            r#"<div style="font-family:sans-serif;line-height:1.8"><p>{}さん</p>
<p>MUのお仕事(Tシャツの仕上げ・音コインなど)、承認されました。下のリンクがあなた専用の仕事キューです(ブックマーク推奨・他の人に共有しないでください)。</p>
<p><a href="{}" style="background:#111;color:#fff;padding:12px 22px;border-radius:8px;text-decoration:none;font-weight:700">仕事キューを開く</a></p>
<p><b>このメールに返信で、キット(資材一式)の郵送先住所を教えてください。</b>梱包資材・封緘シール・カード等をお送りします(住所はキット送付のみに使います)。</p>
<p>報酬は月末締め・翌月振込です。振込先口座は初回の報酬が確定したタイミングで伺います。<br>— MU</p></div>"#,
            esc(&name),
            queue_url
        ),
    )
    .await;
    page(
        "承認しました",
        &format!(
            "<h1>承認しました。</h1><p>{} &lt;{}&gt; に仕事キューのリンクを{}。</p><p class=\"muted\">ワーカーから住所の返信が来たらキット(資材一式)の発送を忘れずに。手順: docs/WORK_OPERATIONS.md</p>",
            esc(&name),
            esc(&email),
            if emailed { "メール送信しました" } else { "送信できませんでした(RESEND未設定?)。手動で共有してください" }
        ),
    )
}

// ── GET /work/queue?token= ──────────────────────────────────────────────
#[derive(Deserialize)]
pub struct QueueQuery {
    pub token: String,
}

struct JobRow {
    order_id: i64,
    sku: String,
    label: String,
    encode_url: Option<String>,
    status: String,
    ship_json: String,
    assigned_to: Option<i64>,
    job_kind: Option<String>,
    review_state: Option<String>,
    fulfillment_route: String,
}

struct Worker {
    id: i64,
    name: String,
    nfc_capable: i64,
    trust_tier: i64,
}

fn worker_of(conn: &rusqlite::Connection, token: &str) -> Option<Worker> {
    if token.is_empty() {
        return None;
    }
    conn.query_row(
        "SELECT id, name, COALESCE(nfc_capable,1), COALESCE(trust_tier,0) FROM work_workers WHERE token=? AND status='active'",
        rusqlite::params![token],
        |r| Ok(Worker { id: r.get(0)?, name: r.get(1)?, nfc_capable: r.get(2)?, trust_tier: r.get(3)? }),
    )
    .ok()
}

/// shipping_address_json から表示用住所を作る。full=false なら市区までに丸める
/// (引き受ける前のワーカーに全住所を見せない)。
fn render_addr(ship_json: &str, full: bool) -> String {
    let v: serde_json::Value = serde_json::from_str(ship_json).unwrap_or_default();
    let addr = &v["address"];
    let name = v["name"].as_str().unwrap_or("");
    let g = |k: &str| addr[k].as_str().unwrap_or("");
    if full {
        format!(
            "{}\n〒{} {} {} {} {}",
            name,
            g("postal_code"),
            g("state"),
            g("city"),
            g("line1"),
            g("line2")
        )
    } else {
        format!("{} {} 在住のお客様", g("state"), g("city"))
    }
}

/// trust_tier ゲート付き住所表示。tier0(新人)は full を要求しても住所を一切出さず
/// 「運営ハブ宛に返送」表示(モードA・ブラインド配送)。tier1+ のみ full住所。
fn render_addr_gated(ship_json: &str, want_full: bool, trust_tier: i64) -> String {
    if want_full && trust_tier >= 1 {
        render_addr(ship_json, true)
    } else if want_full {
        // 新人: 物理的に住所を扱わせない。運営ハブ宛に送り返す運用。
        format!(
            "MU 仕上げハブ宛に返送してください\n{}\n(同梱の返送ラベルをそのまま貼るだけ・お客様の住所は表示しません)",
            render_addr(ship_json, false)
        )
    } else {
        render_addr(ship_json, false)
    }
}

pub async fn work_queue(State(db): State<Db>, Query(q): Query<QueueQuery>) -> Response {
    let (worker, jobs, shipped_count, earned_cash, ito_pending): (Worker, Vec<JobRow>, i64, i64, i64) = {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        let Some(w) = worker_of(&conn, &q.token) else {
            return page("リンクが無効です", "<h1>このリンクは無効です</h1><p>承認メールのリンクをご確認ください。応募は <a href=\"/work\">/work</a> から。</p>");
        };
        let mut stmt = conn
            .prepare(
                "SELECT o.id, o.sku, p.label, p.description_ja, o.status,
                        COALESCE(o.shipping_address_json,'{}'), a.worker_id, a.job_kind, a.review_state,
                        COALESCE(p.fulfillment_route,'manual')
                 FROM catalog_orders o
                 JOIN catalog_products p ON p.sku = o.sku
                 LEFT JOIN work_assignments a ON a.order_id = o.id
                 WHERE p.fulfillment_route='manual'
                   AND o.status IN ('manual_pending','manual_assigned')
                 ORDER BY o.created_at ASC",
            )
            .unwrap();
        let jobs: Vec<JobRow> = stmt
            .query_map([], |r| {
                let desc: String = r.get(3)?;
                Ok(JobRow {
                    order_id: r.get(0)?,
                    sku: r.get(1)?,
                    label: r.get(2)?,
                    encode_url: encode_url_of(&desc),
                    status: r.get(4)?,
                    ship_json: r.get(5)?,
                    assigned_to: r.get(6)?,
                    job_kind: r.get(7)?,
                    review_state: r.get(8)?,
                    fulfillment_route: r.get(9)?,
                })
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        // 完了件数(発送済み)
        let cnt: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM work_assignments WHERE worker_id=? AND shipped_at IS NOT NULL",
                rusqlite::params![w.id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        // 確定現金は email 紐付けの mu_credit_ledger(reason='work_cash')。worker→email を引いて集計。
        let cash: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(l.delta_jpy),0) FROM mu_credit_ledger l
                 JOIN work_workers ww ON ww.email = l.email
                 WHERE ww.id=? AND l.reason='work_cash'",
                rusqlite::params![w.id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let grains: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(grains),0) FROM work_ito_grants WHERE worker_id=?",
                rusqlite::params![w.id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        (w, jobs, cnt, cash, grains)
    };

    let mut cards = String::new();
    for j in &jobs {
        let mine = j.assigned_to == Some(worker.id);
        // この注文を unbox/oto どちらで扱うか: 既に claim 済みなら記録値、未claim はワーカー能力で推定。
        let job_kind = j.job_kind.clone().unwrap_or_else(|| if worker.nfc_capable == 1 { "oto".into() } else { "unbox".into() });
        let is_oto = job_kind == "oto";
        let enc = j
            .encode_url
            .as_deref()
            .map(|u| format!("<a href=\"{0}\">{0}</a>", esc(u)))
            .unwrap_or_else(|| "<span class=\"muted\">(書込URL不明 → 運営に確認)</span>".into());
        let review_state = j.review_state.clone().unwrap_or_else(|| "claimed".into());
        if j.status == "manual_pending" {
            // 未割当: ワーカー能力でどちらの入口を出すか。NFC不可は oto を出さない。
            let mut buttons = format!(
                r#"<form method="POST" action="/api/work/claim" style="margin-top:10px;display:inline-block">
<input type="hidden" name="token" value="{}"><input type="hidden" name="order_id" value="{}">
<input type="hidden" name="job_kind" value="unbox">
<button class="btn green" type="submit">仕上げて届ける(¥{})</button></form>"#,
                esc(&q.token), j.order_id, unbox_fee_jpy()
            );
            if worker.nfc_capable == 1 && j.encode_url.is_some() {
                buttons.push_str(&format!(
                    r#" <form method="POST" action="/api/work/claim" style="margin-top:10px;display:inline-block">
<input type="hidden" name="token" value="{}"><input type="hidden" name="order_id" value="{}">
<input type="hidden" name="job_kind" value="oto">
<button class="btn" type="submit">音コインにする(¥{})</button></form>"#,
                    esc(&q.token), j.order_id, fee_jpy()
                ));
            }
            cards.push_str(&format!(
                r#"<div class="card"><span class="tag">募集中</span>
<h2 style="margin:8px 0 4px">{}</h2>
<table><tr><td>届け先</td><td>{}</td></tr></table>
{}</div>"#,
                esc(&j.label),
                esc(&render_addr(&j.ship_json, false)),
                buttons
            ));
        } else if mine {
            // 担当中: 提出済みかどうかで表示を分ける。
            if review_state == "proof_submitted" {
                cards.push_str(&format!(
                    r#"<div class="card"><span class="tag mine">確認待ち</span>
<h2 style="margin:8px 0 4px">{}</h2>
<p class="bignote">提出ありがとうございます。運営が確認したら報酬が確定します。少しお待ちください。</p></div>"#,
                    esc(&j.label)
                ));
            } else if is_oto {
                // 音コイン担当中(従来フロー): NFC書込URL+発送
                cards.push_str(&format!(
                    r#"<div class="card"><span class="tag mine">あなたが担当中(音コイン)</span>
<h2 style="margin:8px 0 4px">{}</h2>
<table>
<tr><td>① 書込URL</td><td>{}</td></tr>
<tr><td>② 検品</td><td>かざして音が鳴ればOK</td></tr>
<tr><td>③ 届け先</td><td><pre>{}</pre></td></tr>
<tr><td>SKU</td><td class="muted">{}</td></tr>
</table>
<p class="muted" style="margin:8px 0 0">⚠ 宛名を書いたら、この住所メモは破棄してください(個人情報)。</p>
<form method="POST" action="/api/work/ship" style="margin-top:10px">
<input type="hidden" name="token" value="{}"><input type="hidden" name="order_id" value="{}">
<label>追跡番号(クリックポスト等)<input name="tracking" required maxlength="40" placeholder="1234-5678-9012"></label>
<button class="btn" type="submit">発送完了として提出する</button></form></div>"#,
                    esc(&j.label),
                    enc,
                    esc(&render_addr_gated(&j.ship_json, true, worker.trust_tier)),
                    esc(&j.sku),
                    esc(&q.token),
                    j.order_id
                ));
            } else {
                // 仕上げて届ける担当中(unbox): 大きな文字・見本写真・proof写真必須
                cards.push_str(&format!(
                    r#"<div class="card"><span class="tag mine">あなたが担当中(仕上げて届ける)</span>
<h2 style="margin:8px 0 4px">{}</h2>
<img class="hero-img" src="{img}/step3_pack.png" alt="包装の見本" loading="lazy">
<table>
<tr><td>① 検品</td><td>品物にキズ・汚れがないか見てください</td></tr>
<tr><td>② 包む</td><td>薄紙でくるみ、MUのシールで封をする。ひとことカードは任意です</td></tr>
<tr><td>③ 届け先</td><td><pre>{}</pre></td></tr>
<tr><td>SKU</td><td class="muted">{}</td></tr>
</table>
<p class="bignote">完成した品の<b>写真を1枚</b>とって、下からアップしてください。<br><b>宛名・住所・追跡番号は写さないで</b>ください。投函してから追跡番号を入れます。</p>
<form method="POST" action="/api/work/proof" enctype="multipart/form-data" style="margin-top:10px">
<input type="hidden" name="token" value="{}"><input type="hidden" name="order_id" value="{}">
<input type="hidden" name="stage" value="posted">
<label>追跡番号(投函後)<input name="tracking" required maxlength="40" placeholder="1234-5678-9012"></label>
<label>完成写真<input type="file" name="file" accept="image/*" required></label>
<label style="display:flex;gap:8px;align-items:flex-start;font-size:14px;font-weight:400">
<input type="checkbox" name="pii_clear" value="1" required style="width:auto;margin-top:3px;flex:0 0 auto">
<span>この写真に<b>住所・宛名・追跡番号は写っていません</b>。</span></label>
<button class="btn" type="submit">写真を送って提出する</button></form></div>"#,
                    esc(&j.label),
                    esc(&render_addr_gated(&j.ship_json, true, worker.trust_tier)),
                    esc(&j.sku),
                    esc(&q.token),
                    j.order_id,
                    img = "https://raw.githubusercontent.com/yukihamada/mu-mockups/main/work",
                ));
            }
        } else {
            cards.push_str(&format!(
                r#"<div class="card"><span class="tag">他のワーカーが担当中</span>
<h2 style="margin:8px 0 4px">{}</h2><p class="muted">{}</p></div>"#,
                esc(&j.label),
                esc(&j.fulfillment_route)
            ));
        }
    }
    if jobs.is_empty() {
        cards = "<div class=\"card\"><p>いまは仕事がありません。注文が入るとここに表示されます。</p></div>".into();
    }

    // 今月の見込み件数レンジ(過去の発送実績ベースのざっくり目安)。
    let projected = if shipped_count >= 1 {
        format!("これまで {} 件。<b>今月 {}〜{} 件</b>くらいの見込みです", shipped_count, shipped_count, shipped_count * 2)
    } else {
        "まずは1件、ためしてみてください".to_string()
    };
    let tier_note = if worker.trust_tier == 0 {
        "いまは<b>ハブ経由</b>(運営宛に返送)です。お客様の住所は表示されません。実績がたまると直送もお選びいただけます。"
    } else {
        "直送できます(宛名印字用にお客様の住所が表示されます・発送後は破棄してください)。"
    };

    let body = format!(
        r#"<div class="eyebrow">MU — 仕事ページ</div>
<h1>{}さんのページ</h1>
<div class="bignote">
完了 <b>{}</b> 件 ／ 確定した報酬(現金) <b>¥{}</b>(月末締め・翌月払い)<br>
{}<br>
糸(ITO) <b>{} 粒(仮計上)</b> — 10粒で1着と交換できます(交換機能は準備中)<br>
<span class="muted">{}</span><br>
<span class="muted">確定申告の目安: 年20万円。<a href="/work/payouts?token={}">明細を見る</a></span>
</div>
{}
<h2>音コインの書込のやり方(音コイン担当の方だけ)</h2>
<ol class="muted" style="font-size:13.5px">
<li>App Store / Google Play で「<b>NFC Tools</b>」(無料)を入れる</li>
<li>「書く」→「レコード追加」→「URL」→ 上の①のURLを貼り付け→「書く」→コインにかざす</li>
<li>書込後「その他」→「読み取り専用にする」でロック(改ざん防止・必須)</li>
<li>自分のスマホをかざして音のページが開けば検品OK</li>
</ol>"#,
        esc(&worker.name),
        shipped_count,
        earned_cash,
        projected,
        ito_pending,
        tier_note,
        esc(&q.token),
        cards
    );
    page("仕事ページ", &body)
}

// ── POST /api/work/claim ────────────────────────────────────────────────
#[derive(Deserialize)]
pub struct ClaimForm {
    pub token: String,
    pub order_id: i64,
    #[serde(default)]
    pub job_kind: String,
}

pub async fn work_claim(State(db): State<Db>, headers: HeaderMap, Form(f): Form<ClaimForm>) -> Response {
    let ip = crate::client_ip(&headers);
    let job_kind = if f.job_kind.trim() == "oto" { "oto" } else { "unbox" };
    let result: Result<i64, Response> = {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        let Some(w) = worker_of(&conn, &f.token) else {
            return (StatusCode::UNAUTHORIZED, "bad token").into_response();
        };
        // fail-closed ゲート(二段防御):
        //  - oto は NFC 対応端末が必須。
        //  - unbox は新人(tier0)でも可だが、必ずハブ経由(住所非開示)になる。
        if job_kind == "oto" && w.nfc_capable == 0 {
            audit(&conn, Some(w.id), Some(f.order_id), "claim_reject_nfc", &ip, "{}");
            return (
                StatusCode::FORBIDDEN,
                "この端末では音コインのお仕事はできません(NFC非対応)。仕上げて届けるをお選びください。",
            )
                .into_response();
        }
        // 原子的に確保: pending のときだけ assigned へ(早い者勝ち・二重取り防止)
        let n = conn
            .execute(
                "UPDATE catalog_orders SET status='manual_assigned' WHERE id=? AND status='manual_pending'",
                rusqlite::params![f.order_id],
            )
            .unwrap_or(0);
        if n == 1 {
            // review_state='claimed'(報酬held)。確定はあと(承認後)。
            let _ = conn.execute(
                "INSERT OR REPLACE INTO work_assignments
                   (order_id, worker_id, fee_jpy, job_kind, review_state, ito_grains)
                 VALUES (?,?,?,?,'claimed',?)",
                rusqlite::params![f.order_id, w.id, fee_for(job_kind), job_kind, ito_grains_for(job_kind)],
            );
            audit(&conn, Some(w.id), Some(f.order_id), "claim", &ip, &format!("{{\"job_kind\":\"{}\"}}", job_kind));
            Ok(w.id)
        } else {
            Err(Redirect::to(&format!("/work/queue?token={}", f.token)).into_response())
        }
    };
    match result {
        Ok(wid) => {
            // 通知は worker_id のみ(作業者PII保護)。
            let _ = crate::send_telegram_message(&format!(
                "🧵 work: order#{} を worker#{} が引き受けました ({})",
                f.order_id, wid, job_kind
            ))
            .await;
            Redirect::to(&format!("/work/queue?token={}", f.token)).into_response()
        }
        Err(resp) => resp,
    }
}

// ── POST /api/work/ship — 音コイン(oto): 追跡番号で proof_submitted へ ─────
// 報酬はここでは確定しない(運営承認まで held)。
#[derive(Deserialize)]
pub struct ShipForm {
    pub token: String,
    pub order_id: i64,
    pub tracking: String,
    #[serde(default)]
    pub photo_url: String,
}

pub async fn work_ship(State(db): State<Db>, headers: HeaderMap, Form(f): Form<ShipForm>) -> Response {
    let ip = crate::client_ip(&headers);
    let tracking = f.tracking.trim().to_string();
    if tracking.is_empty() {
        return (StatusCode::BAD_REQUEST, "tracking required").into_response();
    }
    let ok: Option<i64> = {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        let Some(w) = worker_of(&conn, &f.token) else {
            return (StatusCode::UNAUTHORIZED, "bad token").into_response();
        };
        // 自分の担当 & まだ claimed/rework のときだけ提出にできる。
        let n = conn
            .execute(
                "UPDATE work_assignments
                   SET shipped_at=datetime('now'), tracking=?, review_state='proof_submitted'
                 WHERE order_id=? AND worker_id=? AND review_state IN ('claimed','rework')",
                rusqlite::params![tracking, f.order_id, w.id],
            )
            .unwrap_or(0);
        if n != 1 {
            None
        } else {
            // 注文ステータスは shipped へ(配送自体は完了。報酬確定だけが後ろ倒し)。
            let _ = conn.execute(
                "UPDATE catalog_orders SET status='manual_shipped' WHERE id=? AND status='manual_assigned'",
                rusqlite::params![f.order_id],
            );
            audit(&conn, Some(w.id), Some(f.order_id), "ship", &ip, "{}");
            Some(w.id)
        }
    };
    if let Some(wid) = ok {
        let _ = crate::send_telegram_message(&format!(
            "📮 work: order#{} 投函提出(確認待ち) by worker#{} (追跡 {})\n承認→ /admin/work/review?token=ADMIN",
            f.order_id, wid, tracking
        ))
        .await;
    }
    Redirect::to(&format!("/work/queue?token={}", f.token)).into_response()
}

// ── 月次振込オペ ────────────────────────────────────────────────────────
// GET  /admin/work/payouts?token=&month=YYYY-MM   … 月次集計(JSON・未払いのみ)
// GET  /admin/work/mark_paid?token=&worker_id=&month=YYYY-MM … 振込済みマーク(冪等)
// 「月末締め・翌月振込」の実体。振込そのものは人間(銀行)。手順: docs/WORK_OPERATIONS.md
#[derive(Deserialize)]
pub struct PayoutQuery {
    pub token: String,
    pub month: Option<String>,
    pub worker_id: Option<i64>,
}

fn month_range(month: &str) -> Option<(String, String)> {
    // "YYYY-MM" → (月初, 翌月初)。datetime('now')形式(UTC)と直接比較できる文字列。
    let (y, m) = month.split_once('-')?;
    let y: i32 = y.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    if !(1..=12).contains(&m) {
        return None;
    }
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    Some((format!("{:04}-{:02}-01", y, m), format!("{:04}-{:02}-01", ny, nm)))
}

pub async fn admin_payouts(State(db): State<Db>, Query(q): Query<PayoutQuery>) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let month = q.month.clone().unwrap_or_default();
    let Some((from, to)) = month_range(&month) else {
        return (StatusCode::BAD_REQUEST, "month=YYYY-MM required").into_response();
    };
    let conn = db.lock().unwrap();
    ensure_tables(&conn);
    let rows: Vec<serde_json::Value> = {
        let mut stmt = conn
            .prepare(
                "SELECT w.id, w.name, w.email, COALESCE(w.rate_jpy,200),
                        COUNT(*), COALESCE(SUM(a.fee_jpy),0)
                 FROM work_assignments a JOIN work_workers w ON w.id = a.worker_id
                 WHERE a.shipped_at >= ? AND a.shipped_at < ? AND a.paid_at IS NULL
                 GROUP BY w.id ORDER BY SUM(a.fee_jpy) DESC",
            )
            .unwrap();
        stmt.query_map(rusqlite::params![from, to], |r| {
            Ok(serde_json::json!({
                "worker_id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)?,
                "email": r.get::<_, String>(2)?, "rate_jpy": r.get::<_, i64>(3)?,
                "jobs": r.get::<_, i64>(4)?, "total_jpy": r.get::<_, i64>(5)?
            }))
        })
        .unwrap()
        .filter_map(|x| x.ok())
        .collect()
    };
    let total: i64 = rows.iter().map(|r| r["total_jpy"].as_i64().unwrap_or(0)).sum();
    let body = serde_json::json!({
        "month": month, "unpaid_workers": rows.len(), "unpaid_total_jpy": total,
        "rows": rows,
        "next": "振込したら /admin/work/mark_paid?worker_id=<id>&month=<YYYY-MM>&token=… で確定"
    })
    .to_string();
    ([(axum::http::header::CONTENT_TYPE, "application/json")], body).into_response()
}

pub async fn admin_mark_paid(State(db): State<Db>, Query(q): Query<PayoutQuery>) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let (Some(wid), Some(month)) = (q.worker_id, q.month.clone()) else {
        return (StatusCode::BAD_REQUEST, "worker_id & month=YYYY-MM required").into_response();
    };
    let Some((from, to)) = month_range(&month) else {
        return (StatusCode::BAD_REQUEST, "month=YYYY-MM required").into_response();
    };
    let n = {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        // 冪等: 未払い分のみ paid_at を打つ(二重払い防止の台帳側マーカー)
        conn.execute(
            "UPDATE work_assignments SET paid_at=datetime('now')
             WHERE worker_id=? AND shipped_at >= ? AND shipped_at < ? AND paid_at IS NULL",
            rusqlite::params![wid, from, to],
        )
        .unwrap_or(0)
    };
    let _ = crate::send_telegram_message(&format!(
        "💴 work: worker#{} の {} 分 {}件を振込済みにマーク",
        wid, month, n
    ))
    .await;
    let body = serde_json::json!({"worker_id": wid, "month": month, "marked": n}).to_string();
    ([(axum::http::header::CONTENT_TYPE, "application/json")], body).into_response()
}

// ── POST /api/work/proof — unbox: 完成写真を提出(EXIF/GPS除去・R2保存) ────
// multipart: token, order_id, stage, tracking, pii_clear, file。
// 写真は image crate で decode→PNG 再エンコードして EXIF/GPS を必ず除去。
// 剥がせなければ受理拒否(fail-closed)。住所/氏名は work_proofs に一切持たない。
pub async fn work_proof(
    State(db): State<Db>,
    headers: HeaderMap,
    mut multipart: axum::extract::Multipart,
) -> Response {
    let ip = crate::client_ip(&headers);
    let mut token = String::new();
    let mut order_id: i64 = 0;
    let mut stage = String::from("posted");
    let mut tracking = String::new();
    let mut pii_clear: i64 = 0;
    let mut file_bytes: Option<axum::body::Bytes> = None;
    while let Some(field) = match multipart.next_field().await {
        Ok(f) => f,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("multipart: {}", e)).into_response(),
    } {
        match field.name().unwrap_or("") {
            "token" => token = field.text().await.unwrap_or_default(),
            "order_id" => order_id = field.text().await.ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0),
            "stage" => stage = field.text().await.unwrap_or_else(|_| "posted".into()),
            "tracking" => tracking = field.text().await.unwrap_or_default(),
            "pii_clear" => pii_clear = if field.text().await.unwrap_or_default().is_empty() { 0 } else { 1 },
            "file" => file_bytes = field.bytes().await.ok(),
            _ => {}
        }
    }
    let stage = match stage.as_str() {
        "wrapped" | "sealed" | "posted" => stage,
        _ => "posted".into(),
    };
    let tracking = tracking.trim().to_string();
    // fail-closed: 自己申告も必須。住所/宛名/追跡が写っていない宣言が無ければ受理しない。
    if pii_clear == 0 {
        return (StatusCode::BAD_REQUEST, "写真に住所・宛名・追跡番号が写っていないことの確認が必要です").into_response();
    }
    let raw = match file_bytes {
        Some(b) if !b.is_empty() && b.len() <= 12 * 1024 * 1024 => b,
        Some(_) => return (StatusCode::BAD_REQUEST, "ファイルが大きすぎます(12MBまで)").into_response(),
        None => return (StatusCode::BAD_REQUEST, "写真がありません").into_response(),
    };
    // image crate で decode → PNG 再エンコード(EXIF/GPS を確実に除去)。
    // decode できない/再エンコードできない = fail-closed で受理拒否。
    let png_bytes: Vec<u8> = {
        let img = match image::load_from_memory(&raw) {
            Ok(i) => i,
            Err(_) => return (StatusCode::BAD_REQUEST, "画像を読み取れませんでした。もう一度撮り直してください。").into_response(),
        };
        let mut buf = std::io::Cursor::new(Vec::new());
        if img.write_to(&mut buf, image::ImageFormat::Png).is_err() {
            return (StatusCode::INTERNAL_SERVER_ERROR, "画像処理に失敗しました。もう一度お試しください。").into_response();
        }
        buf.into_inner()
    };
    // sha256(再エンコード後の正規化画像)。使い回し検知。
    use sha2::{Sha256, Digest};
    let mut h = Sha256::new();
    h.update(&png_bytes);
    let sha = hex::encode(h.finalize());
    let key = format!("work/proof/{}.png", &sha[..16]);
    // R2 へ保存(失敗時 fail-closed)。住所は object_key に含めない。
    let Some(_url) = crate::store_r2_bytes(&key, &png_bytes, "image/png").await else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "写真の保存に失敗しました。もう一度お試しください。").into_response();
    };
    let result: Result<i64, (StatusCode, &str)> = {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        let Some(w) = worker_of(&conn, &token) else {
            return (StatusCode::UNAUTHORIZED, "bad token").into_response();
        };
        // sha 重複(写真の使い回し)は UNIQUE で弾く。
        let inserted = conn.execute(
            "INSERT INTO work_proofs (order_id, worker_id, stage, object_key, sha256, exif_stripped, pii_clear, is_public)
             VALUES (?,?,?,?,?,1,?,0)",
            rusqlite::params![order_id, w.id, stage, key, sha, pii_clear],
        );
        if inserted.is_err() {
            audit(&conn, Some(w.id), Some(order_id), "proof_reject_dup", &ip, "{}");
            return (StatusCode::CONFLICT, "この写真は既に使われています。実際の梱包写真をアップしてください。").into_response();
        }
        // 自分の担当 & claimed/rework のときだけ提出に進める。tracking があれば記録。
        let n = conn
            .execute(
                "UPDATE work_assignments
                   SET review_state='proof_submitted',
                       shipped_at=COALESCE(shipped_at, datetime('now')),
                       tracking=COALESCE(NULLIF(?, ''), tracking)
                 WHERE order_id=? AND worker_id=? AND review_state IN ('claimed','rework')",
                rusqlite::params![tracking, order_id, w.id],
            )
            .unwrap_or(0);
        if n != 1 {
            Err((StatusCode::CONFLICT, "この注文は提出できる状態ではありません。"))
        } else {
            // 投函済みなら注文ステータスも shipped に進める(報酬確定はまだ)。
            let _ = conn.execute(
                "UPDATE catalog_orders SET status='manual_shipped' WHERE id=? AND status='manual_assigned'",
                rusqlite::params![order_id],
            );
            audit(&conn, Some(w.id), Some(order_id), "proof", &ip, &format!("{{\"stage\":\"{}\"}}", stage));
            Ok(w.id)
        }
    };
    match result {
        Ok(wid) => {
            let _ = crate::send_telegram_message(&format!(
                "🎁 work: order#{} 開封パック提出(確認待ち) by worker#{} stage={}\n承認→ /admin/work/review?token=ADMIN",
                order_id, wid, stage
            ))
            .await;
            Redirect::to(&format!("/work/queue?token={}", token)).into_response()
        }
        Err((s, m)) => (s, m).into_response(),
    }
}

// ── GET /admin/work/review?token=[&approve=ORDER_ID][&rework=ORDER_ID][&defect=ORDER_ID] ──
// 運営が証跡を確認 → approve で報酬released(現金 mu_credit_apply + 糸 work_ito_grants)、
// rework で差し戻し、defect で不良再手配(報酬は減らさない)。冪等(ref_id='order:{id}')。
#[derive(Deserialize)]
pub struct ReviewQuery {
    pub token: String,
    #[serde(default)]
    pub approve: Option<i64>,
    #[serde(default)]
    pub rework: Option<i64>,
    #[serde(default)]
    pub defect: Option<i64>,
}

pub async fn admin_review(State(db): State<Db>, headers: HeaderMap, Query(q): Query<ReviewQuery>) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let ip = crate::client_ip(&headers);
    let mut flash = String::new();

    if let Some(oid) = q.approve {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        // 提出済みの担当を取得(冪等: proof_submitted のときだけ確定)。
        let row: Option<(i64, i64, i64, String)> = conn
            .query_row(
                "SELECT worker_id, fee_jpy, COALESCE(ito_grains,0), COALESCE(review_state,'')
                 FROM work_assignments WHERE order_id=?",
                rusqlite::params![oid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .ok();
        match row {
            Some((wid, fee, grains, state)) if state == "proof_submitted" => {
                // worker の email を引く(現金台帳は email キー)。
                let email: String = conn
                    .query_row("SELECT email FROM work_workers WHERE id=?", rusqlite::params![wid], |r| r.get(0))
                    .unwrap_or_default();
                let ref_id = format!("order:{}", oid);
                // 現金: mu_credit_apply(reason='work_cash')。糸: work_ito_grants(冪等 ref_id)。
                if !email.is_empty() {
                    let _ = crate::mu_credit_apply(&conn, &email, fee, "work_cash", Some(&ref_id));
                }
                if grains > 0 {
                    let _ = conn.execute(
                        "INSERT OR IGNORE INTO work_ito_grants (worker_id, order_id, grains, ref_id) VALUES (?,?,?,?)",
                        rusqlite::params![wid, oid, grains, ref_id],
                    );
                }
                let _ = conn.execute(
                    "UPDATE work_assignments SET review_state='approved', approved_at=datetime('now') WHERE order_id=?",
                    rusqlite::params![oid],
                );
                audit(&conn, Some(wid), Some(oid), "approve", &ip, &format!("{{\"cash\":{},\"grains\":{}}}", fee, grains));
                flash = format!("<p class=\"bignote\">order#{} を承認しました。現金 ¥{} + 糸 {}粒(仮計上)を worker#{} に付与しました。</p>", oid, fee, grains, wid);
            }
            Some((_, _, _, state)) => {
                flash = format!("<p class=\"muted\">order#{} は承認できる状態ではありません(現在: {})。</p>", oid, esc(&state));
            }
            None => flash = format!("<p class=\"muted\">order#{} の担当が見つかりません。</p>", oid),
        }
    } else if let Some(oid) = q.rework {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        let n = conn
            .execute(
                "UPDATE work_assignments SET review_state='rework' WHERE order_id=? AND review_state='proof_submitted'",
                rusqlite::params![oid],
            )
            .unwrap_or(0);
        if n == 1 {
            audit(&conn, None, Some(oid), "rework", &ip, "{}");
            flash = format!("<p class=\"bignote\">order#{} を差し戻しました(rework)。作業者がもう一度提出できます。報酬は減りません。</p>", oid);
        } else {
            flash = format!("<p class=\"muted\">order#{} は差し戻せません。</p>", oid);
        }
    } else if let Some(oid) = q.defect {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        let n = conn
            .execute(
                "UPDATE work_assignments SET review_state='defect_reported' WHERE order_id=?",
                rusqlite::params![oid],
            )
            .unwrap_or(0);
        if n == 1 {
            audit(&conn, None, Some(oid), "defect", &ip, "{}");
            flash = format!("<p class=\"bignote\">order#{} を配送/検品不良(defect)としました。当社で再手配します。作業者報酬は減らしません。</p>", oid);
        } else {
            flash = format!("<p class=\"muted\">order#{} を更新できません。</p>", oid);
        }
    }

    // 確認待ち一覧 + 直近の証跡。住所は一切出さない(order# とラベルのみ)。
    let rows = {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        let mut stmt = conn
            .prepare(
                "SELECT a.order_id, a.worker_id, a.job_kind, a.fee_jpy, COALESCE(a.ito_grains,0),
                        COALESCE(p.label, o.sku),
                        (SELECT object_key FROM work_proofs wp WHERE wp.order_id=a.order_id ORDER BY id DESC LIMIT 1),
                        COALESCE(a.tracking,'')
                 FROM work_assignments a
                 LEFT JOIN catalog_orders o ON o.id = a.order_id
                 LEFT JOIN catalog_products p ON p.sku = o.sku
                 WHERE a.review_state='proof_submitted'
                 ORDER BY a.order_id ASC LIMIT 100",
            )
            .unwrap();
        let mut html = String::new();
        let it = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, String>(7)?,
                ))
            })
            .unwrap();
        for row in it.flatten() {
            let (oid, wid, kind, fee, grains, label, key, tracking) = row;
            let proof = key
                .as_deref()
                .map(|_| "写真あり(R2に保存済み)".to_string())
                .unwrap_or_else(|| "(写真未提出・oto)".into());
            html.push_str(&format!(
                r#"<div class="card"><b>order#{}</b> ｜ {} ｜ {} ｜ worker#{}<br>
報酬 ¥{} ＋ 糸 {}粒 ／ 追跡 {} ／ {}<br>
<a class="btn green" href="/admin/work/review?token={}&approve={}">承認(報酬確定)</a>
<a class="btn" href="/admin/work/review?token={}&rework={}">差し戻し</a>
<a class="muted" href="/admin/work/review?token={}&defect={}" style="margin-left:8px">不良として再手配</a></div>"#,
                oid,
                esc(&label),
                esc(kind.as_deref().unwrap_or("oto")),
                wid,
                fee,
                grains,
                esc(&tracking),
                esc(&proof),
                esc(&q.token), oid,
                esc(&q.token), oid,
                esc(&q.token), oid,
            ));
        }
        if html.is_empty() {
            html = "<div class=\"card\"><p>確認待ちはありません。</p></div>".into();
        }
        html
    };

    let body = format!(
        r#"<div class="eyebrow">MU — 作業証跡レビュー(運営)</div>
<h1>確認待ち</h1>
{}
{}
<p class="muted"><a href="/admin/work/payout_sheet?token={}">報酬集計シートへ</a></p>"#,
        flash, rows, esc(&q.token)
    );
    page("作業証跡レビュー", &body)
}

// ── GET /work/payouts?token= — ワーカーの報酬明細(現金/糸を分離・確定申告用) ──
pub async fn work_payouts(State(db): State<Db>, Query(q): Query<QueueQuery>) -> Response {
    let (name, cash_rows, cash_total, grain_total): (String, String, i64, i64) = {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        let Some(w) = worker_of(&conn, &q.token) else {
            return page("リンクが無効です", "<h1>このリンクは無効です</h1><p>承認メールのリンクをご確認ください。</p>");
        };
        let email: String = conn
            .query_row("SELECT email FROM work_workers WHERE id=?", rusqlite::params![w.id], |r| r.get(0))
            .unwrap_or_default();
        // 確定現金(work_cash) を月別に。住所は出さない。
        let mut stmt = conn
            .prepare(
                "SELECT strftime('%Y-%m', datetime(created_at,'unixepoch')) AS ym, COUNT(*), COALESCE(SUM(delta_jpy),0)
                 FROM mu_credit_ledger WHERE email=? AND reason='work_cash'
                 GROUP BY ym ORDER BY ym DESC",
            )
            .unwrap();
        let mut rows = String::new();
        let it = stmt
            .query_map(rusqlite::params![email], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
            })
            .unwrap();
        for row in it.flatten() {
            let (ym, cnt, sum) = row;
            rows.push_str(&format!(
                "<tr><td>{}</td><td>{} 件</td><td style=\"text-align:right\">¥{}</td></tr>",
                esc(&ym), cnt, sum
            ));
        }
        let cash_total: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(delta_jpy),0) FROM mu_credit_ledger WHERE email=? AND reason='work_cash'",
                rusqlite::params![email],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let grain_total: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(grains),0) FROM work_ito_grants WHERE worker_id=?",
                rusqlite::params![w.id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        (w.name, rows, cash_total, grain_total)
    };

    let table = if cash_rows.is_empty() {
        "<div class=\"card\"><p>確定した報酬はまだありません。</p></div>".to_string()
    } else {
        format!(
            "<table style=\"width:100%\"><tr><td>月</td><td>件数</td><td style=\"text-align:right\">確定報酬</td></tr>{}</table>",
            cash_rows
        )
    };
    let alert = if cash_total >= 200_000 {
        "<p class=\"bignote\">今年の確定報酬が <b>20万円</b>を超えました。確定申告が必要な場合があります(目安・税理士にご確認ください)。</p>"
    } else {
        ""
    };
    let body = format!(
        r#"<div class="eyebrow">MU — 報酬明細</div>
<h1>{}さんの報酬</h1>
<div class="bignote">
現金(確定) 合計 <b>¥{}</b>(月末締め・翌月銀行振込・振込手数料は当社負担)<br>
糸(ITO) <b>{} 粒(仮計上)</b> — 糸の交換機能は準備中です。10粒で1着と交換できます。
</div>
{}
{}
<p class="muted">現金と糸は別のものです。現金だけが課税対象です。糸は服と交換できる社内ポイントで、いまは仮計上(交換機能の準備中)です。CSVが必要な方は info@enablerdao.com へ。</p>"#,
        esc(&name), cash_total, grain_total, alert, table
    );
    page("報酬明細", &body)
}

// ── GET /admin/work/payout_sheet?token= — 運営用・work_cash 集計(表示のみ) ──
// 実振込は常に人間ゲート(BUDGET §3)。このページは集計を出すだけ。
pub async fn admin_payout_sheet(State(db): State<Db>, Query(q): Query<QueueQuery>) -> Response {
    let expected = env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let (rows, total): (String, i64) = {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        // worker 別の確定現金合計(work_cash)。住所/口座平文は出さない。
        let mut stmt = conn
            .prepare(
                "SELECT ww.id, COALESCE(SUM(l.delta_jpy),0) AS cash, COUNT(*) AS cnt
                 FROM mu_credit_ledger l
                 JOIN work_workers ww ON ww.email = l.email
                 WHERE l.reason='work_cash'
                 GROUP BY ww.id ORDER BY cash DESC",
            )
            .unwrap();
        let mut rows = String::new();
        let it = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))
            .unwrap();
        let mut total = 0i64;
        for row in it.flatten() {
            let (wid, cash, cnt) = row;
            total += cash;
            let flag = if cash >= 30_000 { " ⚠ ¥30,000超(BUDGET §3ゲート)" } else { "" };
            rows.push_str(&format!(
                "<tr><td>worker#{}</td><td>{} 件</td><td style=\"text-align:right\">¥{}</td><td class=\"muted\">{}</td></tr>",
                wid, cnt, cash, flag
            ));
        }
        (rows, total)
    };
    let table = if rows.is_empty() {
        "<div class=\"card\"><p>確定済みの報酬はありません。</p></div>".to_string()
    } else {
        format!(
            "<table style=\"width:100%\"><tr><td>ワーカー</td><td>件数</td><td style=\"text-align:right\">確定現金</td><td></td></tr>{}</table>",
            rows
        )
    };
    let body = format!(
        r#"<div class="eyebrow">MU — 報酬集計(運営)</div>
<h1>確定現金の支払い対象</h1>
<p class="bignote">合計 <b>¥{}</b>。<b>実際の振込は人間が行います</b>(このページは集計のみ・自動送金しません)。新規ワーカー初回・月¥30,000超は BUDGET §3 のゲートで承認してください。</p>
{}
<p class="muted">糸(ITO)は work_ito_grants に粒数で別計上。現金台帳(mu_credit_ledger, reason='work_cash')には粒数を入れません。</p>"#,
        total, table
    );
    page("報酬集計(運営)", &body)
}
