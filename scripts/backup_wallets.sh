#!/bin/bash
# backup_wallets.sh
#
# Creates an AES-256-CBC encrypted bundle of the receive-wallet private
# keys and stores it in:
#   ~/Library/Mobile Documents/com~apple~CloudDocs/mu-wallet-backup/  (iCloud)
#   ~/Dropbox/mu-wallet-backup/                                       (if Dropbox)
#   Always: ~/.mu-wallets/backup/                                     (local)
#
# Prompts for a passphrase (no echo). Decrypt later with:
#   openssl enc -d -aes-256-cbc -pbkdf2 -iter 200000 \
#     -in mu-wallets-YYYYMMDD.tar.gz.enc -out mu-wallets.tar.gz
#   tar -xzf mu-wallets.tar.gz
#
# This is a defense against MacBook loss / failure. NOT a substitute
# for transferring to a hardware wallet (Ledger/Trezor) for at-rest
# security. Use both.

set -eu

WALLET_DIR="${HOME}/.mu-wallets"
BACKUP_LOCAL="${WALLET_DIR}/backup"
DATE_STAMP=$(date +%Y%m%d)
TAR_NAME="mu-wallets-${DATE_STAMP}.tar.gz"
ENC_NAME="${TAR_NAME}.enc"

if [[ ! -f "${WALLET_DIR}/sol_recipient.json" ]] || [[ ! -f "${WALLET_DIR}/eth_recipient.json" ]]; then
  echo "❌ ${WALLET_DIR} is missing key files. Run gen_receive_wallets.sh first."
  exit 1
fi

mkdir -p "${BACKUP_LOCAL}"
chmod 700 "${BACKUP_LOCAL}"

# ── Build tarball (in a tmp under WALLET_DIR so it never hits /tmp) ──
TMP_TAR="${WALLET_DIR}/.tmp_${TAR_NAME}"
tar -czf "${TMP_TAR}" -C "${WALLET_DIR}" sol_recipient.json eth_recipient.json ADDRESSES.txt 2>/dev/null
chmod 600 "${TMP_TAR}"

# ── Encrypt ──────────────────────────────────────────────────────────
# Passphrase source priority:
#   1. ${MU_BACKUP_PASSPHRASE_FILE} env var pointing to a file
#   2. ~/.mu-wallets/.passphrase.tmp (auto-generated if missing)
#   3. Interactive prompt
PASS_FILE="${MU_BACKUP_PASSPHRASE_FILE:-${WALLET_DIR}/.passphrase.tmp}"
ENC_PATH="${BACKUP_LOCAL}/${ENC_NAME}"

if [[ ! -f "${PASS_FILE}" ]]; then
  echo "🎲 Generating fresh 48-char random passphrase → ${PASS_FILE}"
  umask 077
  LC_ALL=C tr -dc 'A-Za-z0-9!@#$%^&*()_+-=' </dev/urandom | head -c 48 > "${PASS_FILE}"
  echo "" >> "${PASS_FILE}"
  chmod 600 "${PASS_FILE}"
  echo "⚠️  The passphrase is now in ${PASS_FILE}."
  echo "    READ IT → SAVE TO 1Password/Bitwarden → THEN delete the file:"
  echo "       cat ${PASS_FILE}"
  echo "       (copy to password manager)"
  echo "       rm ${PASS_FILE}"
fi

openssl enc -aes-256-cbc -pbkdf2 -iter 200000 \
  -pass "file:${PASS_FILE}" \
  -in "${TMP_TAR}" -out "${ENC_PATH}"
chmod 600 "${ENC_PATH}"
rm -f "${TMP_TAR}"
echo "✅ Local backup: ${ENC_PATH}"

# ── Copy to iCloud if available ──────────────────────────────────────
ICLOUD_DIR="${HOME}/Library/Mobile Documents/com~apple~CloudDocs"
if [[ -d "${ICLOUD_DIR}" ]]; then
  ICLOUD_BACKUP="${ICLOUD_DIR}/mu-wallet-backup"
  mkdir -p "${ICLOUD_BACKUP}"
  cp "${ENC_PATH}" "${ICLOUD_BACKUP}/${ENC_NAME}"
  chmod 600 "${ICLOUD_BACKUP}/${ENC_NAME}"
  echo "✅ iCloud backup: ${ICLOUD_BACKUP}/${ENC_NAME}"
else
  echo "ℹ️  iCloud Drive not mounted; skipped iCloud copy."
fi

# ── Copy to Dropbox if available ─────────────────────────────────────
DROPBOX_DIR="${HOME}/Dropbox"
if [[ -d "${DROPBOX_DIR}" ]]; then
  DROPBOX_BACKUP="${DROPBOX_DIR}/mu-wallet-backup"
  mkdir -p "${DROPBOX_BACKUP}"
  cp "${ENC_PATH}" "${DROPBOX_BACKUP}/${ENC_NAME}"
  chmod 600 "${DROPBOX_BACKUP}/${ENC_NAME}"
  echo "✅ Dropbox backup: ${DROPBOX_BACKUP}/${ENC_NAME}"
fi

# ── Recovery instructions ────────────────────────────────────────────
RECOVERY_FILE="${BACKUP_LOCAL}/HOW_TO_RECOVER.txt"
cat > "${RECOVERY_FILE}" <<'EOF'
MU WALLET RECOVERY INSTRUCTIONS
═══════════════════════════════════════════════════════════════════

If your MacBook is lost/dead and you need to recover the receive wallets:

1. Get any of the encrypted backups:
     ~/.mu-wallets/backup/mu-wallets-YYYYMMDD.tar.gz.enc
     iCloud:  CloudDocs/mu-wallet-backup/mu-wallets-YYYYMMDD.tar.gz.enc
     Dropbox: Dropbox/mu-wallet-backup/mu-wallets-YYYYMMDD.tar.gz.enc

2. Decrypt:
     openssl enc -d -aes-256-cbc -pbkdf2 -iter 200000 \
       -in mu-wallets-YYYYMMDD.tar.gz.enc \
       -out mu-wallets.tar.gz

3. Extract:
     tar -xzf mu-wallets.tar.gz

4. You now have:
     sol_recipient.json   ← Solana keypair (JSON byte array)
     eth_recipient.json   ← Ethereum {address, private_key} JSON
     ADDRESSES.txt        ← Public addresses for cross-reference

5. Re-install:
     mkdir -p ~/.mu-wallets
     mv sol_recipient.json eth_recipient.json ADDRESSES.txt ~/.mu-wallets/
     chmod 700 ~/.mu-wallets
     chmod 600 ~/.mu-wallets/*

6. Verify addresses match the production secrets:
     solana-keygen pubkey ~/.mu-wallets/sol_recipient.json
     python3 -c "import json; print(json.load(open('$HOME/.mu-wallets/eth_recipient.json'))['address'])"

7. If the addresses are stale (Fly secrets point to different ones),
   update Fly:
     fly secrets set MU_SOL_RECIPIENT=<sol_addr> MU_ETH_RECIPIENT=<eth_addr> -a mu-store
EOF
chmod 600 "${RECOVERY_FILE}"
echo "✅ Recovery instructions: ${RECOVERY_FILE}"

echo
echo "════════════════════════════════════════════════════════════════════"
echo "  BACKUP COMPLETE — ${DATE_STAMP}"
echo
echo "  IMPORTANT NEXT STEPS:"
echo "   1. Test recovery now: copy the .enc to a different folder and"
echo "      decrypt it. If decryption fails, your passphrase is wrong."
echo "   2. Also save the passphrase to 1Password / Bitwarden — losing"
echo "      the passphrase is equivalent to losing the wallets."
echo "   3. Schedule a quarterly re-backup (./backup_wallets.sh) since"
echo "      the keys are unchanged but cloud copies may rot."
echo "════════════════════════════════════════════════════════════════════"
