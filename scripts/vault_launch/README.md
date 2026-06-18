# Vault launch — asset map

3 つのオーディエンスに、3 つの違うチャネル+メッセージで届ける。

| オーディエンス | 数 | チャネル | アセット |
|---|---|---|---|
| **既存購入者** | 5 | 個別 DM (X / LINE) | [`dm_5_holders.md`](dm_5_holders.md) |
| **/you 無料 waitlist** | 21 | 一斉 email (Resend) | [`waitlist_email_*`](waitlist_email_body.txt) + [`send_waitlist.py`](send_waitlist.py) |
| **新規** | ∞ | X 投稿 (A1〜A4) | [`x_posts.md`](x_posts.md) |

## 既存購入者へのDM (5名)

`dm_5_holders.md` 内に各人 (MU-AEBD / MU-S2HZ / MU-EG39 / MU-ST6G / MU-LWJB) 向けの個別テンプレあり。
全員に同じ文面を送らない。X DM or LINE で1人ずつ。

## /you waitlist への email (21名)

本番送信:
```bash
flyctl ssh console -a mu-store
cd /app && DB_PATH=/data/products.db python3 scripts/vault_launch/send_waitlist.py --send --confirm
```

事前テスト:
```bash
python3 scripts/vault_launch/send_waitlist.py --send --only mail@yukihamada.jp
```

dry-run (default):
```bash
python3 scripts/vault_launch/send_waitlist.py
```

絞り込み条件: `you_users` で `lifetime_free=0 AND subscription_status IS NULL AND unsubscribed_at IS NULL AND NOT IN mu_purchases`

## X 投稿 (新規流入)

`x_posts.md` に A〜E カテゴリ別の文案。
launch day は A1 (メインアナウンス) から。dashboard か locked page のスクショ1枚添付推奨。

## なお `send_announcement.py` (旧 170名 blast 用) について

当初 170 名へ送る想定で作成したが、実数は 5 名のため不要。
将来 30 名超になったら使う想定で残置 — その時は `mu_purchases.email` から DISTINCT で blast。

## サイトの方の vault 機能 (既に LIVE)

- `/vault` — locked / unlocked カード一覧
- `/vault/stack` — Rust+Gemini+Printful 解説
- `/vault/prompt-cookbook` — Gemini 3 prompt 10本
- `/vault/open-ops` — 原価帳簿 + agent journal 解説
- `/api/vault/dashboard` — LIVE 60秒更新の透明性ダッシュボード

全ページ共通 chrome (header + footer) 適用済。
