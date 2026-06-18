# MSA Pricing Proposal — ¥4,900 vs ¥9,800 vs Tiered

**Status:** Proposal (2026-05-20)
**Decision required by:** Yuki
**Trigger:** Tanaka FB #10 — 「¥4,900 は 安すぎる、 yuki の 14プロダクト の 試行錯誤 が ¥4,900? 信頼性 が 逆に 下がる」

## TL;DR の 3案

| 案 | 価格 | 特徴 | リスク | 推奨度 |
|---|---|---|---|---|
| A. Status quo | ¥4,900 | 既存 SUZURI Tシャツ価格 と 同じ、 摩擦 ゼロ | 「OSS 21リポ が ¥4,900」 で 価値 を 過小評価 される | △ |
| **B. ¥9,800 + bundle** | ¥9,800 (Tシャツ 2枚 込み) | Tanaka 推奨。 友達紹介 flywheel + 単価UP | 1枚 で 良い 人 を 取りこぼす | **◎** |
| C. Tiered | ¥4,900 / ¥9,800 / ¥29,800 | Solo / Bundle / Founder で 3段階 | 認知 複雑、 LP に 表 が 必要 | ○ |

## 案A: ¥4,900 維持

### 内容
今のまま。 SUZURI Tシャツ 1枚 = MSA メンバーシップ。

### 賛成材料
- 既存 価格表 を 触らない → /buy の DB 変更 不要
- 「Tシャツ 1枚 で codebase 全部」 の **シンプルさ** が キャッチー
- Phase 1 (LP 立ち上げ) と 整合

### 反対材料 (Tanaka)
- 「安すぎる」 = 中身 の 信頼性 が 落ちる psychological effect
- 1人 founder の 14プロダクト 試行錯誤 が ¥4,900 は 価値配分 として 歪
- 価格 と 内容 の **不一致 が 不安 を 生む**

### 採用 する なら
最初 の 100名 だけ ¥4,900 (early bird)、 101名 以降 を 値上げ する 道筋 を blog に 書く。 「First 100 lock-in」 が さらに 強力 な scarcity に なる。

---

## 案B: ¥9,800 + Tシャツ 2枚 bundle (Tanaka 推奨)

### 内容
- ¥9,800 で MUGEN Tシャツ **2枚** + MSA 永続 アクセス
- 1枚は 自分用、 1枚は **友達 ギフト or 同居人**
- ギフト Tシャツ には QR code: 受け取った 友達 が QR スキャン → wearmu /msa/from/<gifter-email>?ref=<token> 経由 で 加入 すると **両方 lifetime upgrade**

### 賛成材料
- **単価 が 2倍** に なる ので 100名 売れた 時 の MRR が ¥4,900 case の 2倍 (¥980k)
- **紹介 flywheel** が 物理的 に 動く。 Tシャツ が 歩く 広告
- 「¥9,800 出した」 という 心理的 commit が retention を 上げる
- Tanaka 持つ 紫帯 や B2B SaaS 層 は 「ちゃんと 価値 ある なら ¥9,800 出す」 層 と 推測
- MUGEN の cycle 71-90 (premium scarcity 帯) と 文脈 が 合う

### 反対材料
- 「友達 いない 人」 が 抵抗 感 を 感じる (= "誰 に 渡せば いいか 分から ない、 1枚 で 良い")
- ギフト QR の 実装 が 必要 (Phase 2 を 1-2日 延ばす)
- Stripe の bundle SKU 設定、 Printful の 2枚 出荷フロー、 在庫 管理 が 1段 複雑 化

### 採用 する なら 必要 な 実装
1. Stripe で `msa_bundle_v1` SKU を 作成 (¥9,800)
2. /buy の MSA banner を 「2枚 で ¥9,800」 メイン CTA に
3. `/buy/bundle` 専用 LP (Tシャツ ペア 表示 + ギフト 説明)
4. Stripe webhook で 注文時 に gifter token を 発行、 `gifter@email` に 「ギフト URL こちら」 メール
5. QR は Printful 印刷時 に back-side の hem に 小さく 刷る (jiufight の QR と 同じ パターン 流用)

実装 工数: 1.5-2日 (既存 jiufight QR 機構 を 流用 すれば)。

### 想定 価格 心理
- ¥4,900 — 「Tシャツ 1枚 の 値段」 (zone of negligible commitment)
- ¥9,800 — 「**ちゃんと 何 か を 買った**」 zone (= 心理的 retention up)
- ¥14,800 — 「自分 への 投資」 zone (B2B 個人 出費 上限 の 入口)

Tanaka 風 の 「年商1億 B2B SaaS 創業者」 は ¥9,800 を 「ランチ 4-5回分」 で 思考停止 で 出す 層。 ¥4,900 だと むしろ 「これ で 何 が 手に入る?」 と 詰める 心理。

---

## 案C: Tiered (Solo / Bundle / Founder)

### 内容
| Tier | 価格 | 内容 |
|---|---|---|
| Solo | ¥4,900 | Tシャツ 1枚 + MSA アクセス (lifetime not guaranteed) |
| **Bundle** | ¥9,800 | Tシャツ 2枚 (1ギフト) + MSA lifetime |
| Founder | ¥29,800 | Tシャツ 2枚 + MSA lifetime + Discord/Telegram で Yuki に 月1 30分 オンライン Q&A (最初 の 10名 限定) |

### 賛成材料
- **Solo は 価格 anchor**、 中央 値 の Bundle が 大半 売れる (心理学 的 に decoy effect が 効く)
- Founder tier は 真 の 高 LTV 顧客 を キャプチャ (10名 × ¥29,800 = ¥298k 即時 + コンサル 機会)
- 「lifetime」 を Bundle 以上 に だけ 紐付ける と、 Solo 加入者 が 「lifetime 欲しい → upgrade」 する 流れ が 自然

### 反対材料
- LP 認知 が 複雑 化、 「3つ から 選ぶ」 摩擦
- Founder の 月1 30分 を 10名 守る = 月 5時間 の Yuki 拘束 (継続性 リスク)
- Phase 1 で ローンチ し終わって すぐ tier 設計 を 入れる の は 早すぎる かも

### 採用 する なら
**Phase 2 (2026-05-31) 以降 に tier 化** が 妥当。 まず Bundle 1本 で First 100 を 売って、 反応 を 見て から Founder tier を 追加。

---

## 推奨

**案B (¥9,800 + 2枚 bundle) を Phase 1 即時 採用、 案C の Founder tier は Phase 2 で 追加**。

理由:
- Tanaka FB の 「価値 と 価格 の 不一致 が 不安 を 生む」 は 強い 仮説。 ¥4,900 維持 は その 不安 を 解消 しない。
- Bundle は **紹介 flywheel** を 物理的 に 動かす 唯一 の 案。 マーケ 工数 ゼロ で 動く バイラル が 仕込める。
- 案C は 良いが、 ローンチ 直後 に 認知 を 増やす と CVR を 落とす。 Phase 2 で 「Founder tier 限定 10名 追加 した」 を ニュース に できる。

## ロールアウト 計画 (案B 採用 前提)

| 日 | アクション |
|---|---|
| 2026-05-21 | Stripe で `msa_bundle_v1` SKU 作成 + Printful 2枚 注文フロー テスト |
| 2026-05-22 | /buy MSA banner を 「¥9,800 / 2枚 / lifetime」 に 書き換え |
| 2026-05-23 | /buy/bundle 専用 LP 作成 + ギフト QR 実装 |
| 2026-05-24 | First 5 charter member 招待 (MSA_CHARTER_OUTREACH.md 参照) |
| 2026-05-25 | X thread + ブログで 値段 改定 を 説明 (transparency) |
| 2026-05-31 | Phase 2 (trio リポ DL 開放) と 同時 に 一般公開 |

## やらない こと

- ¥4,900 と ¥9,800 を 同時 並行 で 出す (decoy 狙い) — 混乱 する、 後で 追加 した ほうが クリーン
- ¥14,800 や ¥19,800 の 中間 価格 — 心理 zone と 数字 の 切れ味 で ¥9,800 と ¥29,800 の 2点 が 鋭い
- 無料 trial の MSA — もらい得 が 強すぎる、 紙 Tシャツ の 物理 コスト が gate なので trial は 意味 なし
