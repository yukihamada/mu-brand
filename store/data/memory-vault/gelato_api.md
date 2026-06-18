---
name: gelato-api
description: "Gelato POD API for MU brand — JP-domestic printing confirmed for heavyweight apparel, costs vs SUZURI, key endpoints"
metadata: 
  node_type: memory
  type: project
  originSessionId: b0c0097c-591c-4e83-bbf4-df5d08af70d1
---

Gelato Order Flow API integrated for MU brand (wearmu.com) on 2026-05-16.
Key in `/Users/yuki/.env` as `GELATO_API_KEY` (UUID:UUID format, single header value).

**Why this matters:**
MU's §24-v2 dual-channel was: JP→SUZURI ¥4,900 (¥1,400 margin) / 海外→Printful EU ¥7,800.
Gelato adds a **direct JP-printed channel** at lower wholesale than SUZURI, with full checkout control (no redirect friction to suzuri.jp).

**Endpoints (verified 2026-05-16):**
- Auth: `X-API-KEY: $GELATO_API_KEY` header
- Catalog: `GET https://product.gelatoapis.com/v3/catalogs`
- Product search: `POST https://product.gelatoapis.com/v3/catalogs/{catalogUid}/products:search` with `{"attributeFilters":{...},"limit":N}`
- Price probe: `POST https://order.gelatoapis.com/v4/orders:quote` with full recipient + product
- Orders: `GET/POST https://order.gelatoapis.com/v4/orders`

**JP-printed wholesale (probed 2026-05-16, USD):**
| SKU | Print | Cost USD | Delivery |
|---|---|---|---|
| Gildan H000 Hammer 6.0oz heavy tee black M | 🇯🇵 JP | $13.82 | 5-10d |
| Gildan 5300 Heavy Cotton 5.3oz tee black M | 🇯🇵 JP | $15.07 | 5-10d |
| Gildan 64000 Softstyle 4.5oz tee black M | 🇺🇸 US | $9.24 + $11.31 ship | 10-19d |
| Gildan 18500 Heavy Hoodie black M | 🇯🇵 JP | $19.96 | 5-10d |
| Tote bag black DTG | 🇸🇬 SG | $11.96 | 5-6d |
| Mug 11oz ceramic black | 🇸🇬 SG | $8.11 | 5-6d |
| Canvas 11x14 slim horiz | 🇦🇺 AU | $17.94 | 11-17d |

At ¥160/USD: heavy tee = ¥2,200 wholesale vs SUZURI's ¥3,500. Retail at ¥3,900-4,500 leaves ¥1,700-2,300 margin (better than SUZURI's ¥1,400).

**Catalogs of interest for MU expansion:**
apparel, hoodies, sweatshirts, tote-bags, mugs, posters, canvas, fine-art, framed-posters, hanging-posters, phone-cases, stickers, wood-prints, wallpaper

**How to apply:**
- Heavyweight apparel (Gildan H000/5300/18500) prints in JP — use as primary JP fulfillment, replacing SUZURI mirror for "direct sale on wearmu.com"
- Light apparel (Gildan 64000 softstyle) falls back to US — avoid for JP-target SKUs unless customer accepts 10-19d
- Accessories (tote/mug) ship from Singapore in 5-6d — fine for JP
- Canvas/fine-art from Australia/EU — slow but acceptable for one-off MU Gallery line
- See [[wearmu_suzuri_mirror]] for the prior §24-v2 dual-channel; Gelato lets us add §24-v3 direct-JP channel