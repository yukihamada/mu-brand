//! Fast, network-free, DB-free regression tests for the agent auto-publish
//! gate. Lives as a `#[cfg(test)]` submodule of `agent_api` so it can reach
//! the module-private functions (`assess_product_risk`, `kind_from_sku`,
//! `is_trusted_design_host`). New file + a single `mod` line in agent_api.rs
//! keeps it out of the way of in-flight WIP.

use super::{assess_product_risk, is_trusted_design_host, kind_from_sku};

// ── (a) assess_product_risk: the auto-publish risk gate ──────────────────────

#[test]
fn risk_blocks_trademark_symbol_in_copy() {
    // ™ / ® / © in customer-facing copy → must stay in review.
    let r = assess_product_risk("Cool Tee™", "a nice shirt", None, "");
    assert!(r.is_some(), "trademark symbol must be flagged");
    assert!(r.unwrap().contains("trademark/copyright symbol"));
}

#[test]
fn risk_blocks_known_brand_substring() {
    // Distinctive brand/IP substring anywhere in label/description/prompt.
    let r = assess_product_risk("Belt Hoodie", "inspired by Gucci vibes", None, "");
    assert!(r.is_some(), "brand substring must be flagged");
    assert!(r.unwrap().to_lowercase().contains("gucci"));

    // Japanese IP term too.
    let r2 = assess_product_risk("Tシャツ", "ジブリ風のデザイン", None, "");
    assert!(r2.is_some(), "JP IP term must be flagged");
}

#[test]
fn risk_blocks_celebrity_token_but_not_substring_false_positive() {
    // Whole-token brand match: "nike" as a token must flag …
    let flagged = assess_product_risk("Run Tee", "go nike go", None, "");
    assert!(flagged.is_some(), "'nike' token must be flagged");

    // … but the same letters embedded in an unrelated word must NOT flag
    // (token-boundary logic guards against e.g. pineapple matching "apple").
    let clean = assess_product_risk("Fruit Tee", "fresh pineapple print", None, "");
    assert!(
        clean.is_none(),
        "substring inside an unrelated word must not false-positive, got {:?}",
        clean
    );
}

#[test]
fn risk_blocks_external_image_domain() {
    // design_file pointing at a host we don't control → review.
    let r = assess_product_risk("Plain Tee", "minimal", None, "https://evil.example.com/x.png");
    assert!(r.is_some(), "untrusted external image host must be flagged");
    assert!(r.unwrap().contains("external image"));
}

#[test]
fn risk_blocks_inappropriate_language() {
    let r = assess_product_risk("Tee", "this is porn", None, "");
    assert!(r.is_some(), "NSFW term must be flagged");
    assert_eq!(r.unwrap(), "inappropriate language");
}

#[test]
fn risk_passes_clean_product() {
    // Safe label + safe copy + trusted CDN host → auto-publish (None).
    let r = assess_product_risk(
        "墨黒の朝 Tee",
        "A clean minimal tee. Sumi-black on natural.",
        Some("sumi ink brushstroke, minimal"),
        "https://mockups.wearmu.com/abc.png",
    );
    assert!(r.is_none(), "clean product must pass the gate, got {:?}", r);

    // Also clean with no design_file at all.
    let r2 = assess_product_risk("Quiet Tee", "soft cotton", None, "");
    assert!(r2.is_none(), "clean product (no image) must pass, got {:?}", r2);
}

#[test]
fn trusted_hosts_recognized() {
    assert!(is_trusted_design_host("https://mockups.wearmu.com/x.png"));
    assert!(is_trusted_design_host("https://files.cdn.printful.com/y.png"));
    assert!(is_trusted_design_host("https://foo.r2.dev/z.png"));
    assert!(is_trusted_design_host(
        "https://raw.githubusercontent.com/yukihamada/mu-mockups/main/a.png"
    ));
    assert!(!is_trusted_design_host("https://evil.example.com/x.png"));
    // Another user's GitHub raw must NOT be trusted.
    assert!(!is_trusted_design_host(
        "https://raw.githubusercontent.com/someoneelse/repo/main/a.png"
    ));
}

// ── (b) kind_from_sku: SKU-embedded kind regression ──────────────────────────

#[test]
fn kind_from_sku_parses_embedded_kind() {
    // SKU shape: "<STORE>-AGENT-<KIND>-<rand>" ; KIND may contain hyphens
    // which map to underscores (e.g. RASHGUARD-LS → rashguard_ls).
    assert_eq!(kind_from_sku("FEST-AGENT-TEE-487c1988"), Some("tee"));
    assert_eq!(kind_from_sku("MU-AGENT-RASHGUARD-LS-abc123"), Some("rashguard_ls"));
    assert_eq!(kind_from_sku("MU-AGENT-EVENT-TICKET-deadbeef"), Some("event_ticket"));
    assert_eq!(kind_from_sku("MU-AGENT-SONG-00112233"), Some("song"));
    assert_eq!(kind_from_sku("KOE-AGENT-DEVICE-fa867d59"), Some("device"));
}

#[test]
fn kind_from_sku_rejects_unknown_or_malformed() {
    // No AGENT marker.
    assert_eq!(kind_from_sku("RANDOM-SKU-123"), None);
    // Unknown kind token.
    assert_eq!(kind_from_sku("MU-AGENT-FLYINGCAR-123"), None);
    // Empty.
    assert_eq!(kind_from_sku(""), None);
}
