# MU Autonomy — Operator Notes

Last updated: 2026-05-12.

This file lists the **human-only actions** required for the autonomous-org
layer to operate at full capacity. Everything else is in code.

## Pending: X account auth

Status: `X OAuth: NOT linked` (verified via `/admin/queue`).

Effect: `x_post_worker` early-returns every 60s; `growth`, `drop`, `blog`,
`cultural` posts queue up but never reach @wearmu. Founder reviews keep
calling out "K-factor delusion" because of this.

Fix (option A, recommended — no browser, no token expiry):

```
# 1. https://developer.x.com/en/portal/dashboard
# 2. Open the existing app (Consumer Key starts 123395ca…)
# 3. "Keys and tokens" → "Access Token and Secret" → Generate
# 4. Copy the two values, then:
fly secrets set \
  X_ACCESS_TOKEN=<access_token> \
  X_ACCESS_TOKEN_SECRET=<access_token_secret> \
  -a mu-store
```

x_post_tweet's OAuth-1.0a path activates immediately on next deploy.
The age-filter (`13eb1b3`) prevents burst posting >7d-old queue rows;
recent posts (today's growth + drop) fire at one-per-60s.

Fix (option B): visit `/admin/x/auth?token=<MU_ADMIN_TOKEN>` in a browser,
authorize @wearmu. Writes `x_oauth_tokens` row id=1. Refresh token rotates
so this is less durable than option A.

## Pending: GitHub PAT for `agent_pr_writer`

Status: idle, "GITHUB_TOKEN missing".

Effect: `self_evolve` proposals (e.g. "forbid token 革命的") never become PRs.
The auto-merge harness (workflow `self-evolve-merge.yml`) exists but has
nothing to gate.

Fix:

```
# 1. https://github.com/settings/personal-access-tokens/new
# 2. Fine-grained PAT scoped to yukihamada/mu-brand:
#    - contents: write
#    - pull-requests: write
# 3. fly secrets set GITHUB_TOKEN=<pat> -a mu-store
```

After this, the daily pr_writer agent picks up the most recent
prompt-area self_evolve proposal and opens a labeled PR.

## Optional: Telegram chat moves

Default `TELEGRAM_CHAT_ID=1136442501` (yuki). To redirect digests:

```
fly secrets set TELEGRAM_CHAT_ID=<new_chat_id> -a mu-store
```

## Kill switches reminder

Set via Fly secrets (read at every tick — no redeploy needed):

| Var | Effect |
|---|---|
| `MU_AUTOPILOT=0` | Stops all autonomous crons (master) |
| `AGENT_KILL_ALL=1` | Stops every registered agent |
| `AGENT_KILL_<NAME>=1` | Stops just that agent (e.g. `AGENT_KILL_PRICE_MICRO=1`) |
| `DRY_RUN_ALL=1` | All spending agents log-only |
| `DRY_RUN_<NAME>=1` | Single agent log-only |
| `SELF_EVOLVE_AUTO_MERGE=0` | (GitHub repo secret, not Fly) — pauses auto-merge harness |

## Governance review cadence

`/admin/governance` lists pending T1 items. Click approve / reject. On
approve, `governance_dispatch` (in-process) sends a Telegram and, when
the kind is mechanically actionable, executes inline:

- `price_adjust` → updates collab_products.price_jpy
- everything else → notify only (yuki still does the work)

Auto-expire after 7 days idle.

## What the founder agents (Musk / Bezos) want, in priority

Read `/admin/founders`. Both scored 0.10 / 1.00 on 2026-05-12. Top three:

1. **Set the X creds above** — fixes K-factor critique
2. **Approve at least one strategist proposal** — kicks the drop loop
3. **Watch repeat_rate go up** — the customer_scorecard's only honest
   metric right now (it's 100% on n=1, so any new buyer changes it
   meaningfully)
