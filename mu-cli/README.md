# mu-cli

Terminal client for **[wearmu.com](https://wearmu.com)** — make MU (無) products
from one command. AI-generate (or bring) a design, open a store, create a
product, and optionally ship a physical sample. On-demand print, zero inventory.

> **Three ways to build on MU.** Pick the one that fits you:
> - **AI agents** → use the MCP server `https://mcp.wearmu.com/mcp` (this CLI is *not* for agents).
> - **Any language** → generate a client from [`/openapi.json`](https://wearmu.com/openapi.json).
> - **Humans / scripts** → this CLI (`mu`, `mu-batch`) over `MuClient`.
>
> Full human guide: **https://wearmu.com/build**

---

## Install

```bash
pip install -e mu-cli          # from this repo
# (once published)  pip install mu
```

Exposes two console commands: **`mu`** (one-shot) and **`mu-batch`** (parallel).

## Configure

Credentials/address resolve in this order — env vars win, then a secrets file
(`$MU_SECRETS`, else `./.secrets.local`, else `~/.mu/secrets`), then
`~/.claude.json` (agent key) and `fly ssh` (operator creds):

```ini
# ~/.mu/secrets   (or ./.secrets.local — never commit)
MU_AGENT_KEY=...           # your agent api_key (see "Get a key" below)
MU_ADMIN_TOKEN=...         # optional — only for approve/grant (operator)
PRINTFUL_API_KEY=...       # optional — only for --ship
# ship-to (only for --ship). Printful state_code: Tokyo=13
SHIP_NAME=Your Name
SHIP_ADDR1=2-3-4 Example, #101
SHIP_CITY=Minato-ku
SHIP_STATE=13
SHIP_ZIP=108-0000
SHIP_COUNTRY=JP
```

`GEMINI_API_KEY` (or `GOOGLE_API_KEY`) is needed only for local generation
(`mu-batch`, `--transparent`). `mu --ai` generates server-side and needs none.

### Get a key

```bash
curl -X POST https://wearmu.com/api/agent/register -d '{"email":"you@example.com"}'
# → 6-digit code by email
curl -X POST https://wearmu.com/api/agent/register/verify \
  -d '{"email":"you@example.com","code":"123456"}'   # → api_key (+200pt welcome)
```

---

## `mu` — one product

```bash
# AI design (spends mu_credits, hosted on R2)
mu --ai "a minimal black sumi-e crescent moon on pure white" --kind tee

# bring your own artwork (free)
mu --design-url https://example.com/art.png --kind hoodie

# create + approve + ship a sample to your SHIP_* address
mu --ai "the kanji 無 in bold sumi-e" --kind tee --size L --ship
mu ... --ship --draft        # validate address + cost, don't charge
```

Flags: `--kind` (tee·hoodie·crewneck·rashguard_ls·rashguard_black) ·
`--size` · `--store` (default `mu-lab`) · `--label` · `--price` · `--ship` · `--draft`.

## `mu-batch` — many products, in parallel

```bash
mu-batch briefs.json                 # generate all designs concurrently, create + approve
mu-batch briefs.json --transparent   # white-ink on transparent → floats on black tees (no white panel)
mu-batch briefs.json --gen-only      # just generate + host (no DB writes) — timing/preview
mu-batch briefs.json --workers 8
```

```jsonc
// briefs.json
[
  {"kind": "tee", "label": "月", "description": "...", "prompt": "a single black sumi-e crescent moon on pure white"},
  {"kind": "hoodie", "label": "無", "description": "...", "prompt": "bold brush calligraphy of 無 on pure white"}
]
```

`N` designs take ~the time of the slowest one, not the sum.

## Programmatic

```python
from mu_cli import MuClient
mu = MuClient()
mu.me()                                              # balance, stores, limits
r = mu.create_product("mu-lab", "月", "...", "tee",
                      ai_prompt="a minimal crescent moon")
mu.approve(r["sku"])                                 # operator (ADMIN_TOKEN)
png = mu.gen_design("...");  t = mu.to_transparent(png);  url = mu.host_image(t)
mu.ship_sample("tee", "L", url)                      # Printful sample → SHIP_*
```

---

## Lifecycle & rules

- Products are created **`status: review`** and only go **live** after an
  **MA-council** member approves them (`mu.approve(sku)` with `MU_ADMIN_TOKEN`).
- **Design source:** `--design-url` (free) **or** `--ai` (spends `mu_credits`,
  ~¥50/gen; refunded if generation fails). Check cost/balance with `mu.me()`.
- **Price floors** (auto-clamped up): tee ¥4,900 · crewneck ¥7,800 ·
  hoodie ¥8,800 · rashguard ¥9,800.
- **Rate limit:** 20 products / hour per account.
- Only `https` image URLs; you can only write to stores you own.

## Links

- Guide: https://wearmu.com/build
- OpenAPI: https://wearmu.com/openapi.json · MCP: https://mcp.wearmu.com
- Shop: https://wearmu.com/shop

© 株式会社イネブラ / Enabler Inc.
