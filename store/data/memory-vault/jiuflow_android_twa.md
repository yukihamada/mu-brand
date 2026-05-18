---
name: jiuflow-android-twa
description: JiuFlow Android (TWA via Bubblewrap) — scaffold済み、初回ビルド + Play Console 提出待ち
metadata: 
  node_type: memory
  type: project
  originSessionId: cad8761e-babd-42aa-af02-724c91106229
---

JiuFlow Android = **Trusted Web Activity (TWA)** で jiuflow.com の SSR をラップして Play Store 出稿。アンケート(2026-05-12)で「Android対応してほしい」要望に対する Phase 1。

**Why:** ネイティブ Kotlin/Compose 実装は数ヶ月。TWA なら数日でストア提出可、Web側の改善が即時反映される。要望コメント1件で多大な工数を投じるリスクを最小化。Phase 2 で native 検討。

**How to apply:**
- リポジトリ: `/Users/yuki/workspace/bjj/jiuflow-ssr/android/` (jiuflow-ssr 配下に同居)
- 設定: `android/twa-manifest.json` がSoT。Gradle生成物(`app/` `build/`)はgitignore
- Package: `com.jiuflow.android` (iOS の `art.jiuflow.ios` とは別)
- ドメイン側 assetlinks.json: `/.well-known/assetlinks.json` ルートで配信中、SHA-256 は keystore 生成後に差し替え必須
- ビルド: m5 Mac (`yukihamada@[ip redacted]`) で `bubblewrap init/build`
- 初回必要: JDK17 + Android SDK + 新規 keystore (1Password 保管)
- Play Console: \$25 developer fee、screenshots、feature graphic 1024x500 必要

**残作業 (Phase 1 完了まで):**
1. m5 Mac で `bubblewrap init --manifest ./twa-manifest.json` → SDK/JDK 自動DL
2. keystore 生成 + パスワード保管
3. `bubblewrap fingerprint` で SHA-256 取得
4. `static/.well-known/assetlinks.json` の REPLACE_WITH_SHA256_OF_RELEASE_KEYSTORE を更新 → git push
5. `bubblewrap build` → `app-release-bundle.aab`
6. Play Console: Internal testing track 投入 → Closed → Open → Production

**Phase 2 (将来):** native Kotlin/Compose。jiuflow-swift と同等のオフライン視聴・ハプティック・ネイティブ通知を取り込む段階で着手。

**関連:** [[ios_build_deploy]] (iOS と Android で keystore/署名の管理思想は別)、[[feedback_heavy_tasks_m5]] (Android SDK ビルドは m5 で)