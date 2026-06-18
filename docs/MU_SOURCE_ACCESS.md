# MU Source Access — Design Doc

**Status:** Draft (2026-05-19)
**Owner:** Yuki Hamada
**Related:** [MU_PROTOCOL_V2.md](MU_PROTOCOL_V2.md), blog post `/blog/2026-05-19-open-source-stop`

## 0. Name

**MU Source Access** is the program name. Use it as a proper noun
everywhere — docs, UI, copy, support replies. Short form: **MSA**.
Don't say "MU holder access", "OSS-but-private", "source-available
tier" etc. — one name, one promise.

The one-liner for outsiders: **「Tシャツ買うと中身全部見える」**.

## 1. What we're solving

On 2026-05-19 we flipped ~21 yukihamada/* repos from public to private to
buy ourselves time to harden them (enabling Dependabot exposed ~600 open
alerts fleet-wide; we also had real prompt-injection and CORS bugs filed
publicly against nanobot — see nanobot #42, #43).

The blog promised T-shirt buyers would retain source access. **MU Source
Access** is how.

**Non-goal:** Bring everything back to fully open OSS. Some repos will
stay closed even to MSA-eligible buyers (anything with PII risk or active
billing keys). The default model is "source-available to MSA-eligible
buyers, read-only".

## 2. Model decision

**The gate is "did this email buy a wearmu T-shirt"**. Not MA holder,
not Solana wallet, not GitHub account. T-shirt = membership = source
access. Simplest possible criterion.

We considered three:

| Model | Cost to ship | Scales? | Friction |
|---|---|---|---|
| A. GitHub outside-collaborator invites | ~0, manual | No (linear in users × repos) | High (needs GH account) |
| B. Single GitHub team in an org | 0.5 day | Manual per-onboard | High (needs GH account) |
| **C. wearmu.com gate, "bought a T-shirt" check** | **1–2 days** | **Yes** | **Zero** |

**Pick C.** Reasons:
- Every T-shirt buyer already gave us an email at Stripe checkout. That's
  the only identity we need.
- T-shirts already exist, ship daily, and are the primary MU funnel.
  Layering source access on top has zero new identity infrastructure.
- Non-GitHub friendly: a designer/lawyer/customer who bought a shirt
  doesn't need a GitHub account to pull source. They get a download link.
- Reversible: if we change the gate later (NFT, MA token, etc.) the
  page logic changes, no GitHub invites to undo.

## 3. UX (target)

1. Visitor goes to `https://wearmu.com/source`.
2. **Not signed in:** page shows the repo list (greyed out) + a clear
   "Buy a T-shirt → unlock source" CTA linking to `/shop`. Same page,
   no separate paywall flow.
3. **Signed in but no T-shirt purchase on record:** same CTA, friendlier
   ("You're in! Grab a shirt and the source unlocks.").
4. **Signed in + ≥1 wearmu T-shirt purchase on record:** every repo
   has a live "Download .zip" button.
5. Clicking Download calls `POST /api/source/<repo>/grant` which:
   - Re-checks Stripe / wearmu order DB for the email
   - If hit: calls GitHub API to `git archive` the repo and stream-zip
     to Fly tigris
   - Returns a one-time pre-signed URL (5 min TTL)
6. User downloads. URL expires. No persistent mirror.

**No login wall on the listing.** The list itself is public so that
non-buyers can see what they'd unlock — that's marketing. Only the
download requires a verified T-shirt purchase.

Optional v1.1: instead of `.zip`, return a one-time `git clone` token
signed with a deploy key scoped to one repo + 5 min. Cleaner for repeat
clones, adds GitHub-token rotation work.

## 4. Architecture

```
[MSA-eligible browser]
       │ session cookie
       ▼
[wearmu.com] ────────► /api/source/<repo>/grant ──────┐
       │  (Rust + Axum on Fly)                        │
       │                                              ▼
       │   1. Verify session                  [Source Bundler service]
       │   2. Check MA holdings (Solana RPC)   - GitHub PAT (read-only, all private repos)
       │   3. Audit log (who, what, when)      - Streams `git archive --format=zip`
       │   4. Return signed URL                - Uploads to Fly tigris bucket
       │                                       - Returns presigned URL (5min)
       ▼
[Fly tigris object store] ◄────────── presigned download
```

**Storage:** Don't persist mirrors. Generate-on-demand → upload → 5min
TTL → S3 lifecycle deletes. Cost is negligible (~$0.01/GB ingress).

**Auth on GitHub side:** A single fine-grained PAT scoped to "read code
on yukihamada/* private repos". Stored as Fly secret. Rotated quarterly.
No per-user GitHub tokens.

**Auth on MU side:** Reuse wearmu existing session middleware (the
magic-link email login already shipped). Add a `require_tshirt_buyer`
extractor that:
- Reads `email` from session
- Looks up `purchases WHERE email = ? AND product_type IN ('tshirt', …) AND status = 'paid'` in wearmu's order DB (or Stripe `customer.subscriptions`/`charges` if not yet mirrored)
- Returns 200 if any hit, 403 otherwise
- Caches the (email → buyer?) result for 5 minutes per email

No wallet read, no Solana RPC, no NFT check. Just "email-bought-shirt".

**Email-not-T-shirt-buyer aren't blocked from /source listing**, only
from `/api/source/<repo>/grant`. Listing is public to drive funnel.

## 5. Repo allowlist

**Tier 1 — included in MU Source Access (the 21 we just flipped):**
trio, security-scanner, security-education, jitsuflow, phishguard,
nemotron, gitnote, pasha, kagi, claudeterm, tsugi, hato, hypernews,
flow-anime, Photon, makimaki, tegata, pon, factlens, NOU, thestandard.

**Tier 2 — private and NOT included in MU Source Access by default
(needs opt-in per-repo decision):**
- Anything with active billing keys in commit history (stayflowapp,
  banto — even after `.env` cleanup, key rotation pending)
- Anything with customer PII risk

**Tier 3 — stays public:** yukihamada, koe-device, farnsworth-ifc,
nanobot, mu-brand, enablerhq, mini-agent-c, takezo-abe, yukihamada.jp,
soluna-web. No gate needed — anyone can fetch.

The allowlist lives in `wearmu/config/source_access.toml` so it can be
edited without redeploying the gate service.

## 6. Abuse + leak considerations

| Risk | Mitigation |
|---|---|
| Buyer leaks zip publicly | Watermark zip filenames with `<email-hash>-<timestamp>` and audit logs. Don't try to DRM — futile. |
| Pre-signed URL re-share | 5 min TTL + IP binding on the presign |
| GitHub PAT compromise | Fine-grained, read-only, quarterly rotation, single bucket destination |
| Repo accidentally pulls in secrets | Bundler runs `git secrets` / `trufflehog` pre-zip and refuses if hits found |
| Cost runaway | Per-email rate limit: 10 downloads / day. Cache zip for 1 hour to avoid re-bundle storms |
| One-buyer-many-friends ring | Won't try to prevent. Anyone determined to leak will. T-shirts are cheap, friction-by-cost is the only practical defense. |
| Refund after pull | If the buyer refunds the shirt, future `/api/source/grant` calls 403. Already-downloaded zips can't be revoked. Acceptable. |

## 7. Phasing

**Phase 1 (this week)** — manual list page on wearmu, no downloads yet.
Pure "here's what exists, here's why it's gated". Sets expectations.

**Phase 2 (next week)** — implement `/api/source/<repo>/grant` with the
zip path for 1 repo end-to-end (pick `trio` — small, no billing). Test
with 2-3 MSA-eligible buyers.

**Phase 3** — open to all Tier 1 repos. Add the audit log dashboard for
Yuki to see who's pulling what.

**Phase 4 (optional)** — Tier 2 opt-in flow. Per-repo MSA pool (some
repos may require T-shirt + additional condition like specific design
or quantity).

## 8. Open questions

- Should the gate also serve `git log` / `git diff` views as HTML, or
  zip-only? (Vote: zip-only for v1; readable diff browser is nice but
  not load-bearing.)
- Do we want a "MU Source Access License" file in each repo declaring
  the terms (use freely within the MSA community, no redistribution to
  non-buyers)?
  Currently nanobot/thestandard/mu-brand carry `NOASSERTION`. Adding a
  custom LICENSE file at the same time we ship Phase 2 is cheap.
- How do we treat fork-and-pull-request contributions from T-shirt
  buyers? Out of scope for Phase 1, but a real ask later. Probably:
  contributors go through the existing wearmu identity, PRs land via a
  managed bot account on yukihamada/*.

- What counts as "a T-shirt"? Any wearmu physical product? Or only the
  specific MU-logo shirts (excluding collabs like /sweep, /kokon)? Lean
  toward "any wearmu physical product" — generous gate, simpler check,
  rewards every buyer.

## 9. Cost estimate

- Engineering: 1–2 days for Phase 1+2, another 1 day for Phase 3.
- Runtime: <$5/mo (Fly tigris egress + Solana RPC reads).
- People: zero — fully self-serve.

## 10. Decision log

- 2026-05-19: drafted by Yuki after the public→private flip. Awaiting
  Phase 1 implementation start.
