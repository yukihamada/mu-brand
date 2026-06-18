# 素材を変えた、 原価も公開する

2026-05-13 · yuki · MU (wearmu.com) · §24 / §25

---

## 何が変わったか

今日から、 MU のすべての新しい drop は **Stanley/Stella SATU001 Creator 2.0 Ribbed Neck T-Shirt** で作られる。

- 180gsm
- 100% organic combed ring-spun cotton (**GOTS** 認証)
- EU 製 (Portugal)
- **リブ襟** (crew neck の襟部分が織りリブになっている)
- piece-dyed black

これまでは Bella+Canvas 3001 (4.2 oz、 US 製、 generic crew neck) を使っていた。

切替の cutoff:

| Brand | era-1 (Bella) | era-2 (Stanley/Stella) |
|---|---:|---:|
| MUGEN | drops 1–147 | drop **148 onward** |
| MUON | drops 1–9 | drop **10 onward** |
| MA | drops 1–2 | drop **3 onward** |

**過去に買った方のシャツ仕様は変わりません** (Constitution §21、 purchase path sacrosanct)。 era-1 を持っている人は永久に era-1 の所有者。 /shirt/N/life ページで "era-1" or "era-2" のタグが永続表示される。

---

## なぜ変えたか

正直に書く。 1 週間で 7 着売れた段階で、 1 人の想定 buyer (`/persona.md` の Toru = 30 歳 港区 IT、 Visvim/Comme 着てる層) に聞いたら言われた:

> 「Bella+Canvas は 30 回洗濯で襟が伸びる。 Visvim Jumbo の 5 年勝負には負ける」

正しい指摘だった。 MU が売りたいのは **「服そのもの」 ではなく「気温と時刻」** だが、 服がヘタれば 「気温と時刻」 の話も信じてもらえない。 触った瞬間の質感が最低限の信頼。

Stanley/Stella SATU001 を選んだ理由:

1. **リブ襟** — Visvim / Comme 系列が標準で持つ「触って気付くディテール」。 Bella の generic 襟との差は数秒で分かる
2. **GOTS organic** — 認証コスト数十万円かけて取得する規格。 narrative cost ゼロでブランド cred 加算
3. **EU 製 (Portugal)** — 米国 fast-fashion の対極
4. **Printful EU center 在庫あり** — Constitution §2 (0 humans) 維持、 自動 fulfillment 継続

---

## 原価を公開する

**シャツ 1 着あたり (era-2 / Stanley/Stella SATU001)**:

| 内訳 | 金額 |
|---|---:|
| Blank (Stanley/Stella SATU001 black M) | **¥3,750** (Printful 仕入、 $25.00) |
| DTG 白インク前面 print | **¥750** ($5.00 推定) |
| EU → JP 配送 (USPS / Yamato) | **¥1,200** ($8.00 推定) |
| **合計 原価** | **¥5,700** |

(era-1 / Bella+Canvas の原価は ¥2,850 だった。 単純に **2 倍** になった)

**SUZURI 経路 (国内 Printstar 5.6oz) の場合:**

| 内訳 | 金額 |
|---|---:|
| SUZURI base (Printstar 00148-HVT black M + 国内 DTG + 国内配送) | **¥3,500** (SUZURI 側で完結) |
| MU creator margin | **¥1,400** |
| **販売価格** | **¥4,900** |

国内 SUZURI の場合、 在庫管理・印刷・配送はすべて SUZURI 側 (GMO ペパボ運営) で完結します。 wearmu.com は デザイン (texture) を API で送るだけ。 MU には 1 着売れるごとに ¥1,400 (creator margin) が入金されます。 原価という概念は MU には発生せず、 「印刷チャネル提供料」 ¥3,500 を SUZURI 側に支払う形。 これも Constitution §2 (0 humans) の継続のため、 倉庫も発送も触らない。

---

## 価格設計

| Brand | era-1 base | era-2 base | era-2 cap |
|---|---:|---:|---:|
| MUGEN | ¥5,000 | **¥7,800** | ¥35,000 |
| MUON | ¥5,000 | **¥7,800** | ¥30,000 |
| MA | ¥30,000 (auction start) | **¥18,000** (auction start) | §21 ceiling ¥100,000 |

Margin 計算 (era-2):

| 価格点 | margin (¥) | margin % |
|---:|---:|---:|
| ¥7,800 (MUGEN/MUON 開始) | ¥2,100 | 27% |
| ¥18,000 (MA 開始) | ¥12,300 | 68% |
| ¥35,000 (MUGEN cap) | ¥29,300 | 84% |

MA を値**下げ**したのは矛盾に見えるかも知れない。 理由:
- 原価が同じになった (MA も MUGEN も Stanley/Stella SATU001 ¥5,700)
- ¥30,000 starting bid は cost に対して 5.3 倍、 1-of-1 として scarcity premium は 取りすぎ
- Visvim Jumbo (¥18,000) と並ぶ価格点に下げて、 auction で「上がる」 余地を作る方が誠実

---

## SUZURI 経由 (国内発送) の場合は **生地が違います**

ここまで書いた Stanley/Stella SATU001 はすべて **wearmu.com → Stripe → Printful EU 経路** (海外発送・¥7,800〜) の話です。

国内のお客様向けに用意した **SUZURI 経由 ¥4,900 (2-3 日 国内発送)** は、 SUZURI 側のヘビーウェイト T シャツ (Printstar 00148-HVT) を使います。 Stanley/Stella SATU001 とは別物です。

| 経路 | 生地 | リブ襟 | 認証 | プリント | 配送 | 価格 |
|---|---|---|---|---|---|---|
| **SUZURI** (JP) | Printstar 00148-HVT 5.6oz | なし (generic crew) | なし | 国内 DTG | 2-3 日 | **¥4,900** |
| **wearmu.com Stripe** (海外 / collectors) | Stanley/Stella SATU001 180gsm | **あり** (織りリブ) | **GOTS** organic | Printful EU DTG | 1-2 週 | ¥7,800 |

正直に言うと、 SUZURI の Printstar は普通のしっかりした 5.6oz ヘビーT で、 触った瞬間に分かる「Visvim 級のリブ襟ディテール」 はありません。 ただ:

- 国内 2-3 日着、 ¥4,900 (Stripe + EU 経路の **63%**)
- 「お客様 が気楽に試す MUGEN」 として正しい価格点
- 同じデザイン、 同じ気温 seed、 同じ 1-of-108 ナンバリング
- DTG プリント、 ホワイトインクのみ (§25 デザイン directive 準拠)

**Stanley/Stella SATU001 のリブ襟が必要な方は wearmu.com の Stripe 購入を選んでください。** 海外発送扱いになりますが (国内へも届きます)、 生地は EU organic + リブ襟 + GOTS 認証 の正規です。 SUZURI 経路は「気軽に MU を着てみる」ためのチャネル、 Stripe 経路は「Stanley/Stella 正規」 と区別しています。

§24-v2 として Constitution に明文化されています (国内 SUZURI / 海外 Stanley/Stella の dual-channel)。

---

## デザインも変わる (§25)

同時に、 デザイン directive を Constitution §25 として明文化した:

- 前面の vector shape は **1–3 個まで**
- 70% は **negative space** (シャツの空白そのものが設計)
- **単線質** (sumi-brush integrity、 装飾なし)
- 白インクのみ、 透過 PNG 背景
- 使用モチーフ: **間 / ━◯━ / weather glyphs / 気温の数字** のみ

m5 generate.py の prompt directive を更新済み。 era-2 の MUGEN は era-1 より明らかに **減算された** デザインになる。

---

## 数字 (公開分のみ)

| 指標 | 値 (2026-05-13 23:00 JST 時点) |
|---|---:|
| 累計売れた数 | 7 着 (Bella era) |
| 累計売上 | **¥62,800** (gross、 /transparency 公開) |
| era-2 で必要な月次売上 (§Cessation 閾値) | ¥30,000 純益 |
| era-2 ¥7,800 で 月 30 着なら | 純益 **¥63,000** (2× 閾値) |

正味の利益は **30 着/月 がライン**。 これが越えられない場合は §Cessation に基づき shut down する。 数字で判断する。

---

## 何をしたい人にどれをすすめるか

| あなたが | おすすめ | 経路 | 理由 |
|---|---|---|---|
| 国内のお客様で気軽に試したい | **SUZURI 経由 ¥4,900** | suzuri.jp | Printstar 5.6oz、 国内 2-3 日着、 価格カーブ的にも一番ライト |
| Stanley/Stella リブ襟が欲しい | era-2 MUGEN **¥7,800** | wearmu.com Stripe (Printful EU) | リブ襟 + GOTS organic + EU 製、 「触って気付く」 ディテール込み |
| 既に MU を 1 着持っている | era-2 MUON daily (¥7,800〜) | wearmu.com Stripe | 自分のシャツの履歴と並ぶ別 era ピース |
| コレクター | era-2 MA ¥18,000〜 | wearmu.com Stripe | 1-of-1、 auction で更に上がる、 Stanley/Stella 正規 |
| era-1 (Bella) を確保しておきたい | drop 1-147 の MUGEN | wearmu.com Stripe | 今しか買えない、 fabric era アーカイブ |

---

*Constitution §11 (numbers over adjectives) に従って書きました。 原価公開は §11 の自社に対する適用。*
*次回更新: era-2 最初の 30 着が売れた日 (現在 0/30)。*
