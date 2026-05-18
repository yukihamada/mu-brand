---
name: JiuFlow 正規ドメインは jiuflow.com
description: 2026-05-09 にユーザー確定。.art は 308 リダイレクト、.com を canonical とする
type: feedback
originSessionId: 6c99622e-1170-487d-83ce-c0621993cf8e
---
JiuFlow の正規ドメインは **`jiuflow.com`**。

**Why:**
- 2026-04-25 頃から jiuflow.art の SEO 流入が急減（4/29 ピーク 1,212/d → 5/5 以降 6-14/d）
- 5/4 から jiuflow.com が急成長（5/8 単日 1,885/d 過去最高）
- Google referrer に .com 出現 = SEO クロール .com に切替中
- ユーザー判断（2026-05-09）: 正式に .com を canonical に確定

**How to apply:**
- ソース内 hardcode は全て `jiuflow.com` に統一（135 箇所、2026-05-09 一括置換）
- `jiuflow.art` → `jiuflow.com` への 308 リダイレクトは middleware.rs で維持
- v2.jiuflow.art は別扱い（独立サブドメイン、UV 613 と独立トラフィックあり）
- iOS App の baseURL も `https://jiuflow.com`
- canonical タグは `<link rel="canonical" href="https://jiuflow.com/...">`
- 新しいリンク・記事・SNS は全て .com で統一