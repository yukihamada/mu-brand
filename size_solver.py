#!/usr/bin/env python3
"""自己改善: lot_w/lot_d を反復調整して『実際の延床=目標㎡』に収束させる。
create_house は gross_m2 を無視し lot 寸法で建てる前提を実測で確かめ、各目標に解く。
"""
import json, urllib.request, math, sys
BASE="https://bim.house"
def http(u,p=None,m="GET"):
    d=json.dumps(p).encode() if p is not None else None
    r=urllib.request.Request(u,data=d,method=m,headers={"content-type":"application/json"})
    with urllib.request.urlopen(r,timeout=60) as x: return json.loads(x.read().decode())

def make(lw,ld,floors,name="probe"):
    r=http(f"{BASE}/mcp",{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_house","arguments":{
        "name":name,"address":"北海道川上郡弟子屈町美留和","floors":floors,
        "lot_w_m":round(lw,2),"lot_d_m":round(ld,2),"land_area_m2":round(lw*ld*2.2,1),
        "private":True}}},m="POST")
    o=json.loads(r["result"]["content"][0]["text"])
    slug=o["slug"]
    h=http(f"{BASE}/api/houki/check/{slug}")
    gross=h.get("floor_area_ratio",{}).get("gross_floor_m2")
    built=h.get("building_coverage",{}).get("built_area_m2")
    return slug,gross,built,o["structure"]["pass"],o["houki"]["overall_pass"]

targets=[("SEED",9.9,1),("HEARTH",99.0,2),("ATSUME",180.0,2)]
results={}
for nm,tgt,fl in targets:
    side=math.sqrt(tgt/fl)+1.6  # 初期推定(設定後退分)
    print(f"\n=== {nm} target={tgt} floors={fl} ===")
    last=None
    for it in range(5):
        slug,gross,built,sp,hp=make(side,side,fl,f"{nm} solve it{it}")
        print(f"  it{it}: lot={side:.2f} -> gross={gross} built={built} struct={sp} houki={hp}")
        last=(side,gross,slug)
        if gross is None: break
        if abs(gross-tgt)/tgt < 0.08: print(f"  ✓ within 8%"); break
        side *= math.sqrt(tgt/gross)
        side=max(side,2.2)
    results[nm]=last
print("\nSOLVED:", json.dumps({k:(round(v[0],2),v[1]) for k,v in results.items() if v}, ensure_ascii=False))
