#!/bin/bash
# audit 2026-06-07: 本人訂正によりMUGEN割引を1%→10%へ。旧BLANK_MUGEN_1削除+BLANK_MUGEN_10新規
set -e
curl -s -X DELETE https://api.stripe.com/v1/coupons/BLANK_MUGEN_1 -u "$STRIPE_SECRET_KEY:" -o /tmp/d1.json; grep -o '"deleted": [a-z]*' /tmp/d1.json | head -1; grep -o '"message": "[^"]*"' /tmp/d1.json | head -1
curl -s https://api.stripe.com/v1/coupons -u "$STRIPE_SECRET_KEY:" -d id=BLANK_MUGEN_10 -d percent_off=10 -d duration=once --data-urlencode "name=BLANK_ memory tee MUGEN 10pct" -o /tmp/c3.json; grep -o '"id": "[^"]*"' /tmp/c3.json | head -1; grep -o '"message": "[^"]*"' /tmp/c3.json | head -1
curl -s https://api.stripe.com/v1/promotion_codes -u "$STRIPE_SECRET_KEY:" -d coupon=BLANK_MUGEN_10 -d code=BLANKMUGEN10FBFB -d max_redemptions=200 -d expires_at=1783411223 -o /tmp/p3.json; grep -o '"code": "[^"]*"' /tmp/p3.json | head -1; grep -o '"message": "[^"]*"' /tmp/p3.json | head -1
