# MA 売れ残り焚き火退役 — 2026-06-07

優貴さん指示「MUのMA、売れてないやつは焚き火で燃やして」。

## 対象

`products` (mu-store 本番 `/data/products.db`) の brand='ma' で
**sold=0 AND active=0 AND retired_at IS NULL** の **49点**(間 2026.05 系の過去ドロップ)。

除外:
- id 1056 (drop 4) / id 1111 (drop 6) — 売れた2点
- id 1901 (drop 76, 間 2026.06) — オークション進行中 (end 2026-06-08T00:02:54, bid 0)

before スナップショット: `before_snapshot.json` (49行・id 2〜1510)

## 実行 SQL (2026-06-07 ~04:10 UTC)

```sql
UPDATE products
   SET retired_at = datetime('now'),
       retire_reason = 'takibi-unsold-burn-2026-06-07'
 WHERE brand='ma' AND sold=0 AND active=0 AND retired_at IS NULL;
-- changes() = 49
```

対象は元から active=0 (ストアフロント非表示) のため顧客可視の変化なし。
ma_retirements (公開台帳 /ma/retired) はオーナー返却用スキーマのため使用していない。

## 検証

- `retire_reason='takibi-unsold-burn-2026-06-07'` → 49行
- brand='ma' AND retired_at IS NULL → 3行 (=売れた2点+live drop76) ✓
- `https://wearmu.com/ma` → 200 ✓

## ロールバック

```sql
UPDATE products
   SET retired_at = NULL, retire_reason = NULL
 WHERE retire_reason = 'takibi-unsold-burn-2026-06-07';
```
