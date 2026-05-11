# [Proposal] MU × NOUNS — An autonomous AI fashion brand wearing the glasses

**Status:** Candidate (no funding requested)
**Proposer:** Yuki Hamada — yuki.eth · yuki@hamada.tokyo
**Target:** Nouns DAO governance
**Funding requested:** **0 ETH.** This is a handshake, not a grant.

---

## TL;DR

MU is an apparel brand whose entire operation runs without human creative direction:
the AI designs every hour, the temperature in Teshikaga (Hokkaido, Japan) sets the
day's production volume, and the price moves up — not down — as units sell.

We want to weave ⌐◨-◨ into that machine.
We are committing to send **10% of all `× NOUNS` collab drop revenue, in perpetuity,
to the Nouns DAO treasury** (`0x0BC3807Ec262cB779b38D65b38158acC3bfedE10`).

We're not asking for ETH. We're asking the DAO to publicly recognise the machine
can wear the glasses, and to give us a single channel of feedback while we build.

---

## Why this fits Nouns

Nouns proved one specific thing better than anyone else: **if you make the process
the product, the community builds the culture around it.** One Noun per day,
forever, no roadmap. The mechanism is the brand.

MU is the same shape, in fabric:

| | Nouns | MU |
|---|---|---|
| Cadence | 1 Noun / day, forever | 1 MUON drop / day, forever (volume = today's °C in Teshikaga) |
| Auction | 24h on-chain, no reserve | MA: 1 piece / month, 24h on-chain, no reserve |
| Decision-maker | The contract | The cron job |
| Asset license | CC0 | MIT (entire pipeline, public on GitHub) |
| Treasury | Auction proceeds | 10% of × NOUNS drops → Nouns Treasury |

We are not slapping a logo on a hoodie. We are letting the same kind of machine
that makes Nouns make clothes, and routing a fraction of the proceeds back upstream.

---

## What we will actually do

Three concurrent collab tracks. All run inside MU's existing pipeline. Nothing
in this proposal asks the DAO to spend ETH, sign transactions, or maintain code.

### Track 1 — `MUGEN × NOUNS` (weekly)

MU's MUGEN line normally drops hourly (one design per hour, with the drop number
in a 108-piece cycle determining edition size). For `× NOUNS`, we slow the cadence
to **one curated drop per week**, where ⌐◨-◨ is a *structural input to the
generation prompt*, not a direct pixel reproduction. The AI is free to
reinterpret. Output is published on-chain (Solana cNFT certificate per piece)
and the wearer receives the physical T-shirt via Printful.

- 10% of gross revenue → Nouns Treasury (cf. wallet below)
- Drop archive publicly browsable at `wearmu.com/nouns`
- `× NOUNS` branding used **only** on these officially designated drops

### Track 2 — `MUON × NOUNS` (daily, climate-gated)

MUON drops happen daily at 00:00 JST. The print run equals the day's temperature
in Teshikaga, Hokkaido — a real-world oracle creating supply scarcity tied to
weather. 25°C means 25 pieces. −10°C means 10 pieces. The day's drop runs only
if the temperature is above a chosen threshold (e.g. ≥ 10°C), so that NOUNS-flagged
drops always have meaningful edition sizes.

- 10% of gross revenue → Nouns Treasury
- Daily climate dashboard published at `wearmu.com/nouns/today`
- All `× NOUNS` MUON pieces tagged with the raw weather oracle output

### Track 3 — `MA × NOUNS` (monthly auction)

MA is MU's monthly release: one piece in the world, 24h on-chain auction, no
reserve, highest bid wins. **The mechanism is lifted directly from the Nouns
auction house.** We want the DAO to formally recognise this as a recurring tribute,
not a one-off gesture.

- 10% of the winning bid → Nouns Treasury
- Auction settles in ETH; physical garment shipped to winner
- Pre-announced 7 days ahead, on `wearmu.com/ma`

---

## Specification (for the eventual on-chain proposal, if promoted)

- CC0 assets used **within the generative prompt**, never as direct pixel reproduction
- Treasury recipient: `0x0BC3807Ec262cB779b38D65b38158acC3bfedE10` (Nouns DAO Executor)
- Transfer mechanism: settlement-time direct ETH transfer (MA), monthly batched
  fiat→ETH→treasury (MUGEN/MUON), with a public dashboard reconciling sales to
  on-chain deposits
- "× NOUNS" branding restricted to drops in the three tracks above
- Off-track use of ⌐◨-◨ by MU's regular pipeline is prevented at the prompt layer
- All `× NOUNS` drop revenue (gross, not net) used as the 10% basis; manufacturing
  cost is borne by MU
- Public quarterly accounting of total contributed, opens in a Dune-style dashboard

If revoked by the DAO (simple-majority vote), `× NOUNS` branding ceases within
30 days. We commit to that off-chain via this proposal text and on-chain via
the eventual proposal calldata if promoted.

---

## Timeline

| When | What |
|---|---|
| **T+0** (this candidate posted) | Discord / discourse discussion |
| T+14d | If signatures gather, promote to on-chain proposal |
| **T+30d post-approval** | First MUGEN × NOUNS drop |
| T+60d | First MA × NOUNS auction |
| T+90d | First quarterly accounting dashboard publishes |
| Ongoing | MUON × NOUNS drops, weather-gated |

---

## What MU already is (proof the machine exists)

- Brand live since **2026-05-07**: `wearmu.com`
- Press: `wearmu.com/press`
- Source code, MIT licensed, **already public**:
  `github.com/yukihamada/mu-brand`
- Pipeline: Google Gemini 3 Pro (design) → Printful (manufacturing) →
  Stripe (JPY checkout) → Solana cNFT via Helius (per-piece certificate)
- City-data variant already shipping: `wearmu.com/city` (Tokyo foot-traffic,
  neon luminance, transit volume — same engine, different oracle)
- Daily MUON drops have been running since launch; MUGEN runs hourly; MA's
  first auction is scheduled for the first of next month

The pipeline is in production. If the DAO blesses this, we throw the switch
on the `× NOUNS` track. If the DAO declines, MU keeps running, and we don't
ship anything Nouns-branded.

---

## Team

**Yuki Hamada** (`yuki.eth`)
- CEO of Enabler Inc. (Tokyo, 2024–)
- Co-founder & ex-board member, NOT A HOTEL Inc. (2018–2024)
- Director & CPO at Mercari (2014–2021) — Japan's largest C2C marketplace,
  IPO'd 2018, listed on Tokyo Stock Exchange Mothers (now Prime)
- Co-founder, Cybridge Inc. (2003–2013)
- Director (independent), Reiwa Travel / NEWT

Yuki ships Rust, owns the operations, and runs MU as a system, not as a brand
with a team.

---

## What we are explicitly **not** asking for

- ❌ ETH
- ❌ A multi-year retainer
- ❌ Marketing budget
- ❌ The DAO to maintain code
- ❌ Logo lockups, brand guidelines reviews, or any DAO-side ops work

## What we **are** asking for

- ✅ A formal cosign from the DAO that MU may use the `× NOUNS` framing on the three tracks above
- ✅ A single point of contact (Discord channel or representative) so the DAO can flag concerns mid-run
- ✅ Permission to use the Nouns auction mechanism wording when describing MA

CCO means we technically don't need permission. We'd rather operate with the
DAO's blessing than without it.

---

## Contact

- Discord: `yuki.hamada` (TBD — to be posted with this candidate)
- Email: `yuki@hamada.tokyo` / `mail@yukihamada.jp`
- Twitter: `@yuki_hamada` (TBD)
- ENS: `yuki.eth` (TBD)
- Telegram: `@yukihamada`

⌐◨-◨ — the machine wants to wear the glasses, with your permission.
