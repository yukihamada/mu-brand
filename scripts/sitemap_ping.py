#!/usr/bin/env python3
"""
sitemap_ping.py — notify Google + Bing that wearmu.com/sitemap.xml has changed.

Runs daily from cron.sh. Each ping is a simple GET to the search engine's
publicly documented sitemap-ping endpoint; no auth, no API key, no rate limit
on a once-a-day cadence. Exit code is always 0 — sitemap ping is a "nice to
have" and we never want a transient 503 to break the cron chain.

Background:
  Google docs: https://developers.google.com/search/docs/crawling-indexing/sitemaps/build-sitemap#submit-sitemap
  Bing docs:   https://www.bing.com/webmasters/help/Sitemaps-3b5cf6ed
"""
from __future__ import annotations
import sys
import time
import urllib.parse
import urllib.request
from datetime import datetime, timezone

SITEMAP_URL = "https://wearmu.com/sitemap.xml"
ENGINES = [
    ("google", "https://www.google.com/ping?sitemap="),
    ("bing",   "https://www.bing.com/ping?sitemap="),
]
TIMEOUT_S = 15


def _ts() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def ping(name: str, base: str, sitemap: str) -> None:
    url = base + urllib.parse.quote(sitemap, safe="")
    started = time.time()
    try:
        req = urllib.request.Request(
            url,
            headers={
                "User-Agent": "wearmu-sitemap-ping/1.0 (+https://wearmu.com)",
            },
        )
        with urllib.request.urlopen(req, timeout=TIMEOUT_S) as resp:
            status = resp.status
            body_len = len(resp.read(2048))
        dt_ms = int((time.time() - started) * 1000)
        print(f"[{_ts()}] {name:6s} status={status} bytes={body_len} took={dt_ms}ms url={url}")
    except Exception as e:  # noqa: BLE001 — daily cron; never want to crash
        dt_ms = int((time.time() - started) * 1000)
        print(f"[{_ts()}] {name:6s} ERROR took={dt_ms}ms url={url} err={type(e).__name__}: {e}")


def main() -> int:
    print(f"[{_ts()}] sitemap_ping start sitemap={SITEMAP_URL}")
    for name, base in ENGINES:
        ping(name, base, SITEMAP_URL)
    print(f"[{_ts()}] sitemap_ping done")
    return 0


if __name__ == "__main__":
    # Never propagate non-zero — cron must stay green.
    try:
        sys.exit(main())
    except Exception as e:  # noqa: BLE001
        print(f"[{_ts()}] sitemap_ping fatal {type(e).__name__}: {e}", file=sys.stderr)
        sys.exit(0)
