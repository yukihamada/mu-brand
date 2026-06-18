# Catalog Contract (Single Source of Truth)

**Status**: V1 — drafted by Opus 2026-05-22, approved by Yuki.
**Owner**: `store/src/catalog.rs`
**Last incident this contract prevents**: 137 `CREATE TABLE` statements
spread across 4 parallel design families (drop / collab / proposal /
catalog) — same concept implemented 4 different ways with different
status enums, image columns, and partner-onboarding flows.

## Why this exists

Before V1 we had:
- `*_approval` × 7 (kichinan/asoview/elsoul/ele/nojimahal/ryozo/…) —
  one table per partner with near-identical schema.
- `collab_*` × 5 (products / orders / users / applications / signups).
- `proposal_*` × 6 (proposals / proposal_skus / proposal_extras_skus / …).
- `catalog_*` × 7 (this one — added during the 2026-05-21/22 unification).
- `products` (the original MUGEN drop table — out of scope for this
  contract; bonding-curve / MA / auction lives there and stays there).

Adding the 12th partner shouldn't require a new table. Adding a new
fulfillment vendor shouldn't require a new column.

## The Five Rules

### Rule 1 — `catalog_*` is the only product surface
| Concept                              | Canonical table             |
|--------------------------------------|-----------------------------|
| Products (POD / collab / partner)    | `catalog_products`          |
| Brands (MU / each partner)           | `catalog_brands`            |
| Orders                               | `catalog_orders`            |
| Per-product images (design / mockup / lifestyle / etc.) | `catalog_product_extras` |
| Generation job log                   | `catalog_gen_jobs`          |
| Spend ledger (¥100K cap)             | `catalog_spend`             |
| Limited perks (Founder card 1..100)  | `catalog_founder_cards`     |

**MUGEN drops stay in `products`** — bonding-curve + MA auction semantics
don't fit POD-catalog shape and migration would be lossy.

### Rule 2 — Adding a brand is a single INSERT
```sql
INSERT INTO catalog_brands (slug, name, emoji, color_primary, tagline,
  custom_domain, is_active, revenue_share_pct, config_json)
VALUES ('newpartner', 'New Partner', '🆕', '#888', '…',
  NULL, 1, 20, '{"approval_required": true, "lead_time_days": 14}');
```
Plus 1..N `INSERT INTO catalog_products (sku, brand='newpartner', …)`.
**No new table**, **no new columns**, **no new admin endpoint**.

### Rule 3 — Brand-specific behavior goes in `catalog_brands.config_json`
JSON keyspace examples (extend, don't replace):
- `approval_required` (bool) — partner must approve before live
- `revenue_share_pct` (int) — when set on catalog_brands too, the JSON
  wins for per-SKU overrides
- `lead_time_days` (int) — beats the default 14
- `lp_template` (str) — custom landing-page slug (defaults to /shop/:sku)
- `payment_email` (str) — partner notification on each sale

### Rule 4 — Approval / lifecycle is `catalog_products.status` (TEXT enum)
Values (canonical, ordered):
| Status     | Meaning                                                       |
|------------|---------------------------------------------------------------|
| `draft`    | Created but partner / admin hasn't approved yet               |
| `review`   | Pending partner approval — auto-emails them on insert         |
| `approved` | Partner OK'd but not yet listed (e.g. waiting for asset)      |
| `live`     | Listed on `/shop` and buyable                                 |
| `retired`  | Hidden from listings, existing orders still fulfillable       |
| `dead`     | Soft-delete; do not fulfill even if order webhook arrives     |

`is_active` is retained as a deprecated alias of `status='live'` for
back-compat. **New code reads `status`**.

### Rule 5 — Images go in `catalog_product_extras`, identified by `label`
Canonical label values (extend, don't replace):
- `design` — raw AI-generated print art (white background)
- `print` — same art with white→alpha transparency for AOP
- `mockup_<n>` — Printful mockup-generator output (`<n>` allows multi-angle)
- `lifestyle_v<n>` — Gemini on-body photo (no face)
- `flatlay` — folded product still life
- `partner_custom` — partner-supplied photography

`catalog_products.mockup_url_external` mirrors the primary `mockup_<n>`
for back-compat with merch-bridge import data; **new code reads
`catalog_product_extras`**.

## Fulfillment routes (`catalog_products.fulfillment_route`)

Canonical values:
- `printful_dtg` — DTG ink, white background = no ink (tees, hoodies)
- `printful_aop` — sublimation, every pixel prints (rashguards, AOP totes)
- `printful_embroidery` — thread (caps, beanies)
- `gelato_jp` — Gelato Japan POD (5-10 day domestic ship)
- `suzuri_jp` — SUZURI Japan (no-stock, JP-only)
- `manual` — partner ships themselves; we just process payment
- `digital` — no physical fulfillment (e-book, NFT, license)

Each route maps to a webhook handler in `fulfill_catalog_order()`.

## Migration plan (one-shot, additive, idempotent)

```
Phase A (no-risk):
  - Add status / fulfillment_route / legacy_source columns to
    catalog_products (idempotent ALTER).
  - Add config_json to catalog_brands.
  - Shadow-write: read proposal_skus + collab_products, INSERT OR
    IGNORE into catalog_products with sku = "{LEGACY_PREFIX}-{slug-id}",
    status='live' (or 'draft' for partner_approved=0), and
    legacy_source = 'proposal_skus' | 'collab_products'.
  - Reads stay on old tables — nothing breaks.

Phase B (next session):
  - Switch /shop and partner LPs to read from catalog_products.
  - Backfill catalog_orders from collab_orders / mu_purchases for
    historical analytics.

Phase C (later):
  - Rename legacy tables to `_legacy_*`.
  - Drop after 30 days of zero reads (monitor via SQLite logging).
```

## What this contract bans

- ❌ `CREATE TABLE <partner>_approval` for a new partner → use
      `catalog_brands.config_json.approval_required = true`.
- ❌ New `*_products` table for a new fulfillment vendor → set
      `fulfillment_route` + add the case in `fulfill_catalog_order()`.
- ❌ Inline boolean columns like `is_jiufight`, `is_kokon_legal` →
      use `catalog_products.brand` or a `tags TEXT` column with
      JSON list (`'["bjj","tournament"]'`).
- ❌ Per-vendor image column (`gelato_url`, `suzuri_url`, …) →
      one row in `catalog_product_extras` per vendor with label.

## How a future Claude session uses this

Before adding a `CREATE TABLE` for anything product-shaped, search
this doc for the concept. If it fits one of the existing
`catalog_*` tables, extend instead. The five-line section
"What this contract bans" is checked into CLAUDE.md so the rule
travels with the prompt context.
