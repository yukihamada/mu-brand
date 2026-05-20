# JIUFLOW20 Coupon — Stripe Dashboard Setup

**Status:** Code shipped (banner + ref detection), Yuki must create the coupon in Stripe.
**Date:** 2026-05-21

## What was shipped

- `/buy?ref=jiuflow` shows a green coupon banner: "checkout 画面で `JIUFLOW20` を入力 → 20% off"
- Banner has a "コードをコピー" button (writes `JIUFLOW20` to clipboard)
- Ref is persisted in `localStorage["mu_ref_jiuflow"] = "1"` so it survives navigation
- Stripe checkout already has `allow_promotion_codes=true` everywhere — users can paste the code at the Stripe-hosted checkout

## Yuki's manual step (5 minutes, Stripe Dashboard)

1. Open https://dashboard.stripe.com/coupons (Live mode)
2. **+ New coupon**:
   - Type: **Percentage discount**
   - Percent off: **20**
   - ID: leave default (Stripe will generate)
   - Name: `JiuFlow 紹介 20% off`
   - Duration: **Once** (per redemption — not recurring)
   - Limit number of redemptions: **100** (caps blast radius)
   - Redeem by: **2026-06-30 23:59 JST**
   - Currency: **JPY**
3. Save coupon
4. On the saved coupon page, **+ Promotion code**:
   - Code: **`JIUFLOW20`** (exact, all caps — must match the banner copy)
   - Limit redemptions per customer: **1** (no double-dip per email)
   - Active: ON
5. Save promotion code

## Verify (after creation)

```bash
# Test the flow end-to-end
open "https://wearmu.com/buy?ref=jiuflow"
# → green banner visible
# Click "買う" → Stripe checkout
# Type JIUFLOW20 into the "プロモーションコード" field
# → ¥4,900 → ¥3,920 (20% off)
```

## Cross-promo distribution channels

| Channel | Where | Hook |
|---|---|---|
| JiuFlow paying members (161 active) | Email blast (Yuki sends, NOT auto) | "JiuFlow Pro さんへ — wearmu の MUGEN Tシャツ 20% off"  |
| jiuflow.com /releases page | Footer P.S. line | "P.S. wearmu.com/buy?ref=jiuflow で 20% off (JIUFLOW20)" |
| X thread (Yuki posts) | 8-tweet thread に "JiuFlow Pro 紹介で 20% off" を 1 tweet 追加 | コード visible |
| jiuflow iOS app | /more screen に external link | "wearmu 紹介クーポン →" |
| 個別 charter outreach DM | 立石・元メルカリ・kenny 等 | "限定 20% off コード を 送る" |

## Tracking

- `?ref=jiuflow` is stored in `localStorage` (`mu_ref_jiuflow`) — persists across visits
- Stripe coupon redemption count → /dashboard/coupons/[id] shows N/100 used
- Cross-check: Stripe metadata.utm_source if set (currently not — could add later)

## What's NOT in this PR (out of scope)

- Server-side auto-apply via `discounts[0][promotion_code]=...` — would require the promotion_code ID after creation. Skip for v1 (manual paste is fine).
- Same code for X campaign — could create `X20` separately later
- One-click in-app application for JiuFlow iOS — add later
- jiuflow.com /releases footer link — separate PR (jiuflow-ssr)
