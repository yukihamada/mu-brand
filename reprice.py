#!/usr/bin/env python3
"""Retroactively reprice all products using stored weather data + brand formula."""
import os, json, sqlite3, requests
from pathlib import Path

DB_PATH    = Path(__file__).parent / "products.db"
STORE_URL  = os.environ.get("MU_STORE_URL", "https://wearmu.com")
ADMIN_TOKEN = os.environ.get("MU_ADMIN_TOKEN", "mu-admin")

def recalc_price(brand: str, weather: dict, drop_num: int, name: str = "") -> int:
    temp = weather.get("temp_c", 15)
    wind = weather.get("wind_kmh", 10)
    if brand == "muon":
        return max(3000, round(temp * 1000 / 1000) * 1000)
    elif brand == "mugen":
        if drop_num == 108:
            return 30000
        return max(2000, round((3000 + wind * 150) / 1000) * 1000)
    elif brand == "ma":
        # 2026-05-11: MA starting bid lowered from ¥120k → ¥30k when cadence
        # changed from monthly to weekly 7-day auctions.
        return 30000
    elif brand == "nouns":
        nm = name.upper()
        if "MA" in nm or "間" in nm:
            return 30000
        elif "MUON" in nm:
            return max(3000, round(temp * 1000 / 1000) * 1000)
        else:  # MUGEN × NOUNS
            return max(2000, round((3000 + wind * 150) / 1000) * 1000)
    return 5000

con = sqlite3.connect(DB_PATH)
rows = con.execute(
    "SELECT id, brand, drop_num, name, price_jpy, weather_data, current_bid, bid_count "
    "FROM products WHERE active=1 ORDER BY id"
).fetchall()

print(f"Repricing {len(rows)} products...")
updated = 0

for (pid, brand, drop_num, name, old_price, weather_json, current_bid, bid_count) in rows:
    # NEVER reprice an MA piece that already has bids — that retroactively
    # changes the floor under which bidders entered. 2026-05-11: this guard was
    # added after the monthly→weekly cadence change accidentally lowered the
    # floor on an in-flight MA #11 auction (¥120k → ¥30k mid-auction).
    if brand == "ma" and (bid_count or 0) > 0:
        print(f"  #{pid} {brand}/{drop_num} '{name}': in-flight auction "
              f"(current_bid=¥{current_bid or 0:,}, {bid_count} bids), skip")
        continue

    if not weather_json:
        print(f"  #{pid} {brand}/{drop_num}: no weather data, skip")
        continue

    try:
        weather = json.loads(weather_json)
    except Exception:
        print(f"  #{pid} {brand}/{drop_num}: bad weather JSON, skip")
        continue

    new_price = recalc_price(brand, weather, drop_num, name)

    if new_price == old_price:
        print(f"  #{pid} {brand}/{drop_num} '{name}': ¥{old_price:,} → unchanged")
        continue

    # Update local DB
    con.execute("UPDATE products SET price_jpy=? WHERE id=?", (new_price, pid))
    print(f"  #{pid} {brand}/{drop_num} '{name}': ¥{old_price:,} → ¥{new_price:,}")

    # Push to live store
    try:
        r = requests.post(
            f"{STORE_URL}/api/admin/update-price?token={ADMIN_TOKEN}",
            json={"brand": brand, "drop_num": drop_num, "price_jpy": new_price},
            timeout=8
        )
        status = r.status_code
    except Exception as e:
        status = f"error: {e}"
    print(f"    → store: {status}")
    updated += 1

con.commit()
con.close()
print(f"\nDone. {updated} prices changed.")
