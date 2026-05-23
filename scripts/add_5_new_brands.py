#!/usr/bin/env python3
"""INSERT 5 new MU brands × 4 SKUs each into catalog DB.

Brands: voice / ocean / lodge / octagon / founder
Each gets 4 starter SKUs covering different product types using existing
Printful variant IDs (reused — same shirt blank, different design label).
"""
import sqlite3
from pathlib import Path

DB = Path("/Users/yuki/workspace/mu-brand/store/products.db")

NEW_BRANDS = [
    # slug,    name,                emoji, color,      tagline
    ("voice",   "MU × VOICE",        "🎤",   "#9333ea", "First the word · Koe-first apparel"),
    ("ocean",   "MU × OCEAN",        "🌊",   "#0ea5e9", "Aloha ◐ MU · Salt year"),
    ("lodge",   "MU × LODGE",        "🏔️",   "#92400e", "弟子屈 hut life · 杉 forever"),
    ("octagon", "MU × OCTAGON",      "🥊",   "#dc2626", "Walk-out · 朱と群青"),
    ("founder", "MU × FOUNDER",      "🚀",   "#1f2937", "20 years shipping · Still early"),
]

# Each tuple: (sku, label, concept_name, product_type, printful_pid, printful_vid, price)
# Reusing known-good Printful variants from MU-BJJ-01 set.
NEW_SKUS = [
    # ── VOICE
    ("VOICE-TEE-01",    "FIRST WORD",       "voice", "TEE-BLACK",  71,   4017,  3900),
    ("VOICE-HOOD-01",   "WAV.MU",           "voice", "HOODIE-BLACK", 1543, 48770, 9800),
    ("VOICE-MUG-01",    "聞こえる",          "voice", "MUG",        19,   1320,  2200),
    ("VOICE-STICK-01",  "NO TYPE",          "voice", "STICKER",    358,  10164, 800),
    # ── OCEAN
    ("OCEAN-TEE-01",    "ALOHA・MU",        "ocean", "TEE-BLACK",  71,   4017,  3900),
    ("OCEAN-TANK-01",   "SALT YEAR",        "ocean", "TANK",       145,  3431,  3900),
    ("OCEAN-TOTE-01",   "PACIFIC TIME",     "ocean", "TOTE",       19,   1320,  2900),
    ("OCEAN-TEE-WHITE-01","波 ◐ MOON",      "ocean", "TEE-WHITE",  71,   4012,  3900),
    # ── LODGE
    ("LODGE-HOOD-01",   "WINTER STAY",      "lodge", "HOODIE-BLACK", 1543, 48770, 9800),
    ("LODGE-LST-01",    "杉 = 永遠",         "lodge", "LONG-SLEEVE-BLACK", 356, 10096, 5800),
    ("LODGE-BEAN-01",   "FIRE BUILT",       "lodge", "BEANIE",     205,  6754,  3200),
    ("LODGE-CANVAS-01", "1100 KM SOUTH",    "lodge", "CANVAS",     3,    16,    7800),
    # ── OCTAGON
    ("OCT-TEE-01",      "WALK OUT",         "octagon", "TEE-BLACK",  71,   4017,  3900),
    ("OCT-RASH-01",     "5 ROUNDS",         "octagon", "RASH",     301,  9328,  6800),
    ("OCT-TEE-RED-01",  "朱と群青",          "octagon", "TEE-RED",  71,   4014,  3900),
    ("OCT-CAP-01",      "OCTAGON ◯",        "octagon", "CAP",      438,  12736, 3500),
    # ── FOUNDER
    ("FOUND-TEE-01",    "20 YEARS SHIPPING","founder", "TEE-BLACK",  71,   4017,  3900),
    ("FOUND-HOOD-01",   "STILL EARLY",      "founder", "HOODIE-BLACK", 1543, 48770, 9800),
    ("FOUND-CAP-01",    "CEO・MU",          "founder", "CAP",      438,  12736, 3500),
    ("FOUND-MUG-01",    "DAY 1 EVERY DAY",  "founder", "MUG",      19,   1320,  2200),
]

def main():
    conn = sqlite3.connect(str(DB))
    cur = conn.cursor()

    # 1. brands
    for slug, name, emoji, color, tagline in NEW_BRANDS:
        cur.execute("""
            INSERT OR REPLACE INTO catalog_brands
              (slug, name, emoji, color_primary, tagline, is_active, revenue_share_pct, config_json)
            VALUES (?, ?, ?, ?, ?, 1, 0, '{}')
        """, (slug, name, emoji, color, tagline))
        print(f"  brand: {slug}")

    # 2. SKUs
    for sku, label, brand, ptype, pid, vid, price in NEW_SKUS:
        cur.execute("""
            INSERT OR REPLACE INTO catalog_products
              (sku, brand, label, description_ja, retail_price_jpy,
               printful_product_id, printful_variant_id, printful_placement,
               printful_print_w, printful_print_h,
               is_active, sort_order, status, fulfillment_route)
            VALUES (?, ?, ?, ?, ?, ?, ?, 'front', 4500, 5400, 1, 100, 'live', 'printful_dtg')
        """, (sku, brand, label,
              f"MU × {brand.upper()} #{sku.split('-')[-1]} · {ptype} · {label}",
              price, pid, vid))
        print(f"  sku: {sku} ({brand}, ¥{price:,})")

    conn.commit()
    conn.close()
    print(f"\n{len(NEW_BRANDS)} brands, {len(NEW_SKUS)} SKUs inserted")


if __name__ == "__main__":
    main()
