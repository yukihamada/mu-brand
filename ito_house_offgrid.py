#!/usr/bin/env python3
"""v8 稼ぐ家化: レジ撤去・前処理レス・ヒートプレス硬化・オフグリッド設備・遮音工房。
現 BIM (110) から セルフレジ/コンベア乾燥機/前処理ブース/コンプレッサ を撤去し、
ヒートプレス・蓄電池20kWh・ハイブリッドパワコン・PV増設4kW・薪棚・遮音工房壁 を追加。
"""
import json, urllib.request, os, sys
BASE = "https://bim.house"; SLUG = "u-mut-m14jlqh8"

def http(url, payload=None, method="GET"):
    data = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(url, data=data, method=method,
                                 headers={"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read().decode())

cur = http(f"{BASE}/api/showcase/{SLUG}/bim.json")
els = cur["elements"]
print("current:", len(els))

# ── 撤去 (レジ不要・静音化・前処理レス) ──
REMOVE = {"セルフレジ (QR決済)", "キュア乾燥機 (コンベア)", "前処理ブース (プリトリート)", "静音エアコンプレッサ"}
els = [e for e in els if e.get("label") not in REMOVE]
print("after remove:", len(els))

add = []
def E(cls, label, descr, x, y, z, w, d, h):
    add.append({"cls": cls, "label": label, "descr": descr, "shape": "box",
                "x": x, "y": y, "z": z, "w": w, "d": d, "h": h, "rotation": 0.0})

# ── 作る: ヒートプレス (スポット硬化・低総電力・無音に近い) + 証明印刷ステーション ──
E("IFCUNITARYEQUIPMENT", "ヒートプレス (40x50cm)",
  "スウィングアウェイ熱プレス 1.8kW / インク定着スポット硬化 / 連続運転なし=低電力・静音",
  7000, 4200, 135, 600, 700, 400)
E("IFCFURNISHINGELEMENT", "証明印刷ステーション (タブレット)",
  "その夜のデータ(気温/積雪/出会い=糸)→一点もの生成→印刷。決済は予約に自動精算(レジなし)",
  3500, 4000, 135, 450, 400, 1100)

# ── 自然エネルギーだけ: 電気=太陽光+蓄電 / 熱=薪(バイオマス) ──
E("IFCENERGYCONVERSIONDEVICE", "太陽光パネル 増設 (+4kW=計8kW)",
  "単結晶PV +4kW / 冬季・積雪を見込み大きめ設計 (計8kW)", 5331, 3713, 6030, 2268, 1260, 80)
E("IFCELECTRICFLOWSTORAGEDEVICE", "蓄電池バンク 20kWh (LiFePO4)",
  "リン酸鉄リチウム 20kWh / 曇雪日2-3日分・印刷は晴天バッチ前提", 7600, 9300, 135, 700, 500, 1400)
E("IFCUNITARYEQUIPMENT", "ハイブリッドパワコン (5.5kW)",
  "太陽光+蓄電 ハイブリッドインバータ / 自立運転・系統なしオフグリッド", 7600, 8700, 1200, 400, 250, 600)
E("IFCBUILDINGELEMENTPROXY", "薪棚 (3畳・1冬分)",
  "屋根付き薪ストック 約4-5棚 / 暖房・給湯・サウナ・冬の熱源=森のバイオマス(自然エネルギー)",
  900, 1200, 0, 3000, 900, 1800)

# ── 静けさ: 遮音した工房ゾーン (寝室と縁を切る) ──
E("IFCWALL", "遮音間仕切り 工房A",
  "製材軸組 + 高密度GW + PB二重 / 遮音 Dr-35級 工房囲い (非耐力)", 5900, 3800, 0, 105, 2800, 2900)
E("IFCWALL", "遮音間仕切り 工房B",
  "製材軸組 + 高密度GW + PB二重 / 遮音 Dr-35級 工房囲い (非耐力)", 5900, 3800, 0, 2050, 105, 2900)

print("added:", len(add))
allels = els + add
for i, e in enumerate(allels, 1):
    e["id"] = f"e{i}"
print("total:", len(allels))

if "--verify" in sys.argv:
    res = http(f"{BASE}/mcp", {"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":"create_house","arguments":{
            "name":"MU美留和 v8検証 (private)","address":"北海道川上郡弟子屈町美留和",
            "structure":"杉軸組+CLT床","floors":2,"gross_m2":72,
            "land_area_m2":300,"lot_w_m":15,"lot_d_m":20,
            "zoning":"無指定 (建ぺい70%/容積200%)","private":True,"elements":allels}}}, method="POST")
    obj = json.loads(res["result"]["content"][0]["text"])
    print("VERIFY slug:", obj.get("slug"))
    print("houki:", json.dumps(obj.get("houki"), ensure_ascii=False))
    print("structure:", json.dumps(obj.get("structure"), ensure_ascii=False))

if "--apply" in sys.argv:
    tok = os.environ["ET"]
    body = {"project":{"name":"MU 泊まれる無人Tシャツ店 — 美留和",
                       "construction_jpy":43000000,"proposed_gross_m2":72,"proposed_floors":2,
                       "proposed_structure":"杉軸組+CLT床 / オフグリッド(PV8kW+蓄電20kWh+薪) + 無人DTGプリント工房(遮音)"},
            "elements":allels,
            "note":"v8 稼ぐ家化: レジ撤去・前処理レス・ヒートプレス硬化・PV8kW/蓄電20kWh/薪(自然エネルギーのみ)・遮音工房"}
    print("APPLY:", json.dumps(http(f"{BASE}/api/projects/{SLUG}/bim?token={tok}", body, method="POST"), ensure_ascii=False))
