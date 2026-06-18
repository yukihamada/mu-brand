#!/usr/bin/env bash
# ⛓ 永続証明: 音源付きT(または指紋つき商品)の design+音源を Arweave(Irys/SOL払い)
# に永久保存し、Solana mainnet に memo tx で sku+sha256 を刻み、
# /admin/catalog/permanence (サーバ側 sha256 再検証つき) で PDP に「⛓ 永続証明」を出す。
#
# 使い方:
#   MU_ADMIN_TOKEN=… bash scripts/permanence_inscribe.sh <SKU> [wallet.json]
# 前提: npm i -g @irys/cli / solana CLI / jq / ウォレットに少額SOL
# 🪤 公開RPC api.mainnet-beta はハングする → solana-rpc.publicnode.com を使う
# ⚠ ウォレット鍵(~/sol-wallet.json)の中身は出力・コミットしない
set -euo pipefail
SKU="${1:?usage: permanence_inscribe.sh <SKU> [wallet]}"
WALLET="${2:-$HOME/sol-wallet.json}"
BASE="${MU_BASE:-https://wearmu.com}"
RPC="${SOL_RPC:-https://solana-rpc.publicnode.com}"
TOKEN="${MU_ADMIN_TOKEN:?set MU_ADMIN_TOKEN}"
TK=(-t solana -w "$WALLET")
[ -f "$WALLET" ] || { echo "wallet not found: $WALLET" >&2; exit 1; }

key=$(printf '%s' "$SKU" | tr '[:upper:]' '[:lower:]')
info=$(curl -fsS "$BASE/api/oto/$key")
audio_url=$(jq -r '.url // empty' <<<"$info")
audio_sha=$(jq -r '.sha256 // empty' <<<"$info")
design_url=$(jq -r '.design_url // empty' <<<"$info")
design_sha=$(jq -r '.design_sha256 // empty' <<<"$info")
[ -n "$audio_url" ] || { echo "no fingerprinted audio for $SKU" >&2; exit 1; }

tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
curl -fsS -o "$tmp/audio" "$audio_url"
got=$(shasum -a 256 "$tmp/audio" | awk '{print $1}')
[ "$got" = "$audio_sha" ] || { echo "audio sha256 mismatch ($got != $audio_sha)" >&2; exit 1; }
echo "audio sha256 OK ($audio_sha)"
files=("$tmp/audio")
if [ -n "$design_url" ]; then
  curl -fsS -o "$tmp/design" "$design_url"
  if [ -n "$design_sha" ]; then
    gd=$(shasum -a 256 "$tmp/design" | awk '{print $1}')
    [ "$gd" = "$design_sha" ] || { echo "design sha256 mismatch ($gd != $design_sha)" >&2; exit 1; }
    echo "design sha256 OK ($design_sha)"
  else
    design_sha=$(shasum -a 256 "$tmp/design" | awk '{print $1}')
    echo "design sha256 computed ($design_sha)"
  fi
  files+=("$tmp/design")
fi

# ── Irys: price → fund(2倍バッファ) → upload ──
size_bytes=$(cat "${files[@]}" | wc -c | tr -d ' ')
price=$(irys price "$size_bytes" "${TK[@]}" 2>/dev/null | grep -oE '[0-9]+ lamports' | grep -oE '[0-9]+' | head -1 || true)
if [ -n "${price:-}" ]; then
  echo "== Irys price for ${size_bytes}B = ${price} lamports → fund 2x =="
  echo y | irys fund $(( price * 2 )) "${TK[@]}" || true
fi
upload() { # file -> irys id (stdout)
  local out id
  out=$(echo y | irys upload "$1" "${TK[@]}" 2>&1) || { echo "$out" >&2; return 1; }
  id=$(printf '%s\n' "$out" | sed 's/\x1b\[[0-9;]*[mGKHJf]//g' \
      | grep -oE '(gateway\.irys\.xyz|arweave\.net)/[A-Za-z0-9_-]{40,}' | tail -1 | sed -E 's#.*/##')
  [ -n "$id" ] || { echo "$out" >&2; return 1; }
  echo "$id"
}
echo "== upload audio =="
au_id=$(upload "$tmp/audio")
echo "audio  → https://gateway.irys.xyz/$au_id"
de_id=""
if [ -n "$design_url" ]; then
  echo "== upload design =="
  de_id=$(upload "$tmp/design")
  echo "design → https://gateway.irys.xyz/$de_id"
fi

# ── Solana mainnet memo 刻印(自分宛 1 lamport + memo・手数料 ≈ 5000 lamports) ──
SELF=$(solana-keygen pubkey "$WALLET")
memo="MU:$SKU:audio=$audio_sha${design_sha:+:design=$design_sha}"
tx=$(solana transfer "$SELF" 0.000000001 --allow-unfunded-recipient --with-memo "$memo" \
      --keypair "$WALLET" --url "$RPC" --commitment confirmed --output json 2>/dev/null \
      | jq -r '.signature // empty' || true)
if [ -n "${tx:-}" ]; then
  echo "memo tx → https://solscan.io/tx/$tx"
else
  echo "⚠ memo tx failed/skipped — Arweave のみで続行" >&2
fi

# ── サーバへ刻印(サーバ側でも Arweave 実バイトの sha256 を再検証) ──
body=$(jq -n \
  --arg a "https://gateway.irys.xyz/$au_id" \
  --arg d "${de_id:+https://gateway.irys.xyz/$de_id}" \
  --arg t "${tx:-}" \
  '{audio_ar:$a}
   + (if $d != "" then {design_ar:$d} else {} end)
   + (if $t != "" then {memo_tx:$t} else {} end)')
curl -fsS -X POST "$BASE/admin/catalog/permanence?token=$TOKEN&sku=$SKU" \
  -H 'content-type: application/json' -d "$body"
echo
echo "✅ inscribed $SKU — PDP: $BASE/shop/$SKU"
