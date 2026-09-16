"""Read-only comparison; local analytics acceptance test is explicitly isolated."""
import json
from pathlib import Path
import urllib.request
from playwright.sync_api import sync_playwright

OUT = Path(__file__).resolve().parents[1] / "store/target/storefront-preview"
BASE = 'http://127.0.0.1:8943'
report = {}
with sync_playwright() as p:
    browser = p.chromium.launch()
    context = browser.new_context(viewport={'width':390,'height':844})
    context.route('**/api/v1/event', lambda r:r.fulfill(status=204))
    context.route('**/api/tracking/config', lambda r:r.fulfill(json={}))
    context.route('**/enabler-analytics.fly.dev/**', lambda r:r.abort())
    page = context.new_page()
    for label, base in [('before','https://wearmu.com'),('after',BASE)]:
        page.goto(base+'/shop?lang=ja',wait_until='networkidle')
        report[label] = {'shop_first_product_y':round(page.locator('.grid').bounding_box()['y']),
                         'width':page.evaluate('document.documentElement.scrollWidth')}
    page.goto(BASE+'/?lang=ja',wait_until='networkidle')
    report['images'] = page.locator('img').evaluate_all('''async es=>await Promise.all(es.map(async e=>{
        const i=new Image();i.src=e.src;try{await i.decode();return {src:e.src,ok:true}}catch{return {src:e.src,ok:false}}
    }))''')
    assert all(i['ok'] for i in report['images'])
    # Exercise GET search with special characters and preservation of language.
    page.goto(BASE+'/shop?kind=tee&lang=en',wait_until='domcontentloaded')
    page.locator('input[type=search]').fill('<script>alert(1)</script>')
    page.locator('.shopsearch button').click()
    page.wait_for_load_state('domcontentloaded')
    assert 'lang=en' in page.url and 'kind=tee' in page.url
    assert page.locator('.empty').count()==1
    assert page.locator('.empty a[href="https://apps.apple.com/app/id6781269252"]').count()==1
    report['search_empty_recovery']='pass'
    # Homepage has no JS dependency for product discovery, language or FAQ.
    nojs=browser.new_context(java_script_enabled=False)
    s=nojs.new_page()
    s.goto(BASE+'/?lang=en')
    assert s.locator('.product').count()==8
    href=s.locator('.product').first.get_attribute('href')
    s.goto(BASE+href)
    assert s.locator('#buybtn').get_attribute('href').startswith('/api/shop/checkout?sku=')
    report['no_js_home_to_product']='pass'
    browser.close()

event={'visitor_id':'renewal-local-audit','session_id':'renewal-local-audit','event':'cta_click',
       'path':'/','extra':{'cta':'renewal_hero_bjj','ab':'storefront-20260917'}}
request=urllib.request.Request(BASE+'/api/v1/event',data=json.dumps(event).encode(),headers={'Content-Type':'application/json'})
with urllib.request.urlopen(request) as response:
    report['local_event_endpoint']=response.status
assert 200<=report['local_event_endpoint']<300
print(json.dumps(report,ensure_ascii=False,indent=2))
(OUT/'audit.json').write_text(json.dumps(report,ensure_ascii=False,indent=2))
