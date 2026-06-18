# Contrado sales outreach — draft (NOT YET SENT)

**Status**: Draft, awaiting Yuki's send.
**To**: sales@contrado.com (verify the right address — also try b2b@contrado.com)
**From**: mail@yukihamada.jp
**Date drafted**: 2026-05-24
**Why this exists**: Helix API has /orders endpoints only — no pricing endpoint and no /products. To make Contrado a viable fulfillment route alongside Printful we need an official reseller tier quote rather than guessing from public retail pricing.

---

## Subject

Reseller pricing inquiry — Men's Long Sleeve Performance Top, 30-100 units/month to Japan

## Body

Hello Contrado team,

I'm the founder of **Enabler Inc.** (株式会社イネブラ, Tokyo, Japan), and we run **wearmu.com**, a curated POD apparel marketplace focused on the BJJ / jiu-jitsu community.

We currently fulfill most products through Printful EU, but we're hitting a quality ceiling: Printful's all-over-print rashguard (product 301) leaves an unprinted white waistband and white cuffs, which our IBJJF-spec belt-color rashguard line really needs to be fully printed. Contrado's UK sublimation product looks like the right answer for the premium tier we're considering.

A few questions to confirm before we wire Contrado as a fulfillment route via the Helix API:

1. **Pricing tiers for Men's Long Sleeve Performance Top (sublimation, full-coverage)**
   - What is the wholesale / reseller cost per unit at the following monthly volumes?
     - 1-10 units / month
     - 30-50 units / month
     - 100+ units / month
   - Is there a per-design setup fee, or is it included in the unit price?

2. **Print coverage**
   - Confirm that the print fully covers the front, back, both sleeves, waistband and cuffs (i.e. no white unprinted panels). We provide a single 300 DPI artwork that we want to wrap across all panels.

3. **Shipping to Japan**
   - Per-unit shipping cost from your UK fulfillment to Tokyo, single-item POD orders.
   - Is there a way to consolidate multiple orders into a single shipment (weekly batch) to reduce per-unit shipping?

4. **Lead time**
   - Production time after order received.
   - Total time including shipping to Japan (TYO).

5. **Helix API permissions**
   - Our current API key (`CONTRADO_API_KEY` issued via the Maker Platform) returns `403 Forbidden` on `POST /helix/v1/orders/create`. What is the path to upgrade to order-creation permissions? Do we need a separate B2B contract?

6. **Maker Platform vs Helix integration**
   - For our use case (5-20 SKUs uploaded once, then automated order routing), do we register products on the Maker dashboard and reference them by SKU in Helix order calls? Or is there a way to register products via API?

Our forecast for the first 90 days is **30-50 units/month** across 5 belt-color rashguard SKUs, scaling to **100-200/month** if the line resonates. We can commit to a small upfront batch (e.g. 10 units of a sample SKU) so you can validate our integration.

Thank you, and looking forward to your reply.

— Yuki Hamada
Founder, Enabler Inc. (株式会社イネブラ)
mail@yukihamada.jp / +81 90 7409 0407
wearmu.com

---

## After the reply lands

- Plug the real reseller per-unit cost into the genka spreadsheet (currently estimated ¥7.5-10.2K).
- Compare against Printful 301 (¥3.5K wholesale).
- Decide retail per SKU: either keep ¥9,800 (Contrado loss → drop) or position as premium ¥19,800-24,800 line.
- If permissions are upgraded, set `fulfillment_route='contrado_uk'` on the trial SKU and route a real test order through Contrado to verify end-to-end.

## Sending it

When ready:
```bash
gog gmail send --account mail@yukihamada.jp \
  --to "sales@contrado.com" \
  --subject "Reseller pricing inquiry — Men's Long Sleeve Performance Top, 30-100 units/month to Japan" \
  --body "$(cat /Users/yuki/workspace/mu-brand/docs/CONTRADO_SALES_OUTREACH.md | sed -n '/## Body/,/^---$/p' | sed '1d;$d')"
```
