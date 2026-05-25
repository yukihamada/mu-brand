# Claude Code 実戦マニュアル（完全版）

> これは、僕（MU の人間オペレーター 1 人）が Claude Code を使って 1 ブランド + 28 エージェント + 複数プロダクトを回すための「実戦マニュアル」の完全版です。`claude-md`（CEO の SOP＝なぜこう動かすか）が *憲法* だとすれば、これは *現場の教範* — どう設定し、どう指示し、どう自動化するか。全部コピーして使えます。
>
> ブランドの中の人間が 1 人になっても回り続けるように、「ベストプラクティスを覚えて実践する」を**仕組みに肩代わりさせる**のがゴールです。

---

## 0. たった一つの心的モデル

Claude Code を「賢いオートコンプリート」だと思っている限り、ずっと損をする。正しいモデルはこれ：

> **Claude Code ＝「優秀だが記憶喪失の天才請負人」**

- **天才**：どの言語も書ける、速い。
- **記憶喪失**：セッションが終われば忘れる → `CLAUDE.md` に申し送りを残す。
- **請負人**：勝手に本番を触らせると事故る → 権限のゲートを設ける。
- **そして人間**：「できました」と言うが検証しないと嘘かもしれない → 証拠を要求する。

ここから、管理すべき資源が **3 つ**だけ導かれる。**注意（コンテキスト）／権限／検証**。本書の全テクニックは、このどれかを守る手段にすぎない。

### 三つの法則

1. **コンテキストは消耗品。** 無関係を入れた瞬間、賢さが落ちる。汚れたら `/clear`。
2. **検証手段のない指示は出すな。** 「直して」ではなく「このテストが緑になるまで直して」。
3. **権限は最小から。** 事故る操作だけゲートにかける。`rm -rf` と `git push --force` を区別しろ。

---

## 1. 背骨のワークフロー：Explore → Plan → Code → Commit

いきなりコードを書かせるのが最大の初心者ミス。

1. **Explore** — `Shift+Tab` で Plan Mode（読み取り専用）。「`src/auth/` を読んで。まだ変更しないで」。広い調査は**サブエージェントに投げて**メインの注意を汚さない。
2. **Plan** — 「変更ファイル・原因・副作用・テスト方針を箇条書きで」。`Ctrl+G` で人間が直す。ここの 1 分が実装の 1 時間を救う。
3. **Code** — 「合意した計画どおり実装。失敗テストで再現してから直して」。
4. **Commit** — 規約に沿ってコミット。

---

## 2. 注意（コンテキスト）を制す

| 失敗モード | 症状 | 処方 |
|---|---|---|
| 闇鍋セッション | 無関係タスク混在 | `/clear` |
| 訂正ループ | 同じ点を 2 回直す | `/clear` ＋学びを含めた新プロンプト |
| 無限探索 | 「調べて」で数百ファイル読む | スコープを絞る／サブエージェント |

武器：`/clear`（全消去）・`/compact <焦点>`（要約圧縮）・`/rewind`（巻き戻し）・`/btw`（履歴を汚さない脇道質問）。大規模調査は**サブエージェント**に隔離。並列作業は **worktree**（`claude -w <name>`）で物理分離。

---

## 3. 権限を制す

モード：`default`（読むだけ）／`acceptEdits`（編集まで）／`plan`（探索）／`auto`（無人・背景の分類器が危険だけ阻止）／`bypassPermissions`（隔離環境のみ）。`Shift+Tab` で循環。

```json
{ "permissions": {
    "allow": ["Bash(cargo *)", "Bash(git *)", "Read(src/**)"],
    "deny":  ["Bash(rm -rf *)", "Read(.env*)", "Edit(.git/**)"],
    "defaultMode": "acceptEdits" } }
```

**`CLAUDE.md` は「お願い」、hooks は「強制」。** 必ず実行させたいことは hooks に書く（後述）。

---

## 4. テストと検証を制す（最高レバレッジ）

> 公式：「Claude に自分の成果を検証する手段を与えること。これが単一で最もレバレッジの高い行為だ。」

- **検証ハンドルを同梱**：ロジック→テストケース、UI→スクショ＋目標画像、API→`curl` の期待ステータス。
- **TDD ループ**：失敗テストで赤を確認 → 実装で緑 → リファクタ。“通ったフリ”を物理的に潰す。
- **種類ごとに閉じる**：コード→テスト緑のログ、デプロイ→CI 緑→health 叩く→5xx 確認。
- 検証できないときは **「未検証」と正直に言わせる**。

アンチパターン：信頼ギャップ（検証なしで受け入れ）／テストの改竄（実装でなくテストを緩める）／緑の自己申告（ログを貼らせる）／モック漬け。

---

## 5. 記憶喪失をなくす CLAUDE.md（anti-amnesia）

公式いわく **「会話だけの指示は `/compact` で消えるが、プロジェクト直下の `CLAUDE.md` は再読込されて生き残る」**。つまり*消える記憶をここへ移す*のが治療。記憶は 3 点で消す：

| 仕組み | 誰が書く | 役割 |
|---|---|---|
| `CLAUDE.md` | あなた | 「毎回絶対」守ること（毎セッション全文ロード） |
| Auto Memory | Claude | 学びを**自動蓄積**（既定 ON, v2.1.59+） |
| `.claude/rules/` | あなた | パス限定の規約（該当時のみロード） |

各節が「消す健忘の種類」に対応するテンプレ：

```markdown
# Project: <name>
<1行で何のプロジェクトか>

## コマンド          ← 再導出させない
- Build / Test / Lint / Run / Deploy

## 規約              ← 毎回同じ指摘をさせない（具体的・検証可能に）

## 決定と“理由”      ← 蒸し返させない（why が肝。理由が無いと毎回別案を再提案）

## ハマりどころ/過去の事故  ← ★治療の本体・同じ轍を踏ませない
- 一度ハマったことだけを 1 行ずつ追記

## やるな            ← IMPORTANT で破壊防止
- 直接デプロイ禁止 / .env をコミットしない

<!-- HTMLコメントは context に載らない＝人間用メモをトークン0で残せる -->
```

**維持ループ**：①同じミスを 2 回 ②レビューで「知ってて当然」を指摘 ③去年と同じ訂正を打った ④新メンバーに同じ説明が要る → そのとき「これ CLAUDE.md に追記して」。学び系は「〜と覚えておいて」で Auto Memory に自動保存。`/memory` で監査。

---

## 6. 自律実行：`/goal` で「条件を満たすまで」回す

`/goal` は**達成条件を 1 つ宣言すると、満たすまでターンをまたいで自走**する（v2.1.139+）。毎ターン後に小型モデル（既定 Haiku）が yes/no 判定。作業する側と「終わったと判定する側」が分離されているのがミソ。

```text
/goal all tests in test/auth pass and the lint step is clean
/goal            # 状態確認（経過ターン・トークン・直近の理由）
/goal clear      # 中断
```

効く条件の 4 点セット：**測れる終了状態**／**証明の仕方を明記**（`npm test` が 0）／**崩さない制約**／**回数・時間の上限**（`or stop after 20 turns`）。評価役はツールを呼ばず会話の事実だけ読むので、「いい感じになるまで」のような**判断系は永遠に回る**。無人で回すなら **auto mode と併用**＋必ず上限を入れる。

---

## 7. 全部を自動で効かせる：drop-in `.claude/`

**この一式を置くだけで、上のプラクティスが「覚えていなくても」自動で効く。**

```text
.claude/
├── CLAUDE.md          # 5章（記憶喪失をなくす）
├── settings.json      # hooks + permissions + Auto Memory
├── rules/api.md       # パス限定の規約
└── hooks/
    ├── fmt.sh         # 編集後に整形
    └── block-rm.sh    # 危険コマンド阻止
```

`.claude/settings.json`：

```json
{
  "permissions": {
    "allow": ["Bash(cargo *)", "Bash(npm *)", "Bash(git *)", "Read(src/**)"],
    "deny":  ["Bash(rm -rf *)", "Read(.env*)", "Edit(.git/**)"],
    "defaultMode": "acceptEdits"
  },
  "autoMemoryEnabled": true,
  "hooks": {
    "PostToolUse": [
      { "matcher": "Edit|Write",
        "hooks": [{ "type": "command",
                    "command": "${CLAUDE_PROJECT_DIR}/.claude/hooks/fmt.sh" }] }
    ],
    "PreToolUse": [
      { "matcher": "Bash",
        "hooks": [{ "type": "command", "if": "Bash(rm *)",
                    "command": "${CLAUDE_PROJECT_DIR}/.claude/hooks/block-rm.sh" }] }
    ]
  }
}
```

`.claude/hooks/block-rm.sh`（実行前に `rm -r` を止める）：

```bash
#!/usr/bin/env bash
COMMAND=$(jq -r '.tool_input.command' < /dev/stdin)
if echo "$COMMAND" | grep -qE 'rm +-[a-zA-Z]*r'; then
  jq -n '{ hookSpecificOutput: { hookEventName: "PreToolUse",
    permissionDecision: "deny",
    permissionDecisionReason: "rm -r はフックで禁止。意図的なら人間が手で実行を。" } }'
else
  exit 0
fi
```

`.claude/hooks/fmt.sh`（編集された .rs だけ整形）：

```bash
#!/usr/bin/env bash
FILE=$(jq -r '.tool_input.file_path // empty')
[[ "$FILE" == *.rs ]] && rustfmt "$FILE"
exit 0
```

導入：①上の構成で `.claude/` を作る ②`chmod +x .claude/hooks/*.sh` ③`/doctor`（診断）→ `/hooks`（登録確認）→ `/memory`（ロード確認）。以後は**置いてあるだけで自動適用**。

> ⚠️ hook のイベント名は `PostToolUse` / `PreToolUse`（matcher で対象指定）。`PostEdit` / `PreCommit` という名前は**存在しない**。

---

## 8. アンチパターン名鑑（冷蔵庫に貼る）

1. 闇鍋セッション → `/clear`
2. 訂正ループ → `/clear` ＋新プロンプト
3. 無限探索 → 絞る／サブエージェント
4. 計画なし直行 → Plan Mode
5. 信頼ギャップ → テスト/スクショ同梱
6. 過剰 CLAUDE.md（200 行超） → 削る／rules・skills へ
7. モノリス並列 → worktree 分離
8. 全部許可 → danger は `deny`
9. CLAUDE.md に強制を期待 → 必須は hooks
10. 判断系ゴール（`/goal` を主観 done に） → 機械的に証明できる条件だけ

---

これが、僕が 1 人でブランドを回している「現場の教範」の全部です。`claude-md`（CEO の SOP）と合わせて読むと、*なぜ*こう動かすか（憲法）と*どう*動かすか（教範）の両方が揃います。

同じ訂正を AI に 2 回したら、それは「ルールが足りない」サイン。そのとき、このマニュアルにも 1 行足してください。記憶喪失は、仕組みで治せる。

— *MU autopilot / yuki, last reviewed 2026-05-25*
