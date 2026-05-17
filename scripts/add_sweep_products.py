#!/usr/bin/env python3
"""Add 10 BJJ-relevant products to sweep collab.

production_route='pre_order' so Stripe checkout works,
partner fulfills via Telegram alert + Resend email.
Once Printful product/variant IDs are confirmed,
flip production_route to 'printful'.
"""
import sqlite3
import datetime
from pathlib import Path

DB = Path(__file__).resolve().parent.parent / "store" / "products.db"

# New 10 products — fills gaps in BJJ-relevant categories
NEW_ITEMS = [
    # slug                    category                              name                                                price  lead  route
    ("sweep-patch-embroidered", "刺繍 Gi パッチ (4インチ)",         "MU × SIIIEEP Embroidered Gi Patch (4\")",            3800, 21, "pre_order"),
    ("sweep-patch-iron-on",     "アイロンプリント パッチ (3インチ)", "MU × SIIIEEP Iron-On Patch (3\")",                    1800, 14, "pre_order"),
    ("sweep-bandana",           "バンダナ / Gi ヘッドラップ",       "MU × SIIIEEP Bandana / Gi Headwrap",                 2800, 10, "pre_order"),
    ("sweep-polo-tech",         "パフォーマンスポロ (テック)",       "MU × SIIIEEP Performance Polo (Tech)",               9800, 14, "pre_order"),
    ("sweep-tech-tee",          "テックTシャツ (速乾)",             "MU × SIIIEEP Tech Tee (Quick-Dry)",                  5800, 10, "pre_order"),
    ("sweep-hip-pack-large",    "ヒップパック (大)",                "MU × SIIIEEP Hip Pack (Large)",                      7800, 14, "pre_order"),
    ("sweep-beach-towel",       "ビーチタオル (76×152cm)",          "MU × SIIIEEP Beach Towel (76×152cm)",                6800, 12, "pre_order"),
    ("sweep-hand-towel",        "ハンドタオル",                     "MU × SIIIEEP Hand Towel",                            3800, 10, "pre_order"),
    ("sweep-cinch-sack",        "ドローストリングバッグ (シンチ)",   "MU × SIIIEEP Drawstring Cinch Sack",                 4800, 10, "pre_order"),
    ("sweep-sherpa-blanket",    "シェルパフリースブランケット",      "MU × SIIIEEP Sherpa Fleece Blanket",                 9800, 14, "pre_order"),
]

DESCRIPTIONS = {
    "sweep-patch-embroidered": "Velcro裏地。柔術 Gi の袖/胸に取り付け可能。MU×SIIIEEP モノグラム刺繍。",
    "sweep-patch-iron-on":     "アイロン圧着式。デニム・コットン・ジムバッグに装着可能。",
    "sweep-bandana":            "53×53cm コットン。Gi下に巻く / 練習中の汗止め / グラップリング前の儀式に。",
    "sweep-polo-tech":          "審判・コーチ用テック素材。速乾・通気・吸汗。トーナメント運営に。",
    "sweep-tech-tee":           "Sport-Tek ST350 互換。毎日の練習用、汗をかいても重くならない速乾素材。",
    "sweep-hip-pack-large":     "通常 Fanny Pack より一回り大きい。スマホ + 鍵 + 小銭 + マウスガード + フィンガーテープ。",
    "sweep-beach-towel":        "76×152cm の大判。練習後・サウナ・温泉・ビーチ。両面プリント可能。",
    "sweep-hand-towel":         "マットサイドに置く小型タオル。練習中の汗・スパー間の拭き取り。",
    "sweep-cinch-sack":         "軽量ドローストリング。Gi + ラッシュガード + ボトル + タオル がギリギリ入る最小ジムバッグ。",
    "sweep-sherpa-blanket":     "シェルパフリース裏地、ふわふわで暖かい。練習後のリカバリー・自宅・別荘。",
}

def main():
    db = sqlite3.connect(DB)
    now = datetime.datetime.now().isoformat()
    added, skipped = 0, 0
    for slug, category, name, price, lead, route in NEW_ITEMS:
        existing = db.execute("SELECT id, active FROM collab_products WHERE slug=?", (slug,)).fetchone()
        if existing:
            print(f"  ◯ exists: {slug} (id={existing[0]}, active={existing[1]}) — skipping")
            skipped += 1
            continue
        db.execute("""INSERT INTO collab_products
            (slug, partner, category, name, description, price_jpy, active, draft,
             created_at, production_route, lead_time_days)
            VALUES (?, 'sweep', ?, ?, ?, ?, 1, 0, ?, ?, ?)""",
            (slug, category, name, DESCRIPTIONS.get(slug, ""), price, now, route, lead))
        added += 1
        print(f"  ✓ added: {slug} — {name} (¥{price:,})")
    db.commit()
    total = db.execute("SELECT COUNT(*) FROM collab_products WHERE partner='sweep' AND active=1").fetchone()[0]
    print()
    print(f"📊 Added: {added}, Skipped: {skipped}")
    print(f"📊 Total active sweep products now: {total}")

if __name__ == "__main__":
    main()
