// Gemini 直叩きの自動フォールバック中継 (2026-11-23)。
//
// 背景: main.rs には Google Gemini API を `?key=GEMINI_API_KEY` で直接叩く箇所が
// 約 28 か所ある。2026-11-23 に Google AI Studio の前払いクレジットが枯渇し、
// 全箇所が `402 Payment Required` で失敗していた (お客様対応の返信草案・購入お祝い
// メール・コラボ提案・各エージェントなど)。
//
// 仕組み: 直叩き URL のベースを `GEMINI_API_BASE` (= 自サーバー内のこの中継) に
// 差し替える。中継はまず Google にそのまま転送し、402/429/403/5xx/通信失敗なら
// teai (OpenAI 互換) に同じ依頼を投げ、Gemini 形式の JSON に包み直して返す。
// 呼び出し側の解析コード (candidates[0].content.parts[..]) は無変更で動く。
//
// - 402 を見たら 30 分間は Google を飛ばして teai に直行する (無駄な往復を省く)。
// - 認可: query の key が GEMINI_API_KEY / GOOGLE_API_KEY と一致する時だけ受ける。
// - 止める: `MU_GEMINI_PROXY=0` で全箇所が従来どおり Google 直叩きに戻る。
// - embedContent (ベクトル化) は teai に代替がないので転送のみ。

use axum::{
    body::Bytes,
    extract::{Path, Query, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use base64::Engine;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// main が listen を始めたら true。それまでは Google 直を使う (自分への接続拒否を避ける)。
pub static LISTENING: AtomicBool = AtomicBool::new(false);
/// 中継が teai に切り替えた回数 (起動から)。
pub static FALLBACK_COUNT: AtomicU64 = AtomicU64::new(0);

const GOOGLE_BASE: &str = "https://generativelanguage.googleapis.com";
pub const PROXY_PATH: &str = "/__internal/gemini/v1beta/models/:op";

/// format! の中で `{GEMINI_API_BASE}` として使う。実行時に env を見て
/// 中継 (既定) か Google 直か を決める。
pub struct GeminiBase;
pub const GEMINI_API_BASE: GeminiBase = GeminiBase;

impl std::fmt::Display for GeminiBase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !LISTENING.load(Ordering::Relaxed)
            || std::env::var("MU_GEMINI_PROXY").map(|v| v == "0").unwrap_or(false)
        {
            return f.write_str(GOOGLE_BASE);
        }
        let port = std::env::var("PORT").unwrap_or_else(|_| "3000".into());
        write!(f, "http://127.0.0.1:{}/__internal/gemini", port)
    }
}

/// Google 直を飛ばす期限 (unix 秒)。402 → 30 分、429/5xx/通信失敗 → 5 分。
static DIRECT_SKIP_UNTIL: AtomicU64 = AtomicU64::new(0);
const SKIP_AFTER_402_SECS: u64 = 30 * 60;
const SKIP_AFTER_TRANSIENT_SECS: u64 = 5 * 60;
const MAX_BODY: usize = 26 * 1024 * 1024;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn direct_skipped() -> bool {
    now_secs() < DIRECT_SKIP_UNTIL.load(Ordering::Relaxed)
}

fn skip_direct_for(secs: u64) {
    DIRECT_SKIP_UNTIL.fetch_max(now_secs() + secs, Ordering::Relaxed);
}

/// 一定時間比較 (長さ違いは即 false で可: 長さは秘密ではない)。
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn key_ok(k: &str) -> bool {
    if k.is_empty() {
        return false;
    }
    ["GEMINI_API_KEY", "GOOGLE_API_KEY"]
        .iter()
        .filter_map(|n| std::env::var(n).ok())
        .any(|v| !v.is_empty() && ct_eq(v.as_bytes(), k.as_bytes()))
}

pub async fn proxy(
    Path(op): Path<String>,
    Query(q): Query<HashMap<String, String>>,
    req: Request,
) -> Response {
    // 鍵の確認を本文の読み込みより先に行う (鍵なしで大きな本文を読ませない)
    let key = q.get("key").cloned().unwrap_or_default();
    if !key_ok(&key) {
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }
    let body: Bytes = match axum::body::to_bytes(req.into_body(), MAX_BODY).await {
        Ok(b) => b,
        Err(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "too large").into_response(),
    };
    let (model, method) = op.split_once(':').unwrap_or((op.as_str(), ""));
    let can_fallback = method == "generateContent";

    // 1) Google 直 (402 の直後 30 分は飛ばす)
    let mut direct_err = String::from("skipped (recent 402)");
    if !(can_fallback && direct_skipped()) {
        let url = format!("{}/v1beta/models/{}?key={}", GOOGLE_BASE, op, key);
        let res = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| e.to_string());
        let res = match res {
            Ok(c) => c
                .post(&url)
                .header("content-type", "application/json")
                .body(body.clone())
                .send()
                .await
                .map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        match res {
            Ok(r) => {
                let st = r.status().as_u16();
                let txt = r.text().await.unwrap_or_default();
                let fallback_worthy = st == 402 || st == 403 || st == 429 || st >= 500;
                if !fallback_worthy || !can_fallback {
                    return passthrough(st, txt);
                }
                skip_direct_for(if st == 402 { SKIP_AFTER_402_SECS } else { SKIP_AFTER_TRANSIENT_SECS });
                direct_err = format!("google {}", st);
                // 失敗時の本文は呼び出し元に返す可能性があるので保持
                if let Ok(resp) = fallback(model, &body).await {
                    note_fallback(model, &direct_err);
                    return resp;
                }
                return passthrough(st, txt);
            }
            Err(e) => {
                if !can_fallback {
                    return (StatusCode::BAD_GATEWAY, format!("gemini proxy: {}", e)).into_response();
                }
                skip_direct_for(SKIP_AFTER_TRANSIENT_SECS);
                direct_err = format!("net {}", e);
            }
        }
    }

    // 2) teai
    match fallback(model, &body).await {
        Ok(resp) => {
            note_fallback(model, &direct_err);
            resp
        }
        Err(e) => {
            tracing::warn!("[gemini-proxy] {} fallback failed: {} / {}", model, direct_err, e);
            (StatusCode::BAD_GATEWAY, format!("gemini proxy: {} / teai: {}", direct_err, e))
                .into_response()
        }
    }
}

fn note_fallback(model: &str, why: &str) {
    let n = FALLBACK_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    tracing::warn!("[gemini-proxy] {} → teai fallback ok ({}) total={}", model, why, n);
}

fn passthrough(st: u16, txt: String) -> Response {
    let code = StatusCode::from_u16(st).unwrap_or(StatusCode::BAD_GATEWAY);
    (code, [("content-type", "application/json")], txt).into_response()
}

/// Gemini 形式の依頼を teai に投げ、Gemini 形式の応答に包み直す。
async fn fallback(model: &str, body: &Bytes) -> Result<Response, String> {
    let req: Value = serde_json::from_slice(body).map_err(|e| format!("bad json: {}", e))?;
    let gc = &req["generationConfig"];
    let wants_image = model.contains("image")
        || gc["responseModalities"]
            .as_array()
            .map(|a| a.iter().any(|m| m.as_str().map(|s| s.eq_ignore_ascii_case("IMAGE")).unwrap_or(false)))
            .unwrap_or(false);

    // テキストと参照画像を集める
    let mut system = String::new();
    for p in req["systemInstruction"]["parts"].as_array().into_iter().flatten()
        .chain(req["system_instruction"]["parts"].as_array().into_iter().flatten())
    {
        if let Some(t) = p["text"].as_str() {
            system.push_str(t);
            system.push('\n');
        }
    }
    let mut messages: Vec<Value> = Vec::new();
    let mut all_text = String::new();
    let mut refs: Vec<(String, String)> = Vec::new();
    for c in req["contents"].as_array().into_iter().flatten() {
        let role = if c["role"].as_str() == Some("model") { "assistant" } else { "user" };
        let mut parts: Vec<Value> = Vec::new();
        for p in c["parts"].as_array().into_iter().flatten() {
            if let Some(t) = p["text"].as_str() {
                all_text.push_str(t);
                all_text.push('\n');
                parts.push(json!({"type": "text", "text": t}));
            } else if let Some(d) = p.get("inlineData").or_else(|| p.get("inline_data")) {
                let mime = d.get("mimeType").or_else(|| d.get("mime_type"))
                    .and_then(|v| v.as_str()).unwrap_or("image/png").to_string();
                let data = d["data"].as_str().unwrap_or("").to_string();
                if !data.is_empty() {
                    parts.push(json!({"type": "image_url",
                        "image_url": {"url": format!("data:{};base64,{}", mime, data)}}));
                    refs.push((mime, data));
                }
            }
        }
        if !parts.is_empty() {
            messages.push(json!({"role": role, "content": parts}));
        }
    }
    if messages.is_empty() {
        return Err("no contents".into());
    }

    if wants_image {
        let prompt = format!("{}{}", system, all_text);
        let img = crate::gemini::generate_image_with_refs(&prompt, &refs).await?;
        // 呼び出し側は image/png 前提で保存するので PNG に揃える
        let png = to_png(&img.bytes)?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
        let out = json!({"candidates": [{"content": {"role": "model", "parts": [
            {"inlineData": {"mimeType": "image/png", "data": b64}}
        ]}, "finishReason": "STOP"}], "modelVersion": "teai-fallback/image"});
        return Ok((StatusCode::OK, axum::Json(out)).into_response());
    }

    let json_mode = gc["responseMimeType"].as_str() == Some("application/json");
    if json_mode {
        system.push_str("\nOutput ONLY valid JSON. No markdown code fences, no commentary.");
    }
    if !system.trim().is_empty() {
        messages.insert(0, json!({"role": "system", "content": system.trim()}));
    }
    let temperature = gc["temperature"].as_f64().unwrap_or(0.7);
    // 上流が思考に枠を使っても本文が残るよう下限を 2048 に
    let max_tokens = gc["maxOutputTokens"].as_u64().unwrap_or(4096).clamp(2048, 8192) as u32;
    // 元のモデル (2.5-pro 等) を先に試し、使えなければ flash
    let mut used = model.to_string();
    let mut res = crate::gemini::teai_chat_messages(messages.clone(), model, temperature, max_tokens).await;
    if res.is_err() && model != "gemini-2.5-flash" {
        tracing::warn!("[gemini-proxy] teai {} failed ({:?}), retry flash", model, res.as_ref().err());
        used = "gemini-2.5-flash".into();
        res = crate::gemini::teai_chat_messages(messages, "gemini-2.5-flash", temperature, max_tokens).await;
    }
    let mut text = res?;
    if json_mode {
        text = strip_fences(&text);
    }
    let out = json!({"candidates": [{"content": {"role": "model", "parts": [{"text": text}]},
        "finishReason": "STOP"}], "modelVersion": format!("teai-fallback/{}", used)});
    Ok((StatusCode::OK, axum::Json(out)).into_response())
}

fn to_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.starts_with(b"\x89PNG") {
        return Ok(bytes.to_vec());
    }
    let img = image::load_from_memory(bytes).map_err(|e| format!("decode: {}", e))?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| format!("png: {}", e))?;
    Ok(out.into_inner())
}

fn strip_fences(s: &str) -> String {
    let t = s.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t);
    t.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_json_fences() {
        assert_eq!(strip_fences("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_fences("{\"a\":1}"), "{\"a\":1}");
    }

    #[test]
    fn base_respects_kill_switch() {
        std::env::set_var("PORT", "3000");
        LISTENING.store(false, Ordering::Relaxed);
        assert_eq!(format!("{GEMINI_API_BASE}"), "https://generativelanguage.googleapis.com");
        LISTENING.store(true, Ordering::Relaxed);
        std::env::remove_var("MU_GEMINI_PROXY");
        assert_eq!(format!("{GEMINI_API_BASE}/v1beta"), "http://127.0.0.1:3000/__internal/gemini/v1beta");
        std::env::set_var("MU_GEMINI_PROXY", "0");
        assert_eq!(format!("{GEMINI_API_BASE}/v1beta"), "https://generativelanguage.googleapis.com/v1beta");
        std::env::remove_var("MU_GEMINI_PROXY");
    }

    #[test]
    fn rejects_unknown_key() {
        std::env::set_var("GEMINI_API_KEY", "k-test");
        assert!(key_ok("k-test"));
        assert!(!key_ok("other"));
        assert!(!key_ok(""));
        assert!(ct_eq(b"abc", b"abc") && !ct_eq(b"abc", b"abd") && !ct_eq(b"ab", b"abc"));
    }

    #[test]
    fn jpeg_becomes_png() {
        let img = image::RgbImage::new(2, 2);
        let mut jpg = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img).write_to(&mut jpg, image::ImageFormat::Jpeg).unwrap();
        let png = to_png(&jpg.into_inner()).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
    }
}
