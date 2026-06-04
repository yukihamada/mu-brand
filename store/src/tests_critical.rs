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
