#!/usr/bin/env python3
"""Inject `<script src="/pt_gate.js" defer></script>` into every public
static HTML page so the 30pt unlock widget + discoverability badge load
site-wide. Admin pages and the buy flow (own checkout UI) are skipped.

Idempotent — skips files that already reference pt_gate.js.

Usage:
  python3 scripts/inject_pt_gate.py [--check]
"""
import argparse, sys, re
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
STATIC = REPO / "store" / "static"

# admin pages: own auth flow, badge irrelevant.
# buy.html: own Stripe checkout, the badge would compete with the buy CTA.
# 404.html: error page, no UX value.
SKIP_PATTERNS = ("admin-", "buy.html", "404.html")

TAG = '<script src="/pt_gate.js" defer></script>'
MARKER = "/pt_gate.js"  # idempotency check


def should_skip(p: Path) -> bool:
    return any(p.name.startswith(s) or p.name == s for s in SKIP_PATTERNS)


def inject(html: str) -> str | None:
    if MARKER in html:
        return None  # already present
    # Prefer placement just before </body>; fall back to </html>.
    m = re.search(r"(?i)</body\s*>", html)
    if m:
        return html[: m.start()] + "  " + TAG + "\n" + html[m.start():]
    m = re.search(r"(?i)</html\s*>", html)
    if m:
        return html[: m.start()] + TAG + "\n" + html[m.start():]
    # No close tag — append.
    return html.rstrip() + "\n" + TAG + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true",
                    help="report what would change without writing")
    args = ap.parse_args()

    files = sorted(STATIC.glob("*.html"))
    injected, already, skipped = [], [], []
    for f in files:
        if should_skip(f):
            skipped.append(f.name); continue
        html = f.read_text()
        new = inject(html)
        if new is None:
            already.append(f.name); continue
        if not args.check:
            f.write_text(new)
        injected.append(f.name)

    print(f"injected ({len(injected)}):")
    for n in injected: print(f"  + {n}")
    print(f"already had it ({len(already)}):")
    for n in already: print(f"  = {n}")
    print(f"skipped ({len(skipped)}):")
    for n in skipped: print(f"  - {n}")
    if args.check:
        print("[--check] no files modified")


if __name__ == "__main__":
    main()
