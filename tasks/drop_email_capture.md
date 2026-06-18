# MU Drop メール取得（流入者メアド取得率の最大化）

決定（本人 2026-06-14）:
- incentive = **クーポンなし**。価値訴求＝「毎日1着、世界に生まれるMUの新作を最速で（先行通知/先行アクセス）」。
  - 理由: 割引クーポンは 2026-05-16 に全廃済み（main.rs:48508・CLAUDE.md）。全廃方針を保つ。
- 出し方 = **インライン常設 ＋ 離脱時モーダル**（取得率最大・MUの「ノイズなく」を概ね保つ）。

## スコープ
1. backend `POST /api/subscribe/drop` { email, source } — 検証→`catalog_subscribers`へ upsert→opt-in welcomeメール(Resend)→analytics `subscribe_drop`。
2. table `catalog_subscribers` (新規・購読者リスト。products/catalog_*商品テーブルには触れない)。
3. frontend: インラインフォーム(homepage footer / /shop グリッド下 / PDP CTA下) ＋ 離脱モーダル(1回/セッション・localStorage抑制)。
4. analytics: `funnel_track_server` で `subscribe_drop`(server-only=whitelist変更不要)。モーダル表示は client `cta_view`(既存許可)。

## 制約
- 既存テーブルのスキーマ変更禁止。CATALOG_CONTRACT 準拠（新規は subscriber リストのみ）。
- Resend送信は make_notify(catalog.rs:5304〜) のパターンを踏襲。from "━◯━ MU <noreply@wearmu.com>", reply_to info@wearmu.com。
- 二重送信防止: email UNIQUE・既存なら welcome 再送しない。
- RESEND_API_KEY 未設定でも登録は成功させ、メールのみ warn スキップ（make_notify と同じ）。
- branch 作業・main へ直 push しない（CI が mu-store を deploy するため）。PR 止まり、merge は人間。

## 検証
- `cargo build` 通過（store/）。
- ローカル起動 or ユニットでエンドポイント 200 + 重複で 200 冪等。
- メール文面・モーダル/インラインの見た目を確認（速く・ノイズなく）。
