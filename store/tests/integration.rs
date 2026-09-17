// Integration tests for `mu-store`.
//
// IMPORTANT: src/main.rs is a single ~68k-line bin crate (no lib target),
// so its internal `fn`s are not callable from this `tests/` directory.
// Instead, these tests spawn the compiled `mu-store` release binary
// against a fresh, throw-away SQLite DB on a free localhost port,
// then exercise it with blocking HTTP. Each test owns its server.
//
// Covers the recent feature surface called out in the task brief:
//   - /sitemap.xml: returns 200 + an XML body containing <urlset>
//   - /p/<invalid>: returns 404 ("product not found" path)
//   - /p/<unknown numeric id>: returns 404 (no matching row in empty DB)
//   - /merch/:category: returns 200 even with no products (empty grid)
//
// No external services are hit. No production DB is touched (tempfile
// gives us a per-test sqlite file). The binary is *not* rebuilt by these
// tests — `cargo test --release` builds it as a normal step of the
// test pipeline, so the freshly built artifact is what we spawn.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Locate the freshly built `mu-store` binary. `cargo test --release`
/// builds it under target/release/. Falls back to target/debug/ for
/// plain `cargo test` runs.
fn locate_binary() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let candidates = [
        format!("{}/target/release/mu-store", manifest_dir),
        format!("{}/target/debug/mu-store", manifest_dir),
    ];
    for c in &candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return p;
        }
    }
    panic!(
        "mu-store binary not found in target/{{release,debug}}/. \
         Run `cargo build` or `cargo build --release` first."
    );
}

/// Pick an unused TCP port by binding to 0 then immediately dropping.
/// There is an inherent TOCTOU race here but it's the standard trick
/// and good enough for our serialised test runs.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    listener.local_addr().unwrap().port()
}

/// Spawn `mu-store` against a fresh sqlite DB. Returns the child handle
/// and the base URL once the server starts answering HTTP. Caller must
/// kill the child when done (RAII guard `ServerGuard` below).
struct ServerGuard {
    child: Child,
    base: String,
    // Keep tempdir alive for the test lifetime — drop = rm -rf.
    _db_dir: tempfile::TempDir,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_server() -> ServerGuard {
    let bin = locate_binary();
    let port = free_port();
    let db_dir = tempfile::tempdir().expect("tempdir");
    let db_path = db_dir.path().join("products.db");

    // cwd must be `store/` so the binary can read static/constitution.md
    // (validate_constitution panics otherwise) and static/sitemap.xml.
    let cwd = env!("CARGO_MANIFEST_DIR");

    let child = Command::new(&bin)
        .current_dir(cwd)
        .env("PORT", port.to_string())
        .env("DB_PATH", db_path.to_string_lossy().to_string())
        // Quiet the noisy startup logs in test output.
        .env("RUST_LOG", "error")
        // Belt-and-braces: keep agents off in case the bin starts any.
        .env("AGENT_KILL_ALL", "1")
        .env("DRY_RUN_ALL", "1")
        .env("MU_AUTOPILOT", "0")
        .env("ADMIN_TOKEN", "integration-test-only")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mu-store");

    let base = format!("http://127.0.0.1:{}", port);

    // Poll for readiness. Startup includes opening the DB + running a
    // wad of CREATE TABLE IF NOT EXISTS — usually <2s, but be generous.
    let deadline = Instant::now() + Duration::from_secs(30);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let probe = format!("{}/sitemap.xml", base);
    loop {
        if Instant::now() > deadline {
            panic!("mu-store did not become ready within 30s");
        }
        match client.get(&probe).send() {
            Ok(r) if r.status().is_success() || r.status().is_redirection() => break,
            // 4xx counts as "process is listening" too — sitemap should be 200
            // but if a future change moves it, we still know the server is up.
            Ok(r) if r.status().as_u16() < 500 => break,
            _ => std::thread::sleep(Duration::from_millis(150)),
        }
    }

    ServerGuard {
        child,
        base,
        _db_dir: db_dir,
    }
}

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

#[test]
fn prompt_purchase_status_uses_real_schema_and_requires_admin() {
    use serde_json::{json, Value};
    let srv = start_server();
    let http = client();
    let url = format!("{}/admin/catalog/status", srv.base);
    assert_eq!(http.get(&url).query(&[("token","wrong-token")]).send().unwrap().status().as_u16(),401);
    let read_status = || -> Value {
        let response = http.get(&url).query(&[("token","integration-test-only")]).send().unwrap();
        assert!(response.status().is_success());
        response.json::<Value>().unwrap()["prompt_selection"].clone()
    };
    let status = read_status();
    assert_eq!(status["minimum_purchase_uu"],4);
    assert_eq!(status["policy"],"explore-20-v1");
    assert_eq!(status["exploration_percent"],20);
    assert_eq!(status["adopted_percent"],80);
    assert_eq!(status["effective_exploration_percent"],100);
    assert_eq!(status["patterns"].as_array().unwrap().len(),10);
    for pattern in status["patterns"].as_array().unwrap() {
        assert_eq!(pattern["purchase_uu"],0);
        assert_eq!(pattern["adopted"],false);
    }
    let conn = rusqlite::Connection::open(srv._db_dir.path().join("products.db")).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    for n in 0..4 {
        let id = format!("cs_local_fixture_{n}");
        let session = json!({"id":id,"livemode":true,"amount_total":4900,"payment_status":"paid",
            "customer_details":{"email":format!("fixture{n}@example.test")},
            "payment_intent":{"status":"succeeded","latest_charge":{"paid":true,
                "captured":true,"status":"succeeded","amount_refunded":0,"refunded":false}}});
        let meta = json!({"prompt_selection":{"experiment":"design-prompts-v1","pattern_id":"minimal"}});
        let spec = json!({"lines":[{"sku":"fixture","qty":1,"unit_amount":4900,"meta_json":meta.to_string()}]});
        conn.execute("INSERT INTO catalog_orders(stripe_session_id,payment_status,session_json,checkout_spec_json,amount_jpy,status)
            VALUES (?,'paid',?,?,4900,'submitted')",rusqlite::params![id,session.to_string(),spec.to_string()]).unwrap();
    }
    let status = read_status();
    assert_eq!(status["patterns"][0]["purchase_uu"],4);
    assert_eq!(status["patterns"][0]["adopted"],true);
    assert_eq!(status["effective_exploration_percent"],20);
    assert_eq!(status["effective_adopted_percent"],80);
    assert!(!status.to_string().contains("example.test"));
    conn.execute("UPDATE catalog_orders SET payment_status='refunded',status='refunded' WHERE stripe_session_id='cs_local_fixture_3'",[]).unwrap();
    let status = read_status();
    assert_eq!(status["patterns"][0]["purchase_uu"],3);
    assert_eq!(status["patterns"][0]["adopted"],false);
    assert_eq!(status["effective_exploration_percent"],100);
}

#[test]
fn sitemap_xml_returns_urlset() {
    let srv = start_server();
    let resp = client()
        .get(format!("{}/sitemap.xml", srv.base))
        .send()
        .expect("GET /sitemap.xml");
    assert!(
        resp.status().is_success(),
        "expected 2xx from /sitemap.xml, got {}",
        resp.status()
    );
    let ctype = resp
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = resp.text().expect("body");
    assert!(
        ctype.contains("xml") || body.contains("<?xml"),
        "expected XML content-type or prologue, got ctype={:?} body_head={:?}",
        ctype,
        &body.chars().take(80).collect::<String>()
    );
    assert!(
        body.contains("<urlset"),
        "expected <urlset> in body, got: {}",
        &body.chars().take(200).collect::<String>()
    );
}

#[test]
fn original_collection_routes_keep_their_spa_after_homepage_renewal() {
    let srv = start_server();
    let http = client();
    for lang in ["ja", "en"] {
        let home = http.get(format!("{}/?lang={lang}", srv.base))
            .send().unwrap().text().unwrap();
        assert!(home.contains("class=\"mu-storefront\""));
        assert!(!home.contains("id=\"originals\""));
        for brand in ["mugen", "ma", "muon"] {
            let response = http.get(format!("{}/{brand}?lang={lang}", srv.base))
                .send().unwrap();
            assert!(response.status().is_success());
            let html = response.text().unwrap();
            assert!(html.contains("function initRouter()"), "{brand} must retain its section router");
            assert!(!html.contains("class=\"mu-storefront\""), "{brand} must not render the generic homepage");
        }
    }
}

#[test]
fn product_page_unknown_sku_is_404() {
    let srv = start_server();
    let resp = client()
        .get(format!("{}/p/MU-DOES-NOT-EXIST-XYZ", srv.base))
        .send()
        .expect("GET /p/<invalid>");
    assert_eq!(
        resp.status().as_u16(),
        404,
        "expected 404 for unknown serial code"
    );
}

#[test]
fn product_page_unknown_numeric_id_is_404() {
    let srv = start_server();
    let resp = client()
        .get(format!("{}/p/99999999", srv.base))
        .send()
        .expect("GET /p/<unknown numeric>");
    assert_eq!(
        resp.status().as_u16(),
        404,
        "expected 404 for unknown numeric id"
    );
}

#[test]
fn merch_category_renders_with_empty_db() {
    let srv = start_server();
    let resp = client()
        .get(format!("{}/merch/bjj", srv.base))
        .send()
        .expect("GET /merch/bjj");
    // An empty catalogue should still render the category shell, not 5xx.
    // We accept 200 or a redirect (e.g. if /merch/bjj 301s to a canonical
    // form in some future refactor).
    let s = resp.status().as_u16();
    assert!(
        s == 200 || (300..=399).contains(&s),
        "expected 2xx/3xx from /merch/bjj, got {}",
        s
    );
}
