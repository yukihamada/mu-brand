---
name: m5 Mac セットアップ状況
description: m5 Mac ([ip redacted]) の各種設定・自動化・アクセス方法
type: project
originSessionId: b1e3d51a-414c-4fac-a249-823ca9f98f37
---
## m5 Mac ([ip redacted] / Tailscale: 100.104.27.81)

### アクセス方法
- **LAN**: `ssh yukihamada@[ip redacted]`
- **Tailscale**: `ssh yukihamada@100.104.27.81` または `m5` コマンド (~/bin/m5)
- **Cloudflare tunnel**: `ssh m5` (~/.ssh/config で ProxyCommand 設定済み)
- **iPhone**: Termius → 100.104.27.81、ED25519鍵登録済み

### tmux + Claude Code 自動起動
- LaunchAgent: `~/Library/LaunchAgents/com.yukihamada.claude-tmux.plist`
- 再起動後に `tmux new-session -s main` + `claude` が自動起動
- iPhoneから: `tmux attach -t main` で同じセッションに接続

**Why:** iPhone含め外出先からどこでもClaude Codeに繋げるようにするため

### インストール済みツール
- tmux (`/opt/homebrew/bin/tmux`)
- Ghostty (`/Applications/Ghostty.app`)
- gog バイナリ (`/opt/homebrew/bin/gog`)
- Gmail API ライブラリ (google-auth, google-api-python-client, httpx)

### SOLUNA メールエージェント
- スクリプト: `~/soluna_mail_agent.py`
- cron: `0 * * * *` (毎時0分実行)
- ログ: `/tmp/soluna_agent.log`
- DB: `~/soluna_agent.db` (処理済みメール追跡)
- 動作: Gmail API で「個別相談リクエスト」未読メールを検索 → claude-sonnet-4-6 で返信文生成 → 送信 → SOLUNA activity_feed に記録
- Gmail認証: refresh_token を Pythonスクリプト内に直接埋め込み (keychainはSSH非対応のため)
- SOLUNA API: `GET /api/admin/kpi` + `x-admin-key: LIFEISART`、`POST /api/activity`
- Telegram通知: @yukihamada_ai_bot (chat_id: 1136442501)

**How to apply:** m5のcron設定や自動化タスクを追加するときの参考に