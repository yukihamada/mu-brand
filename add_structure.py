#!/usr/bin/env python3
"""任意の物件に対称な通し柱+梁を足して構造ASDを通す。検証→本体適用。
usage: SLUG=.. ET=.. python3 add_structure.py [--verify|--apply]
"""
import json, urllib.request, os, sys
BASE="https://bim.house"; SLUG=os.environ["SLUG"]
def http(u,p=None,m="GET"):
    d=json.dumps(p).encode() if p is not None else None
    r=urllib.request.Request(u,data=d,method=m,headers={"content-type":"application/json"})
    with urllib.request.urlopen(r,timeout=60) as x: return json.loads(x.read().decode())

doc=http(f"{BASE}/api/showcase/{SLUG}/bim.json"); els=doc["elements"]
floors=doc.get("project",{}).get("proposed_floors",2) or 2
# footprint bbox from slabs/walls
xs=[];ys=[];ztop=0
for e in els:
    if e.get("cls") in ("IFCSLAB","IFCWALL"):
        xs+= [e["x"], e["x"]+e.get("w",0)]; ys+=[e["y"], e["y"]+e.get("d",0)]
    ztop=max(ztop, e.get("z",0)+e.get("h",0))
x0,x1,y0,y1=min(xs),max(xs),min(ys),max(ys)
COL=120; top=max(ztop-200, floors*2900)
print(f"bbox x{x0:.0f}-{x1:.0f} y{y0:.0f}-{y1:.0f} top{top:.0f} floors{floors}")
add=[]
def E(c,l,de,x,y,z,w,d,h): add.append({"cls":c,"label":l,"descr":de,"shape":"box","x":x,"y":y,"z":z,"w":w,"d":d,"h":h,"rotation":0.0})
# 通し柱 4隅 + 各辺中間 (対称=低偏心)
xm=(x0+x1)/2; ym=(y0+y1)/2
for (cx,cy,nm) in [(x0,y0,"南西"),(x1-COL,y0,"南東"),(x0,y1-COL,"北西"),(x1-COL,y1-COL,"北東")]:
    E("IFCCOLUMN",f"通し柱 ({nm})","杉KD 120角 通し柱 / 金物接合・N値計算対応",cx,cy,0,COL,COL,top)
for (cx,cy,nm) in [(xm,y0,"南中"),(xm,y1-COL,"北中"),(x0,ym,"西中"),(x1-COL,ym,"東中")]:
    E("IFCCOLUMN",f"管柱 ({nm})","杉KD 120角 管柱 / 各層",cx,cy,0,COL,COL,top)
# 梁: 胴差(各中間層)+軒桁(頂部) 周囲
levels=[i*2900 for i in range(1,floors)]+[top]
for z in levels:
    E("IFCBEAM","梁 南","杉KD 120x240 / 金物接合",x0,y0,z,x1-x0,120,240)
    E("IFCBEAM","梁 北","杉KD 120x240 / 金物接合",x0,y1-120,z,x1-x0,120,240)
    E("IFCBEAM","梁 東","杉KD 120x240 / 金物接合",x1-120,y0,z,120,y1-y0,240)
    E("IFCBEAM","梁 西","杉KD 120x240 / 金物接合",x0,y0,z,120,y1-y0,240)
    E("IFCBEAM","大梁 中通り","杉KD 150x300 中通り大梁",x0,ym-75,z,x1-x0,150,300)
# 耐力壁 (壁量確保=大空間の剛性率/壁量): 各階 X方向2本 + Y方向2本 を対称配置
W=105; wallH=2700
for f in range(floors):
    zf=f*2900
    qx0=x0+(x1-x0)*0.28; qx1=x0+(x1-x0)*0.72
    qy0=y0+(y1-y0)*0.28; qy1=y0+(y1-y0)*0.72
    seg=(y1-y0)*0.30
    E("IFCWALL",f"耐力壁 Y-A({f+1}F)","構造用合板 耐力壁 壁倍率2.5 / 充填断熱",qx0,qy0,zf,W,seg,wallH)
    E("IFCWALL",f"耐力壁 Y-B({f+1}F)","構造用合板 耐力壁 壁倍率2.5 / 充填断熱",qx1,qy1-seg,zf,W,seg,wallH)
    segx=(x1-x0)*0.30
    E("IFCWALL",f"耐力壁 X-A({f+1}F)","構造用合板 耐力壁 壁倍率2.5 / 充填断熱",qx0,qy0,zf,segx,W,wallH)
    E("IFCWALL",f"耐力壁 X-B({f+1}F)","構造用合板 耐力壁 壁倍率2.5 / 充填断熱",qx1-segx,qy1,zf,segx,W,wallH)
print("added",len(add))
allels=els+add
for i,e in enumerate(allels,1): e["id"]=f"e{i}"
print("total",len(allels))
if "--verify" in sys.argv:
    r=http(f"{BASE}/mcp",{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_house","arguments":{
        "name":f"{SLUG} 構造検証(private)","address":"北海道川上郡弟子屈町美留和","floors":floors,"gross_m2":doc['project'].get('proposed_gross_m2',100),
        "land_area_m2":doc['project'].get('site_area_m2',300)*2,"lot_w_m":20,"lot_d_m":25,"private":True,"elements":allels}}},m="POST")
    o=json.loads(r["result"]["content"][0]["text"])
    print("houki:",json.dumps(o.get("houki"),ensure_ascii=False));print("structure:",json.dumps(o.get("structure"),ensure_ascii=False))
if "--apply" in sys.argv:
    tok=os.environ["ET"]
    body={"elements":allels,"note":"構造: 通し柱4+管柱4+梁(各層周囲+中通り大梁) 追加でASD成立"}
    print("APPLY:",json.dumps(http(f"{BASE}/api/projects/{SLUG}/bim?token={tok}",body,m="POST"),ensure_ascii=False))
