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

## 実行ログ (2026-06-05 完了)
- デプロイ: origin/main 90d8e44 → GHA 成功 / health 200。
- dry_run: candidates=231, composited=105, rash_reused=125, failed=0。
- 本実行(全件): composited=104, rash_reused=125, failed=1, skipped=1。
- エッジケース2件を手動で mockup 流用に統一(AOP全面で胸合成不適):
  - `JF-AOP-HOOD-01` (design_file が相対パスで合成不可) → `/static/jiuflow/m/perfect_JF-AOP-HOOD-01.jpg`
  - `KK-AOP-APRON-01` (apron=baseなし) → `/static/kokon/m/perfect_KK-AOP-APRON-01.jpg`
- 最終検証: **live の lifestyle 行で旧drift(-vN.png)残=0**。
  composited(-fit.png)=104行 / mockup流用=127行。非liveの27行は顧客非表示で対象外。
- PDP目視: 指摘 `JIUFLOW-MU-NL-MUJIUFLOW-TEE-nl0a480c3a` で着画=メインのプリント一致。
  hoodie合成 / rashguard AOPモックも目視OK。
- 監査: `before_snapshot.json`(258行・変更前) / `after_snapshot.json`(239行・変更後 live)。

## 既知の限界 / 今後
- AOP(apron/AOP-hoodie 等)は胸合成不可 → mockup 流用で対応。新規AOPは要同様対応。
- `generate_lifestyle_photo`(autopilot生成経路)は現状 Gemini のまま(MU_AUTOPILOT=unset で停止中)。
  autopilot 再開時に合成経路へ差し替え推奨(本コミットの compose_lifestyle_png を再利用可)。
