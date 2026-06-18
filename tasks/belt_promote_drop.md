# 昇帯記念ドロップ (Belt Promotion Drop) — BJJ需要ドリブン物販

## なぜ (戦略整合)
CLAUDE.md STRATEGY: MU単独で一般アパレルを狙わない。BJJ垂直の「買う理由(需要)」で一次流通を作る(磨きでなく転換)。
昇帯はBJJ最大の感情ピーク → ¥4,900即決ゾーン。一点物=シリアル台帳の provenance とも噛み合う。

## スコープ (mu-brand 単体・JiuFlow自動発火は後追い)
- `public_make` を雛形に特化版を実装。新テーブル無し=catalog契約準拠。
- 構造化入力(名前/道場/帯/段・線/昇帯日/得意技/言語)→墨絵プロンプト→Gemini生成→白T(tee_white)
- `edition_size=1` の一点物 → 既存 `/edition/:sku` シリアル台帳・真正性ページが自動で効く
- ブランド `bjj-promote` を INSERT OR IGNORE (minna と同じ作法)
- 構造化入力なので商標/実在人物リスク低 → 自動 live

## タスク
- [x] make/edition/生成フローの実体確認
- [x] `promote_page` (GET /promote) フォームページ — catalog.rs
- [x] `public_promote` (POST/GET /api/promote) ハンドラ — catalog.rs
- [x] ルート登録 (main.rs L68486-68487)
- [x] `cargo build --release` 通過 (exit 0・新コード由来の警告/エラー無し)
- [ ] deploy=git push (本人GO待ち)
- [ ] 本番 E2E (Gemini生成→R2→checkout→/edition) ※ローカル起動は自律エンジン/課金発火を避け未実施=runtime未検証

## 後追い (別タスク)
- JiuFlow 昇帯イベント→/api/promote 自動発火 (MCP/webhook)
- 残り3本: マイルストーン解錠 / 先生の教えT(Koe) / 道場チームオーダー
