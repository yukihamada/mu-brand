# MU storefront renewal — 2026-09-17

## Production release verified

- Owner requested App Store creation links and deployment. Released commit `58555e7f1bb20e9f0efdbc48b398a506436a4026` after rebasing onto `aa2da081` (existing quota changes preserved).
- Actions https://github.com/yukihamada/mu-brand/actions/runs/35164266427 succeeded; deterministic wait returned success after 1,163 seconds.
- Final local tests: 207 unit + 4 integration passed; release build passed; both browsers × both languages × four widths passed with App Store destination assertions.
- `python3 scripts/verify_storefront_live.py`: production Japanese/English × 390/1440 widths; home/shop/PDP rendering and App Store links passed, no `/make` links in these tested surfaces, no overflow or JS exceptions. All 9 homepage image elements decoded. Purchase URL retained, no real payment initiated.
- Production CSS SHA256 matches source: `9f89581b7912414751ecba2d3e204e610123482dc94cc964724729bce202cf42`. `/healthz` ok=true. Evidence: `store/target/storefront-preview/live-verification.json`.
- Live: https://wearmu.com/ ; App Store: https://apps.apple.com/app/id6781269252 . Visual judgment and actual revenue lift remain unverified.

## Preview / scope

- Japanese: http://127.0.0.1:8943/?lang=ja
- English: http://127.0.0.1:8943/?lang=en
- Source branch: `feat/storefront-renewal-20260917`, base `b8433845`.
- Isolated worktree: `/var/folders/5h/3zhkzzk12_1g06p6w2vxc45r0000gn/T/sente/mu-storefront-renewal`.
- User approved deployment and requested creation CTAs lead to App Store on 2026-09-17. Deployment evidence is recorded below when verified.
- Running preview uses a release binary and an isolated SQLite database, with `MU_AUTOPILOT=0`, `AGENT_KILL_ALL=1`, `DRY_RUN_ALL=1`; no production credentials.
- The preview uses repository seed products plus 60 current public BJJ/JiuFlow product records. It is not a production DB copy. Do not use it to place orders.

## Design and conversion hypotheses

1. **Make the product the first decision.** A warm off-white/ink storefront, one primary BJJ collection CTA, real catalog imagery and prices. The project strategy prioritizes BJJ demand (`CLAUDE.md:23–31`).
2. **Help shoppers browse.** Three category entry points; product cards without the previous multiple purchase/create/LINE floating prompts. Brand/sort controls remain accessible in a disclosure. Search, pagination and kind filters retain their existing handlers.
3. **Remove purchase ambiguity.** A clear checkout button; gift/repeat-order controls in a disclosure; delivery and return terms immediately follow the purchase area. Email signup moves below the product, not between checkout and conditions.
4. **Creation in the app.** Main navigation, “Made by you”, FAQ, catalog banner, empty search recovery and PDP creation links lead to `https://apps.apple.com/app/id6781269252`. App Store listing verified live (MU, iPhone/iPad, v1.0.2); Japanese/English copy explicitly describes downloading the app.
5. **Measure the funnel honestly.** New `renewal_*` click/impression labels, and `data-ab=storefront-20260917` on home/shop/PDP. This is a release identifier, **not a randomized A/B test**.

No new discounts, synthetic reviews, scarcity claims, promises of delivery dates, product prices, fulfillment behavior, or live ad spend.

## Findings that informed the changes

- Old home selection used `ORDER BY RANDOM()` without an apparel-only check (`store/src/main.rs`, old `index` handler). Live browser inspection showed concept vehicles mixed with clothing.
- Old shop displayed creation banners and two floating creation prompts while the visitor browsed products (`store/src/catalog.rs`, `shop_index`).
- PDP stated free exchange for size differences, but `/returns` excludes fit preference and covers incorrect fulfillment/defects. Copy now follows the existing returns policy rather than changing that policy.
- Browser/synthetic server identities are not joined: `funnel_track_server` creates a new `server:<random>` identity for each event (`store/src/main.rs:81430–81450`, line shifts possible). Therefore dividing checkout_paid by all pageview is not a measured purchase CVR.
- First boot of a fresh local DB leaves `collab_users.display_name` absent because its ALTER precedes table creation. The existing shop query then returns an empty list. Second boot applies the already-existing migration. This is an existing clean-DB initialization issue; no schema edits were made here.

## Baseline: read-only production observation

Observed 2026-09-17, rolling 30 days, `funnel_events`:

| Event | Rows | Distinct visitor_id |
|---|---:|---:|
| pageview | 306 | 260 |
| cta_click | 23 | 7 |
| cta_view | 181 | 151 |
| checkout_attempt | 3 | 3 |
| checkout_start | 3,775 | 3,775 |
| checkout_paid | 40 | 40 |

Later read in this session: `checkout_paid` increased to 41; 3,773 starts had `/api/shop/checkout`, two had `/api/webhook/stripe`. These are changing event counts, not audited orders or unique customers.

Do **not** report `40 / 306` as CVR or event totals as net revenue. Payment event purpose, repeated webhook processing, free/test/operator purchases, refunds and attribution must be reconciled before business conclusions. No such reconciliation is claimed in this deliverable.

## Verification (actual runs)

- `cargo test --manifest-path store/Cargo.toml`: **200 unit tests + 4 integration tests passed**, one pre-existing ignored test.
- `cargo build --release`: **passed**, existing warnings remain.
- `git diff --check`: **passed**.
- `python3 scripts/verify_storefront.py`: **16 combinations passed** (Chromium + WebKit × Japanese + English × 320/390/768/1440 px). Each covers home → BJJ collection, general shop, PDP and intercepted checkout intent. No JS page exceptions, no horizontal overflow, locale preserved in tested links, gift disclosure closed initially, old floating banners absent. Event payloads include the release identifier.
- `python3 scripts/audit_storefront.py`: **passed**. Mobile shop first product y-coordinate **738 → 428 px**, measured at 390×844 using current production vs local preview. This is a layout improvement, not a revenue lift. 9 home image elements decoded (8 product cards plus hero). Special-character search and no-results recovery passed. No-JS home → PDP → checkout link passed. Local event endpoint accepted a real audit event with HTTP 204.
- Screenshots + machine reports: `store/target/storefront-preview/` (`browser-verification.json`, `audit.json`, `home-*.png`, `shop-*.png`, `pdp-*.png`).
- Visual judgment: **unverified**, current model cannot read images. Screenshots are available for human review.
- Real payment, manufacturer submission, delivery, production release, actual CVR/revenue lift: **unverified**. Tests intercept checkout; no payment was placed.
- English homepage is fully authored in English. Existing downstream product descriptions/legacy gift/specification widgets can still contain Japanese; these are not represented as fully localized.

## Measurement and next actions

### Immediately after an approved release

1. Verify live HTML/CSS, price/SKU consistency and image loading on home/shop/PDP.
2. Confirm real first-party `renewal_*` events reach production; exclude verification traffic.
3. Use `scripts/storefront_metrics.sql` to observe **browser purchase intent** for the release cohort: distinct sessions with `/` + release id, and the share with PDP view / checkout_attempt. Do not use synthetic server identities in its denominator.
4. Reconcile paid Stripe sessions with order ledgers, refunds and operator/test status. Add consent-compatible browser→checkout session attribution before claiming purchase CVR or revenue per session.

### Business scorecard

- Primary outcome: **net contribution per human session**, with reconciled net revenue, order cost, shipping, refunds and fees. Not yet measurable end-to-end.
- Supporting: product-view/session, checkout-attempt/session, completed-order/eligible-checkout, net AOV, failed/late fulfillment and refund rate.
- Review after a full 14-day comparable window, and again at 28 days if volume is too low. Segment BJJ traffic and creation traffic; use matched weekdays and source/device mix. Avoid declaring a winner from sparse events.
- Revenue roadmap: establish purchase completion and fulfillment reliability first, then evaluate the existing optional sticker add-on and repeat purchase. Do not add discounts/ads to compensate for unmeasured checkout or delivery failures.

## Daily trend tracking

- `scripts/record_storefront_metrics.py` — one read-only observation per run, appended to `store/target/storefront-preview/metrics-history.jsonl`. Only `fly ssh console` + `sqlite3 -readonly`; no production writes.
- `scripts/report_storefront_metrics.py` — prints the table and first→last delta.
- Registered as launchd `tokyo.hamada.mu-storefront-metrics`, daily 09:30 (verified via `launchctl print`; `runs = 0`, waits for the calendar trigger).
- First observation 2026-09-17 09:37 JST: cohort landing 1 / product view 0 / intent 0; 24 h events: pageview 25, cta_click 9, checkout_start 71, checkout_paid 2; `catalog_orders` 71 `checkout_pending` with `amount_jpy` 0.
- Reading rules: `browser_cohort_14d` is a rolling 14-day window of the release cohort, not purchase CVR. `events_24h` counts are event rows, not unique customers or net revenue. `checkout_paid` includes server-synthesised events with unrelated identities. `checkout_pending` rows here carried `amount_jpy` 0, so they are not revenue evidence.
- Judge after a comparable window. Do not call a winner from single-digit daily events.

## Restart / reproduce

```sh
cargo test --manifest-path store/Cargo.toml
cargo build --release --manifest-path store/Cargo.toml
python3 scripts/preview_storefront.py --restart
python3 scripts/verify_storefront.py
python3 scripts/audit_storefront.py
python3 scripts/record_storefront_metrics.py   # production, read-only
python3 scripts/report_storefront_metrics.py
```

On a fresh clone, start once without `--restart`, then restart to apply the existing clean-DB display_name migration. `--seed-public` optionally reads current BJJ/JiuFlow public product fields via authenticated Fly read-only SQLite; it writes only the local preview DB. Prices and products may change between runs.

Only after approval: inspect current upstream/diff, commit intended files, push through the normal Actions deployment, wait for its known run with `te wait github`, then verify the public pages. Do not deploy directly.
