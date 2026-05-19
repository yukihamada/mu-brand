# MSA × MU Pass Solana Mint — Design Doc

**Status:** Draft (2026-05-21) — Phase 2 backlog
**Owner:** Yuki Hamada
**Trigger:** Alex Cheng FB (4人目ペルソナ) P0 #1-4 — "NFT claim と wallet flow が page で 見えない、 collection address ない、 anchor 機能 が href=# のまま、 'NFT 言うなら full commit、 言わない なら drop'"

## 0. Decision

**Full commit**, not drop. Reasons:

- /buy と /source の 両方で "MU Pass" / "デジタル会員証" が **value prop の 1/4** を 占めてる
- 既存 page (`/pass`, `/dao`, anchor card) が NFT 前提 で 書かれて いる
- crypto-native customer (Alex みたい な 海外 buyer 想定 ¥9,800×3) は **proof of on-chain でなければ 買わない**
- drop すると "wearmu = Japan only T-shirt brand" に 戻る = TAM が 急縮小

ただし v1 は **段階的 commitment** で 進める (下記 Phase)。

## 1. 何 を on-chain で 保証 する か

| 約束 | v1 (DB only) | v2 (on-chain) | 違い |
|---|---|---|---|
| 「Tシャツ買った人 = MSA メンバー」 | wearmu DB lookup | NFT 所有 = メンバー | DB なら サービス 死で 失う、 NFT なら 永続 |
| 「First 100 lifetime」 | DB attribute | NFT trait `is_charter: true` | DB は 改ざん可、 trait は 永久 |
| 「108枚 cycle で 永久終了」 | DB count | Metaplex `max_supply: 108` | DB は 後追加可、 max_supply は ハードキャップ |
| 「投票権 1着=1vote」 | DB join | NFT-gated snapshot vote | DB は ホスト依存 |

v1 で 「DB だけど 嘘 つかない」、 v2 で 「on-chain で 永久 担保」。 段階移行。

## 2. mint 戦略

### 候補

| 案 | コスト | UX | tradeoff |
|---|---|---|---|
| A. Metaplex Candy Machine v3 | 設定 1日 + ガス | wallet 必要、 mint button | crypto 向き、 一般客 friction |
| B. Compressed NFT (Bubblegum) | 1日 + 安価 | wallet 必要 (claim) | safer at scale (1000s) |
| **C. 後付け claim 方式** | **2-3日** | **Stripe 後 email → wallet 入力 で mint** | **最良 UX、 crypto も 一般も OK** |
| D. token-2022 with extensions | 設定 2日 | wallet 必要 | metadata 制約 |

**Pick C**. Stripe checkout に wallet 入力 を 入れない (一般客 を 排除 しない)、 購入後 メール で 「wallet 入力 → mint」 リンク を 送る。 入力 した 人 だけ on-chain mint、 残り は DB だけ で MSA 機能 する。

### C の flow

```
Stripe checkout
  ↓ webhook
wearmu DB に entry 追加 (msa_member: true, mint_tx: NULL)
  ↓ メール send (1)
  "Tシャツ ありがとう。 wallet 持ってる人は ここから NFT 受取り"
  → /claim/msa/<token>
    ↓ wallet 入力
  Solana Metaplex mint to wallet
    ↓ webhook
wearmu DB.mint_tx 更新

未claim の人は DB only で MSA 動作 (zip DL 等)。
v3 で 「DB-only 期限 6ヶ月、 期限 切れたら mint 必須」 等 で 移行加速 可。
```

## 3. 必要 な on-chain artifact

### Collection NFT

```
Name: MU Pass
Symbol: MUPASS
Description: Membership badge for MU Source Access (MSA).
             One per wearmu T-shirt purchase. Solana mainnet.
Image: collection.png (square art, MU logo + cycle ring)
External URL: https://wearmu.com/pass
Royalty: 5% (secondary sale, goes to §28 community pool)
Update authority: Yuki (rotate to MU multisig in Phase 4)
```

### Item NFT (per shirt)

```
Name: MU Pass #N (drop_num)
Attributes:
  - cycle: 1-108
  - drop_num: 1-N
  - temperature_c: <seed value at hour of generation>
  - moon_phase: 0-7
  - shirt_design_sha256: <anchor>
  - is_charter: bool (First-100 only)
  - msa_tier: 1 (Phase 2 starts Tier 1 only)
```

`is_charter` が **First-100 lifetime perk** を on-chain 化 する。

### Verifiable anchor

既存 page の `🔗 proof of birth / SHA-256 anchor` を 実装:

```rust
async fn record_anchor(product_id: i64, sha256: [u8; 32]) -> Result<Signature> {
    // Submit memo tx to Solana mainnet
    // memo = `mu-anchor:${product_id}:${hex(sha256)}`
    // Returns tx sig for /buy "🔗 proof of birth" link
}
```

`/api/anchor/:id` returns `{ "tx": "solscan_link", "sha256": "...", "minted_at": "..." }`.

これ で /buy の `product-anchor-link` の `href="#"` が **本物 の solscan link** に なる。 Alex P0 #3 解決。

## 4. /pass ページ 必須 表記

現状 /pass が 何書いて あるか 未確認だが、 Alex P0 #2 で 「collection address、 program、 royalty が どこにも ない」 と 指摘。 /pass 更新 必須:

```
Collection: <mint address>  ← solscan link
Program: Metaplex Token Metadata (TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA)
Symbol: MUPASS
Royalty: 5%
Standard: Metaplex Token Metadata (or Bubblegum compressed)
First-100 trait: `is_charter: bool` (on-chain attribute)
Floor (Magic Eden): <link>
Listings (Tensor): <link>
```

## 5. Phase 順序

| Phase | When | Deliverable |
|---|---|---|
| 1 | 2026-05-31 (既 commit) | trio リポ DL end-to-end (DB-only MSA) |
| **2** | 2026-06-15 | **Collection NFT mint + Item NFT for First-100 + /claim flow + anchor tx** |
| 3 | 2026-06-30 | Magic Eden / Tensor listing + royalty config |
| 4 | 2026-07 | DAO 投票 page live (snapshot from NFT holders) |

## 6. v1 page copy 修正 (今 PR で 部分対応)

短期 で blocker 解消する 表現変更:

| 場所 | Before | After |
|---|---|---|
| /buy 4つ受け取る ②会員証 | "MU Pass NFT / Solana · 永久・剥奪不可" | "MU Pass デジタル会員証 / 買えば一生あなたの · 譲渡 OK" (済) |
| /buy 詳細 details | "デジタル会員証 (NFT) + 投票権" | "デジタル会員証 + 投票権 (Solana 上で on-chain mint 予定、 v1 は DB 発行 → claim flow で wallet 移行可)" (済) |
| /source description | "NFT もウォレットも要らない、 メールだけ" | "MSA 認証 は メールベース、 ウォレット不要 (Solana NFT 会員証 は 別途 mint 予定 → /pass)" (済) |
| /pass | unknown 現状 | TODO: collection address / program / royalty を明記、 v2 deploy 後 |

## 7. cost estimate

- Solana mainnet fee: **mint per Item NFT** ≈ $0.001 (rent-exempt minimum + tx fee)、 100名 で 約 $0.10
- Collection NFT 1回 mint: ≈ $0.50 (大きい metadata account)
- 開発工数: Candy Machine 設定 1日 + claim flow 2日 + anchor tx 0.5日 = **3.5日**
- Phantom / Solflare 接続: web3.js Wallet Adapter で 半日
- runtime cost: 月 $5 以下 (Helius RPC free tier)

## 8. 残 リスク

- **wallet 持ってない buyer が "claim しない" まま 終わる** → DB-only で 機能 する ので OK だが、 retention は wallet 持ち の 方 が 高い 想定
- **Magic Eden / Tensor で 即 floor crashing** → 5% royalty + 透明 価格 で 抑制、 完全 防止 は 不可
- **Solana mainnet outage** → 月 1-2 回 ある、 anchor tx は retry queue で 救う
- **DAO 投票 が theatre に なる** → Phase 4 で 実際 の proposal (毎月 §28 community pool 配分) を ship、 voting 実体 を 作る

## 9. 開発 ticket 切り出し

- [ ] Metaplex Candy Machine v3 設定 (collection.png + metadata template)
- [ ] `/claim/msa/<token>` page (wallet adapter integration、 Phantom/Solflare)
- [ ] Stripe webhook → wearmu DB entry → claim email send
- [ ] `/api/claim/msa/<token>` POST (wallet 受領 → mint 実行 → DB 更新)
- [ ] `/api/anchor/:product_id` (Memo program に sha256 を 書く + /buy link 接続)
- [ ] /pass ページ refresh (collection address / royalty / Magic Eden link)
- [ ] /dao ページ live (snapshot 経由 vote)

## 10. やらない こと

- **Stripe checkout 内 で wallet を取る** — 一般客 を 排除 する、 friction 増、 Alex も 「Stripe のまま で OK」 と 言って る
- **無料 mint button** — bot abuse、 wearmu purchase が gate
- **Solana 以外 への migration** (Ethereum / Base) — Constitution §X で Solana を 約束 してる ので 変更 重大、 別途 discussion
