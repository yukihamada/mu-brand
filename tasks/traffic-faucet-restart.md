# MU 流入の蛇口 再起動プラン (2026-07-02)

## 診断（実データ）

新設した `GET /api/admin/funnel/ab` と enabler-analytics の実測から：

- **MU の流入は枯れている**: `/buy` の pageview は **直近7日=0件**。30日で 2,738PV あるが
  その大半は 6/7〜6/10 のスパイク（6/7=801PV）で、以後は減衰して現在ほぼ停止。
- **A/B は健全だが判定不能**: `/buy` の a〜e は全て正しく割当済（`(none)` は 6/11 の
  A/B 開始前の履歴）。CTR は a=41.7% / b=40%（days=21, 各 24〜30UU）と旧2.4%比で桁違いに
  高いが、**サンプルが小さく（<100/arm）トラフィックが無いので勝者を出せない**。
- **結論**: いま効くレバーは `/buy` のレイアウトでも A/B でもなく、**トラフィックそのもの**。

## 戦略の制約（mu-brand/CLAUDE.md）

> MU 単独で一般アパレルを狙わない。実需のある JiuFlow(BJJ) に MU を従属させる。

→ 新規に一般向け集客を作るのではなく、**既に実需のある JiuFlow の導線に相乗り**する。

## 生きている蛇口（比較）

| プロダクト | PV/7d | 状態 |
|---|---|---|
| **jiuflow.com** | **2,733** | 生きている（実需・BJJ） |
| koe.live | 328 | 生きている |
| wearmu.com | 92 | ほぼ枯れ（/buy は 7d=0） |

JiuFlow は MU の **~30倍** のトラフィック。ここが唯一の現実的な水源。

## 再起動プラン（優先順）

### レバー1 ★最優先: JiuFlow → MU(BJJ) 常設ブリッジ
JiuFlow の実 BJJ トラフィックを MU の BJJ コレクションへ流す。
- 導線先: `https://wearmu.com/shop?brand=<bjj>&ref=jiuflow`（`?ref=` 帰属は MU 側実装済）
- 実装A（今すぐ・低コスト）: JiuFlow に **お知らせ/ニュース** を1本立てる
  （下書き: `tasks/jiuflow-bridge-announcement.md`）。**投稿は人間ゲート**（他者可視）。
- 実装B（恒久）: JiuFlow アプリ内に常設のMUカード（`?ref=jiuflow` 付き）。要 JiuFlow 側PR。
- 計測: `/api/admin/funnel/ab?path=/buy` と MU の `referral_code` で JiuFlow 由来CVを追跡。

### レバー2: YouTube 再ドリップ
30日の最大 referrer は YouTube(338) だが手動プロモの減衰。恒久蛇口が無い。
- 既存インフラの流用: JiuFlow は `~/jiuflow-yt-drip/` の自動アップローダを持つ（memory
  `jiuflow_youtube_manual_faucet`）。MU 用に同型の drip を新設 or MU素材を JiuFlow 枠に相乗り。
- **YouTube アップロードは人間ゲート**。まず1本、BJJ×MU の短尺を限定公開→反応見て公開。

### レバー3: 焚き火(takibi) セラー
`mu-takibi-seller` エージェントが焚き火の流れに合うMU品を紹介文込みで下書き済の仕組みあり。
- 提案まで自動・**投稿と決済は人間ゲート**。`?ref=takibi` で計測。

### やらない
- 一般向けの新規有料広告（戦略により MU 単独集客は非推奨・MU広告は全 PAUSED 継続）。
- `/buy` レイアウトのさらなる磨き（トラフィックが戻るまで A/B 判定不能＝磨いても測れない）。

## 成功の測り方
1. JiuFlow ブリッジ投稿後、`/api/admin/funnel/ab?path=/buy&days=7` の pageview_visitors が回復。
2. `ref=jiuflow` 帰属の buy CTA / Stripe CV が発生。
3. 各 arm が ~100 UU に達したら A/B 勝者を判定 → `MU_BUY_AB_WEIGHTS` を勝者へ収束。

## 人間ゲート（優貴さんの GO 待ち）
- [ ] JiuFlow お知らせ投稿（下書き承認）
- [ ] YouTube 短尺アップロード
- [ ] 焚き火への MU 紹介投稿
