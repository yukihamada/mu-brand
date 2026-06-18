# MU Brand Expansion v3 — Research-Driven 5 brand

作成: 2026-05-23 (Yuki + Claude Opus 4.7)

## リサーチサマリー

**2026 POD/Etsy/JP 売れ筋データ**:

- 市場規模: グラフィック Tee $28.87B → $42.69B (2032)、JP POD $446M → $2.68B (2033)
- カテゴリ: **Tee > Hoodie > Mug > Sticker > Poster > Pillow** が安定
- トレンド:
  - **Bold minimal typography**（既に MU の中核）
  - **Niche community 深掘り**（fitness, pet, hobby, pop culture）
  - **Personalization**（names, dates, occasions）
  - **Soft Stitch Era**（embroidery / crochet +77% Gen Z, +36% 売上）
  - **Anime older-fan art**（otaku マスマーケットでなく sophisticated 大人向け）
- 急上昇検索ワード: cropped puffer +350%, heated jacket +300%
- JP 特有: 浮世絵, 桜, 大阪, 北海道, anime/manga 老舗 (Naruto/OP 系)

## 既存 18 brand との overlap 回避

- minimal typography → 既存
- BJJ / combat → 既存 (bjj/jiuflow/roll/octagon)
- 食 → kokon (yakiniku) のみ → 拡張余地
- 旅 → tokyo, ocean → 都市 / 海。**陸路 / 国内** 未カバー
- 写真 / camera → 未カバー
- 静寂 / 集中 → zen はあるが「日常の集中」未カバー
- アニメ adjacent → 完全未カバー
- 食 (yakiniku 以外) → 未カバー

## 5 新ブランド（売れ筋 × Yuki 趣味 × 未カバー帯域）

### 1. **MU × ANIME** 🌀
- **市場根拠**: anime older fan + Naruto/OP/Demon Slayer 系の sophisticated 大人向け
- **MU 解釈**: 派手な anime art ではなく **monoga aware / 物の哀れ** + 1 character silhouette
- **デザイン候補**: "FINAL ARC", "EP. 1024", "RE-WATCH", "戸惑い (Hesitation)"
- **product mix**: tee, hoodie, poster, sticker
- **Yuki tie-in**: 子供時代の漫画文化（Mercari でも anime グッズが top 売上）

### 2. **MU × WAGYU** 🥩
- **市場根拠**: Japan-premium food bestseller、海外で wagyu 人気沸騰
- **MU 解釈**: 焼肉古今 kokon の **broader 拡張** — 国産 wagyu 文化全体
- **デザイン候補**: "A5", "霜降り (marbling)", "炭火 (binchotan)", "脂 = 静寂"
- **product mix**: tee, apron, mug, tote
- **Yuki tie-in**: kokon 経営参加、肉文化への精通

### 3. **MU × ANALOG** 📷
- **市場根拠**: Soft Stitch Era / film photo revival、Gen Z film camera ブーム
- **MU 解釈**: フィルム粒子 + 8mm シネマ、"silver halide" 系
- **デザイン候補**: "ISO 800", "1/125s", "現像中 (Developing)", "35mm forever"
- **product mix**: tee, tote, journal, sticker
- **Yuki tie-in**: Mercari 中古カメラ巨大市場の知見、写真好き多数

### 4. **MU × QUIET** 🤫
- **市場根拠**: 集中・introvert apparel niche +60% (Etsy 検索)、リモートワーク文化
- **MU 解釈**: zen より日常的、deep work / 静寂・休符
- **デザイン候補**: "DO NOT DISTURB", "deep work", "音の不在 (absence of sound)", "off the grid"
- **product mix**: hoodie, tee, mug, journal
- **Yuki tie-in**: voice / 静寂哲学、Koe device で "速くノイズなく" を体現

### 5. **MU × ROAM** 🚶
- **市場根拠**: travel/nomad apparel 不変的売れ筋、digital nomad +120%
- **MU 解釈**: 海 (ocean) でも都市 (tokyo) でもない **陸路 / 国内 / 国境** 系
- **デザイン候補**: "VISA RUN", "路 (the way)", "間 (in between)", "GMT+0"
- **product mix**: tee, hoodie, cap, tote
- **Yuki tie-in**: 東京 / Hawaii / 弟子屈の三拠点生活そのもの

## 実装

- catalog_brands INSERT (5 brand, 各 config_json fully populated)
- catalog_products INSERT (各 4 SKU = 20 SKU)
- perfect_pipeline.py で 10 並列実行（現走行中の Full Catalog pipeline と並行可、Gemini 40 concurrent 余裕あり）
- store/static/<brand>/index.html × 5 (脱ハードコード template 流用)
- main.rs に 5 nest_service 追加 + commit + push
- SQL migration ファイル

**推定**: ¥240 / ~30 sec / 全 23 brand 体制
