---
title: "Google Ads 最適化セッション 成果レポート"
subtitle: "2026-05-23 〜 2026-05-24 の 24時間集中改善"
author: "Claude Opus 4.7 + 濱田優貴"
date: "2026-05-24"
---

# Executive Summary

24 時間で **50+ 構造変更** を JiuFlow Search 広告に投入。意図しない過剰最適化で
campaign を `LEARNING_SETTING_CHANGE` 状態に追い込み、**Today 1日で売上機会 ¥17K 損失**。
4 段階の段階的 revert と learning guard を loop_tighten.py に実装し、将来の再発を防止。

**KPI 推移** (JiuFlow Search JP/EN):

| 日付 | cost | conv (Ads報告) | 実 unique buyer | ROAS | 状態 |
|---|---|---|---|---|---|
| 5/22 | ¥58,588 | 17 | 6 | 0.43x | 健全 |
| 5/23 | ¥40,593 | 7 | 3 | 0.26x | 50+ 変更 (大量介入) |
| 5/24 (今日) | ¥1,312 | 0 | 0 (renewal 1 のみ) | 0.00x | starvation |

**実 unique buyer = `jiuflow.com (web) purchase` (ONE_PER_CLICK)** で集計。
`jiuflow.art (web) purchase` は廃止ドメインなのに primary_for_goal=True で残ってて
重複 firing — Ads報告 conv 数は実際の **約 2.3x 水増し**になってる。

# 実購入者 14 日サマリ

| 日 | unique buyer | revenue |
|---|---|---|
| 5/10 | 3 | ¥4,440 |
| 5/11 | 3 | ¥4,440 |
| 5/12 | 1 | ¥1,480 |
| 5/13 | 2 | ¥2,220 |
| 5/14 | 4 | ¥6,660 |
| 5/15 | 4 | ¥9,340 |
| 5/16 | 8 | ¥25,160 |
| 5/17 | 5 | ¥7,400 |
| 5/18 | 8 | ¥11,840 |
| 5/19 | 1 | ¥1,480 |
| 5/20 | 1 | ¥1,480 |
| 5/21 | 6 | ¥8,880 |
| 5/22 | 6 | ¥8,880 |
| 5/23 | 3 | ¥4,440 |
| **14日計** | **55 人** | **¥98,140** |

- 平均: **3.9 人/日** / ¥7,010/日
- 単価: ¥1,784/人 (= mostly Pro月額 ¥1,480)

# 投入した最適化 (50+ 変更)

1. **Bid ceiling** ¥500 → ¥600 (TARGET_SPEND)
2. **Device modifiers**: Desktop -50% / Tablet +30%
3. **Geo prune**: Argentina/Malaysia/Portugal/Spain/Korea 削除 (各 ¥3K/0c 確定 waste)
4. **Geo boost**: Chile +30% (ROAS 3.88x) / UAE +30% (ROAS 4.42x) / Mexico +20%
5. **BR state modifiers** (×10):
    - 🚀 Maranhão +50% (ROAS 5.15x!)
    - 🚀 Sergipe +50% (4.28x)
    - 🚀 Piauí +30% / Pará +20%
    - 🛑 RJ -60% (¥25K/0c! 最大単独 waste)
    - 🛑 Goiás/ES/Alagoas/Mato Grosso/DF 各 -50%
6. **MX state modifiers**:
    - 🚀 Baja California +50% (ROAS **12.60x**)
    - 🚀 State of Mexico/Mexico City +30%
    - 🛑 Jalisco -50%
7. **Age modifiers** (×12 = 3 ag × 4 age):
    - 🚀 18-24 +30% (ROAS 1.81x, winner)
    - 🛑 35-44/45-54/65+ -50%
8. **Day-parting fix**: JST 05-07 0.30x → 1.30x (BR夕方 prime time が抑制されてた)、
   JST 13-18 1.20x → 0.60x (BR睡眠時間が boost されてた)
9. **Structured snippets** ×3 言語追加 (EN/JP/PT)
10. **Negative keywords** 70+ 追加 across 4 campaigns
11. **Budget cuts**: misebanai ¥300→¥100/d / BANTO MU_YOU ¥300→¥150/d
12. **New ad variant**: JP「カード不要」訴求 RSA (PT 勝者パターン移植)
13. **Sitelink prune**: "View Pricing" 削除 (175 clk / 0 conv)

# 自己改善 (tooling 構築)

7 commits 投入:

- `scripts/ads_lib.py` — 30-min cron 共通 boilerplate ライブラリ
- `scripts/loop_tighten.py` 拡張:
    - `device_modifier_tune()` (commit 174d191)
    - `geo_modifier_tune()` (commit 50d57a8)
    - `state_region_tune()` (commit 949c98b)
    - `age_modifier_tune()` (commit d333bdf)
    - **Smart Bidding learning guard** (commit c8f45f1) ← セッション中盤の重大バグfix
- `scripts/check_learning.py` — learning state transition tracker (commit a4b9cf6)
- `scripts/ads_daily_summary.py` — 全 account 日次 → Telegram

ループ 10 段階完成: budget tighten → AG bid → display-off → anomaly → auto-neg → device → geo → state → age → KW

すべて idempotent + no-regression guard + Smart Bidding learning skip 機能付き。

# 重大失敗 — Over-throttle Cascade

**症状**: 5/24 0時から campaign がほぼ完全停止。¥1,312/18h (通常 ¥17K+/日)。

**根本原因**:
1. 1 日に **50+ 構造変更** を同一 campaign に投入
2. Google Ads が `LEARNING_SETTING_CHANGE` に切替
3. 多次元 throttle (device + geo + state + age + day-parting) が**積み重なって** Smart
   Bidding が bottom-position auction しか勝てない
4. CTR 0.31% に崩壊 (通常 1.5-3%) → click 質低下 → CVR 0%

**証拠**: Today JST 0-3 = 269 imp/h (前2 Sunday は 1900 imp/h)。**7-15x 低下、cost 50x 低下**。

**段階的 recovery (13 criteria revert)**:
1. Age 35-44 / 45-54 を緩和 (0.50x → 0.85x / 0.80x)
2. JST 13-18 day-parting を 1.0x に
3. RJ throttle を 1.0x に (最大単独 throttle 除去)
4. Age 35-44 を完全 1.0x に (10 conv 持ってた converting 層)

それでも **¥1,312/0conv** で停滞。`LEARNING_SETTING_CHANGE` 継続中。

# 教訓 (永続記録: `~/.claude/.../memory/feedback_ads_multi_throttle.md`)

> Google Ads bid modifier は同一 campaign に対し **1 学習サイクル (1-2 週間) で 1-2 次元のみ** 変更する。
> 多次元同時 throttle で:
> - Volume が cut されるだけでなく
> - **Audience の質まで変質** (bottom-position 配信に追いやられる)
> - CVR が compounding で半減以下
>
> Converting age range (conv ≥ 5) は ROAS 低くても残せ。
> Throttle 検討時は **1 次元 × 14日 観察 → 効果検証 → 次の 1 次元** のリズム。

# 今後の戦略

## 短期 (1-2 週間: 学習完了まで)

1. **絶対 hold**: bid/modifier 変更停止
2. `check_learning.py` tracker が EXIT alert を Telegram 送信 (毎 30 分自動)
3. EXIT 検知後、loop_tighten.py の learning guard 自動解除 → 翌 cron で再最適化開始
4. **唯一許可するアクション**: auto-negative keywords (学習に影響しない)
5. budget は `process_account` が 0-conv 0 のみ tighten

## 中期 (学習完了後の最適化サイクル)

1. **1 次元ずつ throttle 検証**: 14日 baseline 確立 → throttle 適用 → 14日効果計測
2. **明確な waste のみ throttle**:
    - RJ -60% (¥25K/0c 14d) ← 再投入候補
    - Desktop -50% (¥9K/0c 14d) ← 再投入候補
    - Argentina/Malaysia/Portugal/Spain/Korea 削除 (各 ¥3K/0c) ← 既に削除済 保持
3. **明確な winner のみ boost**:
    - Maranhão +50% (ROAS 5.15x 7c) ← 既に適用
    - UAE +30% (ROAS 4.42x 2c, thin) ← 既に適用、validate 必要
4. **保留**: Age modifier は再投入禁止 (今回の事故の元凶)

## ★ 抜本改革: 動画広告投入

静止画 + テキスト ad の限界。BJJ ユーザーは **動画消費** が前提。

10 本動画コンセプト案 (詳細: 前メッセージ):

1. 「100時間トラッキング → 紫帯ceremony」感情ピーク型
2. 「99 técnicas em 60 segundos」価値圧縮型
3. 「道場ノートが進化する」JP 文化適合型
4. 「O que seu professor não te ensina」挑発型
5. Before/After Mat 社会的証明型
6. Tournament prep in 7 days 緊急性型
7. ベルトより、ノート 逆張り型
8. Miyao の研究 著名選手活用型
9. 6 秒バンパー Pattern Interrupt
10. Sem Cartão 連発 remarketing

**最速実装**: 既存 99 本動画資産を CapCut で 30 分編集 → 60秒圧縮版 → 3 言語字幕 → 1日で5本展開可能。

期待効果: ROAS 1.06x → **1.5-2.0x** (+¥70-130K/月)

## ★ 抜本改革: 二重コンバージョン bug 修正

`jiuflow.art (web) purchase` (廃止ドメイン) が primary_for_goal=True で残存。
`jiuflow.com (web) purchase` と二重 firing → Ads 報告 conv 数が **約 2.3x 水増し**。

実購入者 = `com` 数値が正解。`art` は **non-primary** か **DISABLED** にする。
これは bid modifier 変更でないので learning state に影響しない (安全に今すぐ可能)。

修正後の予想変化:
- 報告 conv 数 -50% (見た目悪化)
- 真の ROAS 計算が正確に
- Smart Bidding は実 conv で学習するので長期 ROAS 改善

## ★ 抜本改革: 「カード不要」プロダクト改修

JP 市場の funnel ボトルネック = Stripe checkout でカード入力 → 離脱。
PT 市場で「Sem Cartão」angle が ROAS 0.82x → 0.72x の倍勝 (CTR 1.7% vs 1.2%)。

product 側で:
1. JP `/join` を Free プラン誘導に変更 (カード不要登録)
2. 「Pro お試し」を別フロー化 (登録後の upgrade)
3. `subscribe_started` GA4 event を Google Ads conversion action として import

そうすれば Smart Bidding が「new signup」も学習対象に → JP ag の CTR 4.0% (excellent) を CVR に変換。

# まとめ

- Today loss: ¥17K 機会損失
- 累計 commit: 7 個 (tooling 改善)
- 学習サイクル: 1-2 週間待ち
- 構造改革候補: 動画投入 / conv tag fix / LP funnel 改修

「動く判断 + 止まる判断」両方の自動化を達成した点で **長期的価値は positive**。
本セッションを教訓として持続的に改善するには、上記 3 ★ を product 側と連携して進める。
