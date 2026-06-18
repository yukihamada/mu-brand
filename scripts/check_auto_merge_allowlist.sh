#!/usr/bin/env bash
# Enforce the MU Constitution §"Auto-merge Allowlist" for self_evolve PRs.
# Usage:  check_auto_merge_allowlist.sh <base_sha> <head_sha> <pr_body>
# Exit 0 if PR may be auto-merged, exit 1 otherwise (with reason on stderr).
#
# See static/constitution.md for the canonical rules. This script is the
# machine-enforceable subset.

set -euo pipefail

BASE="${1:?base sha required}"
HEAD="${2:?head sha required}"
BODY="${3:-}"

deny() {
  echo "AUTO_MERGE_DENY: $1" >&2
  exit 1
}

# ── 1. PR body must opt in ──────────────────────────────────────────────
if ! grep -qiE 'auto-merge-eligible: *true' <<< "$BODY"; then
  deny "PR body missing 'auto-merge-eligible: true'"
fi

# ── 2. File allowlist ───────────────────────────────────────────────────
CHANGED=$(git diff --name-only "$BASE" "$HEAD")
if [ -z "$CHANGED" ]; then
  deny "empty diff"
fi
while IFS= read -r f; do
  case "$f" in
    store/src/main.rs) ;;
    static/templates/messages/*.txt) ;;
    *) deny "file outside allowlist: $f" ;;
  esac
done <<< "$CHANGED"

# ── 3. Diff size cap (added + removed < 50 lines) ───────────────────────
SIZE=$(git diff --numstat "$BASE" "$HEAD" | awk '{a+=$1; d+=$2} END{print (a+d)+0}')
if [ "$SIZE" -ge 50 ]; then
  deny "diff size $SIZE >= 50 line cap"
fi

# ── 4. Forbidden tokens scan (case-sensitive, on diff context) ──────────
FORBIDDEN=(
  'STRIPE_'
  'PRINTFUL_API_KEY'
  'GEMINI_API_KEY'
  'X_CLIENT_SECRET'
  'X_ACCESS_TOKEN_SECRET'
  'X_CONSUMER_SECRET'
  'TELEGRAM_BOT_TOKEN'
  'HELIUS_API_KEY'
  'RESEND_API_KEY'
  'SECRET'
  'password'
  'DROP TABLE'
  'DROP COLUMN'
  'ALTER TABLE'
  'DELETE FROM'
  'unsafe'
  'transmute'
  '.amount_jpy'      # touching money math without review
  'price_jpy'        # purchase-path-touching (Constitution §21)
  'collab_products'  # ditto
  '/api/checkout'    # checkout endpoint
  'stripe.com/v1'    # any new Stripe call
  'webhook'          # stripe webhook handler
  'printful.com/'    # any new Printful call
)
DIFF_ADDED=$(git diff "$BASE" "$HEAD" | grep -E '^\+[^+]' || true)
for tok in "${FORBIDDEN[@]}"; do
  if echo "$DIFF_ADDED" | grep -qF "$tok"; then
    deny "forbidden token in added lines: $tok"
  fi
done

# ── 5. For main.rs: every added non-blank, non-comment line must look like
#       data (string content) or a known-safe parameter assignment.
#       Reject if any line looks like Rust *statement* (fn/let/use/match/etc).
if echo "$CHANGED" | grep -qx 'store/src/main.rs'; then
  # Extract just the added lines (without the leading +) for main.rs only.
  ADDED=$(git diff "$BASE" "$HEAD" -- store/src/main.rs | grep -E '^\+[^+]' | sed 's/^\+//' || true)
  # Patterns that are SAFE to add automatically:
  #   - blank
  #   - comment (`//` or `///`)
  #   - inside multi-line string literal (heuristic: not starting with a Rust
  #     statement keyword, no `=>`, no `;` unless in `const = N;` form)
  #   - `interval_secs: N,`
  #   - `pub const *_THRESHOLD* / *_CAP_* / *_LIMIT*: i64 = N;`
  #   - `name: "literal",` / `description: "literal",`
  DANGEROUS_RE='^[[:space:]]*(fn |async fn |let |use |mod |pub |impl |trait |struct |enum |match |if |else|for |while |loop |return |async |move |drop\(|conn\.|db\.|reqwest::|tokio::|std::|env::|serde_json::|tracing::|panic!|unsafe|todo!|unimplemented!|axum::|.route\(|.layer\(|.with_state\()'
  if echo "$ADDED" | grep -vE '^[[:space:]]*$' | grep -vE '^[[:space:]]*//' | grep -E "$DANGEROUS_RE" >&2; then
    deny "main.rs change contains lines that look like Rust statements (see above)"
  fi
fi

echo "AUTO_MERGE_OK: size=$SIZE lines, files=$(echo "$CHANGED" | wc -l | tr -d ' ')"
