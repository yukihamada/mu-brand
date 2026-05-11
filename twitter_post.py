#!/usr/bin/env python3
"""
MU — auto X (Twitter) poster.

Hourly cron flow:
  1. GET /api/admin/x_queue?token=&limit=4 → list of un-posted drops
  2. For each, build tweet (name + image + URL)
  3. POST to Twitter v2 (uses tweepy with OAuth 1.0a user context)
  4. POST /api/admin/x_mark_posted (idempotency)

Env required (all in ~/.env):
  TWITTER_API_KEY            (consumer key)
  TWITTER_API_SECRET         (consumer secret)
  TWITTER_ACCESS_TOKEN       (user access token)
  TWITTER_ACCESS_TOKEN_SECRET
  MU_ADMIN_TOKEN

If any X env is missing the script exits 0 with a log line — cron stays
clean even before tokens are provisioned.
"""
import os, sys, json, urllib.request, urllib.parse, io
from pathlib import Path

STORE_URL   = os.environ.get("MU_STORE_URL", "https://wearmu.com")
ADMIN_TOKEN = os.environ.get("MU_ADMIN_TOKEN", "mu-admin-2026")
LIMIT       = int(os.environ.get("X_POST_LIMIT", "3"))

X_KEY    = os.environ.get("TWITTER_API_KEY", "")
X_SEC    = os.environ.get("TWITTER_API_SECRET", "")
X_TOKEN  = os.environ.get("TWITTER_ACCESS_TOKEN", "")
X_TSEC   = os.environ.get("TWITTER_ACCESS_TOKEN_SECRET", "")

if not all([X_KEY, X_SEC, X_TOKEN, X_TSEC]):
    print("twitter_post: TWITTER_* env vars not set — exiting (no-op).")
    sys.exit(0)

try:
    import tweepy
except ImportError:
    print("twitter_post: tweepy not installed (pip install tweepy). Exiting 0.")
    sys.exit(0)

import requests

def fetch_queue():
    url = f"{STORE_URL}/api/admin/x_queue?token={urllib.parse.quote(ADMIN_TOKEN)}&limit={LIMIT}"
    r = requests.get(url, timeout=20)
    r.raise_for_status()
    return r.json().get("items", [])

def mark_posted(product_id, tweet_id):
    r = requests.post(
        f"{STORE_URL}/api/admin/x_mark_posted",
        json={"admin_token": ADMIN_TOKEN, "product_id": product_id, "tweet_id": tweet_id},
        timeout=15,
    )
    print(f"  mark_posted({product_id}) → {r.status_code} {r.text[:120]}")

def build_tweet_text(item):
    brand_label = {"mugen": "MUGEN", "muon": "MUON", "ma": "間 MA"}.get(item["brand"], item["brand"].upper())
    price = item.get("price_jpy") or 0
    yen = f"¥{price:,}" if price else ""
    return (
        f"{brand_label} · {item['name']}\n"
        f"{yen} · 北海道の天気がデザインした T シャツ\n"
        f"{item['url']}\n"
        f"\n"
        f"#MU #AIfashion #無人ブランド"
    )

def post_one(item, client_v2, api_v1):
    text = build_tweet_text(item)
    media_id = None
    img_url = item.get("image_url") or ""
    if img_url and img_url.startswith("http"):
        try:
            resp = requests.get(img_url, timeout=30)
            if resp.status_code == 200 and len(resp.content) > 1000:
                # tweepy v1.1 media upload
                media = api_v1.media_upload(filename=f"{item['id']}.jpg",
                                            file=io.BytesIO(resp.content))
                media_id = media.media_id_string
        except Exception as e:
            print(f"  media upload failed (post text-only): {e}")
    kw = {"text": text}
    if media_id:
        kw["media_ids"] = [media_id]
    res = client_v2.create_tweet(**kw)
    return res.data.get("id") if res and res.data else None

def main():
    items = fetch_queue()
    print(f"twitter_post: queue size = {len(items)}")
    if not items:
        return
    auth = tweepy.OAuth1UserHandler(X_KEY, X_SEC, X_TOKEN, X_TSEC)
    api_v1   = tweepy.API(auth)
    client_v2 = tweepy.Client(
        consumer_key=X_KEY, consumer_secret=X_SEC,
        access_token=X_TOKEN, access_token_secret=X_TSEC,
    )
    for it in items:
        try:
            print(f"\n[#{it['drop_num']} {it['brand']}] {it['name']}")
            tid = post_one(it, client_v2, api_v1)
            if tid:
                print(f"  posted: https://x.com/wearmu/status/{tid}")
                mark_posted(it["id"], str(tid))
            else:
                print("  posted but no tweet_id returned; skipping mark")
        except Exception as e:
            print(f"  FAILED: {type(e).__name__}: {e}")

if __name__ == "__main__":
    main()
