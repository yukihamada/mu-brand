---
name: feedback_jiuflow_english
description: JiuFlow app must support English - all UI strings via LanguageManager.t()
type: feedback
---

JiuFlowアプリは英語対応必須。UIの全文字列をLanguageManager.t()でラップする。

**Why:** ユーザーが「アプリは英語にも対応してね」と指示。BJJは国際コミュニティなので多言語が重要。

**How to apply:** 新しいSwiftUIビューを作る際、ハードコード日本語ではなく必ず `lang.t("日本語", en: "English")` を使う。APIレスポンスは `name_ja`/`name_en` デュアルフィールドパターンに従う。