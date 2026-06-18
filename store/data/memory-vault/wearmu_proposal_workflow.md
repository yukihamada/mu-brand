---
name: wearmu-proposal-workflow
description: "How to generate a kichinan-class /proposals/<slug> LP for a partner in one shot — script path, template style, Printful photo fetch, route registration steps."
metadata: 
  node_type: memory
  type: reference
  originSessionId: 705e7c8b-754e-4a04-8d13-32fe62d8e27a
---

# /proposals/<slug> 生成 ワークフロー (wearmu.com)

mu-brand に新規パートナー pitch deck / proposal LP を追加するときの一発手順。 ユーザは「今後これが一発で出るように」 と明示要望済み (2026-05-15)。

## 生成スクリプト

`scripts/gen_partner_proposal.py` が ALL を一手にやる:

```bash
cd /Users/yuki/workspace/mu-brand
set -a; source ~/.env; set +a    # PRINTFUL_API_KEY を export
python3 scripts/gen_partner_proposal.py <slug> --pf-fallback
```

- `<slug>` は `collab_products.partner` 列の値 (`sweep`/`[partner]`/`jiuflow` …) で、 `/api/v1/collab/<slug>` から SKU 一覧を引いて LP に焼き込む。
- `--pf-fallback` を付けると `image_url=NULL` の SKU について `printful_variant_id` で Printful catalog 写真を取得して `store/static/proposals/<slug>-pf-<sku_slug>.jpg` に保存。
- 出力: `store/static/proposals/<slug>.html` (kichinan/asoview と同じ template style)

新パートナーを追加するには:
1. `scripts/gen_partner_proposal.py` の冒頭 `META` dict に 1 entry (display_name / tagline / h1 / accent_hex / lede / hero_kv / why_md / use_cases)
2. スクリプト実行
3. `store/src/main.rs` で `proposal_<slug>_pitch()` ハンドラ + `.route("/proposals/<slug>", get(...))` を追加
4. commit + push (GitHub Actions → Fly deploy)

## 既存テンプレートの 2 種類

`/proposals/<slug>` には **2 系統** のパターンが共存していて、 ユースケースで使い分ける:

| 系統 | 用途 | 例 | 承認ゲート |
|---|---|---|---|
| **3rd-party 新規 collab** | まだ売ってない、 商標承諾も未取得な提案 | kichinan / asoview / elsoul / ele | あり (`<slug>_approval` テーブル + `/api/proposals/<slug>/{status,approve,revoke}`) |
| **既存 collab の pitch deck** | 既に売っている collab の拡張提案 | sweep / [partner] | なし (Live 販売中 banner のみ) |

ジェネレータスクリプトは pitch-deck 系統。 新規 collab を立ち上げるなら kichinan/asoview/elsoul/ele のコード (main.rs の `proposal_<slug>_*` 関数群) をコピーする方が早い。

## Printful 写真の取得パターン

各 SKU の variant 写真 (無地状態) は:
```
GET https://api.printful.com/products/variant/<variant_id>
Authorization: Bearer <PRINTFUL_API_KEY>
User-Agent: wearmu/1.0    # ← 無いと 403
```
レスポンスの `result.variant.image` が CDN URL。 そのまま `urllib.request` (User-Agent 付き) でダウンロードして `static/proposals/` に置く。 LP ではローカル URL (`/proposals/<file>.jpg`) を参照 (Printful CDN への runtime 依存を切る)。

## kichinan/asoview 系の Printful 変換 (kind → variant_id) 一覧

`store/src/main.rs::create_printful_order` の match block 参照。 主要マッピング (Black/M baseline):

| Kind | Variant | 製品 |
|---|---|---|
| tee | 4017 | Bella+Canvas 3001 Black M |
| polo | 9899 | Port Authority K500 Black M (※ kichinan match には未登録、 直接 brand match 必要) |
| cap | 4792 | Yupoong 6089M Snapback Black |
| tote | 4533 | All-Over Tote Black |
| hoodie | 9228 | Bella+Canvas 3719 Black M |
| longsleeve | 10095 | Bella+Canvas 3501 Black M |
| mug | 1320 | White Glossy Mug 11oz |
| beanie | 8936 | Yupoong 1501KC Cuffed Beanie Black |
| sticker | 10164 | Kiss-Cut Stickers 4″×4″ |
| kids_tee | 9431 | Bella+Canvas 3001Y Youth Black M |
| pin | 20250 | Acrylic Ornaments Circle |
| pillow | 4532 | All-Over Print Basic Pillow 18″×18″ |

`asoview_*` / `elsoul_*` / `ele_*` brand prefix は `create_printful_order` で `kichinan_*` に正規化されるので、 同じ catalog 表を再利用できる。

## 既知の落とし穴

- `collab_products` API レスポンスに `printful_variant_id` が含まれていないので、 `--pf-fallback` で polo 等の追加 SKU 写真を取得したいときは API を拡張するか、 main.rs の seed コード側で `image_url` を埋めるのが手っ取り早い。
- `kichinan_polo_sample` の variant は match block に未登録 → 注文すると default tee に fall-through する (要修正)。