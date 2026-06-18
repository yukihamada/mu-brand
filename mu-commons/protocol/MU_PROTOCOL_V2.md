# MU Protocol v2 — Universal Autonomous Brand Protocol

**Status**: RFC v2.0
**Date**: 2026-05-18
**Authors**: Yuki Hamada × Claude Opus 4.7
**Supersedes**: `MU_PROTOCOL.md` (v1 — apparel + cities only)

---

## One-line spec

> **MU Protocol turns any time-and-place-stamped, autonomously-operated product into a forkable, settlement-bound brand.** Apparel was the first reference implementation. The protocol is industry-agnostic.

---

## Why generalize (v1 → v2)

v1 of `MU_PROTOCOL.md` scoped the protocol to **apparel + cities**:
- `drop_generator` assumed T-shirts and image generation
- `weather_provider` assumed climate-pinned designs
- "Satellite city" framing assumed geographic operators

But the core insight — *autonomous brand operation tied to a moment* — applies to any industry where:

| Industry | "Release" is… | "Node" is… | "Pin" is… |
|---|---|---|---|
| **Apparel** (MU origin) | a T-shirt drop | a city | weather + AI design |
| **Food** (kokon.tokyo) | tonight's tasting menu | a restaurant | ingredients of the day |
| **Lodging** (SOLUNA / StayFlow) | a room-night | a property | season + occupancy |
| **Service / BJJ** (JiuFlow) | a tournament entry, a training plan | a dojo | athlete + division |
| **Music** | a 30s field recording | a city or venue | exact time + GPS |
| **Real estate** | a parcel allotment | a region | coordinates + cadastre |
| **Hospitality** (cafe / bar) | tonight's playlist + cocktail | a location | weather + crowd |

The same 5 primitives reappear. The protocol abstracts them so one engine serves all.

---

## Universal primitives

```
┌─────────────────────────────────────────────────────┐
│ 1. RELEASE                                          │
│    immutable time/place-pinned product event        │
├─────────────────────────────────────────────────────┤
│ 2. NODE                                             │
│    autonomous operator (city, venue, dojo, parcel)  │
├─────────────────────────────────────────────────────┤
│ 3. TREASURY                                         │
│    settlement + fee split (origin / node / pool)    │
├─────────────────────────────────────────────────────┤
│ 4. IDENTITY                                         │
│    pseudonymous participant log (wearer, diner, …)  │
├─────────────────────────────────────────────────────┤
│ 5. LIFECYCLE                                        │
│    optional expiry → retire/refund flow             │
└─────────────────────────────────────────────────────┘
```

### 1. Release — `mu.release.v2`

A `Release` is a JSON document with stable, content-addressable identity:

```jsonc
{
  "schema": "mu.release.v2",
  "id": "rel_2026-05-18T15:00Z_teshikaga_t01",   // ULID-compatible
  "node": "teshikaga",                            // node slug (FK to Node)
  "kind": "apparel.tee",                          // industry.product
  "pinned_at": "2026-05-18T15:00:00Z",            // immutable moment
  "pinned_to": {                                  // pin = the moment's signature
    "lat": 43.490, "lon": 144.460,
    "temperature_c": 14.2,
    "weather_code": "scattered_clouds"
  },
  "generator": {
    "model": "gemini-3-pro-image",
    "prompt_hash": "sha256:…",
    "output_hash": "sha256:…"                     // content address
  },
  "supply": { "kind": "open|fixed|bonding", "amount": 0 },
  "price": { "currency": "JPY", "base": 4900, "curve": "linear_micro_bond" },
  "lifecycle": { "expires_at": null },            // null = permanent
  "settlement": { "treasury": "DK29rB…",          // protocol Treasury
                  "node_split_pct": 95,
                  "origin_fee_pct": 5 },
  "embeds": ["wearmu.com/p/<id>", "<custom>"]    // public surface
}
```

Required fields: `schema`, `id`, `node`, `kind`, `pinned_at`, `settlement`.
Everything else is industry-optional.

### 2. Node — `mu.node.v2`

A `Node` is an autonomous operator registered with the protocol:

```jsonc
{
  "schema": "mu.node.v2",
  "slug": "teshikaga",                            // unique, lowercase, ≤32 chars
  "name": "Teshikaga",
  "industry": "apparel",                          // free-form, lowercase
  "operator": {
    "pubkey": "<solana_pubkey>",                  // settlement destination
    "contact": "ops@…",
    "human_in_loop": false                        // declared autonomy level
  },
  "anchor": { "lat": 43.490, "lon": 144.460 },    // optional physical anchor
  "adapter": "github.com/enabler/mu-adapter-apparel",
  "status": "active|pilot|paused",
  "registered_at": "2026-05-12T00:00:00Z",
  "origin_approval_sig": "<signature>"            // required before live
}
```

**Naming rule**: nodes that use the literal string "MU" in branding must obtain a one-time origin approval. Nodes that operate the protocol without using the MU mark are free to fork without approval.

### 3. Treasury

Settlement is industry-agnostic. Default contract (`mu-settlement`):

| Action | Effect |
|---|---|
| `register_node(slug, pubkey)` | creates node entry, emits `NodeRegistered` |
| `record_sale(release_id, gross_amount, currency)` | logs sale + splits |
| `distribute_fee(release_id)` | sends 5% origin / 95% node (configurable per node) |
| `retire(release_id, owner_sig)` | marks item retired, optional refund |

Initial reference: Solana / Anchor program at `contracts/mu-settlement/`.
Implementations on other chains (Base, Ethereum L2) are protocol-compliant if they expose the same 4 ABI calls.

**Fee floor**: 5% to origin Treasury. Nodes can raise their cut higher than 95% only by paying a one-time slot fee (unimplemented in v2 — placeholder).

### 4. Identity — `mu.identity.v2`

Participants (wearers, diners, guests, listeners) are pseudonymous by default:

```jsonc
{
  "schema": "mu.identity.v2",
  "participant_id": "p_<base32_hash>",            // hash(email+salt)
  "kind": "wearer|diner|guest|listener|player",
  "node": "teshikaga",
  "first_seen_at": "…",
  "release_count": 7,
  "log_entries": ["log_…"]                        // see WearingLog (=ParticipationLog)
}
```

Explicit anti-pattern: **the protocol never stores identifying name/face data** by default. Reference implementations enforce this at the schema layer (no `name`, no `image_of_face`, no `email` in any public emit).

### 5. Lifecycle — `mu.lifecycle.v2`

Releases may opt into mortality:

```jsonc
{
  "schema": "mu.lifecycle.v2",
  "release_id": "rel_…",
  "expires_at": "2026-08-26T15:00:00Z",           // birth + 100d (MA convention)
  "on_expire": { "action": "retire", "refund_pct": 50 },
  "retired_at": null,
  "retired_by_participant": null
}
```

When `expires_at` passes, the protocol emits `LifecycleExpired(release_id)`. Operators may handle the event (refund issued, item returned to pool, etc).

---

## Industry Adapters

Each industry implements an `IndustryAdapter` trait against `mu-engine`:

```rust
trait IndustryAdapter: Send + Sync {
    fn kind_prefix(&self) -> &'static str;        // "apparel.", "food.", "lodging."
    fn generate(&self, pin: Pin) -> Release;       // moment → product
    fn fulfill(&self, release: &Release, buyer: ParticipantId) -> Fulfillment;
    fn validate(&self, release: &Release) -> Result<(), Error>;
}
```

Reference adapters in this repo:

| Adapter | Kind | Status | Implementation |
|---|---|---|---|
| `mu-adapter-apparel` | `apparel.tee`, `apparel.hood` | **live** | `apps/origin/` (today's MU) |
| `mu-adapter-food` | `food.menu`, `food.tasting` | reference | maps to kokon.tokyo daily menu |
| `mu-adapter-lodging` | `lodging.room_night` | reference | maps to SOLUNA / StayFlow |
| `mu-adapter-service` | `service.tournament`, `service.lesson` | reference | maps to JiuFlow |
| `mu-adapter-music` | `music.field_recording` | sketch | future |

Anyone can publish a third-party adapter. Discovery via `mu-adapter-registry.json` (a flat file in this repo, PR-curated until v3).

---

## Reference repo restructure

```
mu-brand/
├── apps/
│   ├── origin/                      # current store/, MU apparel
│   └── (third-party nodes live in their own repos)
├── crates/
│   ├── mu-engine/                   # release/node/treasury/identity/lifecycle
│   ├── mu-settlement-solana/        # Anchor program client
│   ├── mu-adapter-apparel/          # reference adapter
│   ├── mu-adapter-food/             # reference adapter (kokon.tokyo)
│   ├── mu-adapter-lodging/          # reference adapter (SOLUNA/StayFlow)
│   └── mu-adapter-service/          # reference adapter (JiuFlow)
├── contracts/
│   └── mu-settlement/               # Anchor program (Solana)
├── docs/
│   ├── MU_PROTOCOL_V2.md            # this file (canonical RFC)
│   ├── MU_PROTOCOL.md               # v1 (apparel + cities, kept for history)
│   └── ADAPTERS/                    # per-industry implementation notes
└── mu-adapter-registry.json         # third-party adapter discovery
```

v1 → v2 migration: existing apparel code in `store/` is the de-facto `apps/origin/` + `mu-adapter-apparel`. Extraction into crates happens in v2.1 (incremental, no break).

---

## Conformance levels

A node is **MU Protocol Compliant** if it:

1. emits `Release` documents with `schema: mu.release.v2`
2. registers as a `Node` with the protocol Treasury
3. routes ≥5% of gross sales to origin Treasury via `mu-settlement`
4. exposes a public read endpoint at `/.well-known/mu/releases` returning Release JSON
5. respects the identity anti-pattern (no PII in public emits)

(1)+(2)+(4) make a node **Discoverable**.
(3) makes it **Settled**.
(5) makes it **Compliant**.

Discoverable + Settled + Compliant = full conformance, eligible for inclusion in the public node directory at `wearmu.com/protocol/nodes`.

---

## What this enables

- **Cross-industry composability**: a Soluna lodging release can be paid in ENAI alongside a MU apparel drop and a JiuFlow tournament entry — same Treasury, same identity, single checkout.
- **Forkable brand operations**: anyone can stand up a `mu-adapter-<industry>` and operate a node with their own brand and treasury, paying only the 5% origin fee.
- **Provenance for any product**: the `pinned_at`/`pinned_to` + `output_hash` fields give every product a permanent, content-addressable history.
- **Industry-neutral retirement**: the lifecycle layer means "death-defined" products are not an apparel-only oddity — they work for food (expires end of night), lodging (check-out date), services (event date).

---

## Out of scope for v2

- **Governance**: who can change the spec itself. Tentatively: Enabler Inc. as origin, but a v3 BIP-style RFC process is planned.
- **Disputes**: what happens if a node mis-uses the MU name. Currently bilateral; v3 introduces on-chain arbitration.
- **Cross-chain settlement**: v2 references Solana. Base/Ethereum L2 implementations are explicitly permitted but their RFC lives in `docs/CHAIN_BRIDGES.md` (TBD).

---

## Trigger conditions (before declaring v2 final)

1. ≥1 non-apparel reference adapter shipped in this repo (food: kokon.tokyo, OR lodging: SOLUNA)
2. ≥1 third-party node successfully registers via the protocol (not Enabler-operated)
3. Origin fee distribution executes on-chain at least once for a non-origin node

Until then v2 is RFC; v1 remains canonical for the apparel/cities subset.

---

## Sign-off

This document is **MIT licensed**. Anyone may fork, extend, or implement against it without permission, provided the conformance levels above are met when using the "MU Protocol Compliant" label.

> MU は、衣服を超えて、すべての autonomous brand 運営の共通 protocol になる。
> このドキュメントは v1 の apparel-only スコープを超え、food/lodging/service/music/real-estate を等しく包含する初の試みである。
> The protocol is the brand. The brand is the protocol. The product is incidental.
