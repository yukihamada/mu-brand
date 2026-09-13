# MU make・iOS・注文データ契約 修正結果

2026-09-14。基点 `origin/main ea5bd96d`、ブランチ `fix/make-order-contract-20260913`。
実装は隔離worktree内。未コミット・未公開。注文DBへの保存項目追加と並行実装は本人承認済み。

## 実装

- iOS: Webと同じ `/api/make/kinds` を取得し、検索・カテゴリ別選択。85種類表示、JP向け76種類作成可、9種類は理由を表示。旧生成/添付/価格/着画レスポンス混入を拒否。正規価格・印税・preview revisionを使用。
- Web: upload世代管理、準備中submit拒否、旧SKUの画像上書き防止、make/remix世代管理。
- 画像: revision付き原画・印刷配置・preview対応、古いジョブの書戻し拒否。Printfulの正規mockupを完成プレビューとし、AI参考画像との混同を防止。mockupと注文で同じ商品別geometryを使用。
- 通常/パートナー/サンプル/crypto: checkout時に原画・全印刷面・options・variant選択肢・単価・数量のsnapshotを保存。以降の商品編集から分離。
- 入稿: 表示画像・ロゴ・無地へのfallback廃止。サイズを黙って変更しない。既存sync商品は公式store/variants APIから入稿仕様を取得して固定。
- 発注: 入金確認・返金確認、永続キュー、原子的claim、外部ID照会後の送信、履歴維持、部分失敗、報酬の冪等記帳。cryptoも共通送信契約を使用。
- サンプル: 認証済みパートナーは承認前sampleを作れる。一般公開販売とは状態判定を分離。承認/保留のcanonical同期。
- 在庫: live商品GETでvariant所属・在庫・印刷面を確認。未選択の欠品サイズを除き、Stripeとsnapshotの選択肢を統一。購入済みサイズが欠品しても別サイズへ代替しない。

## 供給先の実測

Printfulの公開商品GET80件成功。84kindすべての既定variantが対応商品に実在した。
ただし以下9種類は現在JP向け作成停止:

| 種類 | 理由 |
|---|---|
| dog_tee | 実商品が人間の子供用Tシャツ |
| hardcover_photo_book / softcover_photo_book | 複数ページ入稿が未対応 |
| pet_collar / christmas_stocking | 監査時の既定variantが欠品（一時停止） |
| tank / pet_bowl / notepad | JPへの配送制限 |
| rashguard_contrado | 製造spec未実装 |

トート色/寸法・素材等の誤記を実カタログに合わせ修正。印刷面15kindの不一致は13kind訂正、写真集2kind停止。vendor IDは推測置換していない。

元実測: temp内 `mu-printful-verification-20260914.md/.json`。永続回帰fixtureは `store/tests/printful_catalog_20260914.json`。

## 最終検証

```sh
# store/
cargo test --offline --locked --bin mu-store
# 192 passed; 0 failed; 1 ignored (既存のdump_qr_sample)

# ios/
swift test --jobs 2
# 30 passed; 0 failed
xcrun swiftc -typecheck -target arm64-apple-ios17.0 \
  -sdk "$(xcrun --sdk iphoneos --show-sdk-path)" -module-name MU \
  MU/Sources/Core/*.swift MU/Sources/Features/*/*.swift MU/Sources/MUApp.swift
# 成功
git diff --check
# 成功
```

実handler応答 `ios/MUTests/Fixtures/make-kinds.json` をRustとSwiftで共用し、全ラベル・価格・可否・理由を照合。
注文テストはメモリ内DBとlocalhostの偽Stripe/Printful。応答喪失後POST1回、返金中POST0回、誤variant/欠品/不明在庫/配送不可時POST0回、snapshot不変、並行claim、報酬ロールバックを検証。

## 未確認と運用上の残件

- m5はこのMacと本人訂正、M5 Max128GB実測。フルXcode build成功。Fastfileの既存ASC API認証で実機署名も解決し、iPhone16Proへインストール・起動済。実機の日時parse失敗をPOSIX/Gregorian固定で修正。iOS26.5.2実機で30unit+2UI=32件成功（`mu-device-smoke.xcresult`）。Make一覧検索→マグ選択、Shop商品詳細の購入URL/SKU一致まで確認。購入ボタンは押していない。本番kinds404なので一覧はfallback、新85種類の本番結合は配備待ち。スクショ7枚保存、モデル画像非対応で目視評価は未確認。
- 認証付きPrintful printfiles/store variantsの実接続・実寸・製造受理と、Stripe実サービスE2Eは未確認。商品GETだけで全配送先の注文成立を保証していない。
- Stripe確認とPrintful注文は別システムで原子的でない。送信直前にも返金確認するが、その後の返金/取消は照合が必要。
- プロセス停止時にsending/submittingだった注文は、時間経過だけで横取り・再発注しない。外部注文の手動照合が必要。
- snapshotのない既存注文は `blocked_legacy_snapshot`。再製造を推測しない。
- 本番DB migration、配備、iOS配布、実課金/製造テストは未実施。公開後検証は別途必要。

詳細iOS契約・実機引継ぎ: `ios/MAKE_FIXES.md`。

## 本番反映・実機結合確認 2026-09-14

- 本人承認で `db69e223` をmainへpush。GitHub Actions run `34768887507` 成功。
  https://github.com/yukihamada/mu-brand/actions/runs/34768887507
- 本番 `/api/make/kinds` HTTP200、85種類・JP作成可76種類、`/healthz` ok:true。
- 実機iPhone16Pro/iOS26.5.2の `testProductionExpandedCatalogLoadsWithoutFallback` 成功。
  fallback警告なし、検索で `wall_clock` を取得し、選択値が一致することを確認。
  証拠: 承認済みtempの `mu-device-expanded-catalog-reconnected.xcresult`。
- アプリ起動済み。新一覧の本番結合は確認済みへ更新。実生成・決済・製造は行っていない。
