---
name: KAGI スマートホーム iOS/Mac アプリ
description: KAGIアプリの構造、ビルド方法、Hueリモート実装、Mac Catalystインストール手順
type: project
originSessionId: 961d9f30-c11b-4c96-840b-2062f44cccb9
---
## プロジェクト構造
- **パス**: `/Users/yuki/workspace/kagi/ios`
- **正しいプロジェクトファイル**: `KAGI.xcodeproj`（全ファイル収録）
  - `Kacha.xcodeproj` はファイルが少なく、ビルドに使わない
- **メインターゲット**: `Kacha` スキーム
- **Bundle ID**: `com.enablerdao.kacha`
- **Team**: 5BV85JW8US

## iOS ビルド＆インストール
```bash
cd /Users/yuki/workspace/kagi/ios

# iPhone実機ビルド
xcodebuild build -project KAGI.xcodeproj -scheme Kacha \
  -destination 'platform=iOS,id=00008140-0005453411E0801C' \
  CODE_SIGN_STYLE=Automatic DEVELOPMENT_TEAM=5BV85JW8US \
  -allowProvisioningUpdates \
  -authenticationKeyPath ~/.appstoreconnect/private_keys/AuthKey_5KT46G9Y29.p8 \
  -authenticationKeyID 5KT46G9Y29 \
  -authenticationKeyIssuerID e0d22675-afb3-45f0-a821-06b477f44da0

# iPhone実機インストール
xcrun devicectl device install app \
  --device 00008140-0005453411E0801C \
  ~/Library/Developer/Xcode/DerivedData/KAGI-*/Build/Products/Debug-iphoneos/Kacha.app
```

## Mac Catalyst ビルド＆インストール
```bash
cd /Users/yuki/workspace/kagi/ios

# Mac Catalystビルド
xcodebuild build -project KAGI.xcodeproj -scheme Kacha \
  -destination 'platform=macOS,variant=Mac Catalyst' \
  CODE_SIGN_STYLE=Automatic DEVELOPMENT_TEAM=5BV85JW8US \
  -allowProvisioningUpdates \
  -authenticationKeyPath ~/.appstoreconnect/private_keys/AuthKey_5KT46G9Y29.p8 \
  -authenticationKeyID 5KT46G9Y29 \
  -authenticationKeyIssuerID e0d22675-afb3-45f0-a821-06b477f44da0

# インストール（sudoは不要、yukiがオーナー）
APP_SRC=~/Library/Developer/Xcode/DerivedData/KAGI-*/Build/Products/Debug-maccatalyst/KAGI.app
osascript -e 'tell application "KAGI" to quit' 2>/dev/null || true
rm -rf /Applications/KAGI.app
cp -R "$APP_SRC" /Applications/KAGI.app
open /Applications/KAGI.app
```

## AutoFill Credential Provider (KachaAutoFill)

### 実装済み
- `KachaAutoFill/CredentialProviderViewController.swift` — `ASCredentialProviderViewController` 実装
- `Kacha/Sources/Services/SharedCredentialSync.swift` — Vault → App Group Keychain 同期
- App Group: `group.com.enablerdao.kacha`、Keychain Service: `kagi.autofill`
- JSON payload: `{username, password, title, url}` で保存

### 検証済み（unit test）
```bash
# テスト用認証情報をシードして検証
xcodebuild test -project KAGI.xcodeproj -scheme Kacha \
  -destination "id=087FCFFD-64D5-463F-B27E-E4D0B06E912D" \
  -only-testing:KachaTests/AutoFillSeedTest
# → Smart EX / えきねっと の2エントリが keychain に保存されることを確認

xcodebuild test -project KAGI.xcodeproj -scheme Kacha \
  -destination "id=087FCFFD-64D5-463F-B27E-E4D0B06E912D" \
  -only-testing:KachaTests/AutoFillExtensionTest
# → 4/4 PASS: 読み込み・ドメイン一致・フォールバック動作を確認
```

### 有効化手順（実機/シミュレーター）
1. KAGI.app をインストール（KachaAutoFill.appex が PlugIns/ に内包されていること）
2. 設定 → パスワード → パスワードオプション → 「KAGIパスワード」をオン
3. JRNOなど `.textContentType(.password)` フィールドをタップ → KAGIの候補が表示

### project.yml の重要設定
```yaml
targets:
  Kacha:
    dependencies:
      - target: KachaAutoFill
        embed: true
    settings:
      base:
        PRODUCT_MODULE_NAME: Kacha  # @testable import Kacha が使えるよう維持
  KachaTests:
    settings:
      base:
        TEST_HOST: $(BUILT_PRODUCTS_DIR)/KAGI.app/KAGI
        BUNDLE_LOADER: $(TEST_HOST)
```

## SwiftData ストアの場所（重要）
Mac Catalyst版KAGIが実際に読み書きするSwiftDataは**App Groupコンテナ**:
```
~/Library/Group Containers/group.com.enablerdao.kacha/Library/Application Support/default.store
```
`~/Library/Application Support/default.store` ではない。直接SQLite書き込みする場合は必ずこちらを使うこと。

### iPhoneからHue認証情報を同期する手順（ボタン押し不要）
```bash
# 1. iPhoneを解除してUSB接続
# 2. App GroupのSwiftDataをコピー
xcrun devicectl device copy from \
  --device 7D75CD36-8850-4F30-A149-9495D3545EBF \
  --source "Library/Application Support/default.store" \
  --destination /tmp/iphone_kacha.store \
  --domain-type appGroupDataContainer \
  --domain-identifier group.com.enablerdao.kacha

# 3. Hue認証情報を抽出
sqlite3 /tmp/iphone_kacha.store "SELECT ZNAME, ZHUEBRIDGEIP, ZHUEUSERNAME FROM ZHOME;"

# 4. MacのApp Group storeに書き込む（アプリを止めてから）
pkill -f KAGI
DB="$HOME/Library/Group Containers/group.com.enablerdao.kacha/Library/Application Support/default.store"
sqlite3 "$DB" "UPDATE ZHOME SET ZHUEBRIDGEIP='[ip redacted]', ZHUEUSERNAME='<user>' WHERE Z_PK=1;"
sqlite3 "$DB" "PRAGMA wal_checkpoint(TRUNCATE);"
```

## 既知の落とし穴

### AutoFill entitlements問題
- **症状**: `Provisioning profile doesn't match ... com.apple.security.application-groups`
- **原因**: `KachaAutoFill/KachaAutoFill.entitlements` に `com.apple.security.application-groups` が入っている（Developer Portalに未登録）
- **修正**: そのキーを削除する
- **注意**: Linter/フォーマッターが元に戻すことがある。ビルド失敗時は再確認

### KachaAutoFill は Mac Catalyst 非対応
- `KAGI.xcodeproj` の AutoFill embed ビルドファイルに `platformFilter = ios` を追加済み
- これによって Mac Catalyst ビルド時に AutoFill 拡張機能が除外される
- 該当行: `5716FF0B3D2DC14F2FC93769` の PBXBuildFile エントリ

### Mac Catalyst デバッグログ
```bash
cat /tmp/kagi_debug.txt  # MacStatusBarController のセットアップログ
# 正常なら "menu attached — DONE" が見える
```

## Philips Hue リモートコントロール実装

### アーキテクチャ（リレー方式）
```
iOS App → kagi-server (Fly.io) → hue-relay.py (キャビンPC) → Hue Bridge → 照明
```
Philips開発者アカウント不要。キャビンのローカルネットワーク内でPythonスクリプトを常時起動するだけ。

### kagi-server エンドポイント
- `POST /api/v1/hue/bridge/register` — ブリッジ登録
- `GET /api/v1/hue/lights` — 照明状態取得
- `PUT /api/v1/hue/lights/:id/state` — 個別ライト制御
- `POST /api/v1/hue/scene` — シーン適用 (welcome/night/all_off)
- `GET /api/v1/hue/relay/poll` — リレースクリプトがポーリング
- `POST /api/v1/hue/relay/report` — リレースクリプトが状態報告

### キャビン側セットアップ
```bash
# キャビンのPCで実行（pip install requests 必須）
export HUE_API_KEY=<KAGIアプリ設定画面に表示されるAPIキー>
export HUE_BRIDGE_IP=192.168.x.x
export HUE_USERNAME=<ブリッジペアリング時のusername>
python3 /path/to/hue-relay.py
# バックグラウンドなら: nohup python3 hue-relay.py >> /tmp/hue-relay.log 2>&1 &
```

### iOS アプリ側
- `Home.hueRelayApiKey: String` — SwiftDataモデルに保存
- `HueClient.shared` — ローカル/リモート両対応のメソッド群
- 設定画面: 「自動セットアップ」ボタン1つで discover → pair → enable remote を自動実行

### Mac ステータスバーの Hue 制御
- メニュー開いた時に自動でライト状態を取得しメニュー再構築
- 照明シーン: ウェルカム / 就寝 / 全消灯
- 個別ライト: 最大8灯まで表示、タップでon/off

## モデル
- `Home` — SwiftData: `hueRelayApiKey`, `hueBridgeIP`, `hueUsername`, `wifiPassword`, `doorCode` など
- `activeHomeId` — UserDefaults に保存