# 100 着 / 20 日 — 素材・価格・クーポン戦略の全部出し

2026-05-14 · yuki · MU (wearmu.com) · §24-v3 + §Cessation 計算

---

20 日後 (2026-06-03) までに 100 着売る、 が今の数値目標です。
現在 8 着 (うち実顧客 1 名、 6 着は yuki dogfood)。 1 日平均 **5 着** 必要。 6 日間で実顧客 1 名のペースから、 30 倍のスケール。

正直に書くと、 100 は **希望値**。 現実的な範囲は **30-60 着** が中央値、 100 は上振れシナリオ。 ただ「数値で逆算する」 が §11 の義務なので、 100 を起点に逆算します。

---

## 1. 素材 quantitative マップ

8 候補を **原価・retail・margin・API 互換性・brand fit** で並べました。 通貨は JPY、 重量は oz (1 oz ≒ 28g、 1 gsm = 1 g/m²)。

| # | 素材 | 重量 | wholesale (¥) | 推定 retail (¥) | margin % | API | brand fit | stage |
|---|---|---:|---:|---:|---:|:-:|---|---|
| **a** | Printstar 00148-HVT (現 SUZURI) | 5.6oz / 190gsm | 3,500* | 4,900 | **29%** | ✅ | 並、 リブ襟なし | **0-100** |
| **b** | United Athle 5942 (5.6oz) | 5.6oz | 1,500 | 4,500 | 36% | △ オリジナルプリント.jp | 並 | 0-100 alt |
| **c** | **Stanley/Stella SATU001** (現 Stripe) | 6.4oz / 180gsm | 3,750 | **7,800** | **27%** | ✅ | **上 (リブ襟+GOTS+EU)** | **0-500** |
| **d** | United Athle 5919-01 heavy | 7.1oz | 1,800 | 6,800 | 38% | △ | 上 (厚手) | 100-500 |
| **e** | Stanley/Stella STTU788 The Heavy | 8.8oz / 250gsm | 4,800 | 9,800 | 35% | ✅ | **上+ (250gsm 重量)** | 100-500 |
| **f** | Camber 302 Max-Weight (US-import) | 8oz | 4,500 | 12,000 | 38% | ✗ | 重 (米軍系) | 200-1000 |
| **g** | Velva Sheen JP輸入 (loopwheel) | 8oz | 6,000 | 16,000 | 47% | ✗ | 高 (US loopwheel) | 500-2000 |
| **h** | **Loopwheeler L-1117** (和歌山 loopwheel) | 7.5oz | 7,500-9,000 | **35,000-50,000** | **70-78%** | ✗ | **最高 (Visvim peer)** | **MA only** |

*SUZURI Printstar base は SUZURI 側で内訳非開示、 wholesale は推定 ¥1,500、 SUZURI 取分込みで ¥3,500。

**読み方:**
- a/c = 今動いている (API ◎)
- b/d/e = オリジナルプリント.jp の返事次第で実装可能
- f/g = 個人輸入 manual、 MA 限定なら現実的
- h = Visvim 級、 §2 例外条項要、 MA 専用

---

## 2. MU の stage map

| stage | 累計売上 | 主力素材 | 価格帯 | 月次見込み |
|---|---:|---|---:|---|
| **Stage 1 — Awareness** | **0-100** | a (¥4,900) + c (¥7,800) | ¥4,900-¥7,800 | 80-100 着 (この記事の目標) |
| **Stage 2 — Validation** | 100-500 | a + c + **d / e (Premium+)** | ¥4,900 / ¥6,800 / ¥9,800 | 60-80 着/月 |
| **Stage 3 — Authority** | 500-2000 | a + c + e + **g (Atelier)** | ¥4,900 / ¥7,800 / ¥9,800 / ¥16,000 | 100-150/月 |
| **Stage 4 — Position** | 2000+ | 上記 + **h (MA 専用 Loopwheeler)** | + ¥35,000-50,000 (MA) | 150-200/月 |

**今は Stage 1 の真ん中。** 100 着到達で Stage 2 へ。

---

## 3. 価格設計の提案 (最終決定じゃなく、 提案)

### MUGEN (毎時 1 着、 108 cycle)

```
era-2 (drop 148+) 現状:
 base ¥7,800 → bonding +¥250/sale → cap ¥35,000

新提案:
 SUZURI (Standard) ¥4,900 固定
 Stripe (Premium)  ¥7,800 → +¥250/sale → cap ¥35,000   [現状維持]
```

**結論: 現状維持で OK。** SUZURI 経路の ¥4,900 が低いという感覚はあるが、 Stage 1 では **「お客様 1 人を獲得する」 が優先** で、 margin より volume。

### MUON (日 N 着、 気温連動)

```
新提案:
 SUZURI ¥4,900 固定
 Stripe ¥7,800 → +¥250/sale → cap ¥30,000   [現状維持]
```

MUON は 気温が seed = ストーリー強い。 ¥7,800 entry を変えない。

### MA (週 1 着、 1-of-1 auction)

```
era-2 現状:
 開始 ¥18,000 → ceiling ¥100,000

新提案 (アンケート option B + E の組合せ):
 開始 ¥18,000 → ceiling ¥100,000   [現状維持]
 + 「内側 woven label + hang tag」 追加 (¥300 原価増)
 + アンケート E (Loopwheeler) 取れる連絡先で正式発注確定したら、 retail ¥35,000+ で MA-Atelier 別ライン追加
```

### /you tee (personalized)

```
新提案: ¥6,800 → ¥7,800 へ 統一
   現状 ¥6,800 で /you の認知 + LP CVR テスト用に低位
   100 着到達後に ¥7,800 へ揃える
```

---

## 4. クーポン戦略 — 4 つの軸

### A. 「FIRST30 ローンチクーポン」 — **やる**

**仕組み:** SUZURI 経路の最初 30 着 = ¥3,900 (¥1,000 off)。 31 着目から ¥4,900 通常価格に戻る。

- 投資: ¥1,000 × 30 = **¥30,000**
- 期待効果: 30 着 確定で売れる (Stage 1 達成保証 + 顧客 30 人獲得)
- 実装: SUZURI 側で価格を ¥3,900 に変更 → 30 売れたら ¥4,900 に戻す (手動 OK)
- 期間: 20 日間または 30 着到達まで

### B. 「Referral コード」 — **やる**

**仕組:** 全 buyer に 5-char コード (例: `MUTOMU`)。 友人が code 入力で **買い手 ¥500 off + 紹介者 次回購入 ¥500 credit**。

- 投資: ¥1,000/成立、 ¥500 は次回購入時のみ消化なので実質 ¥500/成立
- 期待効果: 8 着 × 1.5x = 12 着 (referral multiplier、 業界 ave)
- 実装: 既存 `referral_codes` テーブル拡張、 Stripe coupon 自動発行
- 並行運用: A と組合せ可 (FIRST30 で買った人にも referral code 発行)

### C. 「Bundle 3-pack」 — **保留**

**仕組:** MUGEN 3 着セット ¥21,000 (¥7,000/着、 ¥2,400 off)。

- 投資: ¥800/着 × 3 = ¥2,400/sale
- 期待効果: 5-10 着 (collector layer のみ刺さる)
- リスク: Stage 1 で「セット販売」 は brand 弱体化、 集客効率が悪い
- 判定: Stage 2 (100 着到達後) で再検討

### D. 「TORU500」 X 投稿クーポン — **やる**

**仕組:** X で @wearMUcom + ハッシュタグ #wearMU 付きで写真投稿 → 次回 ¥500 off code 自動発行 (X auto-reply で送信)。

- 投資: ¥500 / 投稿
- 期待効果: 5-15 投稿 (visible UGC)、 + indirect = ¥0 で 5-15 件の SNS impression
- 実装: 既存 X mention agent 拡張、 投稿 verify → coupon API 発行
- 戦略的価値: 「7 人 buyer 全員が yuki 知人」 から脱却する唯一の経路

---

## 5. 20 日 day-by-day plan

```
Day 0 (今日 2026-05-14)
- ✅ /buy LP live
- ✅ PDP 素材ピッカー live
- ✅ B2B page → MU 本体 bridge live
- ✅ Hero に「今買える 1 着」 card live
- ✅ 6 X post enqueue 済 (queue 62-67)
- ✅ Loopwheeler + オリジナルプリント.jp 連絡済
- ⏳ FIRST30 クーポン実装 (今夜)
- ⏳ Referral コード実装 (今夜-明日)

Day 1-3 (~5/15-17)
- 5 founder seed DM 送信 (yuki manual、 ads/2026-05-14-dm-template.md)
- FIRST30 SUZURI 価格 ¥3,900 適用
- ¥50K ad campaign 開始 (yuki manual、 X Ads、 3 variant)
- 想定: 8 着 (seed 3 + ad 2 + organic 3)

Day 4-7 (~5/18-21)
- ad 30 分おき改善 (CVR、 CPC、 frequency)
- 勝った variant scale (¥1,500 → ¥3,000/day)
- DM follow up (Day 3 trigger)
- Referral coupon live
- 想定: 累計 25 着 (+17)

Day 8-14 (~5/22-28)
- FIRST30 ほぼ消化 (30/30)、 価格 ¥4,900 に戻す
- 100 着への中間チェックポイント
- 想定: 累計 50 着 (+25)

Day 15-20 (~5/29-6/3)
- 残 50 着の追い込み
- TORU500 X クーポン強化
- 想定: 累計 80-100 着 (+30-50)

期待値:
 楽観: 100 着 ✓
 中央値: 60 着
 悲観: 30 着 (これでも Stage 1 半分達成)
```

---

## 6. 利益率の議論 — 「困ってる」 への答え

現状の苦悩: ¥7,800 で margin 27%。 ¥4,900 SUZURI は MU 側 ¥1,400 だけ。 これで 100 年運営できるのか?

**僕の判断:**

```
Stage 1 (今、 0-100): margin より volume。 ¥4,900-¥7,800 帯維持。
   ¥30,000 / 月 純益 (§Cessation) ラインは 
   30 着 / 月 = 1 日 1 着 で達成可能 (margin ¥1,000/着 平均と仮定)

Stage 2 (100-500): margin 段階的に上げる。
   - MUGEN base ¥7,800 → ¥9,800 (era-3 候補)
   - SUZURI Premium tier (United Athle 7.1oz ¥6,800) を導入、 SUZURI 経路の収益性 +
   - margin 30-35%

Stage 3 (500-2000): MA-Atelier で高単価帯
   - MA を ¥35,000-50,000 帯に
   - 5-10 着 / 月 × ¥30,000 margin = ¥150,000-300,000 / 月
   - これが「100 年運営」 の利益源
```

**つまり: 今は margin より顧客獲得。** Stage 2 で margin に方向転換 (現在 retail を上げる)。 Stage 3 で MA で稼ぐ。 これは Apple の price ladder と同じ構造 (iPhone SE → iPhone → iPhone Pro Max)。

---

## 7. 「困った場合」 の安全弁

§Cessation:
> もし 2 ヶ月連続で月次純益 ¥30,000 を下回ったら、 ブログ 1 本書いて閉じる。 No mourning. End.

20 日後の 6/3 時点で **30 着以下** だった場合:
- 価格戦略を抜本的に見直す (¥4,900 を ¥2,900 にしてでも 1,000 着売る or shutdown)
- ad は止める (ROI 確定 0)
- 「失敗の数字」 ブログを 6/3 に出す (Constitution §Cessation の dry run)

20 日後 **50-99 着** だった場合:
- Stage 1 半分達成、 戦略継続
- 100 着までの追加 20 日 (合計 40 日 = 6/23) で達成計画書直し

20 日後 **100+ 着** だった場合:
- Stage 2 へ移行宣言ブログ
- Stanley/Stella STTU788 250gsm を Premium+ tier に追加
- Loopwheeler 連絡先返事あれば MA-Atelier 提案

---

## 8. 今夜実装する 2 つ

1. **`FIRST30` クーポン** — SUZURI 経路の最初 30 着 ¥3,900。 30 着到達で自動 ¥4,900 復帰。
2. **Referral コード** — 全 buyer に code 自動発行、 X / mail で渡す、 使用時 ¥500 off。

実装 → push → /transparency に live で 「FIRST30 残り N/30」 を出します。

---

## 9. 既存の前提を疑う質問たち

- **¥4,900 は本当に最適? もっと下げた方が良いか?** → JiuFlow ¥0 conv の経験から、 価格より「商品力 + LP」 が問題と判断。 ¥4,900 維持。
- **Stage 1 で MA をやる意味があるか?** → ある。 MA は narrative anchor (「1-of-1 が ¥18,000 から始まる brand」 が cred に効く)。 売れなくても表示は維持。
- **クーポンは brand を弱体化させないか?** → FIRST30 のような期間/数量限定はむしろ urgency を強化。 「Stage 1 ローンチ祝い」 として明示する。
- **100 着が達成不可だった場合、 嘘になる?** → ならない。 §11 で「数字を出す」 と決めている、 失敗は失敗として書く。 6/3 にどんな数字でも公開する。

---

*Constitution §11 (numbers + honesty) / §24-v2 / §Cessation に従って書きました。*
*関連: [素材を変えた、 原価も公開する](/blog/fabric-shift) / [仕様+prompt](/blog/spec-and-prompt) / [品質アンケート](/survey/quality)*
*次回更新: 2026-06-03 (Day 20 = 100/20 結果発表日)。*
