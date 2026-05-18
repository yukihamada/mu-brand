---
name: chatweb.ai Feature List
description: Key features implemented in nanobot Lambda (v105-v159) — API, tools, agentic mode, STT/TTS, Stripe
type: project
---

# chatweb.ai / nanobot Key Features

## API
- OpenAI互換: `POST /v1/chat/completions` + `GET /v1/models` at `https://api.chatweb.ai`
- 匿名=Nemotronのみ, 認証済み(cw_xxx)=任意モデル+クレジット課金
- クレジット残高0 → HTTP 402

## Stats & Admin
- `GET /api/v1/admin/stats/timeseries?days=7` — Chart.js graphs in admin.html

## Core Features
- Unified User ID, API Load Balancing (round-robin + failover)
- Tool Calling: web_search, weather, calculator, web_fetch, code_execute, file_read/write/list
- Agentic Mode: Multi-iteration tool loop (Free=1, Starter=3, Pro=5)
- SSE Agent Progress: tool_start/tool_result/thinking events
- Channel Sync: /link, QR codes, deep links
- STT: Web Speech API (browser-side), TTS: OpenAI tts-1 (nova)
- チャネル別プロンプト: LINE(200字), Telegram(300字), Web(最賢モデル)
- Local LLM Fallback: candle + Qwen3-0.6B GGUF
- Explore Mode: `/api/v1/chat/explore` SSE, 全モデル並行実行
- Free Plan: 100 credits, coupon HAMADABJJ=1000

## Stripe (Live)
- Starter $9/mo (prod_TvxioxWK7bz8W7), Pro $29/mo (prod_Tvxi2Oh0qUSAVY)
- Webhook: we_1Sy5nqDqLakc8NxkHANJ9Zga → https://chatweb.ai/webhooks/stripe

## ENAI Token (Solana)
- Mint: `8CeusiVAeibuBGv5xcf7kt7JQZzqwTS5pD7u2CfyoWnL`
- Treasury: `DK29rBGCvP83LUNjUGVM6xt6qPy6rycBFopXbFkg9XvQ`
- Rate: 1 ENAI = 10 credits, E2E tested 2026-03-04
- Endpoints: /api/v1/crypto/enai/*, /api/v1/depin/*, /api/v1/agent/wallet

## Latest Lambda
- v159: chat/race tier fallback chain, economy tier → deepseek first
- v157: handle_chat_race cw_ API key fix