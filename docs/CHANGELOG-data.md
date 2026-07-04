# 本番データ変更ログ (mu-store)

## 2026-07-04
- ポテ×焼肉古今「初来店」グッズ 7 SKU の `brand` を `yuki`→`elepote` に UPDATE（手動SQL・
  fly-machine-exec sqlite3 経由）。対象: `YUKI-AGENT-MUG-c0088d6c` / `YUKI-AGENT-STICKER-152d7496` /
  `YUKI-AGENT-TEE-188086c2` / `YUKI-AGENT-COASTER-8f4a0b9a` / `YUKI-AGENT-TOTE-5da2cc28` /
  `YUKI-AGENT-PHONE-CASE-cb77cae3` / `YUKI-AGENT-PILLOW-40d4df8c`（全て status=live のまま・
  printful_product_id/variant_id 等の行内フルフィル設定は不変=印刷影響なし）。
  理由: 本人指示「ELEPOTE の棚に載せて」— /shop?brand=elepote が 15→22 件になったのを実打検証済み。
  注意: brand 移動により agent API (mu_list_mine/mu_retire_product, key=yuki@hamada.tokyo) の
  管理対象から外れる（elepote は pre-seeded brand・owner_email なし）。差し戻しは同 SQL で brand='yuki'。
  実施者: Claude (本人GO済タスク)。

## 2026-06-13
- song 商品『合宿の音 — 弟子屈アンビエンス』を catalog_products に作成 (正規 mu MCP
  `mu_create_product` 経由・agent=yuki@hamada.tokyo)。`AICAMPIKU-AGENT-SONG-6eea639b`、
  store=`ai-camp-iku`、¥500、route=digital。audio_url/design_url は mu-mockups の
  `ai-camp/ambient.mp3`(4分・ffmpeg自前合成・loudnorm I=-16/TP=-1.5・著作権クリア)と
  `ai-camp/sound-cover.png`。**作成直後に `mu_retire_product` で status=`retired`(is_active=0)
  に戻し非公開化**。理由: 樋口さん私的招待文脈のため公開は人間ゲート。作成→retire の間に
  PDP 試聴(`/api/song/preview/:sku` 200・audio/mpeg 冒頭のみ)と PDP 200 を実打検証済み。
  実施者: Claude (本人GO済タスク)。

## 2026-06-12
- house kind 商品 3 件を catalog_products に INSERT (正規 agent API 経由・全件 status=`review`
  着地、即公開なし)。store=`bim-house`、熊牛SOLUNA製品ラインのミラー:
  `BIMHOUSE-AGENT-HOUSE-6fb1bd43` (S 64㎡) / `BIMHOUSE-AGENT-HOUSE-18c4cd7b` (M 110㎡) /
  `BIMHOUSE-AGENT-HOUSE-a910bc2f` (L 156㎡)。価格は設計相談デポジット ¥50,000 (法規ガード準拠)、
  建物概算は bim.house 実ページから取得し説明に記載。詳細 = docs/CHANGELOG_house_kind_shop_2026-06-12.md。
  実施者: Claude (本人GO済タスク・agent=yuki@hamada.tokyo)。
- `MCP-AGENT-MUG-ff12c5d3` を `status='retired', is_active=0` に変更（手動SQL・fly ssh 経由）。
  理由: 黒生地用デザイン(白文字)を白マグに横展開した初期版の欠陥品 — ほぼ無地で印刷される。
  恒久対策: 同日の明暗ゲート(kind_ok_for_luma)で同種の組合せは作成不能に。実施者: Claude (本人指示「全部やって」)。
