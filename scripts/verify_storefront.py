"""Browser checks against the isolated preview; intercept external writes."""
import json
from pathlib import Path
import sqlite3
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "store/target/storefront-preview"
BASE = "http://127.0.0.1:8943"


def inspect_seed():
    conn = sqlite3.connect(OUT / "preview.db")
    print("seed", conn.execute("SELECT COUNT(*) FROM catalog_products WHERE is_active=1").fetchone())
    print("invalid meta", conn.execute("SELECT COUNT(*) FROM catalog_products WHERE meta_json IS NOT NULL AND NOT json_valid(meta_json)").fetchone())
    for table in ("catalog_products", "collab_users"):
        print(table, [r[1] for r in conn.execute(f"PRAGMA table_info({table})")])


def verify():
    results = []
    with sync_playwright() as p:
        for engine in (p.chromium, p.webkit):
            browser = engine.launch()
            context = browser.new_context()
            context.route("**/enabler-analytics.fly.dev/**", lambda r: r.abort())
            context.route("**/api/tracking/config", lambda r: r.fulfill(json={}))
            events = []
            def event(route):
                route.fulfill(status=204)
            context.route("**/api/v1/event", event)
            context.add_init_script("""window.__funnelEvents=[];
                const NativeBlob=window.Blob, payloads=new WeakMap();
                window.Blob=class extends NativeBlob {constructor(parts,opts){super(parts,opts);payloads.set(this,parts);}};
                navigator.sendBeacon=function(url,blob){
                    if(String(url).includes('/api/v1/event')) window.__funnelEvents.push(JSON.parse(payloads.get(blob).join('')));
                    return true;
                };""")
            context.route("**/api/shop/checkout**", lambda r: r.fulfill(status=200, body="Checkout intercepted"))
            page = context.new_page()
            errors = []
            page.on("pageerror", lambda e: errors.append(str(e)))
            for lang in ("ja", "en"):
                for width in (320, 390, 768, 1440):
                    page.set_viewport_size({"width": width, "height": 900})
                    response = page.goto(f"{BASE}/?lang={lang}", wait_until="networkidle")
                    assert response.status == 200
                    assert page.locator("html").get_attribute("lang") == lang
                    assert page.evaluate("document.documentElement.scrollWidth") <= width
                    assert page.locator(".product").count() == 8
                    assert page.locator("h1").count() == 1
                    assert "{{" not in page.content()
                    assert page.locator('a[href^="/make"]').count() == 0
                    assert page.locator('a[href="https://apps.apple.com/app/id6781269252"]').count() == 3
                    assert page.locator('.hero-product img').evaluate('(e)=>e.complete&&e.naturalWidth>0')
                    page.locator(".questions summary").first.click()
                    assert page.locator(".questions details").first.get_attribute("open") is not None
                    href = page.locator(".product").first.get_attribute("href")
                    assert f"lang={lang}" in href
                    if width in (390, 1440):
                        page.screenshot(path=str(OUT / f"home-{lang}-{width}-{engine.name}.png"), full_page=True)
                    page.locator('.hero-copy .primary').click()
                    page.wait_for_load_state('domcontentloaded')
                    assert 'brand=bjj' in page.url and f'lang={lang}' in page.url
                    assert page.locator('.card').count() > 0
                    assert page.evaluate('document.documentElement.scrollWidth') <= width
                    page.goto(f'{BASE}/shop?lang={lang}', wait_until='domcontentloaded')
                    page.locator('link[href^="/storefront.css"]').wait_for(state='attached')
                    assert page.locator('.grid').bounding_box()['y'] < 680
                    assert page.locator('#muMakeFab, #lineMakeFab').count() == 0
                    assert page.locator('.shop-create a').get_attribute('href') == 'https://apps.apple.com/app/id6781269252'
                    if width in (390, 1440):
                        page.screenshot(path=str(OUT / f'shop-{lang}-{width}-{engine.name}.png'), full_page=True)
                    page.goto(BASE + href, wait_until='networkidle')
                    assert page.locator('html').get_attribute('lang') == lang
                    assert page.evaluate('document.documentElement.scrollWidth') <= width
                    assert page.locator('#buybtn').count() == 1
                    assert page.locator('a[href^="/make"]').count() == 0
                    assert page.locator('.shop-create a').get_attribute('href') == 'https://apps.apple.com/app/id6781269252'
                    assert page.locator('.purchase-options').get_attribute('open') is None
                    assert page.locator('#lineMakeFab').count() == 0
                    assert page.locator('.trust-strip').bounding_box()['y'] < page.locator('.body .sku').bounding_box()['y']
                    if width in (390, 1440):
                        page.screenshot(path=str(OUT / f'pdp-{lang}-{width}-{engine.name}.png'), full_page=True)
                    buy_href = page.locator('#buybtn').get_attribute('href')
                    assert 'sku=' in buy_href
                    page.locator('#buybtn').evaluate("e=>e.addEventListener('click',event=>event.preventDefault())")
                    page.locator('#buybtn').click()
                    page.wait_for_function("window.__funnelEvents.some(e=>e.event==='checkout_attempt')")
                    events.extend(page.evaluate('window.__funnelEvents'))
                    results.append({"browser": engine.name, "lang": lang, "width": width,
                                    "home": "pass", "shop": "pass", "pdp": "pass", "checkout_attempt": "pass"})
            print(json.dumps(results, ensure_ascii=False))
            print("errors", errors)
            assert not errors
            assert any(e["event"] == "pageview" and e["extra"]["ab"] == "storefront-20260917" for e in events)
            assert any(e['event']=='checkout_attempt' and e['extra']['ab']=='storefront-20260917' for e in events)
            browser.close()
    (OUT / "browser-verification.json").write_text(json.dumps(results, indent=2))


if __name__ == "__main__":
    inspect_seed()
    verify()
