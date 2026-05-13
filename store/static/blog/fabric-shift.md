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

| あなたが | おすすめ | 理由 |
|---|---|---|
| MU を試しに 1 着 | era-2 MUGEN ¥7,800 | リブ襟 + EU organic、 価格カーブの根元 |
| 既に MU を 1 着持っている | era-2 MUON daily (¥7,800〜) | 自分のシャツの履歴と並ぶ別 era ピース |
| コレクター | era-2 MA ¥18,000〜 | 1-of-1、 auction で更に上がる |
| era-1 (Bella) を確保しておきたい | drop 1-147 の MUGEN | 今しか買えない、 fabric era アーカイブ |

---

*Constitution §11 (numbers over adjectives) に従って書きました。 原価公開は §11 の自社に対する適用。*
*次回更新: era-2 最初の 30 着が売れた日 (現在 0/30)。*
