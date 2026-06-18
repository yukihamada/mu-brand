---
name: Trio Project
description: macOS AI message triage app - architecture, LINE genius method, cloud deployment status
type: project
---

## Trio — macOS AIメッセージアシスタント

### リポジトリ
- GitHub: yukihamada/trio
- Mac app: ~/workspace/trio/Trio.app (Developer ID署名+Apple公証済)
- Server: trio-cloud.fly.dev (Rust+axum+SQLite)
- PKG: Trio-0.1.0.pkg (Apple Notarized)

### LINE自動送信 (天才的方法)
LINE Mac版はAppleScript辞書なし+AXテキスト非公開。以下の方法で安定送信:
1. `keystroke "2" using {command down}` でトーク一覧
2. 検索バー(cliclick c:120,75)で相手名検索
3. ウィンドウメニューで正確なチャット選択確認
4. `cliclick c:600,870` で入力欄クリック (AX不要)
5. `keystroke "v" using {command down}` + `key code 36` で送信

### OCR取得
- LINE: CGWindowListCreateImage + Vision OCR (画面収録権限必要)
- Discord: 同上 (汎用AppOCRScraper)
- 通知DB: /var/folders/.../com.apple.notificationcenter/db2/db

### 重要な注意
- Keychainは使わない (.secrets.enc ファイルベースAES-GCM暗号化)
- ビルド後は必ず `./scripts/make_app_bundle.sh` で Developer ID 署名
- SwiftPMのincremental buildが壊れやすい → `rm -rf .build` で解決

### ユーザーの要望方向性
- ワンクリックで全部片付くUX
- Webからスマホ操作
- Claude Codeとの連携で作業自動化
- 返信だけでなくアクション実行 (デプロイ、コード書き、LINE送信等)