#!/usr/bin/env python3
"""v9 高級・無人・ガーメント小屋: 最上位DTG(GTX600=自動ホワイト循環で詰まり対策)を小屋に残す。
恒湿ユニット/ペレット自動暖房/特定小規模自火報+誘導灯/凍結防止/ICT本人確認カメラ/巡回補充/高級内装。
"""
import json, urllib.request, os, sys
BASE="https://bim.house"; SLUG="u-mut-m14jlqh8"
def http(url,p=None,m="GET"):
    d=json.dumps(p).encode() if p is not None else None
    r=urllib.request.Request(url,data=d,method=m,headers={"content-type":"application/json"})
    with urllib.request.urlopen(r,timeout=60) as x: return json.loads(x.read().decode())

els=http(f"{BASE}/api/showcase/{SLUG}/bim.json")["elements"]
print("current:",len(els))
# 旧DTG(エントリ相当)を最上位機に置換。インク庫は残す。
REMOVE={"DTGプリンタ (Brother GTXpro 相当)"}
els=[e for e in els if e.get("label") not in REMOVE]
print("after remove:",len(els))
add=[]
def E(c,l,de,x,y,z,w,d,h): add.append({"cls":c,"label":l,"descr":de,"shape":"box","x":x,"y":y,"z":z,"w":w,"d":d,"h":h,"rotation":0.0})

# ── 高級ガーメント(小屋に残す)= 詰まり対策は最上位機の自動メンテで ──
E("IFCUNITARYEQUIPMENT","ガーメントプリンタ Brother GTX600 (最上位)",
  "業務用DTG最上位 / 白インク自動循環+定期自動クリーニングで間欠・無人でも詰まりを抑制 / 24h通電前提",6300,4200,135,1300,1100,1400)
E("IFCUNITARYEQUIPMENT","恒湿ユニット (工房 湿度40-60%)",
  "DTG安定印刷の湿度維持 / 白インク品質・ノズル詰まり対策 / 工房ゾーンのみ",6300,5500,135,500,400,1200)
# DTG硬化はヒートプレス(既設)。フィルム不要(小屋でフル印刷)。
# ── 無人の生存暖房 (薪は体験用) ──
E("IFCENERGYCONVERSIONDEVICE","ペレットストーブ (自動給じん)",
  "木質ペレット 自動給じん+サーモ / 無人で室温維持・数日自走 / 生存熱(薪サウナ・薪ストーブは体験用)",3139,7000,135,700,600,1100)
E("IFCFLOWSTORAGEDEVICE","ペレットホッパー (1週間分)","ペレット貯留 / 巡回補充",3139,7700,135,500,500,1200)
# ── 旅館業(特定防火対象物)自火報 全面積 + 誘導灯 ──
E("IFCALARM","特定小規模自火報 受信機","特定小規模施設用 自動火災報知設備 受信機 / 旅館業 全面積義務",6700,3500,1400,300,150,400)
for (x,y,nm,z) in [(4000,5000,"LDK",2700),(4000,8500,"水回り",2700),(4000,5000,"2F寝室",5600)]:
    E("IFCSENSOR",f"煙感知器 ({nm})","光電式煙感知器 / 相互連動",x,y,z,120,120,40)
E("IFCLIGHTFIXTURE","誘導灯 (玄関避難口)","避難口誘導灯 / 非常電源内蔵 / 旅館業義務",3355,3500,2050,250,80,200)
E("IFCLIGHTFIXTURE","誘導灯 (2F階段)","通路誘導灯 / 非常電源内蔵",6799,4000,5500,250,80,200)
# ── 無人運営(ICT帳場代替・本人確認は玄関のみ) ──
E("IFCSENSOR","本人確認カメラ (玄関・ICT帳場代替)","顔+顔写真付き本人確認書類を鮮明画像で / 玄関のみ・居室サウナ寝室は無カメラ",3355,3450,1900,120,120,120)
E("IFCFLOWMOVINGDEVICE","凍結防止ユニット (不凍液+ヒートトレース)","配管ヒートトレース+不凍液 / 長期空室は自動ドレン",2799,9300,135,400,300,500)
E("IFCFURNISHINGELEMENT","補充ストッカー (巡回用)","インク/白T/ペレット/リネン / 集落巡回でまとめ補充",7400,9300,135,500,500,1800)
# ── 高級(luxury) ──
E("IFCSANITARYTERMINAL","檜 露天風呂 (外気浴)","十和田石+檜 露天風呂 / 薪沸かし・摩周の星空 / 高級宿泊の核",3000,1000,0,1800,1600,700)
E("IFCCOVERING","内装 上質仕上 (左官+無垢)","珪藻土左官+ウォールナット無垢+真鍮金物 / 高級デザイン宿",2659,3353,121,5280,6880,3)

print("added:",len(add))
allels=els+add
for i,e in enumerate(allels,1): e["id"]=f"e{i}"
print("total:",len(allels))

if "--verify" in sys.argv:
    r=http(f"{BASE}/mcp",{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_house","arguments":{
        "name":"MU美留和 v9検証(private)","address":"北海道川上郡弟子屈町美留和","structure":"杉軸組+CLT床",
        "floors":2,"gross_m2":72,"land_area_m2":300,"lot_w_m":15,"lot_d_m":20,
        "zoning":"無指定 (建ぺい70%/容積200%)","private":True,"elements":allels}}},m="POST")
    o=json.loads(r["result"]["content"][0]["text"])
    print("VERIFY",o.get("slug"));print("houki:",json.dumps(o.get("houki"),ensure_ascii=False));print("structure:",json.dumps(o.get("structure"),ensure_ascii=False))

if "--apply" in sys.argv:
    tok=os.environ["ET"]
    body={"project":{"name":"MU 泊まれる無人Tシャツ店 — 美留和","construction_jpy":52000000,"proposed_gross_m2":72,"proposed_floors":2,
        "proposed_structure":"杉軸組+CLT床 / 高級無人民泊 + 最上位DTG工房 / オフグリッド(太陽光+ペレット) / 簡易宿所(自火報+誘導灯)"},
        "elements":allels,"note":"v9 高級・ガーメント小屋: 最上位DTG(GTX600・自動ホワイト循環)を小屋に残す+恒湿 / ペレット自動暖房 / 自火報+誘導灯 / 凍結防止+ICTカメラ+巡回補充 / 檜露天+上質内装"}
    print("APPLY:",json.dumps(http(f"{BASE}/api/projects/{SLUG}/bim?token={tok}",body,m="POST"),ensure_ascii=False))
