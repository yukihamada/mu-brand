#!/usr/bin/env python3
"""ダッシュボード HTML 内の全 img src を並列 HEAD で実状況チェック.

出力: /tmp/wearmu_url_status.json — {url: status_code}

Usage:
    python3 scripts/check_dashboard_urls.py /tmp/wearmu_photos.html
"""
from __future__ import annotations
import concurrent.futures as cf
import json
import re
import sys
import urllib.request
from pathlib import Path

HTML = Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/wearmu_photos.html")
OUT = Path("/tmp/wearmu_url_status.json")


def head(url: str, timeout: float = 8.0) -> int:
    if url.startswith("file://"):
        local = url[len("file://"):]
        return 200 if Path(local).exists() else 404
    try:
        req = urllib.request.Request(url, method="HEAD",
                                     headers={"User-Agent": "wearmu-dash-check/1"})
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status
    except urllib.error.HTTPError as e:
        return e.code
    except Exception:
        return 0  # network err


def main():
    data = HTML.read_text(encoding="utf-8")
    urls = sorted(set(re.findall(r'src="([^"]+)"', data)))
    # filter to images
    urls = [u for u in urls if u.endswith((".png", ".jpg", ".jpeg", ".webp"))
            or "/mockups/" in u or "/lifestyle/" in u or "printful" in u or "design" in u]
    print(f"checking {len(urls):,} URLs (parallel=32)…")
    out = {}
    with cf.ThreadPoolExecutor(max_workers=32) as ex:
        for i, (u, code) in enumerate(zip(urls, ex.map(head, urls))):
            out[u] = code
            if (i + 1) % 500 == 0:
                print(f"  {i+1}/{len(urls)} done")
    OUT.write_text(json.dumps(out, indent=2))
    # summary
    by_code = {}
    for c in out.values():
        by_code[c] = by_code.get(c, 0) + 1
    print(f"\nstatus summary (n={len(out):,}):")
    for code in sorted(by_code):
        print(f"  {code}: {by_code[code]:,}")
    print(f"\nwrote {OUT}")


if __name__ == "__main__":
    main()
