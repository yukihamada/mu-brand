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

    private func decode<T: Decodable>(_ type: T.Type, _ json: String) throws -> T {
        try JSONDecoder().decode(type, from: Data(json.utf8))
    }

    private func makeResult(extra: String = "") throws -> MakeResult {
        try decode(MakeResult.self, """
        {"ok":true,"sku":"MAKE-X","kind":"tote","display":"d","hook":"h",
         "retail_jpy":3800,"design_url":"https://example.test/old.png",
         "pdp_url":"https://example.test/shop/MAKE-X","status":"live",
         "auto_approved":true,"note":"n","edit_token":"test","maker_pct":20,
         "maker_earn_jpy":760\(extra)}
        """)
    }

    func testDynamicCatalogAcceptsMoreThan80KindsAndSearchesBothLanguages() throws {
        var rows = (0..<85).map { index in
            ["kind": "physical_\(index)", "label_ja": "物理商品\(index)", "label_en": "Physical product \(index)",
             "retail_jpy": 4000 + index * 100, "category": "home", "can_remix": false] as [String: Any]
        }
        rows.append(["kind": "pet_bowl", "label_ja": "ペットボウル", "label_en": "Pet bowl",
                     "retail_jpy": 5100, "category": "pet", "can_remix": false])
        let data = try JSONSerialization.data(withJSONObject: ["ok": true, "items": rows])
        let catalog = try JSONDecoder().decode(MakeKindsResponse.self, from: data)
        XCTAssertEqual(catalog.items.count, 86)
        let bowl = try XCTUnwrap(catalog.items.last)
        XCTAssertTrue(bowl.matches("ボウル"))
        XCTAssertTrue(bowl.matches(" PET BOWL "))
        XCTAssertTrue(bowl.matches("pet_bowl"))
        XCTAssertFalse(bowl.matches("hoodie"))
        XCTAssertEqual(bowl.estimatedPrice(royalty: 10), 5100)
    }

    func testFallbackHasOnlyPreviouslySupportedKindsAndNoInventedAutoPrice() throws {
        XCTAssertEqual(Set(MakeKindOption.fallback.map(\.kind)), Set(MakeKind.allCases.map(\.rawValue)))
        XCTAssertNil(MakeKindOption.auto.estimatedPrice(royalty: 50))
        let tote = try XCTUnwrap(MakeKindOption.fallback.first { $0.kind == "tote" })
        XCTAssertEqual(tote.estimatedPrice(royalty: 10), 3800)
        XCTAssertEqual(tote.estimatedPrice(royalty: 50), 6800)
        XCTAssertTrue(MakeKindOption.fallback.allSatisfy { !$0.canRemix })
        XCTAssertTrue(MakeKindOption.fallback.allSatisfy(\.isAvailable))
    }

    func testUnavailableContradoKindWithNullPriceIsNotSelectable() throws {
        // Missing PRODUCT_SPECS row: the canonical kind remains visible but cannot be made.
        for capability in ["", ",\"can_make\":false", ",\"can_make\":true"] {
            let option = try decode(MakeKindOption.self, """
            {"kind":"rashguard_contrado","label_ja":"ラッシュガード（完全プリント・プレミアム）",
             "label_en":"Rashguard (full print, premium)","retail_jpy":null,
             "category":"wear","can_remix":false\(capability)}
            """)
            XCTAssertTrue(option.matches("contrado"), "Unavailable kinds stay discoverable")
            XCTAssertFalse(option.isAvailable)
            XCTAssertFalse(MakeKindOption.isAvailable(option.kind, in: [option]))
            XCTAssertNil(option.estimatedPrice(royalty: 10))
        }
    }

    func testCanMakeDenialOverridesPriceAndLegacyKindsRemainAvailable() throws {
        for (capability, available) in [("", true), (",\"can_make\":true", true), (",\"can_make\":false", false)] {
            let option = try decode(MakeKindOption.self, """
            {"kind":"tote","label_ja":"トート","label_en":"Tote","retail_jpy":3800,
             "category":"carry","can_remix":true\(capability)}
            """)
            XCTAssertEqual(option.isAvailable, available)
            XCTAssertEqual(MakeKindOption.isAvailable("tote", in: [option]), available)
            XCTAssertEqual(option.estimatedPrice(royalty: 10), available ? 3800 : nil)
        }
        XCTAssertTrue(MakeKindOption.auto.isAvailable, "Auto has no fixed price")
        XCTAssertFalse(MakeKindOption.isAvailable("removed_kind", in: MakeKindOption.fallback))
    }

    func testExactServerPrintfulPeekPayloadCompletesPreview() throws {
        // Fields/types match catalog.rs make_preview_payload, including is_model:false.
        let peek = try decode(PeekResult.self, """
        {"ok":true,"sku":"MAKE-X","status":"live","design_url":"https://example.test/old.png",
         "mockup":"https://example.test/printful.png","preview_kind":"printful",
         "preview_revision":1,"ready":true,"is_model":false}
        """)
        var variant = DesignVariant(result: try makeResult())
        XCTAssertEqual(peek.previewKind, .printful)
        XCTAssertEqual(peek.previewRevision?.value, "1")
        XCTAssertTrue(variant.applyPeek(peek))
        XCTAssertEqual(variant.shownURL?.absoluteString, "https://example.test/printful.png")
        XCTAssertFalse(variant.previewPending)
        XCTAssertEqual(variant.previewLabelKey, "make.preview.reference")
    }

    func testServerCardAndDesignPayloadsKeepPollingUntilPrintfulReady() throws {
        for (kind, mockup) in [("card", "\"https://example.test/card.png\""), ("design", "null")] {
            let peek = try decode(PeekResult.self, """
            {"ok":true,"sku":"MAKE-X","status":"live","design_url":"https://example.test/old.png",
             "mockup":\(mockup),"preview_kind":"\(kind)","preview_revision":1,"ready":false,"is_model":false}
            """)
            var variant = DesignVariant(result: try makeResult())
            XCTAssertNotEqual(peek.previewKind, .unknown)
            XCTAssertFalse(variant.applyPeek(peek))
            XCTAssertTrue(variant.previewPending)
            XCTAssertNil(variant.mockupURL)
        }
    }

    func testResetRejectsLateAutomaticManualRemixPriceAndPolishResponses() {
        for key in ["make", "variation", "remix", "price", "polish:MAKE-X", "peek:MAKE-X"] {
            var scope = MakeRequestScope()
            let generation = scope.generation
            let old = scope.begin(key)
            XCTAssertTrue(scope.accepts(generation, key: key, token: old))
            scope.reset()
            let new = scope.begin(key)
            XCTAssertFalse(scope.accepts(generation, key: key, token: old))
            XCTAssertTrue(scope.accepts(scope.generation, key: key, token: new))
        }
    }

    func testRestartingPeekRejectsOldResponseWithinSameGeneration() {
        var scope = MakeRequestScope()
        let generation = scope.generation
        let old = scope.begin("peek:X")
        let new = scope.begin("peek:X")
        XCTAssertFalse(scope.accepts(generation, key: "peek:X", token: old))
        XCTAssertTrue(scope.accepts(generation, key: "peek:X", token: new))
    }

    func testIntentCapturesInputAndRemixIsNotRetriedAsCreate() {
        var kind = "tote"
        var royalty = 20
        let intent = MakeIntent.variation(MakeInput(prompt: "moon", kind: kind, royalty: royalty))
        kind = "mug"; royalty = 50
        guard case .variation(let input) = intent else { return XCTFail("Wrong intent") }
        XCTAssertEqual(input.kind, "tote")
        XCTAssertEqual(input.royalty, 20)
        XCTAssertNotEqual(input.kind, kind)
        XCTAssertNotEqual(input.royalty, royalty)
        let retry = MakeIntent.remix(sku: "MAKE-X", words: "gold + blue")
        guard case .remix(let sku, let words) = retry else { return XCTFail("Remix lost") }
        XCTAssertEqual(sku, "MAKE-X")
        XCTAssertEqual(words, "gold + blue")
    }

    func testUnimprovedPolishKeepsOriginalScoreAndImage() throws {
        var variant = DesignVariant(result: try makeResult())
        let polish = try decode(PolishResult.self, """
        {"ok":true,"improved":false,"before":{"total":90,"axes":{},"verdict":"original"},
         "after":{"total":70,"axes":{},"verdict":"rejected"},
         "design_url":"https://example.test/rejected.png","note":"Kept original"}
        """)
        variant.applyPolish(polish)
        XCTAssertEqual(variant.score?.total, 90)
        XCTAssertEqual(variant.shownURL?.lastPathComponent, "old.png")
    }

    func testPolishRejectsLegacyOldDesignAndOldRevisionThenAcceptsNewPreview() throws {
        var variant = DesignVariant(result: try makeResult())
        let old = try decode(PeekResult.self, """
        {"ok":true,"sku":"MAKE-X","design_url":"https://example.test/old.png",
         "mockup":"https://example.test/old-mock.png","preview_kind":"mockup","preview_revision":1,"ready":true}
        """)
        XCTAssertTrue(variant.applyPeek(old))
        variant.applyPolish(try decode(PolishResult.self, """
        {"ok":true,"improved":true,"design_url":"https://example.test/new.png","note":"Improved"}
        """))
        XCTAssertFalse(variant.applyPeek(old))
        XCTAssertFalse(variant.applyPeek(try decode(PeekResult.self, """
        {"ok":true,"mockup":"https://example.test/old-mock.png","is_model":true}
        """)))
        let sameRevision = try decode(PeekResult.self, """
        {"ok":true,"design_url":"https://example.test/new.png","mockup":"https://example.test/old-mock.png",
         "preview_kind":"lifestyle","preview_revision":"1","ready":true}
        """)
        XCTAssertFalse(variant.applyPeek(sameRevision))
        XCTAssertEqual(variant.shownURL?.lastPathComponent, "new.png")
        let fresh = try decode(PeekResult.self, """
        {"ok":true,"design_url":"https://example.test/new.png","mockup":"https://example.test/new-mock.png",
         "preview_kind":"lifestyle","preview_revision":"2","ready":true,"is_model":true}
        """)
        XCTAssertTrue(variant.applyPeek(fresh))
        XCTAssertTrue(variant.applyPeek(fresh), "A repeated current revision is valid")
        XCTAssertEqual(variant.shownURL?.lastPathComponent, "new-mock.png")
        XCTAssertEqual(variant.previewLabelKey, "make.preview.reference")
    }

    func testTypedDesignIsNotProductPreviewAndWrongSKUDoesNotChangeStatus() throws {
        var variant = DesignVariant(result: try makeResult())
        let design = try decode(PeekResult.self, """
        {"ok":true,"sku":"MAKE-X","design_url":"https://example.test/old.png",
         "mockup":"https://example.test/reference.png","preview_kind":"design","ready":true}
        """)
        XCTAssertFalse(variant.applyPeek(design))
        XCTAssertNil(variant.mockupURL)
        XCTAssertEqual(variant.previewLabelKey, "make.preview.design")
        XCTAssertFalse(variant.applyPeek(try decode(PeekResult.self, """
        {"ok":true,"sku":"OTHER","status":"retired"}
        """)))
        XCTAssertTrue(variant.result.isLive)
        let unknown = try decode(PeekResult.self, """
        {"ok":true,"preview_kind":"future_kind","mockup":"https://example.test/a.png","ready":true}
        """)
        XCTAssertEqual(unknown.previewKind, .unknown)
        XCTAssertFalse(variant.applyPeek(unknown), "Unknown types must not be advertised as a finished mockup")
    }

    func testReviewLiveRetiredTransitionsUpdateBuyability() throws {
        var result = try makeResult()
        result.applyStatus("review")
        XCTAssertFalse(result.autoApproved)
        XCTAssertNil(result.checkoutUrl)
        result.applyStatus("live")
        XCTAssertTrue(result.isLive)
        XCTAssertTrue(result.checkoutUrl?.contains("sku=MAKE-X") == true)
        result.applyStatus("retired")
        XCTAssertNil(result.checkoutUrl)
        XCTAssertFalse(result.isLive)
    }

    func testRemixUsesCapabilitiesAndRouteRatherThanApprovalAlone() throws {
        let supported = MakeKindOption(kind: "tote", labelJa: "トート", labelEn: "Tote", retailJpy: 3800,
                                       category: "carry", canRemix: true)
        var result = try makeResult()
        XCTAssertFalse(result.supportsRemix(kinds: MakeKindOption.fallback))
        XCTAssertTrue(result.supportsRemix(kinds: [supported]))
        result.canRemix = false
        XCTAssertFalse(result.supportsRemix(kinds: [supported]))
        let incompatible = try makeResult(extra: ",\"fulfillment_route\":\"printful_aop\"")
        XCTAssertFalse(incompatible.supportsRemix(kinds: [supported]))
        result.canRemix = true
        result.applyStatus("review")
        XCTAssertFalse(result.supportsRemix(kinds: [supported]))
    }

    func testSavedPriceUpdatesRoyaltyAndNeverTrustsSubmittedPrice() throws {
        var result = try makeResult()
        let response = try decode(MakePriceResponse.self, """
        {"ok":true,"price_jpy":99000,"maker_earn_jpy":19800}
        """)
        result.applyPrice(try XCTUnwrap(response.saved))
        XCTAssertEqual(result.retailJpy, 99000)
        XCTAssertEqual(result.makerEarnJpy, 19800)
        result.applyPrice(SavedMakePrice(priceJpy: 3800, makerEarnJpy: nil))
        XCTAssertEqual(result.makerEarnJpy, 760)
        XCTAssertNil(try decode(MakePriceResponse.self, "{\"ok\":true}").saved)
    }

    func testEditPriceUsesCanonicalGETWhenLegacyResponseOmitsPrice() async throws {
        let session = priceSession()
        defer { session.invalidateAndCancel(); PriceURLProtocol.handler = nil }
        PriceURLProtocol.handler = { request in
            if request.httpMethod == "POST" {
                XCTAssertEqual(request.url?.path, "/api/make/edit/MAKE-X")
                return (200, "{\"ok\":true,\"updated\":[\"price\"]}")
            }
            XCTAssertEqual(request.httpMethod, "GET")
            XCTAssertEqual(request.url?.path, "/api/make/item/MAKE-X")
            XCTAssertEqual(URLComponents(url: request.url!, resolvingAgainstBaseURL: false)?.queryItems?.first?.value, "test")
            return (200, "{\"ok\":true,\"price_jpy\":3800}")
        }
        let saved = try await MUAPI.editPrice(sku: "MAKE-X", editToken: "test", priceJpy: 1, session: session)
        XCTAssertEqual(saved.priceJpy, 3800)
    }

    func testEditPriceReturnsServerClampWithoutAdditionalGET() async throws {
        let session = priceSession()
        defer { session.invalidateAndCancel(); PriceURLProtocol.handler = nil }
        PriceURLProtocol.handler = { request in
            XCTAssertEqual(request.httpMethod, "POST")
            return (200, "{\"ok\":true,\"price_jpy\":99000,\"maker_earn_jpy\":19800}")
        }
        let saved = try await MUAPI.editPrice(sku: "MAKE-X", editToken: "test", priceJpy: 999999, session: session)
        XCTAssertEqual(saved.priceJpy, 99000)
        XCTAssertEqual(saved.makerEarnJpy, 19800)
    }

    func testFailedCanonicalPriceReadDoesNotReturnInputAsSuccess() async throws {
        let session = priceSession()
        defer { session.invalidateAndCancel(); PriceURLProtocol.handler = nil }
        PriceURLProtocol.handler = { request in
            request.httpMethod == "POST" ? (200, "{\"ok\":true}") : (503, "{}")
        }
        do {
            _ = try await MUAPI.editPrice(sku: "MAKE-X", editToken: "test", priceJpy: 1, session: session)
            XCTFail("Unconfirmed saved price must not succeed")
        } catch {
            XCTAssertTrue(error is APIError)
        }
    }

    private func priceSession() -> URLSession {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [PriceURLProtocol.self]
        return URLSession(configuration: config)
    }

    func testAuditedJPCatalogThroughAPIHas76AvailableAnd9Blocked() async throws {
        let session = priceSession()
        defer { session.invalidateAndCancel(); PriceURLProtocol.handler = nil }
        let fixture = try auditedCatalogFixture()
        PriceURLProtocol.handler = { request in
            XCTAssertEqual(request.httpMethod, "GET")
            XCTAssertEqual(request.url?.path, "/api/make/kinds")
            XCTAssertEqual(request.cachePolicy, .reloadIgnoringLocalCacheData)
            return (200, fixture)
        }
        let kinds = try await MUAPI.makeKinds(session: session)
        let physical = kinds.filter { !$0.kind.isEmpty }
        XCTAssertEqual(physical.count, 85)
        XCTAssertEqual(Set(physical.map(\.kind)).count, 85)
        XCTAssertEqual(physical.filter(\.isAvailable).count, 76)
        let blocked = physical.filter { !$0.isAvailable }
        XCTAssertEqual(Set(blocked.map(\.kind)), ["tank", "rashguard_contrado", "pet_bowl", "pet_collar", "dog_tee",
                                                  "hardcover_photo_book", "softcover_photo_book", "christmas_stocking", "notepad"])
        XCTAssertTrue(Set(MakeKindOption.fallback.map(\.kind)).isDisjoint(with: Set(blocked.map(\.kind))))
        for option in physical {
            XCTAssertEqual(option.country, "JP")
            XCTAssertEqual(option.requiresVendorPreflight, true)
        }
        for option in blocked {
            XCTAssertEqual(option.unavailableMessage(locale: Locale(identifier: "ja_JP")), option.unavailableReasonJa)
            XCTAssertEqual(option.unavailableMessage(locale: Locale(identifier: "en_US")), option.unavailableReasonEn)
        }
        let collar = try XCTUnwrap(kinds.first { $0.kind == "pet_collar" })
        XCTAssertEqual(collar.temporary, true)
        XCTAssertEqual(collar.stockCheckedAt, "2026-09-13T15:13:47Z")
        let tank = try XCTUnwrap(kinds.first { $0.kind == "tank" })
        XCTAssertEqual(tank.allowedCountries, ["AU", "NZ"])
        XCTAssertEqual(tank.temporary, false)
        XCTAssertEqual(kinds.first { $0.kind == "pet_bowl" }?.excludedCountries, ["KR", "HK", "TW", "JP", "SG"])
        let tote = try XCTUnwrap(kinds.first { $0.kind == "tote" })
        XCTAssertEqual(tote.label(locale: Locale(identifier: "ja")), "トートバッグ（黒・42×42cm）")
        XCTAssertEqual(tote.label(locale: Locale(identifier: "en")), "Tote bag (black, 42 x 42 cm)")
        XCTAssertEqual(tote.estimatedPrice(royalty: 10), 3800)

        var catalog = MakeKindCatalog()
        catalog.received(kinds)
        catalog.failed()
        XCTAssertEqual(catalog.items, kinds, "Refresh failures must retain live reasons, prices and denials")
        XCTAssertTrue(catalog.refreshFailed)
        XCTAssertFalse(MakeKindOption.isAvailable("tank", in: catalog.items))
    }

    func testUpdatedPriceLabelAndRestockReplaceSnapshotRatherThanHardcodedValues() async throws {
        let session = priceSession()
        defer { session.invalidateAndCancel(); PriceURLProtocol.handler = nil }
        var payload = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(auditedCatalogFixture().utf8)) as? [String: Any])
        var rows = try XCTUnwrap(payload["items"] as? [[String: Any]])
        let tote = try XCTUnwrap(rows.firstIndex { $0["kind"] as? String == "tote" })
        rows[tote]["retail_jpy"] = 4300
        rows[tote]["label_en"] = "Updated vendor tote"
        let collar = try XCTUnwrap(rows.firstIndex { $0["kind"] as? String == "pet_collar" })
        rows[collar]["can_make"] = true
        rows[collar]["temporary"] = false
        rows[collar]["unavailable_reason_ja"] = NSNull()
        rows[collar]["unavailable_reason_en"] = NSNull()
        payload["items"] = rows
        let body = String(decoding: try JSONSerialization.data(withJSONObject: payload), as: UTF8.self)
        PriceURLProtocol.handler = { _ in (200, body) }
        var catalog = MakeKindCatalog()
        catalog.failed()
        catalog.received(try await MUAPI.makeKinds(session: session))
        XCTAssertFalse(catalog.refreshFailed)
        let changed = try XCTUnwrap(catalog.items.first { $0.kind == "tote" })
        XCTAssertEqual(changed.estimatedPrice(royalty: 10), 4300)
        XCTAssertEqual(changed.label(locale: Locale(identifier: "en")), "Updated vendor tote")
        let restocked = try XCTUnwrap(catalog.items.first { $0.kind == "pet_collar" })
        XCTAssertTrue(restocked.isAvailable)
        XCTAssertNil(restocked.unavailableMessage())
    }

    func testUnavailableReasonFallsBackAcrossLanguagesAndLegacyPayload() throws {
        var option = try decode(MakeKindOption.self, """
        {"kind":"tank","label_ja":"タンクトップ","label_en":"Tank top","category":"wear",
         "retail_jpy":4200,"can_make":false,"can_remix":false,"unavailable_reason_ja":"  ",
         "unavailable_reason_en":"Ships only to Australia and New Zealand."}
        """)
        XCTAssertEqual(option.unavailableMessage(locale: Locale(identifier: "ja")), "Ships only to Australia and New Zealand.")
        option.unavailableReasonEn = nil
        XCTAssertNotNil(option.unavailableMessage())
        XCTAssertNil(option.temporary)
        XCTAssertNil(option.country)
    }

    // Exact real-handler response, also compared in Rust's make_kinds_tests.
    private func auditedCatalogFixture() throws -> String {
        #if SWIFT_PACKAGE
        let bundle = Bundle.module
        #else
        let bundle = Bundle(for: ModelsTests.self)
        #endif
        let url = try XCTUnwrap(bundle.url(forResource: "make-kinds", withExtension: "json", subdirectory: "Fixtures"))
        return try String(contentsOf: url, encoding: .utf8)
    }
}

// Every request is intercepted; the contract suite cannot place orders or call paid APIs.
private final class PriceURLProtocol: URLProtocol {
    static var handler: ((URLRequest) -> (Int, String))?
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        guard let handler = Self.handler else {
            client?.urlProtocol(self, didFailWithError: URLError(.badServerResponse)); return
        }
        let (status, body) = handler(request)
        let response = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil, headerFields: nil)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(body.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
