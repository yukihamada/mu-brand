//! Shipment observation is independent of purchases, AI generation and SNS.
use crate::{chrono_now, fulfillment_status, Db};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

fn key(pid: i64, order_id: &str) -> String {
    format!("fulfillment_observation:{pid}:{order_id}")
}

pub fn observation(conn: &rusqlite::Connection, pid: i64, order_id: &str) -> Result<Option<Value>, String> {
    let value: Option<String> = conn.query_row("SELECT value FROM cv_config WHERE key=?",
        [key(pid, order_id)], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
    value.map(|v| serde_json::from_str(&v).map_err(|e| e.to_string())).transpose()
}

pub async fn poll(db: &Db, client: &reqwest::Client, base: &str, token: &str) -> Result<usize, String> {
    let pending: Vec<(i64, String)> = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        // Fulfilled/canceled orders still need observation, including replacements.
        let mut stmt = conn.prepare(
            "SELECT p.id, p.printful_order_id FROM mu_purchases p
             LEFT JOIN cv_config c ON c.key='fulfillment_attempt:' || p.id || ':' || p.printful_order_id
             WHERE p.printful_order_id IS NOT NULL AND p.printful_order_id != ''
             ORDER BY COALESCE(CAST(c.updated_at AS INTEGER),0), p.id LIMIT 20"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|e| e.to_string())?;
        rows.collect::<Result<_,_>>().map_err(|e| e.to_string())?
    };
    let mut failures = 0;
    let mut count = 0;
    for (pid, order_id) in pending {
        let result = async {
            let id = order_id.parse::<u64>().map_err(|_| "invalid vendor order id".to_string())?;
            let body: Value = client.get(format!("{base}/orders/{id}"))
                .bearer_auth(token).send().await.map_err(|e| e.to_string())?
                .error_for_status().map_err(|e| e.to_string())?
                .json().await.map_err(|e| e.to_string())?;
            let order = &body["result"];
            if order["id"].as_u64() != Some(id) || order["status"].as_str().is_none_or(str::is_empty) {
                return Err("mismatched or incomplete vendor response".to_string());
            }
            let now = chrono_now();
            let snapshot = json!({"order_id":order_id,"observed_at":now,"fulfillment":fulfillment_status::summary(order)});
            let mut conn = db.lock().map_err(|e| e.to_string())?;
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            // A concurrent replacement must not inherit an old order's status.
            let updated = tx.execute("UPDATE mu_purchases SET last_printful_status=?,last_status_at=? WHERE id=? AND printful_order_id=?",
                params![order["status"].as_str().unwrap(),now,pid,order_id]).map_err(|e| e.to_string())?;
            if updated > 0 {
                tx.execute("INSERT INTO cv_config(key,value,updated_at,reason) VALUES(?,?,?,'shipment observation')
                    ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at,reason=excluded.reason",
                    params![key(pid,&order_id),snapshot.to_string(),now]).map_err(|e| e.to_string())?;
            }
            tx.commit().map_err(|e| e.to_string())?;
            Ok::<_, String>(updated)
        }.await;
        match result {
            Ok(n) => count += n,
            Err(e) => { failures += 1; tracing::warn!(purchase_id=pid,error=%e,"shipment observation failed"); }
        }
        // Rotate even failed orders so twenty permanent failures cannot starve
        // every later purchase. Observation timestamps still mean success only.
        let conn = db.lock().map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO cv_config(key,value,updated_at,reason) VALUES(?,'attempted',?,'shipment poll attempt')
            ON CONFLICT(key) DO UPDATE SET updated_at=excluded.updated_at",
            params![format!("fulfillment_attempt:{pid}:{order_id}"),chrono_now()]).map_err(|e| e.to_string())?;
    }
    if failures > 0 { return Err(format!("{failures} observations failed; {count} updated")); }
    Ok(count)
}

pub async fn run(db: Db) {
    let Ok(token) = std::env::var("PRINTFUL_API_KEY") else { return; };
    if token.is_empty() { return; }
    let client = match reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build() {
        Ok(c) => c,
        Err(e) => { tracing::error!(error=%e,"shipment observer could not start"); return; }
    };
    loop {
        if let Err(e) = poll(&db,&client,"https://api.printful.com",&token).await {
            tracing::warn!(error=%e,"shipment observation batch incomplete");
        }
        tokio::time::sleep(std::time::Duration::from_secs(600)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Json, Router};
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn actual_http_to_sqlite_observes_shipments_without_new_purchases() {
        let conn=rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE mu_purchases(id INTEGER PRIMARY KEY,printful_order_id TEXT,last_printful_status TEXT,last_status_at TEXT);
            CREATE TABLE cv_config(key TEXT PRIMARY KEY,value TEXT NOT NULL,updated_at TEXT NOT NULL,reason TEXT);
            INSERT INTO mu_purchases VALUES(1,'100','fulfilled','0');").unwrap();
        let db=Arc::new(Mutex::new(conn));
        let response=Arc::new(Mutex::new(json!({"result":{"id":100,"status":"fulfilled","items":[{"id":1,"quantity":1}],
            "shipments":[{"id":10,"shipped_at":100,"items":[{"item_id":1,"quantity":1}]}]}})));
        let value=response.clone();
        let app=Router::new().route("/orders/:id",get(move || {let v=value.clone();async move{Json(v.lock().unwrap().clone())}}));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base=format!("http://{}",listener.local_addr().unwrap());
        let server=tokio::spawn(async move {axum::serve(listener,app).await.unwrap();});
        let client=reqwest::Client::new();
        poll(&db,&client,&base,"fixture").await.unwrap();
        assert_eq!(observation(&db.lock().unwrap(),1,"100").unwrap().unwrap()["fulfillment"]["fulfillment_status"],"shipped");
        response.lock().unwrap()["result"]["shipments"][0]["delivered_at"]=json!(200);
        poll(&db,&client,&base,"fixture").await.unwrap();
        assert_eq!(observation(&db.lock().unwrap(),1,"100").unwrap().unwrap()["fulfillment"]["fulfillment_status"],"delivered");
        assert_eq!(db.lock().unwrap().query_row("SELECT last_printful_status FROM mu_purchases",[],|r|r.get::<_,String>(0)).unwrap(),"fulfilled");
        response.lock().unwrap()["result"]["items"].as_array_mut().unwrap().push(json!({"id":2,"quantity":1}));
        response.lock().unwrap()["result"]["shipments"].as_array_mut().unwrap().push(json!({"id":11,"status":"canceled","items":[{"item_id":2,"quantity":1}]}));
        poll(&db,&client,&base,"fixture").await.unwrap();
        assert_eq!(observation(&db.lock().unwrap(),1,"100").unwrap().unwrap()["fulfillment"]["fulfillment_status"],"partially_canceled");
        // Replacement IDs have separate evidence and cannot reuse old delivery.
        db.lock().unwrap().execute("UPDATE mu_purchases SET printful_order_id='101'",[]).unwrap();
        assert!(observation(&db.lock().unwrap(),1,"101").unwrap().is_none());
        assert!(poll(&db,&client,&base,"fixture").await.is_err());
        response.lock().unwrap()["result"]["id"]=json!(101);
        response.lock().unwrap()["result"]["status"]=json!("canceled");
        poll(&db,&client,&base,"fixture").await.unwrap();
        assert_eq!(observation(&db.lock().unwrap(),1,"101").unwrap().unwrap()["fulfillment"]["fulfillment_status"],"vendor_canceled");
        server.abort();
    }
}
