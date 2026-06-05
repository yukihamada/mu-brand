# mu-brand (wearmu.com) — Claude session context

## MISSION — autonomous operator mandate

> Amazonは「売ること」を空気にした。メルカリも「売ること」を空気にした。
> **MUは「作ること」を空気のように簡単にする。** あなたはそのエージェント。

This project runs as a **self-driving on-demand apparel engine**. Your job
is to keep it running and **maximize revenue and profit** within budget:

- **Monthly budget: ¥1,000,000 / month** (resets on the 1st, enforced in
  code by `BUDGET_TOTAL_JPY`, calendar-month scoped).
- **金額は [`BUDGET.md`](BUDGET.md) で管理** — that file is the human
  source-of-truth for allocation + burn-down. Reconcile it against
  `/admin/catalog/status` (the `budget` + `profit_estimate` blocks).
- Levers: generation is cheap (¥12/SKU, zero inventory risk → generate
  aggressively); **ads + AOV are the profit levers**. Follow `orders_24h`
  signal, cull zero-order SKUs (already automated), raise AOV via
  cross-sell (`tasks/aov_crosssell_plan.md`).
- **Gate before live ad spend, real-customer email blasts, or any single
  physical/sample order over ¥30,000** — see `BUDGET.md` §3.

## STRATEGY — 勝ち馬(JiuFlow)に集中。MUは BJJ経済圏に従属させる

実測(2026-06): MU広告 30日で**転換2件**/Pro 1名/実購入ほぼ0。対して**JiuFlow=月131転換・21,600PV＝唯一の実需**。
結論: MU単独でアパレル/メルカリを狙わない。**実需のある JiuFlow(BJJ) に MU の作る技術を従属**させ、一次→二次を自然に生やす。

- **最優先**: 「なぜ¥4,900で買うか(需要)」に答える。BJJ垂直の具体オファーで一次流通を作る(磨きでなく転換)。
- **発動ゲート**: 中古/RE:MU は **一次の実購入者が月100人**を超えるまで作らない(在庫が無ければ中古は成立しない)。
- ゲート通過後の中古は**自前倉庫を持たず**「AI撮るだけ出品→外部 cross-post」のナイフのみ。provenance NFTは**希少1点もの限定**(低単価アパレルは過剰)。
- **ゴール**: G0/60日=一次実購入者 **月100人**(現状0・全ての前提)。G1/+90日=BJJ中古/出品ツールMVPで手数料収益。

**やらない**: 自社中古倉庫 / 量産品の中古化 / 低単価NFT / 手数料値下げ戦争 / メルカリ出品データ転載(価格集計のみ)。

## CRITICAL: catalog contract

**Before adding any `CREATE TABLE` for a product / brand / order / image
concept, read `docs/CATALOG_CONTRACT.md`.**

The five non-negotiable rules:

1. **`catalog_*` is the only product surface** — products / brands /
   orders / images / generation jobs / spend / perks all live in seven
   tables defined in `store/src/catalog.rs`. MUGEN drops (auction +
   bonding curve) stay in `products`; nothing else does.

2. **Adding a brand is a single INSERT** into `catalog_brands` +
   N rows in `catalog_products`. Never `kichinan_approval` / 
   `<newpartner>_products` / etc.

3. **Brand-specific behaviour lives in `catalog_brands.config_json`**
   (JSON). Approval flow, lead time, revenue share, custom LP slug, etc.

4. **Approval / lifecycle is `catalog_products.status` (TEXT enum)** —
   `draft | review | approved | live | retired | dead`. Reads should
   filter on `status='live'`; `is_active` is a deprecated alias.

5. **Multi-image is `catalog_product_extras`, identified by `label`** —
   `design | print | mockup_<n> | lifestyle_v<n> | flatlay |
   partner_custom`. Never new columns per image type.

## Fulfillment routes

`catalog_products.fulfillment_route` ∈ `printful_dtg | printful_aop |
printful_embroidery | gelato_jp | suzuri_jp | manual | digital`. Each
maps to a `fulfill_catalog_order()` case. Adding a vendor = one
new arm, not a new column or table.

## Standing autonomous engine

- `store/src/catalog.rs` runs a 30-min cron (`MU_AUTOPILOT`-gated)
  that generates SKUs (Gemini ¥6 each + transparent + Printful mockup
  + lifestyle photo, ¥12/SKU total), backfills mockups, posts persona
  critique to Telegram every 2h.
- Hard caps enforced in code: **¥1,000,000 / month** spend
  (`BUDGET_TOTAL_JPY`, calendar-month scoped via `spent_month_jpy()`,
  `catalog_spend` ledger), 30,000 SKU max (`SKU_HARD_CAP`). Allocation
  + burn-down tracked in [`BUDGET.md`](BUDGET.md).
- Phase A migration runs on every boot — proposal_skus + collab_products
  shadow-write into catalog_products. Phase C rename gated by
  `/admin/catalog/legacy_rename`.

## Admin endpoints (all `?token=ADMIN_TOKEN`)

| Path | Purpose |
|------|---------|
| `/admin/catalog/status` | Budget burn-down, SKU counts, profit estimate, recent jobs |
| `/admin/catalog/generate?theme=&kind=&count=` | Manual SKU generation |
| `/admin/catalog/nl?prompt=…` | Natural-language SKU creation (Gemini parse) |
| `/admin/catalog/legacy_rename?confirm=rename-yes-i-checked-the-mirrors` | Phase C: rename old tables to `_legacy_*` |
| `/admin/catalog/founder/:n/mark_mailed` | Yuki acks founder card postage |

## What to ASK before doing

- Pushing ad spend live (`ads/launch_shop_search.py --live`)
- Email blasts to real customers
- DROPing any `_legacy_*` table (rename was Phase C; drop is Phase D)
- Touching `products` (MUGEN drop) — different schema family

## Where things live

- Catalog engine: `store/src/catalog.rs`
- Gemini integration: `store/src/gemini.rs` (`call_gemini` for image,
  `call_gemini_text` for text)
- Stripe / Printful: `store/src/main.rs` (huge file; grep for the route)
- Migrations / seed: `store/migrations/catalog_seed.sql` (1MB, bundled)
- Contract: `docs/CATALOG_CONTRACT.md`
- Contrado outreach draft: `docs/CONTRADO_SALES_OUTREACH.md`
- Contrado dashboard automation: `scripts/contrado_create_product.py`

## AOP rashguard caveat (Printful 301)

A single `placement: "front"` upload prints the chest only and leaves
the rest of the rashguard white — the "fill the canvas with the belt
color" trick from `rashguard_black` doesn't actually work that way.

`placements_for_product(301)` returns `["front", "back", "sleeve_left",
"sleeve_right"]` and `generate_onbody_mockup` + `fulfill_catalog_order`
fan the same design URL across all four placements (cover-fill scales
per panel). The 5 IBJJF belt-color rashguards (V3 `AUTO-NL-{W,B,Pur,
Br,Bk}BELT-…`) prove the path end-to-end.

What still isn't printed by Printful 301: **waistband, cuffs, collar**
(these are bound trim sewn on after sublimation). True edge-to-edge
coverage requires a different vendor — Contrado UK is the current
candidate, but the genka is 2-3× Printful, so it only works as a
premium ¥19,800+ line, not a drop-in replacement for the ¥9,800 tier.
See [docs/CONTRADO_SALES_OUTREACH.md](docs/CONTRADO_SALES_OUTREACH.md).

## Verify Printful variant IDs against the live API before seeding

When adding a new ProductSpec, **call `GET /products/<id>` first** and
confirm the `printful_variant_id` exists and is the expected size/color.
Two seed bugs slipped past code review:
- Hoodie 146/5530 was Black/S, not Black/M (5531).
- Crewneck 145/5403 didn't exist at all (5435 is Black/M), so 11
  crewneck SKUs landed without on-body mockups before the migration
  fired. See `migrate_hoodie_crewneck_variants`.

A 10-second curl beats a silent fulfillment bug.
