# POD直接提携 一次接触メール下書き(送信可能版)

> ⚠ 送信は人間ゲート(外部・対外コミュニケーション)。本人承認後に gog で送信。
> 宛先・送信前に最低ロット/原価の希望条件を最終確認すること。

---

## ① Printio(株式会社OpenFactory)宛 — hello@printio.me

**件名:** 【提携のご相談】AIアパレル工房「MU」と Printio API の連携について

本文:

株式会社OpenFactory ご担当者様

突然のご連絡失礼いたします。株式会社イネブラ（代表取締役 濱田優貴）にて、
AIアパレル工房「MU」(https://wearmu.com) を運営しております。

MUは「言えば、作れる」をコンセプトに、ひとことの言葉からAIがデザインを生成し、
その場で販売できるサービスです。作り手には売上の10〜50%が印税として還元される
設計で、無在庫・1点からのオンデマンド生産を前提にしています。

現在は海外PODを利用していますが、①Made in Japan の品質・納期、②原価の最適化、
③事業者として工場と直接つながる体制、を目指し、Printio API での連携を検討して
います。つきましては以下をご相談させてください。

- Printio API の利用条件・接続方法（発注／製造ステータス／出荷追跡）
- 対応アイテムと、1点あたり原価・最低ロット・標準納期
- アパレル（Tシャツ／ラッシュガード等）の対応可否
- 事業提携（API組み込み・継続発注）の進め方

第一弾は柔術(BJJ)の道場公式ギアでの展開を予定しており、発注量は段階的に
拡大する見込みです。オンライン等でお打ち合わせいただけますと幸いです。

何卒よろしくお願いいたします。

────────────────
株式会社イネブラ（Enabler Inc.）
代表取締役 濱田優貴
mail@yukihamada.jp / https://wearmu.com

---

## ② Gelato 宛（英語・グローバル窓口 or Japan production 問い合わせ)

**Subject:** Partnership inquiry — AI-native POD "MU" × Gelato Japan production / API

Body:

Hi Gelato team,

I'm Yuki Hamada, founder of Enabler Inc. (株式会社イネブラ), running **MU**
(https://wearmu.com), an AI-native apparel house. With MU, anyone creates a
product just by describing it in one line — AI generates the design and it's
instantly for sale, with 10–50% royalties flowing back to the maker. We run
fully on-demand, no inventory, one unit at a time.

We want to move core production to **Japan-local printing** for faster delivery,
better margins, and a "Made in Japan" story, and we'd like to integrate Gelato's
**Order Flow API** for fulfillment (we already have a `gelato_jp` route stubbed
in our system).

Could we discuss:
- API onboarding + credentials for **production in Japan**
- Apparel catalog (tees, hoodies, rashguards), **per-unit cost, lead times**
- Partnership / volume terms

Our first vertical is **Brazilian Jiu-Jitsu dojo team gear**, scaling volume
over time. Happy to jump on a call.

Thanks!

Yuki Hamada — Enabler Inc.
mail@yukihamada.jp / https://wearmu.com

---

## 送信手順(承認後)
```bash
# Printio
gog gmail send --account mail@yukihamada.jp \
  --to "hello@printio.me" --subject "【提携のご相談】AIアパレル工房「MU」と Printio API の連携について" \
  --body "<上記本文>"
# Gelato は問い合わせフォーム(gelato.com)経由の可能性 → フォーム送信は人間
```
