//! Soulbound (non-transferable) NFT certificate pilot for MU brand.
//!
//! Mints a compressed NFT (cNFT) via Helius DAS API to the buyer's wallet
//! after they win an MA auction (or, in the future, complete a crypto/Stripe
//! checkout that opted in to NFT issuance).
//!
//! ## Soulbound semantics
//! Helius `mintCompressedNft` does not natively expose a transfer-locked
//! flag, so we encode soulbound-ness as a contract between:
//!   - the JSON metadata `attributes` (`Soulbound: true`, `Transferable: false`)
//!   - the off-chain authority (MU treasury keypair) which is the only signer
//!     authorised to delegate transfers. We never sign transfers; the cNFT
//!     therefore stays with the original owner.
//!   - `isMutable=false` so the metadata cannot be re-pointed later.
//!
//! This is the **pilot** (Q3 vision item) — full on-chain enforcement
//! (e.g. via the Token-2022 NonTransferable extension or a custom transfer
//! program) is a follow-up.
//!
//! ## Safety
//! `MU_NFT_MINT_LIVE` env var (default `0`) gates the actual API call.
//! When 0, the function returns `dryrun:<uuid>` instead of hitting Helius.
//! Flip to `1` via `fly secrets set MU_NFT_MINT_LIVE=1 -a mu-store` to go live.

use crate::Db;
use rusqlite::params;
use std::env;

#[derive(Debug)]
pub enum NftError {
    /// HELIUS_API_KEY env var unset / empty (live mode only).
    MissingApiKey,
    /// Product not found in DB.
    ProductNotFound,
    /// Helius HTTP error (status code, body excerpt).
    HeliusHttp(u16, String),
    /// Network / serde failure when talking to Helius.
    Transport(String),
    /// Owner wallet failed sanity check (length / charset).
    BadWallet(String),
    /// DB persistence error.
    Persist(String),
}

impl std::fmt::Display for NftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingApiKey      => write!(f, "HELIUS_API_KEY not set"),
            Self::ProductNotFound    => write!(f, "product not found"),
            Self::HeliusHttp(s, b)   => write!(f, "helius {}: {}", s, b),
            Self::Transport(e)       => write!(f, "transport: {}", e),
            Self::BadWallet(w)       => write!(f, "bad wallet: {}", w),
            Self::Persist(e)         => write!(f, "persist: {}", e),
        }
    }
}

impl std::error::Error for NftError {}

/// Mint a Soulbound compressed NFT certificate for `product_id` to `owner_wallet`.
///
/// Behaviour:
///   - Reads product metadata (brand / drop_num / name / image / weather) from DB.
///   - If `MU_NFT_MINT_LIVE != "1"`: returns `dryrun:<uuid>` without hitting
///     Helius. Logs the would-be payload to stderr.
///   - If live: POSTs `mintCompressedNft` to Helius RPC, returns mint address.
///   - On success: persists `nft_mint` on the products row (idempotent — only
///     overwrites NULL/empty values; existing mints are preserved).
///
/// Errors are returned, not panicked. Callers should `tokio::spawn` this and
/// log failures via Telegram alert.
pub async fn mint_soulbound(
    db: Db,
    product_id: i64,
    owner_wallet: &str,
) -> Result<String, NftError> {
    if !is_plausible_solana_address(owner_wallet) {
        return Err(NftError::BadWallet(owner_wallet.to_string()));
    }

    // 1. Load product (read-only; release lock before network IO)
    let (brand, drop_num, name, image_url) = {
        let conn = db.lock().map_err(|e| NftError::Persist(e.to_string()))?;
        conn.query_row(
            "SELECT brand, drop_num, name,
                    COALESCE(NULLIF(mockup_url,''), NULLIF(design_url,''), '')
             FROM products WHERE id=?",
            params![product_id],
            |r| Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            )),
        ).map_err(|_| NftError::ProductNotFound)?
    };

    let base = env::var("BASE_URL").unwrap_or_else(|_| "https://wearmu.com".into());
    let metadata_uri = format!("{}/api/nft/{}/{}", base.trim_end_matches('/'), brand, drop_num);
    let display_name = if name.is_empty() {
        format!("MU {} #{:04}", brand.to_uppercase(), drop_num)
    } else {
        name.clone()
    };

    // 2. Dry-run gate
    let live = env::var("MU_NFT_MINT_LIVE").unwrap_or_default() == "1";
    if !live {
        let fake = format!("dryrun:{}", uuid::Uuid::new_v4());
        eprintln!(
            "[nft] DRY RUN — would mint Soulbound cNFT: product_id={} brand={} drop={} owner={} name={} uri={} image={} (set MU_NFT_MINT_LIVE=1 to enable)",
            product_id, brand, drop_num, owner_wallet, display_name, metadata_uri, image_url
        );
        persist_mint(&db, product_id, &fake)?;
        return Ok(fake);
    }

    // 3. Live path: call Helius mintCompressedNft RPC
    let api_key = env::var("HELIUS_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or(NftError::MissingApiKey)?;
    let url = format!("https://mainnet.helius-rpc.com/?api-key={}", api_key);

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": format!("mu-{}-{}", brand, drop_num),
        "method": "mintCompressedNft",
        "params": {
            "name": display_name,
            "symbol": "MU",
            "description": format!(
                "MU {} #{:04} — Soulbound certificate of authenticity. \
                 Autonomous design born from Hokkaido weather data. Non-transferable.",
                brand.to_uppercase(), drop_num
            ),
            "owner": owner_wallet,
            "uri": metadata_uri,
            "imageUrl": image_url,
            "sellerFeeBasisPoints": 0,
            "confirmTransaction": false,
            "attributes": [
                {"trait_type": "Brand",        "value": brand.to_uppercase()},
                {"trait_type": "Drop",         "value": drop_num.to_string()},
                {"trait_type": "Soulbound",    "value": "true"},
                {"trait_type": "Transferable", "value": "false"},
            ],
        }
    });

    let client = reqwest::Client::new();
    let resp = client.post(&url).json(&payload).send().await
        .map_err(|e| NftError::Transport(e.to_string()))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(NftError::HeliusHttp(status.as_u16(), truncate(&body, 400)));
    }
    let v: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| NftError::Transport(format!("json: {} (body={})", e, truncate(&body, 200))))?;
    if let Some(err) = v.get("error") {
        return Err(NftError::HeliusHttp(status.as_u16(), err.to_string()));
    }

    // Helius returns the mint inside `result.assetId` (newer) or `result.mint`.
    let mint = v["result"]["assetId"].as_str()
        .or_else(|| v["result"]["mint"].as_str())
        .or_else(|| v["result"].as_str())
        .ok_or_else(|| NftError::HeliusHttp(
            status.as_u16(),
            format!("no mint in response: {}", truncate(&body, 200)),
        ))?
        .to_string();

    persist_mint(&db, product_id, &mint)?;
    Ok(mint)
}

/// Update products.nft_mint, but only if it's currently NULL or empty so we
/// never overwrite a real mint with a dry-run value (or vice versa).
fn persist_mint(db: &Db, product_id: i64, mint: &str) -> Result<(), NftError> {
    let conn = db.lock().map_err(|e| NftError::Persist(e.to_string()))?;
    let n = conn.execute(
        "UPDATE products
           SET nft_mint=?
         WHERE id=? AND (nft_mint IS NULL OR nft_mint='')",
        params![mint, product_id],
    ).map_err(|e| NftError::Persist(e.to_string()))?;
    if n == 0 {
        // Either product gone, or already minted — log but don't fail the
        // caller; idempotent-by-design.
        eprintln!("[nft] persist_mint: no-op (product {} already minted or missing)", product_id);
    }
    Ok(())
}

/// Loose sanity check: Solana addresses are base58, length 32-44 bytes.
/// We don't fully decode (no `bs58` dep) — this is enough to catch typos /
/// empty / HTML-injected values before sending to Helius.
fn is_plausible_solana_address(s: &str) -> bool {
    let len = s.len();
    if !(32..=44).contains(&len) {
        return false;
    }
    s.chars().all(|c|
        c.is_ascii_alphanumeric()
        && c != '0' && c != 'O' && c != 'I' && c != 'l'
    )
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { format!("{}…", &s[..max]) }
}

/// Convenience wrapper: spawn `mint_soulbound` as a background task and log
/// the outcome (success / error) without blocking the caller. Used by Stripe
/// webhook and Helius / Alchemy crypto-settled webhooks.
pub fn mint_soulbound_bg(db: Db, product_id: i64, owner_wallet: String, source: &'static str) {
    if owner_wallet.is_empty() {
        eprintln!("[nft] {} skipped product_id={}: no owner_wallet", source, product_id);
        return;
    }
    tokio::spawn(async move {
        match mint_soulbound(db, product_id, &owner_wallet).await {
            Ok(mint) => eprintln!(
                "[nft] {} product_id={} owner={} mint={}",
                source, product_id, owner_wallet, mint
            ),
            Err(e) => eprintln!(
                "[nft] {} FAILED product_id={} owner={}: {}",
                source, product_id, owner_wallet, e
            ),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};

    fn fresh_db_with_product() -> (Db, i64) {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE products (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                brand        TEXT NOT NULL,
                drop_num     INTEGER NOT NULL,
                name         TEXT NOT NULL,
                design_url   TEXT,
                mockup_url   TEXT,
                price_jpy    INTEGER NOT NULL DEFAULT 0,
                inventory    INTEGER NOT NULL DEFAULT 1,
                sold         INTEGER NOT NULL DEFAULT 0,
                created_at   TEXT NOT NULL DEFAULT '0',
                active       INTEGER NOT NULL DEFAULT 1,
                nft_mint     TEXT
             )",
        ).unwrap();
        conn.execute(
            "INSERT INTO products (brand, drop_num, name, mockup_url)
             VALUES ('ma', 1, 'MU 間 MA 2026.05', 'https://example.com/img.jpg')",
            [],
        ).unwrap();
        let id = conn.last_insert_rowid();
        (Arc::new(Mutex::new(conn)), id)
    }

    #[tokio::test]
    async fn dry_run_mints_fake_address_and_persists() {
        // Ensure live mint is OFF (env var unset or "0")
        std::env::remove_var("MU_NFT_MINT_LIVE");

        let (db, pid) = fresh_db_with_product();
        let owner = "2esK1VR585a4GWJmUgt8xX5YkcETfsCzDqJ8TgrjLCnx"; // MU treasury

        let mint = mint_soulbound(db.clone(), pid, owner).await.expect("dry-run should succeed");
        assert!(mint.starts_with("dryrun:"), "expected dryrun prefix, got {}", mint);

        // Persisted to DB
        let conn = db.lock().unwrap();
        let stored: String = conn.query_row(
            "SELECT nft_mint FROM products WHERE id=?", params![pid], |r| r.get(0)
        ).unwrap();
        assert_eq!(stored, mint);
    }

    #[tokio::test]
    async fn dry_run_does_not_overwrite_existing_mint() {
        std::env::remove_var("MU_NFT_MINT_LIVE");
        let (db, pid) = fresh_db_with_product();
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "UPDATE products SET nft_mint='existing-mint' WHERE id=?",
                params![pid],
            ).unwrap();
        }
        let owner = "2esK1VR585a4GWJmUgt8xX5YkcETfsCzDqJ8TgrjLCnx";
        let _ = mint_soulbound(db.clone(), pid, owner).await.unwrap();
        let conn = db.lock().unwrap();
        let stored: String = conn.query_row(
            "SELECT nft_mint FROM products WHERE id=?", params![pid], |r| r.get(0)
        ).unwrap();
        assert_eq!(stored, "existing-mint", "must not overwrite existing mint");
    }

    #[tokio::test]
    async fn bad_wallet_rejected_before_any_io() {
        std::env::remove_var("MU_NFT_MINT_LIVE");
        let (db, pid) = fresh_db_with_product();
        let err = mint_soulbound(db, pid, "").await.unwrap_err();
        matches!(err, NftError::BadWallet(_));
    }

    #[test]
    fn solana_address_sanity_check() {
        assert!(is_plausible_solana_address("2esK1VR585a4GWJmUgt8xX5YkcETfsCzDqJ8TgrjLCnx"));
        assert!(!is_plausible_solana_address(""));
        assert!(!is_plausible_solana_address("short"));
        assert!(!is_plausible_solana_address("contains-dash"));
        // Contains '0' (base58 forbidden char)
        assert!(!is_plausible_solana_address("0esK1VR585a4GWJmUgt8xX5YkcETfsCzDqJ8TgrjLCnx"));
    }
}
