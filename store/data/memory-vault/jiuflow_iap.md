---
name: jiuflow_iap
description: JiuFlow iOS/Web subscription setup procedure - App Store Connect API + Stripe
type: reference
---

## JiuFlow サブスクリプション設定手順

### 現在のプラン
| プラン | iOS Product ID | Stripe Price ID | 価格 |
|--------|---------------|-----------------|------|
| PRO | jiuflow_pro_monthly | price_1TE5LPDqLakc8NxkgA7RyBSZ | ¥1,500/月 |
| BLACK BELT | jiuflow_blackbelt_monthly | price_1TE5LPDqLakc8Nxk9JRDwGfx | ¥4,000/月 |

### App Store Connect IDs
- App ID: 6757831498
- Subscription Group: 21992306
- PRO Subscription: 6760995778
- BLACK BELT Subscription: 6760995756
- API Key: 5KT46G9Y29, Issuer: e0d22675-afb3-45f0-a821-06b477f44da0

### iOS: App Store Connect API でサブスクリプション設定

#### 1. JWT トークン生成
```bash
TOKEN=$(python3 -c "
import jwt, time
key = open('/Users/yuki/.appstoreconnect/private_keys/AuthKey_5KT46G9Y29.p8').read()
now = int(time.time())
print(jwt.encode({'iss': 'e0d22675-afb3-45f0-a821-06b477f44da0', 'iat': now, 'exp': now+300, 'aud': 'appstoreconnect-v1'}, key, algorithm='ES256', headers={'kid': '5KT46G9Y29'}))
")
```

#### 2. サブスクリプショングループ作成
```bash
curl -s -X POST -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  "https://api.appstoreconnect.apple.com/v1/subscriptionGroups" \
  -d '{"data":{"type":"subscriptionGroups","attributes":{"referenceName":"GROUP_NAME"},"relationships":{"app":{"data":{"type":"apps","id":"APP_ID"}}}}}'
```

#### 3. サブスクリプション商品作成
```bash
curl -s -X POST -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  "https://api.appstoreconnect.apple.com/v1/subscriptions" \
  -d '{"data":{"type":"subscriptions","attributes":{"name":"PRODUCT_NAME","productId":"PRODUCT_ID","familySharable":false,"subscriptionPeriod":"ONE_MONTH","groupLevel":1},"relationships":{"group":{"data":{"type":"subscriptionGroups","id":"GROUP_ID"}}}}}'
```

#### 4. ローカライゼーション追加
```bash
curl -s -X POST -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  "https://api.appstoreconnect.apple.com/v1/subscriptionLocalizations" \
  -d '{"data":{"type":"subscriptionLocalizations","attributes":{"name":"表示名","description":"説明","locale":"ja"},"relationships":{"subscription":{"data":{"type":"subscriptions","id":"SUB_ID"}}}}}'
```

#### 5. 価格設定（重要：inline create方式）

通常のPOST /subscriptionPricesではエラーになる。PATCH /subscriptionsのincludedで設定する。

手順:
1. 目標のJPN価格のprice pointを探す
2. そのprice pointのUSA equalizationを取得
3. USA price pointをinlineCreateで設定（Apple が全テリトリー自動計算）

```bash
# ステップ1: JPN価格ポイント検索（例: ¥1,500）
curl -s -H "Authorization: Bearer $TOKEN" \
  "https://api.appstoreconnect.apple.com/v1/subscriptions/SUB_ID/pricePoints?filter[territory]=JPN&limit=200" \
  | python3 -c "import json,sys; [print(f'{p[\"id\"]}: ¥{p[\"attributes\"][\"customerPrice\"]}') for p in json.load(sys.stdin)['data'] if p['attributes']['customerPrice'] in ['1500','4000']]"

# ステップ2: USA equalization取得
curl -s -H "Authorization: Bearer $TOKEN" \
  "https://api.appstoreconnect.apple.com/v1/subscriptionPricePoints/JPN_PP_ID/equalizations?filter[territory]=USA" \
  | python3 -c "import json,sys; [print(f'{p[\"id\"]}: \${p[\"attributes\"][\"customerPrice\"]}') for p in json.load(sys.stdin)['data']]"

# ステップ3: 価格をinline createで設定
curl -s -X PATCH -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  "https://api.appstoreconnect.apple.com/v1/subscriptions/SUB_ID" \
  -d '{"data":{"type":"subscriptions","id":"SUB_ID","relationships":{"prices":{"data":[{"type":"subscriptionPrices","id":"${new}"}]}}},"included":[{"type":"subscriptionPrices","id":"${new}","relationships":{"subscriptionPricePoint":{"data":{"type":"subscriptionPricePoints","id":"USA_PP_ID"}}}}]}'
```

#### 6. 価格変更時
同じステップ5を新しいprice pointで繰り返す。古い価格は自動的に上書き。

### Web: Stripe 設定

#### 商品作成
```bash
SK="sk_live_..." # fly ssh console -a jiuflow-ssr -C "sh -c 'env | grep STRIPE_SECRET'"
# 商品作成
PROD=$(curl -s -X POST https://api.stripe.com/v1/products -u "$SK:" --data-urlencode "name=PRODUCT_NAME" --data-urlencode "description=DESC" | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")
# 価格作成（JPYはunit_amount=円額そのまま。セント不要）
PRICE=$(curl -s -X POST https://api.stripe.com/v1/prices -u "$SK:" -d "product=$PROD" -d "unit_amount=1500" -d "currency=jpy" -d "recurring[interval]=month" | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")
# Fly.io環境変数設定
fly secrets set STRIPE_PRICE_PRO=$PRICE -a jiuflow-ssr
```

#### 価格変更時
```bash
# 古い価格を無効化
curl -s -X POST "https://api.stripe.com/v1/prices/OLD_PRICE_ID" -u "$SK:" -d "active=false"
# 新しい価格作成
NEW_PRICE=$(curl -s -X POST https://api.stripe.com/v1/prices -u "$SK:" -d "product=$PROD" -d "unit_amount=NEW_AMOUNT" -d "currency=jpy" -d "recurring[interval]=month" | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")
fly secrets set STRIPE_PRICE_PRO=$NEW_PRICE -a jiuflow-ssr
```

### StoreKit テスト（Xcode）
- Products.storekit ファイル: `JiuFlow/Products.storekit`
- Scheme設定: `JiuFlow.xcodeproj/xcshareddata/xcschemes/JiuFlow.xcscheme` の `storeKitConfigurationFileReference`
- サンドボックスでは月額が5分で更新、6回で自動キャンセル

### 注意
- JPY通貨: Stripeは`unit_amount=1500`（セントなし）。App Storeは自動計算。
- App Store価格: 直接JPN価格は設定不可。USA価格ベース→自動equalization。
- MISSING_METADATA状態: 価格設定済みでもスクリーンショット未設定だと残る。