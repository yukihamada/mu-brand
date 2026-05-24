#!/usr/bin/env python3
"""Contrado product-creation automation (skeleton).

Why this exists: Contrado's Helix API exposes /orders endpoints only — no
public /products or /catalog endpoint. To list a SKU on wearmu.com that
fulfills via Contrado we have to register the product on the Maker
Platform UI first, then submit orders referencing that SKU. This script
handles step 1 (the UI work) for the 5 belt-color rashguards so we can
A/B test Contrado against Printful as a fulfillment route.

Requires CONTRADO_EMAIL and CONTRADO_PASSWORD in /Users/yuki/.env (the
existing CONTRADO_API_KEY only authenticates the Helix API, not the
dashboard). If creds are missing the script exits non-zero with a clear
message instead of guessing — Contrado has CAPTCHA on login so blind
retries will lock the account.

Usage:
  # 1) Once: install Playwright browsers
  pip install playwright && playwright install chromium

  # 2) Add creds to /Users/yuki/.env:
  #    CONTRADO_EMAIL=mail@yukihamada.jp
  #    CONTRADO_PASSWORD=<dashboard pw>

  # 3) Run for one belt at a time (visible browser; --headless for CI)
  python3 contrado_create_product.py --belt blue
  python3 contrado_create_product.py --belt blue --headless

  # Outputs:
  #   /tmp/contrado_<belt>_step_NN_<name>.png  (screenshot per step)
  #   /tmp/contrado_<belt>_result.json         (new SKU + Maker COGS)

The first run is intentionally interactive — Contrado's editor flow
varies by product type and our selectors may be stale. Expect to iterate
the Page Object methods below after watching the first run live.
"""

from __future__ import annotations
import argparse
import json
import os
import sys
import time
from pathlib import Path

# ─── Belt → design URL (the 5 V3 canonical Gemini renders on R2) ───
BELT_DESIGNS = {
    "white":  "https://mockups.wearmu.com/catalog/AUTO-NL-WHITEBELT-RASHGUARD-LS-nl42cb43d6.png",
    "blue":   "https://mockups.wearmu.com/catalog/AUTO-NL-BLUEBELT-RASHGUARD-LS-nlf0f3d97c.png",
    "purple": "https://mockups.wearmu.com/catalog/AUTO-NL-PURPLEBELT-RASHGUARD-LS-nl5df1758d.png",
    "brown":  "https://mockups.wearmu.com/catalog/AUTO-NL-BROWNBELT-RASHGUARD-LS-nl33c9d2a3.png",
    "black":  "https://mockups.wearmu.com/catalog/AUTO-NL-BLACKBELT-RASHGUARD-BLACK-nl6d36782c.png",
}

# Tentative retail at premium tier — Contrado COGS is 2-3× Printful, so
# ¥9.8K won't break even. Pinning ¥19,800 for the trial; Maker UI will
# show "you earn" delta we can refine from.
RETAIL_JPY = 19_800

ENV_PATH = Path("/Users/yuki/.env")
SCREENSHOT_DIR = Path("/tmp")


def load_dotenv(path: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    if not path.exists():
        return out
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        k, _, v = line.partition("=")
        out[k.strip()] = v.strip().strip('"').strip("'")
    return out


def step(page, n: int, name: str, belt: str) -> None:
    """Take a numbered screenshot so a human can follow the run."""
    p = SCREENSHOT_DIR / f"contrado_{belt}_step_{n:02d}_{name}.png"
    page.screenshot(path=str(p), full_page=False)
    print(f"  [step {n:02d}] {name} → {p}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--belt", required=True, choices=list(BELT_DESIGNS))
    ap.add_argument("--headless", action="store_true",
                    help="Run without a visible browser window (CI mode).")
    args = ap.parse_args()

    env = {**os.environ, **load_dotenv(ENV_PATH)}
    email = env.get("CONTRADO_EMAIL")
    password = env.get("CONTRADO_PASSWORD")
    if not email or not password:
        print("ERR: CONTRADO_EMAIL and CONTRADO_PASSWORD must be in /Users/yuki/.env",
              file=sys.stderr)
        print("     The Helix API key won't work for the dashboard — Maker login",
              file=sys.stderr)
        print("     is a separate credential pair. Sign up at",
              file=sys.stderr)
        print("     https://www.contrado.com/sell/get-started first.", file=sys.stderr)
        return 2

    try:
        from playwright.sync_api import sync_playwright, TimeoutError as PWTimeout
    except ImportError:
        print("ERR: playwright not installed. Run: pip install playwright && "
              "playwright install chromium", file=sys.stderr)
        return 2

    belt = args.belt
    design_url = BELT_DESIGNS[belt]
    product_name = f"MU × IBJJF {belt.upper()} BELT Rashguard"
    print(f"→ Creating: {product_name}")
    print(f"  design:   {design_url}")
    print(f"  retail:   ¥{RETAIL_JPY:,}")

    result: dict[str, object] = {
        "belt": belt,
        "design_url": design_url,
        "product_name": product_name,
        "retail_jpy": RETAIL_JPY,
        "status": "pending",
    }

    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=args.headless, slow_mo=200)
        ctx = browser.new_context(viewport={"width": 1400, "height": 900})
        page = ctx.new_page()

        try:
            # ── 1. Login ─────────────────────────────────────────────
            page.goto("https://www.contrado.com/login", wait_until="domcontentloaded")
            step(page, 1, "login_page", belt)

            # Selectors are best-guess; if Contrado uses different IDs the
            # first run will dump the page HTML so a human can fix them.
            try:
                page.locator('input[type="email"], input[name="Email"]').first.fill(email)
                page.locator('input[type="password"], input[name="Password"]').first.fill(password)
                step(page, 2, "creds_filled", belt)
                page.locator('button[type="submit"], button:has-text("Login"), button:has-text("Sign In")').first.click()
            except PWTimeout:
                html_dump = SCREENSHOT_DIR / f"contrado_{belt}_login_html.txt"
                html_dump.write_text(page.content())
                print(f"  ! login selectors not found — HTML dumped to {html_dump}")
                result["status"] = "login_selectors_stale"
                return 3

            # Wait for post-login redirect (account / dashboard URL).
            try:
                page.wait_for_url("**/account/**", timeout=15_000)
            except PWTimeout:
                # Contrado may show MFA / captcha first.
                step(page, 3, "post_login_unexpected", belt)
                print("  ! post-login URL did not match /account/** — may need MFA")
                result["status"] = "mfa_or_captcha"
                return 4
            step(page, 3, "dashboard", belt)

            # ── 2. Navigate to create-product flow ────────────────────
            # Contrado calls it "Studio" or "Create" depending on tier.
            for href_candidate in [
                "/studio/new", "/sell/create",
                "/account/products/new", "/account/maker/products/new",
            ]:
                page.goto(f"https://www.contrado.com{href_candidate}",
                          wait_until="domcontentloaded")
                if "login" not in page.url:
                    step(page, 4, f"create_flow_{href_candidate.replace('/','_')}", belt)
                    break
            else:
                print("  ! no create-product URL worked; dashboard tour needed")
                result["status"] = "create_url_unknown"
                return 5

            # ── 3. Product type — Mens Long Sleeve Sports Top ─────────
            # Contrado's category nav is the hardest part to automate without
            # a live walk-through. Print a clear note for the human.
            print("  ! Product picker requires a live walk-through on first run.")
            print(f"    Manually click through to:  Sportswear > Mens > "
                  f"Long Sleeve Performance Top  (or Rashguard if listed).")
            print(f"    Then upload the design from {design_url}")
            print(f"    Then set the name to:  {product_name}")
            print(f"    And the price to:  ¥{RETAIL_JPY:,}  (selling/retail)")
            print("  Browser will stay open for 5 min so you can finish manually.")
            time.sleep(300)

            # ── 4. Capture final SKU ─────────────────────────────────
            step(page, 9, "final_state", belt)
            # Best-effort: look for a published product URL/SKU on the page.
            sku_candidates = page.locator("text=/SKU\\s*[:#]\\s*([A-Z0-9-]+)/i").all_text_contents()
            if sku_candidates:
                result["contrado_sku_text"] = sku_candidates[0]
            result["final_url"] = page.url
            result["status"] = "human_finished"

        finally:
            out = SCREENSHOT_DIR / f"contrado_{belt}_result.json"
            out.write_text(json.dumps(result, indent=2, ensure_ascii=False))
            print(f"→ Result: {out}")
            browser.close()

    return 0 if result.get("status") == "human_finished" else 1


if __name__ == "__main__":
    sys.exit(main())
