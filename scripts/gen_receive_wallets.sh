#!/bin/bash
# gen_receive_wallets.sh
#
# Generate fresh receive-wallets for MU crypto checkout:
#   - Solana keypair  → ~/.mu-wallets/sol_recipient.json
#   - Ethereum key    → ~/.mu-wallets/eth_recipient.json
#
# Then:
#   - Print each PUBLIC address
#   - Render two QR codes per chain:
#       (a) Raw address (for "send me X" pasting)
#       (b) Solana Pay / EIP-681 URI (mobile-wallet deep link)
#   - Update Fly secrets MU_SOL_RECIPIENT / MU_ETH_RECIPIENT
#
# PRIVATE KEYS NEVER LEAVE THIS MACHINE.
# Files are written with chmod 600 and the secret material is NEVER echoed.

set -eu

WALLET_DIR="${HOME}/.mu-wallets"
SOL_FILE="${WALLET_DIR}/sol_recipient.json"
ETH_FILE="${WALLET_DIR}/eth_recipient.json"

mkdir -p "${WALLET_DIR}"
chmod 700 "${WALLET_DIR}"

# ── 1. Solana keypair ─────────────────────────────────────────────────
if [[ -f "${SOL_FILE}" ]]; then
  echo "[sol] reusing existing keypair at ${SOL_FILE}"
else
  echo "[sol] generating new keypair → ${SOL_FILE}"
  solana-keygen new --outfile "${SOL_FILE}" --no-bip39-passphrase --silent
  chmod 600 "${SOL_FILE}"
fi
SOL_ADDR=$(solana-keygen pubkey "${SOL_FILE}")
echo "[sol] public address: ${SOL_ADDR}"

# ── 2. Ethereum key ───────────────────────────────────────────────────
if [[ -f "${ETH_FILE}" ]]; then
  echo "[eth] reusing existing key at ${ETH_FILE}"
  ETH_ADDR=$(python3 -c "
import json, sys
d = json.load(open('${ETH_FILE}'))
print(d['address'])
")
else
  echo "[eth] generating new key → ${ETH_FILE}"
  ETH_ADDR=$(python3 -c "
from eth_account import Account
import json, os, secrets
Account.enable_unaudited_hdwallet_features()
acct = Account.create(secrets.token_hex(32))
out = {
    'address': acct.address,
    'private_key': acct.key.hex(),
}
with open('${ETH_FILE}', 'w') as f:
    json.dump(out, f)
os.chmod('${ETH_FILE}', 0o600)
print(acct.address)
")
fi
echo "[eth] public address: ${ETH_ADDR}"

echo
echo "════════════════════════════════════════════════════════════════════"
echo "  RECEIVE WALLETS — addresses are PUBLIC, safe to share."
echo "  Private keys are at:"
echo "    ${SOL_FILE}    (chmod 600)"
echo "    ${ETH_FILE}    (chmod 600)"
echo "  BACK THESE UP IMMEDIATELY (encrypted USB, password manager, etc.)"
echo "════════════════════════════════════════════════════════════════════"
echo

# ── 3. QR codes ───────────────────────────────────────────────────────
print_qr() {
  local label=$1; local data=$2
  echo "── ${label} ──"
  echo "  ${data}"
  qrencode -t ANSI256UTF8 -m 1 "${data}"
  echo
}

print_qr "SOL  (raw address)"          "${SOL_ADDR}"
print_qr "SOL  (Solana Pay URI)"       "solana:${SOL_ADDR}"
print_qr "ETH  (raw address)"          "${ETH_ADDR}"
print_qr "ETH  (EIP-681 URI)"          "ethereum:${ETH_ADDR}"

# ── 4. Update Fly secrets ─────────────────────────────────────────────
if command -v fly >/dev/null 2>&1; then
  echo "── Fly secrets ──"
  echo "  setting MU_SOL_RECIPIENT and MU_ETH_RECIPIENT on -a mu-store"
  fly secrets set \
    MU_SOL_RECIPIENT="${SOL_ADDR}" \
    MU_ETH_RECIPIENT="${ETH_ADDR}" \
    -a mu-store 2>&1 | tail -6
else
  echo "[warn] fly CLI not on PATH; secrets not updated. Run manually:"
  echo "    fly secrets set MU_SOL_RECIPIENT=${SOL_ADDR} MU_ETH_RECIPIENT=${ETH_ADDR} -a mu-store"
fi

echo
echo "DONE."
