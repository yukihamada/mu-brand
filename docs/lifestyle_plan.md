# wearmu 商品写真 4 種 — 現状監査 & 生成プラン

作成: 2026-05-23 (Yuki / Claude) — **2026-05-23 訂正版**
ステータス: **判断待ち** — 実行前に Phase 0/1/2 を承認すること

## TL;DR

- 穴は **2 つ**:
  1. **POD（Printful）返り本物モック** — 327/1539 = **21%** しかない
  2. **着画（lifestyle）** — per-SKU 0、ブランド LP hero も jiuflow/kokon/roll が 404
- AI 合成モック（Gemini 製）でフォロー中だが、ユーザー定義の「モック」とは違う物。
- デザイン・印刷データ（透過 PNG）は揃ってる。

## 1. 現状（2026-05-23 計測）

`catalog_products` 1,539 件 live / 10 ブランド。
**「モック = POD に透過 PNG 投げて返ってきた商品写真」** という定義に基づき再分類:

| 種別 | カバー率 | 場所 | 評価 |
|---|---|---|---|
| **デザイン PNG（透過 / 印刷データ）** | 458 ファイル / 推定 458 デザイン | `mu-brand/designs/*.png` + Printful 側 | ◎ |
| **モック（POD 返り）** | **327 / 1,539 = 21%** | `mockup_url_external` 列、`printful-upload.s3-accelerate.amazonaws.com/...` | **✕ 大穴** |
| AI 合成モック（参考） | 1,479 / 1,539 = 96% | `mockup_main_file` 列、`mockups.wearmu.com` R2 | △ POD 代わりに使ってる |
| **着画 / lifestyle** | per-SKU **0 件**、brand LP hero **7/10** | `lifestyle.wearmu.com/<brand>/lifestyle/lifestyle_NN.png` | **✕** |

### POD モックの分布

- 両方持つ: 347 SKU
- POD のみ: 0
- AI のみ: 1,192 SKU ← この層は実物未確認の AI レンダリングで売ってる状態
- どちらも無し: 0

### 着画の穴（per ブランド）

| brand | live SKU | hero lifestyle | 状態 |
|---|---|---|---|
| bjj | 1,073 | ✅ 3 枚 | OK |
| code | 110 | ✅ 3 枚 | OK |
| coffee | 103 | ✅ 3 枚 | OK |
| zen | 85 | ✅ 3 枚 | OK |
| moon | 36 | ✅ 3 枚 | OK |
| mu | 36 | ✅ 3 枚 | OK |
| tokyo | 36 | ✅ 3 枚 | OK |
| **jiuflow** | 20 | ❌ 404 | **穴** |
| **kokon** | 20 | ❌ 404 | **穴** |
| **roll** | 20 | ❌ 404 | **穴** |

`catalog_product_extras` に CLAUDE.md 規格の `lifestyle_v<n>` ラベルは **一件も無い**（既存 91 行は全て Printful 自動取得の角度違いショット: Back/Left/Front 等）。

### 既存生成スクリプトの実態

- `scripts/bulk_lifestyle_gen.py` — Gemini 3 Pro Image ($0.04/枚 ≈ ¥6) + R2 アップロード
- **重要な不整合**: スクリプトは旧 `products` テーブル（MUGEN drops）の `lifestyle_url` を更新する。**`catalog_products` / `catalog_product_extras` は触らない**。
- 最終実行 2026-05-21 15:16 → 100 件中 OK 70 / fail 30。fail は全部 `fetch_mockup HTTP 404` で、コラボ系サンプルブランドの mockup 公開 URL が死んでいる。
- → catalog 側で動かすには **改修が必要**（書き込み先テーブル変更 + label = `lifestyle_v1`）。

## 2. 提案フェーズ

優先順位: **POD モック取得（信頼性）→ Hero lifestyle 穴埋め（CVR）→ Per-design 拡張**

### Phase 0 — データ衛生（¥0 / 30 分）

1. `catalog_product_extras` の label 規約を確定: `pod_mockup_<angle>` / `lifestyle_v<n>` / `flatlay_v<n>`
2. `bulk_lifestyle_gen.py` の対象を `catalog_products WHERE status='live'` に拡張
3. ローカル 458 デザイン PNG ↔ SKU マッピング監査スクリプト `scripts/audit_design_links.py`

### Phase 1 — POD モック取得バックフィル（¥0 / 1-2h）← **最優先**

- 対象: AI 合成のみで POD モック未取得の **1,192 SKU**
- 方法: Printful API `/mockup-generator/create-task` を全 SKU 分発注（Printful 側生成は API 無料）
- 出力: `mockup_url_external` 列 + `catalog_product_extras` に angle 4 枚保存
- 期待効果: 「AI レンダリング売り」を脱して実物 POD 写真にする → 返品リスク・誇大表示クレーム回避
- コスト: API call のみ、画像生成費 0

**実行コマンド案**:
```bash
python scripts/backfill_pod_mockups.py --limit 50 --brand bjj  # まず 50 で動作確認
python scripts/backfill_pod_mockups.py --all                    # 全 1,192 SKU
```
（スクリプトは新規。`store/src/printful.rs` の既存ロジック転用可能）

### Phase 2 — Hero lifestyle 穴埋め（¥250 / 20 分）

- 対象: jiuflow / kokon / roll の 3 ブランド
- 出力: `/<brand>/lifestyle/lifestyle_01-03.png`（既存 7 ブランドと同じ規約）
- 枚数: 3 × 3 = **9 枚**
- コスト: 9 × $0.04 ≒ ¥55 + R2 ≒ ¥250 上限

```bash
for b in jiuflow kokon roll; do
  python scripts/bulk_lifestyle_gen.py --brand $b --hero-only --count 3
done
```

### Phase 3 — Per-design lifestyle（¥720 / 1-2h）

- 対象: ブランド × デザイン番号のユニーク組（推定 120 デザイン）
- 出力: `catalog_product_extras` に `label='lifestyle_v1'` で 1 枚ずつ
- コスト: 120 × ¥6 = ¥720
- **前提**: Phase 1（POD モック）完了後、本物モックを入力として AI に渡すと品質が上がる

### Phase 4 — 全 SKU 着画（¥9,500 / 15h）— **非推奨**

`catalog_orders` が **0 件** で CVR 根拠が無い。Phase 2-3 で uplift 確認できてから判断。

## 3. ブランクの review SKU 扱い

| brand | status='review' | 着画作る？ |
|---|---|---|
| kokon | 61 | Phase 2 と同時で OK（live 20 も含む） |
| sweep | 35 | **保留** — SIIIEEP 契約待ちで非表示中（[[feedback_siiieep_kokon]]） |
| nakamura | 7 | 別案件（中村兄弟ブランド）— Phase 2 後 |

## 4. 質問（次の判断）

1. **Phase 1（POD モック取得 1,192 SKU）を今夜走らせる？** ← 一番効くはず、画像生成費 0
2. **Phase 2（lifestyle hero 9 枚, ¥250）も並行で？**
3. Phase 3 は Phase 1+2 完了後 1 週間 CVR/返品観察してから
4. **design_file 列の 60/1539 は現状維持** で確定 ✅

---

参照:
- `store/CLAUDE.md` catalog contract
- `scripts/bulk_lifestyle_gen.py` 既存スクリプト
- `logs/bulk_lifestyle.log` 最終実行 2026-05-21
