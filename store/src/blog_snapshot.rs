//! The daily log is an evidence-only snapshot, not an invented daily delta.
use serde_json::{json, Value};

pub const MODEL: &str = "snapshot-template-v1";

pub fn article(stats: &Value) -> Result<Value, String> {
    let day = stats["day"].as_str().ok_or("missing snapshot day")?;
    let bytes = day.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-'
        || !bytes.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit()) {
        return Err("invalid snapshot day".into());
    }
    if stats["measurement_basis"] != "cumulative_and_current_snapshot"
        || stats["timezone"] != "Asia/Tokyo" {
        return Err("missing measurement basis or timezone".into());
    }
    let observed = stats["observed_at_unix"].as_i64().filter(|n| *n > 0)
        .ok_or("missing observation time")?;
    let local = observed.checked_add(9 * 3600).ok_or("invalid observation time")?;
    let (year, month, date) = crate::civil_from_days(local / 86_400);
    if day != format!("{year:04}-{month:02}-{date:02}") {
        return Err("day differs from observation time".into());
    }
    let count = |key: &str| -> Result<i64, String> {
        stats[key].as_i64().filter(|n| *n >= 0).ok_or_else(|| format!("invalid {key}"))
    };
    let purchases = count("purchases")?;
    let designs = count("designs_generated")?;
    let subscribers = count("subscribers")?;
    let photos = count("lifestyle_photos")?;
    let title = format!("{day} 記録 / Snapshot");
    let body = format!(
        "## 取得時点の記録 / Recorded snapshot\n\n\
         基準日 / Date: {day} (Asia/Tokyo)\n\
         取得時刻 / Observed at: {observed} (Unix seconds)\n\n\
         | 指標 / Metric | 件数 / Count | 集計範囲 / Scope |\n\
         | --- | ---: | --- |\n\
         | 購入記録 / Purchase records | {purchases} | 保存済み全期間 / All stored records |\n\
         | デザイン記録 / Design records | {designs} | 保存済み全期間 / All stored records |\n\
         | 購読登録 / Subscriptions | {subscribers} | 取得時点の未解除登録 / Currently subscribed records |\n\
         | 商品写真登録 / Product photo records | {photos} | 取得時点の写真URL登録済み商品 / Products with photo URLs |\n\n\
         ## 集計の範囲 / Measurement limits\n\n\
         上記は保存済みの累計と取得時点の状態です。当日の増加数ではありません。\n\
         These are cumulative stored records and current state, not additions for this day.\n\n\
         日次差分・変動の原因・今後の実施予定は、このデータでは確認できません。\n\
         Daily changes, their causes and future actions are not established by this data.\n\n\
         — MU / データから定型生成 / Generated from recorded data"
    );
    Ok(json!({"title": title, "body_md": body, "model": MODEL}))
}

pub fn validate(stats: &Value, slug: &str, title: &str, body: &str, now: i64) -> Result<(), String> {
    let expected = article(stats)?;
    let day = stats["day"].as_str().ok_or("missing snapshot day")?;
    if slug != format!("auto-{day}") {
        return Err("slug and observation day differ".into());
    }
    let observed = stats["observed_at_unix"].as_i64().ok_or("missing observation time")?;
    if observed > now || now - observed > 3600 {
        return Err("snapshot is stale or in the future".into());
    }
    if expected["title"].as_str() != Some(title) || expected["body_md"].as_str() != Some(body) {
        return Err("daily log must match the recorded snapshot template".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> Value {
        json!({"day":"2026-10-02", "measurement_basis":"cumulative_and_current_snapshot",
            "timezone":"Asia/Tokyo", "observed_at_unix":1790896200i64,
            "purchases":12, "designs_generated":234, "subscribers":20, "lifestyle_photos":8})
    }

    #[test]
    fn identifies_scope_in_both_languages_without_daily_claims() {
        let a = article(&stats()).unwrap();
        let body = a["body_md"].as_str().unwrap();
        assert!(body.contains("| 234 | 保存済み全期間 / All stored records |"));
        assert!(body.contains("not additions for this day"));
        assert!(body.contains("当日の増加数ではありません"));
        assert!((200..=8000).contains(&body.len()));
        validate(&stats(), "auto-2026-10-02", a["title"].as_str().unwrap(), body, 1790896201).unwrap();
    }

    #[test]
    fn missing_or_invalid_data_is_not_silently_zero() {
        for key in ["purchases", "designs_generated", "subscribers", "lifestyle_photos"] {
            for value in [Value::Null, json!(-1), json!(1.2), json!("3")] {
                let mut s = stats();
                s[key] = value;
                assert!(article(&s).is_err());
            }
        }
        for key in ["day", "measurement_basis", "timezone", "observed_at_unix"] {
            let mut s = stats();
            s.as_object_mut().unwrap().remove(key);
            assert!(article(&s).is_err());
        }
    }

    #[test]
    fn rejects_fabricated_claims_mismatched_dates_and_stale_inputs() {
        let s = stats();
        let a = article(&s).unwrap();
        let title = a["title"].as_str().unwrap();
        let body = a["body_md"].as_str().unwrap();
        for extra in ["\n今日234件を生成した。", "\n原因は割引。", "\n明日自動処理を実行する。"] {
            assert!(validate(&s, "auto-2026-10-02", title, &format!("{body}{extra}"), 1790896201).is_err());
        }
        assert!(validate(&s, "auto-2026-10-01", title, body, 1790896201).is_err());
        assert!(validate(&s, "auto-2026-10-02", title, body, 1790899801).is_err());
        assert!(validate(&s, "auto-2026-10-02", title, body, 1790896199).is_err());
        let mut relabeled = s.clone();
        relabeled["day"] = json!("2026-09-28");
        assert!(article(&relabeled).is_err());
    }
}
