#!/usr/bin/env python3
"""3棟を正寸で確定: save API で手書きシェルを上書き→houki実測で検証→構造はprivate createで確認。"""
import json, urllib.request, build_houses as B
def http(u,p=None,m="GET"):
    d=json.dumps(p).encode() if p is not None else None
    r=urllib.request.Request(u,data=d,method=m,headers={"content-type":"application/json"})
    with urllib.request.urlopen(r,timeout=60) as x: return json.loads(x.read().decode())

def F(c,l,de,x,y,z,w,d,h): return {"cls":c,"label":l,"descr":de,"shape":"box","x":int(x),"y":int(y),"z":int(z),"w":int(w),"d":int(d),"h":int(h),"rotation":0.0}

def finalize(slug,token,W,D,floors,gross_t,name,gross_label,jpy,program):
    els=B.shell(W,D,floors,program)
    for i,e in enumerate(els,1): e["id"]="e%d"%i
    body={"project":{"name":name,"proposed_gross_m2":gross_label,"proposed_floors":floors,"construction_jpy":jpy,
           "proposed_structure":"杉軸組+CLT床 / 通し柱・耐力壁・金物接合 + オフグリッド(PV+蓄電+薪) + 無人DTF工房"},
          "elements":els,"note":f"正寸シェル {W/1000:.2f}x{D/1000:.2f}m x{floors}F = {gross_label}㎡ (手書き)"}
    s=http(f"https://bim.house/api/projects/{slug}/bim?token={token}",body,"POST")
    h=http(f"https://bim.house/api/houki/check/{slug}")
    # 構造はprivate createで読む
    o=json.loads(http("https://bim.house/mcp",{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_house","arguments":{
        "name":name+" 構造検証","address":"北海道川上郡弟子屈町美留和","floors":floors,"land_area_m2":2000,"lot_w_m":50,"lot_d_m":50,"private":True,"elements":els}}},"POST")["result"]["content"][0]["text"])
    print(f"{name}: save={s.get('ok')} v{s.get('version')} houki_gross={h['floor_area_ratio']['gross_floor_m2']} (目標{gross_t}) overall={h['overall_pass']} struct={o['structure']['pass']} ecc=({o['structure']['eccentricity_x']},{o['structure']['eccentricity_y']})")
    return h['floor_area_ratio']['gross_floor_m2'], h['overall_pass'], o['structure']['pass']

# SEED 9.9㎡ (3.0×3.3, 1F)
seed_prog=[
 F("IFCUNITARYEQUIPMENT","DTFヒートプレス","スポット硬化・低電力・無音に近い",1700,300,135,600,500,400),
 F("IFCSANITARYTERMINAL","コンポストトイレ","無水・無人運営向け",2300,2500,135,550,550,500),
 F("IFCFURNISHINGELEMENT","糸スキャン台","NFC/QR 糸ST",300,2400,135,380,380,1000),
]
finalize("u-museed99-hbshkbu8","et_6a254cf4tyhefvj15rtjtn7l",3000,3300,1,9.9,
         "MU 種小屋 SEED — 9.9㎡ 最小コスト",9.9,6500000,seed_prog)

# HEARTH 99㎡ (7.04×7.04, 2F=99.1)
h_prog=[
 F("IFCUNITARYEQUIPMENT","ガーメントDTG+ヒートプレス","Brother GTX級 自動白循環",900,900,135,1300,1000,1400),
 F("IFCFURNISHINGELEMENT","糸スキャンST","出会いで+1糸",2600,900,135,420,420,1050),
 F("IFCFURNISHINGELEMENT","土間ギャラリー什器","フォーク作品展示",900,3500,135,3000,400,1400),
 F("IFCFURNISHINGELEMENT","ベッド(2F)","ダブル",900,900,2900+135,1450,1950,450),
 F("IFCBUILDINGELEMENTPROXY","薪サウナ小屋","別棟2帖",-2600,500,0,2000,2000,2200),
 F("IFCSANITARYTERMINAL","檜露天風呂","外気浴・摩周の星",-2600,2800,0,1800,1600,700),
]
finalize("u-muhearth99-9yna5jwt","et_6a254cf8obmlc8ygfbj7s3lx",7040,7040,2,99.0,
         "MU 母屋 HEARTH — 99㎡",99.0,52000000,h_prog)

# ATSUME 180㎡ (9.49×9.49, 2F=180.1)
a_prog=[
 F("IFCUNITARYEQUIPMENT","大ガーメントアトリエ DTG#1","Brother GTX600 自動メンテ",900,900,135,1300,1100,1400),
 F("IFCUNITARYEQUIPMENT","大ガーメントアトリエ DTG#2","2台目・並列",2400,900,135,1300,1100,1400),
 F("IFCUNITARYEQUIPMENT","ヒートプレス","硬化",4000,900,135,600,700,400),
 F("IFCFURNISHINGELEMENT","フォーク作品ギャラリー","泊まった人の柄が並ぶ",900,4500,135,5000,400,1600),
 F("IFCFURNISHINGELEMENT","土間ラウンジ","焚き火を囲む",5500,5500,135,2500,2500,400),
 F("IFCFURNISHINGELEMENT","ベッド群(2F)","複数寝室",900,900,2900+135,1450,1950,450),
 F("IFCBUILDINGELEMENTPROXY","大薪サウナ","別棟4帖",-3000,500,0,2600,2600,2300),
 F("IFCSANITARYTERMINAL","檜大露天","星見・外気浴",-3000,3500,0,2600,2000,700),
 F("IFCFURNISHINGELEMENT","星見デッキ","屋上テラス",1000,1000,2900*2,4000,3000,150),
]
finalize("u-muatsume180-b58i4dxb","et_6a254cfc4jhdevpadff5kvlg",9490,9490,2,180.0,
         "MU 集 ATSUME — 180㎡ 体験マックス",180.0,98000000,a_prog)
