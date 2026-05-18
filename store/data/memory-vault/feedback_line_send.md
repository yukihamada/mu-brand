---
name: LINE sending reliable method
description: LINE Mac送信の確実な方法 - チャットリスト座標クリック→ウィンドウメニュー確認→cliclick入力欄→ペースト
type: feedback
---

LINE Mac版への確実な送信方法:
1. `keystroke "2" using {command down}` でトーク一覧表示
2. チャットリスト内の相手をcliclickで座標クリック (検索は不安定)
3. **ウィンドウメニューで名前を確認** (最も重要: `name of every menu item of menu "ウィンドウ"`)
4. 名前が一致したら `cliclick c:600,870` で入力欄クリック
5. `keystroke "v" using {command down}` + `key code 36` で送信

**Why:** Cmd+K検索は検索結果が不安定でKeepメモに行く。ウィンドウメニュー確認が唯一の安全策。
**How to apply:** Trio Dispatcher + コマンドバーのLINE送信で必ずウィンドウメニュー名を検証してから送信。