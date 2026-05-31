# /100 チャレンジ — 締め (達成告知) ドラフト

**ステータス（2026-05-30 時点・`/api/100/progress` 確定値）**

| 指標 | 値 | 出典 |
|---|---|---|
| sold | **106** | `/api/100/progress` `sold` |
| target | 100 | 同 `target` |
| 達成 | ✅ 100 枚突破（残り 0） | 同 `remaining` |
| 期間 | 2026.05.18 — 05.31 (JST) | 同 `started_at` / `deadline` |
| 締切 | 2026-05-31 23:59 JST | 同 `deadline` |

> 締切前（あと約2日）に目標達成。金額・open rate・広告費・原価・残在庫の
> 「全部公開 blog」は kickoff 6/6 で約束済み → **要実数**（下記 §3）。

---

## 1. X 締めツイート（@yukihamada・単発 or 進捗スレッドへ reply）

### A — 達成の瞬間（推奨・短い）

```
達成。

MU の 14 日 100 枚チャレンジ、 締切 2 日前に 106 枚。
人間は 1 度も デザインに 触っていません。

AI が 「需要」 を 動かせるか？ → 動いた。

数字は 全部 公開します（売上 / 原価 / 広告費 / 残在庫）。
まとめ blog、 近日。

https://wearmu.com/100
```

### B — 感謝寄り

```
14 日 100 枚チャレンジ、 106 枚で 達成しました。

買ってくれた人、 友人に 渡してくれた人、 見守ってくれた人、
ありがとうございます。

失敗したら "なぜ売れなかったか" を 公開する つもりでした。
達成したので "なぜ売れたか" を 公開します。

https://wearmu.com/100
```

> 注意: セルフメンション skip ([[feedback-x-self-mention]])・購入者の実名/email を絶対出さない ([[feedback-pii-protection]])。

---

## 2. JiuFlow 締めメール（draft・**dry_run → 人間 OK → 本送信**）

**送信先**: kickoff と同じ active sub（`tier IN ('founder','pro','black-belt')`）
**送信元**: `info@enablerdao.com` via Resend
**campaign_key**: `mu100_close`（冪等・既送 skip）
**utm**: `?utm_source=jiuflow_email&utm_medium=email&utm_campaign=mu100_close`

### 件名候補

- A: `達成しました — MU の 14 日 100 枚チャレンジ`
- B: `[MU] 106 / 100。 なぜ売れたかを 公開します`

### 本文

```text
JiuFlow を 使ってくれている あなたへ。

14 日前、 MU は 「人間が 一切 触らない アパレル」 で
100 枚 売れるか を 公開で 試しました。

結果: 締切 2 日前に 106 枚。 達成です。

買ってくれた方、 友人に 渡してくれた方、 ありがとうございます。
あなたが いた から 試合に なりました。

約束どおり、 売上 / 原価 / 広告費 / 残在庫 を 1 つの記事に まとめて
全部 公開します（近日・@yukihamada と /transparency）。

これは AI が 「需要」 を 動かせる か の 最初 の 試合でした。
動いた、 が 今日 の 答えです。

ありがとうございます。

— MU (yukihamada が 運用)

▸ https://wearmu.com/100  · 最終結果
▸ https://wearmu.com/transparency  · 全数字 公開
```

> 配信ロジック・dry_run 手順は `email_jiuflow_draft.md` §配信ロジックと同一
> （`/api/v1/admin/send-survey-blast`、campaign_key を `mu100_close` に変えるだけ）。

---

## 3. 「全部公開」 blog — 要実数（公開前に埋める）

kickoff 6/6 で約束した build-in-public のまとめ。**未確認の数字を確定情報として
載せない** ([[feedback-no-unverified-public]])。公開前に下記を実データで確定：

- [ ] 売上額（円）— `mu_purchases` 集計（`/api/100/progress` には金額が無い）
- [ ] 原価（Printful/SUZURI fulfillment cost 実績）
- [ ] 広告費（今回 ¥0 の想定だが Google Ads/X 実績を確認）
- [ ] open rate / クリック（JiuFlow メール `email_send_log` + utm conv）
- [ ] 残在庫（POD なので 0 だが「在庫リスク 0」を数字で示す）
- [ ] チャネル別内訳（SUZURI 国内 ¥4,900 / Printful EU ¥7,800 の比率）
- [ ] 期間中の SKU 生成数・生成コスト（`/admin/catalog/status` `gen_spend_jpy`）

→ 数字が揃ったら `docs/100/recap_blog.md` を起こして公開。

---

## 投稿/送信は人間 OK 後（このファイルは下書きのみ）
```
