#!/usr/bin/env python3
"""INSERT 5 v3 brands × 4 SKUs each (research-driven expansion).

Brands: anime / wagyu / analog / quiet / roam
Research-driven (POD/Etsy 2026 bestseller analysis × Yuki's interests).
"""
import json
import sqlite3
from pathlib import Path

DB = Path("/Users/yuki/workspace/mu-brand/store/products.db")

NEW_BRANDS = [
    # slug, name, emoji, color, tagline, design_style, lifestyle_scene, ink_default
    ("anime", "MU × ANIME", "🌀", "#a855f7", "Mono no aware · sophisticated otaku",
     "Anime tribute, sophisticated older-fan aesthetic. Single character silhouette + episode metadata typography. NOT loud anime art — mono no aware feel.",
     "Tokyo Nakano Broadway hallway evening, person in tee browsing vinyl figurines under fluorescent light",
     "white"),
    ("wagyu", "MU × WAGYU", "🥩", "#b91c1c", "霜降り · 脂 = 静寂",
     "Japan-premium wagyu food culture. Marbled texture pattern + A5 grade typography + binchotan charcoal accent. Refined and meaty.",
     "Tokyo yakiniku restaurant private room, chef holding tongs over glowing binchotan, soft red ember light, marbled beef on tray",
     "gold"),
    ("analog", "MU × ANALOG", "📷", "#525252", "ISO 800 · Still developing",
     "Film photography / soft-stitch era. Silver-halide grain texture, frame counter glyph, exposure metadata stamp. Hand-developed feel.",
     "Tokyo darkroom amber safelight, person holding wet print over developing tray, hanging negatives behind, contact sheets on wall",
     "white"),
    ("quiet", "MU × QUIET", "🤫", "#374151", "Do not disturb · Deep work",
     "Introvert / deep-work culture. Minimal sans-serif + music rest notation + 'absence of sound' kanji 静. Library calm aesthetic.",
     "Tokyo library reading room early morning, single person at long wooden desk with closed laptop and a single notebook, soft natural light",
     "white"),
    ("roam", "MU × ROAM", "🚶", "#0f766e", "VISA RUN · 路 · GMT+0",
     "Traveler / nomad print. Border-stamp typography + visa-page motifs + path 路 calligraphy. Earthy palette.",
     "Narita Airport pre-dawn departures hall, person with duffel bag at empty check-in counter, soft blue cold light, suitcase tag visible",
     "white"),
]

NEW_SKUS = [
    # ANIME
    ("ANIME-TEE-01",   "FINAL ARC",        "anime",  3900,   71,  4017),
    ("ANIME-HOOD-01",  "EP. 1024",         "anime",  9800, 1543, 48770),
    ("ANIME-POST-01",  "RE-WATCH",         "anime",  2900,  171,  4530),  # poster
    ("ANIME-STICK-01", "戸惑い",            "anime",   800,  358, 10164),
    # WAGYU
    ("WAGYU-TEE-01",   "A5",               "wagyu",  3900,   71,  4017),
    ("WAGYU-APRON-01", "霜降り",            "wagyu",  4900,  297,  9287),  # apron
    ("WAGYU-MUG-01",   "炭火",              "wagyu",  2200,   19,  1320),
    ("WAGYU-TOTE-01",  "脂 = 静寂",         "wagyu",  2900,   19,  1320),
    # ANALOG
    ("ANALOG-TEE-01",  "ISO 800",          "analog", 3900,   71,  4017),
    ("ANALOG-TOTE-01", "1/125s",           "analog", 2900,   19,  1320),
    ("ANALOG-JOUR-01", "現像中",            "analog", 3500,   19,  1320),  # journal-ish, reuse mug variant
    ("ANALOG-STICK-01","35mm forever",     "analog",  800,  358, 10164),
    # QUIET
    ("QUIET-HOOD-01",  "DO NOT DISTURB",   "quiet",  9800, 1543, 48770),
    ("QUIET-TEE-01",   "DEEP WORK",        "quiet",  3900,   71,  4017),
    ("QUIET-MUG-01",   "音の不在",          "quiet",  2200,   19,  1320),
    ("QUIET-JOUR-01",  "off the grid",     "quiet",  3500,   19,  1320),
    # ROAM
    ("ROAM-TEE-01",    "VISA RUN",         "roam",   3900,   71,  4017),
    ("ROAM-HOOD-01",   "路",                "roam",   9800, 1543, 48770),
    ("ROAM-CAP-01",    "GMT+0",            "roam",   3500,  438, 12736),
    ("ROAM-TOTE-01",   "間",                "roam",   2900,   19,  1320),
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
