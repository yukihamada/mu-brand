import XCTest
@testable import MU

// Network-free unit tests for the app's data layer. These pin the JSON
// contract with the wearmu.com REST API (Models.swift CodingKeys) and the
// kind whitelists that gate App Store 3.1.1 compliance (no digital goods)
// and must stay in lockstep with the server's catalog.rs allow-lists.
final class ModelsTests: XCTestCase {

    // MARK: - FeedProduct (the BJJ funnel unit: gallery + Live + Shop grid)

    func testFeedProductDecodesAndExposesBuyLinks() throws {
        // Mirrors a real /api/shop/feed.json row for a BJJ product. The buy
        // funnel depends on pdp_url / checkout_url surviving the decode —
        // if CodingKeys drift, the app's "buy" button breaks silently.
        let json = """
        {
          "sku": "MU-BJJ-01-TEE-BLACK",
          "brand": "bjj",
          "description": "TAP EARLY TAP OFTEN — 柔術が分かる人に刺さる",
          "price_jpy": 4900,
          "mockup_url": "https://merch.wearmu.com/bjj/mock_01.jpg",
          "sold": 12,
          "created_at": "2026-06-14 03:20:04",
          "pdp_url": "https://wearmu.com/shop/MU-BJJ-01-TEE-BLACK",
          "checkout_url": "https://wearmu.com/api/shop/checkout?sku=MU-BJJ-01-TEE-BLACK"
        }
        """.data(using: .utf8)!

        let p = try JSONDecoder().decode(FeedProduct.self, from: json)
        XCTAssertEqual(p.sku, "MU-BJJ-01-TEE-BLACK")
        XCTAssertEqual(p.brand, "bjj")
        XCTAssertEqual(p.priceJpy, 4900)
        XCTAssertEqual(p.id, p.sku, "Identifiable id must be the sku")
        XCTAssertEqual(p.checkoutUrl, "https://wearmu.com/api/shop/checkout?sku=MU-BJJ-01-TEE-BLACK")
        XCTAssertEqual(p.mockupURL?.scheme, "https")
        XCTAssertNotNil(p.pdpUrl.range(of: "/shop/"), "PDP link should point at the shop PDP")
    }

    func testFeedProductPriceLabelFormatsYen() throws {
        let json = """
        {"sku":"X","brand":"mu","description":"d","price_jpy":12345,
         "sold":0,"created_at":"2026-01-01 00:00:00",
         "pdp_url":"https://wearmu.com/shop/X",
         "checkout_url":"https://wearmu.com/api/shop/checkout?sku=X"}
        """.data(using: .utf8)!
        let p = try JSONDecoder().decode(FeedProduct.self, from: json)
        // grouping separator is locale-dependent; assert the yen sign + digits survive
        XCTAssertTrue(p.priceLabel.hasPrefix("¥"))
        XCTAssertTrue(p.priceLabel.contains("12") && p.priceLabel.contains("345"))
        XCTAssertNil(p.mockupURL, "absent mockup_url decodes to nil")
    }

    func testFeedProductParsesUtcCreatedAt() throws {
        let json = """
        {"sku":"X","brand":"mu","description":"d","price_jpy":100,
         "sold":0,"created_at":"2026-06-14 03:20:04",
         "pdp_url":"https://wearmu.com/shop/X",
         "checkout_url":"https://wearmu.com/api/shop/checkout?sku=X"}
        """.data(using: .utf8)!
        let p = try JSONDecoder().decode(FeedProduct.self, from: json)
        let d = try XCTUnwrap(p.createdDate, "SQLite UTC timestamp must parse")
        // 2026-06-14T03:20:04Z
        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = TimeZone(identifier: "UTC")!
        let c = cal.dateComponents([.year, .month, .day, .hour], from: d)
        XCTAssertEqual(c.year, 2026)
        XCTAssertEqual(c.month, 6)
        XCTAssertEqual(c.day, 14)
        XCTAssertEqual(c.hour, 3)
    }

    func testFeedPageDecodesSnakeCasePageSize() throws {
        let json = """
        {"page":2,"page_size":24,"products":[]}
        """.data(using: .utf8)!
        let page = try JSONDecoder().decode(FeedPage.self, from: json)
        XCTAssertEqual(page.page, 2)
        XCTAssertEqual(page.pageSize, 24)
        XCTAssertTrue(page.products.isEmpty)
    }

    // MARK: - MakeResult (the "make your own" escape hatch the BJJ PDP links to)

    func testMakeResultDecodesBuyLinks() throws {
        let json = """
        {"ok":true,"sku":"AUTO-BJJ-x","kind":"tee","display":"d","hook":"h",
         "retail_jpy":4900,"design_url":"https://wearmu.com/d.png",
         "pdp_url":"https://wearmu.com/shop/AUTO-BJJ-x","status":"live",
         "auto_approved":true,"buy_url":"https://wearmu.com/buy/AUTO-BJJ-x",
         "checkout_url":"https://wearmu.com/api/shop/checkout?sku=AUTO-BJJ-x",
         "note":"n","edit_token":"tok","maker_pct":10,"maker_earn_jpy":490}
        """.data(using: .utf8)!
        let r = try JSONDecoder().decode(MakeResult.self, from: json)
        XCTAssertTrue(r.ok)
        XCTAssertEqual(r.kind, "tee")
        XCTAssertEqual(r.retailJpy, 4900)
        XCTAssertEqual(r.checkoutUrl, "https://wearmu.com/api/shop/checkout?sku=AUTO-BJJ-x")
        XCTAssertEqual(r.designURL?.absoluteString, "https://wearmu.com/d.png")
        XCTAssertEqual(r.makerPct, 10)
    }

    // MARK: - DesignScore (5-axis MU score; ordering is part of the contract)

    func testDesignScoreOrdersAxesCanonically() throws {
        let json = """
        {"total":88,"verdict":"strong",
         "axes":{"desire":90,"visual":85,"craft":88,"concept":92,"universality":80}}
        """.data(using: .utf8)!
        let s = try JSONDecoder().decode(DesignScore.self, from: json)
        XCTAssertEqual(s.total, 88)
        let keys = s.orderedAxes.map { $0.0 }
        XCTAssertEqual(keys, ["visual", "universality", "craft", "concept", "desire"],
                       "axes must render in the fixed visual→desire order")
    }

    func testDesignScoreOrderedAxesSkipsMissingKeys() throws {
        let json = """
        {"total":50,"verdict":"weak","axes":{"visual":50,"craft":50}}
        """.data(using: .utf8)!
        let s = try JSONDecoder().decode(DesignScore.self, from: json)
        XCTAssertEqual(s.orderedAxes.map { $0.0 }, ["visual", "craft"])
    }

    // MARK: - kind whitelists (App Store 3.1.1 + server catalog.rs contract)

    func testProductKindWhitelistHasNoDigitalGoods() {
        let raws = Set(ProductKind.allCases.map { $0.rawValue })
        // App Store 3.1.1: digital goods (song/house/zine/video/ticket) must
        // NOT be sellable in-app — they must never appear as a shop filter.
        XCTAssertEqual(raws, ["", "tee", "rashguard", "hoodie", "sticker"])
        for forbidden in ["song", "house", "zine", "video", "event_ticket", "device"] {
            XCTAssertFalse(raws.contains(forbidden),
                           "\(forbidden) is digital/non-apparel and must not be a Shop filter")
        }
    }

    func testMakeKindRawValuesMatchServerAllowList() {
        let raws = Set(MakeKind.allCases.map { $0.rawValue })
        // These raw values are sent verbatim as ?kind= to /api/make and must
        // match the server's allowed list (catalog.rs). "" = AI auto-pick.
        XCTAssertEqual(raws, ["", "tee", "hoodie", "sticker", "rashguard_ls", "tote", "mug"])
        XCTAssertTrue(raws.contains("rashguard_ls"),
                      "rashguard maps to the server kind 'rashguard_ls', not 'rashguard'")
    }
}
