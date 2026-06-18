# MU Protocol Design (Draft)

**Status**: DRAFT — formal RFC after B (multi-city) validates.

## Goal

Open up MU's engine so anyone can run a satellite city, while origin (Enabler Inc.) retains a soft anchor via:
- ENAI as settlement layer
- 5% protocol fee on every drop sold by any satellite
- Reference implementation + governance via origin

## Layers

```
┌─────────────────────────────────────────────────────┐
│ City Operator UI (per city)                         │  app layer
├─────────────────────────────────────────────────────┤
│ mu-engine crate (Rust)                              │  shared engine
│  - drop_generator (Gemini Image Pro)                │
│  - weather_provider (trait)                         │
│  - inventory + pricing + auction                    │
│  - storefront (axum)                                │
├─────────────────────────────────────────────────────┤
│ mu-protocol contract (Solana / Anchor program)      │  on-chain
│  - register_city(slug, operator_pubkey)             │
│  - record_sale(city_slug, amount_lamports)          │
│  - distribute_fee(origin 5% / city 95%)             │
│  - retire_piece(piece_hash)  [for "death-defined"]  │
└─────────────────────────────────────────────────────┘
```

## Repo restructure

```
mu-brand/
├── apps/
│   ├── origin/         # 既存 store/ をここに
│   ├── honolulu/       # 衛星 1 (Hawaii)
│   └── berlin/         # 衛星 2 (将来)
├── crates/
│   ├── mu-engine/      # 共通エンジン (今の store/src/main.rs から抽出)
│   ├── mu-weather/     # weather provider abstraction
│   └── mu-settlement/  # ENAI Treasury / on-chain RPC
├── contracts/
│   └── mu-protocol/    # Anchor program
└── docs/
```

## Fork rules

- ライセンス: コード MIT、art CC0
- "MU" 名称使用: origin (Enabler Inc.) からの承認 1 回 (geographic non-overlap)
- 5% origin fee は protocol contract 経由で自動 settlement (回避不可)
- 各衛星は自分の treasury と independent operator key を持つ
- origin は技術 governance のみ。営業介入なし

## Trigger

D を実行するための前提条件 (全部満たすまで作業しない):

1. B (multi-city) で Teshikaga + Honolulu の 2 都市が **30 日 stable**
2. F (死を持つ服) が **MA piece の 5% 以上で retire 達成** (= 仕組みが受容されている証)
3. ENAI Treasury から 各衛星都市への USDC 支払いが **手動でも実行可能** (M3 / M4 完了)

## Open questions

- 衛星都市の経営者選定 (公募? 招待制?)
- 5% origin fee の使途 (新衛星 onboarding 補助 / origin infra 維持)
- 紛争解決 (衛星が "MU" 名称を悪用したら)
- カラーと voice の guideline 強制度 (origin が strict vs loose)
