# 工場向け出荷管理画面 /admin/ship (MU内部ツール)

決定: MU内部ツール / 送り状CSV出力→B2クラウド取込方式 / 認証=既存ADMIN_TOKEN

## 実装
- [ ] catalog_orders に列追加(idempotent ALTER): ship_status, courier, tracking_number, shipped_at
- [ ] GET /admin/ship   — 受注ボード(未発送→製作中→発送済→完了) + 住所/サイズ/刺繍指示 + CSV DLボタン
- [ ] GET /admin/ship/csv?courier=yamato|sagawa — 送り状CSV(B2クラウド/e飛伝)添付DL
- [ ] POST /admin/ship/mark — status遷移 + tracking記入
- [ ] ルート3本登録 (68900付近)
- [ ] m5でcargo build --release → 緑
- [ ] git push → Actions → /admin/ship 200確認

## 注意
- shipping_address_json = Stripe形 {name,address:{line1,line2,city,state,postal_code,country},phone}
- 物理出荷のみ対象(shipping_address_json非空 or route=manual/printful)
- B2クラウド列マッピングは初回に取込レイアウト設定必要(CSVはラベル付きで出す)
