# Phase 3 Backlog — crypto checkout follow-up items

Completed in Phase 2 + Phase 2.1:
- ✅ Price cap raised to ¥300,000
- ✅ Crypto surcharge layer (USDC/SOL +3%, ETH +5%) with cap clamp
- ✅ KYC at checkout (JPY + crypto) and at place_bid
- ✅ `pending_crypto_payments` table + Solana Pay URL / EIP-681 URL generation
- ✅ Helius webhook with **constant-time auth**, **recipient verification**,
       **amount-tolerance (95%) check**
- ✅ Stripe Identity verification-session endpoint (`/api/kyc/identity-session`)
- ✅ Admin CSV exports gated by constant-time `x-admin-token`
- ✅ CSV formula-injection prevention (`= + - @` cells prefixed)
- ✅ Self-contained frontend (`/static/payments-ui.js`) with payment selector,
       KYC modal, Solana Pay QR + status polling
- ✅ 12/12 unit tests, 23/23 E2E smoke tests passing

---

## Open items (rank by impact × required-for-launch)

### High — required before crypto goes live to public

1. **Shipping address collection on crypto checkout**
   - Current state: shipping comes from Stripe session in the JPY path. Crypto
     orders persist `pending_crypto_payments` without a shipping address.
   - Needed: extend `CryptoCheckoutBody` with `{name, line1, line2, city,
     state, zip, country, phone}` and add corresponding columns. Frontend
     KYC/crypto modal needs the form. Without this, the Printful auto-order
     cannot fire on Helius confirmation.

2. **Helius webhook → Printful order trigger**
   - Today: marks pending row `confirmed` and increments `products.sold`.
   - Missing: replicate the JPY-flow `create_printful_order` call so the
     garment actually ships. Needs the shipping fields from item 1.

3. **Confirmation email on crypto confirmed**
   - Send via Resend (`RESEND_API_KEY` already in env). Template should
     include tx signature, amount, expected ship date, and "if any shipping
     fields were missing, reply with them".

### Medium — operationally important

4. **JPY ↔ crypto rate refresh cron (Pyth)**
   - Today: `JPY_PER_USD / JPY_PER_SOL / JPY_PER_ETH` env vars, defaults
     150 / 25,000 / 600,000.
   - Needed: tokio cron that fetches Pyth price feeds every 5 min and writes
     latest values back to env / SQLite settings table. Off-ramp processor
     covers ±5% slip so 5-min freshness is fine.

5. **ETH on-chain settlement reconciliation**
   - Today: ETH path generates an EIP-681 URL; payment is detected by no
     mechanism (no ETH RPC integration).
   - Needed: either (a) Alchemy/QuickNode webhook on `MU_ETH_RECIPIENT`
     incoming transactions, or (b) a poller using `eth_getLogs` + reference
     embedded in `data`. Required for NOUNS proposal Track 3 (MA × NOUNS
     auctions settling in ETH).

6. **Stripe Identity webhook**
   - Today: `/api/kyc/identity-session` returns a verification URL; the
     `kyc_records.stripe_identity_*` columns are populated by no path.
   - Needed: Stripe webhook handler that receives
     `identity.verification_session.verified` and updates `kyc_records.
     stripe_identity_status` by `metadata.kyc_record_id`.

### Low — polish

7. **Frontend KYC → Identity redirect button**
   - After KYC fields submitted, optionally show a "提出した内容を Stripe で
     verify (撮影 + OCR)" button that POSTs to `/api/kyc/identity-session`
     and redirects to `url`. Required only when we want strong KYC (level 2).

8. **Rate display in UI**
   - Show today's JPY/USDC rate next to the USDC button when the user picks
     it, so the customer sees the conversion before scanning QR.

9. **Pending payment cleanup cron**
   - Sweep `pending_crypto_payments` older than `CRYPTO_PAYMENT_TTL_MIN`
     and set status='expired'.

---

## Deferred (gated on NOUNS DAO approval)

- NOUNS Treasury 10% routing on `× NOUNS` track drops
- MA × NOUNS auction in ETH (Track 3 of `NOUNS_PROPOSAL.md`)

---

## Acceptance test for "Phase 3 done"

- [ ] Customer can pay USDC/SOL → product ships from Printful within 24h
      without operator intervention
- [ ] Customer receives confirmation email within 60s of on-chain settlement
- [ ] Rates auto-refresh every 5min via Pyth (verifiable via `JPY_PER_USD`
      log line)
- [ ] Stripe Identity verification status backfills into `kyc_records`
- [ ] Pending crypto payments older than 15min show as expired in CSV export
