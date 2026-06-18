# MU × Google Ads API — Bootstrap

2026-05-16 起稿 · script: `scripts/google_ads_setup.py`

このスクリプトを 1 回走らせれば、 MU の 3 つの Google Ads キャンペーン
(Brand / Discovery / PMax) が一括で立ち上がります。 月 ¥30,000 (= ¥1,000/日)
予算。

```
A. MU-Brand     Search   ¥150/day  → /about
B. MU-Discovery Search   ¥500/day  → /you
C. MU-PMax      PMax     ¥350/day  → /buy
                          ─────────
                          ¥1,000/day = ¥30,000/mo
```

---

## 一度だけやる: OAuth bootstrap

Google Ads API は OAuth で叩く。 5 つの値を取って 1 ファイルにまとめる。

### 1. Google Ads アカウントを作る (まだなら)

- https://ads.google.com/intl/ja_jp/home/ → アカウント作成
- 通貨を **JPY** で設定 (後から変えられない)
- 課金カードを登録 (¥0 円のままで OK、 キャンペーン start するまで請求されない)
- ログイン後の URL 末尾の **10 桁の数字** が `customer_id` (例: `123-456-7890` → `1234567890`)

### 2. Developer Token を取る

- https://ads.google.com/aw/apicenter (上記アカウントでログイン)
- 「API センター」 → 開発者トークンを申請
- **Basic アクセス**で十分 (Premium は不要)
- 承認まで通常 1〜2 営業日。 即時 Test access で `test-account` 限定なら今すぐ使える

### 3. OAuth client_id / client_secret を取る

- https://console.cloud.google.com/ → 新しいプロジェクト
- 「API とサービス」 → 「OAuth 同意画面」 → 外部、 アプリ名 `MU AdWords` で作成
- 「認証情報」 → 「認証情報を作成」 → 「OAuth クライアント ID」
  - 種類: **デスクトップ アプリ**
  - 名前: `MU AdWords Desktop`
- 「JSON をダウンロード」 → `client_id` と `client_secret` を取得

### 4. refresh_token を取る (1 回だけのブラウザ操作)

```bash
pip install google-auth-oauthlib

python -c "
from google_auth_oauthlib.flow import InstalledAppFlow
flow = InstalledAppFlow.from_client_secrets_file(
    '/path/to/downloaded/client_secret.json',
    scopes=['https://www.googleapis.com/auth/adwords'],
)
creds = flow.run_local_server(port=0)
print('refresh_token:', creds.refresh_token)
"
```

ブラウザが開いて Google でログイン → 承認 → ターミナルに `refresh_token: 1//…`
が出る。 これを保存。

### 5. `~/.config/google-ads/google-ads.yaml` を書く

```yaml
developer_token: YOUR_DEVELOPER_TOKEN_FROM_STEP_2
client_id:       YOUR_CLIENT_ID.apps.googleusercontent.com
client_secret:   YOUR_CLIENT_SECRET
refresh_token:   1//YOUR_REFRESH_TOKEN_FROM_STEP_4
login_customer_id: 1234567890   # 10 桁、 ダッシュ無し。 MCC 持ってないなら同じ customer_id
use_proto_plus: true
```

`mkdir -p ~/.config/google-ads/ && nano ~/.config/google-ads/google-ads.yaml`

---

## 走らせる

```bash
# SDK インストール
pip install --upgrade google-ads

# 1. プランを確認 (mutate しない)
python scripts/google_ads_setup.py --dry-run

# 2. 3 キャンペーン作成 (本番反映、 課金開始)
python scripts/google_ads_setup.py --create-all --customer-id 1234567890

# 3. 1 週間ごとに数字を見る
python scripts/google_ads_setup.py --status --customer-id 1234567890

# 緊急停止 (全 MU-* キャンペーン pause)
python scripts/google_ads_setup.py --pause-all --customer-id 1234567890
```

### 想定 output (`--create-all`)

```
Created/updated:
  MU-Brand: {'campaign': 'customers/.../campaigns/12345', 'ad_group': '...', 'ad': '...'}
  MU-Discovery: {'campaign': '...', 'ad_group': '...', 'ad': '...'}
  MU-PMax: {'campaign': '...', 'note': 'PMax asset groups must be added in UI or follow-up script'}
```

---

## 走らせた直後にやること

1. **Google Ads UI で承認待ち広告をチェック** — 広告は通常 1 営業日以内に approve される。 NG コメント (誇大表現等) があれば即修正
2. **conversion action を作る** — UI で「ツールと設定」 → 「測定」 → 「コンバージョン」 → 新規 → 「ウェブサイト」 → 「購入」、 タグ生成
3. **tracking shim に conversion label を流す** — Fly secrets でセット:
   ```bash
   flyctl secrets set \
     GA4_MEASUREMENT_ID="G-XXXXXXX" \
     GADS_CONVERSION_ID="AW-XXXXXXX" \
     GADS_PURCHASE_LABEL="<conversion action label>" \
     -a mu-store
   ```
   `/api/tracking/config` が即この値を返し、 `/success` ページで自動 fire される
4. **PMax のアセットグループを UI で作る** — script はキャンペーン skeleton までしか作れない (PMax は画像 / sitelinks / 商品リスト等の asset group が必要)
5. **24h 後の振り返り** — `--status` で impressions / CTR / CPA 確認

---

## トラブル

| 症状 | 原因 / 対処 |
|---|---|
| `AUTHENTICATION_ERROR` | refresh_token expired or wrong account. Step 4 をやり直し |
| `DEVELOPER_TOKEN_NOT_APPROVED` | developer_token が test 限定。 Step 2 で Basic 申請してから |
| `INVALID_CUSTOMER_ID` | ダッシュ入りで渡してる。 `1234567890` (10 桁数字のみ) で |
| `BILLING_SETUP_NEEDED` | 課金カード未登録。 Ads UI で先に登録 |
| `DUPLICATE_*` | 既に作ってあるので safe、 script は skip する |

---

## 月の振り返りメトリクス

`--status` で以下が出る (過去 7 日):

| 列 | 意味 | 目安 |
|---|---|---|
| Spend ¥ | 消化額 | budget の 80%+ が回ってる |
| Impressions | 表示回数 | Brand: 100-500/日, Discovery: 50-200/日 |
| Clicks | クリック | CTR (= Clicks / Impressions) Brand >5%, Discovery >2% を target |
| Conv | コンバージョン | 7 日で 1 件 = まあまあ、 5 件 = 良い |
| CPA ¥ | 獲得単価 | ¥1,100 以下なら黒字、 ¥3,000 超なら戦線停止検討 |

---

## なぜ ¥30K/月で 3 戦線か

- **A (Brand)** は「MU を知った人を逃さない」 防衛戦線。 CPC が安く CVR も高い (= ROI ベスト) ので最優先で出す
- **B (Discovery)** は **スケールする唯一の戦線**。 「AI Tシャツ 生成」 のような non-brand KW でしか新規流入は獲得できない
- **C (PMax)** はリターゲ + lookalike の自動最適化。 A/B のオーガニックなクッキー / リストを再活用する

3 つを併走することで:
- A が「ブランド体力測定」 (検索数が増えてるか)
- B が「新規開拓力」 (新規 CAC)
- C が「LTV 引き上げ」 (リピート率)

の 3 軸を独立に測定できます。 1 つだけ出すと交絡変数で判断ミスる。

---

## 関連

- ad copy / KW 定義: `scripts/google_ads_setup.py` の冒頭定数
- tracking shim: `store/static/tracking.js` + `/api/tracking/config`
- /about LP (Brand キャンペーンの着地点): `store/static/about.html`
- /you LP (Discovery キャンペーンの着地点): `store/static/you.html`
- /buy LP (PMax の着地点): `store/static/buy.html`
