# mu-commons

Public, MIT-licensed spec + reusable widgets extracted from [`mu-brand`](https://github.com/yukihamada/mu-brand) (wearmu.com).

This repo is **§29 C-1 (Source as Donation)** — the parts of MU's stack that other autonomous brand operators can fork freely. The proprietary operations (Stripe webhook, Printful order chain, AI generation prompts) stay in mu-brand. This repo is the **discipline of silence** as code.

## What's here

| Path | What | License |
|---|---|---|
| `protocol/MU_PROTOCOL_V2.md` | Universal autonomous brand protocol (RFC v2.0) | MIT |
| `protocol/protocol.html` | Polished public spec page (JA) | MIT |
| `protocol/protocol.en.html` | English version | MIT |
| `constitution/profit_split.md` | §28 + §29 (Discipline of Silence, progressive donation) | MIT |
| `constitution/MU_FOUNDATION_DIGITAL.md` | Setup guide: no hanko, no paper foundation incorporation in Japan | MIT |
| `flow/needs_now.json` | Schema example: where money flows when the customer says "most needed now" | MIT |
| `widget/pt_gate.js` | Drop-in 30pt paywall widget (`<script src=pt_gate.js>` + `<div data-pt-gate>`) | MIT |
| `schemas/` | JSON Schemas for `mu.release.v2`, `mu.node.v2`, `mu.needs_now.v1`, `mu.today_is_yours.v1` | MIT |

## What's NOT here

By design, this repo excludes:

- API keys, secrets, webhook endpoints
- Stripe / Printful / Gelato / SUZURI integration code (operator-specific)
- AI image generation prompts (proprietary)
- Customer PII handling (per §29 A-3, never in public emits)
- Internal admin routes

If you want to see the full reference implementation, [`mu-brand`](https://github.com/yukihamada/mu-brand) is also MIT, but it carries the brand's specific operational choices. This repo is the **shape** without the **paint**.

## Quickstart

Stand up your own MU-Protocol-compliant node:

1. Read `protocol/MU_PROTOCOL_V2.md` (15 min)
2. Stand up `/.well-known/mu/releases` on your domain that emits Release JSON per `schemas/release.schema.json`
3. (Optional) Drop `widget/pt_gate.js` into any page to add a "30pt to read more" paywall
4. (Optional) Implement progressive donation per `constitution/profit_split.md` §29

## §29 alignment

This repo embodies §29 C-1: **Source as Donation**. Code is value. Forking it is value flowing to you. Use it freely; no attribution required by license (though appreciated).

If you build something with this, you are **not** required to:
- Use the "MU" name (use your own)
- Pay the 5% origin fee (that's only for nodes using the MU mark)
- Tell us about it

You **are** invited to:
- Email a one-liner about what you built to `mail@yukihamada.jp`. Silent network.

## License

MIT — see [LICENSE](./LICENSE).

— Enabler Inc. · 2026-05-18
