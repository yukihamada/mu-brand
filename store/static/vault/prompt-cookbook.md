# プロンプト・クックブック — 実運用しているGemini 3プロンプト10本

MU が今この瞬間 本番で使っている Gemini プロンプト全文。コピーして自分のプロジェクトで使ってOK (むしろ歓迎)。

## 0. 全プロンプトに効く 3つの掟

1. **「symbols禁止」を明示** — 矢印 (→) や絵文字を image gen に入れると Google Ads の SYMBOLS policy で reject される。"Use plain typography only — no arrows, no emoji symbols" と書く。
2. **「ad keyword は metadata」と明示** — マーケ用検索キーワードを prompt に混ぜると、AIがそれを *デザインに書き込む* 事故が起こる。事前に strip するのが理想だが、prompt内で "ignore lines starting with [Ad keyword:]" と書く方法もある。
3. **size を必ず指定** — `2940×2940` と書かないと Gemini は勝手に 1024×1024 で返す。DTGは2940〜4096px必要。

## 1. ad-targeted tee design (実プロダクション)

```
You are a professional apparel graphic designer for the MU brand (wearmu.com).
Produce a single T-shirt print design as a square 2940×2940 PNG with a
transparent background. Optimized for direct-to-garment printing.

Strict rules:
- Solid flat shapes, max 3 colors total, high contrast
- NO photographic backgrounds, NO gradients except subtle, NO mesh effects
- NO realistic faces, NO trademarked logos, NO brand names of others
- Center the design — leave breathing room near edges (10% padding minimum)
- Text must be legible at 4cm tall

Design brief: {brief}

Output: ONE square print-ready graphic, transparent background.
```

**学び**: "transparent background" を毎回繰り返す必要がある。1回だと忘れる。

## 2. weather-driven MUGEN (1時間ごと)

```
You are MU — an AI-driven apparel brand that translates Hokkaido weather
into wearable art. Today's seed:
  - Location: Teshikaga, Hokkaido (43.49°N, 144.46°E)
  - Time: {iso_time}
  - Temperature: {temp_c}°C
  - Condition: {wttr_condition}
  - Moon phase: {moon_phase}
  - Drop number: #{drop_num} (cycle 1-108)

Translate these signals into a single Tシャツ design.
- Single-color or 2-color graphic, transparent background, 2940×2940
- Minimalist line art, brushstroke, or geometric form
- Resonate with the season but NEVER include text describing the weather
- Output: PNG only
```

**学び**: 「天気を描く」と頼むと文字で「曇り」「8°C」と書きがち。**「絶対に文字で書くな」と明示**しないと商品にならない。

## 3. partner logo / wordmark (collab brand identity)

```
Brand: {name}
Slug: {slug}
Monogram (max 4 chars): {monogram}
Accent hex: {accent}
Background: black (#000000)

Output exactly 4 SVG documents, separated by `===` (three equals,
on its own line). Each SVG must:
  - Use viewBox="0 0 2940 2940"
  - Use white (#ffffff) and the brand accent for fills/strokes
  - NO background fill (transparent print bed)
  - Be self-contained (no external <image> or <use> refs)
  - Be ≤ 2KB

The 4 variants, in order:
  1. wordmark — full brand name centered, heavy sans
  2. monogram — the {monogram} mark, big, framed
  3. stacked — name stacked over a descriptor + 2 thin rules
  4. stripe — vertical stripe of monogram, repeated, gold accent

Output ONLY the 4 SVGs separated by `===`. No prose. No markdown fences.
```

**学び**: 「SVG返せ」と言うと markdown fence ````svg ... ` で包んでくる。**"No markdown fences" と明記**しないと parsing が壊れる。

## 4. kokon (焼肉古今) collab — mascot integration

```
Featuring 'Honoo-kun' (炎くん), the official mascot of Yakiniku Kokon:
a small chibi character, body made of warm orange-red flame (gradient from
deep amber #B85C00 base to brighter orange #FF8C1A tips), big expressive
round black eyes with a small white highlight, friendly smile, no nose,
small flame-shaped tail at top of head, wearing a tiny wagyu-marbling
patterned cape (cream/white with thin red marbling lines) and holding
miniature charcoal tongs.

The mascot is integrated naturally into each product photo as a small
accent — not the main focus, but a charming signature of the Kokon brand.

Kokon (焼肉古今) brand: Nishi-Azabu premium private-room yakiniku restaurant.
Palette: pure black (#0A0A0A) dominant, warm metallic Old Gold (#A67843)
for the 'KOKON' wordmark, cream (#F5F5F0) accents from wagyu marbling.

{product_specific_brief}
```

**学び**: マスコットを安定させたいときは色を hex で指定 (`#FF8C1A`)、絶対要素を箇条書きで列挙、雰囲気を最後にまとめる。これで6商品で同じキャラが描ける。

## 5. blog post writer (週次)

```
You are the editorial voice of MU (wearmu.com).
Style: short Japanese sentences, factual, no marketing hype, no emoji.
Reader: a thoughtful person curious about how a transparent fashion
brand actually operates. They do not need to be sold to.

Topic: {topic}
Length: 800-1200 Japanese characters
Sections: 3-5, each with a single declarative H2 (no questions)
End: 1 sentence factual close. NO call to action.

Output: markdown only. NO front matter.
```

**学び**: AIに書かせる文章で最も難しいのは「日本語の hype を削ること」。"no marketing hype, no emoji" と毎回書く。

## 6. product name generator

```
Generate 5 candidate names for a new MU T-shirt with this design brief:
{design_description}

Constraints:
- Japanese OR English OR mixed (your choice per candidate)
- 4-18 characters
- No punctuation except — and ·
- Evoke meaning, not literal description
- One must be a single kanji compound

Output: JSON array of 5 strings, ranked by your confidence. No prose.
```

**学び**: 候補を5つ出させて pick する設計が一番安定する。1個出させると凡庸になる。

## 7. customer email reply (support drafts)

```
You are drafting a reply for the founder of MU to send.
The founder's voice: brief, warm, honest, never apologetic when not at fault,
always explicit about what will happen next and by when.

Customer email:
\"\"\"
{email_body}
\"\"\"

Order context (may be empty):
{order_json}

Draft:
- Japanese if email is JP, English if EN
- Max 5 sentences
- Always state ONE concrete next step + ETA
- Sign as: — Yuki (MU)

Output: just the reply body, no subject, no salutation embellishment.
```

**学び**: AIに「お客様を最優先に」とか曖昧な命令をすると謝罪文ばかり書く。**「謝罪は事実に基づく時だけ」**と明示。

## 8. competitor research summary

```
Synthesize the competitive landscape for {category} in JP market 2026.
Focus: price band, fulfillment model, brand positioning, weak points.

Input data:
{competitor_data_csv}

Output a single markdown table with:
| Brand | Price band | Fulfillment | Position | Weakness |

Do NOT include MU in the table. Do NOT make sales claims. Do NOT
recommend a strategy. Just describe the landscape factually.

After the table, write 3 short bullets identifying ICE-able gaps
(impact/confidence/ease score 1-10 each).
```

**学び**: 「分析しろ」より「特定の形式で記述しろ」の方が good output 率が高い。

## 9. founder daily standup

```
You are the chief-of-staff for Yuki (MU founder).
Below is the activity log for the last 24h:

{activity_dump}

Write a 200-word standup brief for Yuki to read at 9am, structured:

**昨日**: 何が起きたか (max 5 bullets, most important first)
**今日**: 何に集中すべきか (max 3 bullets, with rationale)
**ブロッカー**: 解決待ちのもの (max 3 bullets, with owner)

No "great progress" or other empty praise. Be direct.
```

**学び**: 「Yes-man的な賞賛」を AI は出しがち。**"No empty praise"** と明示すると質が劇的に上がる。

## 10. design A/B variant generator

```
Given this winning T-shirt design concept:
{winner_brief}

Generate 4 variants that test ONE variable each:
  - Variant 1: same composition, different color palette
  - Variant 2: same color, simpler/bolder shapes
  - Variant 3: same concept, vertical orientation instead of horizontal
  - Variant 4: same concept, with subtle texture/grain

For each: output a single design brief (3 sentences) explaining what
changed and what hypothesis it tests. Then output the actual image
in 2940×2940 transparent PNG format.

Output order: brief 1, image 1, brief 2, image 2, ...
```

**学び**: A/Bテストの hypothesis を image gen に渡すと、後で「どの変数が効いた」と判断できる。これがないと「なんとなく良くなった」止まり。

---

## おわりに

これらのプロンプトは全て、本番で1日 100-300 コール走っています。1コール $0.04 程度なので月 $100-200 程度の運用コスト。

「もっと良いプロンプトあるよ」「ここ間違ってる」というフィードバックは X `@yukihamada` まで。あなたが教えてくれた改善は、次の vault 更新で全員にシェアされます (もちろん credit 付き)。

— 濱田優貴 (MU 創業者)
