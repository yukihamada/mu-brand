#!/usr/bin/env python3
"""Add 20 ad-optimized T-shirt SKUs to wearmu (mu-store).

Targets niches identified as high-CVR / low-CPC on Google Ads:
  - 柔術 (NOGI / belt rank / 技名)
  - 地域 (三田 / 北参道 / 弟子屈 / 川湯)
  - 焼肉 古今 collab + gift intent
  - 業界部族 (看護師 / エンジニア)
  - イベント記念 + 父の日 gift

Inserted with active=0 (draft) so they don't appear on the live storefront
until a design is attached and you flip active=1 from /admin/products.
prompt_text is populated for downstream design generation.
"""
import sqlite3
import datetime
from pathlib import Path

DB = Path(__file__).resolve().parent.parent / "store" / "products.db"

# (brand, drop_num, name, price_jpy, inventory, ad_keyword_hint, design_prompt)
SKUS = [
    # ---- ads_jujitsu: 柔術ニッチ (8) ----
    ("ads_jujitsu", 1, "NOGI 柔術家 Tシャツ — Black",
        4900, 30, "ノーギ Tシャツ",
        "Minimalist single-color line drawing of a NOGI grappler in seated guard, "
        "white ink on black tee, centered chest, 3 colors max"),
    ("ads_jujitsu", 2, "NOGI 柔術家 Tシャツ — White",
        4900, 30, "ノーギ Tシャツ メンズ",
        "Same composition as Black variant, reversed: black ink on white tee"),
    ("ads_jujitsu", 3, "白帯 Day One Tee",
        4900, 20, "白帯 Tシャツ 柔術 プレゼント",
        "Single white belt knot illustration with text 'DAY ONE / 白帯' below, "
        "subtle, gift-card vibe, navy tee"),
    ("ads_jujitsu", 4, "青帯 Tee — One Bar Down",
        4900, 20, "青帯 Tシャツ",
        "Blue belt with single black bar, text 'ONE BAR DOWN / 青帯' below"),
    ("ads_jujitsu", 5, "紫帯 Tee — The Long Game",
        4900, 20, "紫帯 Tシャツ",
        "Purple belt with 2 black bars, text 'THE LONG GAME' in serif"),
    ("ads_jujitsu", 6, "黒帯 Tee — Decade",
        5500, 15, "黒帯 Tシャツ 柔術",
        "Black belt with red bar, classic BJJ rank-belt format (not federation-specific), "
        "text 'DECADE' below, premium heavy-weight tee feel"),
    ("ads_jujitsu", 7, "Tap or Nap Tee",
        4900, 30, "柔術 Tシャツ 面白い",
        "Bold sans-serif typography 'TAP OR NAP' across chest, slight retro 80s feel"),
    ("ads_jujitsu", 8, "Berimbolo Specialist Tee",
        4900, 20, "ベリンボロ Tシャツ",
        "Line-art diagram of berimbolo motion sequence (arrows + body shapes), "
        "text 'BERIMBOLO / SPECIALIST' below"),

    # ---- ads_regional: 地域プライド (4) ----
    ("ads_regional", 1, "三田 / MITA Tee",
        4900, 20, "三田 Tシャツ 港区",
        "Vintage city-tag layout: 'MITA / 三田 / MINATO TOKYO' in bold athletic font, "
        "small Tokyo Tower silhouette as accent"),
    ("ads_regional", 2, "北参道 BJJ Tee",
        4900, 20, "北参道 柔術 Tシャツ",
        "Bold serif '北参道 BJJ' chest type, small Tokyo neighborhood tag bottom hem, "
        "subdued (gym merch energy)"),
    ("ads_regional", 3, "弟子屈 / TESHIKAGA Tee",
        4900, 20, "弟子屈 Tシャツ 北海道",
        "Mount Mashu (摩周湖) silhouette + '弟子屈 / TESHIKAGA / HOKKAIDO' city-tag layout, "
        "muted earth tones"),
    ("ads_regional", 4, "川湯温泉 ONSEN Tee",
        4900, 20, "川湯温泉 Tシャツ お土産",
        "Vintage onsen postcard aesthetic: '川湯温泉' kanji + steam swirl illustration + "
        "'KAWAYU ONSEN HOKKAIDO' subtag"),

    # ---- ads_kokon: 焼肉古今 collab (3) ----
    ("ads_kokon", 1, "焼肉古今 Logo Tee",
        4900, 30, "焼肉 Tシャツ 古今 kokon",
        "Restaurant-merch energy: '焼肉古今 / kokon.tokyo' centered chest, "
        "single charcoal-ember icon, premium hand-feel"),
    ("ads_kokon", 2, "焼肉好き Gift Tee — 'Meat is Life'",
        4900, 30, "焼肉 Tシャツ プレゼント",
        "Bold display type '焼肉 / 好き' with small wagyu marbling illustration, "
        "humor-gift positioning"),
    ("ads_kokon", 3, "肉 FRIDAY Tee",
        4900, 20, "肉の日 Tシャツ ギフト",
        "Calendar-block design: 29 highlighted '肉の日' (every 29th), tongue-in-cheek "
        "office-wear feel"),

    # ---- ads_profession: 業界部族 (2) ----
    ("ads_profession", 1, "ICU Night Shift Tee 看護師",
        4900, 20, "看護師 Tシャツ ICU プレゼント",
        "Minimalist EKG line transitioning to text 'ICU / NIGHT SHIFT', "
        "deep navy tee, gift positioning for nurse"),
    ("ads_profession", 2, "Coffee → Code → Compile Tee",
        4900, 20, "エンジニア Tシャツ プログラマー",
        "Three-icon row: coffee cup, terminal cursor, hammer; "
        "monospaced font, dev-conference giveaway energy"),

    # ---- ads_event: イベント記念 + ギフト (3) ----
    ("ads_event", 1, "SOLUNA FEST HAWAII 2026 Tee",
        5500, 50, "SOLUNA FEST 2026 Tシャツ",
        "Festival-merch layout: 'SOLUNA FEST / HAWAII 2026' large display type, "
        "sun/moon dual icon, list of dates on back"),
    ("ads_event", 2, "父の日 釣り好きDad Tee",
        4900, 20, "父の日 プレゼント 釣り Tシャツ",
        "Vintage fishing-camp badge: 'WORLD'S OK-EST FISHING DAD' + bass silhouette, "
        "Father's Day gift-search aimed"),
    ("ads_event", 3, "東日本グラップリングオープン 2026 記念Tee",
        5500, 30, "グラップリング 大会 Tシャツ 記念",
        "Tournament-bracket inspired layout, generic 'EAST JAPAN / GRAPPLING OPEN 2026' "
        "centered, original chest crest design — NO federation logos or names"),
]


def find_design_file(brand: str, drop_num: int) -> str | None:
    """Locate the generated PNG for this SKU. Files are named
    <brand>_<drop:03d>_<hash>.png by scripts/gen_ads_designs.py."""
    from pathlib import Path
    ads_dir = Path(__file__).resolve().parent.parent / "store" / "static" / "ads"
    if not ads_dir.exists():
        return None
    prefix = f"{brand}_{drop_num:03d}_"
    matches = sorted(ads_dir.glob(f"{prefix}*.png"))
    if not matches:
        return None
    # If multiple (older + regenerated), pick the most recently modified
    matches.sort(key=lambda p: p.stat().st_mtime, reverse=True)
    return f"/static/ads/{matches[0].name}"


def main():
    db = sqlite3.connect(DB)
    now = datetime.datetime.now().isoformat()
    added, skipped, activated = 0, 0, 0
    for brand, drop_num, name, price, inventory, ad_keyword, prompt in SKUS:
        existing = db.execute(
            "SELECT id, active, design_url FROM products WHERE brand=? AND drop_num=?",
            (brand, drop_num),
        ).fetchone()
        design_url = find_design_file(brand, drop_num)
        if existing:
            pid, cur_active, cur_design = existing
            # Update design_url + activate if design exists and not already set
            if design_url and (not cur_design or cur_active == 0):
                db.execute(
                    "UPDATE products SET design_url=?, mockup_url=?, active=1 WHERE id=?",
                    (design_url, design_url, pid),
                )
                activated += 1
                print(f"  ↑ activated: {brand} #{drop_num} (id={pid}) → {design_url}")
            else:
                print(f"  ◯ exists: {brand} #{drop_num} (id={pid}, active={cur_active}) — skipping")
                skipped += 1
            continue
        serial = f"{brand.upper().replace('ADS_','')}-{drop_num:03d}"
        full_prompt = f"[Ad keyword: {ad_keyword}] {prompt}"
        active = 1 if design_url else 0
        db.execute(
            """INSERT INTO products
            (brand, drop_num, name, price_jpy, inventory, sold,
             created_at, active, city_slug, prompt_text, serial_code,
             design_url, mockup_url)
            VALUES (?, ?, ?, ?, ?, 0, ?, ?, 'teshikaga', ?, ?, ?, ?)""",
            (brand, drop_num, name, price, inventory, now, active,
             full_prompt, serial, design_url, design_url),
        )
        added += 1
        marker = "✓ added+active" if active else "✓ added (draft)"
        print(f"  {marker}: {brand} #{drop_num} — {name} (¥{price:,}, qty={inventory})")
    db.commit()
    print()
    print(f"📊 Added: {added}, Activated: {activated}, Skipped: {skipped}")

    print()
    print("=" * 70)
    print(f"📊 Added: {added}, Skipped: {skipped}")
    by_brand = db.execute(
        "SELECT brand, COUNT(*), SUM(inventory*price_jpy) "
        "FROM products WHERE brand LIKE 'ads_%' GROUP BY brand ORDER BY 1"
    ).fetchall()
    print()
    print(f"{'brand':<20} {'skus':>6} {'inventory $ JPY':>20}")
    for b, c, gmv in by_brand:
        print(f"{b:<20} {c:>6} {gmv:>20,}")
    total = db.execute(
        "SELECT COUNT(*), SUM(inventory*price_jpy) FROM products WHERE brand LIKE 'ads_%'"
    ).fetchone()
    print(f"{'TOTAL':<20} {total[0]:>6} {total[1]:>20,}")
    print()
    print("Re-run after generating new designs to auto-activate them.")


if __name__ == "__main__":
    main()
