# MU admin surface

Status: actively maintained · last updated 2026-05-22

Every admin endpoint on `wearmu.com` is **token-gated**. The token lives in
the `ADMIN_TOKEN` Fly secret. Pass it one of three ways:

| Method | Example |
|---|---|
| Query param | `/admin?token=<ADMIN_TOKEN>` |
| Cookie | `Set-Cookie: admin_token=<ADMIN_TOKEN>` (set once via login URL) |
| Bearer header | `Authorization: Bearer <ADMIN_TOKEN>` (API only) |

Get the live token from Fly:
```bash
fly ssh console -a mu-store -C 'printenv ADMIN_TOKEN'
```

The cookie is set automatically when you first hit any admin page with
`?token=…`; subsequent pages drop the query parameter.

## /admin · hub

`GET /admin?token=<TOK>` — entry point. Renders:

- **Live KPIs (8 stat tiles)** — revenue today / week / month, orders today /
  month, lifetime orders, products active, broken-mockup alert count.
  Server-rendered with current values for no-JS clients; hydrated every
  10 s (focused tab) or 60 s (background) via `/api/admin/dashboard`.
- **Recent purchases feed** — last 5 orders, buyer email anonymized as
  `f…@domain.com` so the page can be shared in a screenshot.
- **Tile grid** — every sub-page below, grouped by domain.

## Sub-pages

| Path | Purpose |
|---|---|
| `/admin/brands` | Brand grid (catalog_brands + legacy) — per-brand active/sold/revenue + sample mockup + filter (active/legacy/w-domain) |
| `/admin/products` | Product list with thumbnails, inline edit, regen mockup, regen design |
| `/admin/product/new` | One-form manual SKU create |
| `/admin/collabs` | Collab dashboard — AI prompt + cadence + per-collab 8-product grid |
| `/admin/proposals` | All proposal records (approval gate) |
| `/admin/proposal/:slug/manage` | Per-collab approve / revoke / cadence |
| `/admin/users` | `/you` subscriber roster |
| `/admin/outreach` | Multi-niche outreach pipeline + manual add |
| `/admin/sponsor-apps` | Sponsor application status |
| `/admin/sweep` | MU × SIIIEEP collab dashboard |
| `/admin/nakamura` | Nakamura brothers UFC project dashboard |
| `/admin/bounty` | Bug bounty triage + reward issuance |
| `/admin/db` | Raw SQLite table inspector (read-only) |
| `/admin/bids` | MA auction bid history |
| `/admin/pod` | POD vendor catalog (Printful / SUZURI / Gelato) |
| `/admin/costs` | Per-SKU cost / margin / Stripe fee |
| `/admin/retention` | Cohort retention curves |
| `/admin/funnel` | visit → buy aggregated funnel |
| `/admin/agent` | Background-agent journal (decisions, actions, observations) |
| `/admin/collab-signups` | Self-serve collab apply submissions |
| `/admin/create` | NL → SKU pipeline (Gemini parse + Printful mockup) |
| `/admin/catalog/status` | Catalog engine: budget burn, SKU counts, recent jobs |
| `/admin/ma/gifts` | MA Lottery gift history |

## JSON APIs

All return JSON, all token-gated unless noted.

| Method · Path | Purpose |
|---|---|
| `GET /api/admin/dashboard` | Single-call KPI snapshot (revenue, orders, products, alerts, recent purchases). Used by the `/admin` hub for live hydration |
| `GET /api/admin/brands` | Merged brand registry (`catalog_brands` ∪ legacy `products.brand`) — slug / name / emoji / color / tagline / domain / active flag / live counts / lifetime revenue / sample mockup. Used by `/admin/brands` |
| `GET /api/admin/products` | Paginated product list with admin-only fields |
| `POST /api/admin/products` | Create a new product row |
| `POST /api/admin/products/:id/update` | PATCH single product (whitelisted fields incl. `print_url`) |
| `POST /api/admin/products/:id/regen_design` | Trigger Gemini design regen → R2 upload → DB update |
| `POST /api/admin/products/:id/regen_mockup` | Trigger Printful mockup-generator (async; 202 returned, poll `/api/products/:id/mockup.png`) |
| `POST /api/admin/products/:id/regen_lifestyle` | Generate N lifestyle photos (default 3) via Gemini |
| `POST /api/admin/products/:id/regen_similar` | Create a NEW SKU using this row's design seed |
| `POST /api/admin/products/:id/payment-link` | Create or refresh a Stripe Payment Link for direct purchase |
| `GET /api/admin/collabs/dashboard` | Collab dashboard JSON used by `/admin/collabs` |
| `GET /api/admin/outreach/:slug` | Outreach rows for a given niche slug |
| `POST /api/admin/outreach` | Add a manual outreach row |
| `POST /api/admin/outreach/:id/status` | Update status: identified · contacted · replied · agreed · shipped · photo_received · declined · archived |
| `POST /api/admin/add` | Single-call SKU + Printful sync + Stripe Price create (legacy) |

`/api/admin/dashboard` response shape:

```jsonc
{
  "mark": "━◯━",
  "as_of_unix": 1779400000,
  "revenue":  { "today": 6800, "week": 35000, "month": 108600, "lifetime": 108600 },
  "orders":   { "today": 1,    "week": 2,     "month": 16,     "lifetime": 16 },
  "products": {
    "active": 1521,
    "total":  1521,
    "with_design": 1500,
    "with_mockup": 1494,
    "likely_broken_mockup": 7
  },
  "outreach": { "total": 34, "replied": 12 },
  "alerts":   { "broken_mockup_count": 7, "missing_design_count": 21 },
  "recent_purchases": [
    { "created_at": "1779356121", "brand": "you", "drop_num": 10, "amount_jpy": 6800, "buyer": "ke…@atsume.io", "pf_status": null }
  ]
}
```

## Health & monitoring

- `/healthz` — public, returns `ok` (used by Fly probes + CI smoke).
- Post-deploy smoke (`.github/workflows/deploy.yml`) hits `/healthz` then
  `/api/admin/dashboard?token=$ADMIN_TOKEN` and verifies the JSON
  contains `"mark":"━◯━"`.

## Cookie + URL hygiene

When `?token=` is in the URL the server sets `admin_token` cookie
(HttpOnly, SameSite=Strict, 30-day Max-Age) so the operator can drop the
query param and reload. To clear: `GET /admin/logout` deletes the cookie
and redirects back to `/admin` (token re-prompt).

## Adding a new admin route

1. Define the handler in `store/src/main.rs` (or split file).
2. Wrap with `admin_auth(&headers, &q, db, "/admin/<path>")` as the first
   line so unauthenticated calls 401.
3. Register in the router (search `route("/admin` for adjacency).
4. Append to the tile grid in `admin_hub_page` if it's a UI route.
5. Append to this doc.

## Related

- `docs/CATALOG_CONTRACT.md` — schema rules for catalog_* tables.
- `docs/AUTONOMY_OPS.md` — agent / cron-driven self-operation policies.
