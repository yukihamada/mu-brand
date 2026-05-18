---
name: jf-certification
description: JF認定（JiuFlow Certification）プロジェクト。柔術道場・指導者・大会の三層認定制度、SJJJF公益法人化と統合、堤・[partner]・[partner]を発起人理事に
metadata: 
  node_type: memory
  type: project
  originSessionId: 08165038-02ba-4e9f-8289-6cac7b697eb3
---

# JF認定 — JiuFlow Certification

## 目的
日本柔術界の「安全 × 強い × 良い」を底上げする認定制度。
**Why:** 入口で怪我・道場ガチャ・データ不在の三大問題を業界として誰も解決していない。帯（系譜）以外に道場の質を示す軸が無い。
**How to apply:** JiuFlow を SaaS から業界インフラに格上げする中核プロジェクトとして扱う。広告CVR改善などより上位の戦略。

## 構造

### 三層認定
1. **認定道場** (Certified Dojo)
2. **認定指導者** (JF Coach) — 個人ライセンス、Instagram/名刺に番号付きで掲載可
3. **認定大会** (Certified Tournament)

### 5ティア（帯と同じ色）
- 白認定: 保険+応急対応
- 青認定: + 指導者ライセンス + ハラスメント窓口
- 紫認定: + 怪我率データ公開
- 茶認定: + 第三者監査済（覆面audit / ミステリー出稽古）
- 黒認定: + 国際相互認証（IBJJF/ADCC）

## 経済設計（道場が取らないと損になる仕掛け）
1. JiuFlow.com 道場検索で認定が上位固定
2. 損保提携、団体保険30%割引
3. JiuFlow Passport（認定道場間の出稽古ネットワーク）
4. 法人福利厚生BtoB、認定道場のみ対象
5. 用具メーカー提携、会員20%オフ

## 発起人理事（3名）
- **[partner]良蔵**（SJJJF理事長、世界王者）— 競技と組織の権威
- **堤**（S道場オーナー、独立黒帯）— 現場の良識。LinkはYAWARA案件の独立監事候補と同人物。一義師範ではない
- **[partner]健太郎**（青帯、Yawara Australia、共同起案者）— 実務と若手の声

世代・立場・出身が異なる三氏で派閥色を消す中立性を担保。

## SJJJF統合
JF認定は SJJJF公益社団法人 が**認定発行主体**、JiuFlow が**運営委託先**。
- 公益法人化に実体ある事業を与える役割
- YAWARA案件解決（[[yawara-case]]）と接続
- 文科省・スポーツ庁ルートに乗る

## ロードマップ
- Phase 0 〜2026-06: 三氏合意、白認定基準書確定
- Phase 1 2026-07〜09: 10道場β認定、指導者ライセンス試験オンライン化、認定マップ実装
- Phase 2 2026-10〜: 一般受付、保険1社提携、Tokyo World 2026連動
- Phase 3 2027〜: 100校、法人福利厚生、IBJJF相互認証

## 成果物（2026-05-12 時点、全て `bjj/jiuflow-ssr/docs/certification/` 配下）

| # | ファイル | 内容 |
|---|---------|------|
| 01 | `01-pitch-directors.md` + `.pdf` | 発起人理事打診資料 |
| 02 | `02-white-certification-criteria.md` + `.pdf` | 白認定 基準書（5領域25項目） |
| 03 | `03-passport-plan.md` + `.pdf` | JiuFlow Passport 会員特典プラン（White/Gold/Black 3ティア、出稽古¥500還元経済モデル、DBスキーマ案含む） |
| 04 | `04-email-to-directors.md` + `.pdf` | 三氏宛メール文面（[partner]・堤・[partner]） |
| 05 | `05-certification-map.html` | Leaflet ベース認定道場マップ UIモック |
| - | `pdf-style.css` | PDF 生成用スタイルシート |

PDF生成: `pandoc -f gfm -t html → Chrome --headless --print-to-pdf`

## JiuFlow Passport 設計（03-passport-plan.md より）
- **White Passport**（認定道場会員 自動付与）: 月3回出稽古無料 / JiuFlow Premium ¥1,500 / 大会20%オフ
- **Gold Passport**（JF Coach保有）: 月10回出稽古無料 / Premium無料 / セミナー優先登壇
- **Black Passport**（黒帯+50時間実績）: 出稽古無制限 / 同伴1名Gold待遇
- **¥500/回 還元金**で出稽古経済を創出 — 道場が認定取得を求める動機を加速
- DBスキーマ: `dojo_certifications`, `passports`, `dropin_visits`

## 次工程
1. 三氏のメール送付前レビュー
2. jiuflow-ssr への実装拡張（DBスキーマ + /passport ルート + 認定道場マップ動的化）
3. 青認定基準書、JF Coach試験要項の起案

## 本番デプロイ済み（2026-05-12）
- **URL**: https://jiuflow.com/certification (→ /certification/map に308 redirect)
- **URL**: https://jiuflow.com/certification/map
- Rust ハンドラ: `src/handlers/certification.rs`（phase1_dojos() に10校ハードコード）
- Askama テンプレ: `templates/pages/certification_map.html`
- ユニットテスト 3 本（10校データ、テンプレrender、緯度経度）
- 関連 PR/Commit: 1e48648 (実装), bba20a6 (資料)

## 第1期認定候補（β11校、2026-07〜09で順次認定）
1. **YAWAY JIU-JITSU ACADEMY / ヤウェイ柔術アカデミー**（宮崎市、代表: 倉岡ジョンカルロス博、yaway.jp）
2. **Over Limit Sapporo / 札幌オーバーリミット**（[partner]先生本拠）
3. **SIIIEEP（北参道BJJ）**（渋谷区北参道、MUコラボ実績あり [[sweep-collab]]）
4. **YAWARA Jiu-Jitsu Academy（原宿）**（渋谷区神宮前、原宿駅至近、well-being complex内、[[yawara-case]] 撤退案件中だが認定制度は中立に扱い基準満たせば認定）
5. **フロー柔術（仙台）** — 認定審査中
6. **S道場**（堤先生拠点）
7-9. フィリピン提携道場 3校（マニラ・セブ・他、[partner]選手 Yawara Australia 経由で具体名確定予定）
10-11. 発起人三氏の推薦枠（[partner]・堤・[partner] 各1校）

国内6・海外3-5の構成で地理的偏在を避ける。第1期は認定料無料 + 公表ロゴ提供。

**Why 候補選定:** 派閥色を消し、地理・規模・系譜の偏りを排除するため。
**How to apply:** 認定基準書策定後、各道場に対し個別オファー → 同意取得 → 公表。

**Why YAWARA原宿も含める（ユーザー指示 2026-05-12）:**
撤退方針（[[yawara-case]]）は[partner]オーナー個人の倫理問題に対応するもの。
認定制度は道場の安全・衛生・指導の質を見るもので、原則中立に扱う。
基準を満たせば認定する姿勢を示すことで、認定の独立性・公正性を担保。
ただし利益相反規程との関係は要確認（[partner]先生が業務委託で運営、現在撤退方針）。

## 重要決定事項
- 保存場所は `bjj/jiuflow-ssr/docs/certification/`（`bjj/jiuflow` は存在しないため）
- 三氏への打診期限は 2026-05-31（同意取得目標）
- 利益相反: 自経営道場の審査は他理事のみで決議

## 関連
- [[yawara-case]] — 堤氏・[partner]氏の文脈、SJJJF公益法人化との統合
- [[jiuflow-app]] — JiuFlow本体、認定マップ実装先
- [[jiuflow-ads-cvr-findings]] — JF認定はCVR改善の根本解にもなりうる（道場の質可視化で信頼担保）