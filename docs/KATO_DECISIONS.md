# 加藤 健 FB Decision Doc — 戦略 / personal layer

**Status:** Draft (2026-05-21) — Yuki が 5/31 までに 決断
**Trigger:** 元メルカリ Yuki 直属上司 (5人目 ペルソナ) からの 戦略 FB 10項目。
他 4ペルソナ (田中/美咲/翔/Alex) と は 完全 別レイヤー — 「ページ修正」
ではなく **「事業 を やる か やらない か」** レベル。

## 0. ペルソナ FB の 位置づけ

ペルソナ #1-4 (田中 #11 / 美咲 #12 / 翔 #12 / Alex #12) = product surface
の 修正 47項目、 24h で 32 即修正、 15 設計 落とし込み 済。 これ は
**operational**。

ペルソナ #5 (加藤) = **strategic**。 product surface の 修正 で は
解け ない。 Yuki の 個人 決定 マター 10項目。 本 doc は その 棚卸し。

## 1. 加藤 推奨 [1] — 100枚 challenge を 5/31 まで 走らせ、 MU の市場 判定

**Status:** 既に 進行 中 (5/18-5/31)。 残 10日。

**Decision required by 2026-05-31:** 100枚 達成率 で MU への 追加 投下 を 決める:

| 達成率 | 判定 | 翌月 アクション |
|---|---|---|
| ≥ 70/100 | MU の 市場 仮説 成立 | MU に **倍 投下**, Phase 2 trio DL ship, charter outreach 加速 |
| 30-69/100 | 仮説 部分成立 | MU を **maintenance**, JiuFlow か SOLUNA に 注力 切替 |
| < 30/100 | 仮説 不成立 | MU を **freeze**, MSA は 既buyer サポート のみ |

**Data source:** /transparency 「100枚 challenge」 セクション (今 PR で 追加)。

**注意:** Yuki が dogfood で買って 数字 を 膨らます 誘惑 を 排除 する ため、
/transparency の `external` revenue (yuki dogfood 除外) を 真の 判定 基準
とする。

## 2. 加藤 推奨 [2] — 70%+ / 50% 以下 で MU 倍投下 vs maintenance

[1] と 同じ。 上の 表 を 5/31 結果 で 機械的 に 適用。

「迷ったら 倍投下」 「迷ったら maintenance」 の どちらか を 事前に 決めて
おく ことで、 結果 を 見て から 後付け で 解釈する バイアス を 防ぐ。

**Pre-commit:** 50/100 (= 50%) を **明確 な judgment line** とする。
これ より 上 = MU 継続。 下 = MU maintenance 化。

## 3. 加藤 推奨 [3] — 14 product を 「3 active + 11 maintenance」 に 整理

**Decision required:** どれ を active 3 に 残す か。 候補:

| product | 既存 MRR | 加藤 視点 | active 候補? |
|---|---|---|---|
| JiuFlow | ¥180k/mo | 「既に 動いて る 事業、 投下 不足」 | **強く 推奨** |
| stayflowapp (StayFlow) | Stripe Live (具体額 要 確認) | 「動いて る、 ¥7,900/mo Pro tier 真の事業」 | **推奨** |
| MU / wearmu | ¥0 (¥X/mo, 100枚 challenge次第) | 「100枚 結果 が active or maintenance を 決める」 | **条件付** |
| chatweb.ai (nanobot) | 累積 ENAI / 個別 課金 | 「Lambda 削除済、 維持コスト 低」 | maintenance OK |
| SOLUNA / TAPKOP | East Ventures 出資先、 売上 まだ | 「投資家 期待、 個人時間 投下 不足」 | **active 必須** |
| Koe Device | 売上 まだ | 「ハードウェア、 長期 賭け」 | maintenance 候補 |
| nakamura兄弟 UFC | 売上 まだ | 「pause 中、 立石さん と 再起 判断」 | **decision pending** |
| その他 11 (MSA Tier 1) | ほぼ ¥0 | 「private MSA で 維持、 新機能 ゼロ」 | maintenance |

**推奨 active 3 (5/31 後):**
- (A) JiuFlow (revenue 中心)
- (B) SOLUNA / TAPKOP (投資家 義務 + 長期 賭け)
- (C) **MU か nakamura か どちらか 1つ** (5/31 100枚結果 + 立石 確認 後 決定)

**残 11 は maintenance** = security patch + 既存顧客 サポート + Dependabot
auto-merge のみ。 **新機能 ゼロ**、 **新コンテンツ ゼロ**、 **ブログ 言及 月1 まで**。

**Implementation:** /transparency の Fleet status section (今 PR で 追加)
に honest grade を 月初 に 更新。 「active 全部」 振り しない。

## 4. 加藤 推奨 [4] — East Ventures に 月1 進捗 ブリーフィング 開始

**Status:** 未着手 (推定)。 East Ventures (¥5,000万 出資、 優先株 5%、
時価10億) は SOLUNA / TAPKOP 向け の 投資 と 認識。

**Decision required:** 月1 1ページ メモ を 投資家 LP に 送る:

```markdown
# SOLUNA Progress · 2026-05

## Cap table usage
- 入金額: ¥X 残
- 月次 burn: ¥Y
- runway: Z ヶ月

## SOLUNA 直接 進捗
- 建材 SIPs: 鈴工 さん 交渉 [step / next milestone]
- 弟子屈町 拠点: [land / building status]
- 製造 partner: [signed / negotiating / ?]

## Adjacent activity (非 SOLUNA だが founder 時間 を 使う もの)
- JiuFlow: ¥180k MRR, 161 active subs
- MU / wearmu: 100枚 challenge 5/18-5/31, 結果 X/100
- nakamura兄弟 UFC: pause / 再起判断 中
- Koe Device, chatweb.ai 等: maintenance

## 次月 commit
- SOLUNA に: A, B, C
- 他: D, E (時間 配分 X%)

## risk
- 1人 founder 集中 vs 拡散 (加藤 指摘)
- nakamura pause の 機会損失
```

**Frequency:** 月1、 月初 5日 以内 に LP 全員 に メール。 board meeting は
半年 1回 で 良い (法人 cap table size 的に)。

**初回 送信 target:** 2026-06-05 (5/31 100枚 結果 後)。

## 5. 加藤 推奨 [5] — friend audit (元メルカリ 5名 LINE)

**Status:** Yuki 個人 アクション。 本 doc は backup として 記録 のみ。

**Decision required:** 元メルカリ で 半年 連絡 してない 5名 を リスト アップ し、 LINE で 「最近 どう」 と 1人 ずつ 送る。 1日 1人 × 5日 で 完了。

「ペルソナ FB を AI に書かせる」 は 友達 不在 の 代用 で あって、 リアル
friend audit の 代替 では ない (加藤 #4 指摘)。 charter member outreach
(MSA_CHARTER_OUTREACH.md) の 隣 に 並行 で 進める。

**候補** (Yuki が 名前 を 自分 で 書き込む — この doc は 公開 GitHub に
入る ので 第三者 の 名前 は 載せない):

```
1. [元メルカリ 同僚 1]
2. [元メルカリ 同僚 2]
3. [元メルカリ 同僚 3]
4. [元メルカリ 同僚 4]
5. [元メルカリ 同僚 5]
```

完了 期限: 2026-05-26 (5日後)。

## 6. その他 加藤 指摘 — 即時 対応 可能

加藤 #6 (East Ventures cap table 説明) → [4] と 同じ、 月1 ブリーフィング で 解消。

加藤 #7 (nakamura兄弟 UFC pause) → 立石 さん に 5/22 までに 確認連絡 (個別 LINE で 1通)。 続ける か 解散 か を 1ヶ月 以内 に 決断。

加藤 #8 (累計売上 数字 公開) → 今 PR で /transparency に live 数字 強化 済 (100枚 challenge + Fleet status)。

加藤 #9 (100枚 challenge 進捗 透明化) → 同上、 /transparency に sold count 直接 表示 (server-side render、 毎リクエスト 再計算)。

加藤 #10 (家族 / personal health) → personal、 doc 化 しない。 Yuki が
自問 する のみ。

## 7. 5/31 判定 ミーティング (Yuki 1人)

5/31 23:59 JST 時点 で:

1. /transparency を 開く
2. 100枚 challenge sold count を 確認
3. 上の §2 表 で active 判定
4. KATO_DECISIONS.md §3 に 戻り、 active 3 を 確定
5. East Ventures LP ブリーフィング メール (§4 template) を 6/05 までに 送信
6. nakamura兄弟 UFC の 立石 さん 確認結果 を §3 表 に 反映 (active 候補 か 廃案 か)
7. 結果 + 決定 を blog 1本 で 公開 (「100枚 challenge 結果 と 次の 集中」)

期日 = 2026-06-01 朝。

## 8. 加藤 への 返信

加藤 さん へ の 返信 は **5/31 結果 後** に LINE 1通:

```
加藤さん、 ご指摘ありがとうございます。
全部 受け止めて、 docs/KATO_DECISIONS.md に decision doc 化しました。
100枚 challenge は X/100 で 終わりました。 §2 の判定 を 機械的に 適用、
active 3 は 以下 に 確定:

  (A) JiuFlow
  (B) SOLUNA / TAPKOP
  (C) [MU or nakamura, decision]

残 11 は maintenance に 落とします。 East Ventures LP には 6/05 までに
月1 報告 を 開始。 元メルカリ 5名 LINE は 5/26 までに 完了予定。

ペルソナ FB の AI 代用 指摘、 痛い ところ 突かれました。 リアル に
切替えます。

— Yuki
```

このメッセージ を 5/31 23:59 後 に LINE で 送る。 結果 が 70% でも 30%
でも 同じ template で、 数字 と 判定 だけ 入れ替え。

## 9. やらない こと

- 加藤 への 返信 を 5/31 前 に 急いで 送る (=「結果 が 出る 前 に 言い訳」 に なる)
- 100枚 challenge 結果 を hide / cherry-pick (=「数字 で 書く」 §11 違反)
- 「14本 並列 でも 大丈夫」 と 反論 する blog (= 客観 数字 で 既に 否定 されてる)
- active 3 を 4 や 5 に 拡張 する (= 加藤 指摘 の 「集中 が 利かない」 病状)
