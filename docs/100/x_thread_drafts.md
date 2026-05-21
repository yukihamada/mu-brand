# X thread drafts — /100 チャレンジ

**アカウント**: @yukihamada
**頻度**: Day 0 thread (kickoff) + 毎日 21:00 JST 進捗 1 tweet
**utm**: `?utm_source=x&utm_medium=thread&utm_campaign=mu100`

---

## Day 0 (kickoff thread) — 6 tweets

### 1/6 (hook)

```
MU は AI が運営するアパレルブランドです。
人間は 1 度も デザインに 触りません。

今日から 14 日間、 100 枚 売る チャレンジを 始めます。
失敗しても 全部 公開します。

https://wearmu.com/100
```

### 2/6 (why)

```
MU は 217 デザインを 自動生成し、 累計 売上 7 枚 で 停滞中。
在庫 11,182 着、 販売 0.5 枚/週。

AI に 「需要」 は 動かせるのか？
14 日 × 100 枚 = 普段の 14 倍。
これが 試合 1 試合目。
```

### 3/6 (mechanics)

```
ルール:
・期間 2026.05.18 — 05.31 (JST)
・対象 MUGEN ライン Tシャツ のみ
・¥4,900 (国内 SUZURI) / ¥7,800 (海外 Printful EU)
・カウント mu_purchases.created_at >= 5/17 15:00 UTC

進捗 JSON: https://wearmu.com/api/100/progress
```

### 4/6 (transparency)

```
数字は 隠しません。

毎日 21:00 JST に
・売上数
・売上額
・原価
・残り日数
・最新 SKU
を このスレッドに reply します。

/transparency でも 同じ 数字が見れます。
```

### 5/6 (call)

```
1 枚 持ってる人は もう 1 枚を 友人に。
持ってない人は ここから:

https://wearmu.com/100

達成 (100枚) → AI が需要を動かせた 証明。
未達 → "なぜ売れなかったか" を 全部 公開。

どちらでも 学びは 100%。
```

### 6/6 (commitment)

```
14 日後、 達成しても しなくても、 売上 / open rate / 広告費 / 原価 / 残在庫、 全部 1 つの blog post に まとめて 公開します。

build in public の 教科書 にします。

スタート。
```

---

## 毎日 21:00 JST 進捗 reply (template)

```
Day {N} / 14
sold: {sold} / 100 ({pct}%)
残り日数: {days_left}
今日: +{delta}枚 ({yen_today})
最新 drop: MUGEN #{drop_num}

{wearmu.com/100}
```

JSON 駆動で `/api/100/progress` から自動生成。手動投稿でも OK。

---

## 注意事項

- セルフメンション skip ([[feedback-x-self-mention]])
- 顧客名・購入者 email を絶対 晒さない ([[feedback-pii-protection]])
- 顧客向けコピーで「ユーザー」→「お客様」 ([[feedback-customer-wording]])
- thread の link は すべて `?utm_source=x&utm_campaign=mu100_dN`

---

## 投稿実行

```bash
# X API (twitter_post.py に既存ロジックあり)
python3 twitter_post.py --thread docs/100/x_thread_drafts.md --day 0 --dry-run
# 確認後
python3 twitter_post.py --thread docs/100/x_thread_drafts.md --day 0
```
