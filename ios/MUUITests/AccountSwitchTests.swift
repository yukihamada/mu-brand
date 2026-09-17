import XCTest

/// Offline tests for account isolation in Make and Agent. A design belongs to the
/// account that created it: signing out or switching accounts must clear the old
/// account's results and stop its in-flight work.
final class AccountSwitchTests: XCTestCase {
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

    /// The previous account's design must not stay on screen after signing out.
    func testSignOutClearsPreviousAccountDesign() {
        app.launch()
        create("A minimal dojo shirt")
        XCTAssertTrue(app.staticTexts["make.result.FIXTURE-1"].waitForExistence(timeout: 20))

        app.buttons["fixture.signOut"].tap()
        XCTAssertTrue(waitForUser(nil, timeout: 10), "Sign-out was not applied: \(userValue)")

        let stale = app.staticTexts["make.result.FIXTURE-1"]
        let gone = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: stale)
        XCTAssertEqual(XCTWaiter.wait(for: [gone], timeout: 10), .completed,
                       "The previous account's design survived sign-out")
    }

    /// Switching to another account must not leave the first account's design behind.
    func testSwitchingAccountClearsPreviousDesign() {
        app.launch()
        create("A minimal dojo shirt")
        XCTAssertTrue(app.staticTexts["make.result.FIXTURE-1"].waitForExistence(timeout: 20))

        app.buttons["fixture.switchAccount"].tap()
        XCTAssertTrue(waitForUser("second@example.invalid", timeout: 10), "Switch not applied: \(userValue)")

        let stale = app.staticTexts["make.result.FIXTURE-1"]
        let gone = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: stale)
        XCTAssertEqual(XCTWaiter.wait(for: [gone], timeout: 10), .completed,
                       "The first account's design survived an account switch")
    }

    /// A creation response that arrives after the account changed must not be shown.
    /// The request starts under one account and completes under another.
    func testLateCreationResponseDoesNotAppearAfterAccountSwitch() {
        app.launch()
        let prompt = app.textFields["make.prompt"]
        XCTAssertTrue(prompt.waitForExistence(timeout: 15))
        prompt.tap()
        prompt.typeText("A minimal dojo shirt")
        app.buttons["make.create"].tap()
        // Switch while the fixture is still delaying the response.
        app.buttons["fixture.switchAccount"].tap()
        XCTAssertTrue(waitForUser("second@example.invalid", timeout: 10), "Switch not applied: \(userValue)")

        // Give the delayed response time to arrive and be rejected.
        Thread.sleep(forTimeInterval: 5)
        XCTAssertFalse(app.staticTexts["make.result.FIXTURE-1"].exists,
                       "A response from the previous account was applied after switching")
    }

    /// The real first-login path: start signed out, cold prompt opens the AuthGate
    /// sheet, authenticating dismisses it, and the pending creation then runs once.
    func testFirstLoginThroughAuthGateResumesPendingCreation() {
        app.launchEnvironment["MU_UI_FIXTURE_SIGNED_OUT"] = "1"
        app.launchEnvironment["MU_UI_PROMPT"] = "A minimal dojo shirt"
        app.launch()

        // Cold prompt cannot create while signed out, so it must ask to register.
        XCTAssertTrue(app.buttons["fixture.authSignIn"].waitForExistence(timeout: 20),
                      "AuthGate sheet was not shown for a signed-out cold prompt")
        XCTAssertEqual(counter("make"), 0, "A signed-out cold prompt must not create: \(counterValue)")
        XCTAssertEqual(userValue, "user=<none>", "Test must start signed out: \(userValue)")

        // nil -> email: the first login. No Keychain write, no OTP, no network.
        app.buttons["fixture.authSignIn"].tap()
        XCTAssertTrue(waitForUser("first@example.invalid", timeout: 10), "Login not applied: \(userValue)")

        // The sheet dismisses and the pending intent resumes exactly once.
        let dismissed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"),
                                                 object: app.buttons["fixture.authSignIn"])
        XCTAssertEqual(XCTWaiter.wait(for: [dismissed], timeout: 10), .completed, "AuthGate did not dismiss")
        XCTAssertTrue(app.staticTexts["make.result.FIXTURE-1"].waitForExistence(timeout: 20),
                      "Pending creation did not resume after login: \(counterValue)")
        XCTAssertTrue(waitForCounter("make", atLeast: 1, timeout: 10),
                      "Pending creation did not resume after login: \(counterValue)")
        XCTAssertEqual(counter("make"), 1, "Pending intent must resume exactly once: \(counterValue)")
    }

    /// Regression: the first login (nil -> email) also rotates the generation, so
    /// clearing only on "previous identity was non-nil" left `sending` stuck and the
    /// input bar permanently disabled. Logging in mid-flight must release the input
    /// and the old reply must still be discarded.
    func testAgentFirstLoginMidFlightReleasesInputAndDiscardsOldAction() {
        app.launchEnvironment["MU_UI_FIXTURE_SIGNED_OUT"] = "1"
        app.launch()
        app.tabBars.buttons["AI"].tap()
        let input = app.textFields["agent.input"]
        XCTAssertTrue(input.waitForExistence(timeout: 15))
        input.tap()
        input.typeText("Make me a minimal dojo shirt")
        app.buttons["agent.send"].tap()
        XCTAssertTrue(waitForCounter("chat", atLeast: 1, timeout: 10), "Chat was not requested: \(counterValue)")
        XCTAssertEqual(userValue, "user=<none>", "Test must start signed out: \(userValue)")

        // First login while the chat reply is still delayed.
        app.buttons["fixture.loginFirst"].tap()
        XCTAssertTrue(waitForUser("first@example.invalid", timeout: 10), "Login not applied: \(userValue)")

        // The input bar must become usable again, not stay stuck on `sending`.
        // `resetForNewIdentity` clears the text, so type again before checking.
        let field = app.textFields["agent.input"]
        let usable = XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: field)
        XCTAssertEqual(XCTWaiter.wait(for: [usable], timeout: 15), .completed,
                       "Input stayed disabled after the first login: sending was not released")
        field.tap()
        field.typeText("Another idea")
        let sendable = app.buttons["agent.send"]
        let enabled = XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: sendable)
        XCTAssertEqual(XCTWaiter.wait(for: [enabled], timeout: 10), .completed,
                       "Send stayed disabled after the first login")

        // The old reply must still be rejected: no creation from the previous identity.
        Thread.sleep(forTimeInterval: 5)
        XCTAssertEqual(counter("make"), 0,
                       "The pre-login chat action created a design: \(counterValue)")
    }

    /// A chat reply that arrives after the account changed must not run its action.
    /// The fixture delays /api/app/agent/chat and answers action=make, so a leaked
    /// reply would issue a creation under the old account.
    func testAgentReplyAfterAccountSwitchDoesNotCreate() {
        app.launch()
        app.tabBars.buttons["AI"].tap()
        let input = app.textFields["agent.input"]
        XCTAssertTrue(input.waitForExistence(timeout: 15))
        input.tap()
        input.typeText("Make me a minimal dojo shirt")
        app.buttons["agent.send"].tap()
        XCTAssertTrue(waitForCounter("chat", atLeast: 1, timeout: 10), "Chat was not requested: \(counterValue)")

        // Switch while the fixture is still delaying the chat reply.
        app.buttons["fixture.switchAccount"].tap()
        XCTAssertTrue(waitForUser("second@example.invalid", timeout: 10), "Switch not applied: \(userValue)")

        // Give the delayed reply time to arrive and be rejected.
        Thread.sleep(forTimeInterval: 5)
        XCTAssertEqual(counter("make"), 0,
                       "The old account's chat action created a design after switching: \(counterValue)")
    }

    // MARK: - Helpers

    private func create(_ text: String) {
        let prompt = app.textFields["make.prompt"]
        XCTAssertTrue(prompt.waitForExistence(timeout: 15))
        prompt.tap()
        prompt.typeText(text)
        app.buttons["make.create"].tap()
    }

    private var userValue: String {
        app.staticTexts["fixture.account"].value as? String ?? "<missing>"
    }

    private func waitForUser(_ email: String?, timeout: TimeInterval) -> Bool {
        let expected = email.map { "user=\($0)" } ?? "user=<none>"
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if userValue == expected { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.25))
        }
        return false
    }

    private var counterValue: String {
        app.staticTexts["fixture.counters"].value as? String ?? "<missing>"
    }

    private func waitForCounter(_ name: String, atLeast minimum: Int, timeout: TimeInterval) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if counter(name) >= minimum { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.25))
        }
        return false
    }

    private func counter(_ name: String) -> Int {
        guard let value = app.staticTexts["fixture.counters"].value as? String else { return -1 }
        for part in value.split(separator: " ") where part.hasPrefix("\(name)=") {
            return Int(part.dropFirst(name.count + 1)) ?? -1
        }
        return -1
    }
}
