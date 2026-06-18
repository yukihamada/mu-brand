---
name: GEMINI_API_KEY shell env が stale
description: ~/.zshrc の GEMINI_API_KEY は revoke 済み。常に /Users/yuki/.env を source して使う
type: feedback
originSessionId: 1ce5a54b-0fdf-4f54-9bf4-90a31df46c16
---
`~/.zshrc` で export されている GEMINI_API_KEY / GOOGLE_API_KEY は revoke 済みで `400 API_KEY_INVALID` を返す。
有効なキーは `/Users/yuki/.env` 側 (異なる値) にある。

**Why:** zshrc に書いた古いキーが残ったまま、新しいキーは .env に追記された (2026-05 sweep_images.py 実行時に発覚 — 全 12 商品分の Gemini 呼び出しが failed)。

**How to apply:** Gemini を CLI から叩く前は必ず:
  `set -a && source /Users/yuki/.env && set +a`
を実行してから python / curl を呼ぶ。zshrc 由来の export を上書きする必要がある (順番が大事)。Python スクリプトは `os.environ.pop('GOOGLE_API_KEY', None)` で priority を効かせる癖がついている。