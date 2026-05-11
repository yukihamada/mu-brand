# discourse.nouns.wtf — pre-proposal thread

カテゴリ: **NounsDAO Proposals → Pre-Proposal Discussion** または **Ideas**

タイトル:
```
MU × NOUNS — autonomous AI fashion brand, 10% to treasury, zero ETH requested
```

---

本文（マークダウン、discourse 用に少し短く、図表は省略）：

Hi all 👋

I'm **Yuki Hamada** (ex-CPO Mercari, now CEO at Enabler Inc.). I run **MU**
(`wearmu.com`) — an apparel brand whose entire operation is autonomous.

Posting here ahead of submitting a candidate, because I'd rather hear "this
doesn't fit Nouns" *before* I push the button, not after.

## The shape

| | Nouns | MU |
|---|---|---|
| Cadence | 1 Noun/day, forever | 1 MUON drop/day, forever (qty = today's °C in Teshikaga, Hokkaido) |
| Auction | 24h on-chain, no reserve | MA: 1 piece/month, 24h on-chain, no reserve |
| Decision-maker | The contract | The cron job |
| Asset license | CC0 | MIT (whole pipeline, public) |

## The ask

Recognition + a single channel of feedback while I build, on three concurrent
`× NOUNS` tracks:

1. **MUGEN × NOUNS** — weekly curated drop, ⌐◨-◨ as prompt input
2. **MUON × NOUNS** — daily climate-gated drop, volume = day's temperature
3. **MA × NOUNS** — monthly auction (mechanism lifted directly from Nouns)

10% of gross revenue from all × NOUNS drops → Nouns Treasury
(`0x0BC3807Ec262cB779b38D65b38158acC3bfedE10`), in perpetuity. Public
quarterly dashboard reconciling sales → on-chain deposits.

**Funding requested: 0 ETH.**

## Why not just do it (CC0)?

Because I'd rather operate with the DAO's blessing than without it. If the
DAO declines, MU keeps running, and we don't ship anything Nouns-branded. If
revoked later (simple majority), branding ceases within 30 days.

## Proof the machine exists

- Brand live since **2026-05-07**
- Source MIT, public: `github.com/yukihamada/mu-brand`
- Pipeline: Gemini → Printful → Stripe → Solana cNFT
- City-data variant already shipping (Tokyo foot-traffic oracle): `wearmu.com/city`

## Full candidate proposal

(Posted as draft, to be filed at `nouns.camp` after this discussion):
{LINK TO CANDIDATE / GIST}

## Asking specifically

- Is the three-track structure (weekly / daily / monthly) overkill, or
  right-sized for "blessing"?
- Should the 10% basis be **gross** or **net of manufacturing**? I lean
  gross — but want to hear concerns about fairness during downside months.
- Is "10% in perpetuity" the right commitment, or would the DAO prefer a
  sunset/renewal clause (e.g. 24 months, then re-vote)?
- Any concerns about the on-chain settlement design (MA settles in ETH,
  MUGEN/MUON settle in JPY then batched to ETH monthly)?

Roast freely. Better to be told now than after the candidate is signed.

⌐◨-◨

— yuki
`yuki@hamada.tokyo` · ENS `yuki.eth` · Telegram `@yukihamada`
