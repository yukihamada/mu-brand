# MUON / MA のクオリティを上げるなら何がいい？

2026-05-14 · yuki · MU (wearmu.com) · §24-v3 検討

---

§24 で MUGEN / MUON / MA を **Stanley/Stella SATU001 (180gsm, GOTS organic, EU 製、 リブ襟)** に揃えた。 これは Bella+Canvas からの 1 段アップで、 多くのお客様にとっては十分な品質だと思っている。

ただし、 1-of-1 で **¥18,000〜** から始まる **MA** や、 気温連動の **MUON** は MUGEN とは買い方も気持ちも違うので、 「もう 1 段上のクオリティ」 を用意したほうが正直 (Constitution §11 — 数字とお客様の正直な反応)。

MA は 1 着 1 着が「その週 1 週間で 1 着だけ」 のアイテム。 MUON は気温が枚数を決めるので入手難易度が日によって違う。 この 2 ブランドだけ、 MUGEN とは違う 「**仕立てそのものを上げた**」 版を作るとしたら、 何が一番効くか。

§2 (0 humans = 全工程 API 自動化) を可能な限り維持しつつ、 6 案を出した。 **どれが一番欲しいかお客様に聞かせてほしい**。

---

## A. 生地を 250gsm に上げる

**Stanley/Stella STTU788 The Heavy Creator** に切替。 同じ GOTS organic + EU 製のままだが、 180gsm → **250gsm**。 厚み・落ち感・透けにくさ がほぼ別物になる。 Visvim Jumbo に近づく。

- 原価: ¥3,750 → 約 **¥4,500** (+¥750)
- §2 互換: ◎ (Printful 在庫あり、 そのまま API 切替で済む)
- リスク: 夏場の通気性が落ちる

## B. MA 専用 internal label + hang tag

シャツ自体は同じ SATU001 のまま、 MA だけに **内側 woven label** ( "MA · 1-of-1 · {date} · {気温}℃") と **hang tag** (Constitution §17 抜粋 + シリアル番号) を付ける。 触る場所 (襟内側) が変わると、 値札以上に 「これは違う」 と一目で分かる。

- 原価: ¥3,750 → 約 **¥4,050** (+¥300)
- §2 互換: ◎ (Printful の custom label 機能で可)
- リスク: ほぼなし

## C. 加工を変える: garment-dyed (色落ち系)

**Comfort Colors 1717** などの pigment-dyed 生地に切替。 経年で色が抜けて 「自分のシャツ」 になる、 Visvim/Sunspel 系の質感。 ただし GOTS / EU 製ではなくなる (US 製コットン)。 §24 で重視した認証 narrative とトレードオフ。

- 原価: ¥3,750 → 約 **¥2,400** (-¥1,350、 生地は実は下がる)
- §2 互換: ◎ (Printful 在庫あり)
- リスク: 「EU organic」narrative が薄れる。 ただ別軸の物語 (経年変化) が立つ

## D. プリント手法を DTG → screen-print

現状の DTG (デジタル印刷) は摩耗で 30-50 回洗濯後に薄くなる。 **silk screen print** に切替えれば 100 回以上もつ。 ただし Printful の screen-print 対応 SKU は限定的なので、 生地の選択肢が狭まる。

- 原価: +¥400/着 (screen setup 込み)
- §2 互換: △ (対応生地次第)
- リスク: 「Stanley/Stella SATU001 + screen-print」 の組合せが Printful にあるか要確認

## E. MA 限定: 国内 loopwheel (§2 例外)

和歌山の loopwheel 工場 (Goodwear/Whitesville レベル) で 1 着ずつ縫う。 **1着 ¥7,000-9,000 blank**。 これは Visvim / Whitesville と同じ製法。 ただし **完全自動化は無理** — 縫製は人間が動く。 Constitution §2 を 「MA に限り例外」 と緩和する必要がある。

- 原価: ¥3,750 → 約 **¥9,000** (+¥5,250)
- §2 互換: ✗ (例外条項が必要)
- リスク: §2 が穴あく、 100 年計画の自動化軸が揺らぐ
- 想定 retail: ¥35,000-50,000 開始 bid

## F. 現状維持 (Stanley/Stella SATU001 で十分)

「これ以上は気にしすぎ。 Stanley/Stella SATU001 (180gsm + GOTS + リブ襟 + EU) で MUON / MA の品質は完成形。 デザインと narrative に集中しろ」 という選択肢。 これも立派な答え。

- 原価変動なし
- §2 互換: ◎
- リスク: なし

---

## アンケートに参加

**1 つ選んで投票してください** → <https://wearmu.com/survey/quality>

このアンケートは:
- 公開 (誰でも投票結果を見られる、 /survey/quality)
- IP ベース重複防止 (1 票/IP)
- 結果は Constitution §23 の DAO 投票ではなく**お客様向け非拘束アンケート** — 最終決定は yuki が結果 + コスト + §2 整合性を見て決める
- 集計 50 票超えた段階で 「§24-v3 fabric upgrade」 を実装するかどうか決断する

---

## ちなみに、 現状判断

書いてみて、 個人的にはこう考えている:

| 案 | 個人意見 |
|---|---|
| A (250gsm) | やる価値あり。 +¥750 原価で「明確に重い」 が手に入る |
| B (内側 label + hang tag) | やる価値大。 触る場所が変わるのは効く |
| C (garment-dyed) | 別ライン (「MUYU」 — 経年系) として独立させた方が綺麗 |
| D (screen-print) | Printful の組合せ次第。 調査必要 |
| E (国内 loopwheel) | §2 を MA 例外で緩めるか、 別ブランドとして立てるか |
| F (現状維持) | 30 着/月の閾値 (§Cessation) を越えてから検討、 でもいいかも |

ただこれは **僕の意見であって、 お客様の意見ではない**。 アンケート結果を待つ。

---

*Constitution §11 (お客様に対する正直 + 数字) に従って書きました。 §2 (0 humans) と §24 (fabric) のトレードオフを明示しました。*
*次回更新: アンケート 50 票超え時点 / または 2026-06-01。*
