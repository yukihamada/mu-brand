---
name: feedback-jiuflow-hero-cta
description: JiuFlow ホームの hero プライマリCTAは必ずサブスク誘導 (/join) にする。無料機能を派手CTAにすると広告が0転換する
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 4b356be2-d8a9-4e9a-a2f8-7c0ca8ebf82b
---

JiuFlow `templates/pages/home.html` の hero CTA は **必ず /join (サブスク)** をプライマリ(派手なグラデ+pulse)にし、`/tournaments` 等の無料機能はゴーストボタンの secondary に置く。

**Why:** 2026-04後半に Google Ads ¥42K/3日 → 0 conversion 事件。原因は hero primary CTA が `/tournaments`(無料DB) になっており、有料広告流入が無料機能で満足して帰っていた。コミット `a75af35` で CTA href のみ入れ替えて修正。

**How to apply:**
- ホームLPデザイン変更時、必ず subscribe を primary に維持
- A/Bテストで一時的に無料CTAを primary にする場合は、必ず転換率測定 (enabler-analytics) を仕込んでから
- 多言語LP (`/en/`, `/pt/`) も同じ原則