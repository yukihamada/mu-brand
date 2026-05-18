---
name: wearmu_founder_relay
description: MU 4/7 Founder Relay は 2026-05-16 廃止。 kenny 第1回贈与のみ履歴として残る。 MA は今後 MUGEN+stack 自動 unlock or yuki 個別 invite
metadata: 
  node_type: memory
  type: project
  originSessionId: 91f7de97-fd54-4ee2-bc9b-f69b57d95a8e
---

# MU 4/7 Founder Relay — 廃止 (2026-05-16)

**Status**: **DISCONTINUED**。 100日に1回ランダムで MA を贈与する機構は 2026-05-16 に終了。 ランダム抽選は MA の希少性 (100 体限定) と物語性を弱めると判断 (Opus 助言 + yuki 同意)。

## 今後の MA 入手方法

1. **MUGEN + stack 自動 unlock**: MUGEN 1 着以上 + 他 MU NFT (Constitution / MUON) を持つと自動付与
2. **yuki 個別 invite**: その時々の「MU っぽい」 振る舞いをした人に静かに発行 ([email redacted])

## 残されたもの (履歴 / 第 1 回贈与)

**Why:** 第 1 回 (2026-05-12) で [customer] に贈与された MA は honored — DB に永続記録 + blog `/blog/4-7-founder-relay-001.html` に「launch event」として残す。 既発行 action_token `1d9eb0d281e14f75a3fde43ca6b8498c` は kenny がまだ決定していない場合に備えて引き続き機能する。

**How to apply:** lottery 関連の話題が出たら "discontinued, see /ma-lottery for the notice page" を案内。 ランダム MA 配布の提案は基本断る (MUGEN+stack or invite の二択を勧める)。

## コード状態 (2026-05-16)

- `POST /api/admin/ma_lottery/draw` → 410 Gone を返す (実装は dead code として残存)
- `GET /ma-lottery` → 終了通知 + 過去履歴 + 関連リンク表示
- `GET /ma-lottery/<token>` + `POST /api/ma-lottery/<token>/decide` → kenny の既発行 token 用に live のまま
- `ma_lottery_draws` / `ma_lottery_relays` テーブル → 残す (履歴永続)
- index.html / blog: 「4/7 Relay」リンク → 「§27 Donations」に差し替え、 blog #001 ページ冒頭に廃止 banner

## 個人情報原則 (継続)

第 1 回 winner の email は admin endpoint のみで参照。 hash-derived public ID `relay:001:A3QvbgXrGHjG5544` 経由でのみ公開。 winner email を blog / explainer / Twitter に露出させない ([[feedback_pii_protection]] 準拠)。