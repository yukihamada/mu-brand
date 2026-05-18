---
name: uta_live_project
description: uta.live AIカラオケプロジェクト — 全機能・インフラ・KPI・残タスクの完全記録
type: project
originSessionId: 8c3b2335-61c2-4bb4-be8e-4045418dfc1d
---
## uta.live — AI Karaoke Platform

### URLs
- **Live**: https://uta.live (Fly.io karaoke-pro, nrt)
- **Alt**: https://karaoke-pro.fly.dev
- **Admin**: https://uta.live/admin
- **Analytics**: https://uta.live/analytics
- **Royalty**: https://uta.live/royalty
- **Pricing**: https://uta.live/pricing
- **Press**: https://uta.live/press
- **Legal**: https://uta.live/legal

### コードベース
- **パス**: `/Users/yuki/workspace/karaoke/`
- **メインファイル**: `web.py` (~10,000行, HTML/CSS/JS埋め込み)
- **バックエンド**: `karaoke.py` (search, download, transcribe, refine)
- **Fly.io**: `karaoke-pro`, performance-2x, 8GB RAM, 500GB Volume
- **ドメイン**: uta.live (Cloudflare zone: fb925896388dc51131103b6e3d754b55, GMOお名前.com)

### KPI (2026-04-11時点)
- カタログ: 2,347曲
- JSON: 2,301曲
- 即歌える(audio有): 1,475曲 (66%)
- Demucs(HD): 104曲(サーバー) / 1,923曲(m5)
- 30日再生: 83回+

### 技術スタック
- Backend: FastAPI + uvicorn, SQLite on Volume
- AI: demucs htdemucs, faster-whisper, Claude Haiku
- Frontend: Vanilla JS, Web Audio API, Canvas
- Billing: Stripe (¥980/月 Standard), Solana ENAI Token
- Analytics: 独自(SQLite pageviews/events)
- Chat: Claude Haiku AIアシスタント(全ページ💬)

### m5 Mac ([ip redacted])
- 18コア, 128GB RAM
- Python 3.11.15 (pyenv)
- demucs 4並列ビルド実行中
- 1,923曲demucs完了
- ffmpeg/ffprobe: ~/bin/
- cookies.txt: ~/karaoke/cookies.txt

### 主要な修正履歴
- 曲違い問題: quickStartで必ず候補リスト表示
- 戻るボタン: popstateでnewSong()呼び出し
- 英語/中国語歌詞混入: 日本語文字30%未満で拒否
- 空ファイル: instant_playでst_size>1000チェック
- Job消失: get_or_restore_job()でキャッシュから復元
- Service Worker: uta-v11, network-first
- OGP: 曲別OGP、デフォルトog:titleの置換

### 残タスク
1. JASRAC申請 (docs/jasrac_application.md)
2. 歌詞なし曲の一括Whisper処理 (サーバーで進行中)
3. 全曲のaudio DL (サーバーで進行中)
4. m5のdemucsをサーバーに転送する方法の確立
5. NexTone契約
6. カタログ3,000曲到達
7. auto_sync定期デプロイの安定化