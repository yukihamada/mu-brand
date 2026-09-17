import XCTest

/// Offline, fixture-backed regression tests. `MakeUITestFixture` intercepts every
/// request under DEBUG + `MU_UI_FIXTURE=make`, so nothing reaches production.
final class MakeRegressionTests: XCTestCase {
    private var app: XCUIApplication!

    override func setUpWithError() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchEnvironment["MU_UI_FIXTURE"] = "make"
        app.launchArguments = ["-hasOnboarded", "YES", "-aiConsentGiven", "YES",
                               "-didPromptPushAfterMake", "YES", "-mu.makeSuccessCount", "100",
                               "-AppleLanguages", "(en)", "-AppleLocale", "en_US"]
    }

    override func tearDownWithError() throws {
        let image = XCTAttachment(screenshot: app.screenshot())
        image.name = name
        image.lifetime = .keepAlways
        add(image)
        app.terminate()
    }

    func testColdPromptCreatesExactlyOneDesignAndExplicitVariationCreatesNext() {
        app.launchEnvironment["MU_UI_PROMPT"] = "A minimal dojo shirt"
        app.launch()
        let first = app.staticTexts["make.result.FIXTURE-1"]
        XCTAssertTrue(first.waitForExistence(timeout: 20))
        reveal(first)
        // Allow a hidden automatic second POST enough time to finish.
        let unexpected = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == true"),
                                                  object: app.staticTexts["make.result.FIXTURE-2"])
        XCTAssertEqual(XCTWaiter.wait(for: [unexpected], timeout: 4), .timedOut)
        let another = app.buttons["make.another"]
        reveal(another)
        another.tap()
        let second = app.staticTexts["make.result.FIXTURE-2"]
        XCTAssertTrue(second.waitForExistence(timeout: 15))
    }

    func testCreationCompletesAfterSwitchingTabs() {
        app.launch()
        let prompt = app.textFields["make.prompt"]
        XCTAssertTrue(prompt.waitForExistence(timeout: 15))
        prompt.tap()
        prompt.typeText("A minimal dojo shirt")
        let create = app.buttons["make.create"]
        create.tap()
        app.tabBars.buttons["Shop"].tap()
        XCTAssertTrue(app.navigationBars["Shop"].waitForExistence(timeout: 5))
        app.tabBars.buttons["Make"].tap()
        let first = app.staticTexts["make.result.FIXTURE-1"]
        XCTAssertTrue(first.waitForExistence(timeout: 15))
        reveal(first)
    }

    // MARK: - Request-count assertions

    /// A creation that is never shown still costs a creation. Count POST /api/make
    /// instead of watching for a second result, so an unselected or hidden automatic
    /// variation is detected too.
    func testOneColdPromptIssuesExactlyOneCreationRequest() {
        app.launchEnvironment["MU_UI_PROMPT"] = "A minimal dojo shirt"
        app.launch()
        XCTAssertTrue(app.staticTexts["make.result.FIXTURE-1"].waitForExistence(timeout: 20))
        XCTAssertEqual(counter("make"), 1, "Cold prompt must issue exactly one creation: \(counterValue)")
        // The probe refreshes on a timer, so wait for the delivery counter instead
        // of reading it once right after the result appears.
        XCTAssertTrue(waitForCounter("delivered", atLeast: 1, timeout: 10),
                      "Creation response not delivered: \(counterValue)")
        // A hidden automatic second creation would raise this to 2.
        Thread.sleep(forTimeInterval: 6)
        XCTAssertEqual(counter("make"), 1, "Automatic second creation detected: \(counterValue)")
    }

    /// Tapping "Make another idea" is the only path that may spend a second creation.
    func testExplicitAnotherIdeaIssuesSecondCreationRequest() {
        app.launch()
        let prompt = app.textFields["make.prompt"]
        XCTAssertTrue(prompt.waitForExistence(timeout: 15))
        prompt.tap()
        prompt.typeText("A minimal dojo shirt")
        app.buttons["make.create"].tap()
        XCTAssertTrue(app.staticTexts["make.result.FIXTURE-1"].waitForExistence(timeout: 20))
        XCTAssertTrue(waitForCounter("make", atLeast: 1, timeout: 10), "Unexpected creation count: \(counterValue)")
        XCTAssertEqual(counter("make"), 1, "Unexpected creation count: \(counterValue)")
        let another = app.buttons["make.another"]
        reveal(another)
        another.tap()
        XCTAssertTrue(app.staticTexts["make.result.FIXTURE-2"].waitForExistence(timeout: 20))
        XCTAssertTrue(waitForCounter("make", atLeast: 2, timeout: 10), "Second creation was not requested: \(counterValue)")
        XCTAssertEqual(counter("make"), 2, "Second creation was not requested: \(counterValue)")
    }

    /// The response must be delivered while Make is offscreen, and the preview must
    /// still be fetched after returning. Returning too early would only prove that
    /// the request survived, not that a completion arriving offscreen is handled.
    func testPreviewFetchedAfterReturningWhenResponseArrivedOffscreen() {
        app.launch()
        let prompt = app.textFields["make.prompt"]
        XCTAssertTrue(prompt.waitForExistence(timeout: 15))
        prompt.tap()
        prompt.typeText("A minimal dojo shirt")
        app.buttons["make.create"].tap()

        app.tabBars.buttons["Shop"].tap()
        XCTAssertTrue(app.navigationBars["Shop"].waitForExistence(timeout: 5))

        // Stay away until the fixture has actually delivered the creation response.
        XCTAssertTrue(waitForCounter("delivered", atLeast: 1, timeout: 15),
                      "Creation response was never delivered offscreen: \(counterValue)")
        XCTAssertEqual(counter("make"), 1, "Unexpected creation count: \(counterValue)")

        app.tabBars.buttons["Make"].tap()
        XCTAssertTrue(app.staticTexts["make.result.FIXTURE-1"].waitForExistence(timeout: 15),
                      "Result created offscreen was lost on return: \(counterValue)")
        // applyPeek marks a preview complete only when ready==true, the kind is a
        // product preview and a mockup URL is present.
        let ready = app.staticTexts["make.previewReady"]
        XCTAssertTrue(ready.waitForExistence(timeout: 20),
                      "Preview was not fetched after returning to Make: \(counterValue)")
        reveal(ready)
        XCTAssertGreaterThan(counter("peek"), 0, "No preview request was issued after returning")
    }

    // MARK: - Helpers

    private var counterValue: String {
        app.staticTexts["fixture.counters"].value as? String ?? "<missing>"
    }

    /// Reads one counter by name, so assertions never depend on the whole label.
    private func counter(_ name: String) -> Int {
        guard let value = app.staticTexts["fixture.counters"].value as? String else { return -1 }
        for part in value.split(separator: " ") where part.hasPrefix("\(name)=") {
            return Int(part.dropFirst(name.count + 1)) ?? -1
        }
        return -1
    }

    /// Polls one counter directly; the probe sits above the tab bar so it is
    /// readable while Make is offscreen.
    private func waitForCounter(_ name: String, atLeast minimum: Int, timeout: TimeInterval) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if counter(name) >= minimum { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.25))
        }
        return false
    }

    private func reveal(_ element: XCUIElement) {
        for _ in 0..<8 {
            if element.isHittable { return }
            app.scrollViews.firstMatch.swipeUp()
        }
        XCTAssertTrue(element.isHittable)
    }
}
