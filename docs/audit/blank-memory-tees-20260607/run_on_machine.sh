#!/bin/bash
# audit: BLANK_ Atami/Minakami memory tees (2026-06-07, submitted_by=claude-code yuki session)
# 1) mockup_url_external update for 2 SKUs  2) Stripe promo codes (MA 50% / MUGEN 1%)
set -e
command -v sqlite3 >/dev/null 2>&1 || { apt-get update -qq >/dev/null; apt-get install -y -qq sqlite3 curl >/dev/null; }
sqlite3 /data/products.db "UPDATE catalog_products SET mockup_url_external='https://raw.githubusercontent.com/yukihamada/mu-mockups/main/blank/blank-atami-mock.jpg' WHERE sku='BLANKCAMP-AGENT-TEE-1eb22263';"
sqlite3 /data/products.db "UPDATE catalog_products SET mockup_url_external='https://raw.githubusercontent.com/yukihamada/mu-mockups/main/blank/blank-minakami-mock.jpg' WHERE sku='BLANKCAMP-AGENT-TEE-85e287e5';"
echo "--- verify mockup ---"
sqlite3 /data/products.db "SELECT sku, substr(mockup_url_external,1,90) FROM catalog_products WHERE sku IN ('BLANKCAMP-AGENT-TEE-1eb22263','BLANKCAMP-AGENT-TEE-85e287e5');"
echo "--- stripe coupons ---"
curl -s https://api.stripe.com/v1/coupons -u "$STRIPE_SECRET_KEY:" -d id=BLANK_MA_50 -d percent_off=50 -d duration=once -d "name=BLANK_ memory tee MA 50%" -o /tmp/c1.json; grep -o '"id": "[^"]*"' /tmp/c1.json | head -1; grep -o '"message": "[^"]*"' /tmp/c1.json | head -1
curl -s https://api.stripe.com/v1/coupons -u "$STRIPE_SECRET_KEY:" -d id=BLANK_MUGEN_1 -d percent_off=1 -d duration=once -d "name=BLANK_ memory tee MUGEN 1%" -o /tmp/c2.json; grep -o '"id": "[^"]*"' /tmp/c2.json | head -1; grep -o '"message": "[^"]*"' /tmp/c2.json | head -1
echo "--- promotion codes ---"
curl -s https://api.stripe.com/v1/promotion_codes -u "$STRIPE_SECRET_KEY:" -d coupon=BLANK_MA_50 -d code=BLANKMA50499F -d max_redemptions=20 -d expires_at=1783407418 -o /tmp/p1.json; grep -o '"code": "[^"]*"' /tmp/p1.json | head -1; grep -o '"message": "[^"]*"' /tmp/p1.json | head -1
curl -s https://api.stripe.com/v1/promotion_codes -u "$STRIPE_SECRET_KEY:" -d coupon=BLANK_MUGEN_1 -d code=BLANKMUGEN1BB66 -d max_redemptions=200 -d expires_at=1783407418 -o /tmp/p2.json; grep -o '"code": "[^"]*"' /tmp/p2.json | head -1; grep -o '"message": "[^"]*"' /tmp/p2.json | head -1
