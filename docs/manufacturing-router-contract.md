# Manufacturing Router — 契約 (Phase 0)

> 「言えば、最適な供給先に流れる」。MU を平面プリント(POD)1社依存から、
> kind/素材/数量/地域/予算に応じて供給先を選び、見積(価格・MOQ・納期)を即返す層へ。
> CATALOG_CONTRACT v1 を壊さない**追加レイヤー**。新テーブルは可変データ(受領見積)のみ。

## スコープ境界
- **Phase 1（本ドキュメント実装分・read-only）**: 能力レジストリ + `route_request()` + `mu_quote`。
  注文・DB を一切変更しない。見積を返すだけ。
- Phase 2: RFQ パイプライン（`quote_requests` テーブル + `mu_rfq_*`）。
- Phase 3: 受注後製造の状態機械 + ロットロック。
- Phase 4: 2社目の実POD稼働(Gelato JP)。

## 型契約

### SupplierCapability（const レジストリ・catalog.rs）
不変の供給先能力。`PRODUCT_SPECS` と同じ const 流儀。

| field | 型 | 意味 |
|---|---|---|
| `id` | str | 供給先ID（例 `printful` / `isami_gi`） |
| `name` | str | 表示名 |
| `mode` | `"auto"` \| `"quote"` | auto=API即発注 / quote=見積必須(人手RFQ) |
| `route` | str | `fulfillment_route` enum 値。`*` kinds は kind 毎に `route_for_kind` で解決 |
| `regions` | [str] | `"global"` or ISO風（`jp`/`us`…） |
| `kinds` | [str] | 作れる kind。`"*"` = PRODUCT_SPECS の全 POD kind |
| `moq` | i64 | 最小ロット。`-1` = 要見積で確定 |
| `lead_time_days` | i64 | 納期目安。`-1` = 要見積 |
| `est_unit_jpy` | Option<i64> | 単価目安。`*` は kind の floor から解決。None = 要見積 |
| `note` | str | 由来/制約（docs ポインタ等） |

### RouteDecision（route_request の各選択肢・JSON）
`supplier_id, supplier_name, mode, fulfillment_route, unit_price_jpy(null可),
moq(null可), lead_time_days(null可), regions, region_match, meets_moq,
within_budget(null可), requires_rfq, note`

### route_request 入力
`kind?`（未指定なら `description` からキーワード推論）, `qty`, `region?`, `budget?`

ランキング: ①auto かつ価格あり かつ予算内 ②地域一致 ③価格昇順 ④納期昇順。

## 初期レジストリ（Phase 1・docs 由来で裏取り済）
- `printful` — auto / 全POD kind / MOQ1 / ~10日 / 単価=kind floor（EU/US製造・在庫ゼロ）
- `isami_gi` — quote / `gi` / MOQ10 / ~45日 / 要見積（刺繍17箇所・IBJJF準拠 → docs/gi-isami-2026-05-12）
- `heritage_loopwheel` — quote / `loopwheel_sweat`,`cut_and_sew` / MOQ15 / ~90日 / ¥35,000（和歌山×弟子屈×兵庫 → docs/heritage-fulfillment-workflow.md）
- `shima_seamless` — quote / `seamless_knit` / MOQ要見積 / 要見積（島精機WHOLEGARMENT・型代¥1-2M → docs/seamless_knit/tech_pack.json）
- `contrado_uk` — quote / `rashguard_premium` / MOQ1 / ~14日 / ¥19,800（縁まで全面・Printfulの2-3倍原価 → docs/CONTRADO_SALES_OUTREACH.md）

## 人間ゲート
- quote モードの実見積メール/PO 送信は人間（Phase 2/3）。
- Phase 1 は read-only のため不要。
