---
name: soluna_vision
description: Soluna product vision — device/user/channel 3-layer architecture, soluna:// protocol, festival use case
type: project
---

Soluna の目指す UX (2026-03-18):

## 3層アーキテクチャ
- **デバイス** `#D2H92D` — 短いID、コネクトモードONで自動参加
- **ユーザー** `@yuki` — ユーザー追加で会話（通話）可能、@mentionで着信
- **チャンネル** `soluna` — 音楽が流れる場所

## ユースケース
- **フェス**: みんなが「soluna」チャンネルに入れば全員が同じスピーカーになる
- **家**: 一つのデバイスで流せば全デバイスが自動同期、個別設定不要
- **通話**: @username を追加すると会話開始

## プロトコル
- `soluna://channel/soluna` — HTTP のような標準プロトコル
- デバイス間接続は soluna:// で統一

**Why:** ユーザーが設定なしで繋がる体験。AirPlayのように簡単だがオープンで大規模対応。
**How to apply:** UI/UX設計時はこの3層を意識。デバイスID短縮、コネクトモード、自動同期が最優先。