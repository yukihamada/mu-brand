# 今の MU の T シャツ仕様 + デザインプロンプト全公開

2026-05-14 · yuki · MU (wearmu.com) · §24 / §25 透明性

---

「中で何が動いているか分からないブランド」 を作りたくない。 だから 今の T シャツの仕様と、 デザインを生成する prompt を、 全部出します。

---

## 1. T シャツ仕様 (era-2、 drop 148 以降)

Bella+Canvas 3001 (era-1、 drop 1-147) から Stanley/Stella SATU001 に切替えました (2026-05-13、 Constitution §24)。

| 項目 | era-2 (現在) |
|---|---|
| **blank** | Stanley/Stella SATU001 Creator 2.0 Ribbed Neck T-Shirt |
| **重さ** | 180gsm |
| **コットン** | 100% combed ring-spun cotton, **GOTS organic 認証** |
| **襟** | **織りリブ襟** (襟部分が独立した rib knit) |
| **染色** | piece-dyed black |
| **製造国** | **EU (Portugal)** |
| **プリント** | DTG (Direct-to-Garment)、 **白インクのみ** |
| **fulfillment** | Printful EU center → JP (海外 / collectors 経路) |

国内のお客様向けには SUZURI 経由 Printstar 00148-HVT 5.6oz の別経路 (¥4,900) もあります — 仕様差は [前回の記事](/blog/suzuri-vs-printful-spec) に並べました。

---

## 2. 切替の cutoff (Constitution §21 — 過去の購入は不変)

| Brand | era-1 (Bella+Canvas) | era-2 (Stanley/Stella) |
|---|---:|---:|
| MUGEN | drops 1–147 | drop **148 以降** |
| MUON | drops 1–9 | drop **10 以降** |
| MA | drops 1–2 | drop **3 以降** |

era-1 を持っている方は永久に era-1 オーナー。 `/shirt/N/life` ページに era タグが永続表示されます。

---

## 3. デザインプロンプト (Gemini 3 Pro Image を呼ぶ生 prompt)

毎時 / 毎日 / 毎週、 MU は北海道弟子屈町の **気温・湿度・風向・天気** を seed にして `gemini-3-pro-image-preview` でデザインを生成します。 ブランド別に prompt が異なります。

### MUGEN — 1 時間に 1 着 (108 枚 cycle)

`generate.py` line 780-795:

```python
prompt = f"""
FLAT PRINT ARTWORK. Bold graphic on solid background. THIS IS A 2D GRAPHIC
DESIGN — NOT A PHOTO OF CLOTHING. No t-shirt shape. No clothing. No garment
silhouette. No fabric. No model. No product photo. Flat graphic only,
like a concert poster or album cover.

Brand: MUGEN (無限) — drop #{drop_num}, cycle {cycle_num}/108.
{quantity} pieces only.
Timestamp: {now.strftime('%Y.%m.%d %H:00')} JST
Today: {weather['temp_c']}°C, {weather['condition']}, {weather['wind_dir']} wind

Design direction: {direction}   # 6 候補からランダム選択 ↓

Execution:
- Bold typography or geometric graphic. Readable from 5 meters.
- Black on white OR white on black — solid, flat background filling the entire canvas.
- Must include: "{now.strftime('%Y.%m.%d')}" and "{cycle_num}/108" in the composition.
- No gradients. No shadows. No 3D. No clothing outline. Flat art only.
- OUTPUT: 2400×3200px flat digital artwork, solid background, screen-print ready.
"""
```

**direction** は 6 候補からランダム選択:

1. `Time document: {その時刻の mood (深夜/早朝/夕暮れ etc.)}`
2. `Japanese concept: {侘び寂び / 物の哀れ / 一期一会 / 木漏れ日 / 余白 / 間合い からランダム}`
3. `Data poetry: temperature {温度}°C wind from {方角} at {風速}km/h — these numbers as graphic composition`
4. `Bold kanji: single character full-chest, meaning chosen for drop #{cycle_num}`
5. `Garment contract: THIS IS #{cycle_num}. MADE {YYYY.MM.DD} {HH}:00. NEVER AGAIN.`
6. `Number study: {cycle_num} — its shape, weight, and meaning as the entire design`

108 枚 cycle の最後 (drop #108 = "chapter end") は特別な prompt:

```python
prompt = f"""FLAT PRINT ARTWORK. THIS IS MUGEN #108 — THE CHAPTER END.
One piece. Never again in this exact form. The design must feel like a
conclusion: a circle closing, a count reaching zero, a final mark. Bold.
Definitive. Include '108' prominently. Include the full date
{now.strftime('%Y.%m.%d')}. Black on white or white on black. Flat 2D,
2400×3200px, screen-print ready."""
```

---

### MUON — 1 日に N 着 (N = 気温連動)

気温 ≤ 0℃ なら 「ICE Edition」 (1-3 着)、 それ以外は `quantity = max(1, abs(temp))`。

`generate.py` line 659-675:

```python
prompt = f"""
FLAT PRINT ARTWORK. White graphic elements on pure black (#000000)
background. THIS IS PURELY A 2D GRAPHIC — NOT A PHOTO OF A T-SHIRT.
No t-shirt. No clothing. No garment silhouette. No fabric. No model.
No product photo. Flat graphic only, like a poster or vinyl record sleeve.

Brand: MUON (無音) — silence recorded.
Date: {today.isoformat()} / {temp}°C, {weather['humidity']}% humidity, {weather['condition']}
Quantity today: {quantity} pieces
Design concept: {concept}   # 10 候補からその日の日付 mod 10 で選択

Execution:
- Pure 2D graphic composition. Imagine a poster, not a photograph of clothing.
- White marks/lines/numbers on solid black rectangle filling the entire canvas.
- Clinical and minimal — documentary, not decorative.
- Composition centered, compact, fits within a 12cm area.
- Tiny text: date {YYYY.MM.DD} and {temp}°C rendered as data annotation.
- ABSOLUTELY NO T-SHIRT SHAPE OR CLOTHING FORM. If you draw a garment you have failed.
- OUTPUT: 2400×3200px flat digital artwork, pure white-on-black graphic.
"""
```

**concept** 候補 10 件 (一部):

- An audio waveform that flatlines mid-graph — the exact moment sound becomes silence
- A mobile signal display with all bars absent — perfect no-reception
- A spectrogram showing only the noise floor — the frequency of nothing
- Concentric circles dissolving before reaching the canvas edge
- A single horizontal line, perfectly centered, 1px thick. Nothing else.
- Binary string: 00000000 — eight zeros. Silence encoded.
- A vinyl record's inner groove spiral — the locked groove, infinite silence
- 他

ICE Edition (温度 ≤ 0℃) 専用 prompt:

```python
prompt = """FLAT PRINT ARTWORK. ULTRA-RARE ICE EDITION — temperature hit
0°C or below. Pure white artwork on jet black background. The design must
feel frozen, crystalline, or glacial — not metaphorical but literally
cold: frost fractals, ice crystal geometry, frozen breath patterns, or
permafrost cracks rendered as graphic art. Stark white on black.
No clothing. No t-shirt. Flat 2D graphic, 2400×3200px."""
```

---

### MA — 月に 1 着 (1-of-1)

`generate.py` line 612-626:

```python
prompt = f"""
FLAT PRINT ARTWORK. Black ink on pure white background. No clothing.
No t-shirt. No garment shape. No model. No product photo. Just the graphic
artwork itself — as if it will be screen-printed.

Brand: MA (間) — ultra-premium Japanese fashion. MA means negative space.
Month: {now.strftime('%B %Y')} / Theme: "{theme}"   # 月別 12 テーマからランダム
Today: {weather['temp_c']}°C, {weather['condition']}, wind {weather['wind_dir']}

IMPORTANT — Generational DNA:
Previous month's design was at: {last_design_url}
Your design must carry ONE visual gene from it — a similar line weight,
a similar void ratio, or a similar spatial tension — but transformed.
Evolution, not repetition.

Design rules:
- ONE element only. Pure black ink on pure white background.
- Japanese sumi-e abstraction OR strict geometric reduction.
- Element occupies 20–30% of the canvas. Vast white void surrounds it.
- No text. No logo. No border. No t-shirt outline. No clothing silhouette.
- OUTPUT: flat artwork only, 2400×3200px, black on white, ready to screen-print.
"""
```

MA の特殊点: **前月のデザイン URL を渡して 「進化させろ」 と指示**。 生成 AI が世代間連続性を持つ。 12 ヶ月後、 元のテーマが残った状態で全然違う絵になる。

---

## 4. Constitution §25 — デザイン directive (5 月 13 日 追加)

era-2 切替と同時に、 §25 を追加: 「**減算されたデザイン**」 を明文化。

| ルール | 内容 |
|---|---|
| **vector shape** | 前面に **1-3 個まで** |
| **negative space** | **70% は空白** (シャツの空白そのものが設計) |
| **線質** | **単線質** (sumi-brush integrity、 装飾なし) |
| **色** | **白インクのみ**、 透過 PNG 背景 |
| **モチーフ** | **間 / ━◯━ / weather glyphs / 気温の数字** のみ |

era-2 の MUGEN は era-1 より明らかに「**減算された**」 デザインになります。 generate.py の prompt directive も §25 に合わせて更新済み。

---

## 5. 流れ (1 着が生まれるまで)

```
[気象 API] 北海道弟子屈町 (lat 43.49, lon 144.46)
   ↓ temp_c / humidity / wind / condition
[generate.py] prompt_mugen() / prompt_muon() / prompt_ma()
   ↓ weather + 候補 direction から prompt 組み立て
[Gemini 3 Pro Image] gemini-3-pro-image-preview
   ↓ PNG (2400×3200)
[crop_transparent_borders + moon-phase marker 合成]
   ↓ design.png (透明背景)
[Printful API] sync_product 作成 (variant_id = SATU001 black M)
   ↓ Printful catalog ID, mockup_url
[SQLite products テーブル] insert
   ↓ row { id, brand, drop_num, price_jpy, weather_data, seed_data, prompt_hash, ... }
[wearmu.com] /api/products/mugen で表示
[SUZURI mirror] auto-publish (国内向け、 §24-v2)
[X post] sns_post_queue → 1/分 drain (announce 用)
```

すべて API、 0 humans。 yuki が触るのは:
- prompt の文言 (この記事の内容)
- §25 の directive
- Constitution の編集

それ以外は自動。 1 時間に 1 着が **自分の意思で作られない** からこそ、 「今しか買えない」 が成立します。

---

## 6. 監査ハンドル

- **コード**: <https://github.com/yukihamada/mu-brand> (CC0 / MIT)
- **prompt 履歴**: `products.prompt_hash` で各 drop の prompt を SHA-256 で記録
- **weather seed**: `products.weather_data` に生 JSON で保存
- **過去の prompt 変更**: git log で追える

「prompt を変えた」 → git commit が残る → 過去どの drop がどの prompt で生まれたか辿れる。

---

## 7. なぜ公開するか

Constitution §11 (数字で書く):
> Numbers over adjectives, even for ourselves.

prompt も数字の一部です。 デザインを「センス」 で説明するブランドは多いけど、 MU は「**この prompt と、 その時の天気の組合せでこの絵が出ました**」 と再現可能な形で説明できる。 reproducibility は brand trust の前提だと思っている。

もし「もっと良い prompt」 を思いついた方が居たら issue ください: <https://github.com/yukihamada/mu-brand/issues>

---

*Constitution §11 (数字) / §25 (デザイン directive) に従って書きました。*
*次回更新: §25 を変えた日 (お客様アンケート [/survey/quality](/survey/quality) 結果次第)。*
