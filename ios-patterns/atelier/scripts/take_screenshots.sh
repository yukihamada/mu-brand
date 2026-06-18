#!/bin/bash
# MU Atelier — 主要4画面+αのスクリーンショット撮影 (専用シミュレータ MU-PAT-ATELIER)
set -uo pipefail
DEV="MU-PAT-ATELIER"
BID="com.wearmu.mu.atelier"
OUT="/Users/yuki/workspace/mu-brand/ios-patterns/atelier/screenshots"
mkdir -p "$OUT"

shot() { # name wait args...
  local name="$1"; shift
  local wait="$1"; shift
  xcrun simctl terminate "$DEV" "$BID" 2>/dev/null
  xcrun simctl launch "$DEV" "$BID" "$@" >/dev/null
  sleep "$wait"
  xcrun simctl io "$DEV" screenshot "$OUT/$name.png" >/dev/null && echo "OK $name"
}

shot 01-home 12
shot 02-collection 10 -atelier-tab collection
shot 03-pdp 12 -atelier-tab collection -atelier-open-first
shot 04-wishlist 10 -atelier-tab wishlist -atelier-seed-wishlist
shot 05-account 6 -atelier-tab account

# ダークモードでも世界観を確認
xcrun simctl ui "$DEV" appearance dark
shot 06-home-dark 12
xcrun simctl ui "$DEV" appearance light

ls -la "$OUT"
