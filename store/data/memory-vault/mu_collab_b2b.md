---
name: MU Collab 法人向けプラン
description: wearmu.com の法人プランページ /collab + PDF 資料、3 プランの料金体系
type: reference
originSessionId: 1ce5a54b-0fdf-4f54-9bf4-90a31df46c16
---

## 場所
- HTML: `https://wearmu.com/collab` (alias: `/b2b`, `/partners`)
- PDF:  `https://wearmu.com/b2b/mu-collab-pitch.pdf` (8 ページ、1.4MB)
- ソース: `store/static/collab.html` + `collab_page()` in `store/src/main.rs`
- PDF 再生成: `chrome --headless=new --print-to-pdf=static/b2b/mu-collab-pitch.pdf file://...static/collab.html`

## 3 プラン (全プラン初年度 12 か月 基本料 ¥0)

| プラン | 2 年目〜基本料 | レベニューシェア | AI 枠 | 独自ドメイン |
|---|---|---|---|---|
| Starter | ¥29,800/月 | 30% | 100 SKU/月 | サブパス |
| Growth | ¥98,000/月 | 20% | 500 SKU/月 | サブドメイン |
| Enterprise | ¥298,000/月 | 10% | 無制限 | フルドメイン |

- Enterprise のみ: 専属 Stripe アカウント / 専任 AI エージェント / SLA 99.9% / NFT ゲート

## 訴求点
- **24h で 30+ SKU 立ち上げ** (実例 A は 13 日で 31 SKU 稼働)
- **初年度 基本料 ¥0** (lock-in なし、合意できなければ無償解約)
- **在庫ゼロ** (Printful on-demand 製造、北米/欧/日 6 拠点)
- **自律エージェント 7 体** で運用フリー
- **平均 margin 47%** (31 SKU 実測)

## 事例の出し方 (重要 / 2026-05-12 更新)
- **SIIIEEP は正式合意前のため /collab 上では匿名化**: 「事例 A — 都内発 アパレルブランド」表記、社名 / 北参道 / BJJ 道場 などの特定要素は伏せる。margin 実数のみ提示。
- **[partner].tokyo (焼肉) は事例 B として「公式ローンチ済」(2026-05-12)**: /[partner] で 8 SKU 公開、Stripe Live + Printful E2E 検証済。常連向け店内 QR → /[partner] 直リンク導線。
- 先行参照希望企業には NDA ベースで開示 → `[email redacted]`
- `/sweep` ページ (パスワード gate 付き draft preview) は SIIIEEP 名残ったまま — そちらは内部運用URL なのでOK

## マルチパートナーアーキテクチャ
- `collab_products.partner` 列で sweep / [partner] を識別 (将来 partner 追加可)
- `sweep_checkout` / `[partner]_checkout` は `collab_checkout(partner, return_path, label)` 共通ヘルパー経由
- Webhook handler `handle_collab_sweep_order` は `partner IN ('sweep','[partner]')` で lookup
- `metadata[collab]` 値で sweep / [partner] に dispatch (`matches!(... Some("sweep") | Some("[partner]"))`)
- 新 partner 追加時の手順: (1) seed row with `partner='new_name'`, (2) checkout endpoint 1 行追加, (3) webhook の `matches!` に追加, (4) ページ handler 追加

## E2E 検証結果 (2026-05-12)
8 商品全てで Printful draft 注文成功:
- bomber +¥11,060 (56%) / track-jacket +¥5,880 (35%) / backpack +¥3,825 (24%)
- fanny-pack +¥2,360 (35%) / iphone-case +¥940 (25%) / bucket-hat +¥2,210 (38%)
- joggers +¥6,070 (47%) / baseball-jersey +¥9,140 (62%)
- 平均 margin 40.3%

## CTA
全ボタン → `mailto:[email redacted]` (プラン別 subject pre-fill)