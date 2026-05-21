# JiuFlow メール — /100 チャレンジ告知 (draft)

**送信先**: JiuFlow active sub 161 名 (`tier IN ('founder', 'pro', 'black-belt')`)
**送信元**: `info@enablerdao.com` via Resend
**送信タイミング**: dry_run → 人間 OK → 本送信（[[feedback-email-blast-radius]] 準拠）
**utm**: `?utm_source=jiuflow_email&utm_medium=email&utm_campaign=mu100_d1`

---

## 件名（A/B 候補）

- A: `MU の 14 日 — 100 枚 売れたら 「AI が ブランドを動かす」 が 証明される`
- B: `あなたが いた から、 MU は 14 日 で 100 枚 を 試す`
- C: `[MU] 公開チャレンジ: 14 日 で 100 枚。 数字 は 隠さない。`

→ **推奨: A**（"AI が動かす" の証明という建付けが JiuFlow ユーザーの試合 narrative と噛む）

## プリヘッダー

> sold: 0 / 100 ・ 残り 10 日 ・ 達成しても 失敗しても 全部 公開する

## 本文

```text
JiuFlow を 使ってくれている あなたへ。

MU は 「人間 が 一切 触らない アパレル ブランド」 です。
AI が 毎時 1 枚 デザインを 生成し、 北海道 の 天気 で 在庫 を 決め、
売れた 数 だけ 価格 が 上がります。 すべての 数字 を /transparency で 公開しています。

そして 今日、 公開チャレンジ を 始めました。

  2026.05.18 - 05.31 の 14 日間 で 100 枚 売る。
  https://wearmu.com/100

達成しても、 失敗しても、 全部 X (@yukihamada) で 公開します。
これは AI が 「需要」 を 動かせる か どうか の 最初 の 試合 です。

1 枚 = ¥4,900 (国内 SUZURI) / ¥7,800 (海外 Printful EU)。
あなたが もう 1 枚 持っている のなら、 まだ 試合 は 始まっていない 友人 に 渡してください。

ありがとうございます。

— MU (yukihamada が 運用)

▸ https://wearmu.com/100  · 14 日 ライブ 進捗
▸ https://wearmu.com/transparency  · 全 数字 公開
▸ X @yukihamada  · 毎日 21:00 進捗 thread
```

## 配信ロジック

```bash
# 1. dry_run（送信せず 件数 / preview だけ）
curl -X POST https://wearmu.com/admin/email/jiuflow_100_d1 \
  -H "x-admin-token: $ADMIN_TOKEN" \
  -d 'dry_run=true' | jq '.'

# 2. 人間 OK 後の本送信（segment は active のみ、 過去 30 日 unsub を除く）
curl -X POST https://wearmu.com/admin/email/jiuflow_100_d1 \
  -H "x-admin-token: $ADMIN_TOKEN" \
  -d 'dry_run=false&segment=active_no_unsub'
```

## 計測

- `mu_purchases.utm_campaign = 'mu100_d1'` で attribution
- click-thru → Stripe Checkout metadata に utm 5要素を確実に渡す（[[jiuflow-ads-cvr-findings]] 教訓）
- 24h 後 open rate / click rate / conv を Slack #mu-100 に自動 post
