#!/usr/bin/env python3
"""
MU Brand — Watermark Verifier
Usage:
  python3 verify.py <image_file_or_url>
  python3 verify.py designs/mugen_0011_abc12345.png
  python3 verify.py https://i.imgur.com/xxx.jpeg
"""
import sys, io, struct, requests
from pathlib import Path
from PIL import Image
import numpy as np

def load_image(src: str) -> bytes:
    if src.startswith("http://") or src.startswith("https://"):
        r = requests.get(src, headers={"User-Agent": "MU-Verify/1.0"}, timeout=15)
        r.raise_for_status()
        return r.content
    return Path(src).read_bytes()

def decode_watermark(image_bytes: bytes) -> dict | None:
    try:
        from imwatermark import WatermarkDecoder
    except ImportError:
        print("pip3 install invisible-watermark")
        return None
    img = Image.open(io.BytesIO(image_bytes)).convert("RGB")
    dec = WatermarkDecoder("bytes", 64)
    payload = dec.decode(np.array(img), "dwtDctSvd")
    if payload[:2] != b"MU":
        return None
    brand_map = {b"G": "mugen", b"O": "muon", b"A": "ma", b"N": "nouns"}
    brand    = brand_map.get(payload[2:3], "unknown")
    drop_num = struct.unpack(">H", payload[3:5])[0]
    hash_pfx = payload[5:8].hex()
    return {"brand": brand, "drop_num": drop_num, "hash_prefix": hash_pfx}

def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)

    src = sys.argv[1]
    print(f"Loading: {src}")
    try:
        data = load_image(src)
    except Exception as e:
        print(f"Error loading image: {e}")
        sys.exit(1)

    print(f"Image size: {len(data)//1024}KB")
    result = decode_watermark(data)

    if result is None:
        print("\n✗  No MU watermark detected.")
        print("   (JPEG compression may have degraded it — use the PNG from designs/)")
        sys.exit(1)

    brand    = result["brand"].upper()
    drop_num = result["drop_num"]
    hash_pfx = result["hash_prefix"]

    print(f"\n✓  MU Watermark verified")
    print(f"   Brand      : {brand}")
    print(f"   Drop       : #{drop_num:04d}")
    print(f"   Hash prefix: {hash_pfx}")

    # Cross-check against local DB if available
    db_path = Path(__file__).parent / "products.db"
    if db_path.exists():
        import sqlite3
        conn = sqlite3.connect(db_path)
        row = conn.execute(
            "SELECT name, price_jpy, created_at FROM products "
            "WHERE brand=? AND drop_num=? LIMIT 1",
            (result["brand"], drop_num)
        ).fetchone()
        conn.close()
        if row:
            name, price, created = row
            print(f"\n   DB match   : {name}")
            print(f"   Price      : ¥{price:,}")
            print(f"   Created    : {created}")
            if hash_pfx in (row[0] or ""):
                print(f"   Hash       : ✓ matches prompt hash")
        else:
            print(f"\n   DB match   : not found locally (may exist on wearmu.com)")

if __name__ == "__main__":
    main()
