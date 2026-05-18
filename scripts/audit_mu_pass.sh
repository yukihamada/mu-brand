#!/usr/bin/env bash
# audit_mu_pass.sh — End-to-end self-audit of the MU Pass system.
#
# Probes every endpoint, sanity-checks DB consistency, decodes one cNFT
# tx, and prints a PASS/FAIL table. Exits non-zero if anything is wrong.

set -uo pipefail

HOST="${HOST:-https://wearmu.com}"
ADMIN_TOKEN="$(grep '^MU_ADMIN_TOKEN=' /Users/yuki/.env 2>/dev/null | head -1 | cut -d= -f2- | tr -d $'\r"'\')"
if [ -z "$ADMIN_TOKEN" ]; then
  echo "✗ MU_ADMIN_TOKEN not found in /Users/yuki/.env"; exit 1
fi

PASS=0; FAIL=0
results=()

check() {
  local name="$1" expected="$2" got="$3"
  if [ "$expected" = "$got" ]; then
    results+=("✓ $name → $got")
    PASS=$((PASS+1))
  else
    results+=("✗ $name → expected=$expected got=$got")
    FAIL=$((FAIL+1))
  fi
}

# ── 1. Public pages ────────────────────────────────────────────────
echo "── 1. Public pages ──"
check "/pass loads"         "200" "$(curl -sf -o /dev/null -w '%{http_code}' $HOST/pass)"
check "/pass/ loads"        "200" "$(curl -sf -o /dev/null -w '%{http_code}' $HOST/pass/)"
check "/ hero has /pass"    "yes" "$(curl -sf $HOST/ | grep -q '/pass' && echo yes || echo no)"
check "/buy.html has panel" "yes" "$(curl -sf $HOST/buy.html | grep -q 'あなたが受け取る 4 つ' && echo yes || echo no)"
check "/tokushoho MU Pass"  "yes" "$(curl -sf $HOST/tokushoho.html | grep -q 'MU Pass' && echo yes || echo no)"
check "/vault genesis"      "200" "$(curl -sf -o /dev/null -w '%{http_code}' $HOST/vault/genesis-pass-letter)"

# ── 2. Public APIs ─────────────────────────────────────────────────
echo
echo "── 2. Public APIs ──"
stats=$(curl -sf $HOST/api/pass/stats)
total=$(echo "$stats" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("total","?"))')
minted=$(echo "$stats" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("minted","?"))')
check "/api/pass/stats total=20"  "20" "$total"
check "/api/pass/stats minted=20" "20" "$minted"

# Public metadata for #001
meta=$(curl -sf $HOST/api/pass/metadata/1)
mname=$(echo "$meta" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("name","?"))')
check "metadata #001 name" "MU Pass #001" "$mname"

# by_email for a known holder (yuki) — count check (yuki bought 16 sweep test items)
by=$(curl -sf "$HOST/api/pass/by_email?email=mail@yukihamada.jp")
n=$(echo "$by" | python3 -c 'import sys,json; print(len(json.load(sys.stdin).get("passes",[])))')
check "by_email mail@yukihamada returns 16" "16" "$n"
nz=$(curl -sf "$HOST/api/pass/by_email?email=nonexistent@example.com" | python3 -c 'import sys,json; print(len(json.load(sys.stdin).get("passes",[])))')
check "by_email unknown returns 0" "0" "$nz"

# ── 3. Admin APIs (auth gate) ──────────────────────────────────────
echo
echo "── 3. Admin APIs (auth gate) ──"
check "list rejects empty token" "403" "$(curl -s -o /dev/null -w '%{http_code}' -X GET $HOST/api/admin/pass/list)"
check "list rejects bogus token" "403" "$(curl -s -o /dev/null -w '%{http_code}' -X GET $HOST/api/admin/pass/list?admin_token=bogus)"
list_resp=$(curl -sf "$HOST/api/admin/pass/list?admin_token=$ADMIN_TOKEN")
list_count=$(echo "$list_resp" | python3 -c 'import sys,json; print(len(json.load(sys.stdin).get("passes",[])))')
minted_count=$(echo "$list_resp" | python3 -c 'import sys,json; print(sum(1 for p in json.load(sys.stdin).get("passes",[]) if p.get("mint_status")=="minted"))')
check "admin list count = 20"       "20" "$list_count"
check "admin list minted = 20"      "20" "$minted_count"

# Crossmint mint endpoint should reject sans env
mint_resp=$(curl -s -X POST $HOST/api/admin/pass/mint -H 'content-type: application/json' \
  -d "{\"admin_token\":\"$ADMIN_TOKEN\",\"edition\":1}" \
  | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d.get("error","ok") if not d.get("ok") else "ok")')
check "crossmint disabled (no env)" "Crossmint not configured" "$mint_resp"

# ── 4. On-chain ────────────────────────────────────────────────────
echo
echo "── 4. On-chain ──"
tree="2fpSZ1rwZsvupiavocgnyksYu2RSUfrw6yM1ZfEDtrBN"
if command -v solana >/dev/null 2>&1; then
  owner=$(solana account "$tree" 2>&1 | awk '/^Owner:/ {print $2}')
  check "tree owner = SPL Account Compression" "cmtDvXumGCrqC1Age74AVPhSRVXJMd8PJS91L8KbNCK" "$owner"
  # First mint tx
  tx001=$(curl -sf "$HOST/api/admin/pass/list?admin_token=$ADMIN_TOKEN" | python3 -c 'import sys,json; ps=json.load(sys.stdin)["passes"]; p=[p for p in ps if p["edition"]==1][0]; print(p.get("mint_tx",""))')
  if [ -n "$tx001" ]; then
    confirmed=$(solana confirm "$tx001" 2>&1 | head -1)
    check "tx #001 finalized" "Finalized" "$confirmed"
  fi
else
  results+=("⚠ solana CLI not available — skipping on-chain checks")
fi

# ── 5. Pass holder pt_gate bypass (script-level) ───────────────────
echo
echo "── 5. pt_gate.js holder bypass ──"
ptgate=$(curl -sf $HOST/pt_gate.js)
check "pt_gate.js has isHolder()"   "yes" "$(echo "$ptgate" | grep -q 'isHolder' && echo yes || echo no)"
check "pt_gate.js calls by_email"   "yes" "$(echo "$ptgate" | grep -q '/api/pass/by_email' && echo yes || echo no)"
check "pt_gate.js updates badge"    "yes" "$(echo "$ptgate" | grep -q 'updateBadgeForHolder' && echo yes || echo no)"

# ── 6. Background agent registered ─────────────────────────────────
echo
echo "── 6. Background agent ──"
journal=$(curl -sf "$HOST/api/admin/agents/journal?admin_token=$ADMIN_TOKEN&limit=80" 2>/dev/null || echo '[]')
if echo "$journal" | grep -q "pass_pending_alert"; then
  results+=("✓ agent pass_pending_alert has run at least once")
  PASS=$((PASS+1))
else
  results+=("⚠ agent pass_pending_alert not yet observed in journal (may be < 30 min since deploy)")
fi

# ── Report ─────────────────────────────────────────────────────────
echo
echo "─────────────────────────────────────────"
printf '%s\n' "${results[@]}"
echo "─────────────────────────────────────────"
echo "PASS=$PASS  FAIL=$FAIL"
[ "$FAIL" -eq 0 ]
