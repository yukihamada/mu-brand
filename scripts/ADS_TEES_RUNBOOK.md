# ads_* Tees — Launch Runbook

20 SKU を本番 wearmu.com に乗せて Google Ads 流すまでの手順。
2026-05-16 起稿。

## 0. 現状

ローカル `store/products.db` に下記が入っている (active=1, design済):

| brand           | 件数 | 価格帯       | 在庫×単価 GMV |
|-----------------|------|--------------|---------------|
| ads_jujitsu     | 8    | ¥4,900-5,500 | ¥915,500      |
| ads_regional    | 4    | ¥4,900       | ¥392,000      |
| ads_kokon       | 3    | ¥4,900       | ¥392,000      |
| ads_profession  | 2    | ¥4,900       | ¥196,000      |
| ads_event       | 3    | ¥4,900-5,500 | ¥538,000      |
| **合計**        | **20** |            | **¥2,433,500** |

デザインPNG 20枚: `store/static/ads/ads_*_*.png` (各 ~500KB, 計 ~10MB)

## 1. デプロイ (git push → GH Actions → Fly)

```bash
# このプロジェクトの ads_* 関連だけステージング (他の進行中作業を巻き込まない)
git add scripts/add_ads_targeted_tees.py \
        scripts/gen_ads_designs.py \
        scripts/google_ads_setup_ads_tees.py \
        scripts/ADS_TEES_RUNBOOK.md \
        store/static/ads/

git commit -m "feat(ads): 20 ad-targeted T-shirt SKUs + Gemini design pipeline + Google Ads plan"
git push origin main
# → GH Actions builds Docker image with PNGs included, deploys to mu-store on Fly
```

確認: https://github.com/<owner>/<repo>/actions で緑になるまで待つ (~5分)

## 2. 本番DBにSKU投入 + activate

```bash
# Fly に SSH
flyctl ssh console -a mu-store

# /data に products.db がある。scriptは /app/scripts に COPYされている
cd /app
python3 scripts/add_ads_targeted_tees.py
# → 20件 INSERT + design_url 自動設定 + active=1
```

スクリプトは idempotent — 既存row はスキップ、design_url 未設定なら埋めて activate する。

## 3. Stripe payment link 生成 (各SKU)

`POST /api/admin/products/:id/payment-link` がprod上でStripe APIを叩いて payment_link_url を生成する。

```bash
# ローカルから (MU_ADMIN_TOKEN を /Users/yuki/.env から)
source /Users/yuki/.env
for id in 195 196 197 198 199 200 201 202 \
         203 204 205 206 \
         207 208 209 \
         210 211 \
         212 213 214; do
  echo "=== $id ===" ; \
  curl -sX POST "https://wearmu.com/api/admin/products/$id/payment-link" \
    -H "X-Admin-Token: $MU_ADMIN_TOKEN" -H "Content-Type: application/json" \
    | head -c 200 ; echo
done
```

確認: https://wearmu.com/admin/products で各SKUに payment_link_url が入っているか。

## 4. 商品ページ動作確認

各カテゴリ1つずつ目視:
- https://wearmu.com/products/ads_jujitsu/195 (NOGI Tee)
- https://wearmu.com/products/ads_regional/203 (三田)
- https://wearmu.com/products/ads_kokon/207 (焼肉古今)
- https://wearmu.com/products/ads_profession/210 (ICU看護師)
- https://wearmu.com/products/ads_event/212 (SOLUNA FEST)

Stripe checkoutへの遷移、画像表示、価格、在庫がOKか。

## 5. Google Ads OAuth bootstrap (初回のみ)

`.env` に `GOOGLE_ADS_REFRESH_TOKEN` と `GOOGLE_ADS_CUSTOMER_ID` が無いので取得が必要:

```bash
cd /Users/yuki/workspace/mu-brand
pip install --upgrade google-ads google-auth-oauthlib
python3 scripts/google_ads_bootstrap.py
# → ブラウザが開く、Google ログイン → 承認
# → refresh_token と利用可能なcustomer_id一覧が表示される
# → .env に追記:
#    GOOGLE_ADS_REFRESH_TOKEN=...
#    GOOGLE_ADS_CUSTOMER_ID=10桁数字 (例: 1234567890)
```

`~/.config/google-ads/google-ads.yaml` も作成 (bootstrap script に --write-yaml フラグあり)。

## 6. Google Ads キャンペーン作成 (PAUSED)

```bash
python3 scripts/google_ads_setup_ads_tees.py --dry-run    # プラン確認
python3 scripts/google_ads_setup_ads_tees.py --create-all # 作成 (PAUSED)
```

または、UIで手動作成:
- Campaign 名: `MU-AdsTees-Search`
- Daily budget: ¥500
- Geo: 日本、Lang: 日本語
- 5 ad groups: jujitsu, regional, kokon, profession, event
- 各 ad group の keywords / headlines / descriptions / final_url は `scripts/google_ads_setup_ads_tees.py` の `AD_GROUPS` 参照
- Negative keywords: 21個 (中古/古着/無料/Uniqlo等)

## 7. キャンペーン enable + 監視

1. Google Ads UI で `MU-AdsTees-Search` を ENABLED に
2. 24h で **impressions / clicks / CTR / CPC** を見る
3. 7日で **conversions / ROAS** を判定 (>1.5x なら scale, <1.0x なら kill)

| 指標     | 目安 (日本 apparel 2026)   |
|----------|----------------------------|
| CTR      | >2.5% (検索広告)           |
| CPC      | ¥50-150                    |
| CVR      | 1.5-3% (商品LP)            |
| ROAS     | >2.0 で OK, >3.0 で勝ち    |

## 8. ロールバック手順

完全に外したくなった場合:

```sql
-- 本番DB (Fly SSH → sqlite3 /data/products.db)
UPDATE products SET active=0 WHERE brand LIKE 'ads_%';
```

Google Ads:
```bash
python3 scripts/google_ads_setup_ads_tees.py --pause-all
```

## 9. 仕掛け済みの安全装置

- 商標スキャン: ADCC/IBJJF/UFC/Nike/Adidas を name + prompt_text から除外済
- "Earned Not Given" → "One Bar Down" にrename (派生TM回避)
- SOLUNA FEST 日付ハルシネーション削除済 (実日程確定後に design 差替えてOK)
- 三田 Tokyo Tower silhouette → 純typography badge に再生成済
- prompt の "[Ad keyword: ...]" prefix は Gemini に渡す前に strip (デザインに leak しない)

## 10. 次の改善

- [ ] Mockup生成 (PrintfulのDTG mockup APIで実Tシャツ写真化)
- [ ] R2 (lifestyle.wearmu.com) にpush → CDN配信高速化
- [ ] SUZURI 国内ミラー登録 (POST /api/admin/suzuri/publish/:pid)
- [ ] PMax campaign追加 (検索広告で勝率高ければ予算移動)
- [ ] 売上が出たSKUは「色違い展開」 (SUZURIの売れ筋分析パターン)
