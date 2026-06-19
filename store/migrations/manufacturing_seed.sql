-- 製造オーケストレーション層 Phase2+ の参照用マイグレーション（人間レビュー用の写し）。
--
-- ⚠ 実適用はこのファイルではなく store/src/manufacturing_schema.rs の
--    ensure_manufacturing_schema(conn) がインライン execute_batch で行う。
--    include_str! でこの .sql を bundle しないこと（workspace gotcha:
--    cargo clean -p が include_str の内容を invalidate しないため）。
--    変更時は両方を手で一致させる。
--
-- 3機能の新テーブルを1本に統合。命名は CATALOG_CONTRACT の閉じた catalog_* surface を
-- 汚さないよう manufacturing_ / quote_ 接頭辞（製品サーフェス外の可変運用データ）。

-- (A) 製造要件KB
CREATE TABLE IF NOT EXISTS manufacturing_requirements (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    req_type    TEXT NOT NULL CHECK(req_type IN ('spec_floor','legal','supplier_term')),
    kind        TEXT,
    region      TEXT,
    supplier_id TEXT,
    key         TEXT NOT NULL,
    value_json  TEXT NOT NULL,
    severity    TEXT NOT NULL DEFAULT 'required'
                CHECK(severity IN ('required','recommended','info')),
    source_url  TEXT,
    version     INTEGER NOT NULL DEFAULT 1,
    active      INTEGER NOT NULL DEFAULT 1,
    updated_at  TEXT DEFAULT (datetime('now')),
    UNIQUE(req_type, kind, region, supplier_id, key)
);
CREATE INDEX IF NOT EXISTS idx_mfg_req_lookup
    ON manufacturing_requirements(req_type, kind, region, supplier_id, active);

-- (B) 仕様ドラフト（確定SKU前の可変要望・catalog_specs ではない）
CREATE TABLE IF NOT EXISTS manufacturing_specs (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    spec_id      TEXT UNIQUE NOT NULL,
    prompt       TEXT NOT NULL,
    kind         TEXT,
    spec_json    TEXT NOT NULL,
    missing_json TEXT,
    status       TEXT NOT NULL DEFAULT 'draft'
                 CHECK(status IN ('draft','complete','routed','rfq_linked')),
    email        TEXT,
    created_at   TEXT DEFAULT (datetime('now')),
    updated_at   TEXT DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_manufacturing_specs_spec_id ON manufacturing_specs(spec_id);
CREATE INDEX IF NOT EXISTS idx_manufacturing_specs_status  ON manufacturing_specs(status);

-- (C) RFQ 状態機械
CREATE TABLE IF NOT EXISTS quote_requests (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    supplier_id     TEXT NOT NULL,
    kind            TEXT NOT NULL,
    spec_id         TEXT,
    product_ref     TEXT,
    qty             INTEGER NOT NULL DEFAULT 1,
    spec_pack_url   TEXT,
    status          TEXT NOT NULL DEFAULT 'drafted'
                    CHECK(status IN ('drafted','sent','received','expired')),
    quoted_unit_jpy INTEGER,
    moq             INTEGER,
    lead_time_days  INTEGER,
    valid_until     TEXT,
    note            TEXT,
    draft_subject   TEXT,
    draft_body      TEXT,
    created_at      TEXT DEFAULT (datetime('now')),
    updated_at      TEXT DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_quote_requests_supplier_kind
    ON quote_requests(supplier_id, kind, status);
CREATE INDEX IF NOT EXISTS idx_quote_requests_spec ON quote_requests(spec_id);
