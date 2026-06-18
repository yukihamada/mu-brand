---
name: mu-profit-split-28
description: MU 事業の §28 利益分配スキーム — 税引後純利益 P を 6 セグメント (寄付 50 / Yuki 10 / 株主 10 / MA 10 / Community 10 / Reserve 10) に分配。日本法令準拠。2026-05-18 制定
metadata: 
  node_type: memory
  type: project
  originSessionId: 5c2ff586-0cad-494c-9014-129a02366c71
---

# §28 MU 利益分配スキーム (2026-05-18 制定)

**Constitution §28** として制定。 §27 (寄付 50%) を 6 セグメントに拡張・統合。

## 分配比率 (税引後 当期純利益 P を 100%)

| # | セグメント | 比率 | 受益 | 法的フレーム |
|---|---|---:|---|---|
| 1 | 寄付 | 50% | 弟子屈町 (企業版ふるさと納税) | 損金算入 + 法人税特別控除 (~9割) |
| 2 | Yuki 報酬 | 10% | 濱田優貴 (代表取締役) | 定期同額給与 12 等分 (翌期支払) |
| 3 | 株主配当 | 10% | Enabler 全株主 (East Ventures 5% 含む) | 株主総会決議後配当 |
| 4 | MA ホルダー | 10% | MUGEN+stack | MUクーポン (前払式支払手段 自家型) |
| 5 | Community | 10% | コミュニティ (50% 公募 grant) | Enabler 内引当金 |
| 6 | 運転備金 | 10% | Enabler Inc. 内部留保 | 利益剰余金 (端数吸収) |

**Why**: yuki が「50% 寄付 + 10% Yuki + 10% Enabler 投資家 + 10% MA + 10% MU + 10% 運転備金」 を希望、 「opus が完璧な設計を日本法令上問題ない形で」 と依頼。 暗号資産トークン新規発行は規制リスク (改正資金決済法 + 暗号資産税制) のため当面行わず、 community segment は Enabler 内 JPY 引当金として保留。

**How to apply**:
- MU の利益分配/配当/寄付/トークン関連の議論が出たら §28 を一次ソースに
- 数値は `profit_split_breakdown()` (store/src/main.rs) が単一の真実 — 端数は reserve に寄せて P と一致を担保
- 公開 URL: https://wearmu.com/profit-split (HTML), https://wearmu.com/api/profit-split (JSON), /api/transparency 内 profit_split キー
- spec 改訂は store/static/profit_split.md (PR 必須、 §27 / §28 表記は変更しない)
- ledger テーブル (profit_split_distributions) は未実装 — 実支払開始時に追加予定 ([[soluna_tapkop]] の株主・ [[teshikaga_corporate_furusato]] の弟子屈町 9割税控除 と連携)

**コミット**: mu-brand 669a642 (2026-05-18) feat(§28): 6 セグメント実装 + Constitution amendment

**関連メモリ**: [[teshikaga_corporate_furusato]] (弟子屈町 9割税控除), [[soluna_tapkop]] (East Ventures 5%), [[wearmu_founder_relay]] (MA 入手 = MUGEN+stack or invite)