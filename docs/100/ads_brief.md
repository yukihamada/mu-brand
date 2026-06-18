# Ads brief — /100 チャレンジ

**予算**: ¥30,000 (Google Ads + X Ads 半々)
**期間**: 2026-05-21 — 2026-05-31 (10 日)
**目標**: 30 / 100 枚 (会場 50, メール/X 20 と 合わせて 100)
**utm**: `?utm_source={google,x}&utm_medium=cpc&utm_campaign=mu100`

---

## 教訓 ([[jiuflow-ads-cvr-findings]])

¥42K で 0 conv の 失敗を 繰り返さない。

1. **CTA を /100 (LP) に 全部 集中**。トップに 飛ばさない。
2. **Stripe Checkout metadata に utm 5 要素 を 必ず 渡す**（attribution 切れ防止）
3. **3 日連続 0 conv なら PAUSED**（焼きすぎ ガード）
4. **CVR 1% 切ったら ad copy / LP を 即 検証** （広告の問題か LP の問題か 切り分け）

---

## Google Ads (Search) — ¥15K

### Campaign

- 名前: `MU100_search_2026_05`
- 予算: ¥1,500 / day × 10 日
- bid: tCPA ¥2,000 (1 conv = 1 枚販売を想定)
- 言語: 日本語のみ
- 地域: 日本全国

### Ad group: 「AI ブランド 公開チャレンジ」

#### キーワード (broad → phrase)

- `"AI が運営するブランド"`
- `"公開チャレンジ T シャツ"`
- `"build in public アパレル"`
- `"AI ブランド 透明"`
- `[ai apparel transparency]` (exact)
- 除外: `"無料"`, `"中古"`, `"求人"`

#### 広告コピー (3 variant)

**A:**
> AI が運営するブランド MU
> 14 日 で 100 枚 売る 公開チャレンジ
> 数字 は 隠さない。 残り {{days_left}} 日。

**B:**
> 人間 が 1 度も 触らない アパレル
> AI 毎時 デザイン生成 → 売上 全公開
> 14 日 100 枚 チャレンジ 開催中

**C:**
> ¥4,900 / 1 着
> AI が 14 日 で 100 枚 売る 試合
> 達成 or 全部 公開。 wearmu.com/100

### LP

- **DO**: `https://wearmu.com/100?utm_source=google&utm_medium=cpc&utm_campaign=mu100`
- **DO NOT**: `https://wearmu.com/` (トップ flat へ送らない)

---

## X Ads (Promoted Posts) — ¥15K

### Campaign

- 名前: `MU100_x_2026_05`
- 予算: ¥1,500 / day × 10 日
- objective: Website Clicks
- 地域: 日本 + ハワイ (Yuki の二拠点)
- audience:
  - Follower lookalike (@yukihamada, @JiuFlowApp の followers)
  - Interest: アパレル / 柔術 / AI / Build in Public

### Promoted Tweet (Day 0 thread の 1/6 を そのまま promote)

```
MU は AI が運営するアパレルブランドです。
人間は 1 度も デザインに 触りません。

今日から 14 日間、 100 枚 売る チャレンジを 始めます。
失敗しても 全部 公開します。

https://wearmu.com/100?utm_source=x&utm_medium=cpc&utm_campaign=mu100
```

---

## 計測 & ガードレール

### Daily check (cron 22:00 JST)

```bash
python3 scripts/ads_health_check.py --campaign mu100 --threshold-cvr 0.5
```

- CVR < 0.5% かつ spend > ¥3K → Telegram alert (@yukihamada_ai_bot)
- 3 日連続 0 conv → 自動 PAUSED

### Stripe metadata 連携 確認

```bash
# テスト購入 1 件で 全 utm が 渡っているか確認
curl https://wearmu.com/api/admin/stripe/last-checkout | jq '.metadata' | grep utm
```

期待:
```json
{
  "utm_source": "google",
  "utm_medium": "cpc",
  "utm_campaign": "mu100",
  "utm_content": "ad-a",
  "gclid": "..."
}
```

### Attribution dashboard

- `/admin/ads/mu100` で daily spend / clicks / CVR / sold 件数 を表示
- ¥30K 全消化 後 ROAS を blog にまとめる
