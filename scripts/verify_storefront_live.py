"""Read-only production smoke: no live analytics, checkout or app installation."""
import hashlib
import json
from pathlib import Path
import urllib.request
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[1]
BASE = 'https://wearmu.com'
APP = 'https://apps.apple.com/app/id6781269252'
result = {'commit': '58555e7f1bb20e9f0efdbc48b398a506436a4026', 'pages': []}
with urllib.request.urlopen(BASE+'/healthz', timeout=30) as r:
    result['health'] = json.load(r)
assert result['health']['ok']
with urllib.request.urlopen(BASE+'/storefront.css?v=20260917', timeout=30) as r:
    css = r.read()
result['css_sha256'] = hashlib.sha256(css).hexdigest()
assert css == (ROOT/'store/static/storefront.css').read_bytes()

with sync_playwright() as p:
    browser = p.chromium.launch()
    context = browser.new_context()
    context.route('**/api/v1/event', lambda r:r.fulfill(status=204))
    context.route('**/enabler-analytics.fly.dev/**', lambda r:r.abort())
    context.route('**/api/tracking/config', lambda r:r.fulfill(json={}))
    context.route('**/api/shop/checkout**', lambda r:r.abort())
    page = context.new_page()
    errors = []
    page.on('pageerror', lambda e:errors.append(str(e)))
    for lang in ('ja','en'):
        for width in (390,1440):
            page.set_viewport_size({'width':width,'height':900})
            page.goto(BASE+'/?lang='+lang, wait_until='networkidle')
            assert page.locator('body').get_attribute('class')=='mu-storefront'
            assert page.locator(f'a[href="{APP}"]').count()==3
            assert page.locator('a[href^="/make"]').count()==0
            assert page.evaluate('document.documentElement.scrollWidth')==width
            assert page.locator('html').get_attribute('lang')==lang
            images=page.locator('img').evaluate_all('''async es=>Promise.all(es.map(async e=>{
                const i=new Image();i.src=e.src;try{await i.decode();return true}catch{return false}}
            ))''')
            assert all(images)
            href=page.locator('.product').first.get_attribute('href')
            page.goto(BASE+'/shop?lang='+lang,wait_until='networkidle')
            assert page.locator('.shop-create a').get_attribute('href')==APP
            assert page.locator('a[href^="/make"]').count()==0
            assert page.evaluate('document.documentElement.scrollWidth')==width
            page.goto(BASE+href,wait_until='networkidle')
            assert page.locator('.shop-create a').get_attribute('href')==APP
            assert page.locator('a[href^="/make"]').count()==0
            assert page.locator('#buybtn').get_attribute('href').startswith('/api/shop/checkout?sku=')
            assert page.evaluate('document.documentElement.scrollWidth')==width
            result['pages'].append({'lang':lang,'width':width,'home_shop_pdp':'pass','home_images':len(images)})
    assert not errors,errors
    browser.close()
print(json.dumps(result,ensure_ascii=False,indent=2))
(ROOT/'store/target/storefront-preview/live-verification.json').write_text(json.dumps(result,ensure_ascii=False,indent=2))
