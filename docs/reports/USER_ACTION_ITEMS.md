# Google Ads セッション後 — ユーザー手動対応 タスク

私 (Claude) が API 経由で適用できなかった項目。**優先度順**に列挙。

---

## 🔴 P0: 緊急

### 1. Stripe Live Secret Key revoke + rotate

**理由**: 24h前のセッションで Stripe live secret key (`sk_live_...`) が chat 出力に露出。
git/scripts には保存していないが chat 履歴に残っている。

**手順**:
1. https://dashboard.stripe.com/apikeys にアクセス
2. 該当の secret key (末尾 `iHXr`) の "Roll" ボタンクリック
3. 新しい key を Fly secret に設定:
   ```bash
   fly secrets set STRIPE_SECRET_KEY=sk_live_<new> -a jiuflow-ssr
   ```
4. デプロイ完了確認 (`fly logs -a jiuflow-ssr`)

---

## 🟡 P1: 重要 — 報告 conv 数の水増し解消

### 2. 重複 Conversion Action を Secondary 化

**現状**: `jiuflow.com (web) purchase` と `jiuflow.art (web) purchase` 両方が
`primary_for_goal=True`。同じ購入で2回 firing → Ads 報告 conv が **約 2.3x 水増し**。

**API 経由不可** (GA4 import / WEBPAGE_CODELESS は immutable)。Google Ads UI で:

1. https://ads.google.com → Tools & Settings → Conversions
2. 以下 2 件を **Secondary に変更** (チェックボックス):
    - **'購入'** (WEBPAGE_CODELESS, id 7414561612) — 古い generic action
    - **'jiuflow.art (web) purchase'** (GA4 import, id 7415153403) — 廃止ドメイン
3. **'jiuflow.com (web) purchase'** (id 7601484470) のみ Primary のまま

**効果**:
- 報告 conv 数 -50% (見た目悪化、でも実態を反映)
- Smart Bidding が実 conv で学習 → 長期 ROAS 改善
- Learning state には影響しない (conv action 変更は OK)

---

## 🟢 P2: 戦略実行 — 動画 ad 投入

### 3. 60 秒圧縮動画の作成 + Google Ads 投入

**最速 path** (1 日で 5 本展開):

a. **素材**: JiuFlow 既存 99 本技術動画 (Cloudflare Stream URL)
b. **ツール**: CapCut Desktop (Mac版、無料)
c. **編集**: 99 本 × 0.6 秒 = 59 秒 + 1 秒テロップ
d. **テロップ**:
    - PT: "Tudo isso. Grátis 7 dias, sem cartão." → wearmu.com の Sem Cartão pattern 移植
    - EN: "All this. 7 days free, no card needed."
    - JP: "技術99本。7日間無料、カード不要。"
e. **書き出し**: 3 言語 × 9:16 (Shorts) + 16:9 (Pre-roll) = 6 本
f. **アップロード**:
    - YouTube: 各 言語の channel に Unlisted で upload
    - Google Ads → Asset library → Video assets で URL 紐付け
g. **Campaign**: 既存 JF Search に Video asset 追加 (Display ext) OR 新規 YouTube campaign

詳細台本は前回送ったメッセージ参照 (Top 10 動画コンセプト)。

### 4. ★ 6 秒バンパー (最安 CPM)

3 言語 × 6秒 = YouTube pre-roll bumper ads。フォーマット:
- 0-2s: pattern interrupt 文 ("BJJを5年やった")
- 2-4s: 一拍置く ("でも上達しなかった")
- 4-6s: 解決 ("JiuFlow で全て変わった") + ロゴ + URL

これだけで 1 日 1-2 時間で作れる。

---

## 🟢 P3: 戦略実行 — JP funnel 改修

### 5. `/join` LP に「無料プラン」訴求を最前面に

**現状**: jiuflow.com/join は「7日間無料で試す」CTA が中心 → Stripe checkout 必須。
日本人の card 入力 aversion で離脱。

**改修案** (`store/src/main.rs` or `jiuflow-ssr` の関連 template):

```html
<!-- BEFORE -->
<a href="/stripe/checkout?plan=pro">7日間無料で試す →</a>

<!-- AFTER -->
<div class="cta-primary">
  <a href="/signup-free" class="btn-primary">無料で始める (カード不要)</a>
  <div class="cta-secondary">
    <a href="/stripe/checkout?plan=pro" class="link-secondary">Pro 7日間無料を試す</a>
  </div>
</div>
```

### 6. `subscribe_started` GA4 event を Google Ads conversion に import

**現状**: `/join` で「無料登録」ボタンクリック時に GA4 event `subscribe_started` が
発火しているが、Google Ads の conversion action として **import されていない**。

**手順**:
1. Google Ads → Conversions → "+ New conversion action" → Import → Google Analytics 4
2. `subscribe_started` event を選択
3. **Category**: SIGNUP (PURCHASE と別 goal にして primary 化注意)
4. **Value**: 任意 (Pro 1 月分の LTV を入れるなら ¥1,480、保守的に ¥500)
5. **Counting**: ONE_PER_CLICK

これで Smart Bidding が「free signup」も学習対象に → JP ag の CTR 4.0% (excellent) を
全部 CVR に変換する道が開ける。

---

## 🟢 P4: 監視 — Smart Bidding 学習完了の判定

### 7. `check_learning.py` の Telegram alert を確認

毎 30 分 cron で auto 動作 (commit 6cd6bf0)。EXIT 検知時に Telegram 通知。

**手動確認方法**:
```bash
python3 /Users/yuki/workspace/mu-brand/scripts/check_learning.py
```

EXIT 検知後:
- `loop_tighten.py` の learning guard が自動解除
- 翌 cron で全 mutation 関数が再開
- 1-2 週間蓄積した data で再最適化

---

## まとめ

| # | 項目 | API 可否 | 推定所要時間 |
|---|---|---|---|
| 1 | Stripe key rotate | ❌ user | 5 分 |
| 2 | Conv action secondary 化 | ❌ user (dashboard) | 10 分 |
| 3 | 動画 60秒圧縮 ×6 | ❌ user (CapCut) | 1 日 |
| 4 | 6秒バンパー ×3 | ❌ user (CapCut) | 2 時間 |
| 5 | JP /join LP 改修 | 🔧 product code | 2 時間 |
| 6 | GA4 → Ads conv import | ❌ user (dashboard) | 15 分 |
| 7 | 学習 EXIT 監視 | ✅ 自動 | 0 (cron) |

**実施推奨順序**: 1 → 2 → 6 → 5 → 3 → 4
