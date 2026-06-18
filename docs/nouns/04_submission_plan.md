# Nouns DAO 提案提出フロー — 実行計画

## 結論

**いきなり on-chain proposal は出さない**。順番がある。

```
[Step 1] Discord #ideas 投稿
   ↓ (24-72h 反応見る)
[Step 2] discourse.nouns.wtf にプレ提案スレッド
   ↓ (1-2 週間ディスカッション)
[Step 3] フィードバック反映 → nouns.camp に candidate proposal を起こす
   ↓ (3 Nouns の sponsor を集める)
[Step 4] candidate が promoted → official on-chain proposal
   ↓ (3-5 日の投票期間)
[Step 5] 採決
```

**費用感**:
- Step 1〜3: **無料**（Discord 投稿、discourse 投稿、nouns.camp candidate 提出）
- Step 4: sponsor 側の gas（Yuki 負担ではない）
- Step 5: 採決後の treasury transfer は Yuki 不要

---

## Step 1: Discord #ideas 投稿

**前準備**:
1. Nouns DAO Discord に参加: https://discord.gg/nouns
2. 自己紹介を `#introductions` で投稿
3. 数日眺めて、現役の議論の温度感を掴む

**投稿先**: `#ideas` または `#proposals-discussion`
（実際の channel 名は変動するので、参加時に確認）

**投稿内容**: `02_discord_short.md` を貼る。
{LINK TO CANDIDATE} の部分は、discourse 投稿後に埋める。

**ベストタイム**: UTC 14:00-22:00（北米と欧州が両方アクティブな時間帯）
JST だと **23:00-翌 07:00**。日本時間で動く場合は土曜夜が現実的。

**反応見るポイント**:
- メンション付き反応 / 質問 が 2-3 件付くか
- 「これは Nouns DAO のスコープ外じゃない？」が出るか
- 「fund はいらないなら別に DAO 通さなくてもいいのでは」（CC0 だから）と
  言われた場合の答え： 「DAO の正式 endorsement で treasury への送金フローを
  publicly committable にしたい」と返す

---

## Step 2: discourse.nouns.wtf にプレ提案スレッド

**手順**:
1. https://discourse.nouns.wtf にアカウント作成（ENS 連携可）
2. **NounsDAO Proposals → Pre-Proposal Discussion** カテゴリで新規スレッド
3. タイトル + 本文を `03_discourse_post.md` から貼る
4. 「Submitting a Proposal」（公式テンプレート）の体裁に寄せる

**期待される反応**:
- 1-2 週間で 5-15 件のコメントが付く（Nouns は活発）
- 質問内容で「気になっている論点」が判明する
- 反応ゼロの場合 → Discord で thread bumping すると見てくれる

**よくある追加質問への準備回答**:

> Q: Why not just operate under CC0 without a proposal?
A: Because routing 10% to the treasury *publicly* and *contractually* is
   more powerful than just doing it silently. The proposal turns this from a
   gesture into a commitment with on-chain audit trail.

> Q: What guarantees the 10% actually gets sent?
A: (i) MA settles in ETH on-chain, instant transfer is in the auction
   contract logic. (ii) MUGEN/MUON settle in JPY → I batch convert and
   transfer monthly, with a public dashboard. (iii) If I miss two
   consecutive months, DAO can revoke branding immediately.

> Q: Why 10% of *gross*, not net?
A: Gross is auditable from the on-chain certificate count × the listed
   price. Net requires trusting MU's accounting. I prefer gross.

> Q: What if MU pivots away from fashion?
A: × NOUNS branding ceases when MU's apparel pipeline ceases. Re-activation
   would require a new proposal.

> Q: Yuki who?
A: ex-CPO at Mercari (2014–2021), co-founded NOT A HOTEL (2018–2024), now
   CEO Enabler Inc. Public track record. Identity not anon.

---

## Step 3: nouns.camp に candidate proposal を起こす

**前提**:
- Discord/discourse の反応で「OK 出していい」と感じる温度になったら
- 出す前に discourse のフィードバックを 本文に反映 した最新版を準備

**手順**:
1. https://www.nouns.camp/ にウォレット（`yuki.eth`）で接続
2. "Create Proposal" → "Candidate" を選択
3. Title / Markdown body / Transactions の 3 欄を埋める
4. Transactions タブ:
   - For this "blessing" proposal, you can submit it with **0 transactions**
     (purely informational / off-chain commitments)
   - Or add a 1 ETH symbolic transfer to/from MU's address to demonstrate the
     pipeline (not required)
5. Preview → Submit Candidate
6. **無料** (gasless via nouns.camp)

**結果**:
- Candidate URL が発行される: `nouns.camp/candidates/...`
- Signature 集めフェーズ開始
- 3 Nouns 所有者の sponsor で official proposal に promote 可能

---

## Step 4: Sponsor を集める

**現実**:
- Nouns 1 枚の floor は ~30 ETH（変動激しい、要確認）
- Sponsor になることに Nouner 側コストはほぼゼロ（gas くらい）
- 3 Nouns 集まれば promotion 可能

**働きかけ先（一般論）**:
- Discord で interest を示した人に直接お願い
- Twitter で Nouners を tag して「sponsor 募集中」を投稿
- discourse のコメント欄で sponsor 募集を明示
- Nouns Center や Nouns Camp の active proposers と接続

**集まらない場合**:
- Candidate のまま 30 日寝かせて再 promotion 試行
- フィードバックを反映してリライト → 再提出
- 諦めて CC0 で自走（Plan B）

---

## Step 5: On-chain proposal & 採決

**Sponsor 集まったら**:
1. Sponsor が candidate を on-chain proposal に promote（gas は sponsor 負担）
2. 投票期間: 3-5 日（Nouns の投票期間設定に従う）
3. Quorum を超え、過半数賛成で adopted
4. Treasury transfer は本提案では発生しない（ETH 要求 0 のため）

**採決後**:
- 1 ヶ月以内に最初の MUGEN × NOUNS drop
- 四半期ごとに公開ダッシュボード更新
- 12 ヶ月後に DAO に Year-1 Report

---

## 提出前チェックリスト

- [ ] **Yuki の ENS（yuki.eth）が実際に取れているか確認**（取れてない場合は別表記）
- [ ] **`yuki.hamada` Discord アカウント取得**（取れてない場合は別表記）
- [ ] **Twitter ハンドル確認**（@yuki_hamada の在不在）
- [ ] **wearmu.com/nouns / /nouns/today / /ma のページが用意できるか確認**
       （on-chain proposal 採決後 30 日で開設、と書いている）
- [ ] **MU の Solana cNFT 発行が動作しているか確認**（既に MA ラインで稼働、と
       press release に書いてある）
- [ ] **Mercari 在籍年（2014-2021）の正確性確認**
- [ ] **メルカリ「Japan's largest C2C marketplace」の表現に問題ないか確認**
- [ ] **「Director & CPO」の正式英語表記確認**（メルカリ取締役 CPO）
- [ ] **NOT A HOTEL の在籍期間（2018-2024）と「Co-founder, ex-board member」表記の確認**

---

## ベストタイミング

| イベント | JST | UTC | 推奨曜日 |
|---|---|---|---|
| Discord 投稿 | 土 23:00-翌 04:00 | 土 14:00-19:00 | 金〜土 |
| discourse 投稿 | 水〜木 朝 08:00 | 火〜水 23:00 | 平日中盤 |
| candidate 提出 | discourse 投稿後 7-14 日 | 同上 | 平日中盤 |

PR TIMES 配信や WIRED Japan / The Bridge 取材記事と時期を合わせると、
日本側のメディア露出 → Nouns コミュニティ視認 → sponsorship、の連動も
狙える。

---

## Plan B（採決で否決された場合）

- CC0 なので、`× NOUNS` 表記を使わなければ MU の通常パイプラインに NOUNS
  風グラフィックを混ぜるのは技術的に可能
- ただし、DAO に否決されたあとも勝手に NOUNS ブランディングを使うのは
  community 関係を悪化させる
- 否決時は branding を「Nouns-inspired」「⌐◨-◨ tribute」レベルに弱めて
  運用、treasury への自主送金は継続も可（ただし DAO 公認ではないので
  事実上 just a donation）

---

## 関連リンク

- Nouns DAO: https://nouns.wtf
- Nouns Camp (candidate submission): https://www.nouns.camp
- Discourse: https://discourse.nouns.wtf
- Treasury: https://etherscan.io/address/0x0BC3807Ec262cB779b38D65b38158acC3bfedE10
- Proposal flow docs: https://docs.publicnouns.wtf/governance/proposal-flow
- 既存 candidate 例: https://www.nouns.camp/candidates
