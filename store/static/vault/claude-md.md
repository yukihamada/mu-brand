# MU の動かし方 — CEO の Claude Code SOP

> これは、私 (MU の人間オペレーター 1 人) が日々 Claude Code というツールに向かって書いている指示書 `~/.claude/CLAUDE.md` の、共有してよい部分の全文です。MU の 28 エージェントとは別の、私自身が AI と一緒にコードを書くときの作業手順 — つまり「人間 1 人が、どうやって 1 ブランド + 28 エージェント + 複数プロダクトを 1 人で回しているのか」の生のレシピ。
>
> PII (住所・電話・メール・API キー・チーム ID) はすべて除いて、思想と運用ルールだけを残しています。これがそのまま MU の動かし方であり、Constitution の運用版です。

---

## 0. なぜこれを Tシャツ所有者に開示するのか

MU の Constitution は「ブランドは 0 人で運営できる」と書いています。けれど現実には、今は私 1 人が **境界条件** を引いています — 「これは agent に任せていい」「これは人間が決めないと壊れる」。その境界条件を、私は毎朝 AI に向かって書き続けています。

それがこのドキュメントです。これを読むと、MU の運用そのものをコピーできます。

公式 Whitepaper や Constitution が「ブランドの法律」だとしたら、これは **「立法者の作業机」** です。

---

## 1. CRITICAL RULES — 絶対に破らないもの

これらは AI agent ではなく、**私自身** に向けた絶対ルール。

- **証拠なしに "終わった" と言わない。** diff、テスト出力、ビルドログ、スクリーンショットを必ず返答に含める。「たぶん動く」は完了ではない。
- **推測で直さない。** 実際のエラーを読み、実際のコードを grep し、実際のスタックを辿る。症状ではなく根本原因を直す (`--no-verify` 禁止、例外の握りつぶし禁止、テストのコメントアウト禁止)。
- **シークレットは絶対にコミットしない。** API キー / トークン / パスワードは `.env` か Fly secrets か GitHub Secrets だけ。コード・ログ・コミット・チャットに出さない。読んだファイルにキーが含まれていたら、引用する前に redact する。
- **`fly deploy` を直接叩かない。** 必ず `git push` → GitHub Actions が deploy する。例外なし。
- **以下の前には必ず確認する**: DB のスキーマ migration、破壊的操作 (`rm -rf`、force-push、ブランチ削除、テーブル drop)、有料 API のコスト増、他者から見えるもの (PR コメント、メール、Slack)。

---

## 2. WORKFLOW — どんなタスクも同じ 5 ステップ

3 ファイル以上 / 不慣れなコード / 多段の作業は、必ずこの順で。

1. **Explore** — `grep` / `glob` / Read で既存パターンを先に確認する。新しいパターンを設計する前に、既存パターンを読む。広域な検索は subagent に投げて、メインのコンテキストを汚さない。
2. **Plan** — Plan mode (Shift+Tab) または `tasks/todo.md` を書く。一文で説明できる差分のときだけスキップ。
3. **Implement** — 小さく集中した変更。既存パターンに合わせる。KISS / DRY だが、仮想の未来のために過剰に抽象化しない。
4. **Verify** — ビルドを走らせる。テストを走らせる。エンドポイントを叩く。ページを開く。— 後述。
5. **Report** — 何が変わったか (diff か file:line) と、verify の出力を必ず示す。

軽微な修正 (typo、log 行、リネーム) のときだけ 1–2 を飛ばす。

---

## 3. VERIFICATION — 一番効くプラクティス

これが MU を 1 人で回せている理由の半分。常にループを **具体的な証拠** で閉じる。

- **コード変更** → `cargo build` / `npm test` / `swift test` / linter を実行 → 結果を貼る
- **UI 変更** → ブラウザで開く → スクリーンショット → 期待値と比較
- **API 変更** → `curl` でエンドポイントを叩く → ステータスとボディを示す
- **DB マイグレーション** → ステージングのコピーで dry-run → 影響行数を数える
- **デプロイ** → CI green を待つ → health エンドポイントを叩く → ログで 5xx を確認

verify できない場合 (テストがない、ステージングがない) は、レポートに必ず **"unverified"** と明記する。ブラフをしない。

---

## 4. CONTEXT MANAGEMENT

- 1M context あっても、会話が埋まると adherence は落ちる。無関係なタスク間で `/clear`。
- 多ファイル横断のリサーチは Explore subagent に委譲 — メインコンテキストを汚さない。
- たった今編集したファイルを読み返さない。Edit / Write は state を追跡している。
- `tasks/todo.md` = 現在のタスクのスクラッチパッド。session を跨ぐ学習は auto-memory に任せる。

---

## 5. DELEGATION — subagent の使い分け

- "X はどこで定義されている / Y はどのファイルから参照されている" → **Explore** subagent
- 独立した並列作業 → 単一メッセージで複数の Agent 呼び出しを発射する
- 専用タスク → typed agent (`debugger`、`code-reviewer`、`fly-deployer`、`git-workflow`、`log-monitor`、`supabase-debugger`...)
- 同じ問題で 3 つの行き止まり → 止める。`/clear`。学んだことを含めた鋭いプロンプトで再起動。

---

## 6. OPUS-SPECIFIC (使っているモデルのクセ)

- 強いルールには `IMPORTANT:` と `MUST:` を付ける — soft な言い回しより信頼性が高い。
- Opus は agentic loop に強い。verification が自動化されている場面では自律性を与える。不可逆な action には明示的な承認を要求する。
- Plan mode は Opus では安い — 2 ファイル以上に触る不慣れなコードでは default で plan mode。
- 長時間タスクは `run_in_background` を使って、foreground を応答可能に保つ。

---

## 7. ワークスペース横断の地雷 (何度も踏んだもの)

これらは何度もやられたので明文化してある。MU 以外のプロジェクトでも有効。

- **Rust `include_str!`**: `cargo clean -p <pkg>` は include_str の中身をキャッシュ無効化しない。include されたアセットが変わったら full `cargo clean`。
- **Supabase RLS**: anon-key の書き込みは、RLS でブロックされても 204 No Content を返す。`Prefer: return=representation` で本当の失敗を露出させる。
- **iOS App Store 提出**: API キー認証は審査提出に使えない。`fastlane deliver` + Apple ID 認証を使う。
- **画像参照**: HTML をデプロイする前に、全 `<img src>` が解決することを必ず確認。画像生成中に HTML をデプロイしない。
- **顧客向けコピー**: 「ユーザー」ではなく「お客様」と書く。
- **公開ページ / blog / tweet / プレス**: お客様の実名・email・住所詳細を絶対に晒さない。

---

## 8. 検証コマンド (プロジェクト型別)

完了を宣言する前に、最低この 1 行を走らせる。

- **Rust + Fly.io** (chatweb.ai / wearmu / 他): `cargo build --release` ローカル → push → GitHub Actions green → `curl https://<app>.fly.dev/health`
- **React + Supabase**: `npm run build && npm run lint` → Lovable / Cloudflare deploy 確認 → 変更ページの smoke test
- **Swift iOS**: `xcodebuild -scheme <X> build` → 実機にインストール → 変更フローを操作
- **Static / Node**: `npm run build` → ローカルでプレビュー → push → 本番 URL で確認

これが無いプロジェクトでは、独自の 1 行 verify を書いてから「動く」と言う。

---

## 9. これを MU に翻訳すると

| 私 (1 人) の SOP | MU の Constitution / 運用 |
|---|---|
| 証拠なしに "終わった" と言わない | Constitution §1: numbers over adjectives |
| `git push` → GitHub Actions → Fly | `deploy.yml` の smoke_test → mu-store |
| シークレットは `.env` か Fly secrets | `STRIPE` / `GEMINI_API_KEY` を含む diff は self_evolve 自動マージ禁止 |
| 不可逆な action は人間が承認 | Constitution T1 ドア (価格 > ¥500、drop launch、refund > ¥10,000...) |
| 同じ修正を私が 2 回 override したら Constitution に書く | Constitution §10 |
| Verify before reporting | `checkout_health` エージェントが 15 分毎に live purchase path を probe |

つまり、私は AI に対して、自分自身に課しているのと同じ "verify before reporting" を要求しているだけ。Constitution はその外部化です。

---

## 10. このノートが更新される条件

私が同じ訂正を AI に 2 回したら、それは「ルールが足りていない」サイン。そのルールが Constitution に属するなら Constitution に、私個人の作業手順なら CLAUDE.md に追記する。

— *MU autopilot / yuki, last reviewed 2026-05-18*
