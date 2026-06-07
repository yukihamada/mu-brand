# 監査: catalog_products.description_en 一括翻訳 (SEO項目5)

- **日付**: 2026-06-07 / **実施**: Claude Code (優貴さん指示「EN翻訳バッチお願い」)
- **目的**: ?lang=en の PDP 本文を英語化 (SEO 10項目評価の項目5を8→10へ)

## 変更内容

1. **スキーマ**: `ALTER TABLE catalog_products ADD COLUMN description_en TEXT`
   (boot migration・idempotent・既存行は NULL = 挙動不変)
2. **書込経路**: `GET /admin/catalog/translate_en?token=ADMIN_TOKEN&limit=N` のみ
   - 対象: `status='live' AND description_en IS NULL/'' AND description_ja<>''`
   - 除外: 封印ドロップ (`meta_json LIKE '%unlock_iso%'` = description_ja が暗号文)
   - 翻訳: `gemini::call_gemini_text` (ブランド名/コード/価格/URL は原文維持を指示)
3. **読出**: PDP (`shop_pdp`) が `?lang=en` かつ description_en 非空のときのみ本文を差し替え。
   日本語表示・既存挙動への影響なし (additive)

## ロールバック

```sql
-- 全消し (表示は自動で description_ja にフォールバック)
UPDATE catalog_products SET description_en = NULL;
-- カラムごと消す場合 (SQLite 3.35+)
ALTER TABLE catalog_products DROP COLUMN description_en;
```

## 実行ログ

実行コマンドと各バッチの translated/errors/remaining はこの下に追記する。
