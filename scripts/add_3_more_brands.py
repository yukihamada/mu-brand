#!/usr/bin/env python3
"""INSERT 3 more MU brands × 4 SKUs each (v2 expansion).

Brands: news / kagi / chip
"""
import json
import sqlite3
from pathlib import Path

DB = Path("/Users/yuki/workspace/mu-brand/store/products.db")

NEW_BRANDS = [
    # slug, name, emoji, color, tagline, design_style, lifestyle_scene, ink_default
    ("news", "MU × NEWS", "📡", "#06b6d4", "T-minus 0 · No comment",
     "News / journalism aesthetic. Telegraph monospace + cyan accent + date-stamped panels. Single-color screen-print.",
     "Tokyo press room late at night, person typing at desk, multiple monitors with news feed glow, soft cyan light",
     "white"),
    ("kagi", "MU × KAGI", "🔑", "#e6c449", "鍵あり · presence",
     "KAGI smart-home brand. Matte black + brass key cylinder geometry. Single line keychain illustration.",
     "Tokyo apartment doorway dusk, person turning the doorknob to leave, soft hallway light, gold key in hand",
     "gold"),
    ("chip", "MU × CHIP", "⚡", "#22c55e", "ESP32 + ❤ · Solder on",
     "Hardware / maker print. PCB green + silver solder + IC pin grid pattern. Pixel-perfect technical diagram.",
     "Maker workspace garage, person at soldering bench with magnifier and breadboard, warm tungsten lamp",
     "white"),
]

# Reuse known-good Printful variants from v1 expansion
NEW_SKUS = [
    # NEWS
    ("NEWS-TEE-01",   "BREAKING ▮",     "news",  3900,   71,  4017),
    ("NEWS-HOOD-01",  "T-MINUS 0",      "news",  9800, 1543, 48770),
    ("NEWS-JOUR-01",  "EMBARGOED ✕",    "news",  3500,   19,  1320),
    ("NEWS-STICK-01", "NO COMMENT",     "news",   800,  358, 10164),
    # KAGI
    ("KAGI-TEE-01",   "鍵あり ◯",        "kagi",  3900,   71,  4017),
    ("KAGI-CAP-01",   "PRESENCE",       "kagi",  3500,  438, 12736),
    ("KAGI-MUG-01",   "LOCK · UNLOCK",  "kagi",  2200,   19,  1320),
    ("KAGI-TOTE-01",  "鍵束 12",         "kagi",  2900,   19,  1320),
    # CHIP
    ("CHIP-TEE-01",   "ESP32 + ❤",      "chip",  3900,   71,  4017),
    ("CHIP-HOOD-01",  "SOLDER ON",      "chip",  9800, 1543, 48770),
    ("CHIP-MUG-01",   "PCB ART",        "chip",  2200,   19,  1320),
    ("CHIP-STICK-01", "FW v0.1",        "chip",   800,  358, 10164),
]


def main():
    conn = sqlite3.connect(str(DB))
    cur = conn.cursor()

    for slug, name, emoji, color, tagline, style, scene, ink in NEW_BRANDS:
        cfg = {"design_style": style, "lifestyle_scene": scene, "ink_default": ink}
        cur.execute("""
            INSERT OR REPLACE INTO catalog_brands
              (slug, name, emoji, color_primary, tagline, is_active, revenue_share_pct, config_json)
            VALUES (?, ?, ?, ?, ?, 1, 0, ?)
        """, (slug, name, emoji, color, tagline, json.dumps(cfg, ensure_ascii=False)))
        print(f"  brand: {slug}")

    for sku, label, brand, price, pid, vid in NEW_SKUS:
        cur.execute("""
            INSERT OR REPLACE INTO catalog_products
              (sku, brand, label, description_ja, retail_price_jpy,
               printful_product_id, printful_variant_id, printful_placement,
               printful_print_w, printful_print_h,
               is_active, sort_order, status, fulfillment_route)
            VALUES (?, ?, ?, ?, ?, ?, ?, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg')
        """, (sku, brand, label,
              f"MU × {brand.upper()} · {label}",
              price, pid, vid))
        print(f"  sku: {sku} ({brand}, ¥{price:,})")

    conn.commit()
    conn.close()
    print(f"\n{len(NEW_BRANDS)} brands, {len(NEW_SKUS)} SKUs inserted")


if __name__ == "__main__":
    main()
