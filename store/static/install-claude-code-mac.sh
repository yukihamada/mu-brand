#!/usr/bin/env bash
# install-claude-code-mac.sh — クリーン Mac (Apple Silicon / Intel 両対応) で
# Claude Code を一発インストール。 sudo は最小限。
#
# 使い方:
#   curl -fsSL https://wearmu.com/static/install-claude-code-mac.sh | bash
# または:
#   /bin/bash -c "$(curl -fsSL <this-url>)"
#
# やること:
#   1. Xcode Command Line Tools (git 等)
#   2. Homebrew (なければ)
#   3. Node.js (npm 付き、 brew 経由)
#   4. @anthropic-ai/claude-code を npm global install
#   5. シェル PATH を ~/.zshrc に追記 (Apple Silicon の /opt/homebrew/bin)
#   6. 起動コマンドと認証手順を案内

set -euo pipefail

C='\033[1;33m'; G='\033[1;32m'; R='\033[1;31m'; N='\033[0m'
step() { echo -e "\n${C}━━━ $* ━━━${N}"; }
ok()   { echo -e "${G}✓${N} $*"; }
die()  { echo -e "${R}✗${N} $*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || die "macOS 専用です (今は $(uname -s))"

# ─── 1. Xcode CLT ───────────────────────────────────────────
step "1. Xcode Command Line Tools"
if xcode-select -p &>/dev/null; then
  ok "既にインストール済 ($(xcode-select -p))"
else
  echo "  GUI でインストーラが開くので、 完了したらこのスクリプトを再実行してください。"
  xcode-select --install || true
  die "Xcode CLT のインストールを待ってから再実行"
fi

# ─── 2. Homebrew ────────────────────────────────────────────
step "2. Homebrew"
if command -v brew &>/dev/null; then
  ok "既にインストール済 ($(brew --prefix))"
else
  echo "  Homebrew をインストール中…"
  NONINTERACTIVE=1 /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
fi

# PATH を現セッション + ~/.zshrc に追記
if [[ -x /opt/homebrew/bin/brew ]]; then
  BREW_SHELLENV='eval "$(/opt/homebrew/bin/brew shellenv)"'
elif [[ -x /usr/local/bin/brew ]]; then
  BREW_SHELLENV='eval "$(/usr/local/bin/brew shellenv)"'
else
  die "brew が PATH に見つかりません"
fi
eval "$BREW_SHELLENV"

if ! grep -q "brew shellenv" "${HOME}/.zshrc" 2>/dev/null; then
  echo "" >> "${HOME}/.zshrc"
  echo "# Homebrew (installed by install-claude-code-mac.sh)" >> "${HOME}/.zshrc"
  echo "$BREW_SHELLENV" >> "${HOME}/.zshrc"
  ok "~/.zshrc に Homebrew PATH を追記"
fi

# ─── 3. Node.js ─────────────────────────────────────────────
step "3. Node.js (v22 LTS)"
if command -v node &>/dev/null; then
  ok "既にインストール済 ($(node --version))"
else
  brew install node
  ok "Node $(node --version) をインストール"
fi

# ─── 4. Claude Code ─────────────────────────────────────────
step "4. Claude Code (@anthropic-ai/claude-code)"
if command -v claude &>/dev/null; then
  CURRENT_VER="$(claude --version 2>/dev/null | head -1 || echo '?')"
  ok "既にインストール済 (${CURRENT_VER}) — 更新するなら: npm update -g @anthropic-ai/claude-code"
else
  npm install -g @anthropic-ai/claude-code
  ok "Claude Code をインストール"
fi

# ─── 5. 完了 ────────────────────────────────────────────────
step "✓ 完了"
cat <<MSG

  ${G}次にやること:${N}

  1. 新しいターミナルを開く (PATH を反映するため):
     ${C}exec zsh${N}

  2. Claude Code を起動:
     ${C}claude${N}

  3. 初回起動時に認証画面が出るので、 ブラウザで Anthropic アカウントログイン
     (推奨)。 または環境変数で API key を渡す:
     ${C}export ANTHROPIC_API_KEY=sk-ant-...${N}

  4. プロジェクトディレクトリで使う:
     ${C}cd ~/your-project && claude${N}

  ドキュメント: https://docs.claude.com/en/docs/claude-code

MSG
