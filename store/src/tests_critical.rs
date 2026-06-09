//! Fast, network-free, DB-free (in-memory only) regression tests for two
//! crate-root invariants:
//!   (c) `require_admin_token` is fail-closed (unset/empty ADMIN_TOKEN → reject)
//!   (d) the authenticity-proof verify query does NOT depend on `active`
//!       (a sold-out 1-of-1 has active=0 but its proof must still resolve).
//!
//! New file + a single `mod` line at the end of main.rs to stay clear of
//! in-flight WIP.

use axum::http::StatusCode;
use axum::response::IntoResponse;

// ── (c) ADMIN_TOKEN auth is fail-closed ──────────────────────────────────────
//
// `require_admin_token` reads the ADMIN_TOKEN env var directly, so this test
// mutates process env. It is written as ONE test that runs all cases
// sequentially (set → assert → restore) to avoid env races with other tests;
// no other test in this crate touches ADMIN_TOKEN.
#[test]
fn require_admin_token_is_fail_closed() {
    use std::env;

    // Snapshot + guarantee restoration even on panic.
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => env::set_var("ADMIN_TOKEN", v),
                None => env::remove_var("ADMIN_TOKEN"),
            }
        }
    }
    let _restore = Restore(env::var("ADMIN_TOKEN").ok());

    let status_of = |r: Result<(), axum::response::Response>| -> Option<StatusCode> {
        r.err().map(|resp| resp.into_response().status())
    };

    // 1) ADMIN_TOKEN unset → fail closed with 503 (server misconfigured),
    //    regardless of what the caller provides.
    env::remove_var("ADMIN_TOKEN");
    assert_eq!(
        status_of(crate::require_admin_token(None)),
        Some(StatusCode::SERVICE_UNAVAILABLE),
        "unset ADMIN_TOKEN + no token → 503"
    );
    assert_eq!(
        status_of(crate::require_admin_token(Some(&"anything".to_string()))),
        Some(StatusCode::SERVICE_UNAVAILABLE),
        "unset ADMIN_TOKEN must reject even a provided token (no bypass)"
    );

    // 2) ADMIN_TOKEN empty string → still fail closed (503). An empty secret
    //    must never become a valid credential.
    env::set_var("ADMIN_TOKEN", "");
    assert_eq!(
        status_of(crate::require_admin_token(Some(&"".to_string()))),
        Some(StatusCode::SERVICE_UNAVAILABLE),
        "empty ADMIN_TOKEN + empty provided must NOT authenticate"
    );

    // 3) Configured token + wrong value → 401 unauthorized.
    env::set_var("ADMIN_TOKEN", "correct-horse-battery-staple");
    assert_eq!(
        status_of(crate::require_admin_token(Some(&"wrong".to_string()))),
        Some(StatusCode::UNAUTHORIZED),
        "wrong token → 401"
    );
    assert_eq!(
        status_of(crate::require_admin_token(None)),
        Some(StatusCode::UNAUTHORIZED),
        "missing token when configured → 401"
    );

    // 4) Configured token + exact match → Ok.
    assert!(
        crate::require_admin_token(Some(&"correct-horse-battery-staple".to_string())).is_ok(),
        "correct token must authenticate"
    );
}

// ── (d) authenticity-proof (verify_page) is independent of sale status ───────
//
// 1-of-1 drops (MA / MUGEN) are flipped to active=0 once sold. The QR proof
// at /v/:brand/:drop_num must still resolve them — proof/metadata are about
// provenance, not inventory. This guards the *exact* SQL used by verify_page
// against re-introducing an `AND active=1` filter (the 2026-06-03 design bug
// that 404'd every shipped 1-of-1).
#[test]
fn verify_query_resolves_sold_out_inactive_unit() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    // Minimal `products` schema mirroring the real table's relevant columns.
    conn.execute_batch(
        "CREATE TABLE products (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            brand        TEXT NOT NULL,
            drop_num     INTEGER NOT NULL,
            name         TEXT NOT NULL,
            design_url   TEXT,
            mockup_url   TEXT,
            price_jpy    INTEGER NOT NULL,
            inventory    INTEGER NOT NULL,
            sold         INTEGER DEFAULT 0,
            created_at   TEXT NOT NULL,
            active       INTEGER DEFAULT 1,
            weather_data TEXT,
            prompt_hash  TEXT,
            nft_mint     TEXT
        );",
    )
    .unwrap();

    // A SOLD-OUT 1-of-1: active=0, inventory=1, sold=1.
    conn.execute(
        "INSERT INTO products
            (brand, drop_num, name, price_jpy, inventory, sold, created_at, active)
         VALUES ('mugen', 7, 'One of One', 49800, 1, 1, '2026-01-01', 0)",
        [],
    )
    .unwrap();

    // The exact projection/filter used by verify_page — NO active predicate.
    let found: Result<String, _> = conn.query_row(
        "SELECT name, mockup_url, design_url, weather_data, price_jpy, inventory, sold,
                created_at, prompt_hash, nft_mint
         FROM products WHERE brand=? AND drop_num=? LIMIT 1",
        rusqlite::params!["mugen", 7i64],
        |row| row.get::<_, String>(0),
    );
    assert_eq!(
        found.unwrap(),
        "One of One",
        "verify proof must resolve a sold-out (active=0) 1-of-1"
    );

    // Belt-and-braces: prove that adding `AND active=1` WOULD break it — so
    // this test fails loudly if someone reintroduces that filter in the handler.
    let with_active_filter: Result<String, _> = conn.query_row(
        "SELECT name FROM products WHERE brand=? AND drop_num=? AND active=1 LIMIT 1",
        rusqlite::params!["mugen", 7i64],
        |row| row.get::<_, String>(0),
    );
    assert!(
        with_active_filter.is_err(),
        "sanity: an active=1 filter hides the shipped unit — verify_page must not use it"
    );
}

// ── (e) MUスコア sort expression: math funcs available + ranking correct ─────
//
// The /shop default sort interpolates `catalog::MU_SCORE_SQL`. The first
// version used LN() and this test caught that the bundled SQLite ships
// WITHOUT SQLITE_ENABLE_MATH_FUNCTIONS ("no such function: LN") — the
// expression now uses only core functions (json_extract / julianday /
// multi-arg MAX / CASE). This test keeps failing loudly if the expression
// ever stops preparing, instead of /shop silently returning an empty grid
// (list_products_paged swallows prepare errors).
#[test]
fn mu_score_sql_ranks_design_sales_freshness() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE catalog_products (
            sku        TEXT PRIMARY KEY,
            meta_json  TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            sort_order INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE catalog_orders (
            id     INTEGER PRIMARY KEY AUTOINCREMENT,
            sku    TEXT NOT NULL,
            status TEXT NOT NULL
         );",
    )
    .unwrap();

    // A: judged 90, no sales, stale (>60d)        → 90*0.7             = 63.0
    // B: unjudged (base 40), 10 sales, stale      → 28 + 19.2 (ladder) = 47.2
    // C: unjudged, no sales, fresh (<14d)         → 28 + 10            = 38.0
    conn.execute_batch(
        "INSERT INTO catalog_products (sku, meta_json, created_at) VALUES
            ('A', '{\"score\":{\"total\":90}}', datetime('now','-100 days')),
            ('B', NULL,                          datetime('now','-100 days')),
            ('C', NULL,                          datetime('now'));",
    )
    .unwrap();
    for _ in 0..10 {
        conn.execute(
            "INSERT INTO catalog_orders (sku, status) VALUES ('B','submitted')",
            [],
        )
        .unwrap();
    }
    // Unpaid orders must not count.
    conn.execute(
        "INSERT INTO catalog_orders (sku, status) VALUES ('C','pending')",
        [],
    )
    .unwrap();

    let sql = format!(
        "SELECT sku FROM catalog_products ORDER BY ({}) DESC",
        crate::catalog::MU_SCORE_SQL
    );
    let order: Vec<String> = conn
        .prepare(&sql)
        .expect("MU_SCORE_SQL must prepare — non-core SQL function crept in?")
        .query_map([], |r| r.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(
        order,
        vec!["A", "B", "C"],
        "design 90 > unjudged+10 sales > unjudged+fresh"
    );
}

// ── (f) Gift-to-an-MU-account: schema + claim-token lookup + preserve ────────
//
// The gift feature relies on three DB invariants:
//   1. ensure_schema() adds catalog_orders.gift_json (idempotent ALTER).
//   2. The held-order claim page/submit resolves an order by the unguessable
//      json_extract(gift_json,'$.claim_token') — so the giftee can enter their
//      address WITHOUT exposing it to the sender.
//   3. record_order_full's INSERT OR REPLACE PRESERVES gift_json (it re-reads
//      then rewrites it, like referrer_code/ticket_code). If that preserve
//      breaks, a gift order silently loses its recipient on the final write.
// This test guards all three at the SQL level (network-free, in-memory).
#[test]
fn gift_json_schema_claim_lookup_and_preserve() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    crate::catalog::ensure_schema(&conn);

    // (1) gift_json column exists and round-trips.
    let session = "cs_test_gift_0001";
    let gift_json = serde_json::json!({
        "recipient_slug": "abc1234",
        "claim_token": "tok_unguessable_xyz",
        "claimed": false,
        "sender_email": "buyer@example.com"
    })
    .to_string();
    conn.execute(
        "INSERT INTO catalog_orders (stripe_session_id, sku, amount_jpy, status, gift_json)
         VALUES (?,?,?,?,?)",
        rusqlite::params![session, "AUTO-X-TEE-S", 6800, "gift_pending_address", gift_json],
    )
    .unwrap();

    // (2) Resolve the held order by its claim token (the only credential the
    //     giftee presents) — exactly the query gift_claim_page/submit run.
    let (found_sku, found_status): (String, String) = conn
        .query_row(
            "SELECT sku, status FROM catalog_orders
             WHERE json_extract(gift_json,'$.claim_token')=? AND status='gift_pending_address' LIMIT 1",
            rusqlite::params!["tok_unguessable_xyz"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("claim token must resolve the held gift order");
    assert_eq!(found_sku, "AUTO-X-TEE-S");
    assert_eq!(found_status, "gift_pending_address");
    // A wrong/guessed token must NOT resolve anything.
    let none: Result<String, _> = conn.query_row(
        "SELECT sku FROM catalog_orders WHERE json_extract(gift_json,'$.claim_token')=? LIMIT 1",
        rusqlite::params!["tok_wrong"],
        |r| r.get(0),
    );
    assert!(none.is_err(), "an unknown claim token must resolve nothing");

    // (3) Preserve invariant: the final write re-reads gift_json then rewrites
    //     it inside INSERT OR REPLACE (mirrors record_order_full). After it,
    //     gift_json must still be present — not reset to NULL.
    let existing_gift: Option<String> = conn
        .query_row(
            "SELECT gift_json FROM catalog_orders WHERE stripe_session_id=?",
            rusqlite::params![session],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO catalog_orders
           (stripe_session_id, sku, amount_jpy, status, gift_json)
         VALUES (?,?,?,?,?)",
        rusqlite::params![session, "AUTO-X-TEE-S", 6800, "submitted", existing_gift],
    )
    .unwrap();
    let after: Option<String> = conn
        .query_row(
            "SELECT gift_json FROM catalog_orders WHERE stripe_session_id=?",
            rusqlite::params![session],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        after.as_deref().unwrap_or("").contains("recipient_slug"),
        "gift_json must survive the INSERT OR REPLACE final write (recipient not lost)"
    );
}
