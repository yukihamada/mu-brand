# JiuFlow Video Ad Production Brief

**Goal**: CVR 爆上げ。静止画 + テキスト広告から動画化への移行。
**Production budget**: ¥0 (既存資産 + 無料ツール)
**Target output**: 1 日で 5 本展開可能

---

## 必要ツール

| Tool | 用途 | 入手 |
|---|---|---|
| **CapCut Desktop** | 動画編集 (Mac) | https://www.capcut.com/ja-jp/ 無料 |
| **Cloudflare Stream** | 既存 99 本動画 URL | iframe.cloudflarestream.com/customer-8pjeinyro6qd2bcx |
| **DaVinci Resolve** | (代替、より高度) | https://www.blackmagicdesign.com 無料 |
| **Audacity** | 音声編集 | 必要なら無料 DL |

---

## 動画 #1: 「99 técnicas em 60 segundos」(価値圧縮型) ⭐最優先

**所要時間**: 30 分
**書き出し**: 3 言語版 × 2 アスペクト比 = 6 本

### 台本

```
[0:00-0:01] イントロカード
  PT: "Tudo isso. No seu celular."
  EN: "All this. In your pocket."
  JP: "技術 99 本。スマホで。"

[0:01-0:58] 99 本の技術動画から 0.58s/本で連続再生 (57秒)
  - 順序: ガード → パス → サブミッション → エスケープ → 立ち技
  - 各 cut の頭 0.1s に技名のテロップ (fade in/out)
  - BGM: 静かな drum + 緊張感のある synth (CapCut royalty-free)
  - 終盤 (75-90% 地点) に少し速度落として「Mata Leão」「Triangle」など決め技

[0:58-0:59] ロゴ + アプリスクリーン (0.5s) + CTA

[0:59-1:00] CTA カード
  PT: "Grátis 7 dias. SEM cartão. jiuflow.com"
  EN: "Free 7 days. NO card. jiuflow.com"
  JP: "7日間無料。カード不要。jiuflow.com"
```

### 編集手順

1. CapCut で **9:16 縦長プロジェクト** 作成 (1080×1920)
2. 99 本動画を Cloudflare Stream から DL (heading URL)
3. タイムライン に並べ、各 clip を **0.58s** に切り詰め (Tools → Speed)
4. テロップ追加 (Text → 上下に技名 + 国旗絵文字)
5. BGM 追加 (CapCut → Music → Hype / EDM カテゴリ)
6. アスペクト切替 (Project Settings → 16:9 1920×1080 で再書き出し)
7. 完成 → mp4 export → YouTube unlisted upload

---

## 動画 #2: 6 秒バンパー (Pattern Interrupt 型) ⭐最安 CPM

**所要時間**: 30 分 × 3 言語

### 台本 PT

```
[0:00-0:02] 黒画面 + 白文字
  "Eu treinei BJJ por 5 anos."
  (中断、無音)

[0:02-0:04] 同じ画面、フォント変わる
  "Mas não evolui."
  (溜め、効果音 dum)

[0:04-0:06] アプリスクリーン flash → ロゴ
  "JiuFlow."
  小さく "Grátis 7 dias. jiuflow.com"
```

### 台本 EN

```
[0:00-0:02] "I trained BJJ for 5 years."
[0:02-0:04] "But I didn't improve."
[0:04-0:06] App screen + "JiuFlow." + "Free 7 days. jiuflow.com"
```

### 台本 JP

```
[0:00-0:02] 「柔術を 5 年やった。」
[0:02-0:04] 「でも上達しなかった。」
[0:04-0:06] アプリ画面 + 「JiuFlow.」 +「7日無料。jiuflow.com」
```

### 編集 tips

- BGM 不要 (6秒では noise になる)
- 黒画面 + 白テキスト だけで impact 最大
- フォント: 細めの sans (Noto Sans / Open Sans)
- 最後の app screen は 0.5s だけ flash (longer = ad fatigue)

---

## 動画 #3: 「Tournament prep in 7 days」(緊急性型)

**所要時間**: 1 時間
**ターゲット**: 大会前 1 ヶ月の BJJ 練習者 (検索ボリューム高い時期)

### 構成

```
[0:00-0:03] 「7 days until tournament」
  カレンダー 7 → 6 → 5 → 4 → 3 → 2 → 1 のカウントダウン animation

[0:03-0:15] 各日のアプリ画面 (1.5s ずつ)
  Day 7: 対戦相手リサーチ画面
  Day 6: AI Game Plan 生成
  Day 5: 練習記録
  Day 4: 弱点分析
  Day 3: テクニック動画 (Galvao 等)
  Day 2: メンタル準備
  Day 1: 当日のチェックリスト

[0:15-0:25] 試合シーン (BJJ コンペ archival footage、free use)
  勝つシーンで音楽 swell

[0:25-0:30] ゴールドメダル + CTA
  PT: "Vença seu próximo torneio. Comece grátis."
  EN: "Win your next tournament. Start free."
  JP: "次の大会に勝つ。今すぐ無料で。"
```

### 素材調達

- 試合シーン: YouTube 上の "BJJ tournament highlights" free-use compilations
- 注意: 著作権チェック (CC ライセンス or fair use の研究的利用範囲)

---

## 動画 #4: 「道場ノートが進化する」(JP 文化適合)

**所要時間**: 1 時間
**ターゲット**: 35-50 歳の真面目派 JP BJJ ユーザー

### 構成

```
[0:00-0:03] 紙のノートに技術メモを書いてる手 (POV)
  「あの技、何だっけ...」のテロップ

[0:03-0:08] ノート → スマホへ transform animation (CapCut の transition effect)
  「あなたの道場ノートを、もっと賢く」

[0:08-0:20] アプリ画面ツアー (各 1.5s)
  - 技術記録
  - 動画でリプレイ
  - AI ゲームプラン
  - 練習ログ

[0:20-0:25] 「永久無料プランあり / Pro は 7日間無料体験」テキスト

[0:25-0:30] アプリストアバッジ + jiuflow.com URL
```

### こだわりポイント

- 「カード不要」を 2 回繰り返す
- 「永久無料」を強調
- 字体は明朝体 (Yu Mincho) で和の格調
- BGM: 静かな piano (CapCut → Music → Calm)

---

## 動画 #5: 「Sem Cartão 連発」(remarketing 用)

**所要時間**: 15 分 (最短)
**ターゲット**: 既に jiuflow.com 訪問者だが未登録 (cookie ベース)

### 構成

```
[0:00-0:03] アプリスクリーン (静止)

[0:03-0:10] テキストオンリー fade in/out
  Sem cartão.
  Sem cartão.
  SEM CARTÃO.
  (繰り返し、徐々に文字大きく)

[0:10-0:13] 「Te disse, sem cartão.」
  ("言ってる、カード不要だよ")

[0:13-0:15] 「Clica aqui.」 + URL
```

EN/JP 版同様。

---

## 投入計画

### Day 1 (今日)
- ✅ #1 「99 técnicas」60秒 編集 → 3 言語書き出し
- ✅ #2 「6秒バンパー」 → 3 言語

### Day 2-3
- #3 「Tournament prep」
- #4 「道場ノート」JP 用
- #5 「Sem Cartão」remarketing

### Day 4-7
- Google Ads YouTube campaign 設定 (¥1K/d で testing)
- 1 週間 perf 観察 → 勝者 scale

---

## Google Ads への登録

### 既存 Search campaign に Video asset として追加

```python
# scripts/add_video_assets.py (テンプレ)
import sys; sys.path.insert(0, "/Users/yuki/workspace/mu-brand/scripts")
from ads_lib import client_for

c = client_for("JiuFlow")
svc = c.get_service("AssetService")
op = c.get_type("AssetOperation")
op.create.youtube_video_asset.youtube_video_id = "VIDEO_ID_HERE"  # unlisted YouTube ID
resp = svc.mutate_assets(customer_id="4070111170", operations=[op])
print(f"video asset: {resp.results[0].resource_name}")
```

### 新規 YouTube Video Campaign

別 campaign として作成 (Search と分離して budget control):
- Campaign type: Video → Drive conversions
- Budget: ¥1,000/d (testing)
- Target: BJJ-related YouTube channels (custom audience)
- Geo: 既存 JF main の targeting copy

---

## 期待効果

| 指標 | 現状 | 動画展開後 (1 ヶ月後 想定) |
|---|---|---|
| 月 spend | ¥132K | ¥130-200K (動画 ¥30-70K 追加) |
| 月 conv | 19 人 | 40-60 人 |
| ROAS | 1.06x | 1.5-2.0x |
| 月 revenue | ¥98K | ¥240-360K |
| **月 純益** | **¥-34K** (赤字) | **+¥110-160K** (黒字!) |

---

## 注意点

1. **Smart Bidding 学習が完了する前に新規 video asset を追加すると**、学習が
   さらに延長する可能性。**最初は新規 YouTube campaign で testing**、安定後に
   既存 Search に asset 追加が安全。

2. 動画は 1-2 週間で「ad fatigue」(同じ動画見飽きる) するので **4-5 本 rotation**
   が前提。月 1 回新作 fresh が理想。

3. CapCut の **無料 BGM** は商用 OK のものだけ使用 (Music タブに「Commercial」
   フィルタあり)。

4. YouTube unlisted upload → 著作権 ID 検査が走るので **試合映像は public な
   公式 highlight reel** に限定 (UGC 個人撮影は権利クリア難しい)。
