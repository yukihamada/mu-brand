# 本番データ変更ログ (mu-store)

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
