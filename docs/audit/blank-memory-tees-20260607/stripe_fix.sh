#!/bin/bash
set -e
curl -s https://api.stripe.com/v1/coupons -u "$STRIPE_SECRET_KEY:" -d id=BLANK_MA_50 -d percent_off=50 -d duration=once --data-urlencode "name=BLANK_ memory tee MA 50pct" -o /tmp/c1.json; grep -o '"id": "[^"]*"' /tmp/c1.json | head -1; grep -o '"message": "[^"]*"' /tmp/c1.json | head -1
curl -s https://api.stripe.com/v1/coupons -u "$STRIPE_SECRET_KEY:" -d id=BLANK_MUGEN_1 -d percent_off=1 -d duration=once --data-urlencode "name=BLANK_ memory tee MUGEN 1pct" -o /tmp/c2.json; grep -o '"id": "[^"]*"' /tmp/c2.json | head -1; grep -o '"message": "[^"]*"' /tmp/c2.json | head -1
curl -s https://api.stripe.com/v1/promotion_codes -u "$STRIPE_SECRET_KEY:" -d coupon=BLANK_MA_50 -d code=BLANKMA50499F -d max_redemptions=20 -d expires_at=1783407459 -o /tmp/p1.json; grep -o '"code": "[^"]*"' /tmp/p1.json | head -1; grep -o '"message": "[^"]*"' /tmp/p1.json | head -1
curl -s https://api.stripe.com/v1/promotion_codes -u "$STRIPE_SECRET_KEY:" -d coupon=BLANK_MUGEN_1 -d code=BLANKMUGEN1BB66 -d max_redemptions=200 -d expires_at=1783407459 -o /tmp/p2.json; grep -o '"code": "[^"]*"' /tmp/p2.json | head -1; grep -o '"message": "[^"]*"' /tmp/p2.json | head -1
