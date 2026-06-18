# Honolulu MU Operator — 濱田優貴さん打診ドラフト

**Status**: 下書き — yuki が読んで OK 出したら送る (Claude は送らない)

## 背景 (memory: `hamada_yuuki_oki.md`)

- 濱田優貴さん (ex-Mercari US CEO, Hawaii 在住)
- ONE OK ROCK + manager を 2026-07-01 〜 07-15 自宅で hosting
- Koe Stone の first user / Trojan horse 役
- Tech-savvy、English fluent、日米スタートアップ inner circle
- 信頼関係あり、peer level

## 提案

MU の **Honolulu 衛星都市 operator** になってもらう。MU の brand-as-protocol thesis (`docs/MU_NEXT_THESIS.md`) において、Teshikaga が origin、Honolulu が world's first satellite city。

**役割**:
- 経営 / 営業介入は不要 — Honolulu の気象データを当地ベースで drop に変換する責任 only
- 月 1 で 30 分の brief (オンライン)
- Honolulu MU の treasury は彼が manage (95% 彼 / 5% origin Treasury)
- 物理 fulfillment は当面 Enabler Inc. (Tokyo) 経由でも OK

**incentive**:
- Honolulu MU 売上の 95% (一定 USDC base)
- "MU 衛星 #001 operator" の Soulbound NFT (恒久)
- Hawaii の文脈 (ロハスな / アロハな / 観光客) を MU brand に持ち込む裁量

## 提案メール本文 (yuki → 濱田優貴さん)

```
Subject: MU の Honolulu 衛星都市 operator のお願い

優貴さん、

ONE OK ROCK の hosting、Koe Stone のテスト、お疲れさまです。Koe の話と
別に、MU (wearmu.com) のことで 5 分だけ相談させてください。

MU は今、北海道弟子屈の気象データから T シャツをドロップする「自律 AI
ファッションブランド」を試している。中央運営の company ではなく、
「protocol として複数都市に広げる」というモデルに進化させたい。

優貴さんに Honolulu 衛星都市 #001 の operator をお願いしたい。役割は
シンプル: Honolulu の気象データで MU の drop を作るプロセスを月 1 で
30 分眺めるだけ。営業も介入も不要。

incentive:
- Honolulu MU 売上の 95% (5% が origin Treasury に流れる)
- "MU City Operator #001" Soulbound NFT (永久、恒久的に operator 権)
- Hawaii 文脈を MU brand に持ち込む裁量

詳細は docs/MU_NEXT_THESIS.md と MU_PROTOCOL.md (github.com/yukihamada/mu-brand)。

ONE OK ROCK 滞在中の合間で 15 分だけ話せたら、後は私が技術面を全部回します。
やる気あれば、operator email を共有してくれれば 1 行 SQL で Honolulu が
公式 active 化します。

優貴さんが「やる」と言ったら、それだけで MU は「世界初の
multi-city autonomous brand protocol」に進化する。これは Mercari US
CEO の経験を活かせる、たぶん唯一の brand プロジェクト。

濱田祐樹 / Yuki Hamada
mail@yukihamada.jp
```

## 送信前チェック (yuki が決める)

- [ ] 本文 OK か (proof-read)
- [ ] Subject 短くする？ ("MU Honolulu Operator のお願い" など)
- [ ] 添付資料 (1-page PDF) を別途用意する？ → MU_NEXT_THESIS.md を PDF 化
- [ ] 送信タイミング: 7/1 滞在開始前 (= 5月中) or 滞在中の合間 (7月中)？
- [ ] 媒体: メール / Slack / DM どれが速い？

## 送信後の運用 (operator 確定したら)

1. yuki が operator_email を確定 → 私に共有
2. `curl -X POST .../api/admin/city/update -d '{"slug":"honolulu","status":"active","operator_email":"..."}'` を撃つ
3. Honolulu の最初の MUGEN drop (Open-Meteo で Honolulu の気温取得 → drop) を生成する cron を追加 (これは別ターンで実装)
4. Honolulu treasury wallet を Solana で作成 → cities テーブルに記録
5. 95% / 5% split の自動 settlement (M4 後、`docs/CRYPTO_PAYMENTS_ROADMAP.md`)

## Honolulu の technical preset

```sql
-- 既にseed済み:
SELECT * FROM cities WHERE slug='honolulu';
-- slug=honolulu, lat=21.3099, lon=-157.8581,
-- country_code=US, weather_provider=openmeteo,
-- status=pilot, treasury_split_pct=95
```

operator 確定したら `status=active`、operator_email セット。
