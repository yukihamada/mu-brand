# CHANGELOG — /shop キュレーション + 死にSKU cull (2026-06-05)

経緯: アクセス解析 (7d: /shop 113PV・dwell中央値7秒・買うクリック6) と
厳しめレビューを受けた「全部直す」スプリントの一部。
コード側 = feat/shop-fix ブランチ、データ側 = このファイルに記録。

## コード変更

1. 既定ソート(人気順)に `meta_json.featured=true` の看板固定 + `%STICKER%` SKU の降格
   （¥480ステッカーが店の顔になる問題の解消。価格/新着ソートには不適用）
2. `SOLD_BADGE_MIN` 5 → 3（現実の販売量に合わせ社会的証明の表示機会を増やす。実数のみ・捏造なし）
3. `GET /feed/google.tsv` — Google Merchant Center フィード新設
   （live + 実画像 + 価格>0 の物理商品。song/event_ticket 除外）

## 本番DB変更 (mu-store /data)

### featured 設定（看板3商品・人力キュレーション）

```sql
UPDATE catalog_products
SET meta_json = json_set(COALESCE(NULLIF(meta_json,''),'{}'), '$.featured', json('true'))
WHERE sku IN (
  'JIUFLOW-MU-NL-DARCE-RASHGUARD-BLACK-nl41a7e596',   -- BJJ旗艦 ¥12,800
  'JIUFLOW-MU-NL-DOJOSAVAGE-RASHGUARD-BLACK-nl56746ee8', -- 墨絵RG ¥14,800
  'MUON-LOVE-ENSO-TEE-001'                            -- MUONクラシック
);
```

選定基準: 実画像あり / アパレル / ブランドの顔として説明可能。
ロールバック: `json_remove(meta_json,'$.featured')` を同 WHERE で。

### 死にSKU cull

対象: `is_active=1 AND 注文0 AND 画像が存在しないローカル参照
(mockup_url_external IS NULL/期限切れ AND mockup_main_file が静的404)`。
実行前に件数 audit をこのファイルに追記し、`is_active=0`（行は消さない）。

実行記録 (2026-06-05):
- featured UPDATE 実行済み → 3行変更、`json_extract(meta_json,'$.featured')=1` で3件確認
- cull audit: **active 922件 全てが live external mockup 持ち** (extなし・静的参照=0件,
  extなし・main空=0件)。壊れ画像989件は 2026-06-04 の Printful URL 全数修復
  (commit 2709551) で既に非公開化済み → 追加 cull 不要、無作業でクローズ
