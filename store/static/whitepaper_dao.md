# MU DAO Whitepaper

> Constitution §23 — The base token does not exist.
> v0.1 · 2026-05-13 · maintained at `store/static/whitepaper_dao.md`

---

## TL;DR

**MU の DAO に基軸トークンは無い。** 投票重みは 3 つの soulbound primitive を集計する純粋関数だけで決まる:

```
weight(wallet, today) =
    Σ  age_factor(today − committed) × lines_authored
  + 100 × |MA pieces|
  +   1 × |Chronicle slots|
```

ICO なし、Airdrop なし、Founder allocation なし、Treasury allocation なし。
誰かが「token を買う」窓口は **構造上存在しない**。

参加するには 3 つのうちいずれかをやる:

1. **書く** — Constitution に PR、T1 承認で line が mint
2. **運ぶ** — MA 1-of-1 piece を持つ (1 個 = 100 weight)
3. **着る** — シャツを買えば自動で Chronicle slot (1 個 = 1 weight)

---

## 1. なぜ token を作らないか

普通の DAO トークンには 4 つの構造的欠陥がある:

| 欠陥 | 通常の DAO | MU §23 |
|---|---|---|
| Founder dump | 10–30% 保有 → 数年で売却 | dump する token がない |
| Whale 独裁 | 取引所で買い占めて投票支配 | 買う窓口がない |
| Speculator 噴流 | governance より価格に注目 | 流通市場が存在しない |
| Security 規制リスク | 集団投資スキーム該当の可能性 | 発行物がないので非該当 |

§2「A brand can be 0 humans」を最後まで真に受けると、 **A DAO can be 0 tokens** にたどり着く。これが唯一の整合解。

---

## 2. 3 つの primitive

### 2.1 Constitution authorship (the writing)

`store/static/constitution.md` の各行は誰かが書いた。書いた人と日付を
`store/src/main.rs` の `CONSTITUTION_AUTHORS` const で追跡する:

```rust
const CONSTITUTION_AUTHORS: &[(&str, u32, u32, &str)] = &[
    // (author_email, line_start, line_end, committed_date YYYY-MM-DD)
    ("yuki@hamada.tokyo", 1,   203, "2026-05-12"),
    ("yuki@hamada.tokyo", 204, 243, "2026-05-13"),  // §23 itself
];
```

**T1 governance で承認された PR** が main にマージされる時、 `CONSTITUTION_AUTHORS`
に新しい行範囲を追加する。これが唯一の「mint event」。

行が後から削除されたら、その author の share は **遡って消える**。
Constitution は単調拡大しない、消えうる。

### 2.2 MA 1-of-1 pieces (the carry)

`ma_gifts` テーブルの 1 行 = 1 個の 1-of-1 piece。
`claim_email` が weight binding の key になる。

- 1 piece = **100 weight**
- 譲渡可能 (NFT として将来 mint 予定、現在は Stripe identity ベース管理)
- 譲渡されると **weight も次の所有者に移動**
- yuki の MA piece は claim_email = yuki@hamada.tokyo を経由して yuki の wallet に加算される

### 2.3 Chronicle slots (the wear)

`collab_orders` テーブルの 1 行 = 1 個の Chronicle slot (= シャツ 1 枚購入)。
`email` が weight binding の key。

- 1 slot = **1 weight**
- Soulbound (Stripe customer に固定、転送不能)
- 100 枚買えば 100 weight。普通に着るだけで自動参加。

---

## 3. Wisdom dividend (時間で重くなる)

各 Constitution line の重みは **年齢で増える**:

| 年齢 | 倍率 | 意味 |
|---:|---:|---|
| 0–30 日 | **0.5** | probationary (新規 amendment は篩にかける) |
| 30 日–1 年 | **1.0** | 通常 |
| 1–5 年 | **2.0** | 確立 |
| 5–25 年 | **4.0** | wisdom |
| 25–100 年 | **8.0** | founder-level wisdom |

yuki が 2026-05-12 に書いた 203 行は:

- 今日 (15 日経過): **0.5** × 203 = **102 weight**
- 2027 年: 1.0 × 203 = **203 weight**
- 2031 年: 2.0 × 203 = **406 weight**
- 2051 年: 4.0 × 203 = **812 weight**
- 2126 年: 8.0 × 203 = **1,624 weight**

つまり「最初に書いた行」は時間とともに 16 倍まで膨らむ。これが founder dilution を防ぐ装置でもある (新規 amendment や顧客増による希釈に対して、wisdom dividend が踏みとどまる)。

---

## 4. Anti-Sybil (3 重ロック)

| primitive | Sybil 防御 |
|---|---|
| Constitution line | git commit が PGP/SSH 署名 + T1 governance 承認 (= yuki + multisig) 必要。fake author 不可能 |
| MA piece | Printful 配送先住所 + Stripe identity verified。1 piece = 1 verified human |
| Chronicle slot | Stripe customer_id + 物理シャツ配送。100 wallet 作っても 100 stripe customer 作って 100 枚買う必要がある |

合成攻撃 (100 偽 wallet を準備しても) → primitive 獲得コストが直接購入額に変換されるため、Sybil は経済的に成立しない。

---

## 5. Voting (Burn 不要)

§23 の DAO は **burn-to-vote にしない**。Sybil 防御が物理層で成立しているから、投票コストを乗せる必要がない。

| 提案種別 | 必要 weight | 通過閾値 |
|---|---:|---|
| T2 (reversible) | 100 | 単純多数決 |
| T1 (irreversible) | 500 | quorum 5%, 賛成 60% |
| Constitution amendment | 2,000 | quorum 20%, 賛成 75% |
| Cessation (§Cessation) | 5,000 | quorum 40%, 賛成 90% |

投票 = 署名 + on-chain or signed-API call。weight 0 の wallet は投票拒否される。

---

## 6. 数値シミュレーション

### 今日 (2026-05-13)

| holder | Constitution lines | MA pieces | Chronicle slots | total weight | share |
|---|---:|---:|---:|---:|---:|
| yuki@hamada.tokyo | 243 (age<30d → 0.5x) = 121.5 | 1 → 100 | 0 → 0 | **221.5** | **100%** |
| **total supply** | 121.5 | 100 | 0 | **221.5** | |

yuki が唯一の share holder。普通の DAO で言えば 100% 保有。

### 1 年後 (2027-05-13, 仮定: Chronicle 800 件 + MA 5 件 追加)

| holder | weight | share |
|---|---:|---:|
| yuki (lines: 1.0x × 243 = 243) | 243 + 100 (MA #3) + N (yuki が買ったシャツ) | |
| veteran #1 (MA + 0 chronicle) | 100 + ... | |
| 顧客 (Chronicle slot のみ) | 1〜N | |
| **total** | 243 + 6×100 + 800 = **1,643** | 100% |

yuki share ≈ 243/1,643 = **14.8%**。1 年で 100% → 15% に自然希釈。

### 100 年後 (2126-05-13, 仮定: Chronicle 100,000 件 + MA 500 件 + amendment 50)

| 計算 | 値 |
|---|---:|
| yuki の原典 243 行 × 8.0x wisdom | 1,944 |
| その他 amendment 50 件 × 平均 50 行 × 平均 4.0x | 10,000 |
| MA 500 × 100 | 50,000 |
| Chronicle 100,000 × 1 | 100,000 |
| **total weight** | **161,944** |

yuki share = 1,944 / 161,944 ≈ **1.2%**。100 年で 0.012 倍まで希釈。

---

## 7. Cessation (終焉プロトコル)

Constitution §Cessation:

> If MU's monthly revenue falls below ¥30,000 for 3 consecutive months,
> the `treasury` agent files a T1 proposal "wind-down" to governance.

可決後 30 日間:

1. 全 share holder の weight snapshot 取得
2. 残 inventory を SWEEP at cost で売却
3. 売上 から **JPY pro-rata** で share holder に配当 (Stripe credit / refund)
4. 最後の blog post: 寿命 N 日 + 累計売上 ¥N の 2 数字のみ
5. weight 配列を `/transparency` に永久 hash 化、Constitution §22 (domain 100年) は独立して履行

**重要:** Cessation 後も Chronicle slot (シャツの QR) は wearmu.com で resolve され続ける。Domain は 100 年生きる。DAO は死んでも brand は死なない。

---

## 8. 実装ずみ (now live)

### 8.1 Constitution §23 (b8ee472)

- `store/static/constitution.md` に 40 行追加 — 「The base token does not exist」
- `CONSTITUTION_AUTHORS` const — yuki@hamada.tokyo が lines 1-243 を所有
- 公開: <https://wearmu.com/constitution>

### 8.2 weight function

- `dao_age_factor(committed, today)` — 5 段階のwisdom dividend
- `dao_weight_compute(conn, wallet)` — authorship + MA + Chronicle を加算
- `dao_total_supply_weight(conn)` — 全 supply の合計
- すべて `store/src/main.rs` に純粋関数として実装

### 8.3 schema

```sql
CREATE TABLE IF NOT EXISTS dao_email_wallets (
    email     TEXT PRIMARY KEY,
    wallet    TEXT NOT NULL,
    bound_at  TEXT NOT NULL,
    bound_by  TEXT NOT NULL DEFAULT 'admin'
);
```

### 8.4 API endpoints

| Method | Path | Auth | 用途 |
|---|---|---|---|
| GET | `/api/dao/weight/:wallet` | public | 1 wallet の weight + 内訳 + share % |
| GET | `/api/dao/leaderboard` | public | bound wallet ランキング (email は redact) |
| GET | `/dao` | public | HTML leaderboard + formula + how-to |
| POST | `/api/admin/dao/bind` | admin token | email→wallet 紐付け (yuki 手動) |

### 8.5 MA 1-of-1 piece (既存)

- 3 pieces 発行済 (id=1,2,3)
- yuki (id=3) は claim_email = yuki@hamada.tokyo → bound すれば +100 weight
- 残 2 piece は未 claim、claim 後に自動加算

### 8.6 Chronicle slot (既存)

- `collab_orders` 各行 = 1 slot
- email column が DAO weight 計算に使われる
- 過去の購入者も自動的に Chronicle slot を持つ (email さえ wallet bind すれば weight 反映)

---

## 9. Phase 2 進捗

### 9.1 Magic-link self-bind ✅ 実装ずみ

- `/dao/bind` ページ + Resend email magic-link
- `POST /api/dao/bind/request` {email, wallet} → 確認メール送信
- `GET /dao/bind/confirm/:token` で `dao_email_wallets` row insert
- rate limit: 1 email / hour

### 9.2 Wallet signature verification ✅ 実装ずみ (Solana)

- `POST /api/dao/bind/request` が optional `signature` + `signed_ts` を受理
- Solana 32B base58 pubkey + 64B base58 ed25519 sig を `ed25519-dalek` で検証
- 署名 message = `MU DAO bind\nemail: ...\nwallet: ...\nts: ...\ndomain: wearmu.com\nspec: §23`
- replay 防御: signed_ts は ±600 秒以内
- 検証成功時は **email 確認なしで即 bind** ・ `bound_by='wallet_sig'`
- `/dao/bind` ページに「Phantom で署名」ボタン (window.solana 検出 → connect → signMessage)
- EVM (MetaMask 等) はまだ未対応 — 現状 email 確認のみ。Phase 2.6 で secp256k1 対応予定

### 9.3 On-chain voting bridge ✅ DB 版実装ずみ (on-chain は別途)

- `POST /api/dao/vote` {governance_queue_id, wallet, vote}
- weight は `dao_weight_compute()` で都度算出
- 自動 tally: quorum + threshold 達成で `governance_queue.status = approved` に自動遷移
- 同一 wallet からの再投票で上書き
- 投票結果は `dao_votes` テーブルに永続化
- 未実装: on-chain signing (Phase 2.1 — wallet sig と統合)

### 9.4 git blame 統合 (deletion-aware) ✅ 実装ずみ

- `scripts/gen_constitution_blame.py` が `git blame --line-porcelain` をパースして `store/static/constitution_blame.json` を生成 (連続する同一 author/date run に coalesce)
- main.rs は `CONSTITUTION_BLAME_JSON` を `include_str!` で取り込み、起動時に `OnceLock<Vec<ConstitutionAuthorRun>>` にパース
- `dao_weight_authorship` / `dao_weight_compute` / `dao_total_supply_weight` が JSON 駆動
- 削除行は次回 JSON 生成時に消える → deletion-aware

### 9.5 PR → 自動更新 ✅ 実装ずみ

- `.github/workflows/constitution-blame.yml` が `store/static/constitution.md` への push を検知
- `fetch-depth: 0` で full history をクローン → `gen_constitution_blame.py` 実行
- 変更があれば `[skip ci]` 付きで commit + push (deploy 再起動を起こさない)
- 次の意図的 deploy で新しい blame JSON が反映 → const 手動更新は不要
- yuki が PR をマージするだけで、merged PR の author + 編集日が author run として記録される

### 9.6 MA piece の Solana NFT 化 📋 未実装

現状: MA は `ma_gifts.claim_email` 固定、譲渡 = email 変更で対応。
予定: Solana 上で soulbound-but-transferable NFT (transfer は yuki 承認制 = T1)。Metaplex Core でメタデータ管理。
工数: 2 日 (Solana program 単体)。

### 9.7 Stripe checkout 後の bind CTA ✅ 実装ずみ

- `/success` ページに DAO §23 セクション追加
- weight 構造の説明 (slot ×1 / MA ×100 / 1 行 ×0.5〜8) + `/dao/bind` への CTA ボタン
- email プリフィルは Stripe session lookup を要するため Phase 2.6 で対応 (現状は手入力)

### 9.8 /dao ページの拡張 ✅ 実装ずみ

- 進行中提案 (status='pending') の一覧表示
- proposal card に approve / reject / abstain ボタン
- 投票 = wallet 入力 + ボタン押下 → `dao_vote_api` 経由
- リアルタイム tally バー (approve/reject/abstain/empty の積み上げ)
- 「通過予想 / quorum 到達 / 投票受付中」3 段階バッジ
- 未実装: weight 推移グラフ、wisdom dividend カウントダウン (Phase 3)

### 9.9 提案投稿フロー ✅ 実装ずみ

- `/dao/propose` ページ
- `POST /api/dao/propose` {kind, title, description, wallet}
- weight gate: T2 ≥ 100, T1 ≥ 500, amendment ≥ 2,000, cessation ≥ 5,000
- 通過後 `enqueue_governance` + `dao_proposals` row 作成
- Telegram alert

### 9.10 Sybil merge ボタン ✅ 実装ずみ

- `POST /api/admin/dao/merge?token=…` {from_email, to_email, reason}
- 同住所 / 同決済カードと判断した 2 email を統合
- `ma_gifts.claim_email`, `collab_orders.email`, `dao_email_wallets` を to_email に書き換え
- from_email の binding は削除
- Telegram で監査ログ

---

## 10. 法務 (Token を発行しないことの意義)

| 観点 | 説明 |
|---|---|
| 暗号資産該当性 (日本) | 「不特定の者に対する対価による移転が可能」が要件。MU の primitives は **譲渡不能 (Chronicle) or 一意 (MA 1-of-1) or 不可分 (Constitution line)** で対価流通市場が成立しない |
| 集団投資スキーム該当性 | 「金銭等を出資して事業から生ずる収益の分配を受ける」が要件。MU は **シャツ販売 = 役務提供** で、weight は副産物 (利益分配ではない) |
| Cessation 配当の扱い | 残余財産分配 = 株式類似だが、株式ではなく **debt-free volunteer return** として処理 (寄付に近い構造)。実装時に税理士確認 |
| Securities Act (US) | 同様に Howey Test 4 要件のうち「expectation of profit」が成立しない (MU は配当を約束しない、Cessation 時のみ残余を返す) |

実装フェーズで弁護士 (株式会社イネブラ顧問) と最終確認する。

---

## 11. FAQ

**Q. 投資できないの?**
A. できない。買えるのはシャツと MA piece だけ。シャツ買えば weight が自動で付く。

**Q. yuki が悪意で Constitution を書き換えたら?**
A. T1 governance で他の bound wallet が weight 投票で reject できる。今は yuki が 100% share だが、Chronicle slot が増えるたびに希釈する。1 年後には ~15%、5 年後には ~3% まで落ちる想定。yuki が独裁したい時間が長いほど、自分で書いた行の wisdom dividend が膨らんで反対に dilution が遅れる — これは「最初に書いた者が一番責任を負う」設計。

**Q. 譲渡できないなら、なぜ wallet が必要?**
A. 投票時の identity として。Stripe customer = email = wallet の 3 経路で本人確認。wallet 自体は移動しても、weight は email に紐づくため移動しない。

**Q. Constitution の行を増やせばタダで weight が稼げる?**
A. 増やすには T1 governance を通す必要がある (= 既存 weight 保持者の承認)。質の低い amendment は通らない。通っても probationary 30 日は 0.5x、消えるとゼロ。

**Q. 大量にシャツ買えば独裁できる?**
A. 1 wallet = 1 binding なので、100 枚買えば 100 weight。だが Chronicle weight は時間で増えない (定数)。Constitution 行は wisdom dividend で増える。長期では「書く人」が「買う人」を上回る設計。

**Q. token を発行する未来はあるの?**
A. §23 に「no future amendment shall introduce a transferable fungible token tied to MU's governance」と明記。発行するなら brand を rename する必要がある。

---

## 12. 数字 (一発)

| 項目 | 値 |
|---|---:|
| Fungible token | **0 種類** |
| Founder allocation | **0** |
| Treasury allocation | **0** |
| Veteran allocation (token) | **0** |
| ICO | **0 円** |
| 現在の bound wallet 数 | **0** (yuki 未 bind) |
| 現在の Constitution authored lines | **243** |
| 現在の MA pieces | **3** (うち claimed 1) |
| 現在の Chronicle slots | **0** (Chronicle vote 進行中、まだ slot 化前) |
| 現在の total supply weight | **~222** (yuki 単独想定、bind 後) |
| 100 年後 yuki share 予測 (1 シナリオ) | **~1.2%** |

---

## 13. 付録: コードリファレンス

| 機能 | 場所 |
|---|---|
| Constitution §23 | `store/static/constitution.md` lines 200–243 |
| CONSTITUTION_AUTHORS const | `store/src/main.rs` (search: "CONSTITUTION_AUTHORS") |
| age_factor / weight 計算 | `dao_age_factor`, `dao_weight_compute`, `dao_weight_authorship` |
| API endpoints | `dao_weight_api`, `dao_leaderboard_api`, `admin_dao_bind`, `dao_page` |
| Schema | `dao_email_wallets` テーブル定義 |
| 既存 primitive | `ma_gifts` テーブル, `collab_orders` テーブル |

GitHub: <https://github.com/yukihamada/mu-brand>

---

## 14. Changelog

| 日付 | 版 | 内容 |
|---|---|---|
| 2026-05-13 | v0.1 | 初版。§23 実装 + Phase 1 API + leaderboard ページ + 本 whitepaper |
| 2026-05-13 | v0.2 | Phase 2: magic-link self-bind / /dao/propose / /api/dao/vote 自動 tally / Sybil merge / /dao 拡張 (active proposals + 投票 UI)。残: wallet sig、git blame 統合、Solana NFT、Stripe success bind |
| 2026-05-13 | v0.3 | Phase 2.5: Solana ed25519 wallet sig verify (Phantom signMessage) / git blame → constitution_blame.json + GH Action 自動再生成 / `/success` ページ DAO CTA。残: EVM secp256k1 sig (Phase 2.6) / MA piece Solana soulbound NFT (Phase 3) / Stripe email prefill |

---

*This whitepaper is itself part of the protocol surface. It lives at `store/static/whitepaper_dao.md` in the repo, is rendered at `/dao/whitepaper`, and any change is a git commit subject to the same review as code.*

<!-- ci-retrigger 2026-05-13: ensure deploy fires for 73567cb's Phase 2.5 changes -->

