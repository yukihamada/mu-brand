//! Fulfillment evidence is independent from the customer's payment/refund.
use serde_json::{json, Value};

pub fn summary(order: &Value) -> Value {
    let raw = order["status"].as_str().unwrap_or("unknown");
    let items = order["items"].as_array().cloned().unwrap_or_default();
    let shipments = order["shipments"].as_array().cloned().unwrap_or_default();
    let mut ordered = 0u64;
    let mut shipped = 0u64;
    let mut delivered = 0u64;
    let mut canceled = 0u64;
    let mut held = false;
    for item in &items {
        let quantity = item["quantity"].as_u64().unwrap_or(0);
        ordered += quantity;
        let mut shipped_item = 0;
        let mut delivered_item = 0;
        let mut canceled_item = 0;
        for shipment in &shipments {
            let status = shipment["status"].as_str().unwrap_or("");
            held |= matches!(status, "onhold" | "failed" | "returned");
            let Some(parts) = shipment["items"].as_array() else { continue; };
            for part in parts {
                if item["id"].is_null() || part["item_id"] != item["id"] { continue; }
                let n = part["quantity"].as_u64().unwrap_or(0);
                if matches!(status, "canceled" | "cancelled") { canceled_item += n; continue; }
                if matches!(status, "returned" | "failed") { continue; }
                if shipment["delivered_at"].as_i64().is_some_and(|t| t > 0) {
                    delivered_item += n;
                    shipped_item += n;
                } else if shipment["shipped_at"].as_i64().is_some_and(|t| t > 0) {
                    shipped_item += n;
                }
            }
        }
        shipped += shipped_item.min(quantity);
        delivered += delivered_item.min(quantity);
        canceled += canceled_item.min(quantity.saturating_sub(delivered_item));
    }
    let state = if matches!(raw, "canceled" | "cancelled") { "vendor_canceled" }
        else if canceled > 0 { "partially_canceled" }
        else if held { "on_hold" }
        else if ordered > 0 && delivered == ordered { "delivered" }
        else if delivered > 0 { "partially_delivered" }
        else if ordered > 0 && shipped == ordered { "shipped" }
        else if shipped > 0 { "partially_shipped" }
        else if matches!(raw, "inprocess" | "in_production") { "in_production" }
        else if raw == "pending" { "accepted" }
        else if raw == "failed" { "failed" }
        else if raw == "draft" { "draft" }
        else { "unconfirmed" };
    json!({"vendor_status":raw,"fulfillment_status":state,"ordered_quantity":ordered,
        "shipped_quantity":shipped,"delivered_quantity":delivered,"canceled_quantity":canceled,
        "customer_refund_status":"unverified","customer_receipt_confirmed":false,
        "shipments":shipments.iter().map(|s| json!({"id":s["id"],"status":s["status"],
            "shipped_at":s["shipped_at"],"delivered_at":s["delivered_at"]})).collect::<Vec<_>>()})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn order() -> Value {
        json!({"status":"fulfilled","items":[{"id":1,"quantity":2}],
            "shipments":[{"id":10,"status":"shipped","shipped_at":100,"items":[{"item_id":1,"quantity":2}]}]})
    }
    #[test]
    fn fulfilled_is_shipped_not_delivered_and_refunds_are_separate() {
        let mut o = order();
        assert_eq!(summary(&o)["fulfillment_status"], "shipped");
        o["shipments"][0]["delivered_at"] = json!(200);
        assert_eq!(summary(&o)["fulfillment_status"], "delivered");
        o["status"] = json!("canceled");
        assert_eq!(summary(&o)["fulfillment_status"], "vendor_canceled");
        assert_eq!(summary(&o)["customer_refund_status"], "unverified");
    }
    #[test]
    fn mixed_cancellation_cannot_claim_all_items_delivered() {
        let mut o = order();
        o["items"].as_array_mut().unwrap().push(json!({"id":2,"quantity":1}));
        o["shipments"][0]["delivered_at"] = json!(200);
        o["shipments"].as_array_mut().unwrap().push(json!({"status":"canceled","items":[{"item_id":2,"quantity":1}]}));
        let s = summary(&o);
        assert_eq!(s["fulfillment_status"], "partially_canceled");
        assert_eq!(s["delivered_quantity"], 2);
        assert_eq!(s["ordered_quantity"], 3);
    }
    #[test]
    fn partial_missing_and_held_evidence_stays_incomplete() {
        let mut o = order();
        o["shipments"][0]["items"][0]["quantity"] = json!(1);
        assert_eq!(summary(&o)["fulfillment_status"], "partially_shipped");
        o["shipments"][0]["status"] = json!("onhold");
        assert_eq!(summary(&o)["fulfillment_status"], "on_hold");
        o["shipments"] = json!([]);
        assert_eq!(summary(&o)["fulfillment_status"], "unconfirmed");
        o["status"] = json!("pending");
        assert_eq!(summary(&o)["fulfillment_status"], "accepted");
        assert_eq!(summary(&json!({}))["fulfillment_status"], "unconfirmed");
    }
    #[test]
    fn duplicate_delivered_quantity_cannot_cover_a_different_item() {
        let mut o=order();
        o["items"].as_array_mut().unwrap().push(json!({"id":2,"quantity":1}));
        o["shipments"][0]["items"][0]["quantity"]=json!(3);
        o["shipments"][0]["delivered_at"]=json!(200);
        assert_eq!(summary(&o)["fulfillment_status"],"partially_delivered");
    }
}
