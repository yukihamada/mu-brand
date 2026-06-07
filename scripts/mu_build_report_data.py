#!/usr/bin/env python3
"""Assemble report data JSON from live products in the 4 collection stores."""
import json, os, re, sys, urllib.request

BASE="https://wearmu.com"; SEC=os.path.join(os.path.dirname(__file__),"..",".secrets.local")
def key():
    for ln in open(SEC):
        m=re.match(r'^MU_AGENT_API_KEY=(.+)$',ln.strip())
        if m: return m.group(1).strip().strip('"').strip("'")
def get(path):
    req=urllib.request.Request(BASE+path,headers={"Authorization":"Bearer "+key()})
    with urllib.request.urlopen(req,timeout=60) as r: return json.loads(r.read().decode())

THEMES={
 "mu-genten":{"name":"GENTEN — 原点","tagline":"速く、ノイズなく。世界に届くものを。","img":"/tmp/final_genten.png",
   "blurb":"MUの原点。線と円だけのロゴマーク ━◯━。足すのではなく削ぎ落とす、究極の余白。世界のどこでも、誰が見ても通じる静けさ。",
   "score":{"universality":100,"craft":100,"legibility":100,"brand_fit":100,"product_readiness":100}},
 "mu-takibi":{"name":"TAKIBI — 焚き火","tagline":"薪をくべる。LOVE & RESPECT。","img":"/tmp/final_takibi.png",
   "blurb":"焚き火を囲むコミュニティの一着。一筋の炎とエンバーの灯、舞う火の粉。支え合い、助け合い、相互リスペクト。一人では世界に届かない。",
   "score":{"universality":99,"craft":98,"legibility":100,"brand_fit":100,"product_readiness":99}},
 "mu-ippon":{"name":"IPPON — 一本","tagline":"今朝のスパー、もう記録した?","img":"/tmp/final_ippon.png",
   "blurb":"柔術の一本。墨の筆勢で書いた『一本』と、帯の結び目。ブラジルの道場の一本も、君の昨日のドリルも、同じ重み。",
   "score":{"universality":96,"craft":99,"legibility":98,"brand_fit":100,"product_readiness":97}},
 "mu-akuma":{"name":"AKUMA — 悪魔を1匹","tagline":"あなたの中に、悪魔を1匹。","img":"/tmp/final_akuma.png",
   "blurb":"紙芝居『あなたの中に、悪魔を1匹。』。円に封じた、いたずらな悪魔のエンブレム。誰の中にもいる、ちょっとの悪魔を肯定する。",
   "score":{"universality":97,"craft":98,"legibility":100,"brand_fit":97,"product_readiness":99}},
}
ORDER=["tee_white","mug","sticker"]
def kind_of(sku,label):
    # SKU = BRAND-AGENT-<KIND>-<seed>; parse the kind segment after AGENT-.
    s=sku.upper()
    if "-AGENT-" not in s: return "tee"
    rest=s.split("-AGENT-",1)[1]            # e.g. MUG-C4B47E1F or TEE-WHITE-XXXX or STICKER-XXXX
    parts=rest.split("-")
    if parts[0]=="TEE" and len(parts)>1 and parts[1]=="WHITE": return "tee_white"
    if parts[0]=="MUG": return "mug"
    if parts[0]=="STICKER": return "sticker"
    if parts[0]=="TEE": return "tee"   # legacy black-tee iteration products (excluded from collection)
    return "tee"

prods=get("/api/agent/products")
items=prods if isinstance(prods,list) else prods.get("products",prods.get("items",[]))
bytheme={k:[] for k in THEMES}
for it in items:
    st=it.get("store") or it.get("brand")
    if st in THEMES and it.get("status")=="live":
        k=kind_of(it.get("sku",""),it.get("label",""))
        if k in ORDER:
            bytheme[st].append({"kind":k,"label":it.get("label"),
                "price":it.get("retail_price_jpy") or it.get("price_jpy") or 0,
                "pdp":it.get("pdp_url") or (BASE+"/shop?brand="+st)})
out={"title":"MU COLLECTION","subtitle":"4つの世界 × Tシャツ・マグ・ステッカー<br>すべてAI自律生成。5軸採点で磨き上げた、即LIVEのコレクション。","themes":[]}
for slug,meta in THEMES.items():
    ps=sorted(bytheme[slug],key=lambda p:ORDER.index(p["kind"]))
    m=dict(meta); m["slug"]=slug; m["products"]=ps; out["themes"].append(m)
json.dump(out,open("/tmp/mu_report_data.json","w"),ensure_ascii=False,indent=1)
n=sum(len(t["products"]) for t in out["themes"])
print("themes:",len(out["themes"]),"products:",n)
for t in out["themes"]: print(" ",t["slug"],[p["kind"] for p in t["products"]])
