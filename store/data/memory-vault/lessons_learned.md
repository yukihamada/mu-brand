---
name: Engineering Lessons Learned
description: Common pitfalls and fixes for nanobot/chatweb.ai, Lambda, DynamoDB, Rust, SSE streaming
type: feedback
originSessionId: 33ea8652-6f3b-48ba-bb20-6fc175a8224f
---
# Lessons Learned

- **DynamoDB in async context**: Use `std::thread::spawn` (not `std::thread::scope`) to escape Tokio runtime for `block_on`
- **OpenAI model prefix**: Strip `openai/` prefix before sending to API. Check `normalize_model()`
- **include_str!()**: HTML compiled into binary. Must rebuild after HTML changes. `cargo clean -p` may not suffice — use full target clean
- **CORS**: Same Lambda serves chatweb.ai and api.chatweb.ai — use relative URLs
- **Web search from Lambda**: Google/Bing/DDG block cloud IPs. Use **Jina Reader** (`https://r.jina.ai/{url}`)
- **tool_choice**: `"required"`/`{"type":"any"}` on first call, `"auto"` on follow-up, `None` on final
- **ChatResponse拡張**: 全ての`Json(ChatResponse {...})`を更新。エラー系は`None`、正常系は実値
- **candle GGUF**: `ModelWeights::from_gguf(content, &mut file, &device)` — Content must be read first
- **DynamoDB config table**: `nanobot-config` (not `nanobot-config-default`), keys: `pk`/`sk`
- **strip_prefix_ci panic**: Use `text.get(..prefix.len())` not `text[..prefix.len()]` for multibyte strings
- **SSE streaming**: `futures::channel::mpsc::unbounded` + `tokio::spawn` for real-time per-event delivery
- **Lambda AL2023**: No Python/Node.js. `code_execute` must use `language='shell'`
- **Multi-iteration tool loop**: Pass `Some(tools)` on follow-up, `None` only on final iteration
- **Axum 0.7.9 + matchit 0.7.x**: Path params use `:param` (colon), NOT `{param}` (curly braces)
- **Supabase RLS silent failure**: PATCH returns 204 even when blocked. Use `Prefer: return=representation`
- **Lambda musl vs gnu**: Must use `aarch64-unknown-linux-musl`. GNU = `Runtime.ExitError`
- **miseban-ai deploy**: Always `--remote-only --no-cache`
- **pricing.rs**: `p.model.to_lowercase() == lower` (mixed-case entries like Nemotron)
- **DynamoDB user records**: Lambda reads from `PROFILE` sk for credits/plan
- **Nemotron tool limitations**: web_search/weather/wikipedia = works. web_fetch/qr_code = often refuses
- **System prompt tokens**: ~25K input tokens per request (AGENT_COMMON + tools + context)
- **Lambda env vars DANGER**: `update-function-configuration --environment` REPLACES ALL vars. Use `--cli-input-json`
- **soluna-web catch-all順序**: `app.all("/api/*")` が全未マッチAPIをキャッチ。新規APIルートは必ずこの行より前に定義すること（後に書くと404になる）
- **gog drive download**: `gog drive get` はメタデータのみ返す。ファイル本体は `gog drive download <id>` で `~/Library/Application Support/gogcli/drive-downloads/` に保存される