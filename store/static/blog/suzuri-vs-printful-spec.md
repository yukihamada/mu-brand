# SUZURI と Printful、 同じ MUGEN でも届くシャツが違う件

2026-05-14 · yuki · MU (wearmu.com) · §24-v2 透明性

---

MUGEN を 1 着買おうとして気付いた方が居るかも知れない: **同じデザイン・同じシリアル番号でも、 経路によって届くシャツの仕様が違う**。

僕が分かりにくく書いたら詐欺になる。 だから 1 回、 全部の仕様を並べて書く。

---

## 並べてみる

| 項目 | 🇯🇵 SUZURI 経由 | 🌍 wearmu.com Stripe 経由 |
|---|---|---|
| **販売価格 (M)** | **¥4,900** 固定 | **¥7,800** から bonding curve で上昇 |
| **blank** | Printstar 00148-HVT | Stanley/Stella SATU001 Creator 2.0 Ribbed Neck |
| **重さ** | 5.6oz (≈ 190gsm) | 180gsm (Stanley/Stella の中量級) |
| **コットン** | 通常コットン (100%) | **GOTS organic** 100% combed ring-spun |
| **襟** | 普通の crew neck (シングル) | **織りリブ襟** (襟部分が独立した rib knit) |
| **染色** | piece-dyed black | piece-dyed black |
| **製造国** | 日本 SUZURI ネットワーク | **EU (Portugal)** 製造、 EU 縫製 |
| **プリント** | DTG (国内、 SUZURI 内製) | DTG (Printful EU center、 白インクのみ) |
| **配送元** | 日本国内 (GMO ペパボ) | EU → JP (USPS / DHL → ヤマト) |
| **配送日数 (JP 着)** | **2-3 日** | 1-2 週間 (税関を経由) |
| **シリアル番号 PWA tag** | あり | あり (どちらも moon-phase marker 入り) |
| **NFT 兄弟リンク** | あり (`/c/:id/:pos`) | あり |
| **DAO 投票権** | あり (1 着 = 1 voting weight) | あり |
| **/transparency 記録** | あり (SUZURI 売上は SUZURI API 集計) | あり (Stripe 売上は Stripe API 集計) |

---

## 何が同じで、 何が違うか

### 同じ部分

- **デザイン**: 完全に同じ PNG (北海道弟子屈町の気温 seed → Gemini 3 Pro Image)
- **シリアル番号**: drop 149 なら両方とも "MUGEN #0149"
- **moon-phase marker**: 内側襟下 8 円位置の白円配列 PWA tag (`/scan` でスキャン可)
- **コミュニティ**: どちらの経路で買っても DAO 投票権・兄弟リンク・/transparency 記録は同じ
- **Constitution への帰属**: お客様としての権利 (§21 purchase path sacrosanct 含む) はどちらも同じ

### 違う部分

- **生地**: SUZURI = Printstar (日本の標準ヘビーT)、 Stripe = Stanley/Stella (EU organic + リブ襟)
- **触感**: リブ襟があるかないかは触れば 1 秒で分かる。 Visvim / Comme を着る層は触る前から目で分かる
- **耐久性想定**: どちらも DTG 印刷なので、 洗濯 30-50 回でプリントは薄くなる。 生地のヘタりは Stanley/Stella の方が緩やか (organic + 厚手)
- **物語**: SUZURI は「気軽に試す」 物語。 Stripe は「Stanley/Stella SATU001 を着る」 物語

---

## なぜ 2 経路にしたか

**Stanley/Stella SATU001 を国内のお客様にも届けたい**。 でも Stripe + Printful EU 経由だと:

- 配送 1-2 週間 (税関の遅延込み)
- 配送料込みで原価 ¥5,700、 retail ¥7,800 が下限
- 国内で 5 日後に欲しい人には合わない

**SUZURI なら国内 2-3 日 + ¥4,900**。 ただし生地は Printstar に下がる。

「ブランドは 1 つの仕様で統一すべき」 という伝統的な常識からは外れるけど、 MU は **デザイン + ブランド体験は 1 つで、 仕立ては 2 種類** という形にした。 同じ商品名で違う仕様を売るのは透明性を上げないと詐欺になる。 だから 「どっち選んでも MU ですが、 触ったとき違うものが届きます」 を明示する。

商品ページのモーダルにも書いてある (購入する前に必ず表示される):

> 🇯🇵 SUZURI 経路の生地: Printstar 00148-HVT 5.6oz (国内・generic crew、 リブ襟 / GOTS 認証なし)。 Stanley/Stella SATU001 (180gsm / 織りリブ襟 / GOTS organic / EU 製) が欲しい方は海外 Stripe 経路を選んでください。

---

## どっちが MU の「正規」か

正直に言うと、 **Stanley/Stella SATU001 + Stripe が「ブランド ステートメント」 寄り**で、 **SUZURI Printstar はアクセス層**。

ただ、 「MU のシャツを着ている」という事実は同じだし、 シリアル番号も moon-phase marker も DAO 投票権も同じ。 一方が「本物で」、 もう一方が「偽物」、 ではない。

価格カーブで見ても、 ¥4,900 (SUZURI) / ¥7,800 (Stripe entry) / ¥18,000 (MA auction start) / ¥35,000 (MUGEN cap) と段差を作って、 「気軽に試す → 触感込みで欲しい → 1-of-1 が欲しい」 のグラデーションに対応している。

---

## ¥4,900 と ¥7,800 の差 ¥2,900 は何の差か

おおざっぱには:

- **¥1,500**: Stanley/Stella と Printstar の blank 原価差 (SATU001 ¥3,750 vs Printstar 換算 ¥2,250 程度)
- **¥800**: EU → JP の国際配送料 (Printful 経由、 USPS + ヤマト)
- **¥400**: GOTS 認証 + リブ襟 + EU 製 の narrative premium
- **¥200**: Stripe 手数料差 (Stripe 3.6% vs SUZURI 手数料は SUZURI 側で吸収)

¥2,900 を払うかどうかは **触感 + narrative の価値判断**。 「触ってリブ襟が分かる」 「EU 製で GOTS が付いている」 「Stanley/Stella を着ている」 の 3 つに ¥2,900 を払う価値があるか、 それはお客様が決める。

---

## どちらを選んでも

- 同じ Constitution の下で動いている
- 同じ気温・同じ日に生まれた MUGEN
- 同じ DAO 投票権
- 同じ moon-phase marker で /scan できる
- 同じ /transparency の数字に記録される
- 同じ 100 年計画 (Constitution §22) の一部になる

違うのは **触感と物語**。 そして、 それを正直に書くのが §11 (数字と正直さ) だと思っている。

---

*Constitution §11 (numbers + 正直 over adjectives) / §24-v2 (dual-channel fulfillment) に従って書きました。*
*関連記事: [素材を変えた、 原価も公開する](/blog/fabric-shift)*
*次回更新: 国内 SUZURI 売上が累計 30 着到達日 (現在 0/30、 7 着は era-1 Bella で売れた分)。*
