# MU iOS — App Store 提出チェックリスト（人間ゲート）

最終更新: 2026-06-14 / Bundle `com.wearmu.mu` / Team `5BV85JW8US`

自走で完了済み:
- ✅ 実機ビルド BUILD SUCCEEDED（Widget込み・署名通過）
- ✅ 署名済み App Store IPA 生成（`ios/build/MU.ipa`）※`fastlane build`
- ✅ ストアメタデータ 日英 整備（`ios/fastlane/metadata/ja`・`en-US`・`review_information`）
- ✅ privacy=https://wearmu.com/privacy / terms=200 / support=https://wearmu.com を実打確認

以下は **Apple ID 手動（2FA）が必須**で自走不可。上から順に。

## 1. App Store Connect にアプリレコード作成 ★最初の関門
API キーでは作れない（`create_app_online` が `invalid_credentials`）。Apple ID ログインが要る。

**やり方A（推奨・GUI 1分）**: https://appstoreconnect.apple.com → マイApp → ＋ → 新規App
- プラットフォーム=iOS / 名前=`MU ウェアムー`（変更可）/ プライマリ言語=日本語
- バンドルID=`com.wearmu.mu`（既存・provisioningで登録済）/ SKU=`wearmu-mu-001`

**やり方B（CLI）**: ターミナルで `! cd ~/workspace/mu-brand/ios && bundle exec fastlane make_app`
→ Apple ID パスワード+2FAコードを対話入力すれば作成される。

## 2. IPA を TestFlight にアップロード
レコード作成後、`! cd ~/workspace/mu-brand/ios && bundle exec fastlane beta`
（これは API キーで通る。アップロードのみなら Apple ID 不要）。処理完了まで数十分。

## 3. APNs 認証キー（Push 実送信）
今は登録導線だけでサーバから飛ばせない。
- https://developer.apple.com/account → Keys → ＋ → Apple Push Notifications service (APNs) 有効化 → ダウンロード（.p8 は一度きり）
- Key ID と Team ID を控え、MU サーバ（store）の Fly secrets に投入:
  `APNS_KEY_ID` / `APNS_TEAM_ID=5BV85JW8US` / `APNS_AUTH_KEY`(=.p8本文) / `APNS_TOPIC=com.wearmu.mu`
- 投入は `git push` 経由ではなく Fly secrets（鍵はコミット禁止）

## 4. Apple Pay / Stripe merchant（ネイティブ決済）※後回し可
現状は Safari の Stripe Checkout で動くので審査には不要。ネイティブ PaymentSheet にしたいときだけ。
- developer.apple.com → Identifiers → Merchant IDs → `merchant.com.wearmu.mu`
- Stripe ダッシュボード → Apple Pay 証明書を登録

## 5. スクリーンショット（提出に必須）
6.7"（iPhone 16 Pro）で実機撮影。alpha版スクショ不可。
- 推奨5枚: 作る（生成中→完成）/ ライブフィード / ショップPDP / スキャン / アカウント
- ASC の各サイズ枠にアップ。`fastlane deliver` でメタデータごと上げる場合は `fastlane/screenshots/ja/` に配置

## 6. App Privacy / 年齢レーティング / 提出
- App Privacy: 収集=メール・購入履歴・(任意)写真 / Tracking なし（ATT 不要）
- 連絡先電話番号は提出時に手入力（PIIのためリポジトリには置かない）
- レビューノートは `metadata/review_information/notes.txt` に記載済（ログイン不要・Stripeは外部ブラウザ）
- メタデータ一括反映: `! cd ~/workspace/mu-brand/ios && bundle exec fastlane deliver --submit_for_review false`
- 最終「審査に提出」ボタン=Apple ID 手動

## 順序まとめ
1 レコード作成 → 2 TestFlight → 5 スクショ → 6 メタ反映+審査提出。3(APNs)・4(ApplePay) は審査と並行/後でOK。
