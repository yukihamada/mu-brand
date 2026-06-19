# 製造オーケストレーション層 Phase2+ 契約（RFQ / 要件KB / 仕様生成）

> Phase1（見積ルーター `route_request` / `mu_quote` / `GET /api/agent/quote`）は本番LIVE（PR #216）。
> 本書は、その上に「**言う → 完璧な仕様 → 要件で検証 → 供給先選定 → RFQ → 受注 → 納品**」を
> 通すための追加3レイヤーの契約。Phase1 は壊さず**後方互換の追加レイヤー**として同梱する。

## なぜ新テーブルか（CATALOG_CONTRACT 準拠）

`docs/CATALOG_CONTRACT.md` Rule 1 の「唯一の product surface」は catalog_* 7テーブルに閉じる。
本3テーブルは**製品サーフェスではなく**、製造前/製造中に時間変化する運用データなので、
閉じた catalog_* 名前空間を汚さないよう `manufacturing_` / `quote_` 接頭辞で分離する
（`app_push_tokens` 等の非商品別 namespace 前例に倣う）。スキーマ実体は
`store/src/manufacturing_schema.rs::ensure_manufacturing_schema`（boot 時 `catalog::ensure_schema` 末尾から1回）。

## (A) 要件KB — `manufacturing_requirements`

bim.house の houki_check の「製造版」。製品仕様floor・法的要件・メーカー発注条件を保持し、
`source_url + version + active` で「日々アップデート（版上げ）→人手レビュー後に有効化」を表現。

- `req_type ∈ {spec_floor, legal, supplier_term}` の1テーブルに集約（列増殖回避）。
- `check_requirements(conn, kind, region, supplier_id, spec) -> RequirementReport{ok,gaps,actions}`
  = 決定論純関数（`conn=None` でコード既定のみでも動く）。route_request と仕様生成(B)から呼ぶ。
- 取込: `GET /admin/requirements/refresh`（**`?token=ADMIN_TOKEN` 必須**、Gemini 取込は
  **`MU_REQ_INGEST_ENABLED=1` 未設定時は ¥0 のテンプレ経路**＝既定 gated）。
  `POST /admin/requirements/upsert`（ADMIN_TOKEN）。

## (B) 仕様生成 — `manufacturing_specs`

自然文 → 構造化 `ManufacturingSpec`。不足属性は `kind_required_attrs`(A) を根拠に逆質問で埋める。

- `POST /api/agent/spec`（MCP `mu_spec_draft`）。**`require_email` 既定**（無認証の Gemini 課金経路を
  作らない＝must-fix）。無認証開放は env + 人間ゲート。
- `status: draft → complete → routed → rfq_linked`。完成 spec_id を RFQ(C) に渡す。

## (C) RFQ — `quote_requests`

非POD（`mode="quote"`: isami_gi / heritage_loopwheel / shima_seamless / contrado_uk）の「要見積」を
DB駆動の状態機械に昇格。`drafted → sent → received → expired`。

- `mu_rfq_create`（ドラフト生成のみ・**送信しない**）/ `mu_rfq_record`（返答=価格/MOQ/納期を記録）/
  `mu_rfq_list`。サーバは `agent_owner_or_err`（owner-only・ADMIN_EMAIL 一致）でゲート。
- `received_quote_for(conn, supplier_id, kind)` が、受領済み有効見積を `route_request` の
  `est_unit_jpy` に **read-only 反映**（表示値のみ・ランキング sort key は不変＝Phase1順位を壊さない）。
- docs（heritage / seamless_knit / gi-isami）のメール下書きを `seed_rfq_drafts` で drafted 行に移植。

## 全体フロー

```
言う → (B)draft_spec → (A)check_requirements で検証/不足逆質問
     → route_request（Phase1・(A)(C)を read-only 注入）→ (C)mu_rfq_create
     → [人間: 見積メール送信] → mu_rfq_record(received) → 受注 → 状態機械 → 納品(Phase3)
```

## 人間ゲート（CLAUDE.md / BUDGET.md 準拠）

1. 実見積メール / PO の**送信**（ドラフト生成+DB保存まで自動・送信は優貴さんが手動）
2. 3新テーブルの本番 Fly DB への**初回 schema 投入**（= デプロイ。承認後）
3. `MU_REQ_INGEST_ENABLED=1` での Gemini 取込**日次 cron 常時 enable**（コスト増）。既定 OFF・¥0 経路
4. `POST /api/agent/spec` の Gemini を cron で**大量自動生成**する運用（単発は許容）
5. 法的要件データ（PSE/家庭用品品質表示/PL/食品衛生 等の該当フラグ）の**正確性レビュー**後に active 化
6. check が action_required の製品・RFQ received 反映後の**実 PO/発注**（反映自体は read-only）
