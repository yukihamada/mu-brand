# 着画(lifestyle)ミスマッチ修正 — 2026-06-05

## 背景 / 問題
PDP の「着用イメージ」(着画) は `generate_lifestyle_photo` が **Gemini でデザインを
描き直す** 方式だった。参照画像を渡してもプリントが drift し、商品本体と着画の
デザインが食い違っていた。発端: `JIUFLOW-MU-NL-MUJIUFLOW-TEE-nl0a480c3a` で
本体=白ボックス入り「PROCESS OVER OUTCOME / 過程優先」なのに、着画は黒地に白文字
(白ボックス無し) になっていた。

本番実測: live 914件中 **231件** が Gemini 着画を保持(計 **258枚**)。全てが同じ
drift リスク。内訳: rashguard(AOP) 125 / tee 64 / hoodie 27 / crewneck 13 / other 2。

## 方針 (本人承認済み)
- **tee/hoodie/crewneck/tank** → 実 `design_file`(Printful が刷る実物そのもの)を
  **プリント無しの着用ブランク写真**に正規化座標で合成。布の輝度を乗算して
  「印刷された布」に見せる。プリントはピクセル一致 = drift 永久ゼロ。
- **rashguard(AOP 全面)** → 胸ボックス合成は不適。正確な Printful AOP
  `mockup_url_external` を着画に流用。

## 実装
- 着用ブランク素材: `store/static/lifestyle_base/{tee_1,tee_3,hoodie_1,hoodie_2,
  crewneck_1,crewneck_2}.png` (Gemini gemini-3-pro-image-preview・正面/無地黒/
  顔切れ・プリント無し)。生成器 `scripts/gen_lifestyle_base.py`。
- 合成コア `compose_lifestyle_png` (catalog.rs): design_file を四角のまま胸boxへ
  Lanczos リサイズ→ガーメント region のガウシアンぼかし輝度(p90正規化)で乗算
  (0.66–1.0)→alpha 0.95 で overlay→PNG。座標は base ごと `LbBase{cx,cy,wfrac}`。
- backfill: `GET /admin/catalog/fix_lifestyle?token=&dry_run=&limit=&sku=`
  - tee系=合成して `catalog/lifestyle/{sku}-fit.png` に上げ、`catalog_product_extras`
    の lifestyle 行 image_url を UPDATE。
  - rashguard=lifestyle 行 image_url を `mockup_url_external` に UPDATE。
  - 冪等(同一keyへ上書き・同SKUは同baseに固定)。

## 監査 / 復旧
- 変更前スナップショット: `before_snapshot.json` (258行: sku/label/image_url)。
- 復旧: スナップショットの image_url を書き戻せば原状回復可。

## 実行ログ
- (実行後追記: dry_run結果 / 本実行 composited/rash_reused/skipped/failed / 検証PDP)
