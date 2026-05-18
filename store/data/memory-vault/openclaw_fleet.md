---
name: OpenClaw AI Agent Fleet
description: 4 autonomous AI agents on Hetzner VPS running OpenClaw framework, connected via Telegram
type: project
---

# OpenClaw AI Agent Fleet (2026-03-14)

4 autonomous AI agents on Hetzner cax11 ARM servers (4GB RAM, EUR 3.99/mo each), all running OpenClaw framework with OpenAI Codex model.

## Agents

| Name | Role | IP | Telegram Bot | Bot Token |
|------|------|-----|-------------|-----------|
| Hachi 🐝 | 総合アシスタント | [ip redacted] | @yukihamada_ai_bot | [tg token redacted] |
| Kuro ⚡ | 技術特化エンジニア | [ip redacted] | @yukihamada_Codex_Openclaw_bot | [tg token redacted] |
| Ichi 1️⃣ | enablerdao.com/yukihamada.jp専任 | [ip redacted] | @Enabler_Bossdog_bot | [tg token redacted] |
| Ni 2️⃣ | インフラ・セキュリティ特化 | [ip redacted] | @yukihamada_codexclaw_bot | [tg token redacted] |

## Common Config
- **Model**: OpenAI Codex via OAuth (auth-profiles.json from Kuro, account: [email redacted])
- **Memory**: `{"backend": "builtin"}` — file-based (`memory/YYYY-MM-DD.md` + `MEMORY.md`)
- **Browser**: Chromium headless enabled on all
- **Heartbeat**: 30min intervals
- **dmPolicy**: "open" with `allowFrom: ["*"]`
- **SSH**: `ssh root@<IP>` (key-based auth)
- **Service**: `systemctl restart openclaw` (Hachi/Ichi/Ni), `systemctl restart openclaw-openai` (Kuro)
- **Config path**: `~/.openclaw/openclaw.json` (JSON5 strict schema)
- **Workspace**: `~/.openclaw/workspace/` (SOUL.md, USER.md, AGENTS.md, HEARTBEAT.md, MEMORY.md)

## Key Pitfalls
- `agents.defaults.thinking` is NOT a valid config key — causes startup crash loop
- Memory backend must be `"builtin"` or `"qmd"` — NOT `{"enabled": true, "backend": "lancedb"}`
- OpenRouter credits fully consumed ($3,400/$3,400) — agents use OpenAI Codex instead
- Hetzner has 5-server limit; all 5 slots used (4 OpenClaw + soluna-relay)
- Telegram bots cannot initiate DM — user must /start first
- `openclaw doctor --fix` to repair broken configs

## Tasks Given to Agents
1. Research enablerdao.com, yukihamada.jp, enabler.fun
2. Publish findings as blog posts to enablerdao.com (static/blog_seed.json → WASM rebuild)
3. Push to GitHub (yukihamada org) and deploy
4. Discuss as a team what they can do (10+ ideas)
5. Self-awareness development about roles and capabilities