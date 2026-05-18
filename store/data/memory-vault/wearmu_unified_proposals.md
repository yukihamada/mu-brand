---
name: wearmu-unified-proposals
description: wearmu.com 統一プロポーザル基盤 — 新 collab を 1 つの POST で立ち上げる DB 駆動システム (2026-05-15)
metadata: 
  node_type: memory
  type: project
  originSessionId: 705e7c8b-754e-4a04-8d13-32fe62d8e27a
---

wearmu.com の collab proposal システムは DB 駆動に統一済み (2026-05-15)。
新ブランドを立ち上げる時に Rust 側のコードを書かなくて良くなった。

**核となるテーブル**: `proposals` (slug PK + 承認状態) + `proposal_skus` (slug, letter, drop_num, price_jpy, kind, ...)

**新ブランドの立ち上げ手順**:
1. spec.json を書く (slug, name, skus 配列)
2. `MU_ADMIN_TOKEN=... ./scripts/new_proposal.sh <slug> spec.json`
3. (実際の認可) `POST /api/proposal/<slug>/approve` で active=1 に flip

**ルート (singular = 新, plural = 旧 legacy 維持)**:
- `GET  /api/proposal/:slug/state` — 公開
- `GET  /api/proposal/:slug/skus` — 公開
- `POST /api/proposal/:slug/approve` (admin)
- `POST /api/proposal/:slug/revoke` (admin)
- `POST /admin/proposal` — 新ブランド作成 (admin)
- `GET  /admin/proposals` — 全 brand リスト (admin)

**Why**: 旧パターンは 1 ブランドにつき approval table + DESIGNS const + 4〜6 ルート + embed allow-list 書き換えと、約 200 行の Rust コードが必要だった。kichinan/asoview/elsoul/ele/nojimahal/ryozo の 6 ブランドで重複。

**How to apply**: 7 ブランド目以降は `scripts/new_proposal.sh` だけで完結。Rust に手を入れる必要はない。embed_products allow-list も `SELECT slug FROM proposals` で自動。既存 6 ブランドは起動時 migration で透過的に新テーブルへ移行済み (旧ルート/旧 const は legacy として残置)。

関連: [[wearmu-proposal-workflow]] (LP 生成側のワークフロー)