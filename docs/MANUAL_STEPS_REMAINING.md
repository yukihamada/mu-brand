# 残り — yuki の手動 1 回作業（私には不可能な作業）

作成: 2026-05-11

## 1. ウォレット作成 + Fly secrets 置換

**今は PLACEHOLDER で動いてる**（webhook 401 を返すだけ）。本番動作には実値が必要。

### Solana ウォレット (MU_SOL_RECIPIENT)
```bash
# Phantom Wallet (https://phantom.app) または Solflare で受領用 wallet を作成
# 公開アドレス (base58, 32-44字) を取得
fly secrets set MU_SOL_RECIPIENT=<base58_address> -a mu-store
```

### Ethereum ウォレット (MU_ETH_RECIPIENT)
```bash
# MetaMask (https://metamask.io) で受領用 wallet を作成
fly secrets set MU_ETH_RECIPIENT=0x...... -a mu-store
```

> ⚠️ Hardware wallet (Ledger / Trezor) を強く推奨。custodial で良いなら Coincheck / bitFlyer の受領アドレスでも可。

---

## 2. Helius webhook 登録

https://dashboard.helius.dev/ にログイン → Webhooks → Create Webhook

```
Webhook URL:        https://mu-store.fly.dev/api/webhook/helius
Webhook Type:       Enhanced Transaction
Account Addresses:  <上記の MU_SOL_RECIPIENT>
Transaction Types:  TRANSFER, ANY
Authentication:     Header   key=Authorization   value=<生成した秘密文字列>
```

dashboard で生成した値を Fly に：
```bash
fly secrets set HELIUS_WEBHOOK_AUTH=<helius_dashboard_secret> -a mu-store
```

---

## 3. Alchemy webhook 登録

https://dashboard.alchemy.com/ → Notify → Create Webhook

```
Webhook Type:    Address Activity
Chain:           Ethereum Mainnet
Webhook URL:     https://mu-store.fly.dev/api/webhook/alchemy
Addresses:       <MU_ETH_RECIPIENT>
```

dashboard が **Signing Key** を返すので：
```bash
fly secrets set ALCHEMY_WEBHOOK_SIGNING_KEY=<alchemy_signing_key> -a mu-store
```

---

## 4. Stripe Identity webhook 登録

https://dashboard.stripe.com/webhooks → 新規 endpoint

```
Endpoint URL:    https://mu-store.fly.dev/api/webhook/stripe-identity
Events:          identity.verification_session.verified
                 identity.verification_session.requires_input
                 identity.verification_session.canceled
```

webhook signing secret を：
```bash
fly secrets set STRIPE_IDENTITY_WEBHOOK_SECRET=whsec_... -a mu-store
```

（既存の `STRIPE_WEBHOOK_SECRET` をフォールバックとして読むので、別にしなくても動く）

---

## 5. NOUNS DAO discourse 事前スレッド (任意 / 推奨)

discourse.nouns.wtf にアカウント作成 → サインイン →
カテゴリ **NounsDAO Proposals → Pre-Proposal Discussion** で新規スレッド

**タイトル**:
```
MU × NOUNS — autonomous AI fashion brand, 10% to treasury, zero ETH requested
```

**本文**: `/Users/yuki/workspace/mu-brand/docs/nouns/03_discourse_post.md` の中身をそのまま貼付

**Discord** にも投げる場合:
- 招待 URL: https://discord.gg/nouns
- 投稿先: `#ideas` または `#proposals-discussion`
- 本文: `/Users/yuki/workspace/mu-brand/docs/nouns/02_discord_short.md`

候補プロポーザル本体は **nouns.camp** にウォレット接続して提出（discourse の反応を 1-2 週間見てから）。

---

## 6. Wave 2 メディアフォーム（私は失敗）

### Fashionsnap (form)
- URL: https://www.fashionsnap.com/about/contact/
- ✗ 私の curl 試行は **invisible CAPTCHA** で蹴られた
- → yuki がブラウザで開いて、`/Users/yuki/workspace/mu-brand/docs/press/emails_overseas/wave2/07_highsnobiety.txt` の本文をコピペ。**category は「プレスリリース」を選択**

### Highsnobiety (form 廃止、sales@のみ)
- 編集部直の email は非公開。**LinkedIn で editor を一人見つけて DM** が現実的
- 候補: Editor-in-chief は **Steve Carrell** ではないので、サイトの footer で現編集者名を確認

### Business of Fashion (form)
- URL: https://www.businessoffashion.com/contact-us/
- ✗ サイトは bot を 403 で弾く
- → ブラウザで開いて貼付。本文は `wave2/08_bof.txt`

### Dazed
- ✅ **私から partnerships@dazedmedia.com に送信済み** (msg `19e1526a3f159fe0`)
- editorial は **editorial@dazeddigital.com** が一般的だが未確認 — partnerships からの forward を待つ

---

## 7. 配信実績 (現時点)

| 媒体 | 送付済み | msg_id | ステータス |
|---|---|---|---|
| 家入龍太 | ✅ 5/11 | 19e149793e96fa4b | 返信なし |
| THE BRIDGE | ✅ 5/11 | 19e14a7e47e890aa | 返信なし |
| The Verge | ✅ 5/11 | 19e14e757da80bd9 | 返信なし |
| TechCrunch | ✅ 5/11 | 19e14e75cffe338a | 返信なし |
| WIRED | ✅ 5/11 | 19e14e760c2e2e3a | 返信なし |
| Hypebeast | ✅ 5/11 | 19e14e768dfd3fc8 | 返信なし |
| CoinDesk | ✅ 5/11 | 19e14e76f47065dd | 返信なし |
| Dezeen (submit@) | ❌ bounce | — | アドレス無効 |
| Dezeen (tips@) | ✅ 5/11 | 19e14ec5b12fbe07 | 返信なし |
| Dazed (partnerships) | ✅ 5/11 | 19e1526a3f159fe0 | 返信なし（今送ったばかり） |

**7 日後 follow-up** は `docs/press/emails_overseas/followup_day7.txt` のテンプレで自動化可能（私がスレッド単位で reply 送信）。

---

## 8. テスト用本番疎通確認（webhook 登録後にやる）

各 dashboard で「Send test webhook」を押すと、私が書いた auth/signature 検証が走る。期待値：
- Helius test → 401 (auth ヘッダー違うため、書いた auth が活きてる証拠)
- Alchemy test → 401 (HMAC 不一致、書いた検証が活きてる証拠)
- Stripe test → 200 (test event の type が `identity.*` でない場合は 200 with `{"skipped": "..."}`)

real webhook が動くと、Fly の logs で：
```
[helius] confirmed ref=... product_id=... sig=... credited=... expected=...
[fulfill] Printful OK ref=... order_id=...
[fulfill] Resend OK ref=... → email@buyer
```
が見える。

---

## 9. 順番（推奨）

```
Day 0 (今日):    ✅ Phase 3 コード deploy 済み, secrets placeholder, Dazed メール送信済み
Day 1:           ↑ ウォレット作成 (Phantom + MetaMask) → MU_SOL_RECIPIENT / MU_ETH_RECIPIENT 上書き
Day 1:           ↑ Helius webhook 登録 + HELIUS_WEBHOOK_AUTH 上書き
Day 1-2:         ↑ Alchemy webhook 登録 + ALCHEMY_WEBHOOK_SIGNING_KEY 上書き
Day 2:           ↑ Stripe Identity webhook 登録 + STRIPE_IDENTITY_WEBHOOK_SECRET 上書き
Day 2:           ↑ Wave 2 ブラウザ手動 (Fashionsnap / BoF)
Day 3:           ↑ NOUNS discourse pre-thread + Discord post
Day 7 (5/18):    返信なし媒体に followup_day7.txt のテンプレで reply
```

このリストが終われば、クリプト決済 (USDC/SOL/ETH) は完全自走で実顧客に開けます。
