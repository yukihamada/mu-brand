# MU Cold Ad Creatives — Toru Persona

**Date:** 2026-05-14
**Persona:** Toru (30, 港区/渋谷, AI/crypto startup or Tokyo design studio, salary ¥10–18M, Visvim/Y-3/Engineered Garments wardrobe, reads HN + Stratechery + ~200 X accounts)
**LP:** https://wearmu.com/buy
**Budget:** ¥50,000 total (test design: ¥1,500/day × 3 variants × 3 days = ¥13,500 test; ¥36,500 scale on winner)
**Platforms (priority):** X Ads (primary) > Threads > Meta (last resort)
**Success bar:** > 1.5% CTR, > 2% LP CVR, < ¥5,000 CAC

---

## ⚠ Honest priors from JiuFlow data

- JiuFlow: ¥42,000 / 3 days / **0 conversions** (cold ads → JP audience). Same Stripe stack.
- MU has 7 sales total in 6 days, 100% via DM/knows-yuki path.
- Cold paid for unknown JP brand at ¥50K → realistic expectation **0-3 sales** in first 24h.
- Whatever wins, the goal is LEARNING (CTR/CPC/CVR signal), not 10 sales.

---

## Variant A — "気温が決めた" (process authenticity)

**Hook:** Numbers, not narrative. Aligned with Toru's mental model #1 (authenticity through process).

**Image direction:**
- Black T-shirt photographed flat-lay against natural concrete.
- The MU ━◯━ mark visible top-right of chest.
- **Caption overlay (top-left, light gray, small):** `11℃ · partly cloudy · Teshikaga, 2026-05-13 14:00 JST · seed → MUGEN #154`
- No model. No lifestyle. Just the shirt and the metadata that made it.
- Aspect: 1:1 (X feed) + 9:16 (Stories)

**Copy (X, 240 chars):**
> 北海道弟子屈町の今日の気温が、 今日の T シャツを決めた。
>
> 11℃ partly cloudy → MUGEN #154。
> 同じデザインは二度と作られない。 ¥7,800。
>
> Stanley/Stella SATU001 (180gsm / GOTS organic / リブ襟 / EU 製)。
>
> wearmu.com/buy

**CTA button:** `今すぐ買う`
**Targeting:**
- X interests: Visvim, Engineered Garments, Comme des Garçons, Yohji, Margiela, Stratechery, Not Boring, Founders Fund
- Geo: Tokyo, Osaka, Kanagawa
- Age: 26–42
- Lookalike (1%): existing 7 buyers (Stripe export)

---

## Variant B — "1 of 108、 来週には買えない" (scarcity + permanence)

**Hook:** Same logic as buying a vintage Rolex (Toru #4 "permanence + finite").

**Image direction:**
- Stack of 108 black T-shirts photographed in a single column (architectural / Donald Judd reference).
- Sharp negative-space framing.
- **Overlay (small, white, kerned):** `MUGEN · 1/108 · drop expires at sold-out · never reprinted`
- Aspect: 1:1 + 9:16

**Copy (X, 220 chars):**
> 1 時間に 1 着、 108 枚作って終わり。
>
> 売り切れたら、 そのデザインは永久に再生産されない。
> Vintage Rolex と同じ論理で服を作っている。
>
> 今買える MUGEN #154 → ¥7,800
> Stanley/Stella SATU001 · EU 製 · リブ襟
>
> wearmu.com/buy

**CTA button:** `今すぐ買う`
**Targeting:**
- X interests: vintage watches, Audemars Piguet, Hodinkee, Rolex, archive Yohji, Visvim, Maison Margiela
- Geo: Tokyo + 首都圏
- Age: 30–48
- Income proxy: follows Stratechery, Generalist, premium SaaS accounts

---

## Variant C — "誰もいないブランド" (anti-influencer + tech-tribal)

**Hook:** "Made by AI alone, run by 1 person, designed by weather." (Toru #5 — tech tribal signal).

**Image direction:**
- Side-by-side: left photo = the T-shirt (clean, isolated). Right photo = a screenshot fragment of the Rust code that generates the design (literal `weather_data: { temp_c: 11 }` JSON).
- Mono dark background.
- **Overlay (small, gold, ━◯━):** `wearmu.com — designed by weather, made by AI, 0 humans in production`
- Aspect: 1:1 only (this is the meta concept variant)

**Copy (X, 260 chars):**
> 28 個の AI エージェントと、 1 人の maintainer。
> 商品企画も、 デザイン生成も、 在庫管理も、 全部 API。
>
> 北海道弟子屈町の気温が seed の T シャツを、 100 年運営する実験。
> コードは CC0/MIT (github.com/yukihamada/mu-brand)。
>
> 試しに 1 着: ¥7,800
> wearmu.com/buy

**CTA button:** `今すぐ買う`
**Targeting:**
- X interests: OSS, Rust, indie hackers, Stratechery, a16z, GenAI, Anthropic, Karpathy followers
- Geo: Tokyo + Osaka + Honolulu (Hamada-san already there)
- Age: 24–40
- Lookalike: yuki's X follower list (re-engagement-friendly)

---

## Recommended test plan

### Day 1 — discovery (¥4,500 / ¥1,500 per variant)
- Run A, B, C in parallel on X Ads
- Audiences: small (~10K reach each) for variance signal
- Watch metric: CTR (gating signal)
- Kill any variant with CTR < 0.8% by hour 8

### Day 2 — refinement (¥4,500)
- Continue surviving variants
- Add 9:16 Stories variant of the winner
- Watch LP CVR via funnel_events (`?utm_source=x_ads&utm_campaign=cold_toru&utm_content=variant_a/b/c`)

### Day 3 — scale gate (¥4,500)
- If best variant CPC < ¥400 AND LP CVR > 1.5% → continue
- Otherwise → kill paid, return remaining ¥36,500 to DM/seeding strategy

### Anti-scale signals (stop immediately)
- LP CVR < 0.5% after 100 visits → LP is broken, fix before more spend
- Frequency > 2.5 → audience too small or burnt
- CPC > ¥800 → audience too premium for our offer

---

## UTM tracking pattern (must apply to every ad URL)

```
https://wearmu.com/buy?utm_source=x&utm_medium=cpc&utm_campaign=cold_toru_2026_05&utm_content=variant_a_process
https://wearmu.com/buy?utm_source=x&utm_medium=cpc&utm_campaign=cold_toru_2026_05&utm_content=variant_b_scarcity
https://wearmu.com/buy?utm_source=x&utm_medium=cpc&utm_campaign=cold_toru_2026_05&utm_content=variant_c_meta
```

mu-funnel.js auto-captures URL params via referrer. Filter funnel_events
WHERE path = '/buy' AND extra LIKE '%utm_content=variant_a%' for per-variant CVR.

---

## Manual deploy steps for yuki (Claude can't place ads)

1. **Stripe export:** Download last 90 days of paying customer emails from
   https://dashboard.stripe.com/customers — upload as custom audience to X
   Ads → create 1% lookalike for each variant
2. **X Ads Manager:** Create campaign "MU cold Toru 2026-05" → set ¥1,500/day per ad group → upload variant A/B/C images + copy from this file
3. **Tracking:** Apply UTM URLs from above
4. **Monitor:** check https://wearmu.com/admin/funnel/top_paths?token=…&days=1 every 4h for first day; full LP CVR query via SSH if needed

---

*Constitution §11 (numbers over adjectives). Honest priors above were not removed — every option carries ~70% chance of underperforming based on JiuFlow precedent.*
