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
    extract::{Path, Query},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use base64::Engine;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

const GOOGLE_BASE: &str = "https://generativelanguage.googleapis.com";
pub const PROXY_PATH: &str = "/__internal/gemini/v1beta/models/:op";

/// format! の中で `{GEMINI_API_BASE}` として使う。実行時に env を見て
/// 中継 (既定) か Google 直か を決める。
pub struct GeminiBase;
pub const GEMINI_API_BASE: GeminiBase = GeminiBase;

impl std::fmt::Display for GeminiBase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if std::env::var("MU_GEMINI_PROXY").map(|v| v == "0").unwrap_or(false) {
            return f.write_str(GOOGLE_BASE);
        }
        let port = std::env::var("PORT").unwrap_or_else(|_| "3000".into());
        write!(f, "http://127.0.0.1:{}/__internal/gemini", port)
    }
}

/// Google が 402 を返した時刻 (unix 秒)。0 = 未発生。
static DIRECT_BLOCKED_AT: AtomicU64 = AtomicU64::new(0);
const SKIP_DIRECT_SECS: u64 = 30 * 60;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn direct_skipped() -> bool {
    let t = DIRECT_BLOCKED_AT.load(Ordering::Relaxed);
    t != 0 && now_secs().saturating_sub(t) < SKIP_DIRECT_SECS
}

fn key_ok(k: &str) -> bool {
    if k.is_empty() {
        return false;
    }
    ["GEMINI_API_KEY", "GOOGLE_API_KEY"]
        .iter()
        .filter_map(|n| std::env::var(n).ok())
        .any(|v| !v.is_empty() && v == k)
}

pub async fn proxy(
    Path(op): Path<String>,
    Query(q): Query<HashMap<String, String>>,
    body: Bytes,
) -> Response {
    let key = q.get("key").cloned().unwrap_or_default();
    if !key_ok(&key) {
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }
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
                if st == 402 {
                    DIRECT_BLOCKED_AT.store(now_secs(), Ordering::Relaxed);
                }
                direct_err = format!("google {}", st);
                // 失敗時の本文は呼び出し元に返す可能性があるので保持
                if let Ok(resp) = fallback(model, &body).await {
                    tracing::info!("[gemini-proxy] {} → teai fallback ok ({})", model, direct_err);
                    return resp;
                }
                return passthrough(st, txt);
            }
            Err(e) => {
                if !can_fallback {
                    return (StatusCode::BAD_GATEWAY, format!("gemini proxy: {}", e)).into_response();
                }
                direct_err = format!("net {}", e);
            }
        }
    }

    // 2) teai
    match fallback(model, &body).await {
        Ok(resp) => {
            tracing::info!("[gemini-proxy] {} → teai fallback ok ({})", model, direct_err);
            resp
        }
        Err(e) => {
            tracing::warn!("[gemini-proxy] {} fallback failed: {} / {}", model, direct_err, e);
            (StatusCode::BAD_GATEWAY, format!("gemini proxy: {} / teai: {}", direct_err, e))
                .into_response()
        }
    }
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
        let b64 = base64::engine::general_purpose::STANDARD.encode(&img.bytes);
        let out = json!({"candidates": [{"content": {"role": "model", "parts": [
            {"inlineData": {"mimeType": img.mime, "data": b64}}
        ]}, "finishReason": "STOP"}]});
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
    let max_tokens = gc["maxOutputTokens"].as_u64().unwrap_or(4096).clamp(256, 8192);
    let mut text = crate::gemini::teai_chat_messages(
        messages, "gemini-2.5-flash", temperature, max_tokens as u32).await?;
    if json_mode {
        text = strip_fences(&text);
    }
    let out = json!({"candidates": [{"content": {"role": "model", "parts": [{"text": text}]},
        "finishReason": "STOP"}]});
    Ok((StatusCode::OK, axum::Json(out)).into_response())
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
    }
}
