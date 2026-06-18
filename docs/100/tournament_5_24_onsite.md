# 5/24 SUPER YAWARA SWEEP CUP — MU 会場直販 brief

**日付**: 2026-05-24 (土)
**会場**: YAWARA 道場主催 (jiuflow.com DB には 未登録 — Yuki が別ソース管理)
**ターゲット**: 50 枚 / ¥3,500 (会場特別価格)
**100 チャレンジ 寄与**: 50 / 100 ＝ **これが 大黒柱**

---

## 在庫準備（5/19 朝 までに 印刷発注）

サイズ配分（前回 SWEEP CUP の購買データ ベース）:

| サイズ | 枚数 |
|---|---:|
| S | 8 |
| M | 17 |
| L | 17 |
| XL | 8 |
| **計** | **50** |

- デザイン: MUGEN 最新 6 ドロップ から人気上位 2 枚（#274, #273）を 25 枚 ずつ
- 印刷: Gildan Heavy Cotton (国内 DTG → 価格優位)、 黒 Tee に 白 ink
- 5/21 着 → 5/22-23 品質チェック → 5/24 朝 持ち込み

## 会場オペレーション

- 物販ブース: 入口 通路 1 卓 (要 YAWARA 主催者 確認)
- 決済: Stripe Terminal (Tap to Pay iPhone) + 現金（おつり ¥3,500 ジャスト で 不要にする）
- スタッフ: Yuki ＋ 1 名（JiuFlow チームから 募集）
- POS UTM: Stripe Checkout metadata `utm_source=onsite_sweep_524&utm_campaign=mu100_d7`
- レシート: QR で `/100` の LP に飛ばす（ライブ進捗を 当事者意識 で 見せる）

## キービジュアル

- A2 ポスター 2 枚: 「14 日 で 100 枚」 + リアルタイム sold カウンタ QR
- ハンガーラック 1 列（50 枚分）+ 試着鏡

## 試合中 アナウンス

- 試合間 (3 試合に 1 回): 「MU は AI が 動かす アパレル です。 今 X で ライブ進捗 公開中。 hashtag #mu100」
- YAWARA 主催者 (粟田 or 村田) と 事前 調整 必須

## 計測

- 会場 sold → `mu_purchases.utm_source = 'onsite_sweep_524'` で 後で attribution
- 4G ホットスポット 持参（会場 Wi-Fi 不安定リスク）
- 売れ残り は 5/25 SUZURI 在庫に 戻す（廃棄ゼロ）

## ToDo

- [ ] 5/19 朝: Gildan 50 枚 印刷発注（DTG 業者: TBD — [[dtg-tshirt-vendors-jp]] 参照）
- [ ] 5/20: ポスター 印刷 (ラクスル A2 ×2)
- [ ] 5/21: 試着・サイズ確認 (Yuki 自宅)
- [ ] 5/22: スタッフ 1 名 確保
- [ ] 5/23: Stripe Terminal セットアップ、リハ
- [ ] 5/24 09:00: 設営、 試合 13:00 開始
- [ ] 5/24 終了後: 当日 売上 を X で 投稿（DAY 7 thread）
