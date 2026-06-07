#!/usr/bin/env python3
"""Build the MU collection showcase report (HTML -> PDF via headless Chrome).

Input JSON (argv[1]) shape:
{
 "title": "MU COLLECTION",
 "subtitle": "...",
 "themes": [
   {"slug":"mu-genten","name":"GENTEN — 原点","tagline":"...","img":"/abs/path.png",
    "blurb":"...", "score":{"universality":100,"craft":100,"legibility":100,"brand_fit":100,"product_readiness":100},
    "products":[{"kind":"tee_white","label":"...","price":4900,"pdp":"https://..."}, ...]}
 ]
}
Output: argv[2] PDF path (default /tmp/mu_collection_report.pdf)
"""
import base64, json, os, subprocess, sys

AX = [("universality","普遍性"),("craft","クラフト"),("legibility","可読性"),
      ("brand_fit","ブランド一貫性"),("product_readiness","商品適性")]
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"

def b64(path):
    with open(path,"rb") as f: return base64.b64encode(f.read()).decode()

def bar(v):
    color = "#16a34a" if v>=100 else ("#65a30d" if v>=90 else "#ea580c")
    return (f'<div class="ax"><span class="axn">{{}}</span>'
            f'<span class="axbar"><i style="width:{v}%;background:{color}"></i></span>'
            f'<span class="axv">{v}</span></div>')

def theme_block(t):
    s = t.get("score",{})
    axes = "".join(bar(s.get(k,0)).format(ja) for k,ja in AX)
    total = round(sum(s.get(k,0) for k,_ in AX)/len(AX))
    prods = "".join(
        f'<a class="prod" href="{p["pdp"]}"><span class="pk">{p["kind"].replace("_","·")}</span>'
        f'<span class="pl">{p["label"]}</span><span class="pp">¥{p["price"]:,}</span></a>'
        for p in t["products"])
    return f"""
<section class="theme">
  <div class="art"><img src="data:image/png;base64,{b64(t['img'])}"></div>
  <div class="meta">
    <div class="thname">{t['name']}</div>
    <div class="thtag">{t['tagline']}</div>
    <p class="blurb">{t.get('blurb','')}</p>
    <div class="scorecard"><div class="sctitle">5軸採点 <b>{total}</b>/100</div>{axes}</div>
    <div class="prods">{prods}</div>
  </div>
</section>"""

def main():
    data = json.load(open(sys.argv[1]))
    out = sys.argv[2] if len(sys.argv)>2 else "/tmp/mu_collection_report.pdf"
    themes = "".join(theme_block(t) for t in data["themes"])
    n_prod = sum(len(t["products"]) for t in data["themes"])
    totals = [round(sum(t["score"].get(k,0) for k,_ in AX)/len(AX)) for t in data["themes"]]
    avg = round(sum(totals)/len(totals),1) if totals else 0
    n100 = sum(1 for x in totals if x>=100)
    html = f"""<!DOCTYPE html><html lang="ja"><head><meta charset="utf-8">
<style>
@page {{ size: A4; margin: 14mm; }}
*{{box-sizing:border-box;margin:0;padding:0}}
body{{font-family:'Hiragino Mincho ProN','YuMincho',serif;color:#111;-webkit-print-color-adjust:exact;print-color-adjust:exact}}
.cover{{text-align:center;padding:30px 0 24px;border-bottom:2px solid #111;margin-bottom:26px}}
.mark{{font-size:30px;letter-spacing:.3em;font-weight:700}}
.title{{font-size:40px;font-weight:800;letter-spacing:.12em;margin:12px 0 6px}}
.sub{{font-size:13px;color:#555;letter-spacing:.05em;line-height:1.8}}
.stat{{font-size:11px;color:#888;margin-top:10px;letter-spacing:.1em}}
.theme{{display:flex;gap:22px;align-items:flex-start;padding:18px 0;border-bottom:1px solid #e5e5e5;page-break-inside:avoid}}
.art{{flex:0 0 230px}}
.art img{{width:230px;height:230px;object-fit:contain;border:1px solid #eee;background:#fff}}
.meta{{flex:1;min-width:0}}
.thname{{font-size:21px;font-weight:800;letter-spacing:.04em}}
.thtag{{font-size:12px;color:#b45309;margin:2px 0 8px;letter-spacing:.03em}}
.blurb{{font-size:12px;line-height:1.85;color:#333;margin-bottom:12px}}
.scorecard{{background:#fafafa;border:1px solid #eee;border-radius:8px;padding:10px 12px;margin-bottom:12px}}
.sctitle{{font-size:11px;color:#666;letter-spacing:.1em;margin-bottom:7px;text-transform:uppercase}}
.sctitle b{{color:#16a34a;font-size:14px}}
.ax{{display:flex;align-items:center;gap:8px;font-size:10.5px;margin:3px 0}}
.axn{{flex:0 0 88px;color:#444}}
.axbar{{flex:1;height:6px;background:#eee;border-radius:4px;overflow:hidden}}
.axbar i{{display:block;height:100%}}
.axv{{flex:0 0 26px;text-align:right;font-variant-numeric:tabular-nums;color:#111;font-weight:700}}
.prods{{display:flex;flex-wrap:wrap;gap:7px}}
.prod{{display:flex;align-items:baseline;gap:8px;border:1px solid #ddd;border-radius:6px;padding:5px 10px;font-size:11px;text-decoration:none;color:#111}}
.pk{{font-family:monospace;font-size:9px;color:#888;text-transform:uppercase}}
.pl{{font-weight:600}}
.pp{{font-family:monospace;color:#16a34a}}
.foot{{margin-top:22px;text-align:center;font-size:10px;color:#999;letter-spacing:.08em;line-height:1.8}}
</style></head><body>
<div class="cover">
  <div class="mark">━◯━</div>
  <div class="title">{data['title']}</div>
  <div class="sub">{data['subtitle']}</div>
  <div class="stat">{len(data['themes'])} themes · {n_prod} products · 平均 {avg}/100（満点 {n100}/{len(totals)}） · wearmu.com</div>
</div>
{themes}
<div class="foot">MU — 作ることを、空気のように。<br>すべてAI自律生成・即LIVE / 真正性は wearmu.com で検証可 / 株式会社イネブラ</div>
</body></html>"""
    hp = out.replace(".pdf",".html")
    open(hp,"w").write(html)
    subprocess.run([CHROME,"--headless","--disable-gpu","--no-pdf-header-footer",
                    f"--print-to-pdf={out}", "file://"+hp], capture_output=True)
    print("HTML:", hp)
    print("PDF :", out, "(%d bytes)" % (os.path.getsize(out) if os.path.exists(out) else 0))

if __name__ == "__main__":
    main()
