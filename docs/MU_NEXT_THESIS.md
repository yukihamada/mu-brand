# MU Next Thesis

**Author**: Yuki Hamada × Claude Opus 4.7
**Date**: 2026-05-12
**Status**: Adopted as next-phase strategic anchor

## One-Line Thesis

> **MU is a wearable timestamp. Anyone can install a city. The brand is a protocol, not a company.**

## What Shifted

| Before | After |
|--------|-------|
| "AI が art を作り、人がそれを着る" | "AI が moment を捕まえ、人がそれを wear する" |
| ファッションブランド | 時間/場所を身に着ける普遍行為のためのインターフェース |
| 単一都市 (Teshikaga) | 都市のメッシュ。各都市が衛星 |
| 中央運営の company | プロトコル。フォーク可能、ENAI で settlement |

Lululemon が yoga / Patagonia が environmental activism / Supreme が hype を発明した層に MU を置く。それは「AI fashion」というカテゴリ論争を抜けて、**universal interface** に降りる必要がある。

## なぜ "wearable timestamp" が天才かつ受け入れられるか

1. **普遍言語**: 時間と気象を理解しない人類はいない。JP / EN / 何語でも 1 秒で伝わる
2. **可視可能**: 着た瞬間に "I wore the 2026.05.12 piece" という事実が刻まれる。これは物理的な provenance
3. **non-fashion-y な人にも刺さる**: 「データを身に着ける」は研究者・エンジニア・科学者・ミニマリストに直接刺さる
4. **fast fashion の完全反転**: fast fashion は時間を消す。MU は時間を持つ
5. **AI の出口として最も筋がいい**: AI は moment を捕まえるのが上手い。これを服にすると、AI = 時計屋 という新しい職能定義になる

## 7 つの動き (詳細は MU_NEXT_7_MOVES.md)

| # | 動き | 状態 |
|---|------|------|
| A | データ可視化 (シャツに timestamp/temp を visible に) | 着手 — `gemini.rs` の prompt に `wear_log_overlay` 追加 |
| B | Multi-city 衛星化 (Honolulu / Berlin / etc) | 着手 — `cities` テーブル + 地理 abstract layer |
| C | MA 物理 hook (温度応答紙 / 当地の土 / 録音 QR) | 計画 — `docs/MU_MA_PHYSICAL_HOOK.md` で supplier shortlist |
| D | Brand-as-protocol (mu-engine OSS + 連合 settlement) | 計画 — `docs/MU_PROTOCOL.md` で設計 |
| E | Anonymous Wearing Log + 反インフルエンサー宣言 | 着手 — `/wearing` ページ + 投稿 form |
| F | 死を持つ服 (期限付きの drop と ENAI refund) | 着手 — `expires_at` column + retirement flow |
| G | 弟子屈 Residency (autonomous brand 聖地化) | 計画 — `docs/MU_RESIDENCY.md` で年 4 回 |

## 推し順序

```
今すぐ shippable:        A, E, F, B-skeleton  ← 今日のコミットで全部入る
3 ヶ月以内:              C (1 種類だけ), B-full (1 衛星都市 = Honolulu)
6 ヶ月以内:              D (protocol 公開), F (MA 全件)
12 ヶ月以内:             G (residency 第 1 回)
```

## "受け入れられる" を測る指標

- /wearing log の自発投稿数 / 月 → 「ブランドに参加してる」体験の浸透度
- multi-city instance 数 → 衛星化の物理的進捗
- ENAI を介した protocol fee 流量 → composable layer の動作
- MA piece の retirement 率 → "死を持つ服" の物語が受け入れられているか
- 検索クエリ "wearable timestamp" の月次トレンド → カテゴリ言語化が刺さってるか

## 失敗 (= 撤回ライン)

以下が 6 ヶ月続いたら thesis を見直す:

- multi-city が Honolulu 以外に広がらない (= 地理的拡張性なし)
- /wearing log 投稿が月 3 件未満 (= 参加体験が嘘)
- MA retirement 率 < 5% (= 死の物語が物理にならない)
- AI fashion 文脈以外で語られない (= timestamp 普遍化失敗)
