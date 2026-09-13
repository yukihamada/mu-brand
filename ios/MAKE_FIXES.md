# Make iOS audit fixes — integration notes

## Changes

- `MU/Sources/Core/API.swift`: reads `GET /api/make/kinds`; string kind requests
  support the complete physical catalog. Price edits decode persisted values,
  falling back to uncached `GET /api/make/item/:sku?t=` on legacy responses.
  Remix form encoding preserves literal `+` characters.
- `MU/Sources/Core/Models.swift`: dynamic kind metadata, price estimates, typed
  preview kind / opaque revision, generation and operation identities,
  captured create/variation/remix intents, per-design preview validation,
  retained polish score, saved price/earnings updates and status transitions.
- `MU/Sources/Features/Make/MakeView.swift`: searchable, category-grouped picker
  plus five common shortcuts; seven previously shipped choices on initial
  catalog failure. Auto has no invented price; tote starts at ¥3,800.
  Generation, automatic/manual variation, remix, polish, price and peek reject
  stale responses. Reset/disappearance cancels tracked tasks. Authentication
  and consent retry the captured intended action. Preview polling runs 50 times
  at six-second intervals, with delayed/retry UI. All product previews say
  “Reference image”, never verified on-body. Review/live/retired status controls
  buyability. Price editing stays attached to the SKU that opened the editor.
- `MU/Sources/Features/Agent/AgentView.swift`: AI-selected physical kinds use the
  same catalog; unsupported explicit kinds no longer silently become Auto.
- `MU/Resources/{ja,en}.lproj/Localizable.strings`: new picker, price, preview and
  status copy; generic product wording instead of apparel-only wording.
- `MUTests/ModelsTests.swift`, `Package.swift`, `.gitignore`: existing iOS XCTest
  suite also runs as a lightweight macOS Swift package, without an app build.

## Backend contract required

1. `GET /api/make/kinds` returns `{ok:true,items:[...]}` from the canonical Web
   physical kinds and product specs, without a truncated page. Each item has
   `kind`, `label_ja`, `label_en`, `retail_jpy`, `category`, `can_remix`. The client
   also decodes optional `can_make`. `can_make:false` disables selection and
   submission even if a historical price exists. Non-Auto kinds with null or
   non-positive price are also disabled (including `rashguard_contrado` without
   a product spec), with an explanatory Japanese/English label. Legacy priced
   kinds with no `can_make` remain available. Auto is the price-free exception.
   Make, captured auth retries, variations, design requests and Agent generation
   check availability before submitting; unavailable explicit kinds never
   silently become Auto. The client
   adds Auto and deduplicates kind IDs. Known category keys are `wear`/`apparel`,
   `carry`/`accessories`, `home`/`lifestyle`, `pet`/`pets`, and `common`; other
   categories remain visible as returned. `retail_jpy:null` on a physical kind
   means it is not ready for creation. `can_remix` must match `/api/design-remix` eligibility,
   including the actual fulfillment route, not merely whether kind is physical.
   Optional audit metadata is decoded on the kind catalog only:
   `unavailable_reason`, `unavailable_reason_ja/en`, `temporary`,
   `requires_vendor_preflight`, `country`, `allowed_countries`,
   `excluded_countries`, `stock_checked_at`. Disabled rows and attempted actions
   display the server reason in the current language, using the other supplied
   language if needed and generic copy only when neither is supplied. These
   fields do not turn a dated stock snapshot into a guarantee of availability.
   Picker refresh bypasses the URL cache; successful responses replace labels,
   prices and availability. Refresh failures retain the last successful catalog
   rather than resurrecting denied kinds with local fallback data. Pull to
   refresh the full picker to get updated stock decisions and prices.
2. Make/remix may add optional `can_remix` and `fulfillment_route`. Explicit
   per-item capability overrides the kind default; non-live items cannot remix.
3. Peek additions are optional: `sku`, `design_url`, `preview_kind`
   (`printful`/`card`/`design`, plus compatible `mockup`/`lifestyle`),
   `preview_revision` (string or integer), `ready`.
   Existing `ok`, `status`, `mockup`, `is_model` remain supported. `ready:true`
   must mean the returned preview belongs to `design_url` at that revision.
   The actual `make_preview_payload` returns `preview_kind:"printful"` with
   `ready:true,is_model:false`; iOS displays that image and completes polling.
   `card`/`design` with `ready:false` remain pending. An exact server-shaped
   Printful payload regression test pins this integration contract.
   After polish the client requires the new design URL and, if previously
   known, a different revision. Legacy/unprovable old previews cannot replace
   the polished design. A design-only response is not a completed product
   preview. Unknown future preview kinds decode safely without claiming ready.
4. Price edit success should add persisted `price_jpy` and `maker_earn_jpy`.
   Otherwise the existing item GET must return canonical `price_jpy`; earnings
   then derive from the saved price and the known maker percentage. A failed
   canonical read reports unconfirmed, never the submitted price as saved.
5. On transition to `live`, the client uses the existing
   `/api/shop/checkout?sku=...` route when the initial review response lacked a
   checkout URL. Server checkout must continue to enforce actual eligibility.

## Verification (2026-09-14)

From `ios/`:

```sh
python3 verify_make_contract.py
swift test --jobs 2
xcrun swiftc -typecheck -target arm64-apple-ios17.0 \
  -sdk "$(xcrun --sdk iphoneos --show-sdk-path)" -module-name MU \
  MU/Sources/Core/*.swift MU/Sources/Features/*/*.swift MU/Sources/MUApp.swift
plutil -lint MU/Resources/ja.lproj/Localizable.strings MU/Resources/en.lproj/Localizable.strings
git diff --check -- .
```

- PASS: 30 XCTest tests, zero failures, including 86-kind decoding/search,
  fallback prices, stale operation rejection, captured intents, polish revision
  validation, retained score, status transitions and canonical price HTTP tests.
  Integration-review regressions cover exact server Printful/card/design payloads,
  null-price Contrado exclusion, optional `can_make` decoding and denial overriding
  a historical price. Tests and iPhoneOS typecheck rerun after these corrections.
  The JP audit test passes 85 physical IDs/base prices through the actual iOS
  `makeKinds(session:)` API decoder using intercepted URLSession requests:
  76 enabled / 9 disabled, exact blocked IDs, localized server reasons,
  country/temporary/preflight metadata, fallback exclusion, retained catalog
   on failure and updated labels/prices/restock on refresh. The shared fixture
   `MUTests/Fixtures/make-kinds.json` is the actual local `make_kinds()` handler
   response, with only JSON whitespace reformatted. The reconstructed Swift
   fixture has been removed. Swift loads the versioned JSON via `Bundle.module`
   (package) or `Bundle(for: ModelsTests.self)` (Xcode test resources).
- PASS: `make_kinds_tests::make_kinds_matches_ios_contract_fixture` compares the
  complete current handler JSON against that same file, including every label,
  price, capability, country restriction and reason. `verify_make_contract.py`
  extracts the production handler, its actual helpers/constants and the same
  Rust test verbatim into a lightweight crate, then runs Cargo offline. Only the
  manifest-relative fixture path is rebased to the real worktree. Axum's real
  response/serialization is used; no server main, DB, listener or cron runs.
- For the full server crate on m5, the same test can be run with:
  `cargo test --manifest-path store/Cargo.toml --bin mu-store catalog::make_kinds_tests::make_kinds_matches_ios_contract_fixture -- --exact`
  (full-server compilation was not performed in this pass).
- Fixture regeneration is explicit: set `MU_CONTRACT_FIXTURE_OUTPUT` to a new
  file inside the approved temp directory and run `python3 verify_make_contract.py`.
  Export mode creates a new file only, then returns; ordinary test runs do not
  write fixture files and always compare. Review the exported response, apply
  it to the versioned fixture with `apply_patch`, unset the variable and rerun
  both Rust and Swift tests. Do not reconstruct individual fields manually.
- PASS: all app Swift sources typechecked against the iPhoneOS SDK / iOS 17.
- PASS: both localization files and whitespace checks.
- 2026-09-14 correction: this Mac is the M5 Max / 128GB (user confirmation and
  system_profiler). Local full Xcode Debug iOS build with CODE_SIGNING_ALLOWED=NO
  passed. Xcode iPhone 17 Pro / iOS 26.5 simulator tests: 30 passed, 0 failed,
  verified via xcresulttool summary in `../mu-ios-simulator-tests.xcresult`
  (located beside the worktree in the approved temp directory).
- Widget CFBundleVersion now uses CURRENT_PROJECT_VERSION rather than fixed 6;
  the full build was rerun successfully without the parent/extension mismatch warning.
- Physical signing resolved using existing Fastfile ASC API credentials passed to
  xcodebuild -allowProvisioningUpdates with authenticationKeyID/IssuerID/Path.
  Signed app installed and launched on iPhone 16 Pro / iOS 26.5.2.
- Physical tests revealed fixed-format date parsing depended on device settings;
  FeedProduct.createdDate now explicitly uses en_US_POSIX/Gregorian. Re-run:
  30 unit tests + 2 ProductionSmokeTests = 32 passed, 0 failures, verified with
  xcresulttool on `mu-device-smoke.xcresult` beside the worktree.
- XCUITest verifies Make picker search/mug selection and live Shop product detail,
  purchase-button presence and checkout URL/SKU identity, without tapping checkout.
  Seven screenshots exported to `mu-device-smoke-attachments/` beside worktree.
  Visual inspection unavailable to this text-only model. Production kinds API
  remains 404: device picker ran fallback data; expanded 85-kind production UI
  requires backend deployment. No generation, checkout session or orders created.
- Changes are under `ios/` plus the explicitly authorized focused test in
  `store/src/catalog.rs::make_kinds_tests`; no commit or push was performed.
