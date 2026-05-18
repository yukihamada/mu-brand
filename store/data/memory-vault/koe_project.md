---
name: koe_project
description: Koe voice input project - 4 components (macOS/iOS Swift, Windows Rust, ESP32 firmware, landing site), architecture and security status
type: project
---

# Koe プロジェクト全体像

音声入力ツール。whisper.cpp でローカル音声認識し、テキスト変換してペースト。

## 4つのコンポーネント

### 1. Koe-Swift (macOS/iOS) — メインプロダクト
- **パス**: `Koe-swift/Sources/Koe/` (38 Swift files)
- **技術**: Swift/SwiftUI, whisper.cpp (Metal GPU), llama.cpp
- **ビルド**: `Koe-swift/build.sh` → PKG(2.8MB), DMG(3.1MB), App(5.5MB)
- **最適化済み**: `-Osize`, `-whole-module-optimization`, `-dead_strip`, strip

### 2. Koe-Windows (Rust) — Windows版
- **パス**: `Koe-swift/Koe-windows/`
- **技術**: Rust, whisper-rs, cpal (WASAPI), CUDA対応
- **最適化済み**: reqwest→ureq, indicatif削除, opt-level="z", fat LTO, panic=abort

### 3. Koe+Soluna Device (ESP32-S3 ファームウェア) v0.6.0
- **パス**: `koe-device/firmware/` — 2,248行 (8 modules)
- **DSPパイプライン**: ハイパスフィルタ→AEC→ノイズゲート→AGC→リミッター→ボリューム
- **Solunaプロトコル**: ADPCM 4:1, PLC, SNTP, Heartbeat(5秒), ChaCha20暗号化, WANリレー対応
- **操作**: 短押し=録音, ダブルタップ=チャンネル巡回, 中押し=モード切替, 長押し=BLE, 5連打=ファクトリーリセット
- **堅牢性**: ハードウェアWDT(30秒), クラッシュダンプNVS保存, WiFi自動再接続, OTA自動更新
- **通信**: ステータスレポート(5分毎), mDNS, HMAC-SHA256, TLS, NVS/Flash暗号化, Secure Boot v2

### 4. Koe-Site (ランディングページ)
- **パス**: `Koe-swift/site/`
- **最適化済み**: tokio features絞り込み, release profile追加

## ESP32セキュリティアーキテクチャ (v0.2.0で実装済み)
- **認証**: HMAC-SHA256でリクエスト署名 (リプレイ攻撃防止のタイムスタンプ付き)
- **保管**: NVS暗号化 (WiFi SSID/Pass, API Key, Device ID)
- **通信**: TLS + mbedTLS証明書バンドル
- **物理**: Flash暗号化 + Secure Boot v2
- **OTA**: デュアルパーティション (ota_0 + ota_1) — OTAロジック未実装

**Why:** 音声データはプライバシー上最も機密性の高いデータの一つ。デバイスからクラウドへの経路全体の保護が必要。
**How to apply:** サーバー側(chatweb.ai API)でもHMAC検証+タイムスタンプ有効期限チェックの実装が必要。