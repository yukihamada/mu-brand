---
name: Koe build output cleanup
description: Never output Koe.app to workspace directories — only /Applications/Koe.app
type: feedback
---

ビルド成果物(Koe.app)をworkspace内のディレクトリ（build-macos/, Koe-iOS/build/ 等）に置かない。
ビルド後は必ず `/Applications/Koe.app` にコピーして、それ以外の場所には残さない。

**Why:** ユーザーがworkspace内にKoe.appが散らばるのを嫌がった。

**How to apply:** ビルド後は `rm -rf /Applications/Koe.app && cp -R <DerivedData>/Koe.app /Applications/Koe.app` のみ。workspace内にコピーしない。