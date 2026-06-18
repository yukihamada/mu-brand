---
name: Heavy tasks on m5 Mac
description: 重い計算・Docker・ビルド作業はm5 Mac(yukihamada@[ip redacted] ↔ .47, Apple M5 Max)で実行する
type: feedback
originSessionId: c8eaca8e-45f0-4398-80e1-7516ad54e3d2
---
Docker・Rustビルド・AIエージェント実行など重い作業は yukihamada@[m5 IP]（m5 MacBook Pro, Mac17,7 / Apple M5 Max / hostname YUKInoMacBook-Pro.local）で行う。IPはDHCPで変動する（[ip redacted] ↔ [ip redacted]）。見つからない場合は `arp -a` + SSH probeで再発見。**現在: [ip redacted]（2026-05-09 確認、.47 は ARP incomplete = 不在）**。LAN 外からは agent-bot.koe.live 経由で frpc トンネル（m5:8080）で部分アクセス可。

**Why:** ローカルMac(yuki)ではDockerが動いていないことがある。m5はDocker常時起動でCPU/メモリが豊富。

**How to apply:**
- Dockerが必要な処理 → SSHでm5に転送して実行
- 大規模ビルド・AI推論・ベンチマーク → m5で実行
- SSHコマンド: `ssh yukihamada@[ip redacted]`
- ファイル転送: `rsync -az --exclude='.venv' --exclude='target' <local> yukihamada@[ip redacted]:<remote>`
- Docker path: `/usr/local/bin/docker`（PATHに入っていないので要フルパス or `export PATH=/usr/local/bin:$PATH`）
- Docker credential回避: `DOCKER_CONFIG=/tmp /usr/local/bin/docker ...`