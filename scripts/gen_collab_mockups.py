#!/usr/bin/env python3
"""Generate placeholder product mockups for collab_products with NULL image_url.

Strategy:
  1. Query production DB for items missing image_url
  2. Generate a styled square mockup per item (1200x1200 JPG) using PIL
  3. Upload to R2 (wearmu-mockups bucket) at path /{partner}/{slug}.jpg
  4. PATCH /api/admin/collab_image for each item

Run:
  R2_ACCESS_KEY_ID=... R2_SECRET_ACCESS_KEY=... R2_ENDPOINT=... \
  R2_BUCKET=wearmu-mockups ADMIN_TOKEN=... python3 gen_collab_mockups.py
"""
import argparse, io, json, os, sqlite3, sys, hashlib, time, urllib.request
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
DB_PATH = "/tmp/prod_products.db"

FONT_BOLD = "/System/Library/Fonts/ヒラギノ角ゴシック W7.ttc"
FONT_REG = "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc"
FONT_MINCHO = "/System/Library/Fonts/ヒラギノ明朝 ProN.ttc"

# Partner branding
THEMES = {
    "sweep": {
        "bg":      (10, 10, 10),
        "panel":   (28, 28, 28),
        "accent":  (230, 196, 73),   # MU yellow
        "fg":      (245, 245, 240),
        "mute":    (140, 140, 140),
        "wordmark": "MU × SIIIEEP",
        "tagline": "Sapporo · BJJ",
    },
    "kokon": {
        "bg":      (10, 10, 10),
        "panel":   (24, 18, 12),
        "accent":  (166, 120, 67),   # gold
        "fg":      (245, 245, 240),
        "mute":    (170, 150, 120),
        "wordmark": "MU × KOKON",
        "tagline": "焼肉 古今 · 西麻布",
    },
    "jiuflow": {
        "bg":      (10, 10, 10),
        "panel":   (16, 22, 36),
        "accent":  (90, 158, 200),   # blue
        "fg":      (245, 245, 240),
        "mute":    (140, 160, 180),
        "wordmark": "JiuFlow",
        "tagline": "BJJ Media · 柔",
    },
}

SIZE = 1200


def text_width(font, s):
    bbox = font.getbbox(s)
    return bbox[2] - bbox[0]


def wrap_text(text, font, max_width):
    """Wrap text by character (Japanese-friendly), return list of lines."""
    if text_width(font, text) <= max_width:
        return [text]
    lines = []
    current = ""
    for ch in text:
        candidate = current + ch
        if text_width(font, candidate) > max_width:
            if current:
                lines.append(current)
            current = ch
        else:
            current = candidate
    if current:
        lines.append(current)
    return lines


def generate_mockup(partner: str, slug: str, category: str, name: str) -> bytes:
    """Produce a stylized product card PNG and return bytes."""
    t = THEMES[partner]
    img = Image.new("RGB", (SIZE, SIZE), t["bg"])
    d = ImageDraw.Draw(img)

    # Background gradient (subtle)
    for y in range(SIZE):
        ratio = y / SIZE
        r = int(t["bg"][0] * (1 - ratio) + t["panel"][0] * ratio)
        g = int(t["bg"][1] * (1 - ratio) + t["panel"][1] * ratio)
        b = int(t["bg"][2] * (1 - ratio) + t["panel"][2] * ratio)
        d.line([(0, y), (SIZE, y)], fill=(r, g, b))

    # Border accent (thin frame)
    margin = 80
    d.rectangle([(margin, margin), (SIZE - margin, SIZE - margin)],
                outline=t["accent"], width=2)

    # Header strip: partner wordmark (top center)
    f_wordmark = ImageFont.truetype(FONT_BOLD, 36)
    wm_w = text_width(f_wordmark, t["wordmark"])
    d.text(((SIZE - wm_w) // 2, margin + 32), t["wordmark"],
           font=f_wordmark, fill=t["accent"])

    # Tagline below wordmark
    f_tag = ImageFont.truetype(FONT_REG, 18)
    tag_w = text_width(f_tag, t["tagline"])
    d.text(((SIZE - tag_w) // 2, margin + 80), t["tagline"],
           font=f_tag, fill=t["mute"])

    # Category (mid-upper)
    f_cat = ImageFont.truetype(FONT_REG, 28)
    cat_lines = wrap_text(category, f_cat, SIZE - margin * 2 - 80)
    cat_y = margin + 200
    for line in cat_lines:
        lw = text_width(f_cat, line)
        d.text(((SIZE - lw) // 2, cat_y), line, font=f_cat, fill=t["fg"])
        cat_y += 42

    # Big product name (center)
    # Split name into two parts: "MU × XXX " prefix + body. Keep just the body.
    body = name
    for prefix in ("MU × SIIIEEP ", "MU × KOKON ", "MU × kokon.tokyo ", "JiuFlow ", "MU × JiuFlow "):
        if body.startswith(prefix):
            body = body[len(prefix):]
            break

    # Auto-shrink font to fit
    for size_px in (72, 64, 56, 48, 42, 38, 34, 30):
        f_name = ImageFont.truetype(FONT_BOLD, size_px)
        name_lines = wrap_text(body, f_name, SIZE - margin * 2 - 60)
        if len(name_lines) <= 3:
            break
    line_h = size_px + 14
    total_h = line_h * len(name_lines)
    start_y = (SIZE - total_h) // 2 + 40
    for i, line in enumerate(name_lines):
        lw = text_width(f_name, line)
        d.text(((SIZE - lw) // 2, start_y + i * line_h),
               line, font=f_name, fill=t["fg"])

    # Bottom: slug as serial
    f_slug = ImageFont.truetype(FONT_REG, 18)
    slug_w = text_width(f_slug, slug)
    d.text(((SIZE - slug_w) // 2, SIZE - margin - 50), slug,
           font=f_slug, fill=t["mute"])

    # Bottom: timestamp/sentinel
    sentinel = f"COLLAB · {slug.split('-')[0].upper()}"
    f_sent = ImageFont.truetype(FONT_REG, 14)
    sent_w = text_width(f_sent, sentinel)
    d.text(((SIZE - sent_w) // 2, SIZE - margin - 24), sentinel,
           font=f_sent, fill=t["accent"])

    # Corner accent dot
    d.ellipse([(SIZE - margin - 12, margin), (SIZE - margin, margin + 12)],
              fill=t["accent"])

    out = io.BytesIO()
    img.save(out, format="JPEG", quality=88, optimize=True)
    return out.getvalue()


def upload_r2(key_id, secret, endpoint, bucket, key, body_bytes, content_type="image/jpeg"):
    """Upload to R2 using boto3 (S3-compatible)."""
    import boto3
    s3 = boto3.client(
        "s3",
        endpoint_url=endpoint,
        aws_access_key_id=key_id,
        aws_secret_access_key=secret,
        region_name="auto",
    )
    s3.put_object(
        Bucket=bucket,
        Key=key,
        Body=body_bytes,
        ContentType=content_type,
        CacheControl="public,max-age=86400",
    )


def patch_image_url(slug, image_url, admin_token, base="https://wearmu.com"):
    req = urllib.request.Request(
        f"{base}/api/admin/collab_image?token={admin_token}",
        data=json.dumps({"slug": slug, "image_url": image_url}).encode(),
        method="PATCH",
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--partner", choices=["sweep", "kokon", "jiuflow"], default=None)
    ap.add_argument("--limit", type=int, default=999)
    args = ap.parse_args()

    required = ["R2_ACCESS_KEY_ID", "R2_SECRET_ACCESS_KEY", "R2_ENDPOINT", "R2_BUCKET", "ADMIN_TOKEN"]
    missing = [v for v in required if not os.environ.get(v)]
    if missing:
        print(f"ERR: missing env vars: {missing}", file=sys.stderr); sys.exit(1)

    public_base = os.environ.get("R2_PUBLIC_BASE", "https://mockups.wearmu.com")
    bucket = os.environ["R2_BUCKET"]
    print(f"R2 bucket: {bucket}, public base: {public_base}")

    # Query production DB for items missing image_url
    db = sqlite3.connect(DB_PATH)
    where = "active=1 AND (image_url IS NULL OR image_url='')"
    if args.partner:
        where += f" AND partner='{args.partner}'"
    rows = list(db.execute(
        f"SELECT slug, partner, category, name FROM collab_products WHERE {where} ORDER BY partner, id LIMIT {args.limit}"
    ))
    print(f"Found {len(rows)} items needing mockups")
    if not rows:
        return

    success, failed = 0, 0
    for i, (slug, partner, category, name) in enumerate(rows, 1):
        if partner not in THEMES:
            print(f"  ⏩ skip unknown partner: {partner}/{slug}")
            continue
        try:
            print(f"[{i:3}/{len(rows)}] {slug:38} ", end="", flush=True)
            png_bytes = generate_mockup(partner, slug, category, name)
            key = f"{partner}/{slug}.jpg"
            url = f"{public_base}/{key}"

            if args.dry_run:
                print(f"DRY → {len(png_bytes):>7} bytes  {url}")
                continue

            # Upload
            upload_r2(
                os.environ["R2_ACCESS_KEY_ID"],
                os.environ["R2_SECRET_ACCESS_KEY"],
                os.environ["R2_ENDPOINT"],
                bucket, key, png_bytes,
            )

            # PATCH DB
            res = patch_image_url(slug, url, os.environ["ADMIN_TOKEN"])
            ok = res.get("updated", 0) > 0
            print(f"{'✓' if ok else '✗'} uploaded + db {'updated' if ok else 'NOT updated'}  {url}")
            if ok: success += 1
            else: failed += 1
        except Exception as e:
            print(f"ERR {e}")
            failed += 1

    print()
    print(f"DONE — success={success} failed={failed}")


if __name__ == "__main__":
    main()
