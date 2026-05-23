# MU Brand Expansion v2 — 追加 3 brand 案

作成: 2026-05-23 (Yuki + Claude Opus 4.7)
前提: v1 で voice / ocean / lodge / octagon / founder = 計 15 brand に到達。

## 拡張軸の残り穴

v1 でカバーしきれなかった Yuki の事業領域:

| 領域 | 現状 brand | 拡張 brand 候補 |
|---|---|---|
| 音声 / AI | voice ✓ | — |
| 海 / 旅 | ocean ✓ | — |
| 田舎 / クラフト | lodge ✓ | — |
| 格闘技 | bjj / jiuflow / roll / octagon ✓ | — |
| 創業 | founder ✓ | — |
| 開発 / Tech | code ✓ | — |
| 飲食 | coffee / kokon ✓ | — |
| **メディア / news** | ❌ | **NEWS** ← news.xyz hypernews |
| **スマートホーム** | ❌ | **KAGI** ← KAGI app, 鍵, 住空間 |
| **ハードウェア** | ❌ | **CHIP** ← Koe device, ESP32, 半田付け |

## 3 brand 詳細

### 1. **MU × NEWS** 📡
- **コア**: news.xyz / hypernews, "速くノイズなく", 日刊スピード
- **トーン**: 単色 cyan + 黒、telegraph タイポ、日付スタンプ
- **ターゲット**: news.xyz 視聴者、tech journalism マニア、hacker news 読者
- **デザイン候補**:
  - "BREAKING ▮" — 緊急速報の点滅
  - "T-MINUS 0" — リアルタイム
  - "EMBARGOED ✕" — Tech ジャーナリズム
  - "NO COMMENT" — オフレコ
- **product mix**: tee, hoodie, journal, sticker

### 2. **MU × KAGI** 🔑
- **コア**: KAGI smart home app、鍵 = 信頼 = presence
- **トーン**: 黄金 + マットブラック、シリンダーロック幾何
- **ターゲット**: スマートホームユーザー、住空間 maxer、Yale/SwitchBot 系
- **デザイン候補**:
  - "鍵あり ◯" — 鍵ありの安心
  - "PRESENCE" — 在宅
  - "LOCK · UNLOCK" — 二項対立
  - "鍵束 12" — 物理的な鍵の重さ
- **product mix**: tee, cap, mug, keychain (POD digital tag?)

### 3. **MU × CHIP** ⚡ (旧 HARDWARE)
- **コア**: Koe device, ESP32, 半田、PCB、自作
- **トーン**: 緑 PCB + 銀 solder、IC pin 配列
- **ターゲット**: maker community, ESP32/Arduino 民、自作キーボード民
- **デザイン候補**:
  - "ESP32 + ❤" — ESP32 愛
  - "SOLDER ON" — 半田し続ける
  - "PCB ART" — 基板アート
  - "FW v0.1" — ファーム初版
- **product mix**: tee, hoodie, mug, sticker

## 実装プラン（perfect_pipeline.py で）

1. catalog_brands に 3 行 INSERT + config_json (design_style + lifestyle_scene + ink_default)
2. catalog_products に各 brand × 4 SKU = 12 SKU INSERT
3. perfect_pipeline.py で 12 並列実行（既存パイプライン無変更）
4. `static/<brand>/index.html` × 3 を copy + 微修正（脱ハードコード generic template）
5. main.rs に nest_service 3 行追加 + commit + push + fly deploy

総コスト見積: **¥144 / 約 3 分**（12 SKU × ¥12）

## 拡張後の総計

| 項目 | 現在 (v1 後) | v2 後 |
|---|---|---|
| 一級ブランド数 | 15 | **18** |
| 完璧化 SKU 数 | ~50 | **~62** |
| Yuki 個人事業反映度 | 90% | **100%**（残った主要事業全カバー） |

## 戦略 KPI（v1 と同じ枠組み）

- NEWS は news.xyz 視聴者に直接 push notification + tee リンク
- KAGI は KAGI iOS アプリ内に "Get the merch" バナー
- CHIP は GitHub README に sticker QR + Maker Faire 配布

---

メモ: deploy ブロック解除されてから動くので、ローカル準備だけ先に進める。
