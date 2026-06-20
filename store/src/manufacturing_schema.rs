//! 製造オーケストレーション層 Phase2+ の新テーブル統合スキーマ。
//!
//! 対象3機能の可変運用データを1関数 `ensure_manufacturing_schema` に集約する:
//!   (A) 製造要件KB     → `manufacturing_requirements`
//!   (B) 仕様生成ドラフト → `manufacturing_specs`
//!   (C) RFQ 状態機械    → `quote_requests`
//!
//! ## なぜ `catalog_*` でないか（CATALOG_CONTRACT 準拠）
//! `docs/CATALOG_CONTRACT.md` の Rule 1 が定める「唯一の product surface」は
//! catalog_* 7テーブル（products/brands/orders/product_extras/gen_jobs/spend/
//! founder_cards）に閉じている。本3テーブルは **製品サーフェスではなく**、
//! 供給先との往復・要件版管理・仕様ドラフトという「時間変化する製造前/製造中の
//! 運用データ」なので、閉じた catalog_* 名前空間を汚さないよう
//! `manufacturing_` / `quote_` 接頭辞で分離する（app_push_tokens 等の非商品
//! 別 namespace 前例に倣う）。
//!
//! ## 流儀
//! - 既存 `catalog::ensure_schema` 末尾から boot 時に1回だけ呼ぶ。全て idempotent。
//! - `include_str!` で .sql を bundle しない（workspace gotcha: `cargo clean -p`
//!   が include_str の内容を invalidate しないため）。SQL は本関数にインライン。
//!   `store/migrations/manufacturing_seed.sql` は人間レビュー用の写しに限定。
//! - `updated_at` は SQLite の自動更新トリガを置かない。UPDATE 側で必ず
//!   `updated_at=datetime('now')` を手書きする（既存 catalog.rs 流儀）。

use rusqlite::Connection;

/// 3機能の新テーブル＋インデックスを冪等に作成する。
pub fn ensure_manufacturing_schema(conn: &Connection) {
    let _ = conn.execute_batch(
        "
        -- (A) 製造要件KB。req_type で spec_floor / legal / supplier_term を1テーブルに
        -- 集約し列増殖を回避。value_json に要件本体、source_url+version+active で
        -- 「日々アップデート（版上げ）」と人手レビュー後の有効化を表現する。
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

        -- (B) 仕様ドラフト。確定SKU前の可変要望。自然文 prompt → 構造化 spec_json、
        -- missing_json は不足属性（逆質問のソース）。status で draft→complete→routed→
        -- rfq_linked のライフサイクル。catalog_specs ではなく manufacturing_specs:
        -- 製品サーフェス（catalog_products.status enum）と混ざらないよう分離。
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

        -- (C) RFQ 状態機械。supplier との往復で時間変化する可変運用データ。
        -- spec_id で (B) と連結。drafted→sent→received→expired。
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
        ",
    );
    // per-agent 所有者列（後付け・既存テーブルにも足す）。重複時エラーは握り潰す。
    let _ = conn.execute("ALTER TABLE quote_requests ADD COLUMN owner_email TEXT", []);
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_quote_requests_owner ON quote_requests(owner_email)",
        [],
    );
}
