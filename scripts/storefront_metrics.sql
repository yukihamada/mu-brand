-- Read-only browser-intent cohort. NOT purchase CVR; server events use unrelated IDs.
-- Exclude verification sessions. Run on the production DB with sqlite3 -readonly.
WITH browser AS (
  SELECT session_id, event, path, CAST(created_at AS INTEGER) AS ts,
         CASE WHEN json_valid(extra) THEN json_extract(extra,'$.ab') END AS release
  FROM funnel_events
  WHERE visitor_id NOT LIKE 'server:%'
    AND visitor_id NOT LIKE 'renewal-local-%'
    AND CAST(created_at AS INTEGER) >= strftime('%s','now','-14 days')
), cohort AS (
  SELECT session_id, MIN(ts) AS entered
  FROM browser
  WHERE event='pageview' AND path='/' AND release='storefront-20260917'
  GROUP BY session_id
), per_session AS (
  SELECT c.session_id,
         MAX(CASE WHEN b.event='pageview' AND b.path LIKE '/shop/%' THEN 1 ELSE 0 END) AS pdp,
         MAX(CASE WHEN b.event='checkout_attempt' THEN 1 ELSE 0 END) AS intent
  FROM cohort c LEFT JOIN browser b ON b.session_id=c.session_id AND b.ts>=c.entered
  GROUP BY c.session_id
)
SELECT COUNT(*) AS landing_sessions,
       COALESCE(SUM(pdp),0) AS product_view_sessions,
       COALESCE(SUM(intent),0) AS checkout_intent_sessions,
       ROUND(100.0*SUM(pdp)/NULLIF(COUNT(*),0),2) AS product_view_pct,
       ROUND(100.0*SUM(intent)/NULLIF(COUNT(*),0),2) AS checkout_intent_pct
FROM per_session;
