# MSA Launch Comms — Drafts

**Status:** Draft (2026-05-20) — for Yuki's manual review and send
**Companion to:** [MU_SOURCE_ACCESS.md](MU_SOURCE_ACCESS.md), blog `/blog/2026-05-19-open-source-stop`, blog `/blog/2026-05-20-msa-inside`

This doc contains:
1. Email draft to JiuFlow paying members (161 active)
2. X (Twitter) thread (8 tweets)
3. Discord/Telegram setup checklist
4. MUGEN #71-90 publish picker — runbook + commands

Each section says "**Action**" — Yuki sends/runs after review.

---

## 1. Email to JiuFlow paying members

**Action:** Yuki reviews + sends via wearmu/jiuflow mail infra. Per
`feedback_email_blast_radius.md` — real customer email requires
explicit OK. Don't auto-send.

### To
JiuFlow active subscribers (status=active) — currently 161 members per
[jiuflow_subscribers.md](jiuflow_subscribers.md). Filter out trialing /
past_due / canceled.

### From
`info@enablerdao.com` (Resend, per workspace conventions)

### Subject
`オープンソースをやめました。Tシャツ買うと中身全部見えるようにしました。`

### Body (Japanese, plain text + 1 link)

```
{name} さん、

JiuFlow 使ってくれてありがとうございます。今日は MU のお知らせです。

----

5月19日、yukihamada/* の公開リポジトリ21本を private に落としました。

理由はセキュリティです。Dependabot を全部有効化したら600件の脆弱性
アラートが出てきて、nanobot で実バグ (prompt injection + CORS
reflect) も2件見つかりました。公開で抱え続けるのが重くなった。

代わりに、wearmu のTシャツを買ってくれた人にはソースを読める仕組みを
作りました。「MU Source Access」と呼んでいます。

  https://wearmu.com/source

Tシャツ ¥4,900 で、21リポ全部のzipがダウンロードできます。
最初の100名は、将来追加されるリポも自動でアクセス権が付きます。

JiuFlow を使ってくれている柔術家のあなたには、jitsuflow と
jiuflow 周辺のコードも当然読めるようになります。試合フロー記録、
Cloudflare Worker での state管理、Flutter周りの試行錯誤、全部。

理由と詳細はブログに書きました:
  https://yukihamada.jp/blog/2026-05-19-open-source-stop
  https://yukihamada.jp/blog/2026-05-20-msa-inside

JiuFlow の機能改善も続けます。MSA はそれと別軸で、 OSS への一つの
返し方として作っています。

何か質問あれば返信ください。

Yuki Hamada
Enabler Inc.
```

### Send command

(For Yuki to verify before running.)

```bash
# Dry-run first
cd /Users/yuki/workspace/bjj/jiuflow-ssr  # or wherever the mail script lives
python3 scripts/blast_msa_launch.py --dry-run

# Verify list size, sample 3 messages, then for real
python3 scripts/blast_msa_launch.py --confirm-customer-blast
```

**Hard check:** count must equal 161 ± a few; subject must not contain
"$1" or unresolved template vars; from must be `info@enablerdao.com`.

---

## 2. X (Twitter) thread (Yuki = @yukihamada)

**Action:** Yuki posts manually. Don't auto-post (per
`feedback_x_self_mention.md` — be careful with X automation).

8 tweets, comma-separated for easy paste. **Key:** lead with the
one-liner, not the security story.

---

**Tweet 1/8 (anchor)**

```
オープンソースをやめました。

その代わり、wearmu のTシャツを買ってくれた人だけが、 yukihamada/*
の private リポ21本のソースを全部読めるようにしました。

¥4,900。NFTもウォレットも要らない、メールだけ。

https://wearmu.com/source
```

**Tweet 2/8**

```
やめた理由はセキュリティです。

Dependabot を全部有効化したら、600件 の脆弱性アラートが出てきた。

banto 98 / soluna-web 79 / stayflowapp 55 / nanobot 51 ... 全部
upstream の supply chain 経由で、 公開リポにそれを抱え続ける重さに
耐えられなくなった。
```

**Tweet 3/8**

```
さらに、 自分のコードに実バグも見つけてた。

nanobot #43: /api/v1/chat の session_id を全リクエストで共有して
prompt injection ができる状態
nanobot #42: CORS が任意の Origin を反射

両方 公開issue として 放置していた。 公開issue で書く時点で 攻撃側
にヒントを渡してた。
```

**Tweet 4/8**

```
じゃあOSSは死んだのか? いいえ。

僕はOSSで育ったしOSSに返したい。 ただ「世界中の誰でも読める」極まで
開くのは、 1人 founder には重すぎるコストになった、 というだけ。

代わりに wearmu の Tシャツ買ってくれた人には open する。 そういう
中間の形を作ります。
```

**Tweet 5/8**

```
仕組みはシンプル:

Tシャツ buy → Stripe email → wearmu /source ログイン → 各リポの zip
ボタンが live → 5分有効な署名URLでDL

GitHub アカウント不要。 NFT 不要。 ウォレット不要。
ただメール。
```

**Tweet 6/8**

```
最初の100名は lifetime perk:

これから増える private リポ ぜんぶ 自動 アクセス権。
101名目以降は、 その時点の条件で再加入。

「buy a shirt → get the codebase forever」 です。
```

**Tweet 7/8**

```
中身は何かというと、 14個のプロダクトを1人で並列に動かしている
試行錯誤のコード一式:

trio / kagi / pasha / pon / NOU / claudeterm / jitsuflow / nemotron
/ phishguard / security-scanner / security-education / tsugi /
hato / hypernews / gitnote / flow-anime / Photon / makimaki /
tegata / factlens / thestandard
```

**Tweet 8/8**

```
詳細はブログ2本に書きました:

なぜ閉じたか:
https://yukihamada.jp/blog/2026-05-19-open-source-stop

中身は何か:
https://yukihamada.jp/blog/2026-05-20-msa-inside

買うかどうかの前に、 まず /source の中身見て下さい。
https://wearmu.com/source
```

---

## 3. Discord / Telegram setup

**Action:** Yuki creates the channel + posts the link on /source.

### Recommendation: Telegram over Discord

理由:
- 既存ユーザー (JiuFlow customers, MU buyers) は Telegram親和性高い
  (LINE 文化に近い)
- Yuki は既に Telegram bot を運用済み (@yukihamada_ai_bot 他)
- onboarding が Discord より速い (招待リンク 1クリック)
- 日本の友人にDiscord は最近やや不評気味

### Setup steps

1. Telegram で **「MU Source Access」** という名前の private supergroup を作成
   - description: "Tシャツ買ってくれた人専用。 yukihamada/* のソースを
     一緒に読む場。 Yuki に直接質問 OK。"
2. Pinned message:
   - blog `/blog/2026-05-20-msa-inside` リンク
   - 行動規範 (be kind, no redistribution outside, ask before sharing
     code in other channels)
   - 「困ったら @yukihamada にDM」
3. Invite link を発行 (revoke 可能なもの)、 wearmu /source ページの
   FAQ "Q. プルリクエストは送れる？" 直下に追加
4. Stripe → Telegram 自動招待 は Phase 2 で実装。 まずは手動で
   1人ずつ承認

### Discord代案 (やる場合)

- Server名: **MU Source Access**
- Roles: `msa-member` (T-shirt buyer), `founder` (Yuki)
- Channels:
  - #welcome (rules + onboarding)
  - #general (free chat)
  - #repos (one thread per Tier 1 repo for Q&A)
  - #bug-reports (private)
  - #show-and-tell (members' derivatives)
- 招待: 認証 bot (Stripe email 照合) は Phase 2、 初期は手動

---

## 4. MUGEN #71-90 publish picker

**Action:** Yuki picks winners from contact sheet, runs publish script.

### Step 1 — Review

```bash
cd /Users/yuki/workspace/mu-brand
python3 scripts/mugen_contact_sheet.py --start 71 --end 90 --out /tmp/sheet.html
open /tmp/sheet.html
```

Toggle 白地/黒地 で確認。 各 drop から 1 winner を選ぶ。

### Step 2 — 候補リスト作成

`/tmp/mugen_71_90_winners.txt` に1行1ファイル名:

```
mugen_0071_xxxxxxxx.png
mugen_0072_xxxxxxxx.png
...
mugen_0090_xxxxxxxx.png
```

(計20行、 各 drop につき 1 winner)

### Step 3 — Printful publish

既存の generate.py / scripts/* には MUGEN を1ドロップ ずつ publish
する path がある。 一括 publish の薄いラッパーが必要。

提案: `scripts/publish_mugen_winners.py` (未実装):
- 引数: winners list file
- 各ファイルにつき:
  - Printful files API へ upload
  - mockup generate (黒/白/ベージュ各色)
  - products.db insert
  - SUZURI mirror も同時に (¥4,900 国内発送ライン)

実装は ~1日。 まずは手動で1枚publishして flow確認することを推奨。

### Step 4 — /source ページに「MSA-bundled」表示

publish した SKU には sticker / badge を入れる:
**「Tシャツ買うと /source アクセス権 付き」**

これで Tシャツ単品買い も MSA 経路 buy も 同じ商品で 解決する。

---

## Sequencing

1. **Today (2026-05-20)**: /source page deploy (PR #10 で完了) +
   blog 2本 公開 (PR #5 で完了) → **already done by Claude**.
2. **Tomorrow**: Yuki が MUGEN winners 20枚を選定 + 3枚を手動publish
   (smoke test). Telegram group 作成 + invite link を /source FAQ に追加.
3. **+2-3 days**: X thread post + JiuFlow email blast (dry-run 後).
4. **+1 week**: Phase 2 (/api/source/<repo>/grant 実装) で actual
   zip DL を 1リポ end-to-end でテスト. trio から開ける.
5. **+2-3 weeks**: First-100 counter を live 化 (Stripe webhook で
   count 更新). 残数表示が動的に減るとscarcity演出になる.

---

## What's NOT in this doc (deliberately)

- 価格の議論 (¥4,900 はSUZURI 価格、 ここでは触らない)
- License文面 (MU Source Access License は Phase 2 で書く)
- アフィリエイト (jiufight 100着 は別文脈、 まずはMSA単独で立ち上げる)
- B2B/法人向けプラン (Phase 4+)
