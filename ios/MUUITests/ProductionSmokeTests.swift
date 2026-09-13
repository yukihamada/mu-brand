import XCTest

/// Uses the real MU app and its production API. Never submits generation or checkout.
final class ProductionSmokeTests: XCTestCase {
    private var app: XCUIApplication!

    override func setUpWithError() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        // NSArgumentDomain overrides AppStorage for this launch only. Tapping the
        // onboarding Skip button would persist hasOnboarded and a seed variant.
        app.launchArguments += ["-hasOnboarded", "YES", "-AppleLanguages", "(ja)", "-AppleLocale", "ja_JP"]
        app.launch()
        XCTAssertTrue(app.tabBars.firstMatch.waitForExistence(timeout: 30), "MU tab bar did not appear")
    }

    override func tearDownWithError() throws {
        if let app = app {
            capture("final-screen")
            if let run = testRun, run.failureCount > 0 {
                let hierarchy = XCTAttachment(string: app.debugDescription)
                hierarchy.name = "failed-accessibility-hierarchy"
                hierarchy.lifetime = .keepAlways
                add(hierarchy)
            }
            app.terminate()
        }
        app = nil
    }

    func testMakeKindPickerSelectsMug() throws {
        app.tabBars.buttons["作る"].tap()
        let picker = app.buttons["make.kindPicker"]
        XCTAssertTrue(picker.waitForExistence(timeout: 30))
        let enabled = XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: picker)
        XCTAssertEqual(XCTWaiter.wait(for: [enabled], timeout: 75), .completed, "Production catalog/fallback never became ready")
        reveal(picker)

        let usesFallback = app.staticTexts["make.kindsUnavailable"].exists
        if usesFallback {
            XCTAssertEqual(app.staticTexts["make.kindsUnavailable"].label,
                           "種類一覧を取得できませんでした。定番の商品を表示しています。")
        }
        capture(usesFallback ? "make-production-fallback" : "make-production-catalog")
        picker.tap()
        XCTAssertTrue(app.navigationBars["作れるもの"].waitForExistence(timeout: 10))

        // /api/make/kinds currently returns 404. Check the fallback when shown;
        // do not mistake it for the full backend catalog.
        if usesFallback {
            XCTAssertTrue(app.buttons["make.kind.option.auto"].exists)
        }
        // Search by the backend kind, avoiding offscreen/lazily-created rows.
        let search = app.searchFields.firstMatch
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        search.tap()
        search.typeText("mug")
        let mug = app.buttons["make.kind.option.mug"]
        XCTAssertTrue(mug.waitForExistence(timeout: 10), "Production catalog/fallback has no mug")
        XCTAssertTrue(mug.isEnabled)
        capture("make-kind-picker")
        mug.tap()

        let dismissed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"),
                                                object: app.navigationBars["作れるもの"])
        XCTAssertEqual(XCTWaiter.wait(for: [dismissed], timeout: 10), .completed)
        XCTAssertTrue(picker.waitForExistence(timeout: 10), "Kind picker sheet did not dismiss")
        XCTAssertEqual(picker.value as? String, "mug")
        XCTAssertTrue(picker.label.contains("マグ"))
        capture("make-mug-selected-no-generation")
    }

    func testShopProductDetailHasPurchaseURLWithoutCheckout() throws {
        app.tabBars.buttons["ショップ"].tap()
        let product = app.descendants(matching: .any)
            .matching(NSPredicate(format: "identifier BEGINSWITH %@", "shop.product.")).firstMatch
        XCTAssertTrue(product.waitForExistence(timeout: 45), "Production shop feed did not expose a product")
        let sku = String(product.identifier.dropFirst("shop.product.".count))
        XCTAssertFalse(sku.isEmpty)
        capture("shop-production-products")
        product.tap()
        XCTAssertTrue(app.navigationBars[sku].waitForExistence(timeout: 10), "Opened a different product")

        let buy = app.buttons["pdp.buy"]
        XCTAssertTrue(buy.waitForExistence(timeout: 15))
        reveal(buy)
        XCTAssertTrue(buy.isEnabled)
        XCTAssertEqual(buy.label, "購入する")
        let rawURL = try XCTUnwrap(buy.value as? String, "Purchase URL is not exposed")
        let url = try XCTUnwrap(URLComponents(string: rawURL))
        XCTAssertEqual(url.scheme, "https")
        XCTAssertEqual(url.host, "wearmu.com")
        XCTAssertEqual(url.path, "/api/shop/checkout")
        XCTAssertEqual(url.queryItems?.first(where: { $0.name == "sku" })?.value, sku)
        capture("product-before-purchase-no-stripe-session")
        // Deliberately no buy.tap(): even GET checkout creates a real Stripe session.
    }

    private func reveal(_ element: XCUIElement, file: StaticString = #filePath, line: UInt = #line) {
        for _ in 0..<5 {
            if element.isHittable { return }
            app.scrollViews.firstMatch.swipeUp()
        }
        XCTAssertTrue(element.isHittable, "Element is not reachable: \(element.identifier)", file: file, line: line)
    }

    private func capture(_ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
