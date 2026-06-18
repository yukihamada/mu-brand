---
name: Koe Mac app install path
description: Always install Koe Mac app to /Applications/Koe.app before launching
type: feedback
---

Koe Mac版をビルドした後は必ず `/Applications/Koe.app` にコピーしてから `open /Applications/Koe.app` で起動すること。DerivedDataから直接起動しない。

**Why:** ユーザーが明示的に指示。/Applications が正式なインストール先。

**How to apply:** `cp -R <build_output>/Koe.app /Applications/Koe.app && open /Applications/Koe.app`。既存がある場合は先に `rm -rf /Applications/Koe.app`。