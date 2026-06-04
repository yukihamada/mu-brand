// work.rs — 在宅ワーカー向け「音コイン」フルフィルメント・ジョブ基盤。
//
// manual ルート(NFC音コイン)の注文を、在宅ワーカーが自分のスマホで
// NFC書込→検品→梱包→発送できるジョブキューにする。
//   /work               … 求人LP(公開)。応募フォーム付き
//   POST /api/work/apply … 応募(承認待ち) → Telegramで運営に通知
//   GET  /admin/work/approve?token=&id= … 運営承認 → worker_token発行+メール
//   GET  /work/queue?token= … ワーカー専用キュー(着手/発送完了)
//   POST /api/work/claim … 仕事を引き受ける(原子的: manual_pending→manual_assigned)
//   POST /api/work/ship  … 発送完了(追跡番号) → 顧客へ発送メール+台帳記帳
//
// 注文ステータスは catalog_orders.status を単一ソースにする(契約準拠):
//   manual_pending → manual_assigned → manual_shipped
// ワーカー帰属・報酬は work_assignments(注文1行=1ジョブ)に持つ。
// 報酬単価は env WORK_FEE_JPY (既定 ¥300/件)。

use axum::{
    extract::{Form, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use std::env;

use crate::Db;

fn fee_jpy() -> i64 {
    env::var("WORK_FEE_JPY").ok().and_then(|v| v.parse().ok()).unwrap_or(300)
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
            approved_at TEXT
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

fn page(title: &str, body: &str) -> Response {
    let html = format!(
        r#"<!doctype html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="robots" content="noindex">
<title>{title}｜MU</title>
<style>
:root{{--ink:#111;--sub:#666;--line:#e5e5e5;--accent:#1f8a4c}}
body{{font-family:-apple-system,"Hiragino Sans",sans-serif;color:var(--ink);max-width:640px;margin:0 auto;padding:32px 20px 80px;line-height:1.9}}
h1{{font-size:24px;margin:0 0 4px}} h2{{font-size:17px;margin:36px 0 8px}}
.eyebrow{{font-size:12px;letter-spacing:.18em;color:var(--sub)}}
.card{{border:1px solid var(--line);border-radius:12px;padding:18px 20px;margin:14px 0}}
.muted{{color:var(--sub);font-size:13px}}
.btn{{display:inline-block;background:var(--ink);color:#fff;border:0;border-radius:8px;padding:11px 20px;font-size:14px;font-weight:700;cursor:pointer;text-decoration:none}}
.btn.green{{background:var(--accent)}}
input,select{{font-size:15px;padding:10px 12px;border:1px solid #ccc;border-radius:8px;width:100%;box-sizing:border-box;margin:4px 0 12px}}
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
@media(max-width:480px){{.steps img{{width:88px;height:64px}}}}
</style></head><body>{body}</body></html>"#,
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

// ── GET /work — 求人LP ──────────────────────────────────────────────────
pub async fn work_page() -> Response {
    let fee = fee_jpy();
    let body = format!(
        r#"<div class="eyebrow">MU — おうちでできる仕事</div>
<h1>音コインを、つくって届ける。</h1>
<img class="hero-img" src="{img}/step2_write.png" alt="スマホでNFCコインに書き込む様子" loading="lazy">
<p>MUの「音コイン」(かざすと音が鳴るNFCコイン・¥1,800)を、<b>自宅でNFC書込→検品→梱包→発送</b>する出来高制のお仕事です。1件あたり10分ほど。特別なスキルはいりません。</p>
<p><a class="btn green" href='#apply'>応募する（30秒）</a></p>

<div class="brand">
<div class="eyebrow" style="color:#888">作っているブランドのこと</div>
<h2>MU ／ 音コイン(OTO)</h2>
<p style="margin:6px 0">MU(ムー)は、<b>AIが毎時1着、服やプロダクトを生み出す</b>新しいものづくりブランドです。在庫を持たず、注文が入ってから作る。数字は<a href="https://wearmu.com/transparency">/transparency</a>で全部公開しています。</p>
<p style="margin:6px 0">その中の<b>「音コイン」</b>は、手のひらサイズの黒いコイン。スマホをかざすと、その人のための一曲が鳴ります(声・音は<a href="https://koe.live">Koe</a>で作られたもの)。鍵やバッグ、道着に付けて持ち歩く、"音のおまもり"です。</p>
<p class="muted" style="margin:6px 0 0;font-size:12.5px">あなたが書き込んで届けたコインから、誰かの毎日に音が灯ります。運営: 株式会社イネブラ。</p>
</div>

<h2>仕事の流れ</h2>
<ul class="steps">
<li><img src="{img}/step1_kit.png" alt="" loading="lazy"><div><span class="n">STEP 1</span><br><b>キットを受け取る</b><br><span class="muted">ブランクのコイン・封筒・宛名シールをまとめてお送りします</span></div></li>
<li><img src="{img}/step2_write.png" alt="" loading="lazy"><div><span class="n">STEP 2</span><br><b>NFCに書き込む</b><br><span class="muted">無料アプリ(NFC Tools)で指定URLを書込→ロック(約30秒)</span></div></li>
<li><img src="{img}/step3_pack.png" alt="" loading="lazy"><div><span class="n">STEP 3</span><br><b>検品して封筒へ</b><br><span class="muted">自分のスマホでかざして音が鳴ればOK。封筒に入れ宛名を貼る</span></div></li>
<li><img src="{img}/step4_mail.png" alt="" loading="lazy"><div><span class="n">STEP 4</span><br><b>ポストに投函・完了報告</b><br><span class="muted">クリックポスト等で投函→追跡番号を入力。お客様への発送メールは自動</span></div></li>
</ul>

<table style="margin-top:18px">
<tr><td>報酬</td><td><b>¥{fee} / 件</b>(月末締め・翌月銀行振込・<b>振込手数料は当社負担</b>)</td></tr>
<tr><td>送料</td><td><b>当社負担</b>。クリックポスト用の予納分はキットに同梱します(立替不要)</td></tr>
<tr><td>必要なもの</td><td>NFC対応スマホ(iPhone 7以降 / 大半のAndroid)・ポストに行ける環境</td></tr>
<tr><td>時間</td><td>完全に自分のペース。引き受けた分だけ。<b>ノルマなし・いつでも辞められます</b></td></tr>
<tr><td>場所</td><td>日本国内どこでも</td></tr>
</table>

<h2>よくある質問</h2>
<div class="card">
<p><b>Q. 不良品・配送事故のときは？</b><br>再発送のコイン・送料は<b>当社が負担</b>します。あなたの自己負担はありません。</p>
<p><b>Q. ノルマや納期は？</b><br>ありません。引き受けた分だけ・自分のペースで。引き受けなければ通知が来るだけです。</p>
<p><b>Q. お客様の住所はどう扱う？</b><br>発送のためだけに使い、宛名を書いたら<b>すみやかに破棄</b>してください(応募時に同意いただきます)。第三者への提供は禁止です。</p>
<p><b>Q. 税金は？</b><br>業務委託のため、報酬は雑所得/事業所得になります。年間の合計額によっては確定申告が必要です(目安: 給与所得者で年20万円超など)。</p>
<p><b>Q. 辞めたいときは？</b><br>メール一本でOK。違約金などはありません。</p>
</div>

<h2 id="apply">応募する</h2>
<form method="POST" action="/api/work/apply" class="card">
<label>お名前<input name="name" required maxlength="60" placeholder="山田 はなこ"></label>
<label>メールアドレス<input name="email" type="email" required maxlength="120" placeholder="you@example.com"></label>
<label>お住まいの都道府県<input name="region" maxlength="20" placeholder="北海道"></label>
<label style="display:flex;gap:8px;align-items:flex-start;font-size:13px;font-weight:400">
<input type="checkbox" name="agree" required style="width:auto;margin-top:3px;flex:0 0 auto">
<span>お客様の氏名・住所などの配送情報を<b>発送目的のみ</b>に使い、第三者に渡さず、<b>発送後すみやかに破棄</b>することに同意します。</span></label>
<button class="btn" type="submit">応募する</button>
<p class="muted">承認されると、仕事キューのリンクをメールでお送りします。</p>
</form>
<p class="muted">運営: <b>株式会社イネブラ</b>(Enabler Inc.)／〒102-0074 東京都千代田区九段南1-5-6 りそな九段ビル5階KSフロア・業務委託。<br>質問は info@enablerdao.com へ。商品ページ: <a href="/shop?brand=oto">音コインを見る</a></p>"#,
        img = "https://raw.githubusercontent.com/yukihamada/mu-mockups/main/work",
    );
    page("おうちでできる仕事 — 音コイン", &body)
}

// ── POST /api/work/apply ────────────────────────────────────────────────
#[derive(Deserialize)]
pub struct ApplyForm {
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub agree: Option<String>,
}

pub async fn work_apply(State(db): State<Db>, Form(f): Form<ApplyForm>) -> Response {
    let name = f.name.trim().to_string();
    let email = f.email.trim().to_lowercase();
    let region = f.region.trim().to_string();
    if name.is_empty() || !email.contains('@') {
        return page("入力エラー", "<h1>お名前とメールアドレスを入力してください</h1><p><a href=\"/work\">戻る</a></p>");
    }
    // 個人情報(客の住所)の取扱い同意は必須(HTMLのrequiredに加えサーバ側でも検証)
    if f.agree.as_deref().unwrap_or("").is_empty() {
        return page("同意が必要です", "<h1>配送情報の取扱いへの同意が必要です</h1><p>お客様の住所をお預かりするため、取扱いへの同意にチェックをお願いします。</p><p><a href=\"/work#apply\">戻る</a></p>");
    }
    let worker_id: i64 = {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        let _ = conn.execute(
            "INSERT OR IGNORE INTO work_workers (email, name, region) VALUES (?,?,?)",
            rusqlite::params![email, name, region],
        );
        conn.query_row(
            "SELECT id FROM work_workers WHERE email=?",
            rusqlite::params![email],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };
    let admin = env::var("ADMIN_TOKEN").unwrap_or_default();
    let _ = crate::send_telegram_message(&format!(
        "🧵 *work応募* (音コイン在宅ワーカー)\n{} <{}> {}\n承認→ https://wearmu.com/admin/work/approve?id={}&token={}",
        name, email, region, worker_id, admin
    ))
    .await;
    page(
        "応募ありがとうございます",
        "<h1>応募を受け付けました。</h1><p>内容を確認して、承認されると<b>仕事キューのリンクをメール</b>でお送りします。少しお待ちください。</p>",
    )
}

// ── GET /admin/work/approve?token=&id= ──────────────────────────────────
#[derive(Deserialize)]
pub struct ApproveQuery {
    pub token: String,
    pub id: i64,
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
        "【MU おしごと】承認されました — 仕事キューのご案内",
        format!(
            r#"<div style="font-family:sans-serif;line-height:1.8"><p>{}さん</p>
<p>音コインのお仕事、承認されました。下のリンクがあなた専用の仕事キューです(ブックマーク推奨・他の人に共有しないでください)。</p>
<p><a href="{}" style="background:#111;color:#fff;padding:12px 22px;border-radius:8px;text-decoration:none;font-weight:700">仕事キューを開く</a></p>
<p>最初のキット(ブランクコイン・封筒・宛名シール)は別途お送りします。<br>— MU</p></div>"#,
            esc(&name),
            queue_url
        ),
    )
    .await;
    page(
        "承認しました",
        &format!(
            "<h1>承認しました。</h1><p>{} &lt;{}&gt; に仕事キューのリンクを{}。</p><p class=\"muted\">キット(ブランクコイン・封筒)の発送を忘れずに。</p>",
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
    tracking: Option<String>,
}

fn worker_of(conn: &rusqlite::Connection, token: &str) -> Option<(i64, String)> {
    if token.is_empty() {
        return None;
    }
    conn.query_row(
        "SELECT id, name FROM work_workers WHERE token=? AND status='active'",
        rusqlite::params![token],
        |r| Ok((r.get(0)?, r.get(1)?)),
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

pub async fn work_queue(State(db): State<Db>, Query(q): Query<QueueQuery>) -> Response {
    let fee = fee_jpy();
    let (worker_id, worker_name, jobs, shipped_count, earned): (i64, String, Vec<JobRow>, i64, i64) = {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        let Some((wid, wname)) = worker_of(&conn, &q.token) else {
            return page("リンクが無効です", "<h1>このリンクは無効です</h1><p>承認メールのリンクをご確認ください。応募は <a href=\"/work\">/work</a> から。</p>");
        };
        let mut stmt = conn
            .prepare(
                "SELECT o.id, o.sku, p.label, p.description_ja, o.status,
                        COALESCE(o.shipping_address_json,'{}'), a.worker_id, a.tracking
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
                    tracking: r.get(7)?,
                })
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        let (cnt, sum): (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(fee_jpy),0) FROM work_assignments WHERE worker_id=? AND shipped_at IS NOT NULL",
                rusqlite::params![wid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));
        (wid, wname, jobs, cnt, sum)
    };

    let mut cards = String::new();
    for j in &jobs {
        let mine = j.assigned_to == Some(worker_id);
        let enc = j
            .encode_url
            .as_deref()
            .map(|u| format!("<a href=\"{0}\">{0}</a>", esc(u)))
            .unwrap_or_else(|| "<span class=\"muted\">(書込URL不明 → 運営に確認)</span>".into());
        if j.status == "manual_pending" {
            cards.push_str(&format!(
                r#"<div class="card"><span class="tag">募集中</span>
<h2 style="margin:8px 0 4px">{}</h2>
<table><tr><td>届け先</td><td>{}</td></tr><tr><td>報酬</td><td>¥{}</td></tr></table>
<form method="POST" action="/api/work/claim" style="margin-top:10px">
<input type="hidden" name="token" value="{}"><input type="hidden" name="order_id" value="{}">
<button class="btn green" type="submit">この仕事を引き受ける</button></form></div>"#,
                esc(&j.label),
                esc(&render_addr(&j.ship_json, false)),
                fee_jpy(),
                esc(&q.token),
                j.order_id
            ));
        } else if mine {
            cards.push_str(&format!(
                r#"<div class="card"><span class="tag mine">あなたが担当中</span>
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
<button class="btn" type="submit">発送完了にする</button></form></div>"#,
                esc(&j.label),
                enc,
                esc(&render_addr(&j.ship_json, true)),
                esc(&j.sku),
                esc(&q.token),
                j.order_id
            ));
        } else {
            cards.push_str(&format!(
                r#"<div class="card"><span class="tag">他のワーカーが担当中</span>
<h2 style="margin:8px 0 4px">{}</h2><p class="muted">{}</p></div>"#,
                esc(&j.label),
                j.tracking.as_deref().map(esc).unwrap_or_default()
            ));
        }
    }
    if jobs.is_empty() {
        cards = "<div class=\"card\"><p>いまは仕事がありません。注文が入るとここに表示されます。</p></div>".into();
    }

    let body = format!(
        r#"<div class="eyebrow">MU — 仕事キュー</div>
<h1>{}さんのキュー</h1>
<p class="muted">完了 {} 件 ／ 報酬累計 <b>¥{}</b>(月末締め・翌月払い)・単価 ¥{}/件</p>
{}
<h2>書込のやり方(初回だけ読む)</h2>
<ol class="muted" style="font-size:13.5px">
<li>App Store / Google Play で「<b>NFC Tools</b>」(無料)を入れる</li>
<li>「書く」→「レコード追加」→「URL」→ 上の①のURLを貼り付け→「書く」→コインにかざす</li>
<li>書込後「その他」→「読み取り専用にする」でロック(改ざん防止・必須)</li>
<li>自分のスマホをかざして音のページが開けば検品OK</li>
</ol>"#,
        esc(&worker_name),
        shipped_count,
        earned,
        fee,
        cards
    );
    page("仕事キュー", &body)
}

// ── POST /api/work/claim ────────────────────────────────────────────────
#[derive(Deserialize)]
pub struct ClaimForm {
    pub token: String,
    pub order_id: i64,
}

pub async fn work_claim(State(db): State<Db>, Form(f): Form<ClaimForm>) -> Response {
    let claimed: Result<String, &str> = {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        let Some((wid, wname)) = worker_of(&conn, &f.token) else {
            return (StatusCode::UNAUTHORIZED, "bad token").into_response();
        };
        // 原子的に確保: pending のときだけ assigned へ(早い者勝ち・二重取り防止)
        let n = conn
            .execute(
                "UPDATE catalog_orders SET status='manual_assigned' WHERE id=? AND status='manual_pending'",
                rusqlite::params![f.order_id],
            )
            .unwrap_or(0);
        if n == 1 {
            let _ = conn.execute(
                "INSERT OR REPLACE INTO work_assignments (order_id, worker_id, fee_jpy) VALUES (?,?,?)",
                rusqlite::params![f.order_id, wid, fee_jpy()],
            );
            Ok(wname)
        } else {
            Err("conflict")
        }
    };
    if let Ok(wname) = claimed {
        let _ = crate::send_telegram_message(&format!(
            "🧵 work: order#{} を {} が引き受けました",
            f.order_id, wname
        ))
        .await;
    }
    Redirect::to(&format!("/work/queue?token={}", f.token)).into_response()
}

// ── POST /api/work/ship ─────────────────────────────────────────────────
#[derive(Deserialize)]
pub struct ShipForm {
    pub token: String,
    pub order_id: i64,
    pub tracking: String,
}

pub async fn work_ship(State(db): State<Db>, Form(f): Form<ShipForm>) -> Response {
    let tracking = f.tracking.trim().to_string();
    if tracking.is_empty() {
        return (StatusCode::BAD_REQUEST, "tracking required").into_response();
    }
    let done: Option<(String, String, String)> = {
        let conn = db.lock().unwrap();
        ensure_tables(&conn);
        let Some((wid, wname)) = worker_of(&conn, &f.token) else {
            return (StatusCode::UNAUTHORIZED, "bad token").into_response();
        };
        // 自分の担当 & 未発送のときだけ完了にできる
        let n = conn
            .execute(
                "UPDATE work_assignments SET shipped_at=datetime('now'), tracking=?
                 WHERE order_id=? AND worker_id=? AND shipped_at IS NULL",
                rusqlite::params![tracking, f.order_id, wid],
            )
            .unwrap_or(0);
        if n != 1 {
            None
        } else {
            let _ = conn.execute(
                "UPDATE catalog_orders SET status='manual_shipped' WHERE id=? AND status='manual_assigned'",
                rusqlite::params![f.order_id],
            );
            conn.query_row(
                "SELECT COALESCE(o.customer_email,''), p.label, ? FROM catalog_orders o JOIN catalog_products p ON p.sku=o.sku WHERE o.id=?",
                rusqlite::params![wname, f.order_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok()
        }
    };
    if let Some((email, label, wname)) = done {
        if !email.is_empty() {
            let _ = send_resend(
                &email,
                "【MU】音コインを発送しました",
                format!(
                    r#"<div style="font-family:sans-serif;line-height:1.8"><p>{} を発送しました。</p>
<p>追跡番号: <b>{}</b>(クリックポスト等)</p>
<p>届いたらスマホをかざしてみてください。音が鳴ります。<br>— MU</p></div>"#,
                    esc(&label),
                    esc(&tracking)
                ),
            )
            .await;
        }
        let _ = crate::send_telegram_message(&format!(
            "📮 work: order#{} 発送完了 by {} (追跡 {})",
            f.order_id, wname, tracking
        ))
        .await;
    }
    Redirect::to(&format!("/work/queue?token={}", f.token)).into_response()
}
