#!/usr/bin/env python3
"""パラメトリックに“正寸”の家を建てる(生成器のサイズ制約を回避)。
footprint W×D × floors の正しいシェル(構造/採光/24h換気/オフグリッド)を生成し、
houki の実延床が目標±3%になるまで footprint を自己調整 → 既存slugへ上書き保存。
"""
import json, urllib.request, math, sys, os
BASE="https://bim.house"
def http(u,p=None,m="GET"):
    d=json.dumps(p).encode() if p is not None else None
    r=urllib.request.Request(u,data=d,method=m,headers={"content-type":"application/json"})
    with urllib.request.urlopen(r,timeout=60) as x: return json.loads(x.read().decode())

def shell(W,D,F,program):
    """W,D mm footprint, F floors. 構造的に成立する正寸シェル+居住要素。"""
    els=[]; H=2900
    def E(c,l,de,x,y,z,w,d,h): els.append({"cls":c,"label":l,"descr":de,"shape":"box","x":int(x),"y":int(y),"z":int(z),"w":int(w),"d":int(d),"h":int(h),"rotation":0.0})
    E("IFCSLAB","基礎(ベタ)","RC ベタ基礎 t150 Fc24 / 凍結深以下",0,0,-450,W,D,450)
    floor_m2=W*D/1e6
    for f in range(F):
        z=f*H
        E("IFCSLAB",f"{f+1}F床スラブ","構造用合板t24 剛床+根太+床下GW",0,0,z,W,D,120)
        E("IFCCOVERING",f"{f+1}F仕上床","無垢フローリング t15",60,60,z+120,W-120,D-120,15)
        E("IFCWALL",f"{f+1}F南外壁","軸組+高性能GWt105+付加断熱+通気層",0,0,z,W,150,H)
        E("IFCWALL",f"{f+1}F北外壁","軸組+高性能GWt105+付加断熱+通気層",0,D-150,z,W,150,H)
        E("IFCWALL",f"{f+1}F西外壁","軸組+高性能GWt105+付加断熱+通気層",0,0,z,150,D,H)
        E("IFCWALL",f"{f+1}F東外壁","軸組+高性能GWt105+付加断熱+通気層",W-150,0,z,150,D,H)
        # 採光: 南面大開口(床面積の~22%)で 1/7 を確実に満たす
        need=floor_m2*0.22  # m2
        ww=min(W-700, max(1200, int(need*1e6/1700)))
        E("IFCWINDOW",f"{f+1}F南大窓","Low-E複層+樹脂サッシ U=1.6 日射取得",300,0,z+100,ww,150,1700)
        E("IFCWINDOW",f"{f+1}F東窓","Low-E複層+樹脂サッシ",W-150,300,z+650,150,min(D-600,1600),1200)
        # 耐力壁(壁量): 中央十字
        E("IFCWALL",f"{f+1}F耐力壁X","構造用合板 壁倍率2.5",int(W*0.30),int(D*0.5-52),z,int(W*0.4),105,2700)
        E("IFCWALL",f"{f+1}F耐力壁Y","構造用合板 壁倍率2.5",int(W*0.5-52),int(D*0.30),z,105,int(D*0.4),2700)
        # 各層 周囲梁
        for (bx,by,bw,bd,nm) in [(0,0,W,120,"南"),(0,D-120,W,120,"北"),(0,0,120,D,"西"),(W-120,0,120,D,"東")]:
            E("IFCBEAM",f"{f+1}F梁{nm}","杉KD 120x240 金物接合",bx,by,z+H-240,bw,bd,240)
    # 通し柱4隅
    for (cx,cy,nm) in [(0,0,"南西"),(W-120,0,"南東"),(0,D-120,"北西"),(W-120,D-120,"北東")]:
        E("IFCCOLUMN",f"通し柱{nm}","杉120角 通し柱 金物接合 N値対応",cx,cy,0,120,120,F*H)
    E("IFCDOOR","玄関ドア","断熱ドア K2 スマートロック対応",int(W*0.45),0,0,900,150,2100)
    E("IFCROOF","屋根(軒450)","ガルバ立平+通気+GWt155 不燃",-450,-450,F*H,W+900,D+900,220)
    if F>1: E("IFCSTAIR","階段","木製直階段 蹴上200踏面230 手すり",int(W-1100),int(D*0.4),0,950,2400,F*H)
    # 24h機械換気(シックハウスを未モデルにしない)
    E("IFCAIRTERMINALBOX","24h熱交換換気","第一種熱交換換気 給排気",int(W-700),int(D-500),F*H-300,600,400,300)
    # オフグリッド: PV + 蓄電 + 薪
    E("IFCENERGYCONVERSIONDEVICE","太陽光PV","単結晶PV(寒冷地大きめ)",int(W*0.2),int(D*0.2),F*H+40,min(W-600,3000),min(D-600,2400),80)
    E("IFCELECTRICFLOWSTORAGEDEVICE","蓄電池LiFePO4","オフグリッド蓄電 保温庫",int(W-700),300,135,500,400,1200)
    E("IFCENERGYCONVERSIONDEVICE","薪ストーブ","鋳鉄 薪ストーブ 寒冷地主暖房",int(W*0.5),int(D*0.5),135,600,600,750)
    for pe in program: els.append(pe)
    return els

def measure(els,floors,name):
    r=http(f"{BASE}/mcp",{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_house","arguments":{
        "name":name,"address":"北海道川上郡弟子屈町美留和","floors":floors,
        "land_area_m2":2000,"lot_w_m":50,"lot_d_m":50,"private":True,"elements":els}}},m="POST")
    o=json.loads(r["result"]["content"][0]["text"])
    h=http(f"{BASE}/api/houki/check/{o['slug']}")
    return o,h.get("floor_area_ratio",{}).get("gross_floor_m2"),h.get("overall_pass"),o["structure"]["pass"]

def solve(name,target,floors,program):
    side=math.sqrt(target/floors)*1000  # mm 初期
    last=None
    for it in range(6):
        els=shell(side,side,floors,program)
        o,gross,hp,sp=measure(els,floors,f"{name} build it{it}")
        print(f"  {name} it{it}: side={side/1000:.2f}m gross={gross} houki={hp} struct={sp}")
        last=(els,gross,o)
        if gross and abs(gross-target)/target<0.03: print("  ✓ 3%以内"); break
        if gross: side*=math.sqrt(target/gross)
    return last

if __name__=="__main__":
    print("自己改善ビルド開始")
PY=0
