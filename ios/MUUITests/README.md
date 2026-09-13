# 実機・本番接続スモーク

メイン担当が物理端末で実行する2件。実装担当ではビルド・実機起動は未実施（unverified）。

- `MUUITests/ProductionSmokeTests/testMakeKindPickerSelectsMug`
  - 「作る」→種類一覧→`mug`検索→マグ選択。取得失敗表示がある場合はフォールバック文言と「おまかせ」も確認する。
- `MUUITests/ProductionSmokeTests/testShopProductDetailHasPurchaseURLWithoutCheckout`
  - 実際の商品フィード→商品詳細→購入ボタンの表示・操作可能状態・HTTPS checkout URL・SKU一致を確認。購入ボタンは押さない。

各段階のスクリーンショットを `XCTAttachment.lifetime = .keepAlways` で保存。失敗時は最終画面とアクセシビリティ階層も保存する。

`-hasOnboarded YES` は既存の `@AppStorage` を起動時の引数ドメインで上書きする。オンボーディングのスキップ操作による永続保存を避ける。言語・地域も起動引数で日本語に固定。通常の本番通信を利用するため、既存の起動・画面表示analytics、および通知許可済み端末の既存APNs再登録は通常通り動く。テスト操作から通知許可・メール・生成・購入は呼ばない。

## 実測済みの本番API（2026-09-14）

- `GET https://wearmu.com/api/make/kinds` → **HTTP 404**（HTML）。本番の全種類一覧、85種類のUI検証は未確認。
- `GET https://wearmu.com/api/shop/feed.json?page=1&physical=1` → 商品60件。先頭SKU `MAKE-MAKE-STICKER-mk5dc044fd`、checkout URLは `https://wearmu.com/api/shop/checkout?sku=MAKE-MAKE-STICKER-mk5dc044fd`。
- Checkout URL自体へのリクエストはしていない。Safari遷移・Stripeセッション作成・決済完了は検証範囲外。

## 実行

作業ディレクトリ: `/var/folders/5h/3zhkzzk12_1g06p6w2vxc45r0000gn/T/sente/mu-make-order-fix/ios`

`DEVICE_UDID` にメイン担当で署名・接続確認済みの物理端末UDIDを設定する。署名引数は既存 `fastlane/Fastfile:9–11,24` と同じ。結果出力先は既存の空きディレクトリ配下で未使用の名前を指定する。

```bash
xcodebuild test \
  -project MU.xcodeproj \
  -scheme MU \
  -configuration Debug \
  -destination "platform=iOS,id=${DEVICE_UDID}" \
  -only-testing:MUUITests/ProductionSmokeTests \
  -parallel-testing-enabled NO \
  -resultBundlePath /var/folders/5h/3zhkzzk12_1g06p6w2vxc45r0000gn/T/sente/mu-device-smoke.xcresult \
  -allowProvisioningUpdates \
  -authenticationKeyID 5KT46G9Y29 \
  -authenticationKeyIssuerID e0d22675-afb3-45f0-a821-06b477f44da0 \
  -authenticationKeyPath "$HOME/.appstoreconnect/private_keys/AuthKey_5KT46G9Y29.p8"
```

プロジェクトは `xcodegen generate` で再生成済み。`MUTests/Fixtures` の既存リソース設定を保持し、`MU` スキームに `MUUITests` を非並列で追加した。静的確認: `plutil -lint`、スキームの `xmllint --noout`、テストSwiftの構文パース、`git diff --check`。
