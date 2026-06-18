# wearmu data persistence

データ消失防止のルール（2026-05-23 確立）。

## どこに何があるか

| 種別 | パス | 揮発性 | 復元方法 |
|---|---|---|---|
| 本番 DB | `store/products.db` | 開発機ローカル / Fly volume `/data` | `data/backups/` から restore |
| DB バックアップ | `data/backups/products_YYYYMMDD_HHMMSS.db` | 48 世代 保持（毎時 cron） | ファイルコピー or sqlite3 .restore |
| パイプライン状態 | `data/pipeline_state/*.json` | repo 内、永続 | scripts 再実行で再生成可能 |
| 生成画像 (designs/mockup/lifestyle) | `store/static/<brand>/{d,m,lifestyle}/*` | repo 内 | Gemini 再生成 ¥6/枚 |
| 概念 lifestyle | `store/static/<brand>/lifestyle/concept_*.jpg` | repo 内 | 同上 |
| brand hero lifestyle | `store/static/<brand>/lifestyle/lifestyle_NN.png` | repo 内 | 同上 |
| brand metadata (style/scene) | `catalog_brands.config_json` | DB 内 | `scripts/populate_brand_configs.py` で復元 |

## /tmp は使わない

旧スクリプトは `/tmp/wearmu_*.json` を使っていたが **OS 再起動で消える**。
全部 `data/pipeline_state/` に移動済み。後方互換のため `/tmp` には symlink。

```
/tmp/wearmu_perfect10.json → data/pipeline_state/wearmu_perfect10.json
/tmp/wearmu_perfect_pipeline.json → data/pipeline_state/wearmu_perfect_pipeline.json
/tmp/wearmu_composites.json → ...
/tmp/wearmu_printful_variants.json → ...
/tmp/wearmu_url_status.json → ...
/tmp/wearmu_design_quality.json → ...
```

新スクリプト (`perfect_pipeline.py`, `gen_photo_dashboard.py`, `gen_all_perfect_dashboard.py`) は
`data/pipeline_state/` 直参照。

## バックアップ運用

### 手動

```bash
scripts/backup_db.sh
```

### 自動（cron 登録方法）

```bash
crontab -e
# add:
0 * * * * cd /Users/yuki/workspace/mu-brand && scripts/backup_db.sh
```

- 毎時 0 分に store/products.db を data/backups/ に snapshot
- 48 世代（≈2 日）保持、それ以降は自動 prune
- ログは `logs/backup_db.log`

## 復元手順

### DB を昔の状態に戻したい

```bash
# 1. 現在の DB を退避
mv store/products.db store/products.db.broken
# 2. 戻したい backup を選んでコピー
cp data/backups/products_20260523_104301.db store/products.db
```

### brand config_json を消した

```bash
python3 scripts/populate_brand_configs.py
# idempotent。既存キーは保持、不足キーだけ補充。
```

### 生成画像が消えた

```bash
# 該当 SKU の design / mockup / lifestyle を再生成
python3 scripts/perfect_pipeline.py --skus <SKU> <SKU> --workers 8
# 1 SKU あたり ~10 秒、¥12 程度
```

## Fly 本番への適用

ローカル DB の変更は **Fly volume には自動反映されない**。デプロイ時:

```bash
# 1. populate_brand_configs.py を Fly 上で実行
fly ssh console -a mu-store -C "cd /app && python3 scripts/populate_brand_configs.py"
# 2. もしくは Rust boot logic (store/src/catalog.rs) に migration step を追加
```

## NEVER ルール

- `store/products.db` を **コミット しない**（.gitignore 既存）
- `data/backups/*.db` も **コミット しない**（大きすぎる、頻繁に変わる）
- `data/pipeline_state/*.json` は **コミットする**（再生成コスト ¥1,000+）
- `store/static/*/m/*.jpg`, `*/d/*.png`, `*/lifestyle/*.jpg` も **コミットする**（Fly の Rust binary にバンドルされる）

## 関連

- `scripts/populate_brand_configs.py` — brand metadata migration
- `scripts/backup_db.sh` — DB hourly backup
- `scripts/perfect_pipeline.py` — design/mockup/lifestyle 並列生成（脱ハードコード版）
- `docs/mu_brand_expansion_2026_05_23.md` — 5 新ブランド戦略書
