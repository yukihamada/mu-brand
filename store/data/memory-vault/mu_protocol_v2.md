---
name: mu-protocol-v2
description: MU Protocol v2 RFC — apparel/food/lodging/service/music を1つのspecで包む universal autonomous brand protocol (2026-05-18 shipped)
metadata: 
  node_type: memory
  type: project
  originSessionId: 4b356be2-d8a9-4e9a-a2f8-7c0ca8ebf82b
---

## What

MU Protocol を v1 (apparel + cities only) から v2 (全産業対応) に拡張。仕様は MIT、wearmu.com/protocol で公開。

## 仕様の核

5 universal primitives:
1. **Release** (`mu.release.v2`) — time/place-pinned product event
2. **Node** (`mu.node.v2`) — autonomous operator (city, restaurant, dojo, parcel)
3. **Treasury** — settlement + fee split (origin 5% / node 95%)
4. **Identity** (`mu.identity.v2`) — pseudonymous participant (PII禁止)
5. **Lifecycle** (`mu.lifecycle.v2`) — optional expiry → retire/refund

各産業は `IndustryAdapter` trait を実装:
- mu-adapter-apparel (live, origin = MU)
- mu-adapter-food (reference, [partner].tokyo 候補)
- mu-adapter-lodging (reference, SOLUNA/StayFlow 候補)
- mu-adapter-service (reference, JiuFlow 候補)
- mu-adapter-music (sketch)

## Shipped (2026-05-18)

- `docs/MU_PROTOCOL_V2.md` — canonical RFC (commit 47f8d0a)
- `store/static/protocol.html` — 公開LP (commit 82a56ea, autonomous git agent経由)
- `GET /protocol` ルート (main.rs:3121 `protocol_page`)
- `GET /.well-known/mu/releases` ルート (main.rs:3128) — conformance level 4 endpoint
- `/developers` に /protocol への callout 追加

## Trigger conditions (v2 → final)

1. ≥1 non-apparel adapter shipped (food: [partner].tokyo, or lodging: SOLUNA)
2. ≥1 third-party node registers (not Enabler-operated)
3. Origin fee distribution on-chain for non-origin node

## 関連

- `docs/MU_PROTOCOL.md` (v1, kept for history)
- [[wearmu-unified-proposals]] — 既存の collab proposal 統一基盤
- [[mu-profit-split-28]] — §28 利益分配 (50% 寄付 + 各10%)
- [[product-philosophy]] — 「速く、ノイズなく」