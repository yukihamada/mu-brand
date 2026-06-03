# MU チケット販売機能（event_ticket kind）

要件（確定）: QR付き入場券 / 定員あり（売り切れ）/ 第1弾=AI合宿チケット（asoview-camp・限定リンク）

契約準拠（docs/CATALOG_CONTRACT.md）: 新テーブル作らない。route=`digital`（既存）。
capacityは汎用JSON1列に。per-attribute列は作らない。

## 実装（store/src/catalog.rs 中心）
- [ ] ensure_schema: ALTER 追加（idempotent）
      - catalog_products.meta_json TEXT  … {"capacity":N} 等
      - catalog_orders.ticket_code TEXT + index … /t/:code 逆引き
- [ ] PRODUCT_SPECS に kind="event_ticket"（printful_id=0/placement none/floor ¥1000）
- [ ] agent_insert_product route 分岐: event_ticket => "digital"
- [ ] kind_from_sku: -TICKET- → "event_ticket"
- [ ] shop_checkout:
      - row 取得に fulfillment_route, meta_json 追加
      - route=digital: 定員ゲート（売れた数 >= capacity → 完売HTML）
      - route=digital: shipping_address_collection を付けない（物理のみ付与）
- [ ] fulfill_catalog_order: manual arm の後に digital arm（早期return）
- [ ] issue_ticket() コア: code生成(sha256)→QR PNG→R2(tickets/<code>.png)→記録→Resendメール→Telegram
      - fulfill_digital_ticket（webhook用）と admin_ticket_issue（comp/検証用）が共用
- [ ] ticket_view: GET /t/:code 公開券面（VALID + QR + イベント名）noindex
- [ ] admin_ticket_issue: GET /admin/catalog/ticket_issue?token=&sku=&email=&name= （comp発券=E2E検証用）
- [ ] main.rs route 登録: /t/:code, /admin/catalog/ticket_issue

## 追加スコープ（同バッチ）
- [x] 曲(song) kind: route=digital、購入後に視聴/DLリンクをメール、/t/:code に audio player
- [x] アフィリエイト: ?ref=/mu_refクッキー→Stripe metadata→fulfillでコミッション(既定10%,brand config_jsonで上書)→mu_credit_ledger+mu_referrals計上→catalog_orders監査列。/affiliate 発行 + /affiliate/:code ダッシュボード
- [x] agent API: capacity/audio_url 受付(meta_json) + GET /api/agent/affiliate (JSON)
- [x] MCP(mu-mcp): KINDSに event_ticket/song、capacity/audio_url パラメータ、mu_affiliate tool、"not live"固定文をstatus準拠に修正

## 検証
- [x] cargo build --release（store: exit0 / mcp tsc: ok）※最終ビルド確認中
- [ ] push→GHA→deploy (mu-brand + mu-mcp)
- [ ] checkout: ticket SKU が Stripe session(302) を返す & shipping収集が無いこと
- [ ] 定員ゲート: capacity到達で完売応答
- [ ] admin_ticket_issue で実際にQR付きメール着弾（E2E）＋ /t/:code がVALID表示
- [ ] song: /t/:code に audio player / メールにDLリンク
- [ ] affiliate: /affiliate 発行→/r/:code クッキー→ /api/agent/affiliate JSON
- [ ] MCP: tools/list に event_ticket/song/mu_affiliate が出る
- [ ] AI合宿チケットを asoview-camp に作成（capacity/価格は本人確認の上）
