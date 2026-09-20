#!/usr/bin/env python3
"""MU 泊まれる無人Tシャツ店 — 美留和 を「実際に作れる・泊まれる」へ。
現 BIM (65要素) に、構造(柱梁)・熱源(薪ストーブ+煙突)・換気・防火警報・
無人店舗什器(セルフレジ/糸スキャンST/試着室/ハンガー)・薪サウナ・浄化槽を追加。
検証用に bim.house /mcp create_house で構造ASD+houkiを再判定する。
"""
import json, urllib.request, os, sys

BASE = "https://bim.house"
SLUG = "u-mut-m14jlqh8"

def http(url, payload=None, method="GET"):
    data = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(url, data=data, method=method,
                                 headers={"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read().decode())

# 現状要素を取得 (raw bim.json)
cur = http(f"{BASE}/api/showcase/{SLUG}/bim.json")
els = cur["elements"]
print(f"current elements: {len(els)}")

# 既存座標系: min-corner 原点。footprint x 2599..7999 (W5400), y 3293..10293 (D7000),
# 2層 z=0 / 2900, 軒 z=5800。対称配置で偏心を抑える。
X0, X1 = 2599, 7879      # 柱外面 (120角)
Y0, Y1 = 3293, 10173
YM = (Y0 + Y1) // 2      # 長辺中間
COL = 120
add = []

def E(cls, label, descr, x, y, z, w, d, h):
    add.append({"cls": cls, "label": label, "descr": descr, "shape": "box",
                "x": x, "y": y, "z": z, "w": w, "d": d, "h": h, "rotation": 0.0})

# ── 構造: 通し柱4隅 + 長辺中間2 (z=0..5800 h=5800) — 軸組の柱梁を明示
for (cx, cy, nm) in [(X0,Y0,"南西"),(X1,Y0,"南東"),(X0,Y1,"北西"),(X1,Y1,"北東")]:
    E("IFCCOLUMN", f"通し柱 ({nm})", "杉KD 120角 通し柱 (z0-軒) / 金物接合・N値計算対応",
      cx, cy, 0, COL, COL, 5800)
E("IFCCOLUMN", "管柱 (西中)", "杉KD 120角 管柱 / 各層", X0, YM, 0, COL, COL, 2900)
E("IFCCOLUMN", "管柱 (東中)", "杉KD 120角 管柱 / 各層", X1, YM, 0, COL, COL, 2900)
E("IFCCOLUMN", "管柱 (西中2F)", "杉KD 120角 管柱 / 各層", X0, YM, 2900, COL, COL, 2900)
E("IFCCOLUMN", "管柱 (東中2F)", "杉KD 120角 管柱 / 各層", X1, YM, 2900, COL, COL, 2900)

# ── 構造: 梁 (胴差 z=2900 / 軒桁 z=5800) 周囲 + 棟
for z, lv in [(2900, "胴差"), (5800, "軒桁")]:
    E("IFCBEAM", f"{lv} 南", "杉KD 平角 120x240 / 金物接合", X0, Y0, z, X1-X0+COL, 120, 240)
    E("IFCBEAM", f"{lv} 北", "杉KD 平角 120x240 / 金物接合", X0, Y1, z, X1-X0+COL, 120, 240)
    E("IFCBEAM", f"{lv} 東", "杉KD 平角 120x240 / 金物接合", X1, Y0, z, 120, Y1-Y0+COL, 240)
    E("IFCBEAM", f"{lv} 西", "杉KD 平角 120x240 / 金物接合", X0, Y0, z, 120, Y1-Y0+COL, 240)
E("IFCBEAM", "棟木", "杉KD 120x180 棟木", (X0+X1)//2, Y0, 6300, 120, Y1-Y0, 180)

# ── 熱源: 薪ストーブ + 煙突 (寒冷地・主暖房)
E("IFCENERGYCONVERSIONDEVICE", "薪ストーブ", "鋳鉄 薪ストーブ 8kW / 炉台ステンレス・離隔確保",
  3139, 6300, 135, 600, 600, 750)
E("IFCCHIMNEY", "煙突 (断熱二重)", "ステンレス断熱二重煙突 φ150 / 屋根貫通・トップ",
  3289, 6450, 885, 300, 300, 5800)

# ── 換気: 24h機械換気 (シックハウス 令20条の8) + レンジフード
E("IFCAIRTERMINALBOX", "24h換気 熱交換ユニット", "第一種 熱交換換気 (給排気) / フィルタ点検口",
  6800, 9600, 5300, 600, 400, 300)
E("IFCAIRTERMINAL", "レンジフード", "同時給排レンジフード / 寒冷地ダンパ", 5299, 3573, 2050, 750, 450, 300)

# ── 防火: 住宅用火災警報器 (寝室・階段・LDK)
for (cx, cy, cz, nm) in [(3139,5500,2700,"1F LDK"),(6487,8800,5640,"2F寝室"),(6799,4200,5640,"階段上")]:
    E("IFCALARM", f"火災警報器 ({nm})", "住宅用火災警報器 (煙式) / 電池式・相互連動", cx, cy, cz, 120, 120, 40)

# ── 無人店舗 什器 (1F土間・コンセプトの核)
E("IFCFURNISHINGELEMENT", "セルフレジ (QR決済)", "無人セルフレジ端末 / QR・タッチ決済・スマートロック連動",
  3500, 4000, 135, 500, 450, 1200)
E("IFCFURNISHINGELEMENT", "糸スキャンステーション", "NFC/QR 糸(ITO)スキャン台 / 服をかざすと両者に+1糸",
  4300, 4000, 135, 420, 420, 1050)
E("IFCFURNISHINGELEMENT", "ハンガーウォール", "壁面ハンガーレール / Tシャツ陳列 (土間)",
  2750, 4500, 1400, 200, 3000, 300)
E("IFCWALL", "試着室 壁A", "製材軸組 + PB / 試着室囲い (非耐力)", 6000, 5000, 0, 105, 1200, 2200)
E("IFCWALL", "試着室 壁B", "製材軸組 + PB / 試着室囲い (非耐力)", 6000, 5000, 0, 1000, 105, 2200)
E("IFCDOOR", "試着室カーテン", "遮像カーテン / 試着室", 6900, 5000, 0, 100, 900, 2000)

# ── 薪サウナ (外気浴デッキ脇・別棟小屋)
E("IFCBUILDINGELEMENTPROXY", "薪サウナ小屋", "杉板張り 薪サウナ 2帖 / 薪ストーブ式・外気浴デッキ隣接",
  3000, 600, 0, 2000, 2000, 2200)

# ── 設備: 合併浄化槽 (排水・寒冷地は凍結深以下に埋設)
E("IFCFLOWSTORAGEDEVICE", "合併浄化槽 (5人槽)", "FRP 合併処理浄化槽 5人槽 / 凍結深以下埋設・放流",
  1200, 8000, -1200, 1600, 1100, 1200)
# ── スマートロック (無人運営: 宿泊チェックインも兼ねる)
E("IFCBUILDINGELEMENTPROXY", "スマートロック (玄関)", "電子錠 / QR・暗証・遠隔解錠 (無人チェックイン)",
  3355, 3293, 900, 80, 160, 200)

print(f"added elements: {len(add)}")
allels = els + add
# id 再採番
for i, e in enumerate(allels, 1):
    e["id"] = f"e{i}"
print(f"total: {len(allels)}")

if "--verify" in sys.argv:
    # 検証用 private house を作って構造ASD+houkiを読む
    res = http(f"{BASE}/mcp", {
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "create_house", "arguments": {
            "name": "MU美留和 構造検証 (private)",
            "address": "北海道川上郡弟子屈町美留和",
            "structure": "杉CLT/軸組 + 籾殻断熱", "floors": 2, "gross_m2": 72,
            "land_area_m2": 300, "lot_w_m": 15, "lot_d_m": 20, "zoning": "無指定 (建ぺい70%/容積200%)",
            "private": True, "elements": allels,
        }}
    }, method="POST")
    txt = res["result"]["content"][0]["text"]
    obj = json.loads(txt)
    print("VERIFY slug:", obj.get("slug"))
    print("houki:", json.dumps(obj.get("houki"), ensure_ascii=False))
    print("structure:", json.dumps(obj.get("structure"), ensure_ascii=False))
    json.dump(allels, open("/tmp/ito_house_els.json", "w"))
    print("verify_slug=" + (obj.get("slug") or ""))

if "--apply" in sys.argv:
    tok = os.environ["ET"]
    body = {"project": {"name": "MU 泊まれる無人Tシャツ店 — 美留和",
                        "construction_jpy": 38000000, "proposed_gross_m2": 72,
                        "proposed_floors": 2,
                        "proposed_structure": "杉軸組+CLT床 / 通し柱・金物接合 (寒冷地・付加断熱)"},
            "elements": allels,
            "note": "実建築化: 柱梁明示+薪ストーブ/煙突+24h換気+火災警報器+無人店舗什器+薪サウナ+浄化槽"}
    res = http(f"{BASE}/api/projects/{SLUG}/bim?token={tok}", body, method="POST")
    print("APPLY:", json.dumps(res, ensure_ascii=False))
