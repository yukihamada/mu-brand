---
name: bonding-ai project
description: つながりAI = Aron et al.(1997)の36の質問で親密さを育てる日本語音声AIボイスエージェント
type: project
originSessionId: d8011dc7-b4a5-4267-8888-86b1fcfa917b
---
**場所**: `/Users/yuki/workspace/bonding-ai/` (m5: `~/bonding-ai/`, IP: `[ip redacted]` ※DHCP変動あり)

**構成** (全てm5 `[ip redacted]` で実行):
- Pipeline mode: mlx-whisper (large-v3-turbo) → Qwen3.5-122B-A10B-4bit → edge-tts NanamiNeural
- Moshi mode (日本語S2S): akkikiki/j-moshi-ext-mlx-q4 (5GB)
- Web UI: aiohttp on port 8765、Moshi WS on 8998
- A/Bテスト: 15パターン挨拶 (I1〜I15、Opus設計で心理フック)
- 管理画面: `/admin` にランキング・設定・全バリアント表示

**起動**:
```bash
> [line redacted]
```

**起動Python**: `/opt/homebrew/bin/python3.12` (必ずフルパス、`python3.12` コマンドは無い)

**Moshiプロトコル**: ハンドシェイク `b"\x00"` → send `b"\x01"+opus` / recv `b"\x01"+opus` + `b"\x02"+text` 、24kHz Opus

**テスト**:
- `test_pipeline.py` — TTS→Whisper CER + LLM品質
- `test_pipeline_e2e.py` — WebSocket経由でPipeline動作検証
- `test_moshi.py` — WebSocket経由でMoshi動作検証

**Why**: ユーザーがAIと深い会話を体験し、リアルの人間関係に波及させるのが目的。ユーモアや心理フックで「思わず答えてしまう」会話を研究中。

**How to apply**: 変更後は必ず test_pipeline_e2e.py で動作確認。モデル同時ロードでM5 Maxのメモリが足りなくなるのでbf16+122B同時起動はNG。Moshiはq4必須。