---
name: iOS build & deploy procedures
description: Elio/Pasha iOS apps - fastlane, signing, TestFlight, App Store審査の正しい手順。コマンドラインでできること/できないこと。
type: feedback
---

## 共通情報
- **Team ID**: 5BV85JW8US (Yuki Hamada)
- **Apple ID**: [email redacted]
- **API Key**: 5KT46G9Y29 (issuer: e0d22675-afb3-45f0-a821-06b477f44da0)
- **API Key Path**: `~/.appstoreconnect/private_keys/AuthKey_5KT46G9Y29.p8`
- **Distribution Cert**: `20FD22928A6D5ACF3D34A278979F712E5B13ED64` (Apple Distribution)

## Elio (love.elio.app)
- **Path**: `ai/elio`
- **Version**: 1.2.41 (build 54) — 2026-03-18時点
- **fastlane**: 完全自動パイプライン
- **Certificate管理**: Match (git: github.com/yukihamada/elio-certificates)

```bash
# TestFlightアップロード（約10分）
cd ai/elio && fastlane ios beta

# ユニットテストのみ
fastlane ios unit_test

# フルテスト（unit + UI）
fastlane ios test

# App Storeリリース
fastlane ios release

# 証明書同期
fastlane match appstore
```

## パシャ (com.enablerdao.pasha)
- **Path**: `pasha/ios`
- **Version**: 1.0.0 (build 5) — 2026-03-18時点
- **XcodeGen**: `project.yml` → `xcodegen generate` が必須

```bash
cd pasha/ios

# 1. ビルド番号更新 (project.yml の CURRENT_PROJECT_VERSION)
# 2. xcodegen generate
# 3. fastlane ios beta（TestFlight）
fastlane ios beta

# 手動フロー（fastlane使わない場合）
xcodebuild archive -project Pasha.xcodeproj -scheme Pasha -configuration Release \
  -destination 'generic/platform=iOS' -archivePath build/Pasha.xcarchive \
  PROVISIONING_PROFILE_SPECIFIER="com.enablerdao.pasha AppStore" \
  CODE_SIGN_STYLE=Manual CODE_SIGN_IDENTITY="Apple Distribution" DEVELOPMENT_TEAM=5BV85JW8US

xcodebuild -exportArchive -archivePath build/Pasha.xcarchive \
  -exportOptionsPlist build/ExportOptions.plist -exportPath build/export \
  -authenticationKeyPath ~/.appstoreconnect/private_keys/AuthKey_5KT46G9Y29.p8 \
  -authenticationKeyID 5KT46G9Y29 \
  -authenticationKeyIssuerID e0d22675-afb3-45f0-a821-06b477f44da0
```

## コマンドラインでできないこと

**App Store審査提出のみ** APIキー(5KT46G9Y29)ではできない。
→ fastlane deliver + Apple ID認証が必要:
```bash
FASTLANE_USER=[email redacted] \
> [line redacted]
> [line redacted]
fastlane deliver --username [email redacted] --app_identifier <bundle_id> \
  --skip_binary_upload --skip_metadata --skip_screenshots \
  --submit_for_review --automatic_release false --force \
  --precheck_include_in_app_purchases false
```

**Why:** APIキーの権限では `appStoreVersionSubmissions CREATE` が不可。Apple ID + App-specific passwordが必要。

**How to apply:**
- ビルド → TestFlightアップロード: `fastlane ios beta` で完結（APIキーで可）
- 審査提出: fastlane deliver経由（Apple ID認証が必要）
- パシャは `demoAccountRequired: false` 必須（ログイン不要アプリ）

## コマンドラインで全部できること
| 操作 | コマンド |
|------|---------|
| ビルド&TestFlight | `fastlane ios beta` |
| テスト | `fastlane ios test` / `fastlane ios unit_test` |
| App Storeリリース | `fastlane ios release` |
| 証明書同期 | `fastlane match appstore` |
| メタデータ更新 | `fastlane deliver --skip_binary_upload` |
| スクリーンショット | `fastlane snapshot` |
| ビルド番号確認 | `fastlane run app_store_build_number` |
| 実機インストール | `xcrun devicectl device install app --device <UDID> <path>` |
| 実機のUDID | iPhone: `00008140-0005453411E0801C` |