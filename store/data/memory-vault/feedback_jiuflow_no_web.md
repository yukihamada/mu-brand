---
name: feedback_jiuflow_no_web
description: JiuFlow iOS app must not open Safari/WebView - everything should be native in-app
type: feedback
---

JiuFlow iOSアプリからWebに飛ばさない。全てアプリ内ネイティブで完結させる。

**Why:** ユーザーが明確に「アプリからはウェブに飛ばないように全部アプリ完結してね」と指示。

**How to apply:** 新機能追加時にLink(destination:)やSafariViewControllerを使わず、SwiftUIネイティブビューで表示する。外部登録URL・プライバシーポリシー・利用規約は例外OK。