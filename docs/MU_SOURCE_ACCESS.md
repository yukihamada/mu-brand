# MU Source Access — Design Doc

**Status:** Draft (2026-05-19)
**Owner:** Yuki Hamada
**Related:** [MU_PROTOCOL_V2.md](MU_PROTOCOL_V2.md), blog post `/blog/2026-05-19-open-source-stop`

## 1. What we're solving

On 2026-05-19 we flipped ~21 yukihamada/* repos from public to private to
buy ourselves time to harden them (enabling Dependabot exposed ~600 open
alerts fleet-wide; we also had real prompt-injection and CORS bugs filed
publicly against nanobot — see nanobot #42, #43).

The blog promised MU holders would retain source access. This doc
specifies how.

**Non-goal:** Bring everything back to fully open OSS. Some repos will
stay closed even to MU holders (anything with PII risk or active billing
keys). The default model is "source-available to verified MU holders,
read-only".

## 2. Model decision

We considered three:

| Model | Cost to ship | Scales? | MU-native? |
|---|---|---|---|
| A. GitHub outside-collaborator invites | ~0, manual | No (linear in users × repos) | No |
| B. Single GitHub team in an org | 0.5 day | Yes per-user, manual per-onboard | No |
| **C. wearmu.com gated zip download** | **1–2 days** | **Yes** | **Yes** |

**Pick C.** Reasons:
- A and B require every MU holder to have a GitHub account and to give it
  to us. Cold-friction; MU is not a developer product.
- C reuses the existing MU holder verification (wearmu.com session, MA
  ownership check, founder-relay graph). Zero new identity surface.
- C decouples "source access" from GitHub permissions, so we can change
  the storage backend later without re-inviting users.

## 3. UX (target)

1. MU holder visits `https://wearmu.com/source`.
2. wearmu checks the active session against the MA-holder set (same gate
   as the existing dashboard). If not a holder: 403 + buy-MA CTA.
3. Page lists every private yukihamada/* repo with: name, one-line
   description, last-updated, language, "Download .zip" button.
4. Clicking the button calls `POST /api/source/<repo>/grant` which:
   - Re-checks MA ownership
   - Calls GitHub API to clone the repo to a temp dir + zip it
   - Returns a one-time pre-signed URL (5 min TTL) to the zip on S3 /
     Fly Object Storage
5. User downloads. URL expires. No persistent mirror.

Optional v1.1: instead of `.zip`, return a one-time `git clone` token
signed with a deploy key (PAT) that's scoped to one repo + 5 min. Cleaner
for repeat clones but adds GitHub-token rotation work.

## 4. Architecture

```
[MU holder browser]
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

**Auth on MU side:** Reuse wearmu existing session middleware. Add a
`require_ma_holder` extractor that:
- Reads wallet from session
- Calls Solana RPC `getTokenAccountsByOwner` for the MA mint
- Caches result for 60 seconds per wallet
- Returns 403 if balance is 0

## 5. Repo allowlist

**Tier 1 — source-available to MU holders (the 21 we just flipped):**
trio, security-scanner, security-education, jitsuflow, phishguard,
nemotron, gitnote, pasha, kagi, claudeterm, tsugi, hato, hypernews,
flow-anime, Photon, makimaki, tegata, pon, factlens, NOU, thestandard.

**Tier 2 — also private but MU access NOT granted automatically (needs
opt-in per-repo decision):**
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
| MU holder leaks zip publicly | Watermark zip filenames with `<wallet>-<timestamp>` and audit logs. Don't try to DRM — futile. |
| Pre-signed URL re-share | 5 min TTL + IP binding on the presign |
| GitHub PAT compromise | Fine-grained, read-only, quarterly rotation, single bucket destination |
| Repo accidentally pulls in secrets | Bundler runs `git secrets` / `trufflehog` pre-zip and refuses if hits found |
| Cost runaway | Per-wallet rate limit: 10 downloads / day. Cache zip for 1 hour to avoid re-bundle storms |

## 7. Phasing

**Phase 1 (this week)** — manual list page on wearmu, no downloads yet.
Pure "here's what exists, here's why it's gated". Sets expectations.

**Phase 2 (next week)** — implement `/api/source/<repo>/grant` with the
zip path for 1 repo end-to-end (pick `trio` — small, no billing). Test
with 2-3 MU holders.

**Phase 3** — open to all Tier 1 repos. Add the audit log dashboard for
Yuki to see who's pulling what.

**Phase 4 (optional)** — Tier 2 opt-in flow. Per-repo holder pool (some
repos may require MA + additional condition).

## 8. Open questions

- Should the gate also serve `git log` / `git diff` views as HTML, or
  zip-only? (Vote: zip-only for v1; readable diff browser is nice but
  not load-bearing.)
- Do we want a "MU Developer License" file in each repo declaring the
  terms (use freely / no redistribution outside MU holder community)?
  Currently nanobot/thestandard/mu-brand carry `NOASSERTION`. Adding a
  custom LICENSE file at the same time we ship Phase 2 is cheap.
- How do we treat fork-and-pull-request contributions from MU holders?
  Out of scope for Phase 1, but a real ask later. Probably: contributors
  go through the existing wearmu identity, PRs land via a managed bot
  account.

## 9. Cost estimate

- Engineering: 1–2 days for Phase 1+2, another 1 day for Phase 3.
- Runtime: <$5/mo (Fly tigris egress + Solana RPC reads).
- People: zero — fully self-serve.

## 10. Decision log

- 2026-05-19: drafted by Yuki after the public→private flip. Awaiting
  Phase 1 implementation start.
