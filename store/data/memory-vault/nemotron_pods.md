---
name: Nemotron & Qwen GPU Pods
description: RunPod GPU pod configurations for Nemotron 9B and Qwen3-32B used by chatweb.ai Lambda
type: project
---

# Nemotron GPU Pod — 現在の構成 (2026-03-19更新)

## RunPod API Key
- `[key redacted]`

## chatweb.ai (Fly.io chatweb-ai) — Primary
- **Active Pod**: 522yvkztmvm2n0 (nemotron-new), URL: `https://522yvkztmvm2n0-8000.proxy.runpod.net`
- **⚠️ 旧Pod (0cnrvycoh0fom8) は消滅済み (2026-03-19確認)**
- **Image**: `vllm/vllm-openai:latest-x86_64` (v0.16.0はNemotron-Hクラッシュ)
- **Model**: `nvidia/NVIDIA-Nemotron-Nano-9B-v2-Japanese`
- **Args**: `--trust-remote-code --max-num-seqs 32 --max-model-len 8192 --gpu-memory-utilization 0.90 --dtype bfloat16`
- **max_model_len**: 8192 (8K) — 高スループット設定。max_num_seqs=32で32同時推論可能

## 長コンテキスト用ポッド
- 12kg1obn9cunp1 (nemotron-32k): max_model_len=32768, max_num_seqs=8
- s0ccajsbvef6cd (nemotron-128k-new): max_model_len=131072, max_num_seqs=4
- **⚠️ AVOID 128K**: 巨大KVキャッシュでmax_num_seqs=4しか使えず輻輳→タイムアウト

## Key Settings
- enable_thinking: false (Lambda側でvLLMに送信 → `</think>`タグなし)
- safe_max_tokens: 16384
- 起動時間: ~4分 (新マシン), ~90秒 (キャッシュあり)
- **CRITICAL**: `volumeInGb`/`volumeMountPath`禁止 — ヘルスチェックタイムアウトループの原因
- ツール呼び出し: `<TOOLCALL>[...]</TOOLCALL>`形式 → `parse_toolcall_format()`でパース

## Qwen3-32B GPU Pod (2026-03-02追加)
- **⚠️ 旧Pod (r4lgsrcfwoew89) は消滅済み (2026-03-19確認)**
- ~~Active Pod: r4lgsrcfwoew89 (qwen3-32b), URL: `https://r4lgsrcfwoew89-8000.proxy.runpod.net`~~
- **GPU**: RTX A6000 (48GB VRAM), $0.49/hr
- **Model**: `Qwen/Qwen3-32B-AWQ` → served as `qwen3-32b`
- **Args**: `--max-num-seqs 4 --max-model-len 16384 --gpu-memory-utilization 0.90 --dtype bfloat16`
- **CRITICAL**: RTX A5000 (24GB) crashes with max_model_len>4096 for Qwen3-32B-AWQ (OOM). Use 48GB+ GPU

## nemotron-voice (chat.elio.love)
- nemotron-voice Fly.io: NEMOTRON_POD_URL=https://hoxj04jprx4uqq-8000.proxy.runpod.net

## Serverless Fallback
- RunPod endpoint: 8kfvmqlgc3deam, Model: Llama-3.1-Swallow-8B
- Gemini fallback: OpenRouter `google/gemini-2.0-flash-001`