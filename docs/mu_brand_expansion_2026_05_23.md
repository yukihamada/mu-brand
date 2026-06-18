# MU Brand Expansion — 5 new brands + 20+ designs

作成: 2026-05-23 (Yuki + Claude Opus 4.7)
ステータス: **承認待ち**（コード/DB 変更前）

## 戦略のフレーム

MU は **「無 / 月 / Mu × X」** の引き算ブランド。
既存: BJJ・CODE・COFFEE・ZEN・MOON・MU・TOKYO・JIUFLOW・KOKON・ROLL = 10 brand。
拡張軸は **Yuki の趣味と事業の交差点** から取る。重複を避け、別ペルソナを取りに行く。

| 拡張軸 | Yuki との関連 | 既存と重ならない理由 |
|---|---|---|
| VOICE | Koe Device, 音声 P2P, "速くノイズなく" 哲学 | 既存はテキスト/ヴィジュアル中心。声は別 |
| OCEAN | Hawaii Park Ward 移住 + Soluna Fest 7/1 | TOKYO はストリート、海は新しい |
| LODGE | Soluna SIPs 杉 CLT、弟子屈拠点、薪と雪 | ZEN は禅、LODGE は山の生活 |
| OCTAGON | 中村兄弟 UFC PJ + 立石 COO + Yuki 青帯 | BJJ は道場、OCTAGON はリング/プロ |
| FOUNDER | Mercari→NAH→Enabler、20年連続会社作り | CODE はエンジニア、FOUNDER は経営者 |

## 新ブランド 5 本の位置付け

### 1. **MU × VOICE** 🎤
- **コア**: 声で世界を書く。Koe device、Soluna 音声受信、AI 音声合成。
- **トーン**: テクノロジカル + 静謐。波形 + カナ。グレースケール + 蛍光。
- **デザイン候補**:
  - "FIRST WORD" — 始まりは声から
  - "WAV.MU" — .wav ファイル拡張子 + MU
  - "NO TYPE" — 入力ゼロ哲学
  - "聞こえる" — 単一書道
- **ターゲット**: Koe early adopter、声入力ユーザー、Discord/Twitch creator
- **product mix**: tee, hoodie, mug, sticker

### 2. **MU × OCEAN** 🌊
- **コア**: 太平洋。Hawaii ⇄ Tokyo の往復、Soluna Fest 7月、夏。
- **トーン**: 塩、サンドベージュ、太陽、ALOHA カナ。
- **デザイン候補**:
  - "ALOHA・MU" — アロハ × 無
  - "SALT YEAR" — 塩漬けの一年（移住）
  - "波 ◐ MOON" — 波と月（MOON 兼用候補）
  - "PACIFIC TIME" — 時差すら愛
- **ターゲット**: Hawaii 移住者、サーファー、夏先取り
- **product mix**: tee, tank, tote, beach towel

### 3. **MU × LODGE** 🏔️
- **コア**: 弟子屈、Soluna SIPs、薪、雪、籠もる。
- **トーン**: 焦茶、亜麻色、ネイビー。木目とランタン。
- **デザイン候補**:
  - "WINTER STAY" — 冬籠もり
  - "杉 = 永遠" — 杉材 100年保証
  - "FIRE BUILT" — 自分で組んだ火
  - "1100 KM SOUTH" — 弟子屈→東京の距離
- **ターゲット**: アウトドア、SIPs hut 投資家、田舎志向
- **product mix**: hoodie, long-sleeve, canvas, beanie

### 4. **MU × OCTAGON** 🥊
- **コア**: 中村兄弟 UFC 計画、プロ格闘技、リング上の無。
- **トーン**: 朱赤 + 群青（中村ブランド色）、太字、緊張感。
- **デザイン候補**:
  - "WALK OUT" — 入場時の無音
  - "5 ROUNDS" — 5R 制
  - "朱と群青" — 中村兄弟の二色
  - "OCTAGON ◯" — リング型の MU
- **ターゲット**: 格闘技ファン、PRIDE 世代、UFC 視聴者
- **product mix**: tee, rashguard, fight-shorts, hand-wrap

### 5. **MU × FOUNDER** 🚀
- **コア**: Mercari → NAH → Enabler、起業家の自虐 + 矜持。
- **トーン**: ジェットブラック、白文字、書類フォント。
- **デザイン候補**:
  - "20 YEARS SHIPPING" — 20 年連続出荷
  - "STILL EARLY" — まだ早い（投資家へ）
  - "CEO・MU" — CEO 肩書きと無の両立
  - "DAY 1 EVERY DAY" — Amazon リスペクト
- **ターゲット**: VC 界隈、創業者 / シードフェイズ起業家、Stripe Atlas 卒業生
- **product mix**: tee, hoodie, cap, journal, mug

## 実装プラン

1. catalog_brands に 5 行 INSERT（slug, name, emoji, color, tagline）
2. catalog_products に各 brand × 4 SKU = **20 SKU** INSERT（tee-black 中心、product mix を 1 つずつ）
3. perfect_pipeline.py に 5 brand 分の BRAND_STYLE / scene 追記
4. パイプライン実行（20 SKU × ¥12 = **¥240** / 2-3 min）
5. ダッシュボード再生成 + open

## 拡張後の総計

| 項目 | 現在 | 拡張後 |
|---|---|---|
| 一級ブランド数 | 10 | **15** |
| 完璧化 SKU 数 | 10 + bench 20 = 30 | **50** |
| Yuki 個人の趣味反映度 | 30% (BJJ/COFFEE 程度) | **80%** (5本中5本が個人事業) |

## 戦略 KPI（30日）

- VOICE / OCEAN は **Koe 早期ユーザー** + **Soluna Fest 参加者** に直接当てる → CVR 計測
- OCTAGON は **中村兄弟 SNS** で告知してプロ格闘技ファン取り込み
- FOUNDER は **VC コミュニティ Slack** に投下 → 自虐 CTR 検証
- LODGE は **企業版ふるさと納税** で弟子屈町に紐づけて B2B 切り口テスト

## ASK before doing

このドキュメントの位置付けで **進めて OK** か:
- 5 brand × 4 SKU = 20 SKU の追加（¥240 / ~3min）
- 各 brand の hero lifestyle 3 枚（既存と同じ schema）
- ダッシュボードに 20 SKU 追加表示

承認したらコードに落とす。
