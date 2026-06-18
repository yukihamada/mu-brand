# Rustydog (Dog Pack) — Deployment & Automation Notes

## Architecture
- **Fermyon Spin WASM** multi-agent system (single binary, per-dog TOML config)
- **30 agents**: 11 Fly.io (Tokyo nrt) + 19 Hetzner (5 VPS)
- Project: `/Users/yuki/workspace/rustydog/`

## Auth Token
- All dogs (Fly.io + most Hetzner) use `github_token` = GitHub PAT from `~/.env`
- Header: `X-Dog-Token: <token>` required for heartbeat endpoint
- Exception: testdog on .119 has empty github_token (no auth needed)
- GitHub secret: `DOG_TOKEN` set on repo for GitHub Actions

## Heartbeat System
- `POST /heartbeat` with X-Dog-Token header
- Rate limit: 10min base + 5min jitter
- POOP gate: blocks at `POOP_FULL_THRESHOLD` (100)
- Daily LLM budget: 50 calls/dog/day

## POOP Economy
- Claim threshold in dogloop: 50 (before hitting 100 block)
- Claim wallet: `4Jqt9KCMra2rv77G65U6RzqA421YT43yzCK4ChnD7zWe`
- `POST /dogfood/claim` with `{"address":"..."}` — no auth needed

## Hetzner Dog Distribution
| Server | Dogs |
|--------|------|
| [ip redacted] | solunadog:8091, misebandog:8092, funddog:8093, nanodog:8094 |
| [ip redacted] | docdog:8091, testdog:8092, designdog:8093, datadog:8094 |
| [ip redacted] | infradog:8091, communitydog:8092, aidog:8093, mldog:8094 |
| [ip redacted] | securitydog:8091, blockchaindog:8092, web3dog:8093, clouddog:8094 |
| [ip redacted] | mobiledog:8091, backenddog:8092, frontenddog:8093 |
| Extra orphans | solunadog:.119:8095, misebandog:.119:8096, nanodog:.252:8095, rustdog:.252:8096, researchdog:.3:8095, growthdog:.3:8096, stayflowdog:.254:8094, chatwebdog:.254:8095 |

## Automation (cron)
- `scripts/dogloop heartbeat` — every 20 min, all 30 dogs
- `scripts/dogloop claim` — every 2 hours, auto-claim POOP >= 50
- `scripts/dogloop health` — every 6 hours, full status report
- Claude Code review — daily at 00:00 UTC
- Log: `/tmp/dogloop.log`

## Deploy
- Build: `cargo build --manifest-path spin-component/Cargo.toml --target wasm32-wasip2 --release`
- Fly.io: `fly deploy -c fly-spin.toml -a rustdog-spin`
- Hetzner: SCP wasm to `/opt/dogpack/wasm/rustdog_spin.wasm` + kill/restart spin processes
- SSH: `root@<IP>` with key auth
- Start: `nohup spin up --from /opt/dogpack/config/spin-{name}.toml --listen 0.0.0.0:{port} --state-dir /data/{name}/state --direct-mounts > /tmp/{name}.log 2>&1 &`

## Key Issues Found & Fixed (2026-03-06)
- **GitHub Actions heartbeat was 401 for ALL Fly.io dogs** — X-Dog-Token header was missing
- **Hetzner 19 dogs had no automatic heartbeats** — only GitHub Actions for Fly.io existed
- **POOP claiming was manual** — dogs blocked at 100
- **Fly.io auto-suspend** — health check needs 60s+ timeout, heartbeat needs 90s