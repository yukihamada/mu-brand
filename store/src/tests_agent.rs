//! Fast, network-free, DB-free regression tests for the agent auto-publish
//! gate. Lives as a `#[cfg(test)]` submodule of `agent_api` so it can reach
//! the module-private functions (`assess_product_risk`, `kind_from_sku`,
//! `is_trusted_design_host`). New file + a single `mod` line in agent_api.rs
//! keeps it out of the way of in-flight WIP.

use super::{assess_product_risk, is_trusted_design_host, kind_from_sku, store_write_allowed};

// ── store collaborator write-access gate ─────────────────────────────────────

#[test]
fn store_write_allowed_owner_and_collaborators() {
    let collabs = vec!["kenny@atsume.io".to_string(), "Bob@Example.com".to_string()];
    // owner qualifies (case-insensitive)
    assert!(store_write_allowed(Some("Yuki@Hamada.Tokyo"), &collabs, "yuki@hamada.tokyo"));
    // collaborators qualify (case-insensitive both sides)
    assert!(store_write_allowed(Some("yuki@hamada.tokyo"), &collabs, "KENNY@atsume.io"));
    assert!(store_write_allowed(Some("yuki@hamada.tokyo"), &collabs, "bob@example.com"));
    // a stranger is rejected
    assert!(!store_write_allowed(Some("yuki@hamada.tokyo"), &collabs, "eve@evil.com"));
    // no owner + empty allowlist → rejected (pre-seeded brand stays locked)
    assert!(!store_write_allowed(None, &[], "anyone@example.com"));
    // collaborator present but owner None still works
    assert!(store_write_allowed(None, &collabs, "kenny@atsume.io"));
}

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
    // Physical BJJ goods added 2026-06-08 (single-token kinds).
    assert_eq!(kind_from_sku("MU-AGENT-TOTE-11223344"), Some("tote"));
    assert_eq!(kind_from_sku("MU-AGENT-TANK-aabbccdd"), Some("tank"));
    assert_eq!(kind_from_sku("MU-AGENT-CAP-0f0f0f0f"), Some("cap"));
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

// ── (c) print position: %→box math + per-product gating ──────────────────────

#[test]
fn pct_print_box_resolves_center_and_edges() {
    // Full width, any x → side 1800, left 0; y=50 centers vertically.
    assert_eq!(crate::catalog::pct_print_box(100.0, 50.0, 50.0), (1800, 0, 300));
    // Half width centered: side 900, left (1800-900)/2, top (2400-900)/2.
    assert_eq!(crate::catalog::pct_print_box(50.0, 50.0, 50.0), (900, 450, 750));
    // Corners pin to the print-area bounds.
    assert_eq!(crate::catalog::pct_print_box(50.0, 0.0, 0.0), (900, 0, 0));
    assert_eq!(crate::catalog::pct_print_box(50.0, 100.0, 100.0), (900, 900, 1500));
}

#[test]
fn pct_print_box_clamps_out_of_range_inputs() {
    // w below 20% clamps to the 360px floor; x/y clamp into 0-100.
    let (side, left, top) = crate::catalog::pct_print_box(5.0, -50.0, 999.0);
    assert_eq!(side, 360);
    assert_eq!(left, 0);
    assert_eq!(top, 2400 - 360);
    // w above 100 clamps to full area (left/top forced to 0 by no remaining space).
    assert_eq!(crate::catalog::pct_print_box(150.0, 100.0, 100.0), (1800, 0, 600));
}

#[test]
fn position_gating_is_dtg_front_print_only() {
    // tee(71)/hoodie(146)/crewneck(145)/tank(539)/long_sleeve(356) are editable…
    for pp in [71, 146, 145, 539, 356] {
        assert!(crate::catalog::position_editable_product(pp), "pp={} must be editable", pp);
    }
    // …AOP rashguards, mug, sticker, poster, cap and digital (0) are NOT.
    for pp in [301, 302, 19, 358, 1, 99, 601, 0, -1] {
        assert!(!crate::catalog::position_editable_product(pp), "pp={} must NOT be editable", pp);
    }
}
