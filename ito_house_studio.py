#!/usr/bin/env python3
"""B: 無人プリント工房を追加 — その場で1枚からTシャツを刷れる設計に。
現 BIM (98要素) に DTGプリンタ / 前処理 / キュア乾燥 / 白T在庫 / 乾燥ラック /
検品畳み / インク保管 / 局所排気 / 動力盤 / 消火器 / コンプレッサ を追加。
電源・局所排気・防火の要件も要素として明示。検証→本体適用。
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

add = []
def E(cls, label, descr, x, y, z, w, d, h):
    add.append({"cls": cls, "label": label, "descr": descr, "shape": "box",
                "x": x, "y": y, "z": z, "w": w, "d": d, "h": h, "rotation": 0.0})

# ── 無人プリント工房 (1F 東側ワークゾーン z=135) — その場で1枚から刷る一筆書き
E("IFCUNITARYEQUIPMENT", "DTGプリンタ (Brother GTXpro 相当)",
  "ダイレクト・トゥ・ガーメント印刷機 / 単相200V・白+CMYK・1枚から / 自社1枚オンデマンド",
  6300, 4200, 135, 1300, 1000, 1400)
E("IFCUNITARYEQUIPMENT", "前処理ブース (プリトリート)",
  "前処理液 自動スプレー + 局所排気ブース / 白印刷の下地・水性",
  6300, 5400, 135, 1000, 800, 1500)
E("IFCUNITARYEQUIPMENT", "キュア乾燥機 (コンベア)",
  "インク定着 コンベア乾燥機 160℃ / 熱源・局所排気必須・離隔確保",
  7000, 4200, 135, 1500, 700, 900)
E("IFCFURNISHINGELEMENT", "白T在庫棚 (ブランク)",
  "Bella+Canvas 3001 白/黒 ブランク在庫棚 / サイズ別・補充検知",
  7400, 5600, 135, 400, 1800, 2000)
E("IFCFURNISHINGELEMENT", "乾燥・冷却ラック",
  "刷り上がり 乾燥/冷却ラック / 多段", 7000, 6000, 135, 700, 600, 1800)
E("IFCFURNISHINGELEMENT", "検品・畳み・梱包カウンター",
  "検品 → 畳み → 糸タグ封入 → セルフレジ受け渡し / 作業台",
  6300, 6200, 135, 1500, 700, 900)
E("IFCFURNISHINGELEMENT", "インク・前処理液 保管庫 (防火)",
  "水性インク/前処理液 保管 / SDS掲示・転倒防止・施錠 (危険物該当外の水性)",
  7500, 4200, 135, 400, 500, 1200)
# 設備要件: 局所排気・動力電源・防火
E("IFCAIRTERMINAL", "局所排気ファン (乾燥機/前処理上)",
  "プッシュプル局所排気 / キュア熱・前処理ミスト屋外排出", 7000, 4200, 2400, 500, 400, 300)
E("IFCELECTRICDISTRIBUTIONBOARD", "動力盤 (単相200V/契約増設)",
  "プリンタ・乾燥機用 専用回路 / 単相200V・漏電遮断・契約電力増設", 7800, 3500, 1400, 120, 300, 500)
E("IFCFIRESUPPRESSIONTERMINAL", "消火器 (工房A)",
  "ABC粉末消火器 / 乾燥機(熱源)近接配置", 6900, 4900, 135, 200, 200, 500)
E("IFCFIRESUPPRESSIONTERMINAL", "消火器 (玄関B)",
  "ABC粉末消火器 / 出入口避難動線", 3355, 3600, 135, 200, 200, 500)
E("IFCUNITARYEQUIPMENT", "静音エアコンプレッサ",
  "前処理スプレー用 静音コンプレッサ / 防振架台", 7500, 6200, 135, 400, 600, 600)

print("added:", len(add))
allels = els + add
for i, e in enumerate(allels, 1):
    e["id"] = f"e{i}"
print("total:", len(allels))

if "--verify" in sys.argv:
    res = http(f"{BASE}/mcp", {"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "create_house", "arguments": {
            "name": "MU美留和 工房検証 (private)", "address": "北海道川上郡弟子屈町美留和",
            "structure": "杉軸組+CLT床", "floors": 2, "gross_m2": 72,
            "land_area_m2": 300, "lot_w_m": 15, "lot_d_m": 20,
            "zoning": "無指定 (建ぺい70%/容積200%)", "private": True, "elements": allels}}}, method="POST")
    obj = json.loads(res["result"]["content"][0]["text"])
    print("VERIFY slug:", obj.get("slug"))
    print("houki:", json.dumps(obj.get("houki"), ensure_ascii=False))
    print("structure:", json.dumps(obj.get("structure"), ensure_ascii=False))

if "--apply" in sys.argv:
    tok = os.environ["ET"]
    body = {"project": {"name": "MU 泊まれる無人Tシャツ店 — 美留和",
                        "construction_jpy": 41000000, "proposed_gross_m2": 72, "proposed_floors": 2,
                        "proposed_structure": "杉軸組+CLT床 / 通し柱・金物接合 + 無人DTGプリント工房"},
            "elements": allels,
            "note": "B案: 無人プリント工房 (DTG/前処理/キュア乾燥/在庫/検品/局所排気/動力盤/消火器) を追加 — その場で1枚から刷れる"}
    print("APPLY:", json.dumps(http(f"{BASE}/api/projects/{SLUG}/bim?token={tok}", body, method="POST"), ensure_ascii=False))
