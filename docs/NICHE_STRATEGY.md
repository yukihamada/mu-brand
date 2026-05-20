# MU Multi-Niche Strategy (2026-05)

> 全員 に 売る = 誰 にも 刺さらない。 4 つ の niche に 分けて、 それぞれ 別 budget / 別 entry / 別 KPI で 動かす。
> 中身 は **DB (`mu_niches` テーブル) で 管理** — markdown は 思想 と template だけ、 数値 は SQL から 取る。

## なぜ multi-niche に した か

ad spend ¥150K/月 で 5 ヶ月 → MRR ¥150K で 停滞 = 「commodity ¥4,900 を cold で 売る」 方針 が 失敗。 1 方向 に 集中 する べき か 散らす べき か → **散らす + それぞれ 専用 funnel** が 答え。

LP の 化粧 直し で は 解決 しない 根本 課題:
- brand 認知 ≈ 0
- social proof = 0
- 哲学 narrative は cold には 響かない
- 着用 イメージ が ない

これら を **niche ごと に 別 戦略 で 解決 する** の が 次 の 6 ヶ月。

---

## 4 niches (= `mu_niches` テーブル 初期 seed)

| slug | name | entry_url | hero | 初期 月予算 | MRR 目標 | priority |
|---|---|---|---|---|---|---|
| `bjj` | BJJ tribe | `/bjj/about` | jf-hero | ¥80K | ¥500K | 100 |
| `founder` | Founder Edition (luxury) | `/buy/founder` | hero-on-model | ¥20K | ¥200K | 80 |
| `collab` | B2B Collab (rev share) | `/bjj/about` | jf-hero | ¥0 | ¥100K | 70 |
| `mugen` | MUGEN daily ¥4,900 | `/buy/today` | mugen-hero | ¥0 | ¥50K | 30 |

→ live data: `GET /api/niches` で JSON 取得 (status / 数値 は DB が source of truth)

---

## 1. `bjj` — 直近 動かす 唯一 の hot lane

**why**: Yuki 青帯、 JiuFlow (170+ 道場 SaaS)、 5/24 JIU FIGHT 大会、 関連 audience が **唯一 ある warm pool**。

**入口**: `/bjj/about` (Why BJJ + 4 pillars + creator profile) → `/buy/event` (5/24 SUZURI ¥4,900)

**¥80K/月 配分**:
- 影響者 seed: ¥30K = 3 名 × ¥10K 相当 (T 贈呈 + paid post 依頼)
- ads (Meta + X、 BJJ Japan target、 JiuFlow lookalike): ¥30K
- retargeting (/jiufight/ /buy/event 訪問者): ¥10K
- organic 制作 (TikTok / Reels 動画 撮影): ¥10K

**期待 KPI**:
- 1 ヶ月 で 30-80 着 (¥150K-400K 売上)、 ROAS 2-5x
- 3 ヶ月 で MRR ¥500K、 6 ヶ月 で ¥1-2M

**Yuki action**:
1. 影響者 3 名 候補 を `mu_outreach` に INSERT (status='identified')
2. テンプレ で email 送信 → status='contacted'
3. 返信 来たら status='replied' → T 発送 → status='shipped'
4. 着用 photo 来たら status='photo_received' → /bjj/about の social proof セクション に 載せる
5. 5/24 当日 撮影 + transparency 投稿

---

## 2. `founder` — luxury / press-driven (ads 軽め)

**why**: ¥48,000 1 着 × 年 4 回 = LTV 高い tier。 cold ads には 向かない (CVR 0%) が、 warm retargeting + 著名人 gift + press 露出 で 動く。

**入口**: `/buy/founder` (Loopwheel + 鉱物染料 + NFC + 100年 保証)

**¥20K/月 配分**:
- retargeting のみ (/buy/founder 訪問者): ¥10K
- PR / press outreach の 撮影費用 (jf-hero 系 を 実物 で 再撮影 する 際): ¥10K

**Yuki action**:
- press kit (Hypebeast / Casabrutus / GQ) outreach → outreach kind='press'
- BJJ + tech 著名人 1-2 名 に gift → photo 取得
- 顧問税理士 + 顧問弁護士 で 100年 reserve fund 法的 構成 確定 → LP の `⏳` を `✓` に 差し替え

---

## 3. `collab` — B2B outbound (ads ゼロ)

**why**: cold ads 不要、 道場 / 飲食店 / SaaS 主催 と の 直接 outreach。 在庫 リスク ゼロ で 始められる (Starter ¥0 + 30% rev share)。

**入口**: `/bjj/about` の B2B CTA (`bjj@enablerdao.com` mailto)

**¥0/月**: paid spend なし、 outreach 労力 のみ。

**Yuki action**:
1. 道場 100 軒 リスト (関東 主要 + JiuFlow 既存 170 道場 から うち の team tee 未契約) → mu_outreach に kind='dojo' で INSERT
2. 月 20 軒 ペース で outreach (= 5 ヶ月 で 100 軒 cover)
3. 1 軒 commit → team tee 50 着 × 30% rev share = ¥73K = 道場側 ¥51K + MU ¥22K
4. 5 軒 commit で MRR ¥110K = niche KPI 達成

**飲食 collab (memory)**: kokon.tokyo (焼肉) は 既存 partner。 飲食 業態 で 「店舗 オリジナル T」 = staff tee + 顧客 gift の funnel。

---

## 4. `mugen` — commodity tier (background)

**why**: ¥4,900 daily T = MU の existing 商品 line。 cold ads は 過去 ¥42K で 0 conv = **paid 不向き 確定**。 organic + retargeting で 維持。

**入口**: `/buy/today` (visual cold LP)

**¥0/月 paid**: 認知 上 ある 客 のみ、 ads は しない。

**organic 戦略**:
- 毎日 21:00 JST に X / Instagram で 「今日 の 1 枚」 投稿 (= Yuki の 既存 ルーチン)
- /100 (= 14 日 transparency) で build-in-public 続行 (5/31 まで)

---

## DB schema

```sql
CREATE TABLE mu_niches (
    slug             TEXT PRIMARY KEY,
    name             TEXT NOT NULL,
    audience         TEXT NOT NULL,
    entry_url        TEXT NOT NULL,
    hero_img         TEXT,
    ad_budget_jpy    INTEGER NOT NULL DEFAULT 0,
    target_mrr_jpy   INTEGER NOT NULL DEFAULT 0,
    status           TEXT NOT NULL DEFAULT 'active',   -- 'active'|'paused'|'archived'
    priority         INTEGER NOT NULL DEFAULT 0,
    notes            TEXT,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);

CREATE TABLE mu_outreach (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    niche_slug       TEXT NOT NULL REFERENCES mu_niches(slug),
    kind             TEXT NOT NULL,                    -- 'influencer'|'dojo'|'press'|'partner'
    alias            TEXT NOT NULL,
    contact_url      TEXT,
    contact_email    TEXT,
    status           TEXT NOT NULL DEFAULT 'identified',
    -- 'identified'|'contacted'|'replied'|'agreed'|'shipped'|'photo_received'|'declined'|'archived'
    last_action_at   TEXT,
    notes            TEXT,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);
```

### 操作 例 (CLI / curl)

```bash
# 全 niche 取得
curl https://wearmu.com/api/niches | jq

# 特定 niche の outreach pipeline
curl https://wearmu.com/api/niches/bjj/outreach | jq

# 新規 outreach 候補 を 追加 (sqlite3 直 か 将来 の admin API)
sqlite3 store/mu.db "INSERT INTO mu_outreach
  (niche_slug, kind, alias, contact_url, status, notes, created_at, updated_at)
  VALUES ('bjj', 'influencer', 'A. K.', 'https://instagram.com/...', 'identified',
          'BJJ 紫帯、 関東、 follower 3K', datetime('now'), datetime('now'));"

# status 更新
sqlite3 store/mu.db "UPDATE mu_outreach
  SET status='shipped', last_action_at=datetime('now'), updated_at=datetime('now'),
      notes='5/21 SUZURI 発送 完了'
  WHERE id=N;"
```

---

## Outreach email templates

### A. 影響者 (kind='influencer')

```
件名: MU × JIU FIGHT 5/24 公式 T を 1 着 贈らせて ください

[alias] さん、

MU (wearmu.com) を やってる 濱田 優貴 (BJJ 青帯) と 申します。 練習 で 三田 の 道場
通ってます。

5/24 の JIU FIGHT 大会 公式 T を 150 着 限定 で 作ってます。
[alias] さん に 1 着 贈らせて いただけますか (= 無料、 こちら 負担)。

サイズ + 配送先 教えて いただければ 24 時間 以内 に 発送。
着用 photo を SNS に 投げて いただけたら 嬉しい (= 義務 では ない)。

製品: https://wearmu.com/buy/event
私 の 経歴: https://wearmu.com/bjj/about
直近 の MU + BJJ: [@yukihamada]

無理 なら 1 行 「無理」 で OK です。

Yuki Hamada
Enabler Inc. CEO / ex-Mercari CPO
mail@yukihamada.jp
```

### B. 道場 / 大会 主催 (kind='dojo')

```
件名: MU が trophy tee 50 着 ¥0 で 提供 します

[alias] 様、

MU を やってる 濱田 優貴 (BJJ 青帯) と 申します。

[大会 名 / 道場 名] の trophy tee 50 着 を MU から ¥0 で
ご提供 させて いただきたく ご連絡 しました。

ご提供 内容:
- AI 生成 デザイン + 大会 ロゴ collab tee 50 着 (¥0)
- サイズ XS-XXL 混合、 SUZURI 即発送
- 出場 選手 が 着る photo を MU 公式 で 拡散

MU 側 メリット = 認知 + 着用 photo。 道場 側 = trophy 経費 削減 + 参加者 価値 提供。

製品 例: https://wearmu.com/buy/event
私 の 経歴: https://wearmu.com/bjj/about

ご興味 あれば 1 行 で OK です。 24 時間 以内 に 詳細 ご連絡。

Yuki Hamada
bjj@enablerdao.com
```

### C. B2B partner (kind='partner', 飲食 / SaaS / イベント)

```
件名: MU collab — Starter ¥0 + 30% rev share、 在庫 リスク ゼロ

[alias] 様、

[業態 / 屋号] さん の team apparel / staff tee を、 MU で 即 作れます。

仕組み:
- 初期費用 ¥0、 在庫 リスク ゼロ
- AI + Yuki が デザイン 提案
- 売れた 分 だけ 30% rev share (= [業態] 側 70%)
- SUZURI 即出荷 (¥4,900 / 着)

既存 partner 例:
- JiuFlow (柔術 SaaS) — 大会 / 道場 team tee
- kokon.tokyo (焼肉) — staff tee + 顧客 gift

ご興味 あれば 1 行 「興味あり」 で OK。 24 時間 以内 に 詳細。

Yuki Hamada
mail@yukihamada.jp
```

### D. press / メディア (kind='press')

```
件名: AI が 運営 する アパレル の BJJ 集中 戦略 — 取材 ご検討 いかが

[媒体名] [編集者] 様、

MU は AI と 弟子屈町 の 気温 で 動く アパレル ブランド (株式会社 イネブラ 運営)。
2026 年 5 月 から 日本 の ブラジリアン 柔術 community 集中 戦略 に pivot。

特徴:
- 創業者 = 元 Mercari CPO の 濱田 優貴 (BJJ 青帯)
- 柔術 SaaS 「JiuFlow」 (170+ 道場) と 同 チーム
- 5/24 TOKYO 大会 公式 T 150 着 限定 を 始め、 道場 team apparel、
  ¥48,000 1 着 限定 luxury edition (Loopwheel 14oz + 鉱物染料 + NFC) も 展開

「全員 に 売ろう として 誰 にも 刺さらない」 を、 「BJJ tribe で no.1 に なる」 に
反転 させた 6 ヶ月 戦略。

取材 ご検討 いただけたら 嬉しい です。 wearmu.com / wearmu.com/bjj/about

Yuki Hamada
Enabler Inc. CEO / ex-Mercari CPO
mail@yukihamada.jp
```

---

## 5/24 TOKYO LIVE EVENT — 当日 playbook

### 事前 (5/20-5/23)
- [ ] 影響者 3 名 outreach → 1 名 でも OK → T 発送 + photo 取得
- [ ] /buy/event の social proof section に photo 追加 (届いた 順)
- [ ] ads ¥30K 開始 (/buy/event 直 land、 Meta + X、 BJJ Japan target)
- [ ] retargeting pixel 設置 確認

### 当日 (5/24)
- [ ] 会場 で 着用 photo 撮影 (= Yuki 本人 or アシスタント 1 名)
- [ ] X / Instagram で live 投稿 ×4 回
- [ ] hashtag `#JIUFIGHT524 #MUxBJJ` 統一
- [ ] 当日 売上 を 即 LP に 追加

### 翌日 (5/25)
- [ ] 結果 + 売上 数 を X で transparency 投稿
- [ ] 着用 photo を /bjj/about + /buy/event に 追加
- [ ] 残 在庫 で 「5/31 まで」 緊急性 維持
- [ ] retargeting 配信 開始 (買って ない 訪問者 へ)

---

## 6 ヶ月 milestone

| month | bjj | founder | collab | mugen | total MRR |
|---|---|---|---|---|---|
| M1 (5月) | ¥150K | ¥0 | ¥0 | ¥30K | ¥180K |
| M2 (6月) | ¥250K | ¥48K (1着) | ¥30K | ¥30K | ¥358K |
| M3 (7月) | ¥350K | ¥48K | ¥70K | ¥30K | ¥498K |
| M6 (10月) | ¥1M | ¥150K | ¥300K | ¥50K | ¥1.5M |

これ ら の 数値 は **`mu_niches.target_mrr_jpy` で track**、 月末 に actual を 比較。

---

## 私 (Claude) が やった / やれる (= このPR + 後続)

- [x] /buy/event v2 — jf-hero + BJJ tribe credibility + product closeup
- [x] /bjj/about — MU × BJJ strategic LP (4 pillars + creator profile + B2B CTA)
- [x] /jiufight/ 仕上げ — jf-hero + countdown + 全 SUZURI 即購入 化
- [x] mu_niches + mu_outreach DB schema + 4 niche seed
- [x] /api/niches + /api/niches/:slug/outreach 公開 API
- [x] このdoc (NICHE_STRATEGY.md) — multi-niche + DB-driven、 中村兄弟 削除

## Yuki が やる しか ない

- [ ] 影響者 3 名 候補 を mu_outreach に INSERT + email 送信
- [ ] 道場 5 件 / 100 軒 を mu_outreach に INSERT + email 送信
- [ ] 5/24 当日 撮影
- [ ] ads ¥80K 投下 (Meta / Google 操作)
- [ ] PR press 配信 (Hypebeast / Casabrutus / GQ / PR TIMES)
- [ ] 顧問税理士 + 顧問弁護士 → Founder reserve fund 法的 構成
- [ ] 国税庁 → 適格請求書 発行事業者 登録
