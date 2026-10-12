-- store/migrations/20260609_1_work_unbox.sql
-- 「開封体験パック」MVP: 既存 /work(音コインNFC)に「仕上げて届ける人」レイヤーを additive で足す。
-- additive / idempotent。本番DBには当てない(PRに同梱・work.rs ensure_tables() が起動時に冪等適用)。
-- CATALOG_CONTRACT 準拠: work_* は商品/ブランド/注文/画像サーフェス外(既存 work_workers/work_assignments が前例)。
-- 注文状態の単一ソースは catalog_orders.status(manual_pending→manual_assigned→manual_shipped)のまま壊さない。
-- 重要: 以下のテーブルに住所・氏名カラムは一切置かない。住所は catalog_orders.shipping_address_json にだけ存在し、
--       表示は常に work.rs:render_addr(full) 経由でゲートする。

-- 既存 work_workers 拡張(PII本体は持たない。口座はハッシュのみ)
-- SQLite は重複ALTERで err を返すが ensure_tables() は execute の戻りを swallow するため冪等。
ALTER TABLE work_workers ADD COLUMN nfc_capable  INTEGER NOT NULL DEFAULT 1;  -- 0=NFC非対応端末(和子)→'oto'を割り当てない
ALTER TABLE work_workers ADD COLUMN trust_tier   INTEGER NOT NULL DEFAULT 0;  -- 0=新人(ハブ経由・住所非開示) / 1=直送可
ALTER TABLE work_workers ADD COLUMN kyc_state    TEXT NOT NULL DEFAULT 'none';-- none|pending|verified
ALTER TABLE work_workers ADD COLUMN payout_hash  TEXT;                        -- 銀行口座の突合用ハッシュ。平文は持たない
ALTER TABLE work_workers ADD COLUMN flagged      INTEGER NOT NULL DEFAULT 0;

-- 既存 work_assignments 拡張(order_id PRIMARY KEY は維持＝CAS claim をそのまま継承)
ALTER TABLE work_assignments ADD COLUMN job_kind     TEXT NOT NULL DEFAULT 'oto';   -- 'oto' | 'unbox'
ALTER TABLE work_assignments ADD COLUMN review_state TEXT NOT NULL DEFAULT 'claimed';
  -- claimed → proof_submitted → approved | rework | defect_reported
ALTER TABLE work_assignments ADD COLUMN ito_grains   INTEGER NOT NULL DEFAULT 0;     -- 付与予定の糸(粒)。balance には入れない
ALTER TABLE work_assignments ADD COLUMN approved_at  TEXT;

-- 作業証跡。写真のR2キーのみ保持。住所/氏名/伝票は絶対に入れない
CREATE TABLE IF NOT EXISTS work_proofs (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id      INTEGER NOT NULL,          -- catalog_orders.id への論理FK
    worker_id     INTEGER NOT NULL,
    stage         TEXT NOT NULL,             -- 'wrapped' | 'sealed' | 'posted'
    object_key    TEXT NOT NULL,             -- store_r2_bytes が返した R2 キー
    sha256        TEXT,                       -- 同一画像の使い回し検知(不正対策)
    exif_stripped INTEGER NOT NULL DEFAULT 0, -- 1=サーバが decode→再エンコードでGPS等を除去した証跡
    pii_clear     INTEGER NOT NULL DEFAULT 0, -- 作業者の「住所/宛名/追跡が写っていない」申告
    is_public     INTEGER NOT NULL DEFAULT 0, -- 1=焚き火/SNS共有用のマスク済み派生のみ。原本(審査用)は0
    created_at    INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_work_proofs_order ON work_proofs(order_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_work_proofs_sha ON work_proofs(sha256) WHERE sha256 IS NOT NULL;

-- 糸(ITO)付与の隔離台帳。mu_credit_ledger.delta_jpy には粒数を入れない(=spendable円残高/景表法20%キャップを汚すバグ回避)。
-- 糸本体(PR#109)マージ後にこの粒数を実残高へ結線する。それまでは「仮計上」。
CREATE TABLE IF NOT EXISTS work_ito_grants (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    worker_id  INTEGER NOT NULL,
    order_id   INTEGER NOT NULL,
    grains     INTEGER NOT NULL,             -- 糸の粒数(円ではない)
    ref_id     TEXT NOT NULL,                -- 'order:{id}' 冪等キー
    settled    INTEGER NOT NULL DEFAULT 0,   -- 1=糸本体へ結線済み
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_work_ito_ref ON work_ito_grants(ref_id);

-- 不正/マネロン監査ログ。client_ip は fly-validated(catalog_return_requests と同パターン)。PIIは丸めて格納
CREATE TABLE IF NOT EXISTS work_audit (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    worker_id   INTEGER,
    order_id    INTEGER,
    event       TEXT NOT NULL,               -- claim|proof|approve|rework|release|flag|defect
    client_ip   TEXT,
    detail_json TEXT,                          -- 住所は入れない
    created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

-- 現金エスクロー報酬は新テーブルを作らず mu_credit_ledger(email, delta_jpy, reason='work_cash', ref_id='order:{id}') に記帳。
-- 月末締め支払い対象は reason='work_cash' AND released で SUM。実振込は人間ゲート(BUDGET §3)。
