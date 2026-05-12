// Gemini 3 Pro Image — MU × YOU collab tee design generator.
//
// Each subscriber's daily design (mood + palette + scene + day seed) becomes
// a photorealistic T-shirt mockup image. Cached as bytes in SQLite, served
// via /api/you/design/:id/image.png.

use base64::Engine;
use serde_json::json;

const MODEL: &str = "gemini-3-pro-image-preview";

pub struct GeneratedImage {
    pub bytes: Vec<u8>,
    pub mime: String,
}

pub struct TeeDesign<'a> {
    pub name: &'a str,
    pub prompt: &'a str,
    pub mood: &'a [String],
    pub palette: &'a [String],
    pub scene: &'a [String],
    pub seed: &'a str,
    /// Free-text "one line about you" supplied by the wearer. Goes into the
    /// design brief so the artwork interprets the wearer's self-description,
    /// but is NEVER printed verbatim on the shirt.
    pub bio: &'a str,
    /// MU Next Thesis (A): "wearable timestamp" overlay. A single small line
    /// of machine-tone text printed near the hem (NOT in the chest area).
    /// Example: "2026-05-12 · Teshikaga · 14.2°C". Optional — pass empty
    /// string to disable (legacy behavior: no text on shirt at all).
    pub wear_log_overlay: &'a str,
}

pub async fn generate_tee(p: &TeeDesign<'_>) -> Result<GeneratedImage, String> {
    let key = std::env::var("GEMINI_API_KEY")
        .or_else(|_| std::env::var("GOOGLE_API_KEY"))
        .map_err(|_| "GEMINI_API_KEY not set".to_string())?;

    let prompt = build_tee_prompt(p);
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        MODEL, key
    );
    let body = json!({
        "contents": [{"parts": [{"text": prompt}]}],
        "generationConfig": {"responseModalities": ["IMAGE", "TEXT"]}
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("client: {}", e))?;
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("send: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("gemini {}: {}", status, &txt[..txt.len().min(400)]));
    }
    let json: serde_json::Value = resp.json().await.map_err(|e| format!("parse: {}", e))?;
    let parts = json["candidates"][0]["content"]["parts"]
        .as_array()
        .ok_or_else(|| {
            let pf = json["promptFeedback"].clone();
            format!("no parts (promptFeedback={})", pf)
        })?
        .clone();
    for part in parts {
        for k in &["inline_data", "inlineData"] {
            if let Some(d_obj) = part.get(*k) {
                if let Some(b64) = d_obj.get("data").and_then(|v| v.as_str()) {
                    let mime = d_obj
                        .get("mimeType")
                        .or_else(|| d_obj.get("mime_type"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("image/png")
                        .to_string();
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(b64)
                        .map_err(|e| format!("b64 decode: {}", e))?;
                    return Ok(GeneratedImage { bytes, mime });
                }
            }
        }
    }
    Err("no image data in gemini response".into())
}

fn build_tee_prompt(p: &TeeDesign) -> String {
    let mood = if p.mood.is_empty() { "minimal, quiet".to_string() } else { p.mood.join(", ") };
    let palette = if p.palette.is_empty() { "muted earth tones".to_string() } else { p.palette.join(", ") };
    let scene = if p.scene.is_empty() { "every-day".to_string() } else { p.scene.join(", ") };
    let bio_clause = if p.bio.trim().is_empty() {
        String::new()
    } else {
        format!(
            "\nWearer self-description (interpret as personality, do NOT print on shirt): \"{}\"",
            p.bio.replace('"', "'").chars().take(240).collect::<String>(),
        )
    };

    // MU Next Thesis (A) — "wearable timestamp" overlay. A small, machine-tone
    // text line near the hem. When empty, falls back to the original
    // "no text at all" rule.
    let (overlay_brief, text_rule) = if p.wear_log_overlay.trim().is_empty() {
        (String::new(),
         "- NO text on the T-shirt itself. NO watermark. NO model. NO mannequin. NO hangers.")
    } else {
        let safe = p.wear_log_overlay.replace('"', "'").chars().take(80).collect::<String>();
        (format!("\nWearable timestamp overlay (single small line of text, near hem): \"{}\"", safe),
         "- The chest area must NOT contain any text. The ONLY text on the shirt is the small \
          single-line wearable-timestamp overlay near the hem (left-aligned, ~10pt equivalent, \
          neutral sans-serif, in the same ink colour as the chest graphic, machine-tone). \
          NO watermark. NO model. NO mannequin. NO hangers.")
    };

    format!(
        "Photorealistic editorial product photograph of a single high-quality cream / off-white \
         heavyweight cotton T-shirt laid flat on a soft warm-grey concrete or paper surface, top-down 4:3 view. \
         The T-shirt features a printed graphic design centered on the chest:\n\n\
         === DESIGN BRIEF (do not write any of this text on the shirt unless explicitly noted) ===\n\
         Concept name (poetic, Japanese): \"{name}\"\n\
         Description: {prompt}\n\
         Mood keywords: {mood}\n\
         Palette: {palette}\n\
         When it is worn: {scene}{bio_clause}{overlay_brief}\n\
         Deterministic seed (variation key): {seed}\n\n\
         === RENDERING RULES ===\n\
         - The chest graphic should be an artistic, abstract / minimal illustration that interprets \
           the concept above. NOT literal, NOT a logo, NOT a word mark. Think Aesop / Kinfolk \
           editorial, slightly hand-drawn, slightly imperfect.\n\
         - The graphic should occupy roughly the chest area, ~15% of the shirt's width.\n\
         {text_rule}\n\
         - Subtle natural shadow under the shirt, gentle directional sunlight from upper-left.\n\
         - High-fidelity fabric texture (visible weave at close range).\n\
         - Backdrop should be calm, slightly desaturated, ~80% of frame is the shirt.\n\
         - 4:3 aspect ratio.",
        name = p.name,
        prompt = p.prompt,
        mood = mood,
        palette = palette,
        scene = scene,
        seed = p.seed,
        bio_clause = bio_clause,
        overlay_brief = overlay_brief,
        text_rule = text_rule,
    )
}
