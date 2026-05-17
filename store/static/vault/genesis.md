# MU の誕生 — 公開からの最初の 11 日間

Tシャツ所有者だけに公開しています。

これは MU が **2026 年 5 月 7 日 (公開日)** から **2026 年 5 月 18 日 (今日)** までに、何を考え、何を作り、何を捨てたかを、日付と数字に縛りつけて記録した文書です。 公式の press release / Constitution / Whitepaper では削ぎ落とした「途中の判断」も含めて残します。 これを読むと、MU を **コピーできる**ようになります。

---

## 0. 前提 — なぜこのブランドが必要なのか

ファッション産業は世界の温室効果ガスの **約 10%** を排出していると見積もられています (UN Alliance for Sustainable Fashion / Quantis 2018)。 構造的な原因は単純で、需要予測 → 大量生産 → 売れ残り → 廃棄 という流れが回転している以上、業界全体の規模が拡大するほど排出も廃棄も増えます。

仮説:

> **需要を予測することそのものが間違っている。 まず気象データで生産量を決め、注文が来てから印刷するなら、廃棄はゼロにできる。**

これを実証するには、ブランドを「持っている人」が必要です。 ただし、その人が引退/死亡/離反したら止まる仕組みでは仮説の証明にならない。 **だから、ブランド運営の意思決定そのものを AI に委譲する**。 そうすれば、創業者がいなくなってもブランドは生き続け、ファッション業界全体に対する反証実験として機能し続けます。

MU はこの 1 行の仮説検証のために作られた**フィールド実験**です。

---

## 1. 創業者プロフィール

| 項目 | 内容 |
|---|---|
| 氏名 | 濱田 優貴 |
| 生年 | 1983 |
| 現職 | 株式会社イネブラ (Enabler Inc.) 代表取締役 CEO (2024〜) |
| 主要経歴 | メルカリ 取締役 CPO (2014〜2021) / NOT A HOTEL 共同創業者・元取締役 (2018〜2024) |
| 接続先 | mail@yukihamada.jp / X: @yukihamada |
| 居住地 | 東京 |

MU は濱田個人ではなく **株式会社イネブラ** が運営する一ブランドです。 イネブラ自体は AI / SaaS / デバイスを複数同時開発しています。 MU はそのうち「ブランド経営の自律化」テーマを担います。

公式略歴: `docs/press/prtimes_release.md`

---

## 2. 公開: 2026-05-07

```
2026-05-07 09:00 JST  wearmu.com 公開
                      最初の MUGEN drop (#1) 発生
                      Tシャツ: Bella+Canvas 3001、4.2oz、US made
                      seed: 北海道弟子屈町 気温・湿度・風向・天気
                      gen: gemini-3-pro-image-preview
                      retail: ¥3,500 (era-1 価格)
```

公開と同時に下記が稼働:

- 毎時 cron による MUGEN ドロップ生成
- 北海道弟子屈町の wttr.in からの気象データ取得
- Stripe Payment Link 自動発行
- Printful EU での DTG 印刷自動化
- Cloudflare R2 への画像保存

この日、宣伝はしていません。 X (旧 Twitter) で 1 ポストのみ。 アクセス解析は同日中に installしたため、初日の正確な UV は不明 (推定 50-100)。

---

## 3. 公開 + 4 日 (2026-05-11): "automation から autonomy へ"

公開からちょうど 4 日目、最初の長文ブログ `from-automation-to-autonomy` が公開されました。 そこに残された当時の状態:

> 北海道弟子屈町の気象データから AI が T シャツを生成する MU というブランドを始めた。 今日まで **4 日**。 誰も雇っていないが、毎時 / 毎日 / 毎月、勝手に新しい服が生まれている。 これは「自動 (automation)」だ。 次にやりたいのは「自律 (autonomy)」だ。

同日、内部向け field log #001 も公開:

- 1 人と複数の cron で動いている
- MUGEN #95 がドロップに失敗 (cron 落ち)
- MUON は 2026-05-08, 05-09 が欠損
- Soulbound NFT の仕様が drift していた

「動いている」と「動いている**つもり**」の差を、4 日目で初めて自分で測定しました。 ここから「観測 → 修正 → 公開」が MU の唯一のリズムになります。

---

## 4. Constitution v1: 2026-05-12

公開 5 日目、ブランドの**書面憲法** (`constitution.md`) を確定しました。 30 条文、preamble の 4 行が中核:

> 1. Fashion's seasonal cycle is a marketing artifact. MU has no seasons — only weather and hours.
> 2. A brand can be 0 humans. We are proving it daily.
> 3. A T-shirt is a small piece of climate, hashed to the day it was generated.
> 4. Quiet confidence over loud announcements. Negative space matters. Numbers over adjectives.

仕組み:

- **write 権限**: `yuki@hamada.tokyo` のみ
- **audit trail**: `git log` が永久不変記録
- **last reviewed**: 2026-05-12

§1〜§30 はそれぞれ 1 つの判断ルールを定義します。 たとえば:

| 条 | 内容 |
|---|---|
| §1 | 季節サイクルを否定 |
| §2 | 0 humans 原則 |
| §11 | 数値で逆算する義務 (希望値ではなく目標値で書く) |
| §22 | wearmu.com を 2126-05-13 まで稼働させる 100 年計画 |
| §23 | DAO は token を持たない |
| §24 | fabric era (布地世代) の切替条件 |

Constitution が code repository 内に置かれている意味は単純で、**ブランドの方針変更には git commit が必要**ということです。 私が「気が変わった」と言うだけでは変更できません。 ブランチ作成 → PR → merge → デプロイ という手続きを踏まないと意思は反映されません。 これは「創業者の一存で変える」 ことを物理的に難しくする仕掛けです。

---

## 5. Whitepaper + §23: 2026-05-13 — "0 tokens DAO"

公開 6 日目、DAO 設計書を公開しました (`store/static/whitepaper_dao.md`)。 ここで一番議論したのは **「base token を発行するかどうか」** です。

通常の DAO は ERC-20 を発行し、保有量に投票権が比例します。 これは資本主義的に分かりやすい一方、

- 投機目的の保有者と運営参加者の区別がつかない
- token 発行時点で創業者と初期投資家にプレミアム配分が発生する
- 投票権が市場価格で売買できる (= 民主主義の腐敗パターン)

これらの問題を避けようとした結果、 **「DAO に token は不要である」** という結論に到達しました。

§23 (Constitution / Whitepaper 共通):

> The base token does not exist。 投票重みは 3 つの soulbound primitive を集計する純粋関数だけで決まる。 ICO なし、Airdrop なし、Founder allocation なし、Treasury allocation なし。
>
> §2「A brand can be 0 humans」を最後まで真に受けると、**A DAO can be 0 tokens** にたどり着く。 これが唯一の整合解。

3 つの primitive:

| Primitive | 取得方法 | 期間 |
|---|---|---|
| `tee_holder` | Tシャツを 1 枚以上購入 | 永続 |
| `field_witness` | field log にコメント / Bounty 提出 / Sighting 報告 | 報告日から 365 日 |
| `ma_winner` | MA (間) 週次 auction で落札 | 落札から 100 日 |

これらは soulbound = 譲渡不可。 投票時にウォレットや email から自動算出され、結果が `dao_proposals` に書き込まれます。 これにより:

- 投票権を売買できない (token がない)
- 過去の貢献は時間と共に減衰する (永続独裁を防ぐ)
- 新規参加者にも常に道がある (Tシャツ 1 枚で参加権)

---

## 6. Fabric era-2 への切替: 2026-05-13

公開 6 日目、布地を変えました。

| era | drop_num | 布地 | wholesale | retail | margin |
|---|---|---|---|---|---|
| era-1 | MUGEN 1-147 / MUON 1-9 / MA 1-2 | Bella+Canvas 3001 (4.2oz, US made) | $9 (¥1,440) | ¥3,500 | 59% |
| era-2 | MUGEN 148+ / MUON 10+ / MA 3+ | Stanley/Stella SATU001 Ribbed Neck Creator 2.0 (180gsm organic, EU made) | $13 (¥2,080) | ¥7,800 | 73% |

era-2 を選ぶ理由 (公開当時の `100-in-20-days-strategy.md` から):

- **耐久性**: era-1 は 30 回洗濯で襟がヨレる、era-2 は 100 回でも持つ
- **オーガニック**: GOTS 認証コットン
- **EU 印刷拠点**: Printful Riga (ラトビア) で印刷、海外配送が高速
- **margin 改善**: 1 枚あたり ¥3,800 → ¥5,720 (¥1,920 改善)

era 切替日 (2026-05-13) が公開と Constitution 制定の 1 日後だったのは偶然ではなく、 **5 日間 era-1 を売って数値を見て、布地のコストとマージンの実測値を持ってから判断**しました。

---

## 7. 28 エージェント

MU の自律運営は 28 個 (実際は 39 個、うち 11 個は週次以下のメタ agent) の独立した cron / 関数で構成されています。 主要なものを抜粋:

| Agent | 頻度 | 役割 |
|---|---|---|
| `business_health` | 1h | 在庫率 / SWEEP 👎 ratio / FB backlog / missing daily blog の検知 |
| `treasury` | 6h | Stripe 残高、24h 売上 / コスト、推定 margin の集計 |
| `customer_support` | 30m | 未返信フィードバックを Gemini で分類、テンプレ返信生成 |
| `auto_refund` | 1h | ¥10,000 以下の返金要望を Stripe で自動処理 |
| `compliance_watch` | 24h | 特商法 / プライバシー / 利用規約の最終更新日チェック |
| `self_improvement` | 24h | `agent_journal` をスキャンして繰り返しエラーパターンを検出 |
| `vision_drift` | 24h | 公開している vision text と Constitution を Gemini に読ませて整合性をスコア化 |
| `field_log_compose` | 24h | 1 日の出来事を要約して field log 草稿を生成 (yuki が公開承認) |
| `blog_compose` | 6h | ブログネタ候補を 3 件提示 |
| `x_brand` | 4h | X (旧 Twitter) ポスト草稿生成、`@yukihamada` セルフメンションは skip |
| `mu_sightings` | 1h | Tシャツ着用報告を集約して /city マップに反映 |
| `ma_cancel_auction` | 24h | bid が 0 で 7 日経過した MA auction を自動キャンセル |
| `cron_curl` | 5m | 自分自身のヘルスチェック (Telegram 通知連動) |

各 agent は `agent_journal` テーブルに **1 実行 = 1 行**で記録を残します。 これにより:

- どの agent が何回 fire したかカウント可能
- 失敗 / 成功率を時系列で見える
- self_improvement agent が他の agent のエラーパターンを学習できる

agent 間のオーケストレーション中央管理はありません (Airflow / Temporal なし)。 各 agent は単独で動き、結果を DB に書くだけ。 これは「中央集権がないので、1 つが壊れても他が動く」 という耐障害性パターンです。

---

## 8. 商品体系 — MUGEN / MUON / MA / S404

| Brand | 意味 | 頻度 | 形態 |
|---|---|---|---|
| **MUGEN (無限)** | 無限ループするドロップ | 毎時 (24/day) | 在庫 1-30 枚、累進価格 (¥250/枚売れるごとに値上がり) |
| **MUON (無音)** | 静かな日々のドロップ | 毎日 1 件 | 在庫 100 枚、固定価格 |
| **MA (間)** | 間 = 余白、希少な大型作品 | 毎週 1 件 (公開当初は月 1) | auction、入札制、開始 ¥18,000 (era-2)、bid ¥1,000 刻み |
| **S404** | "404 Not Found" = 売れ残らなかった伝説の design | 不定期 | 月末に売れ残り在庫が 0 になった drop を表彰する非売品 NFT |

`brand` 軸は単純な分類ではなく、**運営原則ごとに別のループに切り分けてる**ことが本質です。 たとえば MUGEN の累進価格は「希望値で書くな (§11)」を強制するために、 「希望売価」 を始値にしてあとは市場が決める設計になっています。

---

## 9. 100 in 20 days — 数値で語る運営原則

公開 7 日目 (2026-05-14)、20 日後 = 2026-06-03 までに **累計 100 着販売** という目標を立てました。

当時の実態:

- 公開 7 日経過、 **累計販売: 8 枚**
- うち**純外部顧客**: 1 名 (Kenny@atsume.io、Hawaii 在住)
- 残り 7 枚は yuki 自身 + 共同創業者 (テスト購入 + サンプル)

100 - 8 = 92 枚、残り 20 日 = 1 日 4.6 枚。 これは現実的でない。 でも、 **「希望値ではなく数値で逆算する (§11)」** の実例として、無理にでも 100 着を起点にし、 そこから逆算で必要施策 (広告予算、PR、コラボ) を導出しました。

経過 (本記録時点 = 2026-05-18 / 公開 11 日目):

- 累計販売: 進行中
- 広告: Google Ads 投入開始 (3 日で ¥42K 投下、0 conv → checkout UX を疑い PAUSED)
- コラボ: kokon (焼肉店)、SIIIEEP (BJJ 道場) 等 4 社と進行中

「数値で逆算する」 ことの最大の効果は、**幻想を切り捨てられる** ことです。 100 着が遠いと分かれば、 30 着を新目標にするか、または「100 着でも届く施策」 を作るかの 2 択になる。 希望ベースで「いつか 100 着」 と書いていたら、永遠に来ない明日の話で終わります。

---

## 10. 100 年計画 (§22)

Constitution §22:

> `wearmu.com` shall remain registered through at least **2126-05-13** (100 years from the Constitution's first publication)。 `/transparency` shows the live expiry date — every visitor can verify the commitment is being kept.

何故 100 年か:

- 創業者 1 名の生存可能性が低い時間軸を選んだ
- 「自律運営」 を主張するならブランドは創業者と切り離されなければならない
- 通常のドメイン契約は 1-10 年。 100 年は明示的に「異常」 で、誰か (含む me) が手動で更新を続けないと達成できない

仕掛け:

- `/transparency` ページに wearmu.com の `paid_through_date` をライブ表示
- 残期間が 5 年を切ったら `compliance_watch` agent が yuki にメール + Telegram alert
- 創業者死亡時に DAO が更新権を引き継ぐ手続書 (legal succession) を準備中 (進行中)

これは「自律ブランド」 の最終試験です。 創業者がいなくなった時に、ドメインが切れた瞬間に MU は死にます。 そうならない仕組みを 100 年分用意することが、 §2「0 humans」 と §22 を両立させる唯一の道です。

---

## 11. 何が次か — 2036 vision

公開 11 日目 (今日 2026-05-18)、 vision.html に書いてある 10 年目標:

> 地球で最初の**完全無人ブランド**を目指す。 Amazon を 1 つの指標で超える。 MUer、MA Council、10 年計画。

中間マイルストン:

| 期日 | 目標 |
|---|---|
| 2026-06-03 | 累計販売 100 着 |
| 2026-12-31 | 月次黒字 (Stripe 売上 > Fly + Printful 原価 + AI 課金) |
| 2027-05-07 | 公開 1 周年、累計販売 1,000 着、DAO アクティブ参加者 50 名 |
| 2030 | MUer 10,000 名 (Tシャツ所有者)、MA Council 100 名 (auction 落札者) |
| 2036 | Amazon を 1 つの指標で超える (どの指標かは MA Council が決める) |

「Amazon を 1 指標で超える」 は意図的に曖昧。 売上では超えない (し、超える必要もない)。 でも、「1 着あたりの CO2 排出量」「ブランド寿命」「在庫廃棄率」 「自律度 (人手介入回数 / 月)」 のうちどれかでは超える可能性がある。 どれを選ぶかは 10 年後の MA Council の判断に委ねます。

---

## 12. このログが読者に対して言いたいこと

MU は「AI でファッションを作ってみた」 という表面的な実験ではなく、 **「組織が人間を必要としないとは何を意味するか」 を 100 年スパンで検証する野外実験**です。

5 月 7 日に何もなかった場所に、

- 30 条文の Constitution
- DAO 設計書 (token なし)
- 39 個の自律 agent
- 4 ブランド (MUGEN / MUON / MA / S404)
- 100 年間運営される約束

が立ち上がりました。 11 日間の作業量としては多いです。 でも、 これらは全部 **public な commit log + Constitution の更新履歴**から再構築できます。 私が居なくなっても、 これを 1 行ずつ追えば誰でも「なぜそれがそうなっているか」が分かるように作りました。

これが、Tシャツ 1 枚から始まる関係に MU が約束できる唯一のことです。

> MU は無くなりません。 100 年は持ちます。 もし切れたら、それは Constitution §22 の違反で、誰かが声を上げるべきです。

---

**最終更新**: 2026-05-18 (公開 11 日目)
**著者**: 濱田 優貴 (yuki@hamada.tokyo)
**ライセンス**: コードは MIT、 文書は CC0
**この記録のソース**: `Constitution §1-§30` / `whitepaper_dao.md` / `docs/press/prtimes_release.md` / `store/static/blog/*` / `git log`
