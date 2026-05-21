# MU Heritage Edition — Fulfillment Workflow

> 30 着 limited / made-to-order / ¥35,000 / 60-90 日 pre-order ETA
> Edition: No.001 · Created: 2026-05-21
> Spec figures (14oz / 60-90 days / ¥35,000) are **pre-supplier-confirmation
> 想定**. Update once supplier replies land.

---

## Overview

```
order(Stripe) ─▶ DB capture ─▶ wait for lot fill (Day 0-30)
                                    │
                                    ▼
                             Day 30: lock lot
                                    │
                ┌───────────────────┼───────────────────┐
                ▼                   ▼                   ▼
        和歌山 Loopwheel       弟子屈 mineral dye   兵庫 ヒラオカ縫製
        (反物 編成)              (染色 + 単洗い)        (flatlock 縫製 + 内刺繍)
                │                   │                   │
                └─────────▶ Day 60 ─▶ ─────────▶ Day 80 ─┤
                                                          ▼
                                                NFC 書込 + 検品
                                                          ▼
                                                オンコ 木箱 packing
                                                          ▼
                                                3PL ─▶ ヤマト 発送 (Day 90)
                                                          ▼
                                                顧客 受領 + NFC 紐づけ
```

---

## Step 1: Order intake (Day 0)

| 項目 | 内容 |
| --- | --- |
| Trigger | `POST /api/checkout` from `/heritage` |
| 責任者 | 自動 (store-rs / Stripe webhook) |
| 期日 | リアル タイム |
| 記録先 | `products.db` の `mu_purchases` table + Stripe Dashboard |
| 確認方法 | `/admin/orders?token=…` で 受注 確認、 hourly cron で Telegram alert |

- 顧客 は LP で 色 (BLK / NAT) を 選び、 Stripe Checkout で 決済 (`payment_method=jpy`)
- 支払 完了 で `mu_purchases.brand='heritage'` に row が 入る
- Stripe receipt email + MU の自動 ack mail (Day 0 確定通知)
- 在庫 update: `products.sold += 1`、 LP の live counter が 即時 反映

---

## Step 2: Lot 確定 (Day 30)

| 項目 | 内容 |
| --- | --- |
| Trigger | Day 30 経過 or 30 着 完売 (どちらか 早い 方) |
| 責任者 | 濱田優貴 (main operator) |
| 期日 | Day 30 23:59 JST |
| 記録先 | `docs/heritage-supplier-inquiries.md` の dispatch ログ |

- Day 30 時点 で **15 着 以上** の pre-order が 確定 して いれば 発注 GO
- 15 着 未満 の 場合: 顧客 全員 に メール で 状況 共有 + 延長 (もう 30 日) or 全額 返金 を 選択
- GO 判断 後、 3 supplier (久保田メリヤス / 弟子屈 染色 lab / ヒラオカ縫製) に
  正式 発注 メール (本文 は `heritage-supplier-inquiries.md` の draft を ベース)
- Stripe の charge は 既に capture 済 (immediate charge 方式) なので、
  キャンセル は manual refund で 対応 (Day 30 までは 全額 返金 約束)

---

## Step 3: 生地 編成 (Day 30 - Day 50)

| 項目 | 内容 |
| --- | --- |
| 担当 | 和歌山 久保田メリヤス工業 (想定) |
| 期日 | 反物 出荷 まで 20 日以内 |
| 確認方法 | 週次 メール 進捗 + 写真 1 枚 / 週 |
| 出荷先 | 弟子屈 mineral dye lab に 直送 |

- 14oz tubular knit 反物 約 50-60 m を 編成
- 染色 前 (生成 / kibata) の状態 で 出荷
- 1 反 ごと に ロット 番号 を 付与、 撮影 して MU 側 に 共有

---

## Step 4: 染色 + 単洗い (Day 50 - Day 65)

| 項目 | 内容 |
| --- | --- |
| 担当 | 弟子屈 mineral dye lab (新規 / 商工会 経由 で 紹介) |
| 期日 | 反物 受領 から 15 日 |
| 確認方法 | 染色 前後 の 反物 写真 + 色 サンプル 郵送 (1 枚 を 兵庫 縫製 + MU 東京 へ) |
| 出荷先 | 兵庫 縫製 工場 に 直送 |

- **Black ロット** (20 着分): 火山灰 + 鉄 媒染 で mineral dye 染色 → 単洗い → enzyme wash
- **Natural ロット** (10 着分): 染色 なし、 単洗い + enzyme wash のみ
- ロット ごとに 個体差 を 容認 する 旨、 LP の `/heritage` に 明記 済み
- 染料 が 残ら ない よう、 単洗い の 排水 は 地下 浸透 NG → 業者 経由 で 処理

---

## Step 5: 縫製 + 内刺繍 + 検品 (Day 65 - Day 85)

| 項目 | 内容 |
| --- | --- |
| 担当 | 兵庫 ヒラオカ縫製 (想定) |
| 期日 | 染色済 生地 受領 から 20 日 |
| 確認方法 | 縫製 開始 / 完了 で 写真 1 枚 / 週 |
| 出荷先 | 国内 3PL (下記 Step 6) |

- パターン (型紙) は 事前 に 確定: L のみ (身幅 56 / 着丈 71 / 肩幅 49 / 袖丈 22 cm 想定)
- flatlock 平縫い で 肩 / 脇 / 袖 / 裾 を 縫製
- 衿 / 肩 に 補強 テープ
- **内刺繍** で 個体 serial を 内ネック 下 に 入れる:
  - `MU-HER-001-LS-BLK-L #001` ~ `#020` (Black ロット)
  - `MU-HER-002-LS-NAT-L #001` ~ `#010` (Natural ロット)
- 内ネック は タグ なし、 silk screen で 「無 / MU · Loopwheel 14oz · mineral dye · made in JAPAN」
- 検品: 縫い目 / 染色 ムラ (想定範囲 内) / サイズ / 刺繍 番号 を チェック

---

## Step 6: NFC 書込 + 木箱 packing (Day 85 - Day 88)

| 項目 | 内容 |
| --- | --- |
| 担当 | MU 直営 (東京 or 弟子屈 拠点 で 手作業) |
| 期日 | 縫製 完了 から 3 日以内 |
| 確認方法 | 各 個体 の NFC ID を `mu_nft` table に 紐付け、 OK で next |

- 1 着 ごとに NFC タグ (NTAG215 想定) を 裾 内側 に 縫い込み (or 内ポケット 想定)
- NFC URL: `https://wearmu.com/heritage/n/<nfc_id>` (要 route 追加 — Phase 2)
- DB: `mu_nft` table に `product_id`, `serial_code`, `nfc_id`, `manufactured_at` を 紐付け
- 北海道 オンコ 木箱 (1 着 用、 250mm × 350mm × 80mm 想定) に
  - Tee を 折りたたまず ロール で 収納
  - 100 年 修繕券 (紙、 名義 NFC 連動) を 同梱
  - 内 ステッカー で edition / serial / 染色 ロット 番号 を 明記

---

## Step 7: 3PL ─▶ 顧客 発送 (Day 88 - Day 90)

| 項目 | 内容 |
| --- | --- |
| 担当 | 国内 3PL 候補: オープンロジ (OPENLOGI) / Logiq / ロジクラ 等 (推定 — 要確認) |
| 期日 | NFC 書込 完了 から 2 日以内 |
| 確認方法 | ヤマト 追跡番号 を MU から 顧客 に メール 通知 |

- ヤマト 元払い (送料 込み)、 配達 日 指定 可
- 包装: オンコ 木箱 → クッション → ヤマト 箱
- 海外 注文 が 入った 場合: EMS or DHL (送料 別 で 後請求 想定 — Phase 2)
- 配達 完了 後、 MU から 「NFC で 個体 ID を 登録 ください」 の フォロー メール

---

## Step 8: 顧客 受領 + 100 年 修繕券 紐付け (Day 90+)

| 項目 | 内容 |
| --- | --- |
| 担当 | 顧客 + MU support |
| 期日 | 顧客 ペース |
| 確認方法 | NFC スキャン で `mu_nft` table に `claimed_at` 記録 |

- 顧客 が NFC を スキャン → `mu_nft.owner_token` を 顧客 メール に 紐付け
- 100 年 修繕券 が active 化 (修繕 要請 は `/heritage/repair?serial=…` から 受付 — Phase 2)
- 譲渡 時は NFC の `owner_token` を 新 オーナー に 切り替え (Phase 2 で UI 実装)

---

## 3PL 候補 (推定 — 要 個別 確認)

| 候補 | 拠点 | 強み | 留意点 |
| --- | --- | --- | --- |
| オープンロジ (OPENLOGI) | 千葉 / 関東 | API 連携 / 小ロット 対応 | 1 着 単位 出庫 の 単価 |
| Logiq | 関東 | EC 連携 多 | 木箱 packing の 受託 可否 要確認 |
| 自社 倉庫 (東京 三田) | 港区 | 完全 手作業 / 個体 管理 | 30 着 規模 ならば 自社 で 十分 |

**初回 ロット (30 着) は 自社 倉庫 で 手作業 packing を 推奨**。 100 年 残す
プロダクト の 1 ロット 目 で、 個体 ごとに NFC + 木箱 + 修繕券 を 紐付ける
手間 を 外部 3PL に 完全 委託 する のは リスク が 高い。

---

## 寄付 配分 ledger (Day 90+)

| 配分先 | 比率 | 金額 (¥35,000 × 30) | 振込 期日 | 公開 先 |
| --- | --- | --- | --- | --- |
| 弟子屈町 (企業版 ふるさと納税) | 35% | ¥367,500 | Day 120 | `/profit-split` + Notion ledger |
| 気候 reserve | 10% | ¥105,000 | Day 120 | `/profit-split` + Notion ledger |
| 運営 (NFC / 修繕 fund) | 5% | ¥52,500 | Day 120 | 内部 帳簿 |
| 原価 + 物流 | 50% | ¥525,000 | 各 supplier 締日 | 帳簿 のみ |
| **合計** | **100%** | **¥1,050,000** | | |

- 弟子屈町 への 企業版 ふるさと納税 は MEMORY `teshikaga_corporate_furusato.md` を 参照、
  最大 9 割 税軽減 (損金 + 税額控除) 想定
- 寄付 振込 完了 後、 `/profit-split` の Heritage line セクション (Phase 2 で 追加)
  に 帳簿 を 公開

---

## エラー シナリオ + 対処

| シナリオ | 対処 |
| --- | --- |
| Day 30 で 15 着 未満 | 顧客 メール → 30 日 延長 or 全額 返金 を 選択 |
| Loopwheel 反物 編成 失敗 (色 / 厚み NG) | 在庫 反物 から 代替 supplier (FreshService 系) に 切り替え、 顧客 に 通知 |
| 染色 ロット 全数 NG | 弟子屈 を skip、 別 染色 工房 で 再染色 (ETA +20 日)、 顧客 に 延期 通知 |
| 縫製 ロット 部分 NG | 不良 個体 のみ 再縫製、 残り は 予定 通り 発送 |
| 顧客 が キャンセル 要求 (Day 30 後) | 製造 進行 中 ゆえ NG、 但し 受領 後 30 日 以内 の 返品 は 受付 (全額 返金) |
| NFC タグ 書込 失敗 | 該当 個体 は 出荷 保留、 NFC タグ 交換 後 出荷 |

---

## 関連 docs

- `/docs/heritage-supplier-inquiries.md` — 3 supplier draft email
- `/store/static/heritage.html` — 公開 LP
- `/store/products.db` — SKU rows (id=294 BLK / id=295 NAT)
- `/.well-known/mu/releases` — 公開 release feed (heritage は 次回 release で 追加)
- MEMORY: `teshikaga_corporate_furusato.md`, `soluna_tapkop.md`, `mu_profit_split_28.md`
