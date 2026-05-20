# MUGEN #∞ — Founder Edition

> 「100年 残る もの」 と 「1ヶ月で 捨てる もの」 を 区別 する。

**価格** ¥48,000 (≈ $320) · **数量** 1着 / drop · **頻度** 年4回 (春分・夏至・秋分・冬至 21:00 JST)

LP: <https://wearmu.com/buy/founder>
Checkout API: `POST /api/checkout/founder` → Stripe Checkout Session URL

---

## 1. 設計 原則

「服 を 1着 100年 大事にする」 という 文化 を 物理化 する。
コスト は **意味あるもの だけ** に 投じる。 unboxing 動画 では 映えない けど、 30年後 も 着られる。

### 投じる コスト

| 項目 | % | 理由 |
|---|---|---|
| Loopwheel 工房 + 縫製 工賃 | 30% | 14oz 吊り編み + 完全ロック縫い = 50年 持つ 物理 base |
| 鉱物染料 + 染色 工程 | 15% | 弟子屈 川湯温泉 由来、 「土地」 narrative の 中核 |
| 100年 修繕 reserve fund | 20% | 各 1着 に エスクロー、 法人 倒産 でも 担保 |
| NFC tag + 革ラベル + 箔押し | 5% | 物理 → デジタル の anchor、 永久 read |
| on-chain mint + DAO infra | 3% | Solana mint + Chronicle ホスト 永久 |
| 配送 + 決済 fee | 7% | Stripe 3.6% + ヤマト/DHL 実費 |
| Enabler Inc. 利益 + 運営 | 20% | MU 全体 開発・人件費 へ (Yuki 個人 ではない) |

### 投じない コスト

木箱・専用包装・包装紙・ノベルティ・装飾印刷・過剰ステッチ・ブランド袋・香り・ステッカー。
**¥5,000〜¥10,000 を 1ヶ月で 捨てられる もの に 投じない**。

---

## 2. 物理 仕様

| 要素 | 仕様 |
|---|---|
| **生地** | Loopwheel 吊り編み 14 oz (~400 gsm) garment-dyed organic cotton、 和歌山 高田馬鞍機 製 |
| **縫製** | 完全ロック縫い (脇 inside-out)、 首リブ二重折り、 裾チェーンスティッチ |
| **染色** | 弟子屈町 川湯温泉 由来 鉱物染料 (黒褐色)。 配合 比率 を Chronicle で 公開 |
| **印刷** | DTG ベース、 1 着 ずつ 印刷者 サイン入り。 装飾的 gold foil/過剰ステッチ は 付けない |
| **首裏 ラベル** | 黒革 ラベル に NFC tag 埋込 + シリアル番号 (#N of ∞) 箔押し |
| **サイズ** | XS / S / M / L / XL / XXL (購入時 選択)。 Loopwheel 特性で ±2cm 個体差 |

---

## 3. デジタル / IP

- **on-chain anchor**: Solana mainnet に SHA-256 mint。 NFC スキャン で live tx sig 表示
- **MSA Lifetime**: wearmu 全 private リポ (現 21 本) + 将来 永久 アクセス。 First-100 charter 待遇
- **MU Pass NFT**: Founder 専用 trait (`is_founder_edition: true`, `mint_serial: N`)。 Magic Eden / Tensor 二次流通 可、 5% royalty
- **次回 drop の vote 権**: DAO Council seat 1票
- **Chronicle ページ**: `/c/founder/{serial}` — 製造ログ + 染料配合 + 縫製職人 + past/future owners (匿名 ID)

---

## 4. 同梱

- 本体 T シャツ 1 着
- **鉱物染料 サンプル 5g** (真空 小分け) — 自家修繕 enable する 唯一手段
- **Yuki 手書き ノート 1 枚** — シリアル #N と 通し番号 一致、 二次流通で 真贋 verify
- 配送箱: 再生段ボール 1重、 緩衝材 = 古紙
- **保証書 は 紙では 出さない** — NFC → Chronicle → 100 年保証ステータス
- (option) 手渡し: 三田 or 弟子屈 引取、 別料金 なし、 強制しない

---

## 5. Drop スケジュール

| Drop | 公開日時 (JST) | 制作完了 |
|---|---|---|
| #001 | 2026-06-21 21:00 (夏至) | 〜 2026-09-15 |
| #002 | 2026-09-23 21:00 (秋分) | 〜 2026-12-15 |
| #003 | 2026-12-22 21:00 (冬至) | 〜 2027-03-15 |
| #004 | 2027-03-20 21:00 (春分) | 〜 2027-06-15 |

100 年 続けば **400 着**。 これ が 「∞」 の 意味。

### 購入 → 出荷 フロー

1. wearmu.com/buy/founder で 予約購入 (Stripe Checkout、 ¥48,000)
2. 入金 確認 → シリアル番号 (#N) 確定
3. 次の drop 日 に 制作開始 (Loopwheel → 染色 → 縫製 → NFC ラベル)
4. ~3ヶ月 後 出荷
5. NFC スキャン で Chronicle ページ open

---

## 6. 100 年 修繕 reserve fund

各 1着 売上 の **20% (¥9,600)** を `enabler-founder-reserve` Stripe Connect 子アカウント に エスクロー。
将来 修繕 リクエスト が 入った時、 同じ 鉱物染料 配合 + 同じ Loopwheel 工房 で 修繕。

- 法人 倒産時: tsugi succession token で 後継法人 が 引継ぎ
- 後継 もない時: 鉱物染料 配合 + 縫製図 を 全 owner に 開示 (= self-修繕)

---

## 7. 既存 MUGEN との 比較

| 軸 | MUGEN (¥4,900) | Founder Edition (¥48,000) |
|---|---|---|
| 物理 品質 | Stanley/Stella 180gsm · DTG | Loopwheel 14oz · 鉱物染色 · 完全ロック縫い |
| デジタル | (Phase 2 mint) | NFC + on-chain + Founder trait + DAO seat |
| 配信 頻度 | 1着/h、 commodity | 1着/3ヶ月、 「待つ」 体験 |
| 同梱 | 本体 のみ | 本体 + 染料 5g + 手書きノート |
| 100年 narrative | "buy and forget" | 50年 持つ + 100年 修繕 reserve fund |
| 二次流通 | n/a | NFT `mint #N` + 物 セット |

---

## 8. 実装 ステータス

| 項目 | 状況 |
|---|---|
| LP `/buy/founder` (写真 5枚 + 沈黙 リライト) | ✅ ship (v2) |
| `POST /api/checkout/founder` (manual capture mode) | ✅ ship (v2) — オーソリ のみ、 課金確定 = 製造完了後 |
| Size picker (XS〜XXL) UI | ✅ ship (v2) |
| 先行予約 / 全額返金 ポリシー LP 明示 | ✅ ship (v2) |
| 特商法・事業者情報 LP 明示 | ✅ ship (v2) |
| NFC 25年 無料交換 narrative | ✅ ship (v2) — 100年 narrative の chip-vs-anchor 整合 |
| 海外 配送 = 鉱物染料 別送 注記 | ✅ ship (v2) — 国際郵便 危険物 規制 (UN3077) 配慮 |
| Drop 日 自動 計算 (春分/夏至/秋分/冬至 21:00 JST) | ✅ ship |
| Loopwheel 工房 契約 | ⏳ 6/21 前 までに 和歌山 訪問 |
| 鉱物染料 配合 確定 + 重金属 試験 | ⏳ 弟子屈町 川湯温泉 サンプル 採取 + ph + EU REACH 適合 試験 |
| NFC tag + 革ラベル 試作 | ⏳ 1 着 (Yuki 用) |
| Solana mint + DAO seat 実装 | ⏳ MU Pass v2 と 統合 |
| `/c/founder/{serial}` Chronicle | ⏳ NFC ラベル 完成 後 |
| 100年 修繕 reserve fund (信託 口座 or Stripe Connect Custom Account) | ⏳ 顧問税理士 + 顧問弁護士 相談 後 |
| 適格請求書発行事業者番号 | ⏳ 登録完了 後 LP の placeholder を 確定値 に 差し替え |
| tsugi succession token 紐付け | ⏳ tsugi v1 動作 後 |
| Phantom wallet セットアップ ガイド + メール vote 代行 mode | ⏳ Chronicle 完成 後 |

### v2 ship 内訳 (2026-05-20)

このPR で 解決 した リスク:
- 写真 ゼロ で ¥48k 表示 → 5 枚 (hero/loopwheel macro/NFC label/染料 vial/手書きノート) 統合
- live mode で 即課金 → manual capture mode (オーソリ のみ)
- 適格請求書 placeholder `T9011001129xxx` → 登録完了後 メール記載 と 明文化
- NFC 100年 narrative 矛盾 → 25年 無料 chip 交換 + serial/on-chain anchor 永久
- 海外 鉱物染料 同梱 = 国際郵便 規制 違反 リスク → 別送 / 修繕時 持参 を 明文化
- Yuki 1人 bottleneck → #001-#100 = 創業者 直接、 #101 以降 = 後継 founder 連名 と 明文化
- size 強制 M → XS/S/M/L/XL/XXL picker 追加
- 特商法 表記 不在 → 事業者情報 + リンク を LP に 設置
- refund ポリシー 弱体 → 製造開始前 = 全額返金、 開始後 = NFT 二次流通 を 明文化

---

## 9. リスク / 反論

- **¥48k 高すぎる** → MA (¥30k〜¥100k) と 並ぶ tier。 「100年」 narrative の 物理化
- **量産 不可** → 木箱 + 鉱物染料 = だから 4枚/年 だけ
- **100年保証 は 法人継続 リスク** → tsugi succession token で 既に 緩和、 self-修繕 可能 (染料配合 公開)
- **Founder Yuki が 死ぬ** → 後継法人 が 同 仕様 で 継続。 配合・工房 連絡先 全部 文書化
- **第1回 制作 が 遅延** → 全 buyer に 返金 + 待つ option を 提示。 SLA 違反 を 隠さない

---

## 10. 連絡先

- 仕様 質問 / 試作 / 工房 紹介: <founder@enablerdao.com>
- 一般 問合せ: <info@enablerdao.com>
- 修繕 リクエスト (将来): NFC スキャン → Chronicle ページ から 申請
