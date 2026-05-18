---
name: Koe OWW install flow
description: openWakeWord setup must call download_models() after pip install; otherwise detection is silently broken
type: feedback
originSessionId: 5bac33ca-2450-4670-a65f-bee3e47284b0
---
Koe の OWW (openWakeWord) セットアップ (`OWWSetupManager.swift`) で `pip install openwakeword` だけでは**モデルファイル本体がダウンロードされない**。`resources/models/` 配下の `melspectrogram.onnx` / `embedding_model.onnx` / `hey_jarvis_v0.1.onnx` 等18個のファイルは、別途 `openwakeword.utils.download_models()` を明示的に呼ばないと作られない。

**Why:** `pip install openwakeword` はコードだけ入れてモデルDLはユーザー任せの設計。呼び忘れると `Model(inference_framework="onnx")` 起動時に `NoSuchFile: melspectrogram.onnx` で落ちる。Koeはこのミスで v1 以来ずっと検出機能が動いてなかった（koe.logに `OWWEngine: ready ✓` が一度も出てない）。

**How to apply:** OWW venv をいじる作業では必ず以下を満たす:
- `install()` 関数で pip install の直後に `python -c "from openwakeword.utils import download_models; download_models()"` を実行
- `checkInstallation()` で `resources/models/melspectrogram.onnx` の存在も併せて確認し、無ければ自己修復として `download_models()` を呼ぶ
- カスタム学習 (`openwakeword.train`) は upstream CLI が `--training_config <YAML>` しか受け付けない。`--training_text` / `--model_name` / `--output_dir` を直接渡す呼び方は**存在しない**ので使わない。カスタム学習は `koe-wake-train` クラウド経由にする